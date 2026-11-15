use std::cmp::Reverse;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use crate::rpc::RpcCall;
use crate::tokens::{CONTEXT_WINDOWS, percentage_of};
use crate::trace::{EventKind, TraceEvent};

#[derive(Debug, Default, PartialEq)]
pub struct CallStats {
    pub calls: usize,
    pub errors: usize,
    pub request_tokens: usize,
    pub response_tokens: usize,
    pub total_latency_ms: f64,
    pub max_latency_ms: f64,
}

impl CallStats {
    fn record(&mut self, call: &RpcCall) {
        self.calls += 1;
        self.errors += usize::from(call.is_error);
        self.request_tokens += call.request_tokens;
        self.response_tokens += call.response_tokens;
        self.total_latency_ms += call.latency_ms;
        self.max_latency_ms = self.max_latency_ms.max(call.latency_ms);
    }

    fn total_tokens(&self) -> usize {
        self.request_tokens + self.response_tokens
    }

    fn average_latency_ms(&self) -> f64 {
        if self.calls == 0 {
            return 0.0;
        }
        self.total_latency_ms / self.calls as f64
    }
}

#[derive(Debug, Default)]
pub struct Summary {
    pub totals: CallStats,
    pub by_target: Vec<((String, String), CallStats)>,
    pub token_warnings: usize,
    pub raw_lines: usize,
}

pub fn summarize(events: impl IntoIterator<Item = TraceEvent>) -> Summary {
    let mut summary = Summary::default();
    let mut by_target: HashMap<(String, String), CallStats> = HashMap::new();
    for event in events {
        match event.kind {
            EventKind::RpcCall(call) => {
                summary.totals.record(&call);
                let key = (call.method.clone(), call.target.clone().unwrap_or_default());
                by_target.entry(key).or_default().record(&call);
            }
            EventKind::TokenWarning { .. } => summary.token_warnings += 1,
            EventKind::RawStdout { .. } | EventKind::RawStdin { .. } => summary.raw_lines += 1,
            _ => {}
        }
    }
    summary.by_target = by_target.into_iter().collect();
    summary
        .by_target
        .sort_by_key(|(key, stats)| (Reverse(stats.total_tokens()), key.clone()));
    summary
}

pub fn run(path: &Path) -> io::Result<()> {
    let content = fs::read_to_string(path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("cannot read trace file {}: {error}", path.display()),
        )
    })?;
    let parsed: Vec<Option<TraceEvent>> = content
        .lines()
        .map(|line| serde_json::from_str(line).ok())
        .collect();
    let skipped = parsed.iter().filter(|event| event.is_none()).count();
    if skipped > 0 {
        eprintln!(
            "mcp-tap: skipped {skipped} unparseable lines in {}",
            path.display()
        );
    }
    let summary = summarize(parsed.into_iter().flatten());
    print_summary(&mut io::stdout().lock(), path, &summary)
}

fn print_summary(out: &mut impl Write, path: &Path, summary: &Summary) -> io::Result<()> {
    let totals = &summary.totals;
    let context_usage: Vec<String> = CONTEXT_WINDOWS
        .iter()
        .map(|(label, size)| {
            format!(
                "{label} {:.1}%",
                percentage_of(totals.total_tokens(), *size)
            )
        })
        .collect();
    writeln!(out, "mcp-tap report: {}\n", path.display())?;
    writeln!(
        out,
        "  RPC calls   {} ({})",
        totals.calls,
        pluralize(totals.errors, "error")
    )?;
    writeln!(
        out,
        "  Tokens      request {} | response {} | total {}",
        totals.request_tokens,
        totals.response_tokens,
        totals.total_tokens()
    )?;
    writeln!(out, "  Context     {}", context_usage.join(" | "))?;
    writeln!(
        out,
        "  Latency     avg {:.1} ms | max {:.1} ms",
        totals.average_latency_ms(),
        totals.max_latency_ms
    )?;
    writeln!(
        out,
        "  Warnings    {} | {}\n",
        pluralize(summary.token_warnings, "oversized response"),
        pluralize(summary.raw_lines, "non-JSON line")
    )?;
    print_breakdown(out, &summary.by_target)
}

fn pluralize(count: usize, noun: &str) -> String {
    let suffix = if count == 1 { "" } else { "s" };
    format!("{count} {noun}{suffix}")
}

fn print_breakdown(out: &mut impl Write, rows: &[((String, String), CallStats)]) -> io::Result<()> {
    writeln!(
        out,
        "  {:<24} {:<28} {:>6} {:>5} {:>9} {:>10} {:>9} {:>9}",
        "METHOD", "TARGET", "CALLS", "ERR", "REQ TOK", "RESP TOK", "AVG MS", "MAX MS"
    )?;
    for ((method, target), stats) in rows {
        writeln!(
            out,
            "  {:<24} {:<28} {:>6} {:>5} {:>9} {:>10} {:>9.1} {:>9.1}",
            method,
            target,
            stats.calls,
            stats.errors,
            stats.request_tokens,
            stats.response_tokens,
            stats.average_latency_ms(),
            stats.max_latency_ms
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::Direction;

    fn call_event(
        target: &str,
        latency_ms: f64,
        response_tokens: usize,
        is_error: bool,
    ) -> TraceEvent {
        TraceEvent {
            ts_ms: 0,
            kind: EventKind::RpcCall(RpcCall {
                direction: Direction::ClientToServer,
                method: "tools/call".to_owned(),
                target: Some(target.to_owned()),
                latency_ms,
                request_tokens: 10,
                response_tokens,
                is_error,
            }),
        }
    }

    #[test]
    fn aggregates_totals_and_sorts_targets_by_tokens() {
        let events = vec![
            call_event("small", 10.0, 5, false),
            call_event("big", 30.0, 500, true),
            call_event("small", 50.0, 15, false),
            TraceEvent {
                ts_ms: 0,
                kind: EventKind::Stderr {
                    line: "log".to_owned(),
                },
            },
            TraceEvent {
                ts_ms: 0,
                kind: EventKind::RawStdout {
                    line: "oops".to_owned(),
                },
            },
        ];
        let summary = summarize(events);
        assert_eq!(summary.totals.calls, 3);
        assert_eq!(summary.totals.errors, 1);
        assert_eq!(summary.totals.request_tokens, 30);
        assert_eq!(summary.totals.response_tokens, 520);
        assert_eq!(summary.totals.max_latency_ms, 50.0);
        assert_eq!(summary.totals.average_latency_ms(), 30.0);
        assert_eq!(summary.raw_lines, 1);
        let targets: Vec<&str> = summary
            .by_target
            .iter()
            .map(|((_, t), _)| t.as_str())
            .collect();
        assert_eq!(targets, ["big", "small"]);
        assert_eq!(summary.by_target[1].1.calls, 2);
    }

    #[test]
    fn empty_trace_has_zero_average() {
        let summary = summarize(Vec::new());
        assert_eq!(summary.totals.average_latency_ms(), 0.0);
        assert!(summary.by_target.is_empty());
    }
}
