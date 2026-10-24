//! Updates in the app (`docs/suite/PLAN.md`, "Updates in der App"). Septet asks GitHub for the
//! latest release, downloads the file made for this platform and for the way Septet was installed,
//! checks it against the release's `SHA256SUMS.txt` and puts it in place:
//!
//! | Installed as | Downloads | Puts in place |
//! |---|---|---|
//! | Linux AppImage (`$APPIMAGE`) | the AppImage | over the AppImage, right away |
//! | Linux tar.gz (`install.sh`) | the tar.gz | over the `septet` program, right away |
//! | Windows installer (`unins000.exe` beside it) | the setup | runs it silently when Septet quits |
//! | Windows portable zip | the zip | over `septet.exe` (the running one becomes `septet.exe.old`), right away |
//! | macOS `Septet.app` | the disk image | over the app, right away |
//!
//! The new version runs from the next start; "Restart" quits Septet (asking about unsaved work as
//! always) and starts it again. Built from source, or in a folder Septet can't write to (a package
//! manager's), there are no updates in the app and the Apps page says why. Nothing downloads
//! without the user asking; the daily check only looks. `SEPTET_UPDATE_URL` points the check at
//! another releases API (tests).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const LATEST: &str = "https://api.github.com/repos/LouBoi161/septet/releases/latest";
pub const STORAGE_KEY: &str = "septet/updates";
const DAY: u64 = 24 * 60 * 60;
const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// Saved in Septet's storage ([`STORAGE_KEY`]).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Look for a new version once a day (only look: downloading waits for the user).
    pub auto_check: bool,
    /// When the last check got an answer (Unix seconds).
    pub last_check: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { auto_check: true, last_check: 0 }
    }
}

/// How this Septet was installed: what to download and how to put it in place.
#[derive(Clone, Debug, PartialEq)]
pub enum Install {
    AppImage(PathBuf),
    /// The `septet` program from the Linux tar.gz.
    Binary(PathBuf),
    WindowsSetup,
    WindowsPortable(PathBuf),
    /// The `.app` bundle.
    MacApp(PathBuf),
    /// No updates in the app, and why.
    Unsupported(String),
}

impl Install {
    pub fn detect() -> Install {
        let Ok(exe) = std::env::current_exe() else {
            return Install::Unsupported("Septet can't tell where it is installed.".into());
        };
        let exe = exe.canonicalize().unwrap_or(exe);
        let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
        detect_from(&exe, appimage, std::env::consts::OS, std::env::consts::ARCH)
    }

    /// The release file for this install.
    fn asset(&self, version: &str) -> Option<String> {
        Some(match self {
            Install::AppImage(_) => format!("Septet-{version}-x86_64.AppImage"),
            Install::Binary(_) => format!("septet-{version}-linux-x86_64.tar.gz"),
            Install::WindowsSetup => format!("Septet-{version}-windows-x64-setup.exe"),
            Install::WindowsPortable(_) => format!("Septet-{version}-windows-x64-portable.zip"),
            Install::MacApp(_) => format!("Septet-{version}-macos-{}.dmg", if std::env::consts::ARCH == "aarch64" { "arm64" } else { "x86_64" }),
            Install::Unsupported(_) => return None,
        })
    }
}

fn detect_from(exe: &Path, appimage: Option<PathBuf>, os: &str, arch: &str) -> Install {
    // target/debug/septet, target/release/septet, target/<triple>/release/septet.
    if exe.ancestors().skip(1).take(3).any(|a| a.file_name().is_some_and(|n| n == "target")) {
        return Install::Unsupported("This Septet was built from source: update it with git pull and cargo build.".into());
    }
    let cant_write = || Install::Unsupported("Septet is installed in a folder it can't change (by a package manager?): update it there.".into());
    let Some(dir) = exe.parent() else { return cant_write() };
    match os {
        "linux" if arch == "x86_64" => match appimage.filter(|a| a.is_file()) {
            Some(a) if a.parent().is_some_and(writable) => Install::AppImage(a),
            Some(_) => cant_write(),
            None if writable(dir) => Install::Binary(exe.to_path_buf()),
            None => cant_write(),
        },
        "windows" if arch == "x86_64" => {
            if dir.join("unins000.exe").is_file() {
                Install::WindowsSetup
            } else if writable(dir) {
                Install::WindowsPortable(exe.to_path_buf())
            } else {
                cant_write()
            }
        }
        "macos" => {
            // Septet.app/Contents/MacOS/septet
            let bundle = exe.ancestors().nth(3).filter(|b| b.extension().is_some_and(|e| e == "app") && dir.ends_with("Contents/MacOS"));
            match bundle {
                Some(b) if b.parent().is_some_and(writable) => Install::MacApp(b.to_path_buf()),
                Some(_) => cant_write(),
                None => Install::Unsupported("Septet isn't running from Septet.app: download the new version from the releases page.".into()),
            }
        }
        _ => Install::Unsupported("There are no Septet downloads for this system: build it from source.".into()),
    }
}

fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".septet-write-test-{}", std::process::id()));
    let ok = std::fs::File::create(&probe).is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

#[derive(Clone, Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

#[derive(Clone, Debug)]
pub struct Release {
    /// Without the "v".
    pub version: String,
    /// The release notes (Markdown).
    pub notes: String,
    /// The release on the web.
    pub page: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Clone, Debug)]
pub enum Phase {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading {
        release: Release,
        done: u64,
        total: u64,
    },
    /// Downloaded, checked and in place (or, the Windows installer, waiting for Septet to quit).
    Ready(Release),
    Failed {
        message: String,
        release: Option<Release>,
    },
}

/// What Septet does as it quits once an update is in place.
#[derive(Clone, Debug)]
enum Finish {
    /// The program that starts the new version, and its arguments.
    Start(PathBuf, Vec<String>),
    /// The Windows installer: always run it, and start Septet afterwards on a restart.
    Setup(PathBuf),
}

pub struct Updater {
    pub install: Install,
    pub settings: Settings,
    phase: Arc<Mutex<Phase>>,
    finish: Arc<Mutex<Option<Finish>>>,
    /// Quitting to start the new version.
    pub restart: bool,
    /// The program as started (Linux reports a replaced one as "… (deleted)").
    exe: Option<PathBuf>,
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Updater {
    pub fn new(settings: Settings) -> Self {
        cleanup();
        Updater {
            install: Install::detect(),
            settings,
            phase: Arc::new(Mutex::new(Phase::Idle)),
            finish: Arc::default(),
            restart: false,
            exe: std::env::current_exe().ok(),
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn set(&self, phase: Phase) {
        *self.phase.lock().unwrap_or_else(PoisonError::into_inner) = phase;
    }

    /// For Home's hint: a newer version was found, and whether it is in place already.
    pub fn available(&self) -> Option<(String, bool)> {
        match self.phase() {
            Phase::Available(r) | Phase::Downloading { release: r, .. } => Some((r.version, false)),
            Phase::Ready(r) => Some((r.version, true)),
            _ => None,
        }
    }

    /// Once a frame: the daily check, when it is due.
    pub fn logic(&mut self, ctx: &egui::Context) {
        let tests = std::env::var_os("SEPTET_AUTOTEST").is_some() && std::env::var_os("SEPTET_UPDATE_URL").is_none();
        if !self.settings.auto_check || tests || matches!(self.install, Install::Unsupported(_)) {
            return;
        }
        if matches!(self.phase(), Phase::Idle | Phase::UpToDate) && now().saturating_sub(self.settings.last_check) >= DAY {
            // Counted as checked even if it fails: offline, the next try is tomorrow (or "Check now").
            self.settings.last_check = now();
            self.check(ctx);
        }
    }

    /// Ask for the latest release, on a thread.
    pub fn check(&mut self, ctx: &egui::Context) {
        if matches!(self.phase(), Phase::Checking | Phase::Downloading { .. } | Phase::Ready(_)) {
            return;
        }
        self.set(Phase::Checking);
        self.settings.last_check = now();
        let (phase, ctx) = (self.phase.clone(), ctx.clone());
        std::thread::Builder::new()
            .name("septet-update-check".into())
            .spawn(move || {
                let next = match latest() {
                    Ok(r) if newer(&r.version, CURRENT) => Phase::Available(r),
                    Ok(_) => Phase::UpToDate,
                    Err(message) => Phase::Failed { message, release: None },
                };
                *phase.lock().unwrap_or_else(PoisonError::into_inner) = next;
                ctx.request_repaint();
            })
            .ok();
    }

    /// Download `release`, check it and put it in place, on a thread.
    pub fn download(&mut self, ctx: &egui::Context, release: Release) {
        if matches!(self.phase(), Phase::Downloading { .. } | Phase::Ready(_)) {
            return;
        }
        self.set(Phase::Downloading { release: release.clone(), done: 0, total: 0 });
        let (phase, finish, ctx, install, exe) = (self.phase.clone(), self.finish.clone(), ctx.clone(), self.install.clone(), self.exe.clone());
        std::thread::Builder::new()
            .name("septet-update-download".into())
            .spawn(move || {
                let progress = {
                    let (phase, ctx, release) = (phase.clone(), ctx.clone(), release.clone());
                    move |done: u64, total: u64| {
                        *phase.lock().unwrap_or_else(PoisonError::into_inner) = Phase::Downloading { release: release.clone(), done, total };
                        ctx.request_repaint();
                    }
                };
                let next = match fetch_and_apply(&install, &release, exe.as_deref(), &progress) {
                    Ok(f) => {
                        *finish.lock().unwrap_or_else(PoisonError::into_inner) = Some(f);
                        Phase::Ready(release)
                    }
                    Err(message) => Phase::Failed { message, release: Some(release) },
                };
                *phase.lock().unwrap_or_else(PoisonError::into_inner) = next;
                ctx.request_repaint();
            })
            .ok();
    }

    /// As Septet exits (after it saved everything and gave up its instance file): run the Windows
    /// installer, and start the new version if this was a restart.
    pub fn finish(&self) {
        let Some(finish) = self.finish.lock().unwrap_or_else(PoisonError::into_inner).take() else { return };
        // `--portable` and the like, as Septet was started (the files come back with the session).
        let flags: Vec<String> = std::env::args().skip(1).filter(|a| a.starts_with("--")).collect();
        let result = match finish {
            Finish::Start(program, args) if self.restart => std::process::Command::new(program).args(args).args(&flags).spawn().map(drop),
            Finish::Start(..) => Ok(()),
            Finish::Setup(setup) => {
                let mut cmd = std::process::Command::new(setup);
                cmd.args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS"]);
                if self.restart {
                    cmd.arg("/RELAUNCH=1");
                }
                cmd.spawn().map(drop)
            }
        };
        if let Err(e) = result {
            eprintln!("couldn't finish the update: {e}");
        }
    }
}

/// Leftovers of the last update (the portable `septet.exe.old`, partial downloads).
fn cleanup() {
    let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) else { return };
    for name in ["septet.exe.old", ".septet-update.part", ".septet.new"] {
        let _ = std::fs::remove_file(dir.join(name));
    }
}

/// `candidate` is a later version than `current` (major.minor.patch).
fn newer(candidate: &str, current: &str) -> bool {
    fn parse(v: &str) -> [u64; 3] {
        let mut out = [0; 3];
        let core = v.trim().trim_start_matches('v').split(['-', '+']).next().unwrap_or("");
        for (slot, part) in out.iter_mut().zip(core.split('.')) {
            *slot = part.parse().unwrap_or(0);
        }
        out
    }
    parse(candidate) > parse(current)
}

fn agent(timeout: Option<Duration>) -> Result<ureq::Agent, String> {
    Ok(ureq::Agent::config_builder()
        .timeout_global(timeout)
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        // A download that stalls ends some time; a slow one (300 MB at 50 kB/s) still finishes.
        .timeout_recv_body(Some(Duration::from_secs(2 * 60 * 60)))
        .user_agent(concat!("Septet/", env!("CARGO_PKG_VERSION")))
        .tls_config(crate::assistant::assets::tls_config()?)
        .build()
        .new_agent())
}

fn latest() -> Result<Release, String> {
    let url = std::env::var("SEPTET_UPDATE_URL").unwrap_or_else(|_| LATEST.to_owned());
    let mut resp = agent(Some(Duration::from_secs(30)))?
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("Couldn't reach the releases: {e}"))?;
    let text = resp.body_mut().with_config().limit(4 << 20).read_to_string().map_err(|e| format!("Couldn't read the releases: {e}"))?;
    let api: ApiRelease = serde_json::from_str(&text).map_err(|e| format!("The releases answered in a way Septet doesn't understand: {e}"))?;
    Ok(Release { version: api.tag_name.trim_start_matches('v').to_owned(), notes: api.body.unwrap_or_default(), page: api.html_url, assets: api.assets })
}

/// The SHA-256 `name` has in a `sha256sum` listing.
fn expected_hash(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name).then(|| hash.trim().to_ascii_lowercase())
    })
}

/// Download `url` into `dest`, checking its SHA-256 on the way.
fn download_to(url: &str, dest: &Path, sha256: &str, size: u64, progress: &dyn Fn(u64, u64)) -> Result<(), String> {
    let mut resp = agent(None)?.get(url).call().map_err(|e| format!("The download failed: {e}"))?;
    let total = resp.body().content_length().unwrap_or(size);
    let mut body = resp.body_mut().as_reader();
    let mut file = std::fs::File::create(dest).map_err(|e| format!("Can't write {}: {e}", dest.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let (mut done, mut shown) = (0u64, 0u64);
    loop {
        let n = body.read(&mut buf).map_err(|e| format!("The download broke off: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(|e| format!("Can't write {}: {e}", dest.display()))?;
        done += n as u64;
        if done - shown >= 1 << 20 {
            shown = done;
            progress(done, total);
        }
    }
    file.sync_all().map_err(|e| e.to_string())?;
    let got: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if got != sha256 {
        let _ = std::fs::remove_file(dest);
        return Err("The download is damaged (its checksum doesn't match). Try again.".into());
    }
    Ok(())
}

fn fetch_and_apply(install: &Install, release: &Release, exe: Option<&Path>, progress: &dyn Fn(u64, u64)) -> Result<Finish, String> {
    let name = install.asset(&release.version).ok_or("Updates aren't available for this Septet.")?;
    let find = |n: &str| release.assets.iter().find(|a| a.name == n);
    let asset = find(&name).ok_or_else(|| format!("Version {} has no {name} yet.", release.version))?;
    let sums = find("SHA256SUMS.txt").ok_or("The release has no checksums, so Septet won't install it.")?;
    let mut text = String::new();
    agent(Some(Duration::from_secs(60)))?
        .get(&sums.browser_download_url)
        .call()
        .map_err(|e| format!("Couldn't get the checksums: {e}"))?
        .body_mut()
        .as_reader()
        .take(1 << 20)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    let sha = expected_hash(&text, &name).ok_or_else(|| format!("The checksums don't list {name}."))?;
    let get = |dest: &Path| download_to(&asset.browser_download_url, dest, &sha, asset.size, progress);
    match install {
        Install::AppImage(path) => {
            let part = path.with_file_name(".septet-update.part");
            get(&part)?;
            replace_file(&part, path)?;
            Ok(Finish::Start(path.clone(), Vec::new()))
        }
        Install::Binary(path) => {
            let part = path.with_file_name(".septet-update.part");
            get(&part)?;
            let new = path.with_file_name(".septet.new");
            let result = extract_tar_program(&part, &new);
            let _ = std::fs::remove_file(&part);
            result?;
            replace_file(&new, path)?;
            Ok(Finish::Start(exe.map_or_else(|| path.clone(), Path::to_path_buf), Vec::new()))
        }
        Install::WindowsPortable(path) => {
            let dir = path.parent().ok_or("No folder")?;
            let part = dir.join(".septet-update.part");
            get(&part)?;
            let result = extract_portable_zip(&part, path);
            let _ = std::fs::remove_file(&part);
            result?;
            Ok(Finish::Start(path.clone(), Vec::new()))
        }
        Install::WindowsSetup => {
            let setup = std::env::temp_dir().join(&name);
            get(&setup)?;
            Ok(Finish::Setup(setup))
        }
        Install::MacApp(bundle) => {
            let dmg = std::env::temp_dir().join(&name);
            get(&dmg)?;
            let result = replace_mac_app(&dmg, bundle);
            let _ = std::fs::remove_file(&dmg);
            result?;
            // Septet's own flags follow `--args` (see `finish`).
            Ok(Finish::Start("open".into(), vec!["-n".into(), bundle.display().to_string(), "--args".into()]))
        }
        Install::Unsupported(why) => Err(why.clone()),
    }
}

/// Put `new` where `target` is, keeping `target`'s permissions (a running program may be replaced
/// this way on Linux and macOS: it keeps running from the old file).
fn replace_file(new: &Path, target: &Path) -> Result<(), String> {
    let mut perms = std::fs::metadata(target).map(|m| m.permissions()).map_err(|e| format!("{}: {e}", target.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(perms.mode() | 0o755);
    }
    #[cfg(not(unix))]
    perms.set_readonly(false);
    std::fs::set_permissions(new, perms).map_err(|e| format!("{}: {e}", new.display()))?;
    std::fs::rename(new, target).map_err(|e| format!("Can't replace {}: {e}", target.display()))
}

/// The `septet` program out of the Linux tar.gz (`septet-<version>-linux-x86_64/septet`).
fn extract_tar_program(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    for entry in tar.entries().map_err(|e| format!("The download isn't a tar.gz: {e}"))? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        if entry.header().entry_type().is_file() && path.file_name().is_some_and(|n| n == "septet") && path.components().count() <= 2 {
            let mut out = std::fs::File::create(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
            return out.sync_all().map_err(|e| e.to_string());
        }
    }
    Err("The download has no septet program in it.".into())
}

/// The portable zip (`Septet/septet.exe` and the license files) over the folder of `exe`. Windows
/// lets a running program be renamed, not overwritten: it becomes `septet.exe.old`, removed at the
/// next start. `portable.txt` stays as it is.
fn extract_portable_zip(archive: &Path, exe: &Path) -> Result<(), String> {
    let dir = exe.parent().ok_or("No folder")?;
    let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("The download isn't a zip: {e}"))?;
    let new = dir.join(".septet.new");
    let mut program = false;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        // PowerShell's Compress-Archive may write "Septet\septet.exe".
        let name = entry.name().replace('\\', "/");
        let Some(file_name) = name.strip_prefix("Septet/").filter(|n| !n.is_empty() && !n.contains('/') && !n.contains("..")) else { continue };
        let dest = match file_name {
            "portable.txt" => continue,
            "septet.exe" => {
                program = true;
                new.clone()
            }
            other => dir.join(other),
        };
        let mut out = std::fs::File::create(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
    }
    if !program {
        return Err("The download has no septet.exe in it.".into());
    }
    let old = dir.join("septet.exe.old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old).map_err(|e| format!("Can't move {} aside: {e}", exe.display()))?;
    if let Err(e) = std::fs::rename(&new, exe) {
        let _ = std::fs::rename(&old, exe);
        return Err(format!("Can't put the new septet.exe in place: {e}"));
    }
    Ok(())
}

/// The `Septet.app` out of the disk image, over `bundle`.
fn replace_mac_app(dmg: &Path, bundle: &Path) -> Result<(), String> {
    let parent = bundle.parent().ok_or("No folder")?;
    let mount = std::env::temp_dir().join(format!("septet-update-{}", std::process::id()));
    std::fs::create_dir_all(&mount).map_err(|e| e.to_string())?;
    let run = |cmd: &str, args: &[&std::ffi::OsStr]| -> Result<(), String> {
        let out = std::process::Command::new(cmd).args(args).output().map_err(|e| format!("{cmd}: {e}"))?;
        if out.status.success() { Ok(()) } else { Err(format!("{cmd} failed: {}", String::from_utf8_lossy(&out.stderr).trim())) }
    };
    run(
        "hdiutil",
        &["attach".as_ref(), "-nobrowse".as_ref(), "-readonly".as_ref(), "-noautoopen".as_ref(), "-mountpoint".as_ref(), mount.as_os_str(), dmg.as_os_str()],
    )?;
    let staged = parent.join(".Septet-update.app");
    let _ = std::fs::remove_dir_all(&staged);
    let copied = run("ditto", &[mount.join("Septet.app").as_os_str(), staged.as_os_str()]);
    let _ = run("hdiutil", &["detach".as_ref(), mount.as_os_str(), "-force".as_ref()]);
    let _ = std::fs::remove_dir(&mount);
    copied?;
    let old = parent.join(".Septet-old.app");
    let _ = std::fs::remove_dir_all(&old);
    std::fs::rename(bundle, &old).map_err(|e| format!("Can't move {} aside: {e}", bundle.display()))?;
    if let Err(e) = std::fs::rename(&staged, bundle) {
        let _ = std::fs::rename(&old, bundle);
        return Err(format!("Can't put the new Septet.app in place: {e}"));
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("septet-update-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn versions() {
        assert!(newer("0.3.0", "0.2.0"));
        assert!(newer("v0.10.0", "0.9.9"));
        assert!(newer("1.0.0", "0.99.0"));
        assert!(!newer("0.2.0", "0.2.0"));
        assert!(!newer("0.2.0-rc1", "0.2.0"));
        assert!(!newer("0.1.9", "0.2.0"));
    }

    #[test]
    fn checksums() {
        let sums = "abc123  Septet-0.3.0-x86_64.AppImage\nDEF456 *septet-0.3.0-linux-x86_64.tar.gz\n";
        assert_eq!(expected_hash(sums, "Septet-0.3.0-x86_64.AppImage").as_deref(), Some("abc123"));
        assert_eq!(expected_hash(sums, "septet-0.3.0-linux-x86_64.tar.gz").as_deref(), Some("def456"));
        assert_eq!(expected_hash(sums, "Septet-0.3.0-windows-x64-setup.exe"), None);
    }

    #[test]
    fn install_kinds() {
        let dir = scratch("detect");
        let exe = dir.join("septet");
        std::fs::write(&exe, b"").unwrap();
        assert_eq!(detect_from(&exe, None, "linux", "x86_64"), Install::Binary(exe.clone()));
        let image = dir.join("Septet-0.2.0-x86_64.AppImage");
        std::fs::write(&image, b"").unwrap();
        assert_eq!(detect_from(Path::new("/tmp/.mount_Septet/usr/bin/septet"), Some(image.clone()), "linux", "x86_64"), Install::AppImage(image));
        assert!(matches!(detect_from(&exe, None, "linux", "aarch64"), Install::Unsupported(_)));
        assert!(matches!(detect_from(Path::new("/home/u/septet/target/release/septet"), None, "linux", "x86_64"), Install::Unsupported(_)));
        assert!(matches!(detect_from(Path::new("/x/target/x86_64-apple-darwin/release/septet"), None, "macos", "aarch64"), Install::Unsupported(_)));
        let win = dir.join("septet.exe");
        assert_eq!(detect_from(&win, None, "windows", "x86_64"), Install::WindowsPortable(win.clone()));
        std::fs::write(dir.join("unins000.exe"), b"").unwrap();
        assert_eq!(detect_from(&win, None, "windows", "x86_64"), Install::WindowsSetup);
        let app = dir.join("Septet.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        assert_eq!(detect_from(&app.join("Contents/MacOS/septet"), None, "macos", "aarch64"), Install::MacApp(app));
        assert!(matches!(detect_from(&dir.join("septet"), None, "macos", "aarch64"), Install::Unsupported(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn asset_names_match_the_release_workflow() {
        let v = "0.3.0";
        assert_eq!(Install::AppImage("a".into()).asset(v).unwrap(), "Septet-0.3.0-x86_64.AppImage");
        assert_eq!(Install::Binary("a".into()).asset(v).unwrap(), "septet-0.3.0-linux-x86_64.tar.gz");
        assert_eq!(Install::WindowsSetup.asset(v).unwrap(), "Septet-0.3.0-windows-x64-setup.exe");
        assert_eq!(Install::WindowsPortable("a".into()).asset(v).unwrap(), "Septet-0.3.0-windows-x64-portable.zip");
        assert!(Install::MacApp("a".into()).asset(v).unwrap().starts_with("Septet-0.3.0-macos-"));
    }

    #[test]
    fn tar_program_replaces_the_binary() {
        let dir = scratch("tar");
        let archive = dir.join("septet.tar.gz");
        {
            let gz = flate2::write::GzEncoder::new(std::fs::File::create(&archive).unwrap(), flate2::Compression::fast());
            let mut tar = tar::Builder::new(gz);
            for (path, data) in [("septet-0.3.0-linux-x86_64/README.md", &b"readme"[..]), ("septet-0.3.0-linux-x86_64/septet", &b"new program"[..])] {
                let mut header = tar::Header::new_gnu();
                header.set_size(data.len() as u64);
                header.set_mode(0o755);
                header.set_cksum();
                tar.append_data(&mut header, path, data).unwrap();
            }
            tar.into_inner().unwrap().finish().unwrap();
        }
        let exe = dir.join("septet");
        std::fs::write(&exe, b"old program").unwrap();
        let new = dir.join(".septet.new");
        extract_tar_program(&archive, &new).unwrap();
        replace_file(&new, &exe).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new program");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn portable_zip_keeps_the_marker() {
        let dir = scratch("zip");
        let archive = dir.join("septet.zip");
        {
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            for (name, data) in
                [("Septet\\septet.exe", &b"new exe"[..]), ("Septet/README.md", b"new readme"), ("Septet/portable.txt", b"x"), ("../evil", b"no")]
            {
                zip.start_file(name, opts).unwrap();
                zip.write_all(data).unwrap();
            }
            zip.finish().unwrap();
        }
        let exe = dir.join("septet.exe");
        std::fs::write(&exe, b"old exe").unwrap();
        std::fs::write(dir.join("portable.txt"), b"mine").unwrap();
        extract_portable_zip(&archive, &exe).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new exe");
        assert_eq!(std::fs::read(dir.join("septet.exe.old")).unwrap(), b"old exe");
        assert_eq!(std::fs::read(dir.join("README.md")).unwrap(), b"new readme");
        assert_eq!(std::fs::read(dir.join("portable.txt")).unwrap(), b"mine");
        assert!(!dir.parent().unwrap().join("evil").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
