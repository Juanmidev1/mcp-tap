mod cli;
mod proxy;

use std::process::ExitCode;

use clap::Parser;

use cli::Cli;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("mcp-tap: failed to start async runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    let result = runtime.block_on(proxy::run(&cli.command));
    // Tokio's stdin reader blocks a thread that a normal shutdown would wait on forever
    runtime.shutdown_background();
    match result {
        Ok(exit_code) => exit_code,
        Err(error) => {
            eprintln!("mcp-tap: {error}");
            ExitCode::FAILURE
        }
    }
}
