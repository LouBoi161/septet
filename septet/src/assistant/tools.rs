//! The tools Septet gives Claude, and how the UI thread carries them out.

use std::path::{Path, PathBuf};

use egui::Context;
use serde_json::{Value, json};

use super::mcp::{ToolCall, ToolReply};
use super::{Approval, approval_key};
use crate::kinds::AppKind;
use crate::shell::{OpenRequest, Shell, TabKind};

fn app_names() -> Vec<String> {
    AppKind::ALL.iter().map(|k| k.name().to_ascii_lowercase()).collect()
}

pub fn app_from(v: &Value) -> Result<Option<AppKind>, String> {
    match v.as_str() {
        None => Ok(None),
        Some(name) => AppKind::ALL
            .into_iter()
            .find(|k| k.name().eq_ignore_ascii_case(name))
            .map(Some)
            .ok_or_else(|| format!("Unknown app {name:?}; use one of {}", app_names().join(", "))),
    }
}

/// Which files each app opens, for the tool descriptions and the system prompt.
pub fn file_types() -> String {
    AppKind::ALL.iter().map(|k| format!("{}: {}", k.name(), k.extensions().join(", "))).collect::<Vec<_>>().join("; ")
}

/// The `tools/list` answer.
pub fn definitions() -> Vec<Value> {
    let apps = app_names();
    let paths = json!({"type": "array", "items": {"type": "string"}, "minItems": 1, "description": "Absolute paths, or paths relative to the workspace (your working directory)."});
    let mut tools = vec![
        json!({
            "name": "septet_state",
            "description": "What is open in Septet: its windows, their tabs (one per app), each app's document title, whether it has unsaved changes, and which tab is visible. Also the workspace folder.",
            "inputSchema": {"type": "object", "properties": {}},
            "annotations": {"readOnlyHint": true},
        }),
        json!({
            "name": "septet_open",
            "description": format!("Open files as documents in a Septet app (its tab comes to the front). Without `app`, the app is chosen by file type: {}. Files must be in the workspace or among the files the user opened in Septet.", file_types()),
            "inputSchema": {"type": "object", "properties": {"paths": paths, "app": {"type": "string", "enum": apps}}, "required": ["paths"]},
        }),
        json!({
            "name": "septet_place",
            "description": "Place files (images, SVG, PDF pages, clips) into the document that is open in an app, like File > Place. The app must already have a document open (use septet_open first if septet_state shows none).",
            "inputSchema": {"type": "object", "properties": {"paths": paths, "app": {"type": "string", "enum": apps}}, "required": ["paths", "app"]},
        }),
        json!({
            "name": "septet_activate",
            "description": "Bring an app's tab to the front, starting the app if it is not running.",
            "inputSchema": {"type": "object", "properties": {"app": {"type": "string", "enum": apps}}, "required": ["app"]},
        }),
        json!({
            "name": "septet_render",
            "description": "Render an SVG file from the workspace to PNG and look at it. Use it after writing or changing an SVG to check the result before showing it to the user. Fonts in the workspace are used.",
            "inputSchema": {"type": "object", "properties": {
                "path": {"type": "string", "description": "The SVG, in the workspace."},
                "size": {"type": "integer", "description": "Longest side in pixels (64–2048, default 1024)."},
                "out": {"type": "string", "description": "Also save the PNG here (in the workspace)."},
            }, "required": ["path"]},
            "annotations": {"readOnlyHint": true},
        }),
        json!({
            "name": "septet_fetch",
            "description": "Download a file into the workspace from a free icon or font source: https://api.iconify.design/ (SVG icons, e.g. /mdi/home.svg?color=%23000), https://fonts.googleapis.com/ and https://fonts.gstatic.com/ (Google Fonts), https://raw.githubusercontent.com/google/fonts/ (font files with their OFL license). No other hosts.",
            "inputSchema": {"type": "object", "properties": {
                "url": {"type": "string"},
                "path": {"type": "string", "description": "Where to save it, in the workspace."},
            }, "required": ["url", "path"]},
        }),
        json!({
            "name": "approve",
            "description": "Internal: asks the user whether a tool may run.",
            "inputSchema": {"type": "object", "properties": {"tool_name": {"type": "string"}, "input": {"type": "object"}, "tool_use_id": {"type": "string"}}, "required": ["tool_name", "input"]},
        }),
    ];
    tools.extend(super::apps::definitions(&apps));
    tools
}

/// Added to Claude Code's system prompt for every conversation.
pub fn system_prompt(workspace: &Path) -> String {
    format!(
        "You are working inside Septet, a desktop suite of seven creative apps shown as tabs: Photocraft (photo editing), \
         Vectorcraft (vector illustration), Lightcraft (raw photo library), Designcraft (page layout), Pdfcraft (PDF editing), \
         Filmcraft (video editing) and Effectcraft (motion graphics). The user sees Septet next to this chat.\n\n\
         Your working directory is the workspace {ws}. Create files there (you may write there without asking), then show them \
         with the septet_* tools: septet_open opens a file as a document, septet_place puts it into the open document. Check \
         septet_state to see what is open. Apps by file type: {types}.\n\n\
         Prefer generating content as code (SVG, HTML, JSON, scripts) and opening it in the right app. Keep replies short; the \
         user watches the apps change.\n\n\
         To change what is open in an app, drive it with the app_* tools (Vectorcraft so far): find commands with app_commands \
         (always with a filter), run them with app_execute, read ids and properties with app_inspect, look at the result with \
         app_render (the whole artboard or page, or one object or layer alone) and take mistakes back with app_undo. Check your \
         work with app_render before you say it is done.\n\n\
         Septet's skills (septet:*) explain how to do creative work with these apps: load septet:septet-apps with the Skill tool \
         at the start of a creative task, then the skill for the job (svg-graphics, logo-and-icons, typography, color, \
         print-and-pdf, image-editing, video-editing, motion-lottie). Skills named my:* are the user's own; use them when they fit.\n\n\
         Know the limits: Photocraft opens no SVG or PDF; Designcraft opens only .designcraft and .idml documents (you can place \
         files into one the user has open, not create one); in Filmcraft, opening or placing media only adds it to the bin, so \
         build edits as OTIO/FCPXML/EDL; Effectcraft takes Lottie only through its own menu (File > Import > Lottie…), not \
         through septet_open; Vectorcraft drops most SVG filters and uses installed fonts only, so outline text you want exact.",
        ws = workspace.display(),
        types = file_types(),
    )
}

/// Answer every tool call that arrived since the last frame.
pub fn drain(shell: &mut Shell, ctx: &Context) {
    super::apps::retry(shell, ctx);
    let calls: Vec<ToolCall> = match &shell.assistant.mcp {
        Some(server) => server.calls.try_iter().collect(),
        None => return,
    };
    for call in calls {
        if call.name == "approve" {
            approve(shell, call);
            continue;
        }
        if super::apps::is_app_tool(&call.name) {
            super::apps::call(shell, ctx, call, false);
            continue;
        }
        // File work off the UI thread.
        if matches!(call.name.as_str(), "septet_render" | "septet_fetch") {
            let Some(workspace) = shell.assistant.conversation.as_ref().map(|c| c.workspace.clone()) else {
                let _ = call.reply.send(ToolReply::error("No conversation is running."));
                continue;
            };
            std::thread::spawn(move || {
                let r = if call.name == "septet_render" { super::assets::render(&workspace, &call.args) } else { super::assets::fetch(&workspace, &call.args) };
                let _ = call.reply.send(r.unwrap_or_else(ToolReply::error));
            });
            continue;
        }
        let reply = match run(shell, ctx, &call.name, &call.args) {
            Ok(r) => r,
            Err(e) => ToolReply::error(e),
        };
        let _ = call.reply.send(reply);
    }
}

fn approve(shell: &mut Shell, call: ToolCall) {
    let tool = call.args.get("tool_name").and_then(Value::as_str).unwrap_or_default().to_owned();
    let input = call.args.get("input").cloned().unwrap_or(json!({}));
    let a = &mut shell.assistant;
    let Some(conv) = a.conversation.as_mut() else {
        let _ = call.reply.send(super::decision(false, &input));
        return;
    };
    if conv.allowed.contains(&approval_key(&tool, &input)) {
        let _ = call.reply.send(super::decision(true, &input));
        return;
    }
    conv.approvals.push(Approval { tool, input, reply: call.reply });
}

fn run(shell: &mut Shell, ctx: &Context, name: &str, args: &Value) -> Result<ToolReply, String> {
    match name {
        "septet_state" => Ok(ToolReply::json(&state(shell, ctx))),
        "septet_open" | "septet_place" => {
            let app = app_from(&args["app"])?;
            let paths = allowed_paths(shell, &args["paths"])?;
            let place = name == "septet_place";
            if place {
                let kind = app.ok_or("septet_place needs `app`.")?;
                if !shell.apps.contains_key(&kind) {
                    return Err(format!("{} is not running; open a document in it with septet_open first.", kind.name()));
                }
            }
            let list = paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ");
            shell.opens.push(OpenRequest { paths, app, window: None, place: place.then_some(None) });
            ctx.request_repaint();
            Ok(ToolReply::text(format!("{} {list}. The app updates on the next frame; call septet_state to check.", if place { "Placing" } else { "Opening" })))
        }
        "septet_activate" => {
            let kind = app_from(&args["app"])?.ok_or("`app` is required.")?;
            super::apps::show(shell, ctx, kind);
            Ok(ToolReply::text(format!("{} is in front.", kind.name())))
        }
        _ => Err(format!("Septet has no tool {name:?}.")),
    }
}

fn state(shell: &Shell, ctx: &Context) -> Value {
    let focused = shell.focused_window(ctx);
    let windows: Vec<Value> = shell
        .windows
        .iter()
        .filter(|w| !w.hidden)
        .map(|w| {
            let tabs: Vec<Value> = w
                .tabs
                .iter()
                .enumerate()
                .map(|(i, t)| match t.kind {
                    TabKind::Home => json!({"app": "home", "visible": i == w.active}),
                    TabKind::App(k) => {
                        let slot = shell.apps.get(&k).and_then(|s| s.try_borrow().ok());
                        json!({
                            "app": k.name().to_ascii_lowercase(),
                            "visible": i == w.active,
                            "running": slot.is_some(),
                            "document": slot.as_ref().and_then(|s| s.tab_title()),
                            "unsaved": slot.as_ref().map(|s| s.app.has_unsaved_changes()),
                        })
                    }
                })
                .collect();
            json!({"focused": w.viewport == focused, "tabs": tabs})
        })
        .collect();
    let workspace = shell.assistant.conversation.as_ref().map(|c| c.workspace.display().to_string());
    json!({"workspace": workspace, "windows": windows})
}

/// Paths Claude may hand to an app: inside the workspace, or files the user opened in Septet.
fn allowed_paths(shell: &Shell, v: &Value) -> Result<Vec<PathBuf>, String> {
    let workspace = shell.assistant.conversation.as_ref().map(|c| c.workspace.clone()).ok_or("No conversation is running.")?;
    let root = workspace.canonicalize().map_err(|e| e.to_string())?;
    let list = v.as_array().filter(|a| !a.is_empty()).ok_or("`paths` must be a non-empty list.")?;
    list.iter()
        .map(|p| {
            let p = p.as_str().ok_or("Paths must be strings.")?;
            let path = if Path::new(p).is_absolute() { PathBuf::from(p) } else { workspace.join(p) };
            let real = path.canonicalize().map_err(|e| format!("{p}: {e}"))?;
            let opened_by_user = shell.recent.files.iter().any(|f| f.path.canonicalize().is_ok_and(|r| r == real));
            if real.starts_with(&root) || opened_by_user {
                Ok(real)
            } else {
                Err(format!("{p} is outside the workspace. Copy it into {} first.", workspace.display()))
            }
        })
        .collect()
}
