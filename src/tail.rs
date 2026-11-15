use std::fs::File;
use std::io::{self, ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::thread;
use std::time::Duration;

use colored::{ColoredString, Colorize};

use crate::rpc::{Direction, RpcCall};
use crate::trace::{EventKind, TraceEvent};

const POLL_INTERVAL: Duration = Duration::from_millis(250);
const MEDIUM_LATENCY_MS: f64 = 100.0;
const SLOW_LATENCY_MS: f64 = 1000.0;
const MILLIS_PER_SECOND: f64 = 1000.0;

pub fn run(path: &Path) -> io::Result<()> {
    eprintln!("mcp-tap: following {} (Ctrl+C to stop)", path.display());
    let mut out = io::stdout().lock();
    let mut follower = Follower::default();
    let mut session_start_ms = None;
    loop {
        for line in follower.poll(path)? {
            let Ok(event) = serde_json::from_slice::<TraceEvent>(&line) else {
                continue;
            };
            if matches!(event.kind, EventKind::SessionStart { .. }) || session_start_ms.is_none() {
                session_start_ms = Some(event.ts_ms);
            }
            let elapsed_ms = event
                .ts_ms
                .saturating_sub(session_start_ms.unwrap_or(event.ts_ms));
            writeln!(out, "{}", format_event(&event.kind, elapsed_ms))?;
        }
        thread::sleep(POLL_INTERVAL);
    }
}

#[derive(Default)]
struct Follower {
    offset: u64,
    partial_line: Vec<u8>,
}

impl Follower {
    fn poll(&mut self, path: &Path) -> io::Result<Vec<Vec<u8>>> {
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        // A shorter file means it was truncated or replaced by a new session
        if file.metadata()?.len() < self.offset {
            self.offset = 0;
            self.partial_line.clear();
        }
        file.seek(SeekFrom::Start(self.offset))?;
        let read = file.read_to_end(&mut self.partial_line)?;
        self.offset += read as u64;
        let Some(last_newline) = self.partial_line.iter().rposition(|&byte| byte == b'\n') else {
            return Ok(Vec::new());
        };
        let complete: Vec<u8> = self.partial_line.drain(..=last_newline).collect();
        Ok(complete
            .split(|&byte| byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(<[u8]>::to_vec)
            .collect())
    }
}

fn format_event(kind: &EventKind, elapsed_ms: u64) -> String {
    let elapsed = format!("+{:.3}s", elapsed_ms as f64 / MILLIS_PER_SECOND);
    let timestamp = format!("{elapsed:>10}").dimmed();
    let body = match kind {
        EventKind::RpcCall(call) => format_call(call),
        EventKind::Notification { method, .. } => format!("  notify {method}").cyan().to_string(),
        EventKind::TokenWarning {
            method,
            target,
            response_tokens,
            limit,
        } => {
            let target = target.as_deref().unwrap_or_default();
            format!("! {method} {target} returned {response_tokens} tokens (limit {limit})")
                .yellow()
                .to_string()
        }
        EventKind::RawStdout { line } => format!("! non-JSON stdout: {line}").red().to_string(),
        EventKind::RawStdin { line } => format!("! non-JSON stdin: {line}").red().to_string(),
        EventKind::Stderr { line } => format!("  stderr: {line}").dimmed().to_string(),
        EventKind::SessionStart { command } => format!("session started: {}", command.join(" "))
            .bold()
            .to_string(),
        EventKind::SessionEnd { exit_code } => format!("session ended (exit code {exit_code})")
            .bold()
            .to_string(),
    };
    format!("{timestamp} {body}")
}

fn format_call(call: &RpcCall) -> String {
    let arrow = match call.direction {
        Direction::ClientToServer => "->",
        Direction::ServerToClient => "<-",
    };
    let name = format!(
        "{} {}",
        call.method,
        call.target.as_deref().unwrap_or_default()
    );
    let status = if call.is_error {
        " ERROR".red().bold()
    } else {
        "".normal()
    };
    format!(
        "{arrow} {:<40} {} {:>6} -> {:>6} tok{status}",
        name.bold(),
        color_latency(call.latency_ms),
        call.request_tokens,
        call.response_tokens
    )
}

fn color_latency(latency_ms: f64) -> ColoredString {
    let text = format!("{latency_ms:>9.1} ms");
    if latency_ms >= SLOW_LATENCY_MS {
        text.red()
    } else if latency_ms >= MEDIUM_LATENCY_MS {
        text.yellow()
    } else {
        text.green()
    }
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;

    use super::*;

    fn append(path: &Path, text: &str) {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        file.write_all(text.as_bytes()).unwrap();
    }

    #[test]
    fn follows_appends_partial_lines_and_truncation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trace.jsonl");
        let mut follower = Follower::default();
        assert!(follower.poll(&path).unwrap().is_empty());
        append(&path, "first\nsec");
        assert_eq!(follower.poll(&path).unwrap(), [b"first".to_vec()]);
        append(&path, "ond\n");
        assert_eq!(follower.poll(&path).unwrap(), [b"second".to_vec()]);
        std::fs::write(&path, "new\n").unwrap();
        assert_eq!(follower.poll(&path).unwrap(), [b"new".to_vec()]);
    }

    #[test]
    fn formats_completed_call() {
        colored::control::set_override(false);
        let call = RpcCall {
            direction: Direction::ClientToServer,
            method: "tools/call".to_owned(),
            target: Some("search".to_owned()),
            latency_ms: 12.5,
            request_tokens: 7,
            response_tokens: 420,
            is_error: true,
        };
        let line = format_event(&EventKind::RpcCall(call), 1500);
        assert!(
            line.starts_with("   +1.500s -> tools/call search"),
            "{line}"
        );
        assert!(line.contains("12.5 ms"));
        assert!(line.contains("7 ->    420 tok"));
        assert!(line.ends_with("ERROR"));
    }
}
