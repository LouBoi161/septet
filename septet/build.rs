//! The Windows executable's icon and version information, and the assistant's bundled plugin.

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=assets/septet.ico");
    bundle_plugin();
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/septet.ico");
        res.set("ProductName", "Septet");
        res.set("FileDescription", "Septet");
        res.set("LegalCopyright", "PolyForm Noncommercial 1.0.0; bundled apps MIT OR Apache-2.0");
        if let Err(e) = res.compile() {
            println!("cargo:warning=no Windows resources: {e}");
        }
    }
}

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// `assistant-plugin/` as a list of `(relative path, include_bytes!)` plus a content hash
/// (`assistant/plugin.rs` unpacks it for Claude Code's `--plugin-dir`).
fn bundle_plugin() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assistant-plugin");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut list = Vec::new();
    files(&root, &mut list);
    list.sort();
    // FNV-1a over paths and contents: stable across builds and compilers.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    };
    let mut code = String::from("pub const FILES: &[(&str, &[u8])] = &[\n");
    for p in &list {
        let rel = p.strip_prefix(&root).expect("under root").to_string_lossy().replace('\\', "/");
        feed(rel.as_bytes());
        feed(&std::fs::read(p).expect("readable plugin file"));
        code.push_str(&format!("    ({rel:?}, include_bytes!({:?})),\n", p.display().to_string()));
    }
    code.push_str("];\n");
    code.push_str(&format!("pub const HASH: &str = \"{hash:016x}\";\n"));
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR")).join("assistant_plugin.rs");
    std::fs::write(out, code).expect("write assistant_plugin.rs");
}
