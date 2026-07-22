//! Runs a child process hidden (no console flash), with timeout and captured output.

use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
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

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "timeout after {timeout_secs}s: {} {}",
                    program.display(),
                    args.join(" ")
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => return Err(format!("wait: {e}")),
        }
    }

    let output = child.wait_with_output().map_err(|e| format!("output: {e}"))?;
    Ok(CapturedOutput {
        exit_ok: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}
