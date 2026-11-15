use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::rpc::{Correlator, Direction, Observation, RpcCall};
use crate::trace::{EventKind, Tracer};

const TOOL_CALL_METHOD: &str = "tools/call";

// Bridges forwarded frames to the correlator and the trace writer
#[derive(Clone)]
pub struct Tap {
    pub correlator: Arc<Mutex<Correlator>>,
    pub tracer: Tracer,
    pub max_response_tokens: Option<usize>,
}

impl Tap {
    pub fn observe_rpc(&self, direction: Direction, frame: &[u8]) {
        let Ok(mut correlator) = self.correlator.lock() else {
            return;
        };
        let Some(observation) = correlator.observe(direction, frame, Instant::now()) else {
            return;
        };
        drop(correlator);
        let event = match observation {
            Observation::Call(call) => {
                self.warn_if_oversized(&call);
                EventKind::RpcCall(call)
            }
            Observation::Notification { direction, method } => {
                EventKind::Notification { direction, method }
            }
            Observation::Malformed {
                direction: Direction::ServerToClient,
            } => EventKind::RawStdout {
                line: frame_text(frame),
            },
            Observation::Malformed {
                direction: Direction::ClientToServer,
            } => EventKind::RawStdin {
                line: frame_text(frame),
            },
        };
        self.tracer.emit(event);
    }

    fn warn_if_oversized(&self, call: &RpcCall) {
        let Some(limit) = self.max_response_tokens else {
            return;
        };
        if call.method == TOOL_CALL_METHOD && call.response_tokens > limit {
            self.tracer.emit(EventKind::TokenWarning {
                method: call.method.clone(),
                target: call.target.clone(),
                response_tokens: call.response_tokens,
                limit,
            });
        }
    }
}

pub fn frame_text(frame: &[u8]) -> String {
    String::from_utf8_lossy(frame)
        .trim_end_matches(['\r', '\n'])
        .to_owned()
}
