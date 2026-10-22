//! Finding a ComfyUI installation, starting it so that Septet keeps its memory (a reserve of
//! graphics memory, lowest CPU priority, and on Linux the first process the kernel ends when
//! memory runs out), stopping it, and what the graphics card has free.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// What it takes to start ComfyUI.
#[derive(Clone, Debug, PartialEq)]
pub struct Install {
    /// `ComfyUI/main.py`.
    pub main: PathBuf,
    /// The Python of its environment.
    pub python: PathBuf,
    /// Arguments its launcher passes (Comfy Desktop's `launchArgs`, its shared model folders).
    pub args: Vec<String>,
}

/// The installation to start: `main` if the user chose one, else `COMFYUI_MAIN`, else Comfy
/// Desktop's first installation, else the usual folders.
pub fn find(main: Option<&Path>) -> Option<Install> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut args = Vec::new();
    let mut candidates: Vec<PathBuf> = main.map(Path::to_path_buf).into_iter().collect();
    candidates.extend(std::env::var_os("COMFYUI_MAIN").map(PathBuf::from));
    if let Some(home) = &home {
        let desktop = home.join(".local/share/comfyui-desktop-2");
        if let Ok(bytes) = std::fs::read(desktop.join("installations.json"))
            && let Ok(list) = serde_json::from_slice::<serde_json::Value>(&bytes)
        {
            for inst in list.as_array().into_iter().flatten().filter(|i| i["status"] == "installed") {
                if let Some(path) = inst["installPath"].as_str() {
                    candidates.push(Path::new(path).join("ComfyUI/main.py"));
                    if args.is_empty() {
                        args.extend(inst["launchArgs"].as_str().unwrap_or("").split_whitespace().map(str::to_owned));
                    }
                }
            }
        }
        let shared = desktop.join("shared_model_paths.yaml");
        if shared.is_file() {
            args.extend(["--extra-model-paths-config".to_owned(), shared.display().to_string()]);
        }
        for dir in ["ComfyUI-Installs/ComfyUI/ComfyUI", "ComfyUI", "comfy/ComfyUI"] {
            candidates.push(home.join(dir).join("main.py"));
        }
    }
    let main = candidates.into_iter().find(|m| m.is_file())?;
    let dir = main.parent()?;
    let python = std::env::var_os("COMFYUI_PYTHON")
        .map(PathBuf::from)
        .into_iter()
        .chain([".venv/bin/python3", "venv/bin/python3", "../.venv/bin/python3", ".venv/Scripts/python.exe"].map(|p| dir.join(p)))
        .find(|p| p.is_file())?;
    Some(Install { main, python, args })
}

/// The port of `address` (`http://127.0.0.1:8188` → 8188).
pub fn port(address: &str) -> Option<u16> {
    address.trim_end_matches('/').rsplit(':').next()?.parse().ok()
}

/// Start ComfyUI on `port`, leaving `reserve_gb` of graphics memory to Septet. Its output goes
/// to `log`.
pub fn start(install: &Install, port: u16, reserve_gb: f32, log: Option<&Path>) -> std::io::Result<Child> {
    let mut args: Vec<String> = Vec::new();
    // Only keep the launcher's arguments that don't fight ours.
    let mut skip = false;
    for a in &install.args {
        if skip {
            skip = false;
            continue;
        }
        if matches!(a.as_str(), "--port" | "--reserve-vram" | "--listen") {
            skip = true;
            continue;
        }
        args.push(a.clone());
    }
    args.extend(["--port".into(), port.to_string(), "--listen".into(), "127.0.0.1".into(), "--reserve-vram".into(), format!("{reserve_gb:.1}")]);
    let mut cmd;
    #[cfg(unix)]
    {
        // Lowest CPU priority, so Septet's own work comes first.
        cmd = Command::new("nice");
        cmd.args(["-n", "15"]).arg(&install.python);
    }
    #[cfg(not(unix))]
    {
        cmd = Command::new(&install.python);
    }
    cmd.arg("-s").arg(&install.main).args(&args).current_dir(install.main.parent().unwrap_or(Path::new("."))).stdin(Stdio::null());
    // Claude Code and some shells hide the GPU with an empty CUDA_VISIBLE_DEVICES.
    if std::env::var_os("CUDA_VISIBLE_DEVICES").is_some_and(|v| v.is_empty()) {
        cmd.env_remove("CUDA_VISIBLE_DEVICES");
    }
    match log.and_then(|l| std::fs::File::create(l).ok()) {
        Some(f) => {
            let err = f.try_clone()?;
            cmd.stdout(f).stderr(err);
        }
        None => {
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }
    }
    let child = cmd.spawn()?;
    #[cfg(target_os = "linux")]
    {
        // When memory runs out, the kernel ends ComfyUI, not Septet.
        let _ = std::fs::write(format!("/proc/{}/oom_score_adj", child.id()), "1000");
    }
    Ok(child)
}

/// End a ComfyUI Septet started: politely first, then for good.
pub fn stop(child: &mut Child) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill").args(["-TERM", &child.id().to_string()]).status();
        for _ in 0..50 {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// The graphics card's memory in MiB, `(free, total)`, and who uses it (name, MiB), largest
/// first: NVIDIA only (`nvidia-smi`), `None` elsewhere.
pub fn gpu_memory() -> Option<((u64, u64), Vec<(String, u64)>)> {
    let out = Command::new("nvidia-smi").args(["--query-gpu=memory.free,memory.total", "--format=csv,noheader,nounits"]).output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let mut nums = text.lines().next()?.split(',').map(|s| s.trim().parse::<u64>());
    let (free, total) = (nums.next()?.ok()?, nums.next()?.ok()?);
    let apps = Command::new("nvidia-smi").args(["--query-compute-apps=process_name,used_memory", "--format=csv,noheader,nounits"]).output().ok();
    let mut users: Vec<(String, u64)> = apps
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|t| {
            t.lines()
                .filter_map(|l| {
                    let (name, mib) = l.rsplit_once(',')?;
                    Some((program(name), mib.trim().parse().ok()?))
                })
                .collect()
        })
        .unwrap_or_default();
    users.sort_by_key(|(_, m)| std::cmp::Reverse(*m));
    Some(((free, total), users))
}

/// A process as nvidia-smi names it (a path, often with its arguments) → the program's name.
fn program(name: &str) -> String {
    let exe = name.split_whitespace().next().unwrap_or("");
    exe.rsplit(['/', '\\']).next().unwrap_or(exe).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_names_lose_paths_and_arguments() {
        assert_eq!(program("/usr/lib/vesktop/vesktop --type=gpu-process --x=a,b"), "vesktop");
        assert_eq!(program("C:\\Tools\\x.exe"), "x.exe");
        assert_eq!(program(" /usr/local/lib/ollama/llama-server"), "llama-server");
    }

    #[test]
    fn ports_and_arguments() {
        assert_eq!(port("http://127.0.0.1:8188/"), Some(8188));
        assert_eq!(port("http://localhost"), None);
    }

    #[test]
    fn finds_a_given_install_with_its_venv() {
        let dir = std::env::temp_dir().join(format!("septet-comfy-{}", std::process::id()));
        let main = dir.join("ComfyUI/main.py");
        std::fs::create_dir_all(dir.join("ComfyUI/.venv/bin")).unwrap();
        std::fs::write(&main, "").unwrap();
        std::fs::write(dir.join("ComfyUI/.venv/bin/python3"), "").unwrap();
        let found = find(Some(&main)).unwrap();
        assert_eq!(found.main, main);
        assert_eq!(found.python, dir.join("ComfyUI/.venv/bin/python3"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
