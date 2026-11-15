use std::env;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::fs::{self, File};
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

use crate::rpc::{Direction, RpcCall};

const FALLBACK_LOG_FILE: &str = "mcp-tap.jsonl";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceEvent {
    pub ts_ms: u64,
    #[serde(flatten)]
    pub kind: EventKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventKind {
    SessionStart {
        command: Vec<String>,
    },
    RpcCall(RpcCall),
    Notification {
        direction: Direction,
        method: String,
    },
    TokenWarning {
        method: String,
        target: Option<String>,
        response_tokens: usize,
        limit: usize,
    },
    RawStdout {
        line: String,
    },
    RawStdin {
        line: String,
    },
    Stderr {
        line: String,
    },
    SessionEnd {
        exit_code: u8,
    },
}

// Cheap to clone into every forwarding task; sending never blocks the proxied streams
#[derive(Clone)]
pub struct Tracer {
    sender: UnboundedSender<TraceEvent>,
}

impl Tracer {
    pub fn emit(&self, kind: EventKind) {
        let event = TraceEvent {
            ts_ms: unix_millis(),
            kind,
        };
        // A closed channel means the writer already failed and reported it
        let _ = self.sender.send(event);
    }
}

pub fn default_log_path() -> PathBuf {
    env::var_os("HOME").map_or_else(
        || PathBuf::from(FALLBACK_LOG_FILE),
        |home| PathBuf::from(home).join(".cache/mcp-tap/latest.jsonl"),
    )
}

pub async fn spawn_writer(path: &Path) -> io::Result<(Tracer, JoinHandle<()>)> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).await?;
    }
    let file = File::create(path).await.map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("cannot create trace file {}: {error}", path.display()),
        )
    })?;
    let (sender, receiver) = mpsc::unbounded_channel();
    let writer = tokio::spawn(async move {
        if let Err(error) = write_events(BufWriter::new(file), receiver).await {
            eprintln!("mcp-tap: trace writer stopped: {error}");
        }
    });
    Ok((Tracer { sender }, writer))
}

async fn write_events(
    mut file: BufWriter<File>,
    mut receiver: UnboundedReceiver<TraceEvent>,
) -> io::Result<()> {
    while let Some(event) = receiver.recv().await {
        let mut line = serde_json::to_vec(&event)?;
        line.push(b'\n');
        file.write_all(&line).await?;
        // Flushing per event keeps `mcp-tap tail` live
        file.flush().await?;
    }
    Ok(())
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_rpc_call_flat_with_type_tag() {
        let event = TraceEvent {
            ts_ms: 1,
            kind: EventKind::RpcCall(RpcCall {
                direction: Direction::ClientToServer,
                method: "tools/call".to_owned(),
                target: Some("search".to_owned()),
                latency_ms: 12.5,
                request_tokens: 3,
                response_tokens: 40,
                is_error: false,
            }),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "rpc_call");
        assert_eq!(json["direction"], "client_to_server");
        assert_eq!(json["target"], "search");
        let round_trip: TraceEvent = serde_json::from_value(json).unwrap();
        assert_eq!(round_trip, event);
    }

    #[tokio::test]
    async fn writes_one_json_line_per_event() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/trace.jsonl");
        let (tracer, writer) = spawn_writer(&path).await.unwrap();
        tracer.emit(EventKind::Stderr {
            line: "boot".to_owned(),
        });
        tracer.emit(EventKind::SessionEnd { exit_code: 3 });
        drop(tracer);
        writer.await.unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let events: Vec<TraceEvent> = content
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].kind, EventKind::SessionEnd { exit_code: 3 });
    }
}
