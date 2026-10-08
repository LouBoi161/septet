# Septet embedding contract — phase 3: content moving between apps

Same rules as phase 1 (your app dir only, no commits, match style/lints, build from the workspace root with
`cargo build -p <app> --lib` and `--bins`, report at the end). Add these methods to `Embedded`
(apps/<app>/src/embed.rs). The host (septet/) already has the tab strip, spring-loaded tabs and drop
handling; it calls these.

```rust
impl Embedded {
    /// While the user drags CONTENT out of the app with the mouse — whatever the app already lets you drag
    /// and that makes sense in another app: a layer (Layers panel / Move tool), selected art dragged off the
    /// canvas, photos from the grid/filmstrip, project-panel items (footage/clips), page thumbnails … —
    /// return a short label for the drag ghost, e.g. "Layer “Sky”", "3 photos", "Page 4". None when no such
    /// drag is in progress. Called every frame while the app's tab is visible and the primary button is down.
    /// Must be cheap (no rendering here).
    pub fn outgoing_drag(&self) -> Option<String>;

    /// The content of the current outgoing drag as files. `accept` lists the target's placeable formats as
    /// lowercase extensions, best first (e.g. ["psd","tif","png"] for Photocraft, ["svg","pdf","png"] for
    /// Vectorcraft). Produce the first one you can (original source files are best when the target accepts
    /// their extension — e.g. footage, raw photos' DEVELOPED render rather than the raw, etc.); write
    /// generated files into `dir` (exists, persistent) with readable names ("Sky.png"). Called once, when
    /// the drag is dropped on another app. May take up to ~1 s; render at full/sensible resolution.
    pub fn take_outgoing_files(&mut self, accept: &[&str], dir: &std::path::Path) -> Vec<std::path::PathBuf>;

    /// The drag ended in another app (which took it): forget it WITHOUT acting on the release (the app was
    /// not on screen when the button went up, so make sure its drag state, ghost and any "drop into album /
    /// reorder" logic are reset and nothing happens when its tab shows again).
    pub fn cancel_outgoing_drag(&mut self);

    /// "Send to <other app>": the active document / selected photo / current page / current frame as ONE
    /// file, same `accept`/`dir` rules. None if there is nothing to send.
    pub fn export_active(&mut self, accept: &[&str], dir: &std::path::Path) -> Option<std::path::PathBuf>;
}
```

Notes:
- Prefer reusing the app's own export/encode/render functions (see the phase-1 analysis in your context).
- outgoing_drag must only report drags that started in the app's own panels/canvas (not text selection
  drags, not window/tab dragging, not slider scrubbing).
- If your app has nothing meaningful to drag out, implement outgoing_drag as None and the rest accordingly,
  but export_active should still work.

## Also in phase 3: links to the sibling apps

Some apps show buttons/links to the other ArtCraft apps (e.g. Effectcraft's Home "More ArtCraft apps":
PhotoCraft, VectorCraft, FilmCraft, LightCraft, PdfCraft, DesignCraft), which open web pages. When hosted,
those should switch to the sibling app's tab instead. Add:

```rust
    /// When hosted, links/buttons that point at a sibling ArtCraft app call `handler(name)` instead of opening
    /// a web page; `name` is the lowercase app name ("photocraft", "vectorcraft", "lightcraft", "designcraft",
    /// "pdfcraft", "filmcraft", "effectcraft"). If the app has no such links, store the handler and do nothing.
    pub fn set_open_app(&mut self, handler: Box<dyn FnMut(&str)>);
```
Only route links that clearly name a sibling app (its product page / button); keep Discord, GitHub, docs and the
app's OWN homepage links as they are.

## Test status from the host (FYI)
The host has run all seven apps embedded end to end (screenshots verified): opening files by type, tabs,
torn-off windows, the close-request round trip (e.g. Pdfcraft's own "Save changes?" prompt on tab close), and
the clipboard bridge (Photocraft copy → Pdfcraft paste placed the image via place_paths).
