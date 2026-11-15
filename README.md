# mcp-tap

[![CI](../../actions/workflows/ci.yml/badge.svg)](../../actions/workflows/ci.yml)

A transparent `stdio` proxy, JSON-RPC 2.0 profiler and token auditor for
[Model Context Protocol](https://modelcontextprotocol.io) servers, written in Rust.

MCP servers that speak over `stdio` are hard to debug: `stdout` is the transport, so
you can't print to it, and you can't see how slow each tool call is or how many tokens
it costs the model. `mcp-tap` wraps the server command, passes every byte through
unchanged and writes what happened to a `.jsonl` trace.

```bash
mcp-tap --log session.jsonl -- node dist/index.js
```

![mcp-tap demo: live tail of an MCP session followed by the session report](docs/demo.gif)

## Features

- **Transparent proxy**: `stdin`, `stdout` and `stderr` are forwarded byte for byte.
  `mcp-tap` never writes to `stdout`, and the server's exit code is passed back.
- **Latency per call**: requests and responses are matched by JSON-RPC `id`, including
  requests the server sends to the client, such as sampling.
- **Token audit**: estimates the tokens in each request and response and flags tool
  responses over a limit you set (`--max-response-tokens`).
- **MCP-aware**: records the tool name for `tools/call` and `prompts/get`, the URI for
  `resources/read`, protocol errors and `isError` tool results.
- **Live tail and reports**: follow a session as it runs, or summarize one afterwards.

## Install

```bash
cargo install --git https://github.com/Juanmidev/mcp-tap
```

Or download a prebuilt binary from the Releases page.

## Usage

### Wrap a server

```bash
mcp-tap [--log FILE] [--max-response-tokens N] -- <COMMAND> [ARGS...]
```

If you leave out `--log`, the trace goes to `~/.cache/mcp-tap/latest.jsonl`. With an MCP
client such as Claude Desktop, put `mcp-tap` in front of the real command:

```json
{
  "mcpServers": {
    "my-server": {
      "command": "mcp-tap",
      "args": ["--max-response-tokens", "4000", "--", "node", "/path/to/dist/index.js"]
    }
  }
}
```

On `SIGINT`/`SIGTERM`, `mcp-tap` closes the server's `stdin`, waits up to 2 seconds for
it to exit, and kills it if it is still running.

### Follow a session live

```bash
mcp-tap tail [FILE]
```

```text
   +0.000s session started: node dist/index.js
   +0.004s   notify notifications/initialized
   +0.206s -> tools/call search                       206.1 ms     20 ->    125 tok
   +0.207s -> tools/call missing                        0.8 ms     20 ->     17 tok ERROR
```

Latency is green under 100 ms, yellow under 1 s and red above. Colors are turned off
when the output is not a terminal. `tail` notices when the trace is truncated by a new
session and starts again from the beginning of the file.

### Summarize a session

```bash
mcp-tap report [FILE]
```

```text
  RPC calls   2 (1 error)
  Tokens      request 40 | response 142 | total 182
  Context     128k 0.1% | 200k 0.1% | 1M 0.0%
  Latency     avg 103.4 ms | max 206.1 ms
  Warnings    1 oversized response | 0 non-JSON lines

  METHOD        TARGET     CALLS   ERR   REQ TOK   RESP TOK    AVG MS    MAX MS
  tools/call    search         1     0        20        125     206.1     206.1
  tools/call    missing        1     1        20         17       0.8       0.8
```

## Trace format

Each line is a JSON object with a `ts_ms` Unix timestamp and a `type` field:

| `type`          | Fields                                                                                |
|-----------------|---------------------------------------------------------------------------------------|
| `session_start` | `command`                                                                             |
| `rpc_call`      | `direction`, `method`, `target`, `latency_ms`, `request_tokens`, `response_tokens`, `is_error` |
| `notification`  | `direction`, `method`                                                                 |
| `token_warning` | `method`, `target`, `response_tokens`, `limit`                                        |
| `raw_stdout`    | `line` (a non-JSON line on the server's `stdout`, still forwarded untouched)          |
| `raw_stdin`     | `line` (a non-JSON line from the client)                                              |
| `stderr`        | `line`                                                                                |
| `session_end`   | `exit_code`                                                                           |

Each session overwrites the trace file.

## Token estimation

Token counts are an offline estimate (`chars / 3.8`), tuned for code and JSON and
matching [repo-pack](https://github.com/Juanmidev/repo-pack). They are fine for comparing
tools and spotting oversized responses, but they are not exact tokenizer counts.

## Limitations

- Frames are newline-delimited JSON, as the MCP `stdio` transport specifies. JSON-RPC
  batches (removed in MCP 2025-06-18) are forwarded but not correlated.
- Requests that never get a response are not reported.

## Development

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## License

MIT
