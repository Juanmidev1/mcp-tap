use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tokens::estimate_tokens;

const MICROS_PER_MILLI: f64 = 1000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    ClientToServer,
    ServerToClient,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcCall {
    pub direction: Direction,
    pub method: String,
    pub target: Option<String>,
    pub latency_ms: f64,
    pub request_tokens: usize,
    pub response_tokens: usize,
    pub is_error: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Observation {
    Call(RpcCall),
    Notification {
        direction: Direction,
        method: String,
    },
    Malformed {
        direction: Direction,
    },
}

struct PendingCall {
    started_at: Instant,
    method: String,
    target: Option<String>,
    request_tokens: usize,
}

// Keyed by the request's direction too, so a server->client request (e.g. sampling)
// cannot collide with a client request that reuses the same id
#[derive(Default)]
pub struct Correlator {
    pending: HashMap<(Direction, String), PendingCall>,
}

impl Correlator {
    pub fn observe(
        &mut self,
        direction: Direction,
        line: &[u8],
        now: Instant,
    ) -> Option<Observation> {
        let text = String::from_utf8_lossy(line);
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let Ok(message) = serde_json::from_str::<Value>(text) else {
            return Some(Observation::Malformed { direction });
        };
        let tokens = estimate_tokens(text);
        let method = message.get("method").and_then(Value::as_str);
        match (method, message.get("id")) {
            (Some(method), Some(id)) => {
                let pending = PendingCall {
                    started_at: now,
                    method: method.to_owned(),
                    target: extract_target(method, &message),
                    request_tokens: tokens,
                };
                self.pending.insert((direction, id.to_string()), pending);
                None
            }
            (Some(method), None) => Some(Observation::Notification {
                direction,
                method: method.to_owned(),
            }),
            (None, Some(_)) => self.finish_call(direction, &message, tokens, now),
            // Valid JSON that is not JSON-RPC (e.g. a stray JSON logger) still corrupts the transport
            (None, None) => Some(Observation::Malformed { direction }),
        }
    }

    fn finish_call(
        &mut self,
        response_direction: Direction,
        message: &Value,
        response_tokens: usize,
        now: Instant,
    ) -> Option<Observation> {
        let request_direction = opposite(response_direction);
        let id = message.get("id")?.to_string();
        let pending = self.pending.remove(&(request_direction, id))?;
        Some(Observation::Call(RpcCall {
            direction: request_direction,
            method: pending.method,
            target: pending.target,
            latency_ms: to_millis(now.duration_since(pending.started_at)),
            request_tokens: pending.request_tokens,
            response_tokens,
            is_error: is_error_response(message),
        }))
    }
}

// Microsecond precision is plenty and keeps trace lines readable
fn to_millis(elapsed: Duration) -> f64 {
    elapsed.as_micros() as f64 / MICROS_PER_MILLI
}

fn opposite(direction: Direction) -> Direction {
    match direction {
        Direction::ClientToServer => Direction::ServerToClient,
        Direction::ServerToClient => Direction::ClientToServer,
    }
}

fn extract_target(method: &str, message: &Value) -> Option<String> {
    let field = match method {
        "tools/call" | "prompts/get" => "name",
        "resources/read" | "resources/subscribe" | "resources/unsubscribe" => "uri",
        _ => return None,
    };
    message
        .pointer(&format!("/params/{field}"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn is_error_response(message: &Value) -> bool {
    let is_tool_error = message.pointer("/result/isError") == Some(&Value::Bool(true));
    message.get("error").is_some() || is_tool_error
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIENT: Direction = Direction::ClientToServer;
    const SERVER: Direction = Direction::ServerToClient;

    fn expect_call(observation: Option<Observation>) -> RpcCall {
        match observation {
            Some(Observation::Call(call)) => call,
            other => panic!("expected a completed call, got {other:?}"),
        }
    }

    #[test]
    fn correlates_tool_call_and_measures_latency() {
        let mut correlator = Correlator::default();
        let start = Instant::now();
        let request =
            br#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"search"}}"#;
        let response = br#"{"jsonrpc":"2.0","id":7,"result":{"content":[]}}"#;
        assert_eq!(correlator.observe(CLIENT, request, start), None);
        let later = start + Duration::from_millis(250);
        let call = expect_call(correlator.observe(SERVER, response, later));
        assert_eq!(call.method, "tools/call");
        assert_eq!(call.target.as_deref(), Some("search"));
        assert_eq!(call.latency_ms, 250.0);
        assert_eq!(
            call.request_tokens,
            estimate_tokens(std::str::from_utf8(request).unwrap())
        );
        assert_eq!(
            call.response_tokens,
            estimate_tokens(std::str::from_utf8(response).unwrap())
        );
        assert!(!call.is_error);
        assert!(correlator.pending.is_empty());
    }

    #[test]
    fn distinguishes_numeric_and_string_ids() {
        let mut correlator = Correlator::default();
        let now = Instant::now();
        correlator.observe(CLIENT, br#"{"id":1,"method":"ping"}"#, now);
        correlator.observe(CLIENT, br#"{"id":"1","method":"tools/list"}"#, now);
        let call = expect_call(correlator.observe(SERVER, br#"{"id":"1","result":{}}"#, now));
        assert_eq!(call.method, "tools/list");
        let call = expect_call(correlator.observe(SERVER, br#"{"id":1,"result":{}}"#, now));
        assert_eq!(call.method, "ping");
    }

    #[test]
    fn extracts_resource_uri() {
        let mut correlator = Correlator::default();
        let now = Instant::now();
        let request = br#"{"id":2,"method":"resources/read","params":{"uri":"file:///a.txt"}}"#;
        correlator.observe(CLIENT, request, now);
        let call = expect_call(correlator.observe(SERVER, br#"{"id":2,"result":{}}"#, now));
        assert_eq!(call.target.as_deref(), Some("file:///a.txt"));
    }

    #[test]
    fn flags_protocol_and_tool_errors() {
        let mut correlator = Correlator::default();
        let now = Instant::now();
        correlator.observe(CLIENT, br#"{"id":1,"method":"tools/call"}"#, now);
        correlator.observe(CLIENT, br#"{"id":2,"method":"tools/call"}"#, now);
        let protocol_error = br#"{"id":1,"error":{"code":-32601,"message":"nope"}}"#;
        let tool_error = br#"{"id":2,"result":{"isError":true,"content":[]}}"#;
        assert!(expect_call(correlator.observe(SERVER, protocol_error, now)).is_error);
        assert!(expect_call(correlator.observe(SERVER, tool_error, now)).is_error);
    }

    #[test]
    fn notifications_do_not_stay_pending() {
        let mut correlator = Correlator::default();
        let line = br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        let observation = correlator.observe(CLIENT, line, Instant::now());
        let expected = Observation::Notification {
            direction: CLIENT,
            method: "notifications/initialized".to_owned(),
        };
        assert_eq!(observation, Some(expected));
        assert!(correlator.pending.is_empty());
    }

    #[test]
    fn correlates_server_initiated_requests() {
        let mut correlator = Correlator::default();
        let now = Instant::now();
        correlator.observe(
            SERVER,
            br#"{"id":1,"method":"sampling/createMessage"}"#,
            now,
        );
        let call = expect_call(correlator.observe(CLIENT, br#"{"id":1,"result":{}}"#, now));
        assert_eq!(call.direction, SERVER);
        assert_eq!(call.method, "sampling/createMessage");
    }

    #[test]
    fn tolerates_malformed_and_unknown_lines() {
        let mut correlator = Correlator::default();
        let now = Instant::now();
        let malformed = correlator.observe(SERVER, b"Server listening on stdio\n", now);
        assert_eq!(
            malformed,
            Some(Observation::Malformed { direction: SERVER })
        );
        let stray_log = correlator.observe(SERVER, br#"{"level":30,"msg":"started"}"#, now);
        assert_eq!(
            stray_log,
            Some(Observation::Malformed { direction: SERVER })
        );
        assert_eq!(correlator.observe(SERVER, b"\n", now), None);
        assert_eq!(
            correlator.observe(SERVER, br#"{"id":99,"result":{}}"#, now),
            None
        );
    }
}
