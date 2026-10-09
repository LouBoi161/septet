//! One conversation: a long-running `claude -p` process, fed through stdin and read on a thread.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};

use super::cli::{self, SessionArgs};
use super::protocol::{self, Event, ImageInput};

/// What the reader thread reports.
#[derive(Debug)]
pub enum Output {
    Event(Event),
    /// A line that was not JSON (kept for diagnostics).
    Noise(String),
    /// The process ended; `stderr` is its last few lines.
    Exited {
        stderr: String,
    },
}

pub struct Session {
    pub id: String,
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<Output>,
    next_request: u64,
}

impl Session {
    /// Start Claude Code in `workspace`. `wake` runs after every output line (e.g. a repaint request).
    pub fn start(exe: &Path, args: &SessionArgs, workspace: &Path, wake: impl Fn() + Send + Clone + 'static) -> std::io::Result<Self> {
        std::fs::create_dir_all(workspace)?;
        let mut cmd = cli::command(exe);
        cmd.args(args.to_args()).current_dir(workspace).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        // Its own process group, so the shell commands it runs end with it.
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().expect("piped");
        let stderr = child.stderr.take().expect("piped");
        let (tx, rx) = channel();
        let (err_tx, err_rx) = channel::<String>();
        std::thread::Builder::new().name("claude-stderr".into()).spawn(move || {
            let mut buf = String::new();
            let _ = BufReader::new(stderr).read_to_string(&mut buf);
            let tail: Vec<&str> = buf.lines().rev().take(20).collect();
            let _ = err_tx.send(tail.into_iter().rev().collect::<Vec<_>>().join("\n"));
        })?;
        let w = wake.clone();
        std::thread::Builder::new().name("claude-stdout".into()).spawn(move || read(stdout, tx, err_rx, w))?;
        let stdin = child.stdin.take();
        Ok(Session { id: args.session_id.clone(), child, stdin, rx, next_request: 0 })
    }

    fn write(&mut self, line: &str) -> std::io::Result<()> {
        let stdin = self.stdin.as_mut().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "Claude Code has stopped"))?;
        stdin.write_all(line.as_bytes())?;
        stdin.write_all(b"\n")?;
        stdin.flush()
    }

    pub fn send(&mut self, text: &str, images: &[ImageInput]) -> std::io::Result<()> {
        let line = protocol::user_message(&self.id, text, images);
        self.write(&line)
    }

    /// Stop the current turn; the conversation stays open.
    pub fn interrupt(&mut self) -> std::io::Result<()> {
        self.next_request += 1;
        let line = protocol::interrupt(&format!("septet-{}", self.next_request));
        self.write(&line)
    }

    /// Everything that arrived since the last call.
    pub fn drain(&self) -> Vec<Output> {
        self.rx.try_iter().collect()
    }

    /// End the process: close stdin (Claude Code exits on its own), give it a moment, then kill it
    /// and whatever it started.
    pub fn stop(&mut self) {
        self.stdin = None;
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1500);
        while std::time::Instant::now() < deadline {
            if !matches!(self.child.try_wait(), Ok(None)) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        #[cfg(unix)]
        // SAFETY: plain syscall; the group id is our child's pid (it leads its own group).
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGTERM);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
    }
}

fn read(stdout: impl Read, tx: Sender<Output>, stderr: Receiver<String>, wake: impl Fn()) {
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let out: Vec<Output> = match protocol::parse_line(&line) {
            Ok(events) => events.into_iter().map(Output::Event).collect(),
            Err(_) => vec![Output::Noise(line)],
        };
        for o in out {
            if tx.send(o).is_err() {
                return;
            }
        }
        wake();
    }
    let stderr = stderr.recv_timeout(std::time::Duration::from_secs(2)).unwrap_or_default();
    let _ = tx.send(Output::Exited { stderr });
    wake();
}
