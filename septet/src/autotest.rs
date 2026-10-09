//! `SEPTET_AUTOTEST=<dir>`: drive the shell through a fixed script and save screenshots of every
//! window into `<dir>`, plus a log of what happened. For checking the shell end to end without
//! clicking through it by hand.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;

use egui::{Context, Event, Key, Modifiers, PointerButton, Pos2, Vec2, ViewportCommand, ViewportId, vec2};

use crate::kinds::AppKind;
use crate::shell::{Action, OpenRequest, Shell, TabKind};

#[derive(Clone, Debug)]
enum Step {
    Wait(f64),
    Shot(&'static str),
    Open(AppKind),
    OpenFile(PathBuf),
    TearOff(AppKind),
    Activate(AppKind),
    CloseTab(AppKind),
    Report,
    Quit,
    /// Press keys (with Ctrl) in the window showing an app.
    Keys(AppKind, Vec<Key>),
    /// An `Event::Copy` (what Ctrl+C becomes) in the window showing an app.
    Copy(AppKind),
    /// Drag an app's tab by an offset with the mouse.
    DragTab(AppKind, Vec2),
    /// "Send to": the first app's active document placed into the second.
    SendTo(AppKind, AppKind),
    /// Low-level mouse in the window showing the app: press at, move to (over n frames), release.
    PointerDown(AppKind, Target),
    PointerMove(AppKind, Target, u32),
    PointerUp(AppKind),
    /// Plain key presses (no modifiers).
    Press(AppKind, Vec<Key>),
    /// Open the Claude settings dialog in the first window.
    ClaudeSettings,
    /// Press "Test connection" there.
    ClaudeTest,
    /// Log what the dialog shows: Claude Code found, signed in, test outcome.
    ClaudeReport,
    /// Start a conversation (model from `SEPTET_AUTOTEST_MODEL`) and send this message.
    ClaudeAsk(&'static str),
    /// Wait until Claude's turn ends (at most this many seconds), declining any approval it asks for.
    ClaudeWait(f64),
    /// Log the conversation so far.
    ClaudeTranscript,
    /// Show the Claude panel in the first window and put the keyboard in its chat field.
    ClaudePanel,
    /// Type into the focused widget of the first window (Enter: `\n`).
    TypeText(&'static str),
    /// Call one of Claude's app tools directly, without Claude. `"$id"` in the arguments stands for
    /// the last `id` a tool answered with.
    Tool(&'static str, serde_json::Value),
    /// Wait for the tools' answers (at most this many seconds); log them and save their images.
    ToolWait(f64),
    /// A Home tab in the first window, which hides the app shown there.
    Home,
    Exit,
}

#[derive(Clone, Copy, Debug)]
enum Target {
    At(Pos2),
    /// The middle of an app's tab.
    Tab(AppKind),
    /// The middle of the window's content area.
    Content,
}

#[derive(Clone, Debug, PartialEq)]
struct ShotTag(String);

pub struct Autotest {
    dir: PathBuf,
    steps: Vec<Step>,
    next_at: f64,
    started: Instant,
    log: Vec<String>,
    /// Synthetic input, one batch per frame.
    input: VecDeque<(ViewportId, Vec<Event>)>,
    /// Where the synthetic pointer is.
    pointer: Pos2,
    /// When the last message went to Claude.
    claude_since: f64,
    approval_shot: bool,
    /// Tool calls made by `Step::Tool`, waiting for their answers.
    tools: Vec<(String, std::sync::mpsc::Receiver<crate::assistant::mcp::ToolReply>)>,
    tool_answers: usize,
    last_id: Option<serde_json::Value>,
}

impl Autotest {
    pub fn from_env() -> Option<Self> {
        let dir = PathBuf::from(std::env::var_os("SEPTET_AUTOTEST")?);
        std::fs::create_dir_all(&dir).ok()?;
        let file = std::env::var_os("SEPTET_AUTOTEST_FILE").map(PathBuf::from);
        let pdf = std::env::var_os("SEPTET_AUTOTEST_PDF").map(PathBuf::from);
        let mut steps = match std::env::var("SEPTET_AUTOTEST_SCENARIO").as_deref() {
            Ok("interop") => Self::interop(file, pdf),
            Ok("content") => Self::content(file, pdf),
            // The Claude settings dialog against the Claude Code installed here (needs it signed in).
            Ok("assistant") => vec![
                Step::Wait(2.0),
                Step::ClaudeSettings,
                Step::Wait(5.0),
                Step::ClaudeReport,
                Step::Shot("a01-settings"),
                Step::ClaudeTest,
                Step::Wait(25.0),
                Step::ClaudeReport,
                Step::Shot("a02-tested"),
                Step::Wait(1.0),
                Step::Exit,
            ],
            // The Claude panel beside Vectorcraft: type a request (keys must not reach the app), let Claude work.
            Ok("assistant-panel") => vec![
                Step::Wait(2.0),
                Step::Open(AppKind::Vectorcraft),
                Step::Wait(5.0),
                Step::ClaudePanel,
                Step::Wait(3.0),
                Step::Shot("p01-panel"),
                Step::TypeText(Box::leak(
                    std::env::var("SEPTET_AUTOTEST_PROMPT")
                        .unwrap_or_else(|_| "Make a 400x400 SVG of a blue star on a yellow background, save it as star.svg and open it in Vectorcraft.".into())
                        .into_boxed_str(),
                )),
                Step::Wait(1.0),
                Step::Report,
                Step::Shot("p02-typed"),
                Step::TypeText("\n"),
                Step::Wait(3.0),
                Step::Shot("p03-working"),
                Step::ClaudeWait(240.0),
                Step::Wait(3.0),
                Step::ClaudeTranscript,
                Step::Report,
                Step::Shot("p04-done"),
                Step::Wait(1.0),
                Step::Exit,
            ],
            // A conversation with Septet's tools: Claude writes an SVG and opens it in Vectorcraft.
            Ok("assistant-tools") => vec![
                Step::Wait(2.0),
                Step::ClaudeAsk(
                    "Write a simple logo as logo.svg in the workspace: a red circle with a white letter S in the middle, 512x512. \
                     Then open it in Vectorcraft and check with septet_state that it is open. Answer in one sentence.",
                ),
                Step::ClaudeWait(240.0),
                Step::ClaudeTranscript,
                Step::Report,
                Step::Wait(3.0),
                Step::Shot("t01-vectorcraft"),
                Step::Wait(1.0),
                Step::Exit,
            ],
            // Claude's app tools against one app (`SEPTET_AUTOTEST_APP`, default Vectorcraft), called directly:
            // commands, inspecting and rendering (also while the tab is hidden), a blocked command, undo.
            Ok("assistant-apps") => {
                let name = std::env::var("SEPTET_AUTOTEST_APP").unwrap_or_else(|_| "vectorcraft".into());
                let kind = AppKind::ALL.into_iter().find(|k| k.name().eq_ignore_ascii_case(&name))?;
                Self::app_tools(kind)
            }
            // Run 1 of 2: a few tabs and a torn-off window, then quit (the layout is saved).
            Ok("session-save") => vec![
                Step::Wait(1.0),
                Step::Open(AppKind::Vectorcraft),
                Step::Wait(3.0),
                Step::Open(AppKind::Pdfcraft),
                Step::Wait(3.0),
                Step::Open(AppKind::Designcraft),
                Step::Wait(3.0),
                Step::TearOff(AppKind::Designcraft),
                Step::Wait(3.0),
                Step::Report,
                Step::Quit,
            ],
            // Run 2 of 2: what came back.
            Ok("session-restore") => vec![Step::Wait(5.0), Step::Report, Step::Shot("r01-restored"), Step::Wait(1.0), Step::Exit],
            Ok("drag") => vec![
                Step::Wait(1.0),
                Step::Open(AppKind::Vectorcraft),
                Step::Wait(3.0),
                Step::Open(AppKind::Pdfcraft),
                Step::Wait(3.0),
                Step::Activate(AppKind::Vectorcraft),
                Step::Wait(1.0),
                Step::Report,
                Step::DragTab(AppKind::Pdfcraft, vec2(40.0, 320.0)),
                Step::Wait(3.0),
                Step::Report,
                Step::Shot("d01"),
                Step::Wait(1.0),
                Step::Exit,
            ],
            _ => Self::tour(file),
        };
        steps.reverse();
        Some(Autotest {
            dir,
            steps,
            next_at: 0.0,
            started: Instant::now(),
            log: Vec::new(),
            input: VecDeque::new(),
            pointer: Pos2::ZERO,
            claude_since: 0.0,
            approval_shot: false,
            tools: Vec::new(),
            tool_answers: 0,
            last_id: None,
        })
    }

    /// The `assistant-apps` script for one app: make a document, change it with a few commands,
    /// look at it (whole and one part) while the tab is hidden, undo.
    fn app_tools(kind: AppKind) -> Vec<Step> {
        use serde_json::json;
        let app = kind.name().to_ascii_lowercase();
        let call = |tool: &'static str, more: serde_json::Value| {
            let mut v = json!({"app": app});
            if let (Some(v), Some(more)) = (v.as_object_mut(), more.as_object()) {
                v.extend(more.clone());
            }
            Step::Tool(tool, v)
        };
        let run = |command: &str, params: serde_json::Value| call("app_execute", json!({"command": command, "params": params}));
        // (document, filter, changes, the part to look at, undo steps, steps of its own)
        let (new, filter, changes, part, undo, extra): (Step, &str, Vec<Step>, &str, u64, Vec<Step>) = match kind {
            AppKind::Vectorcraft => (
                run("file.new", json!({"width": 400, "height": 300})),
                "rectangle",
                vec![
                    run("shape.ellipse", json!({"x": 40, "y": 40, "width": 200, "height": 140})),
                    run("paint.setFill", json!({"color": "#e8a33d"})),
                    run("shape.rectangle", json!({"x": 180, "y": 120, "width": 160, "height": 120})),
                    run("paint.setFill", json!({"color": "#2f6fd6"})),
                ],
                "object",
                2,
                vec![run("file.newDialog", json!({}))],
            ),
            AppKind::Photocraft => (
                run("file.new", json!({"width": 400, "height": 300})),
                "fill",
                vec![
                    run("select.rect", json!({"x": 30, "y": 30, "width": 200, "height": 140})),
                    run("edit.fill", json!({"color": "#e8a33d"})),
                    run("layer.new.layer", json!({"name": "Blue box"})),
                    run("select.rect", json!({"x": 180, "y": 120, "width": 160, "height": 120})),
                    run("edit.fill", json!({"color": "#2f6fd6"})),
                    run("select.deselect", json!({})),
                ],
                "layer",
                3,
                vec![run("filter.nonsense", json!({}))],
            ),
            AppKind::Designcraft => (
                run("file.new", json!({"width": 420, "height": 300, "pages": 1})),
                "frame",
                vec![
                    run("swatch.create", json!({"name": "Orange", "color": "#e8a33d"})),
                    run("frame.create", json!({"rect": [40, 40, 260, 180], "shape": "ellipse", "content": "unassigned"})),
                    run("object.fill", json!({"swatch": "Orange"})),
                    run("frame.create", json!({"rect": [180, 120, 390, 260], "content": "text", "text": "Hello from Septet"})),
                ],
                "object",
                2,
                vec![run("file.place", json!({}))],
            ),
            AppKind::Effectcraft => (
                run("comp.new", json!({"name": "Main", "width": 640, "height": 360, "duration": 4, "background": "#202830"})),
                "solid",
                vec![
                    run("layer.newSolid", json!({"name": "Orange", "color": "#e8a33d", "width": 220, "height": 150})),
                    run("prop.set", json!({"layer": "$id", "path": "transform/position", "value": [220, 150]})),
                    run("layer.newSolid", json!({"name": "Blue", "color": "#2f6fd6", "width": 160, "height": 120})),
                    run("prop.set", json!({"layer": "$id", "path": "transform/position", "value": [420, 230]})),
                ],
                "layer",
                2,
                vec![run("file.quitNonsense", json!({}))],
            ),
            _ => (Step::Report, "", vec![], "object", 1, vec![]),
        };
        let mut steps = vec![Step::Wait(2.0), new, Step::ToolWait(40.0), call("app_commands", json!({})), call("app_commands", json!({"filter": filter}))];
        for change in changes {
            steps.extend([change, Step::ToolWait(10.0)]);
        }
        steps.extend([
            Step::Wait(1.0),
            Step::Shot("x01-changed"),
            Step::Home,
            Step::Wait(1.5),
            call("app_inspect", json!({"what": "document", "depth": 2})),
            call("app_inspect", json!({"what": part, "id": "$id"})),
            call("app_inspect", json!({"what": "nonsense"})),
            call("app_render", json!({"target": "document"})),
            call("app_render", json!({"target": part, "id": "$id", "max_side": 400})),
            call("app_render", json!({"target": "selection", "background": "transparent"})),
            Step::ToolWait(30.0),
            Step::Report,
            Step::Shot("x02-hidden"),
        ]);
        steps.extend(extra);
        steps.extend([
            run("app.quit", json!({})),
            call("app_undo", json!({"steps": undo})),
            Step::ToolWait(20.0),
            Step::Wait(1.5),
            Step::Report,
            call("app_inspect", json!({"what": "history"})),
            call("app_render", json!({"target": "document", "max_side": 600})),
            Step::ToolWait(20.0),
            Step::Shot("x03-undone"),
            Step::Wait(1.0),
            Step::Exit,
        ]);
        steps
    }

    /// Every app once, a torn-off window, a closed tab, quit.
    fn tour(file: Option<PathBuf>) -> Vec<Step> {
        let mut steps = vec![Step::Wait(3.0), Step::Shot("01-home"), Step::Report];
        let names = ["photocraft", "vectorcraft", "lightcraft", "designcraft", "pdfcraft", "filmcraft", "effectcraft"];
        for (n, (kind, name)) in AppKind::ALL.into_iter().zip(names).enumerate() {
            steps.push(Step::Open(kind));
            steps.push(Step::Wait(7.0));
            steps.push(Step::Shot(Box::leak(format!("{:02}-{name}", n + 2).into_boxed_str())));
            steps.push(Step::Report);
        }
        if let Some(file) = file {
            steps.extend([Step::OpenFile(file), Step::Wait(6.0), Step::Shot("09-opened-file"), Step::Report]);
        }
        steps.extend([
            Step::TearOff(AppKind::Effectcraft),
            Step::Wait(5.0),
            Step::Shot("10-torn-off"),
            Step::Report,
            Step::Activate(AppKind::Vectorcraft),
            Step::Wait(3.0),
            Step::Shot("11-vectorcraft-again"),
            Step::CloseTab(AppKind::Designcraft),
            Step::Wait(3.0),
            Step::Shot("12-designcraft-closed"),
            Step::Report,
            Step::Quit,
        ]);
        steps
    }

    /// Copy in Photocraft and paste into a PDF, reorder and tear off tabs with the mouse, close a tab
    /// with unsaved work.
    fn interop(file: Option<PathBuf>, pdf: Option<PathBuf>) -> Vec<Step> {
        let mut steps = vec![Step::Wait(2.0)];
        if let Some(file) = file {
            steps.extend([Step::OpenFile(file), Step::Wait(5.0)]);
        }
        if let Some(pdf) = pdf {
            steps.extend([Step::OpenFile(pdf), Step::Wait(6.0)]);
        }
        steps.extend([
            Step::Shot("i01-pdf-before"),
            Step::Report,
            Step::Activate(AppKind::Photocraft),
            Step::Wait(1.5),
            Step::Keys(AppKind::Photocraft, vec![Key::A]),
            Step::Wait(0.7),
            Step::Copy(AppKind::Photocraft),
            Step::Wait(2.0),
            Step::Activate(AppKind::Pdfcraft),
            Step::Wait(1.5),
            Step::Keys(AppKind::Pdfcraft, vec![Key::V]),
            Step::Wait(3.0),
            Step::Shot("i02-pdf-after-paste"),
            Step::Report,
            Step::Open(AppKind::Vectorcraft),
            Step::Wait(4.0),
            Step::DragTab(AppKind::Vectorcraft, vec2(-420.0, 4.0)),
            Step::Wait(1.0),
            Step::Report,
            Step::DragTab(AppKind::Pdfcraft, vec2(40.0, 320.0)),
            Step::Wait(4.0),
            Step::Shot("i03-tearoff"),
            Step::Report,
            Step::CloseTab(AppKind::Pdfcraft),
            Step::Wait(3.0),
            Step::Shot("i04-close-prompt"),
            Step::Report,
            Step::Exit,
        ]);
        steps
    }

    /// "Send to" and dragging content from one app into another.
    fn content(file: Option<PathBuf>, pdf: Option<PathBuf>) -> Vec<Step> {
        let mut steps = vec![Step::Wait(2.0)];
        if let Some(file) = file {
            steps.extend([Step::OpenFile(file), Step::Wait(5.0)]);
        }
        if let Some(pdf) = pdf {
            steps.extend([Step::OpenFile(pdf), Step::Wait(5.0)]);
        }
        steps.extend([
            Step::Open(AppKind::Lightcraft),
            Step::Wait(6.0),
            Step::Report,
            Step::SendTo(AppKind::Lightcraft, AppKind::Photocraft),
            Step::Wait(5.0),
            Step::Shot("c01-lightcraft-sent-to-photocraft"),
            Step::Report,
            Step::SendTo(AppKind::Photocraft, AppKind::Pdfcraft),
            Step::Wait(4.0),
            Step::Shot("c02-photocraft-sent-to-pdfcraft"),
            Step::Report,
            // Lightcraft's grid: drag a photo onto Photocraft's tab, wait for it to open, drop.
            Step::Activate(AppKind::Lightcraft),
            Step::Wait(1.5),
            Step::Press(AppKind::Lightcraft, vec![Key::G]),
            Step::Wait(2.0),
            Step::Shot("c03-lightcraft-grid"),
            Step::PointerDown(AppKind::Lightcraft, Target::At(egui::pos2(78.0, 280.0))),
            Step::PointerMove(AppKind::Lightcraft, Target::At(egui::pos2(130.0, 330.0)), 8),
            Step::PointerMove(AppKind::Lightcraft, Target::Tab(AppKind::Photocraft), 16),
            Step::Wait(1.2),
            Step::Shot("c04-spring-loaded"),
            Step::PointerMove(AppKind::Lightcraft, Target::Content, 16),
            Step::Wait(0.5),
            Step::Shot("c05-over-photocraft"),
            Step::PointerUp(AppKind::Lightcraft),
            Step::Wait(5.0),
            Step::Shot("c06-dropped"),
            Step::Report,
            Step::Exit,
        ]);
        steps
    }

    fn note(&mut self, line: String) {
        let line = format!("[{:7.2}s] {line}", self.started.elapsed().as_secs_f64());
        eprintln!("autotest {line}");
        self.log.push(line);
        let _ = std::fs::write(self.dir.join("log.txt"), self.log.join("\n"));
    }

    /// Once per frame, before the windows are drawn.
    pub fn tick(shell: &mut Shell, ctx: &Context) {
        let Some(mut at) = shell.autotest.take() else { return };
        let now = ctx.input(|i| i.time);
        // Feed synthetic input one batch per frame; the script waits until it's all delivered.
        if !at.input.is_empty() {
            let mut sh = shell.router.lock();
            if let Some((v, _)) = at.input.front()
                && !sh.inject_events.contains_key(v)
                && let Some((v, events)) = at.input.pop_front()
            {
                sh.inject_events.insert(v, events);
                ctx.request_repaint_of(v);
            }
            drop(sh);
            ctx.request_repaint();
            shell.autotest = Some(at);
            return;
        }
        while now >= at.next_at {
            let Some(step) = at.steps.pop() else { break };
            match step {
                Step::Wait(s) => at.next_at = now + s,
                Step::Shot(name) => {
                    // The whole screen too: eframe can't screenshot extra (immediate) windows.
                    if let Some(display) = std::env::var_os("DISPLAY") {
                        let out = at.dir.join(format!("{name}-screen.png"));
                        let _ = std::process::Command::new("import").arg("-display").arg(display).args(["-window", "root"]).arg(out).spawn();
                    }
                    for (i, w) in shell.windows.iter().enumerate().filter(|(_, w)| !w.hidden) {
                        let tag = format!("{name}-w{i}");
                        shell.router.send(w.viewport, ViewportCommand::Screenshot(egui::UserData::new(ShotTag(tag))));
                        ctx.request_repaint_of(w.viewport);
                    }
                    at.next_at = now + 0.5;
                }
                Step::Open(kind) => {
                    at.note(format!("open {}", kind.name()));
                    let window = shell.focused_window(ctx);
                    shell.actions.push(Action::NewTab { window, kind: TabKind::App(kind) });
                }
                Step::OpenFile(path) => {
                    at.note(format!("open file {}", path.display()));
                    shell.opens.push(OpenRequest { paths: vec![path], app: None, window: None, place: None });
                }
                Step::TearOff(kind) => {
                    if let Some((wi, ti)) = shell.find_app_tab(kind) {
                        let tab = shell.windows[wi].tabs[ti].id;
                        let size = shell.window_size(shell.windows[wi].viewport);
                        at.note(format!("tear off {}", kind.name()));
                        shell.actions.push(Action::TearOff { tab, at: Some(egui::pos2(120.0, 90.0)), size });
                    }
                }
                Step::Activate(kind) => {
                    if let Some((wi, ti)) = shell.find_app_tab(kind) {
                        let tab = shell.windows[wi].tabs[ti].id;
                        shell.actions.push(Action::Activate { tab });
                    }
                }
                Step::CloseTab(kind) => {
                    if let Some((wi, ti)) = shell.find_app_tab(kind) {
                        let tab = shell.windows[wi].tabs[ti].id;
                        at.note(format!("close tab {}", kind.name()));
                        shell.actions.push(Action::CloseTab { tab });
                    }
                }
                Step::Report => {
                    let mut lines = Vec::new();
                    for (i, w) in shell.windows.iter().enumerate() {
                        let tabs: Vec<String> = w
                            .tabs
                            .iter()
                            .enumerate()
                            .map(|(ti, t)| {
                                let mark = if ti == w.active { "*" } else { "" };
                                match t.kind {
                                    TabKind::Home => format!("{mark}Home"),
                                    TabKind::App(k) => {
                                        let s = shell.apps.get(&k).and_then(|s| s.try_borrow().ok());
                                        let info = s
                                            .map(|s| format!("{:?} dirty={} frames={}", s.tab_title(), s.app.has_unsaved_changes(), s.frames))
                                            .unwrap_or_else(|| "not started".into());
                                        format!("{mark}{} [{info}]", k.name())
                                    }
                                }
                            })
                            .collect();
                        lines.push(format!("window {i} {:?} hidden={} closing={} title={:?}: {}", w.viewport, w.hidden, w.closing, w.title, tabs.join(" | ")));
                    }
                    for l in lines {
                        at.note(l);
                    }
                }
                Step::Keys(kind, keys) => {
                    if let Some((wi, _)) = shell.find_app_tab(kind) {
                        let v = shell.windows[wi].viewport;
                        let m = Modifiers { ctrl: true, command: true, ..Default::default() };
                        for key in keys {
                            at.note(format!("Ctrl+{key:?} in {}", kind.name()));
                            let ev = |pressed| Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers: m };
                            at.input.push_back((v, vec![Event::ModifiersChanged(m), ev(true)]));
                            at.input.push_back((v, vec![ev(false), Event::ModifiersChanged(Modifiers::NONE)]));
                        }
                    }
                }
                Step::Copy(kind) => {
                    if let Some((wi, _)) = shell.find_app_tab(kind) {
                        at.note(format!("copy in {}", kind.name()));
                        at.input.push_back((shell.windows[wi].viewport, vec![Event::Copy]));
                    }
                }
                Step::DragTab(kind, by) => {
                    if let Some((wi, ti)) = shell.find_app_tab(kind) {
                        let w = &shell.windows[wi];
                        let id = w.tabs[ti].id;
                        if let Some((_, r)) = w.tab_rects.iter().find(|(t, _)| *t == id) {
                            let v = w.viewport;
                            let start = r.center();
                            at.note(format!("drag {} tab from {start:?} by {by:?}", kind.name()));
                            let button = |pos: Pos2, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
                            at.input.push_back((v, vec![Event::PointerMoved(start)]));
                            at.input.push_back((v, vec![button(start, true)]));
                            for i in 1..=14 {
                                at.input.push_back((v, vec![Event::PointerMoved(start + by * (i as f32 / 14.0))]));
                            }
                            at.input.push_back((v, vec![button(start + by, false)]));
                        }
                    }
                }
                Step::SendTo(source, target) => {
                    at.note(format!("send {} → {}", source.name(), target.name()));
                    shell.actions.push(Action::SendTo { source, target });
                }
                Step::Press(kind, keys) => {
                    if let Some((wi, _)) = shell.find_app_tab(kind) {
                        let v = shell.windows[wi].viewport;
                        for key in keys {
                            at.note(format!("{key:?} in {}", kind.name()));
                            let ev = |pressed| Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers: Modifiers::NONE };
                            at.input.push_back((v, vec![ev(true)]));
                            at.input.push_back((v, vec![ev(false)]));
                        }
                    }
                }
                Step::PointerDown(kind, target) => {
                    if let Some(v) = window_of(shell, kind) {
                        let p = resolve(shell, v, target);
                        at.note(format!("pointer down at {p:?}"));
                        at.pointer = p;
                        at.input.push_back((v, vec![Event::PointerMoved(p)]));
                        at.input
                            .push_back((v, vec![Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]));
                    }
                }
                Step::PointerMove(kind, target, frames) => {
                    if let Some(v) = window_of(shell, kind) {
                        let to = resolve(shell, v, target);
                        let from = at.pointer;
                        at.note(format!("pointer move to {to:?}"));
                        for i in 1..=frames.max(1) {
                            at.input.push_back((v, vec![Event::PointerMoved(from + (to - from) * (i as f32 / frames.max(1) as f32))]));
                        }
                        at.pointer = to;
                    }
                }
                Step::PointerUp(kind) => {
                    if let Some(v) = window_of(shell, kind) {
                        at.note(format!("pointer up at {:?}", at.pointer));
                        at.input.push_back((
                            v,
                            vec![Event::PointerButton { pos: at.pointer, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }],
                        ));
                    }
                }
                Step::ClaudeSettings => {
                    at.note("open Claude settings".into());
                    shell.assistant.dialog = Some(shell.windows[0].viewport);
                    shell.assistant.recheck(ctx);
                }
                Step::ClaudePanel => {
                    let v = shell.windows[0].viewport;
                    if let Ok(m) = std::env::var("SEPTET_AUTOTEST_MODEL") {
                        shell.assistant.settings.model = Some(m);
                    }
                    shell.assistant.settings.enabled = true;
                    shell.assistant.toggle_panel(ctx, v);
                    at.note("Claude panel shown".into());
                }
                Step::TypeText(text) => {
                    let v = shell.windows[0].viewport;
                    let events = text
                        .split('\n')
                        .enumerate()
                        .flat_map(|(i, part)| {
                            let enter = (i > 0).then(|| {
                                [true, false].map(|pressed| Event::Key {
                                    key: Key::Enter,
                                    physical_key: None,
                                    pressed,
                                    repeat: false,
                                    modifiers: Modifiers::NONE,
                                })
                            });
                            enter.into_iter().flatten().chain((!part.is_empty()).then(|| Event::Text(part.to_owned())))
                        })
                        .collect();
                    at.input.push_back((v, events));
                    at.claude_since = now;
                }
                Step::ClaudeTest => {
                    at.note("test Claude connection".into());
                    shell.assistant.start_test(ctx);
                }
                Step::ClaudeReport => {
                    let a = &shell.assistant;
                    let probe = match &a.probe {
                        crate::assistant::ProbeState::Checking(_) => "checking".to_owned(),
                        crate::assistant::ProbeState::Done(p) => format!("{p:?}"),
                    };
                    let test = a.test.as_ref().map(|t| format!("{:?} reply={:?} took={:.1}s", t.outcome, t.reply, t.took));
                    at.note(format!("claude: {probe}; test: {test:?}; error: {:?}", a.login_error));
                }
                Step::ClaudeAsk(text) => {
                    at.note(format!("ask Claude: {text}"));
                    if let Ok(m) = std::env::var("SEPTET_AUTOTEST_MODEL") {
                        shell.assistant.settings.model = Some(m);
                    }
                    shell.assistant.recheck(ctx);
                    // The probe runs on a thread; wait for it here (tests only).
                    let deadline = Instant::now() + std::time::Duration::from_secs(20);
                    while matches!(shell.assistant.probe, crate::assistant::ProbeState::Checking(_)) && Instant::now() < deadline {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        shell.assistant.logic(ctx);
                    }
                    match shell.assistant.start_conversation(ctx, None) {
                        Ok(()) => shell.assistant.conversation.as_mut().expect("started").send(text),
                        Err(e) => at.note(format!("could not start: {e}")),
                    }
                    at.claude_since = now;
                }
                Step::ClaudeWait(limit) => {
                    let Some(conv) = shell.assistant.conversation.as_mut() else { continue };
                    if !conv.approvals.is_empty() {
                        // First time: let the card show for a screenshot, then answer.
                        if !at.approval_shot {
                            at.approval_shot = true;
                            at.steps.extend([Step::ClaudeWait(limit), Step::Shot("approval"), Step::Wait(1.5)]);
                            continue;
                        }
                        let a = &conv.approvals[0];
                        let allow = std::env::var("SEPTET_AUTOTEST_APPROVE").is_ok_and(|v| v == "allow");
                        at.note(format!("{} approval for {} {}", if allow { "allowing" } else { "declining" }, a.tool, a.input));
                        conv.answer(0, if allow { crate::assistant::Choice::Once } else { crate::assistant::Choice::Deny });
                    }
                    if conv.busy && now - at.claude_since < limit {
                        at.steps.push(Step::ClaudeWait(limit));
                        at.next_at = now + 0.25;
                        ctx.request_repaint_after(std::time::Duration::from_millis(250));
                        break;
                    }
                    at.note(format!("Claude done after {:.1}s (busy={})", now - at.claude_since, conv.busy));
                }
                Step::ClaudeTranscript => {
                    let lines: Vec<String> = shell
                        .assistant
                        .conversation
                        .as_ref()
                        .map(|c| {
                            c.entries
                                .iter()
                                .map(|e| match e {
                                    crate::assistant::conversation::Entry::Tool { name, input, result, .. } => {
                                        let r = result.as_ref().map(|(parts, err)| {
                                            format!(
                                                "err={err} {:?}",
                                                parts.iter().map(|p| format!("{p:?}").chars().take(200).collect::<String>()).collect::<Vec<_>>()
                                            )
                                        });
                                        format!("tool {name} {} -> {r:?}", input.to_string().chars().take(300).collect::<String>())
                                    }
                                    other => format!("{other:?}"),
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    for l in lines {
                        at.note(format!("transcript: {l}"));
                    }
                    let limit = shell.assistant.conversation.as_ref().and_then(|c| c.rate_limit.clone());
                    at.note(format!("rate limit: {limit:?}"));
                }
                Step::Tool(name, mut args) => {
                    if let Some(id) = &at.last_id {
                        fn put(v: &mut serde_json::Value, id: &serde_json::Value) {
                            match v {
                                serde_json::Value::String(s) if s == "$id" => *v = id.clone(),
                                serde_json::Value::Object(o) => o.values_mut().for_each(|v| put(v, id)),
                                _ => {}
                            }
                        }
                        put(&mut args, id);
                    }
                    at.note(format!("tool {name} {args}"));
                    let (reply, rx) = std::sync::mpsc::channel();
                    at.tools.push((format!("{name} {args}"), rx));
                    at.claude_since = now;
                    crate::assistant::apps::call(shell, ctx, crate::assistant::mcp::ToolCall { name: name.into(), args, reply }, false);
                }
                Step::ToolWait(limit) => {
                    let mut waiting = Vec::new();
                    for (what, rx) in std::mem::take(&mut at.tools) {
                        let Ok(r) = rx.try_recv() else {
                            waiting.push((what, rx));
                            continue;
                        };
                        at.tool_answers += 1;
                        let n = at.tool_answers;
                        let mut parts = Vec::new();
                        for c in &r.content {
                            if let Some(text) = c["text"].as_str() {
                                // Ids of new things: Vectorcraft's objects, Photocraft's layers, Designcraft's items…
                                let made = serde_json::from_str::<serde_json::Value>(text).ok();
                                if let Some(id) = made.and_then(|v| {
                                    ["id", "layer", "item", "clip"].iter().find_map(|k| v.get(*k).filter(|i| i.is_u64() || i.is_string()).cloned())
                                }) {
                                    at.last_id = Some(id);
                                }
                                parts.push(text.chars().take(700).collect::<String>());
                            } else if let Some(data) = c["data"].as_str() {
                                use base64::Engine;
                                let path = at.dir.join(format!("tool-{n:02}.png"));
                                let saved = base64::engine::general_purpose::STANDARD.decode(data).ok().map(|png| std::fs::write(&path, png));
                                parts.push(format!("[image {}: {}]", path.display(), if matches!(saved, Some(Ok(()))) { "saved" } else { "NOT SAVED" }));
                            }
                        }
                        at.note(format!("answer {n} to {what}: error={} {}", r.is_error, parts.join(" | ")));
                    }
                    let left = waiting.len();
                    at.tools = waiting;
                    if left > 0 && now - at.claude_since < limit {
                        at.steps.push(Step::ToolWait(limit));
                        at.next_at = now + 0.1;
                        break;
                    }
                    if left > 0 {
                        at.note(format!("{left} tool calls did not answer"));
                    }
                }
                Step::Home => {
                    let window = shell.windows[0].viewport;
                    shell.actions.push(Action::NewTab { window, kind: TabKind::Home });
                }
                Step::Exit => {
                    at.note("exit".into());
                    std::process::exit(0);
                }
                Step::Quit => {
                    at.note("quit".into());
                    shell.actions.push(Action::Quit);
                }
            }
        }
        // Wake up for the next step without keeping the frame rate up meanwhile.
        ctx.request_repaint_after(std::time::Duration::from_secs_f64((at.next_at - now).clamp(0.02, 1.0)));
        shell.autotest = Some(at);
    }

    /// In each window's pass: save screenshots that arrived.
    pub fn window(shell: &mut Shell, ctx: &Context, viewport: ViewportId) {
        let Some(at) = shell.autotest.as_mut() else { return };
        let shots: Vec<(String, std::sync::Arc<egui::ColorImage>)> = ctx.input(|i| {
            i.raw
                .events
                .iter()
                .filter_map(|e| match e {
                    Event::Screenshot { user_data, image, .. } => {
                        let tag = user_data.data.as_ref()?.downcast_ref::<ShotTag>()?;
                        Some((tag.0.clone(), image.clone()))
                    }
                    _ => None,
                })
                .collect()
        });
        for (tag, image) in shots {
            let path = at.dir.join(format!("{tag}.png"));
            let [w, h] = image.size;
            let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
            let saved = image::RgbaImage::from_raw(w as u32, h as u32, rgba).map(|img| img.save(&path));
            at.note(format!("screenshot {} ({w}x{h}) from {viewport:?}: {}", path.display(), if matches!(saved, Some(Ok(()))) { "ok" } else { "FAILED" }));
        }
    }
}

fn window_of(shell: &Shell, kind: AppKind) -> Option<ViewportId> {
    shell.find_app_tab(kind).map(|(wi, _)| shell.windows[wi].viewport)
}

fn resolve(shell: &Shell, viewport: ViewportId, target: Target) -> Pos2 {
    let Some(wi) = shell.window_index(viewport) else { return Pos2::ZERO };
    let w = &shell.windows[wi];
    match target {
        Target::At(p) => p,
        Target::Tab(kind) => w
            .tabs
            .iter()
            .find(|t| t.kind == TabKind::App(kind))
            .and_then(|t| w.tab_rects.iter().find(|(id, _)| *id == t.id))
            .map_or(Pos2::ZERO, |(_, r)| r.center()),
        Target::Content => egui::pos2(w.strip.center().x, w.strip.bottom() + 400.0),
    }
}
