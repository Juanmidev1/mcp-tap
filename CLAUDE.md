# CLAUDE.md - `mcp-tap`

Transparent async `stdio` proxy, JSON-RPC 2.0 profiler, and token auditor for Model Context Protocol (MCP) servers, written in Rust.

## Project Goal

When developing or running MCP servers over `stdio`, debugging and performance auditing are blind because `stdout` is occupied by the JSON-RPC transport. `mcp-tap` wraps any MCP server command transparently:

```bash
mcp-tap --log session.jsonl -- node dist/index.js
```

It forwards `stdin`, `stdout`, and `stderr` without altering the protocol, correlates JSON-RPC requests and responses by `id` to measure exact latency, estimates token consumption per payload, and writes structured telemetry to a `.jsonl` trace file that can be inspected live (`mcp-tap tail`) or summarized (`mcp-tap report`).

---

## Tech Stack & Cargo Configuration

* **Rust Edition:** `2024`
* **Dependencies:**
  * `tokio` (`features = ["rt-multi-thread", "macros", "process", "io-util", "io-std", "fs", "sync", "time", "signal"]`) - Async process management and concurrent pipe multiplexing.
  * `clap` (`features = ["derive"]`) - CLI argument and subcommand parsing (wrapped server command goes after `--`).
  * `serde` (`features = ["derive"]`) & `serde_json` - JSON-RPC 2.0 framing and `.jsonl` trace serialization.
  * `colored` - Terminal output styling (disabled when `stdout` is not a TTY).
* **Dev Dependencies:**
  * `tempfile` - Integration tests with temporary `.jsonl` logs and mock `stdio` servers.
* **Release Profile (keep binary small & fast):**
  ```toml
  [profile.release]
  strip = true
  lto = true
  codegen-units = 1
  ```

---

## Architecture & Module Layout (YAGNI)

Keep the codebase flat, modular, and free of unnecessary abstractions:

```text
mcp-tap/
├── Cargo.toml
├── README.md
├── CLAUDE.md
├── tests/proxy_integration.rs
└── src/
    ├── main.rs      # Entrypoint, CLI dispatch, ExitCode propagation
    ├── cli.rs       # clap definitions (proxy mode, `tail`, `report`)
    ├── proxy.rs     # Async child process spawning, bidirectional stdio forwarding, shutdown
    ├── tap.rs       # Bridges forwarded frames to the correlator and the trace writer
    ├── rpc.rs       # JSON-RPC 2.0 frame parsing, ID correlation, MCP method extraction
    ├── tokens.rs    # Offline token estimation (`chars / 3.8`) & context percentages
    ├── trace.rs     # `.jsonl` event types and async writer
    ├── tail.rs      # Live `tail` follower
    └── report.rs    # `report` aggregator and summary tables
```

### 1. CLI Interface (`src/cli.rs`)

Support three modes cleanly:

1. **Default Proxy Mode:**
   `mcp-tap [OPTIONS] -- <COMMAND> [ARGS...]`
   * `-l, --log <FILE>`: Path to write `.jsonl` trace events (default: `~/.cache/mcp-tap/latest.jsonl`, or `./mcp-tap.jsonl` when `HOME` is unset, creating parent directories if needed).
   * `--max-response-tokens <N>`: Emit a warning event in the trace if any single tool response exceeds `<N>` tokens.
2. **Live Tail Subcommand:**
   `mcp-tap tail [FILE]`
   * Watches the `.jsonl` trace file in real time (polling appended lines, handling file truncation) and prints formatted RPC calls with color-coded latency and token counts.
3. **Session Report Subcommand:**
   `mcp-tap report [FILE]`
   * Reads a recorded `.jsonl` file and outputs an engineering summary: total RPC calls, error count, total request/response tokens, average & max latency, and a per-tool breakdown table sorted by token usage.

### 2. Transparent Async Proxy (`src/proxy.rs`)

* **Protocol Integrity (CRITICAL):**
  * Never write logs, banners, or ANSI escape codes to `stdout`. `stdout` belongs exclusively to the wrapped MCP server.
  * Forward `stdin` -> child `stdin` and child `stdout` -> parent `stdout` byte-for-byte on newline-delimited frames (`read_until(b'\n')` using `tokio::io::BufReader` so arbitrarily large JSON payloads never truncate). Frames are forwarded before they are observed.
  * Forward child `stderr` -> parent `stderr` transparently while recording stderr lines as `stderr` events in the `.jsonl` trace.
* **Lifecycle & Signals:**
  * When parent `stdin` reaches EOF, close child `stdin` gracefully so the child MCP server shuts down cleanly.
  * On `SIGINT`/`SIGTERM`, close child `stdin`, wait a grace period, then kill the child.
  * Propagate the exact exit code of the child process back through `main() -> ExitCode` (`128 + signal` when killed by a signal).
  * Handle `ErrorKind::BrokenPipe` cleanly without panicking.

### 3. JSON-RPC 2.0 Correlator (`src/rpc.rs`)

* Parse outgoing (client -> server) and incoming (server -> client) lines with `serde_json::Value` (tolerant parsing: if a line is not valid JSON, still forward the raw bytes untouched and log a `raw_stdout` / `raw_stdin` event).
* **Correlation:**
  * Store pending requests in a `HashMap` keyed by request direction and serialized `id` (supporting both numeric and string JSON-RPC IDs, and server-initiated requests), recording `Instant`, `method`, extracted MCP target (e.g., `params.name` for `tools/call`, `params.uri` for `resources/read`), and `request_tokens`.
  * When a response with a matching `id` arrives, compute `latency_ms`, `response_tokens`, and `is_error` (`result.isError == true` or top-level `error` field present), then emit a completed `RpcCall` trace event.
  * Handle JSON-RPC notifications (messages without `id`, such as `notifications/initialized`) without leaking memory in the pending map.

### 4. Token Estimation (`src/tokens.rs`)

* Calibrated offline ratio for code/JSON payloads: `(text.chars().count() as f64 / 3.8).ceil() as usize`.
* Provide context window percentage helpers (`128k`, `200k`, `1M`) consistent with `repo-pack`.

---

## Coding Standards & Verification

* **Error Handling:** Idiomatic `Result` propagation; no `.unwrap()` or `.expect()` in production paths (only allowed inside `#[cfg(test)]` and `tests/`).
* **Formatting & Linting:** Every change must pass:
  ```bash
  cargo fmt --check
  cargo clippy --all-targets -- -D warnings
  cargo test
  ```
* **Testing Strategy:**
  * Unit tests in `src/rpc.rs` for JSON-RPC request/response matching, `tools/call` name extraction, notifications, and malformed JSON resilience.
  * Unit tests in `src/tokens.rs`, `src/trace.rs`, `src/tail.rs` and `src/report.rs`.
  * End-to-end integration tests in `tests/proxy_integration.rs`: spawn `mcp-tap` wrapping a `sh -c` server that answers JSON-RPC with a controlled delay, asserting that `stdout` is unaltered, `.jsonl` records accurate latency and token counts, and the child exit code is preserved.
