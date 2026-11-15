# Regenerate with: cargo build --release && vhs docs/demo.tape
Output docs/demo.gif

Set Shell "bash"
Set FontSize 16
Set Width 1340
Set Height 640
Set Padding 20
Set Theme "Catppuccin Mocha"
Set TypingSpeed 40ms

Hide
Type "export PATH=$PWD/target/release:$PATH PS1='$ ' && rm -f demo.jsonl && clear"
Enter
Show

Type "sh docs/demo-client.sh | mcp-tap -l demo.jsonl --max-response-tokens 1000 -- sh docs/demo-server.sh > /dev/null 2>&1 &"
Sleep 300ms
Enter
Sleep 300ms
Type "mcp-tap tail demo.jsonl"
Sleep 300ms
Enter
Sleep 6s
Ctrl+C
Sleep 500ms

Type "clear && mcp-tap report demo.jsonl"
Sleep 300ms
Enter
Sleep 5s

Hide
Type "rm -f demo.jsonl"
Enter
