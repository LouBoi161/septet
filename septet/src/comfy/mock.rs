//! A stand-in ComfyUI for tests and the autotest: the HTTP endpoints Septet uses, no GPU. A
//! text-to-image job gives a red image of the asked size, an edit gives the uploaded image 1
//! back with its grey turned blue (so a Generative Fill shows where it filled).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

#[derive(Default)]
struct State {
    requests: Vec<String>,
    uploads: HashMap<String, Vec<u8>>,
    /// Prompt id → (finished at, the result's PNG); `None` result: still running or cancelled.
    jobs: HashMap<String, (Instant, Option<Vec<u8>>)>,
    hidden: Vec<String>,
    /// A model "Ollama" holds; unloading it frees the graphics memory.
    ollama: Option<String>,
}

pub struct Server {
    port: u16,
    state: Arc<Mutex<State>>,
    slow: Arc<AtomicBool>,
    vram_free_mib: Arc<AtomicU64>,
}

impl Server {
    /// Listen on a free local port.
    pub fn start() -> std::io::Result<Server> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let server = Server { port, state: Arc::default(), slow: Arc::default(), vram_free_mib: Arc::new(AtomicU64::new(10_000)) };
        let (state, slow, vram) = (server.state.clone(), server.slow.clone(), server.vram_free_mib.clone());
        std::thread::Builder::new().name("septet-comfy-mock".into()).spawn(move || {
            for stream in listener.incoming().flatten() {
                let (state, slow, vram) = (state.clone(), slow.clone(), vram.clone());
                let _ = std::thread::Builder::new().spawn(move || {
                    let _ = serve(stream, &state, &slow, &vram);
                });
            }
        })?;
        Ok(server)
    }

    pub fn address(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// "METHOD /path" of every request so far.
    pub fn requests(&self) -> Vec<String> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).requests.clone()
    }

    /// Jobs take ten seconds (to test cancelling).
    #[cfg(test)]
    pub fn set_slow(&self, slow: bool) {
        self.slow.store(slow, Ordering::Relaxed);
    }

    /// Leave a model file out of the loaders' lists.
    #[cfg(test)]
    pub fn hide_model(&self, file: &str) {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).hidden.push(file.to_owned());
    }

    /// Also play Ollama, holding `model` (`/api/ps`); unloading it frees the memory.
    #[cfg(test)]
    pub fn set_ollama_model(&self, model: Option<&str>) {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).ollama = model.map(str::to_owned);
    }

    #[cfg(test)]
    pub fn set_vram_free_mib(&self, mib: u64) {
        self.vram_free_mib.store(mib, Ordering::Relaxed);
    }
}

const MODELS: &[&str] = &[
    "qwen_image_2.1_turbo_int8_convrot.safetensors",
    "qwen_image_2.1_int8_convrot.safetensors",
    "qwen3vl_8b_int8_convrot.safetensors",
    "qwen_image_2.1_vae_bf16.safetensors",
];

fn serve(stream: TcpStream, state: &Mutex<State>, slow: &AtomicBool, vram: &AtomicU64) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or("").to_owned(), parts.next().unwrap_or("").to_owned());
    let mut length = 0usize;
    let mut content_type = String::new();
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 || h == "\r\n" {
            break;
        }
        let lower = h.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            length = v.trim().parse().unwrap_or(0);
        }
        if lower.starts_with("content-type:") {
            content_type = h["content-type:".len()..].trim().to_owned();
        }
    }
    let mut body = vec![0; length.min(512 << 20)];
    reader.read_exact(&mut body)?;
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let lock = || state.lock().unwrap_or_else(PoisonError::into_inner);
    lock().requests.push(format!("{method} {path}"));
    let (status, ctype, out): (u16, &str, Vec<u8>) = match (method.as_str(), path) {
        ("GET", "/system_stats") => {
            let free = vram.load(Ordering::Relaxed) << 20;
            (
                200,
                "application/json",
                json!({"system": {"comfyui_version": "stand-in"}, "devices": [{"vram_free": free, "vram_total": 12u64 << 30}]}).to_string().into_bytes(),
            )
        }
        ("GET", p) if p.starts_with("/object_info/") => {
            let class = &p["/object_info/".len()..];
            let hidden = lock().hidden.clone();
            let files: Vec<&str> = MODELS.iter().copied().filter(|m| !hidden.iter().any(|h| h == m)).collect();
            let required = match class {
                "UNETLoader" => json!({"unet_name": [files]}),
                "CLIPLoader" => json!({"clip_name": [files]}),
                "VAELoader" => json!({"vae_name": [files]}),
                _ => json!({}),
            };
            (200, "application/json", json!({ class: {"input": {"required": required}} }).to_string().into_bytes())
        }
        ("POST", "/upload/image") => {
            let (name, png) = multipart_image(&content_type, &body).unwrap_or_default();
            lock().uploads.insert(name.clone(), png);
            (200, "application/json", json!({"name": name, "subfolder": "", "type": "input"}).to_string().into_bytes())
        }
        ("POST", "/prompt") => {
            let v: Value = serde_json::from_slice(&body).unwrap_or_default();
            let graph = &v["prompt"];
            let id = uuid::Uuid::new_v4().to_string();
            let result = result_for(graph, &lock().uploads);
            let wait = if slow.load(Ordering::Relaxed) { Duration::from_secs(10) } else { Duration::from_millis(50) };
            lock().jobs.insert(id.clone(), (Instant::now() + wait, result));
            (200, "application/json", json!({"prompt_id": id, "number": 1}).to_string().into_bytes())
        }
        ("GET", p) if p.starts_with("/history/") => {
            let id = &p["/history/".len()..];
            let done = lock().jobs.get(id).filter(|(at, png)| Instant::now() >= *at && png.is_some()).is_some();
            let v = if done {
                json!({ id: {"status": {"status_str": "success"}, "outputs": {"8": {"images": [{"filename": format!("{id}.png"), "subfolder": "", "type": "temp"}]}}} })
            } else {
                json!({})
            };
            (200, "application/json", v.to_string().into_bytes())
        }
        ("GET", "/view") => {
            let file = query.split('&').find_map(|kv| kv.strip_prefix("filename=")).unwrap_or("");
            let id = file.trim_end_matches(".png");
            match lock().jobs.get(id).and_then(|(_, png)| png.clone()) {
                Some(png) => (200, "image/png", png),
                None => (404, "text/plain", b"no such file".to_vec()),
            }
        }
        ("POST", "/interrupt") => {
            // The running job never finishes.
            for (_, png) in lock().jobs.values_mut() {
                *png = None;
            }
            (200, "application/json", b"{}".to_vec())
        }
        ("GET", "/api/ps") => {
            let models: Vec<Value> = lock().ollama.iter().map(|m| json!({"name": m})).collect();
            (200, "application/json", json!({ "models": models }).to_string().into_bytes())
        }
        ("POST", "/api/generate") => {
            let v: Value = serde_json::from_slice(&body).unwrap_or_default();
            if v["keep_alive"] == 0 {
                let model = v["model"].as_str().unwrap_or("").to_owned();
                let mut st = lock();
                st.requests.push(format!("POST /api/generate keep_alive=0 {model}"));
                if st.ollama.as_deref() == Some(model.as_str()) {
                    st.ollama = None;
                    vram.store(10_000, Ordering::Relaxed);
                }
            }
            (200, "application/json", b"{}".to_vec())
        }
        ("POST", "/free" | "/queue") => (200, "application/json", b"{}".to_vec()),
        _ => (404, "text/plain", b"not here".to_vec()),
    };
    let mut stream = stream;
    write!(stream, "HTTP/1.1 {status} X\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", out.len())?;
    stream.write_all(&out)?;
    stream.flush()
}

/// The image a stand-in job makes (see the module docs).
fn result_for(graph: &Value, uploads: &HashMap<String, Vec<u8>>) -> Option<Vec<u8>> {
    let img = match graph["100"]["inputs"]["image"].as_str() {
        Some(name) => {
            let mut img = image::load_from_memory(uploads.get(name)?).ok()?.to_rgba8();
            for p in img.pixels_mut() {
                if p.0[..3] == super::workflow::FILL_GREY {
                    p.0 = [40, 90, 220, 255];
                }
            }
            img
        }
        None => {
            let w = graph["5"]["inputs"]["width"].as_u64().unwrap_or(512) as u32;
            let h = graph["5"]["inputs"]["height"].as_u64().unwrap_or(512) as u32;
            image::RgbaImage::from_pixel(w, h, image::Rgba([220, 40, 40, 255]))
        }
    };
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
    Some(png)
}

/// The `image` part of a multipart upload: its file name and bytes.
fn multipart_image(content_type: &str, body: &[u8]) -> Option<(String, Vec<u8>)> {
    let boundary = format!("--{}", content_type.split("boundary=").nth(1)?.trim());
    let start = find(body, b"name=\"image\"")?;
    let name_at = start + find(&body[start..], b"filename=\"")? + 10;
    let name_end = name_at + find(&body[name_at..], b"\"")?;
    let name = String::from_utf8_lossy(&body[name_at..name_end]).into_owned();
    let data = name_end + find(&body[name_end..], b"\r\n\r\n")? + 4;
    let end = data + find(&body[data..], format!("\r\n{boundary}").as_bytes())?;
    Some((name, body[data..end].to_vec()))
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}
