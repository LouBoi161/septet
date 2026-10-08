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
        Some(Autotest { dir, steps, next_at: 0.0, started: Instant::now(), log: Vec::new(), input: VecDeque::new(), pointer: Pos2::ZERO })
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
