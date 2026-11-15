use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "mcp-tap",
    version,
    about = "Transparent stdio proxy, JSON-RPC profiler and token auditor for MCP servers"
)]
pub struct Cli {
    /// MCP server command to wrap, given after `--`
    #[arg(last = true, required = true, value_name = "COMMAND")]
    pub command: Vec<String>,
}
