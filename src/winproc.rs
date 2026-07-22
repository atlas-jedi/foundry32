//! Runs a child process hidden (no console flash), with timeout and captured output.

use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// After the child exits, how long to wait for pipe EOF before giving up —
/// a surviving descendant that inherited the handles can hold them open.
const PIPE_GRACE: Duration = Duration::from_secs(2);

pub struct CapturedOutput {
    pub exit_ok: bool,
    pub stdout: String,
    pub stderr: String,
}

enum PipeText {
    Stdout(String),
    Stderr(String),
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

    // Drain both pipes on detached reader threads so a chatty child can never
    // fill a pipe buffer, block on write, and read as a false timeout. Results
    // come back over a channel so collecting them can itself be bounded.
    let (sender, receiver) = mpsc::channel();
    spawn_pipe_reader(child.stdout.take(), PipeText::Stdout, sender.clone());
    spawn_pipe_reader(child.stderr.take(), PipeText::Stderr, sender);

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                // The readers are NOT joined here: a surviving descendant can
                // hold the pipes open past the kill. They exit on eventual EOF.
                return Err(format!(
                    "timeout after {timeout_secs}s: {} {}",
                    program.display(),
                    args.join(" ")
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("wait: {e}"));
            }
        }
    }

    // Child exited: EOF is normally immediate. The grace cap covers a
    // descendant that inherited the pipe handles and is still alive.
    let mut stdout = String::new();
    let mut stderr = String::new();
    let grace_deadline = Instant::now() + PIPE_GRACE;
    for _ in 0..2 {
        let remaining = grace_deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(remaining) {
            Ok(PipeText::Stdout(text)) => stdout = text,
            Ok(PipeText::Stderr(text)) => stderr = text,
            Err(_) => break,
        }
    }

    let status = child.wait().map_err(|e| format!("wait: {e}"))?;
    Ok(CapturedOutput {
        exit_ok: status.success(),
        stdout,
        stderr,
    })
}

fn spawn_pipe_reader<R: Read + Send + 'static>(
    pipe: Option<R>,
    wrap: fn(String) -> PipeText,
    sender: mpsc::Sender<PipeText>,
) {
    if let Some(mut pipe) = pipe {
        std::thread::spawn(move || {
            let mut buffer = Vec::new();
            let _ = pipe.read_to_end(&mut buffer);
            let _ = sender.send(wrap(String::from_utf8_lossy(&buffer).into_owned()));
        });
    }
}
