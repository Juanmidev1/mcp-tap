#![cfg(unix)]

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::Value;

const SERVER_DELAY_MS: f64 = 100.0;
const SERVER_EXIT_CODE: i32 = 3;
const NOTIFICATION: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
const REQUEST: &str = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"greet"}}"#;
const RESPONSE: &str =
    r#"{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"hello"}]}}"#;

// Replies to request id 1 after a fixed delay, logs to stderr and exits with a custom code
const SERVER_SCRIPT: &str = r#"
while IFS= read -r line; do
  case "$line" in
    *'"id":1'*) sleep 0.1; printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"hello"}]}}' ;;
  esac
done
echo 'server shutting down' >&2
exit 3
"#;

fn mcp_tap() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mcp-tap"))
}

fn run_proxy(trace: &Path) -> Output {
    let mut child = mcp_tap()
        .arg("--log")
        .arg(trace)
        .args(["--", "sh", "-c", SERVER_SCRIPT])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "{NOTIFICATION}\n{REQUEST}").unwrap();
    drop(stdin);
    child.wait_with_output().unwrap()
}

fn read_events(trace: &Path) -> Vec<Value> {
    let content = std::fs::read_to_string(trace).unwrap();
    content
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn expected_tokens(text: &str) -> u64 {
    (text.chars().count() as f64 / 3.8).ceil() as u64
}

#[test]
fn proxies_transparently_and_records_the_call() {
    let dir = tempfile::tempdir().unwrap();
    let trace = dir.path().join("session.jsonl");
    let output = run_proxy(&trace);

    assert_eq!(output.status.code(), Some(SERVER_EXIT_CODE));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{RESPONSE}\n")
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("server shutting down"));

    let events = read_events(&trace);
    let call = events
        .iter()
        .find(|event| event["type"] == "rpc_call")
        .unwrap();
    assert_eq!(call["method"], "tools/call");
    assert_eq!(call["target"], "greet");
    assert_eq!(call["is_error"], false);
    assert!(call["latency_ms"].as_f64().unwrap() >= SERVER_DELAY_MS);
    assert_eq!(call["request_tokens"], expected_tokens(REQUEST));
    assert_eq!(call["response_tokens"], expected_tokens(RESPONSE));
    assert!(events.iter().any(|event| event["type"] == "notification"));
    let last = events.last().unwrap();
    assert_eq!(last["type"], "session_end");
    assert_eq!(last["exit_code"], SERVER_EXIT_CODE);
}

#[test]
fn report_summarizes_a_recorded_session() {
    let dir = tempfile::tempdir().unwrap();
    let trace = dir.path().join("session.jsonl");
    run_proxy(&trace);

    let output = mcp_tap().arg("report").arg(&trace).output().unwrap();
    let report = String::from_utf8(output.stdout).unwrap();
    assert!(output.status.success());
    assert!(report.contains("RPC calls   1 (0 errors)"), "{report}");
    assert!(
        report
            .lines()
            .any(|line| line.contains("tools/call") && line.contains("greet"))
    );
}

#[test]
fn fails_cleanly_when_the_server_cannot_start() {
    let dir = tempfile::tempdir().unwrap();
    let output = mcp_tap()
        .arg("--log")
        .arg(dir.path().join("session.jsonl"))
        .args(["--", "definitely-not-a-real-binary"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("failed to spawn"));
}
