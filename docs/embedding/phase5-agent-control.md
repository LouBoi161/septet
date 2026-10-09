# Embedding contract — phase 5: control and rendering for the agent

Septet's Claude assistant drives the apps through their command registries and looks at its work
through off-screen renders, never through window screenshots or synthetic clicks. Each app gets
four methods on `Embedded` that Septet's own MCP server (`septet/src/assistant/tools.rs`) calls
in-process. There is no extra server, socket or thread in the app (the phase 1 rule "NO
control/TCP/MCP servers" stands).

Same rules as before: change only your app's directory, follow its lints (no panics in shipped code),
and build lib + bins from the workspace root. Unlike the earlier phases, these changes are committed
in the Septet monorepo, one commit per app.

## API

```rust
/// A picture made off the UI thread: premultiplied RGBA (egui's `Color32` convention), transparent
/// where nothing is drawn.
pub type AgentRender = Box<dyn FnOnce() -> Result<egui::ColorImage, String> + Send>;

impl Embedded {
    /// The app's command registry: what its menus, shortcuts and command palette run. One JSON
    /// object per command:
    /// `{"id", "label", "menu"?: "Object › Arrange", "params"?: <text or JSON Schema>,
    ///   "enabled": bool, "disabled_reason"?: string, "shortcut"?: string}`.
    /// `params` is whatever the app's own registry documents (a line of text or a JSON Schema).
    pub fn agent_commands(&mut self, ctx: &egui::Context) -> Vec<serde_json::Value>;

    /// Run one command by id, through the same path its menu item takes, so it is journaled and
    /// undoable when the app's command is. The reply is the control protocol's envelope:
    /// `{"ok": true, "result": …}` or `{"ok": false, "error": "…"}`. It is in the receiver when the
    /// call returns, unless the command has to wait for the app's frames (queued input, background
    /// jobs): then the app sends it later from its `logic()`, and the host keeps the tab on screen
    /// until it arrives. `edit.undo` and `edit.redo` must work through this method.
    /// A command that would open a dialog and wait for the user (it was given no params) must not
    /// leave that dialog open: answer with an error that says which params it needs.
    pub fn agent_execute(&mut self, ctx: &egui::Context, command: &str, params: serde_json::Value)
        -> std::sync::mpsc::Receiver<serde_json::Value>;

    /// Structured state for the agent. `what` names a view of the document; use these names where
    /// they fit: "document" (summary and object tree; honours `depth`), "selection",
    /// "object"/"layer"/"clip"/"page" (one item by `params.id`), "history". App-specific views are
    /// fine ("sequence", "comp", "photo", "find"…). An unknown `what` is an error that lists the
    /// app's views. Keep answers compact: ids, names, kinds, bounds, the few properties that
    /// matter; trim long lists and say so (`"childCount": 812`).
    pub fn agent_inspect(&mut self, ctx: &egui::Context, what: &str, params: &serde_json::Value)
        -> Result<serde_json::Value, String>;

    /// A picture of the document or of one of its parts, made without the window. `target` is
    /// `{"target": "document" | "page" | "frame" | "layer" | "object" | "selection" | "clip" | …,
    ///   "id"?: <as agent_inspect reports it>, "page"?: 1-based, "time"?: seconds,
    ///   "max_side": pixels}` plus app-specific keys. On the UI thread, only find the target and
    /// snapshot what the render needs (cheap clones); the returned job does the work on a worker
    /// thread. The image fits in `max_side` × `max_side` (scale vector art to fill it; shrink
    /// rasters, never enlarge them more than 2×). Parts (layers, objects, clips) are cut out on
    /// a transparent background, cropped to their bounds. The `String` is a one-line caption
    /// ("Layer “Sky” (id 12), 1200 × 800 px").
    pub fn agent_render(&mut self, ctx: &egui::Context, target: &serde_json::Value)
        -> Result<(String, AgentRender), String>;
}
```

## When the host calls them

- From its own frame on the UI thread, with the app's isolated egui state swapped in (as for every
  call into the app), **whether or not the app's tab is visible**. The app may not have drawn a
  frame for a long time and must not need one to answer, except for the deferred replies of
  `agent_execute` described above.
- No panics: guard each call the way the app guards its control channel, and catch panics inside
  the render job too.
- Ids must stay valid at least as long as the app runs (Vectorcraft's `NodeId`, Designcraft's
  `ItemId`, …). Say in `agent_inspect` which id a render or a command expects.

## What the host does (for reference)

- Tools: `app_commands` (filtered list, required filter once the list is long), `app_execute`,
  `app_inspect`, `app_render`, `app_undo`. Every tool starts the app if needed. Changing tools bring
  the app's tab to the front so the user sees what happens. Queries and renders leave the tabs alone.
- `app_render` clamps `max_side` (64–1568, default 1024), composites the image over a background
  for Claude to look at (white for documents and pages, a checkerboard for parts, or what Claude
  asks for), encodes PNG and, on request, saves the transparent original in the workspace.
- Blocked in Septet: quitting the app and the control protocol's window methods (`app.quit`,
  `ui.*`). Commands that write files (`save` or `export` in the id, or a path-like parameter) run
  only for paths in Claude's workspace; anything else needs the user's approval in the chat panel.
- Answers longer than about 40 000 characters are cut, with a note to narrow the query.

## Per app (where to start)

| App | Commands | Inspect | Render |
|---|---|---|---|
| Vectorcraft | `app.run(id, params)`, registry `control::all_commands` | `document.inspect`, `document.node`, `document.find` | artboard: `Renderer::render_region`; objects/layers: `fileio::objects_document` → own document |
| Photocraft | `engine.execute` | `document.inspect` | `compose::thumbnail`, `render_layer` |
| Designcraft | `engine.execute` (`undoable` flag) | `document.inspect` (per page) | `Renderer::render_page`; items via `RenderOptions.hidden` |
| Effectcraft | `engine.execute` / `engine.batch`, JSON Schema params | `project.summary`, `layer.tree`, `prop.get` | `render_rgba8_alpha`; a layer alone via `Renderer::layer_buf` (never the Solo switch) |
| Filmcraft | `engine.execute` (needs the UI dispatcher) | `project.inspect`, `sequence.inspect` | `render_sequence`, `render_clip`, `render_item` |
| Lightcraft | `engine.execute` | `photo.inspect`, `develop.get`, `catalog.query` | `Session::render_now`; masks via `Overlay::Mask` |
| Pdfcraft | registry ids without params; no `attach_control` (it hooks the shared context) | `ui.state`, `DocInfo` | `PageRenderer::render` on the session's bytes |
