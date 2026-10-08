//! The Windows executable's icon and version information.

fn main() {
    println!("cargo:rerun-if-changed=assets/septet.ico");
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
