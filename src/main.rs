mod cli;
mod proxy;
mod report;
mod rpc;
mod tail;
mod tap;
mod tokens;
mod trace;

use std::io::{self, ErrorKind, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;

use cli::{Cli, Command, ProxyArgs};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match &cli.subcommand {
        Some(Command::Tail { file }) => run_viewer(tail::run, file.clone()),
        Some(Command::Report { file }) => run_viewer(report::run, file.clone()),
        None => run_proxy(&cli.proxy),
    };
    match result {
        Ok(exit_code) => exit_code,
        Err(error) => {
            eprintln!("mcp-tap: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_proxy(args: &ProxyArgs) -> io::Result<ExitCode> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("failed to start async runtime: {error}"),
            )
        })?;
    let result = runtime.block_on(proxy::run(args));
    // Tokio's stdin reader blocks a thread that a normal shutdown would wait on forever
    runtime.shutdown_background();
    result.map(ExitCode::from)
}

fn run_viewer(viewer: fn(&Path) -> io::Result<()>, file: Option<PathBuf>) -> io::Result<ExitCode> {
    if !io::stdout().is_terminal() {
        colored::control::set_override(false);
    }
    let path = file.unwrap_or_else(trace::default_log_path);
    match viewer(&path) {
        // Piping into `head` or `less` and quitting early is a normal way to stop
        Err(error) if error.kind() == ErrorKind::BrokenPipe => Ok(ExitCode::SUCCESS),
        result => result.map(|()| ExitCode::SUCCESS),
    }
}
