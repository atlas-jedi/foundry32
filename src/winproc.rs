//! Runs a child process hidden (no console flash), with timeout and captured output.

use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct CapturedOutput {
    pub exit_ok: bool,
    pub stdout: String,
    pub stderr: String,
}

pub fn run_captured(
    program: &Path,
    args: &[String],
    cwd: Option<&Path>,
    timeout_secs: u64,
) -> Result<CapturedOutput, String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }

    let mut child = command
        .spawn()
        .map_err(|e| format!("spawn {}: {e}", program.display()))?;

    // Drain both pipes on reader threads while polling, so a chatty child can
    // never fill a pipe buffer, block on write, and read as a false timeout.
    let stdout_reader = spawn_pipe_reader(child.stdout.take());
    let stderr_reader = spawn_pipe_reader(child.stderr.take());

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let timed_out = loop {
        match child.try_wait() {
            Ok(Some(_)) => break false,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break true;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => return Err(format!("wait: {e}")),
        }
    };

    let stdout = join_pipe_reader(stdout_reader);
    let stderr = join_pipe_reader(stderr_reader);

    if timed_out {
        return Err(format!(
            "timeout after {timeout_secs}s: {} {}",
            program.display(),
            args.join(" ")
        ));
    }

    let status = child.wait().map_err(|e| format!("wait: {e}"))?;
    Ok(CapturedOutput {
        exit_ok: status.success(),
        stdout,
        stderr,
    })
}

fn spawn_pipe_reader<R: Read + Send + 'static>(pipe: Option<R>) -> Option<JoinHandle<String>> {
    let mut pipe = pipe?;
    Some(std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = pipe.read_to_end(&mut buffer);
        String::from_utf8_lossy(&buffer).into_owned()
    }))
}

fn join_pipe_reader(reader: Option<JoinHandle<String>>) -> String {
    reader.and_then(|handle| handle.join().ok()).unwrap_or_default()
}
