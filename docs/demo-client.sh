#!/bin/sh
# Plays the client side of an MCP session for the README demo
send() {
  printf '%s\n' "$1"
  sleep "$2"
}

send '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}' 0.4
send '{"jsonrpc":"2.0","method":"notifications/initialized"}' 0.4
send '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' 0.6
send '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"search","arguments":{"q":"todo"}}}' 0.8
send '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"read_file","arguments":{"path":"big.json"}}}' 1.6
send '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"deploy"}}' 0.8
