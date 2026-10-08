# Embedding contract — phase 4: release readiness (portable mode, brand, platforms)

The host suite is about to be published (public GitLab + GitHub) with downloads for Windows (installer +
PORTABLE zip), Linux and macOS, built on native CI runners. Same rules as before (your app dir only, no
commits, lints, build lib+bins from the workspace root, report at the end).

## 1. Portable data root
Add to `Embedded`:
```rust
    /// Before `new()`: keep EVERYTHING this app writes or reads as per-user state under `root` instead of
    /// the per-user OS folders — preferences/ui.json, recovery/autosave/journal folders, presets/libraries
    /// the app creates, caches, logs, downloaded models, eframe-storage fallbacks, the default library or
    /// project location (e.g. Lightcraft's default library becomes `<root>/Library`), etc. `root` is
    /// app-specific already (the host passes e.g. `<exe dir>/Data/Photocraft`), create it if missing.
    /// None = normal per-user folders (default).
    pub fn set_data_root(root: Option<std::path::PathBuf>);
```
Implement it as a process-wide setting in the UI crate's `hosted` module (e.g. `hosted::data_root()`), and
make every per-user directory lookup in the app (UI crate, engine crates, and the moved desktop/services
code) go through one helper that honours it. Grep thoroughly (`dirs::`, `directories::`, `ProjectDirs`,
`config_dir`, `data_dir`, `data_local_dir`, `cache_dir`, `home_dir`, `picture_dir`, `document_dir`,
`APPDATA`, `XDG_`, `storage_dir`, `temp_dir` used for persistent data). Leave genuine temp files in temp.
Standalone behaviour unchanged when no root is set. If the app already has a portable mode (Photocraft
does), reuse it.

## 2. ArtCraft brand marks must not appear in hosted builds
`docs/brand/LICENSE-brand.txt`: in a modified/derived build the ArtCraft Marks (the ArtCraft NAME, wordmark
and mark/logo) must be removed, and nothing may suggest the ArtCraft Team made/endorses it. Plain text
"based on <App> by the ArtCraft team" is allowed (keep that in the About/credits). So when
`hosted::is_hosted()`:
- don't draw any ArtCraft logo/wordmark/mark image (and don't embed/register them if avoidable);
- hide or neutralise UI that brands the app as an ArtCraft product or advertises ArtCraft (e.g. "Join the
  ArtCraft community/Discord" promos, "ArtCraft website" links, "More ArtCraft apps" HEADINGS — the
  sibling-app buttons may stay but without the ArtCraft name; Discord buttons that invite to ArtCraft's
  community should be hidden);
- keep the app's own name (PhotoCraft etc.), its own icon (app-icon is MIT/Apache) and links to its own
  source repository / license / credits;
- About boxes: add a plain-text line like "Based on <App> by the ArtCraft team (MIT OR Apache-2.0)".
Standalone builds unchanged.

## 3. Windows and macOS
The CI will build the host natively on Windows (MSVC) and macOS (arm64). Review every cfg(windows) /
cfg(target_os = "macos") / cfg(unix) path you added or moved in phases 1–3 (embed.rs, desktop/services
modules, hosted gates) so it compiles there: no Linux-only APIs on those paths, correct imports under each
cfg, no unused-import warnings per platform. If you can, run `cargo check --target x86_64-pc-windows-msvc
-p <pkg> --lib` (rustup target add first; it may fail in C build scripts — that's fine, report what you
could verify). Also: the app's `build.rs` (Windows resources via winresource) must NOT embed its icon /
version info into a host executable when the package is built as a dependency: skip resource embedding
when the env var `HOSTED_BUILD` is set (the host sets it for its CI builds); standalone unchanged.

## 2b. ADDENDUM — remove the ArtCraft Marks from the published tree (not only hide them)
The brand license says a published MODIFIED version must have the ArtCraft Marks removed (or replaced),
which applies to the source tree we publish, not just the hosted UI. So, in your repo:
- delete the image files in `docs/brand/` (artcraft-logo*, artcraft-mark*, and their `.attribution`
  side files), keep `docs/brand/LICENSE-brand.txt` and add a short `docs/brand/README.md`: "The ArtCraft
  marks were removed from this modified version, as LICENSE-brand.txt requires.";
- find every OTHER copy or embedding of an ArtCraft mark (e.g. `include_bytes!`/`include_str!` of an
  artcraft logo/mark SVG/PNG under assets/ or src/, hard-coded logo path data drawn in code, icons named
  artcraft*) and remove it, replacing the visual with the app's OWN app icon (assets/app-icon, MIT/Apache)
  or nothing — in standalone too, since the published tree must not contain the marks;
- user-visible text naming ArtCraft as the maker/brand (e.g. "Join the ArtCraft community", "More ArtCraft
  apps", "by ArtCraft") becomes neutral in ALL builds; a plain-text credit "Based on <App> by the ArtCraft
  team" in About/credits is allowed and should stay; README/NOTICE may keep factual attribution;
- update tests/snapshots/docs that referenced the removed files. Use `rg -i "artcraft"` over the repo
  (excluding target/ and .git/) to find everything; report what remains and why.
