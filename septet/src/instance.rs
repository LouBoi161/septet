//! One Septet at a time. Starting it again (from the launcher, or by opening a file in the file
//! manager) hands the files to the running Septet and exits, so each app still has one tab.
//!
//! The running Septet listens on a loopback port it writes, with a random token, to `instance` in
//! the config folder. `SEPTET_MULTI_INSTANCE=1` (and autotests) skip all of this.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct Primary {
    listener: TcpListener,
    token: String,
    file: PathBuf,
}

fn instance_file() -> Option<PathBuf> {
    crate::recent::config_dir().map(|d| d.join("instance"))
}

/// `None` when another Septet took `files`: this one should exit. Otherwise the listener to serve
/// (if one could be set up at all).
pub fn claim(files: &[PathBuf]) -> Option<Option<Primary>> {
    if std::env::var_os("SEPTET_MULTI_INSTANCE").is_some() || std::env::var_os("SEPTET_AUTOTEST").is_some() {
        return Some(None);
    }
    let Some(file) = instance_file() else { return Some(None) };
    if let Ok(text) = std::fs::read_to_string(&file)
        && forward(&text, files).is_some()
    {
        return None;
    }
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).ok();
    Some(listener.and_then(|listener| {
        let token = new_token();
        let port = listener.local_addr().ok()?.port();
        std::fs::create_dir_all(file.parent()?).ok()?;
        std::fs::write(&file, format!("{port} {token}\n")).ok()?;
        Some(Primary { listener, token, file })
    }))
}

/// Hand `files` to the Septet named in the instance file; `Some` once it confirmed.
fn forward(text: &str, files: &[PathBuf]) -> Option<()> {
    let (port, token) = text.trim().split_once(' ')?;
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port.parse::<u16>().ok()?));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(500)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    let mut msg = format!("{token}\n");
    for f in files {
        // The running Septet has its own working directory.
        let f = std::path::absolute(f).unwrap_or_else(|_| f.clone());
        msg.push_str(f.to_str()?);
        msg.push('\n');
    }
    msg.push('\n');
    stream.write_all(msg.as_bytes()).ok()?;
    let mut ack = String::new();
    BufReader::new(stream).read_line(&mut ack).ok()?;
    (ack.trim() == "ok").then_some(())
}

fn new_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u32(std::process::id());
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    let a = h.finish();
    h.write_u64(a);
    format!("{a:016x}{:016x}", h.finish())
}

/// The running Septet's side: handovers waiting to be opened.
pub struct Running {
    inbox: Arc<Mutex<Vec<Vec<PathBuf>>>>,
    token: String,
    file: PathBuf,
}

impl Primary {
    /// Accept handovers on a thread; they wait in the inbox and wake the UI.
    pub fn serve(self, ctx: egui::Context) -> Running {
        let inbox: Arc<Mutex<Vec<Vec<PathBuf>>>> = Arc::default();
        let out = inbox.clone();
        let token = self.token.clone();
        std::thread::Builder::new()
            .name("septet-instance".into())
            .spawn(move || {
                for stream in self.listener.incoming().flatten() {
                    if let Some(files) = receive(stream, &self.token) {
                        out.lock().unwrap().push(files);
                        ctx.request_repaint();
                    }
                }
            })
            .ok();
        Running { inbox, token, file: self.file }
    }
}

impl Running {
    /// File lists other launches handed over since the last call (an empty list: just come to the front).
    pub fn take(&self) -> Vec<Vec<PathBuf>> {
        std::mem::take(&mut *self.inbox.lock().unwrap())
    }

    /// On exit: the next launch starts fresh instead of trying us (unless another Septet took over).
    pub fn release(&self) {
        if std::fs::read_to_string(&self.file).is_ok_and(|t| t.trim().ends_with(&self.token)) {
            let _ = std::fs::remove_file(&self.file);
        }
    }
}

fn receive(stream: TcpStream, token: &str) -> Option<Vec<PathBuf>> {
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    if line.trim_end() != token {
        return None;
    }
    let mut files = Vec::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).ok()? == 0 {
            break;
        }
        let path = line.trim_end_matches(['\r', '\n']);
        if path.is_empty() {
            break;
        }
        files.push(PathBuf::from(path));
    }
    (&stream).write_all(b"ok\n").ok()?;
    Some(files)
}

