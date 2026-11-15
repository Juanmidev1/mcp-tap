use std::io::{self, ErrorKind};
use std::process::{ExitCode, ExitStatus, Stdio};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

const SHUTDOWN_GRACE_PERIOD: Duration = Duration::from_secs(2);

pub async fn run(command: &[String]) -> io::Result<ExitCode> {
    let mut child = spawn_server(command)?;
    let child_stdin = take_pipe(child.stdin.take(), "stdin")?;
    let child_stdout = take_pipe(child.stdout.take(), "stdout")?;
    let child_stderr = take_pipe(child.stderr.take(), "stderr")?;

    // Dropping child_stdin when parent stdin hits EOF lets the server shut down cleanly
    let stdin_task = tokio::spawn(forward_lines(tokio::io::stdin(), child_stdin));
    let stdout_task = tokio::spawn(forward_lines(child_stdout, tokio::io::stdout()));
    let stderr_task = tokio::spawn(forward_lines(child_stderr, tokio::io::stderr()));

    let outputs_drained = async {
        report_task_error("stdout", stdout_task).await;
        report_task_error("stderr", stderr_task).await;
    };
    tokio::select! {
        () = outputs_drained => {}
        () = shutdown_requested() => {}
    }
    if stdin_task.is_finished() {
        report_task_error("stdin", stdin_task).await;
    } else {
        stdin_task.abort();
    }
    let status = wait_or_kill(&mut child).await?;
    Ok(exit_code_from(status))
}

// Mirrors the MCP stdio shutdown sequence: close stdin, give the server time to exit, then kill
async fn wait_or_kill(child: &mut Child) -> io::Result<ExitStatus> {
    match tokio::time::timeout(SHUTDOWN_GRACE_PERIOD, child.wait()).await {
        Ok(status) => status,
        Err(_elapsed) => {
            child.kill().await?;
            child.wait().await
        }
    }
}

async fn shutdown_requested() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let Ok(mut terminate) = signal(SignalKind::terminate()) else {
            return std::future::pending().await;
        };
        tokio::select! {
            result = tokio::signal::ctrl_c() => if result.is_err() { std::future::pending().await },
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    if tokio::signal::ctrl_c().await.is_err() {
        std::future::pending::<()>().await;
    }
}

fn spawn_server(command: &[String]) -> io::Result<Child> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "missing MCP server command"))?;
    Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("failed to spawn `{program}`: {error}"),
            )
        })
}

fn take_pipe<T>(pipe: Option<T>, name: &str) -> io::Result<T> {
    pipe.ok_or_else(|| io::Error::other(format!("child {name} pipe is not available")))
}

async fn forward_lines<R, W>(reader: R, mut writer: W) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut frame = Vec::new();
    loop {
        frame.clear();
        if reader.read_until(b'\n', &mut frame).await? == 0 {
            return Ok(());
        }
        match write_frame(&mut writer, &frame).await {
            Err(error) if error.kind() == ErrorKind::BrokenPipe => return Ok(()),
            result => result?,
        }
    }
}

async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, frame: &[u8]) -> io::Result<()> {
    writer.write_all(frame).await?;
    writer.flush().await
}

async fn report_task_error(stream: &str, task: JoinHandle<io::Result<()>>) {
    match task.await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => eprintln!("mcp-tap: error forwarding {stream}: {error}"),
        Err(error) => eprintln!("mcp-tap: {stream} forwarder crashed: {error}"),
    }
}

fn exit_code_from(status: ExitStatus) -> ExitCode {
    let code = status.code().unwrap_or_else(|| signal_exit_code(status));
    ExitCode::from(u8::try_from(code).unwrap_or(u8::MAX))
}

#[cfg(unix)]
fn signal_exit_code(status: ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    const SIGNAL_EXIT_CODE_BASE: i32 = 128;
    status
        .signal()
        .map_or(1, |signal| SIGNAL_EXIT_CODE_BASE + signal)
}

#[cfg(not(unix))]
fn signal_exit_code(_status: ExitStatus) -> i32 {
    1
}
