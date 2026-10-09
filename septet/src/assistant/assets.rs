//! Tools that work on files without an app: rendering SVG for Claude to look at, and fetching icons
//! and fonts from a few trusted sources. Both run on a thread of their own.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use serde_json::{Value, json};

use super::mcp::ToolReply;

/// Where `septet_fetch` may download from (host, path prefix). Free icon and font sources only.
const SOURCES: &[(&str, &str)] = &[
    ("api.iconify.design", "/"),
    ("fonts.googleapis.com", "/"),
    ("fonts.gstatic.com", "/"),
    ("raw.githubusercontent.com", "/google/fonts/"),
    ("github.com", "/google/fonts/raw/"),
];

const MAX_DOWNLOAD: u64 = 25 << 20;

/// A path inside `workspace` (relative paths are taken from it). Must not escape it.
pub fn in_workspace(workspace: &Path, p: &str) -> Result<PathBuf, String> {
    let path = if Path::new(p).is_absolute() { PathBuf::from(p) } else { workspace.join(p) };
    let root = workspace.canonicalize().map_err(|e| e.to_string())?;
    // The file may not exist yet: check its folder.
    let parent = path.parent().ok_or("Not a file path.")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("{p}: {e}"))?;
    let real = parent.canonicalize().map_err(|e| format!("{p}: {e}"))?.join(path.file_name().ok_or("Not a file path.")?);
    if real.starts_with(&root) { Ok(real) } else { Err(format!("{p} is outside the workspace {}.", workspace.display())) }
}

/// `septet_render`: an SVG as PNG, returned as an image (and saved when `out` is given).
pub fn render(workspace: &Path, args: &Value) -> Result<ToolReply, String> {
    let src = in_workspace(workspace, args["path"].as_str().ok_or("`path` is required.")?)?;
    let max = args["size"].as_u64().unwrap_or(1024).clamp(64, 2048) as f32;
    let svg = std::fs::read(&src).map_err(|e| format!("{}: {e}", src.display()))?;
    let mut opt = resvg::usvg::Options { resources_dir: src.parent().map(Path::to_path_buf), ..Default::default() };
    opt.fontdb_mut().load_system_fonts();
    // Fonts Claude downloaded into the workspace.
    opt.fontdb_mut().load_fonts_dir(workspace);
    let tree = resvg::usvg::Tree::from_data(&svg, &opt).map_err(|e| format!("Not a valid SVG: {e}"))?;
    let size = tree.size();
    let scale = max / size.width().max(size.height());
    let (w, h) = ((size.width() * scale).ceil() as u32, (size.height() * scale).ceil() as u32);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1)).ok_or("The image is too large.")?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    let png = pixmap.encode_png().map_err(|e| e.to_string())?;
    let mut note = format!("{} × {} px (SVG size {} × {}).", w, h, size.width(), size.height());
    if let Some(out) = args["out"].as_str() {
        let out = in_workspace(workspace, out)?;
        std::fs::write(&out, &png).map_err(|e| e.to_string())?;
        note.push_str(&format!(" Saved {}.", out.display()));
    }
    Ok(ToolReply {
        content: vec![
            json!({"type": "image", "data": base64::engine::general_purpose::STANDARD.encode(&png), "mimeType": "image/png"}),
            json!({"type": "text", "text": note}),
        ],
        is_error: false,
    })
}

fn allowed(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let (host, path) = rest.split_once('/').map(|(h, p)| (h, format!("/{p}"))).unwrap_or((rest, "/".into()));
    SOURCES.iter().any(|(h, prefix)| host.eq_ignore_ascii_case(h) && path.starts_with(prefix))
}

fn agent() -> Result<ureq::Agent, String> {
    let certs: Vec<ureq::tls::Certificate<'static>> =
        rustls_native_certs::load_native_certs().certs.iter().map(|c| ureq::tls::Certificate::from_der(c.as_ref()).to_owned()).collect();
    if certs.is_empty() {
        return Err("No trusted certificates found on this system.".into());
    }
    Ok(ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
        // Redirects could leave the allowed sources; only GitHub's raw links redirect, to raw.githubusercontent.com.
        .max_redirects(0)
        .tls_config(ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::new_with_certs(&certs)).build())
        .build()
        .new_agent())
}

/// `septet_fetch`: download a file from an allowed source into the workspace.
pub fn fetch(workspace: &Path, args: &Value) -> Result<ToolReply, String> {
    let mut url = args["url"].as_str().ok_or("`url` is required.")?.to_owned();
    let dest = in_workspace(workspace, args["path"].as_str().ok_or("`path` is required.")?)?;
    // github.com/google/fonts/raw/<ref>/<path> is raw.githubusercontent.com/google/fonts/<ref>/<path>.
    if let Some(rest) = url.strip_prefix("https://github.com/google/fonts/raw/") {
        url = format!("https://raw.githubusercontent.com/google/fonts/{rest}");
    }
    if !allowed(&url) {
        let list = SOURCES.iter().map(|(h, p)| format!("https://{h}{p}")).collect::<Vec<_>>().join(", ");
        return Err(format!("Septet only downloads from {list}. Use WebFetch for other pages."));
    }
    let agent = agent()?;
    let mut resp = agent
        .get(&url)
        // Google Fonts serves TTF links to browsers it does not recognise; WOFF2 to modern ones.
        .header("User-Agent", concat!("Septet/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|e| format!("Download failed: {e}"))?;
    let mut bytes = Vec::new();
    resp.body_mut().as_reader().take(MAX_DOWNLOAD + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_DOWNLOAD {
        return Err("The file is larger than 25 MB.".into());
    }
    std::fs::write(&dest, &bytes).map_err(|e| format!("{}: {e}", dest.display()))?;
    Ok(ToolReply::text(format!("Saved {} ({} bytes).", dest.display(), bytes.len())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources() {
        assert!(allowed("https://api.iconify.design/mdi/home.svg"));
        assert!(allowed("https://raw.githubusercontent.com/google/fonts/main/ofl/inter/OFL.txt"));
        assert!(!allowed("https://raw.githubusercontent.com/evil/repo/main/x"));
        assert!(!allowed("http://api.iconify.design/mdi/home.svg"));
        assert!(!allowed("https://api.iconify.design.evil.com/x"));
        assert!(!allowed("https://example.com/"));
    }

    #[test]
    fn workspace_paths() {
        let dir = std::env::temp_dir().join(format!("septet-ws-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(in_workspace(&dir, "icons/a.svg").is_ok());
        assert!(in_workspace(&dir, "../escape.svg").is_err());
        assert!(in_workspace(&dir, "/etc/passwd").is_err());
        let svg = dir.join("t.svg");
        std::fs::write(&svg, r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="20"><rect width="10" height="20" fill="red"/></svg>"#).unwrap();
        let r = render(&dir, &json!({"path": "t.svg", "size": 100, "out": "t.png"})).unwrap();
        assert_eq!(r.content[0]["type"], "image");
        assert!(dir.join("t.png").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
