use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "mcp-tap",
    version,
    about = "Transparent stdio proxy, JSON-RPC profiler and token auditor for MCP servers",
    args_conflicts_with_subcommands = true,
    subcommand_negates_reqs = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub subcommand: Option<Command>,

    #[command(flatten)]
    pub proxy: ProxyArgs,
}

#[derive(Debug, Args)]
pub struct ProxyArgs {
    /// Trace file for `.jsonl` events [default: ~/.cache/mcp-tap/latest.jsonl]
    #[arg(short, long, value_name = "FILE")]
    pub log: Option<PathBuf>,

    /// Record a warning when a single tool response exceeds this many tokens
    #[arg(long, value_name = "N")]
    pub max_response_tokens: Option<usize>,

    /// MCP server command to wrap, given after `--`
    #[arg(last = true, required = true, value_name = "COMMAND")]
    pub command: Vec<String>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Follow a trace file live and print each RPC call as it completes
    Tail {
        /// Trace file [default: ~/.cache/mcp-tap/latest.jsonl]
        file: Option<PathBuf>,
    },
    /// Summarize a recorded trace: calls, errors, tokens, latency and per-tool breakdown
    Report {
        /// Trace file [default: ~/.cache/mcp-tap/latest.jsonl]
        file: Option<PathBuf>,
    },
}
