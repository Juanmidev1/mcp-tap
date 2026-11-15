#!/bin/sh
# Fake MCP server for the README demo: answers a few methods with different delays and sizes
reply() {
  printf '{"jsonrpc":"2.0","id":%s,%s}\n' "$1" "$2"
}

echo "demo-server: ready on stdio" >&2
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) sleep 0.05; reply "$id" '"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}}}' ;;
    *'"method":"tools/list"'*) sleep 0.02; reply "$id" '"result":{"tools":[{"name":"search"},{"name":"read_file"}]}' ;;
    *'"name":"search"'*) sleep 0.3; reply "$id" '"result":{"content":[{"type":"text","text":"3 matches in src/"}]}' ;;
    *'"name":"read_file"'*)
      sleep 1.2
      body=$(head -c 6000 /dev/zero | tr '\0' 'x')
      reply "$id" "\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"$body\"}]}" ;;
    *'"method":"tools/call"'*) reply "$id" '"error":{"code":-32602,"message":"Unknown tool"}' ;;
  esac
done
echo "demo-server: stdin closed, bye" >&2
