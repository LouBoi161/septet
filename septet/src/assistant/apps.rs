//! The `app_*` tools: Claude drives an app through its command registry and looks at its document
//! through off-screen renders (`docs/embedding/phase5-agent-control.md`). The apps answer in-process
//! through the agent methods of [`crate::hosted::HostedApp`].

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError, channel};
use std::time::{Duration, Instant};

use base64::Engine;
use egui::{Color32, Context};
use serde_json::{Value, json};

use super::mcp::{ToolCall, ToolReply};
use super::{Approval, approval_key};
use crate::hosted::AgentRender;
use crate::kinds::AppKind;
use crate::shell::{Action, Shell, TabKind};

/// Longest side of an image for Claude (larger ones are scaled down by the API anyway).
const MAX_SIDE: u64 = 1568;
/// Longer text answers are cut (Claude Code's default limit is 25 000 tokens a tool result).
const MAX_TEXT: usize = 40_000;
/// Without a filter, longer command lists are summed up instead of listed.
const MAX_UNFILTERED: usize = 150;
/// How long a command may take to answer when it waits for the app's frames.
const DEFERRED_TIMEOUT: Duration = Duration::from_secs(120);
/// How long an app may take to start.
const START_TIMEOUT: Duration = Duration::from_secs(30);

/// Commands Claude may never run here: the host owns the app's lifetime and its window.
fn blocked(command: &str) -> bool {
    matches!(command, "app.quit" | "app.exit" | "file.quit" | "file.exit") || command.starts_with("ui.")
}

/// Parameters that name files.
const PATH_KEYS: &[&str] = &["path", "paths", "file", "files", "folder", "dir", "directory", "out", "output", "dest", "destination"];

/// A tool call that has to wait: for its app to start, or for the user's answer to an approval.
pub struct Waiting {
    call: ToolCall,
    kind: AppKind,
    until: Instant,
    approval: Option<Receiver<ToolReply>>,
}

pub fn is_app_tool(name: &str) -> bool {
    name.starts_with("app_")
}

pub fn definitions(apps: &[String]) -> Vec<Value> {
    let app = json!({"type": "string", "enum": apps});
    let id = json!({"type": ["integer", "string"], "description": "An id from app_inspect (Vectorcraft's objects and Photocraft's layers are numbers)."});
    vec![
        json!({
            "name": "app_commands",
            "description": "List an app's commands: the ones its menus, shortcuts and command palette run, with their parameters and whether they can run now. Lists are long (hundreds of commands): pass `filter`, words that must all appear in the id, label or menu (e.g. \"rectangle\", \"align\", \"layer\"). Starts the app if it is not running.",
            "inputSchema": {"type": "object", "properties": {
                "app": app,
                "filter": {"type": "string"},
                "enabled_only": {"type": "boolean", "description": "Only commands that can run now."},
            }, "required": ["app"]},
            "annotations": {"readOnlyHint": true},
        }),
        json!({
            "name": "app_execute",
            "description": "Run one of an app's commands (ids from app_commands) the way its menu item does: it changes the open document, the user sees it happen, and app_undo takes it back. Give the parameters the command lists; a command that would need a dialog fails without them. Paths are relative to the workspace. Saving or exporting outside the workspace, and changing the app's settings, asks the user first. Check the result with app_render.",
            "inputSchema": {"type": "object", "properties": {
                "app": app,
                "command": {"type": "string"},
                "params": {"type": "object"},
            }, "required": ["app", "command"]},
        }),
        json!({
            "name": "app_inspect",
            "description": "Read an app's document as JSON. `what`: `document` (summary and object or layer tree with ids; `depth` levels), `selection`, `object` or `layer` (one item by `id`), `history`, `documents` (all open ones); Vectorcraft also has `find` (params name/kind/text). An unknown `what` lists the app's views.",
            "inputSchema": {"type": "object", "properties": {
                "app": app,
                "what": {"type": "string"},
                "id": id,
                "depth": {"type": "integer", "minimum": 0},
                "params": {"type": "object", "description": "More of the view's options (e.g. {\"name\": \"Logo\"} for find)."},
            }, "required": ["app", "what"]},
            "annotations": {"readOnlyHint": true},
        }),
        json!({
            "name": "app_render",
            "description": "Look at an app's document as a picture, made off-screen (not a screenshot): `target` `document` (the artboard or page in view, or `page`, counted from 1), `object` or `layer` (one by `id`, cut out on its own), or `selection`. Use it after changes to check them. The picture is shown over `background` (auto: white for documents, a checkerboard for parts; or white, black, checker, transparent, #rrggbb). `save_as` also writes it as a transparent PNG into the workspace. Works while the app's tab is hidden.",
            "inputSchema": {"type": "object", "properties": {
                "app": app,
                "target": {"type": "string"},
                "id": id,
                "page": {"type": "integer", "minimum": 1},
                "time": {"type": "number", "description": "Seconds, for video and animation."},
                "max_side": {"type": "integer", "minimum": 64, "maximum": MAX_SIDE, "description": "Longest side in pixels (default 1024)."},
                "background": {"type": "string"},
                "save_as": {"type": "string"},
            }, "required": ["app"]},
            "annotations": {"readOnlyHint": true},
        }),
        json!({
            "name": "app_undo",
            "description": "Undo the last change in an app's document (Edit › Undo), or redo it with `redo: true`. `steps` repeats it.",
            "inputSchema": {"type": "object", "properties": {
                "app": app,
                "steps": {"type": "integer", "minimum": 1, "maximum": 50},
                "redo": {"type": "boolean"},
            }, "required": ["app"]},
        }),
    ]
}

/// Calls still waiting: their app runs now, the user answered, or time is up.
pub fn retry(shell: &mut Shell, ctx: &Context) {
    let now = Instant::now();
    let mut keep = Vec::new();
    for w in std::mem::take(&mut shell.assistant.waiting) {
        match &w.approval {
            Some(answer) => match answer.try_recv() {
                Ok(reply) if allowed(&reply) => call(shell, ctx, w.call, true),
                Ok(_) | Err(TryRecvError::Disconnected) => {
                    let _ = w.call.reply.send(ToolReply::error("The user declined this."));
                }
                Err(TryRecvError::Empty) => keep.push(w),
            },
            None if shell.apps.contains_key(&w.kind) => call(shell, ctx, w.call, false),
            None if now > w.until => {
                let _ = w.call.reply.send(ToolReply::error(format!("{} did not start.", w.kind.name())));
            }
            None => {
                ctx.request_repaint_after(Duration::from_millis(50));
                keep.push(w);
            }
        }
    }
    shell.assistant.waiting.extend(keep);
}

/// An `app_*` tool call. `approved`: the user already said yes to what it does.
pub fn call(shell: &mut Shell, ctx: &Context, call: ToolCall, approved: bool) {
    let kind = match super::tools::app_from(&call.args["app"]) {
        Ok(Some(kind)) => kind,
        Ok(None) => return reply(&call.reply, Err("`app` is required.".into())),
        Err(e) => return reply(&call.reply, Err(e)),
    };
    let Some(slot) = shell.apps.get(&kind).cloned() else {
        // Start the app; the call goes on once it runs.
        show(shell, ctx, kind);
        shell.assistant.waiting.push(Waiting { call, kind, until: Instant::now() + START_TIMEOUT, approval: None });
        return;
    };
    let Ok(mut slot) = slot.try_borrow_mut() else {
        shell.assistant.waiting.push(Waiting { call, kind, until: Instant::now() + START_TIMEOUT, approval: None });
        return;
    };
    let args = &call.args;
    let name = kind.name();
    let unsupported = || Err(format!("{name} can't be driven by Claude yet; use septet_open and septet_place."));
    match call.name.as_str() {
        "app_commands" => {
            let r = match slot.with_app(ctx, |a| a.agent_commands(ctx)) {
                Some(list) => {
                    Ok(ToolReply::text(commands(name, list, args["filter"].as_str().unwrap_or_default(), args["enabled_only"].as_bool().unwrap_or(false))))
                }
                None => unsupported(),
            };
            reply(&call.reply, r);
        }
        "app_inspect" => {
            let what = args["what"].as_str().unwrap_or("document");
            let mut params = args.get("params").filter(|p| p.is_object()).cloned().unwrap_or_else(|| json!({}));
            for key in ["id", "depth"] {
                if let Some(v) = args.get(key).filter(|v| !v.is_null()) {
                    params[key] = v.clone();
                }
            }
            let r = match slot.with_app(ctx, |a| a.agent_inspect(ctx, what, &params)) {
                Some(r) => r.map(|v| ToolReply::text(text_of(&v))),
                None => unsupported(),
            };
            reply(&call.reply, r);
        }
        "app_render" => {
            let max_side = args["max_side"].as_u64().unwrap_or(1024).clamp(64, MAX_SIDE);
            let mut target = args.clone();
            target["target"] = json!(args["target"].as_str().unwrap_or("document"));
            target["max_side"] = json!(max_side);
            let r = slot.with_app(ctx, |a| a.agent_render(ctx, &target));
            let job = match r {
                Some(Ok(job)) => job,
                Some(Err(e)) => return reply(&call.reply, Err(e)),
                None => return reply(&call.reply, unsupported()),
            };
            let workspace = shell.assistant.conversation.as_ref().map(|c| c.workspace.clone());
            let (args, out) = (call.args.clone(), call.reply.clone());
            std::thread::spawn(move || {
                let _ = out.send(render(job, &args, max_side as u32, workspace.as_deref()).unwrap_or_else(ToolReply::error));
            });
        }
        "app_execute" | "app_undo" => {
            let undo = call.name == "app_undo";
            let command = if undo {
                if args["redo"].as_bool().unwrap_or(false) { "edit.redo" } else { "edit.undo" }
            } else {
                match args["command"].as_str() {
                    Some(c) => c,
                    None => return reply(&call.reply, Err("`command` is required.".into())),
                }
            }
            .to_owned();
            let command = command.as_str();
            if blocked(command) {
                return reply(&call.reply, Err(format!("`{command}` is not available in Septet.")));
            }
            let mut params = args.get("params").filter(|p| p.is_object()).cloned().unwrap_or_else(|| json!({}));
            let workspace = shell.assistant.conversation.as_ref().map(|c| c.workspace.clone());
            if !undo {
                let outside = match resolve_paths(&mut params, workspace.as_deref(), &shell.recent) {
                    Ok(outside) => outside,
                    Err(e) => return reply(&call.reply, Err(e)),
                };
                if !approved && let Some(question) = needs_approval(command, &params, &outside) {
                    drop(slot);
                    let command = command.to_owned();
                    return ask(shell, ctx, call, kind, &command, question, outside);
                }
            }
            // Changes happen where the user sees them.
            if !slot.visible {
                show(shell, ctx, kind);
            }
            let steps = if undo { args["steps"].as_u64().unwrap_or(1).clamp(1, 50) } else { 1 };
            let mut done = Vec::new();
            for _ in 0..steps {
                let Some(rx) = slot.with_app(ctx, |a| a.agent_execute(ctx, command, params.clone())) else {
                    return reply(&call.reply, unsupported());
                };
                match rx.try_recv() {
                    Ok(v) => match envelope(v) {
                        Ok(v) => done.push(v),
                        Err(e) if done.is_empty() => return reply(&call.reply, Err(e)),
                        Err(e) => {
                            done.push(json!(format!("stopped: {e}")));
                            break;
                        }
                    },
                    // The app answers from a later frame of its own (it is on screen now).
                    Err(TryRecvError::Empty) if !undo => return later(rx, call.reply),
                    Err(_) => return reply(&call.reply, Err(format!("{name} did not answer."))),
                }
            }
            let text = match (undo, &done[..]) {
                (false, [v]) if v.is_null() => "Done.".to_owned(),
                (false, [v]) => text_of(v),
                (true, _) => {
                    // Say what was undone when the app tells (Vectorcraft names the step; others just say yes).
                    let named: Vec<&Value> = done.iter().filter(|v| !v.is_boolean() && !v.is_null()).collect();
                    let verb = if command == "edit.redo" { "Redid" } else { "Undid" };
                    let steps = if done.len() == 1 { "1 step".to_owned() } else { format!("{} steps", done.len()) };
                    if named.is_empty() { format!("{verb} {steps}.") } else { format!("{verb} {steps}: {}", text_of(&json!(named))) }
                }
                _ => text_of(&json!(done)),
            };
            reply(&call.reply, Ok(ToolReply::text(text)));
        }
        other => reply(&call.reply, Err(format!("Septet has no tool {other:?}."))),
    }
}

fn reply(to: &Sender<ToolReply>, r: Result<ToolReply, String>) {
    let _ = to.send(r.unwrap_or_else(ToolReply::error));
}

/// Bring the app's tab to the front, starting the app if it is not running.
pub fn show(shell: &mut Shell, ctx: &Context, kind: AppKind) {
    match shell.find_app_tab(kind) {
        Some((wi, ti)) => {
            let tab = shell.windows[wi].tabs[ti].id;
            shell.actions.push(Action::Activate { tab });
        }
        None => {
            let window = shell.focused_window(ctx);
            shell.actions.push(Action::NewTab { window, kind: TabKind::App(kind) });
        }
    }
    ctx.request_repaint();
}

/// The control protocol's `{"ok", "result" | "error"}`.
fn envelope(v: Value) -> Result<Value, String> {
    if v["ok"].as_bool() == Some(true) {
        Ok(v.get("result").cloned().unwrap_or(Value::Null))
    } else {
        Err(v["error"].as_str().map_or_else(|| v.to_string(), str::to_owned))
    }
}

/// Answer once the app replies from one of its frames.
fn later(rx: Receiver<Value>, to: Sender<ToolReply>) {
    std::thread::spawn(move || {
        let r = match rx.recv_timeout(DEFERRED_TIMEOUT) {
            Ok(v) => envelope(v).map(|v| ToolReply::text(if v.is_null() { "Done.".to_owned() } else { text_of(&v) })),
            Err(RecvTimeoutError::Timeout) => Err("The app did not answer within two minutes; check its state with app_inspect.".into()),
            Err(RecvTimeoutError::Disconnected) => Err("The app dropped the request.".into()),
        };
        reply(&to, r);
    });
}

/// JSON for Claude: compact, and cut when it is too long.
fn text_of(v: &Value) -> String {
    let s = match v {
        Value::String(s) => s.clone(),
        v => v.to_string(),
    };
    cut(s)
}

fn cut(mut s: String) -> String {
    if s.len() <= MAX_TEXT {
        return s;
    }
    let total = s.len();
    let mut end = MAX_TEXT;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
    s.push_str(&format!("… [cut: {} of {total} characters shown; ask for less, e.g. a smaller depth, a filter or one object]", end));
    s
}

/// `app_commands`: the registry, filtered; one compact JSON object a line.
fn commands(app: &str, list: Vec<Value>, filter: &str, enabled_only: bool) -> String {
    let words: Vec<String> = filter.split_whitespace().map(str::to_lowercase).collect();
    let hit = |c: &&Value| {
        let hay = ["id", "label", "menu"].iter().filter_map(|k| c[*k].as_str()).collect::<Vec<_>>().join(" ").to_lowercase();
        words.iter().all(|w| hay.contains(w.as_str())) && (!enabled_only || c["enabled"].as_bool() != Some(false))
    };
    let mut found: Vec<Value> = list.iter().filter(|c| hit(c)).cloned().collect();
    if words.is_empty() && found.len() > MAX_UNFILTERED {
        let mut groups: Vec<(String, usize)> = Vec::new();
        for c in &found {
            let g = c["id"].as_str().unwrap_or_default().split('.').next().unwrap_or_default().to_owned();
            match groups.iter_mut().find(|(name, _)| *name == g) {
                Some((_, n)) => *n += 1,
                None => groups.push((g, 1)),
            }
        }
        let groups: Vec<String> = groups.into_iter().map(|(g, n)| format!("{g} ({n})")).collect();
        return format!("{app} has {} commands; pass `filter` to list them. Groups by id prefix: {}.", found.len(), groups.join(", "));
    }
    if found.is_empty() {
        return format!("No {app} command matches “{filter}” (of {}). Try fewer or other words.", list.len());
    }
    for c in &mut found {
        // Most commands can run: only say when one can't.
        if c["enabled"].as_bool() == Some(true)
            && let Some(o) = c.as_object_mut()
        {
            o.remove("enabled");
        }
    }
    let lines: Vec<String> = found.iter().map(Value::to_string).collect();
    cut(format!("{} of {} {app} commands:\n{}", found.len(), list.len(), lines.join("\n")))
}

/// Make the path parameters absolute (relative ones are in the workspace) → the ones outside the
/// workspace that the user did not open in Septet.
fn resolve_paths(params: &mut Value, workspace: Option<&Path>, recent: &crate::recent::Recent) -> Result<Vec<PathBuf>, String> {
    let mut outside = Vec::new();
    let root = workspace.and_then(|w| w.canonicalize().ok());
    let Some(map) = params.as_object_mut() else { return Ok(outside) };
    for (key, value) in map.iter_mut() {
        if !PATH_KEYS.contains(&key.as_str()) {
            continue;
        }
        let items: Vec<&mut Value> = match value {
            Value::Array(a) => a.iter_mut().collect(),
            v => vec![v],
        };
        for item in items {
            let Some(p) = item.as_str() else { continue };
            let path = if Path::new(p).is_absolute() { PathBuf::from(p) } else { workspace.ok_or("Relative paths need a conversation's workspace.")?.join(p) };
            let real = real_path(&path);
            let inside = root.as_ref().is_some_and(|r| real.starts_with(r));
            let opened = recent.files.iter().any(|f| f.path.canonicalize().is_ok_and(|r| r == real));
            if !inside && !opened {
                outside.push(real.clone());
            }
            *item = json!(real.to_string_lossy());
        }
    }
    Ok(outside)
}

/// `path` with links and `..` resolved as far as it exists (the file itself may not yet).
fn real_path(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut base = path.to_path_buf();
    loop {
        if let Ok(real) = base.canonicalize() {
            return rest.into_iter().rev().fold(real, |p, part| p.join(part));
        }
        match (base.file_name().map(|f| f.to_os_string()), base.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                base = parent.to_path_buf();
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// What the user must agree to before `command` runs, if anything.
fn needs_approval(command: &str, params: &Value, outside: &[PathBuf]) -> Option<String> {
    let id = command.to_lowercase();
    let writes = id.contains("save") || id.contains("export");
    let has_path = PATH_KEYS.iter().any(|k| params.get(*k).is_some_and(|v| !v.is_null()));
    if id.starts_with("prefs.") || id.starts_with("shortcuts.") || id == "app.language" {
        Some("Let Claude change the app's settings?".into())
    } else if !outside.is_empty() {
        Some(if writes { "Let Claude write a file outside its workspace?" } else { "Let Claude use a file outside its workspace?" }.into())
    } else if writes && !has_path {
        Some("Let Claude save or export the document?".into())
    } else {
        None
    }
}

/// Put the question to the user in the chat panel; the call waits for the answer.
fn ask(shell: &mut Shell, ctx: &Context, call: ToolCall, kind: AppKind, command: &str, question: String, outside: Vec<PathBuf>) {
    let mut input = json!({"app": kind.name(), "command": command, "question": question, "params": call.args.get("params").cloned().unwrap_or(json!({}))});
    if let Some(p) = outside.first() {
        input["path"] = json!(p.to_string_lossy());
    }
    let Some(conv) = shell.assistant.conversation.as_mut() else {
        return reply(&call.reply, Err("This needs the user's approval, and no conversation is running.".into()));
    };
    if conv.allowed.contains(&approval_key("app_execute", &input)) {
        return self::call(shell, ctx, call, true);
    }
    let (tx, rx) = channel();
    conv.approvals.push(Approval { tool: "app_execute".into(), input, reply: tx });
    shell.assistant.waiting.push(Waiting { call, kind, until: Instant::now(), approval: Some(rx) });
}

/// The user's answer to an approval card.
fn allowed(r: &ToolReply) -> bool {
    r.content.first().and_then(|c| c["text"].as_str()).and_then(|t| serde_json::from_str::<Value>(t).ok()).is_some_and(|d| d["behavior"] == "allow")
}

/// Finish an `app_render` on a worker thread: shrink, save, put on the background, encode.
fn render((caption, job): (String, AgentRender), args: &Value, max_side: u32, workspace: Option<&Path>) -> Result<ToolReply, String> {
    let image = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).map_err(|_| "The app failed to render this.".to_owned())??;
    let [w, h] = image.size;
    if w == 0 || h == 0 {
        return Err("The picture is empty.".into());
    }
    let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    let mut img = image::RgbaImage::from_raw(w as u32, h as u32, rgba).ok_or("The app returned a broken picture.")?;
    if w.max(h) as u32 > max_side {
        let s = max_side as f32 / w.max(h) as f32;
        let (nw, nh) = (((w as f32 * s).round() as u32).max(1), ((h as f32 * s).round() as u32).max(1));
        img = image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle);
    }
    let mut note = format!("{caption}. Picture: {} × {} px.", img.width(), img.height());
    if let Some(out) = args["save_as"].as_str() {
        let workspace = workspace.ok_or("save_as needs a conversation's workspace.")?;
        let out = super::assets::in_workspace(workspace, out)?;
        img.save_with_format(&out, image::ImageFormat::Png).map_err(|e| format!("{}: {e}", out.display()))?;
        note.push_str(&format!(" Saved {}.", out.display()));
    }
    let target = args["target"].as_str().unwrap_or("document");
    let whole = matches!(target, "document" | "page" | "artboard" | "frame" | "comp" | "sequence" | "photo");
    let background = match args["background"].as_str().unwrap_or("auto") {
        "auto" if whole => Background::Solid(Color32::WHITE),
        "auto" | "checker" | "checkerboard" => Background::Checker,
        "transparent" | "none" => Background::None,
        other => {
            Background::Solid(color(other).ok_or_else(|| format!("Unknown background {other:?}: use auto, white, black, checker, transparent or #rrggbb."))?)
        }
    };
    composite(&mut img, background);
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(ToolReply {
        content: vec![
            json!({"type": "image", "data": base64::engine::general_purpose::STANDARD.encode(&png), "mimeType": "image/png"}),
            json!({"type": "text", "text": note}),
        ],
        is_error: false,
    })
}

#[derive(Clone, Copy)]
enum Background {
    None,
    Solid(Color32),
    Checker,
}

fn color(name: &str) -> Option<Color32> {
    match name {
        "white" => Some(Color32::WHITE),
        "black" => Some(Color32::BLACK),
        hex => {
            let hex = hex.strip_prefix('#')?;
            let v = u32::from_str_radix(hex, 16).ok().filter(|_| hex.len() == 6)?;
            Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
        }
    }
}

/// Put `img` over `bg` (a checkerboard shows what is transparent).
fn composite(img: &mut image::RgbaImage, bg: Background) {
    if matches!(bg, Background::None) {
        return;
    }
    for (x, y, px) in img.enumerate_pixels_mut() {
        let under = match bg {
            Background::Solid(c) => c,
            _ if (x / 12 + y / 12) % 2 == 0 => Color32::from_gray(255),
            _ => Color32::from_gray(214),
        };
        let a = px[3] as u32;
        let mix = |fg: u8, bg: u8| ((fg as u32 * a + bg as u32 * (255 - a) + 127) / 255) as u8;
        *px = image::Rgba([mix(px[0], under.r()), mix(px[1], under.g()), mix(px[2], under.b()), 255]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_command_lists_need_a_filter() {
        let list: Vec<Value> = (0..200).map(|i| json!({"id": format!("g{}.c{i}", i % 3), "label": format!("Command {i}"), "enabled": i % 2 == 0})).collect();
        let all = commands("Vectorcraft", list.clone(), "", false);
        assert!(all.starts_with("Vectorcraft has 200 commands"), "{all}");
        assert!(all.contains("g0 (67)"), "{all}");
        let some = commands("Vectorcraft", list.clone(), "g1 command", true);
        let lines: Vec<&str> = some.lines().skip(1).collect();
        assert!(!lines.is_empty() && lines.iter().all(|l| l.contains("\"g1.") && !l.contains("enabled")), "{some}");
        assert!(commands("Vectorcraft", list, "nothing-like-this", false).starts_with("No Vectorcraft command"));
    }

    #[test]
    fn writing_and_settings_need_the_user() {
        let none: Vec<PathBuf> = vec![];
        assert!(needs_approval("shape.rectangle", &json!({"x": 1}), &none).is_none());
        assert!(needs_approval("file.save", &json!({}), &none).is_some());
        assert!(needs_approval("file.saveAs", &json!({"path": "/ws/a.svg"}), &none).is_none());
        assert!(needs_approval("document.export", &json!({"path": "/x.png"}), &[PathBuf::from("/x.png")]).is_some());
        assert!(needs_approval("prefs.set", &json!({}), &none).is_some());
        assert!(blocked("app.quit") && blocked("ui.screenshot") && !blocked("edit.undo"));
    }

    #[test]
    fn paths_resolve_against_the_workspace() {
        let ws = std::env::temp_dir().join(format!("septet-apps-test-{}", std::process::id()));
        std::fs::create_dir_all(&ws).unwrap();
        let recent = crate::recent::Recent::default();
        let mut p = json!({"path": "out/logo.svg", "paths": ["a.png", "/etc/hostname"], "width": 3});
        let outside = resolve_paths(&mut p, Some(&ws), &recent).unwrap();
        let real = ws.canonicalize().unwrap();
        assert_eq!(p["path"], json!(real.join("out/logo.svg").to_string_lossy()));
        assert_eq!(p["paths"][0], json!(real.join("a.png").to_string_lossy()));
        assert_eq!(outside.len(), 1, "{outside:?}");
        assert!(resolve_paths(&mut json!({"path": "../escape.svg"}), Some(&ws), &recent).unwrap().len() == 1);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn long_answers_are_cut() {
        let s = cut("é".repeat(MAX_TEXT));
        assert!(s.len() < MAX_TEXT + 200 && s.contains("[cut:"));
        assert_eq!(cut("short".into()), "short");
    }
}
