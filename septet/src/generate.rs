//! Generative Fill and Generate Image in the apps (`docs/suite/PLAN.md`, Phase H3): the app's
//! command asks Septet, Septet asks for the prompt, takes the area from the app, runs the job in
//! ComfyUI (`comfy`) with its progress in the window, and hands the result back to the app as a
//! new layer, shown only inside the selection. The original pixels are never touched.

use std::cell::RefCell;
use std::rc::Rc;

use egui::{Context, Id, RichText, ViewportId};
use serde_json::json;

use crate::comfy::{self, Comfy, Event, Job, JobId, Stage, Task, workflow};
use crate::hosted::Generate;
use crate::kinds::AppKind;
use crate::shell::Shell;
use crate::theme::{self, ShellColors};

pub struct Generation {
    pub comfy: Comfy,
    /// The Image generation settings, open in this window.
    pub settings: Option<ViewportId>,
    /// The apps' generation commands since the last frame.
    pub requests: Rc<RefCell<Vec<(AppKind, Generate)>>>,
    prompt: Option<Prompt>,
    jobs: Vec<Running>,
    /// The apps the graphics-memory reserve was last worked out for.
    reserved_for: Vec<AppKind>,
}

/// The prompt dialog.
struct Prompt {
    app: AppKind,
    kind: Generate,
    window: ViewportId,
    text: String,
    error: Option<String>,
}

/// A job and where its result goes.
struct Running {
    id: JobId,
    app: AppKind,
    kind: Generate,
    window: ViewportId,
    document: u64,
    rect: [u32; 4],
    mask: Option<Vec<u8>>,
    /// The new layer's name: the prompt and seed, to make the same one again.
    name: String,
    stage: Stage,
    error: Option<String>,
}

impl Generation {
    pub fn new(ctx: &Context) -> Self {
        Generation { comfy: Comfy::new(ctx), settings: None, requests: Rc::default(), prompt: None, jobs: Vec::new(), reserved_for: Vec::new() }
    }

    /// A job is still running (not failed).
    pub fn busy(&self) -> bool {
        self.jobs.iter().any(|j| j.error.is_none())
    }

    /// What the failed jobs say.
    pub fn errors(&self) -> Vec<String> {
        self.jobs.iter().filter_map(|j| j.error.clone()).collect()
    }
}

/// For the autotest: as if `app`'s command ran and the user typed `text` and pressed Generate.
pub fn start_now(shell: &mut Shell, ctx: &Context, app: AppKind, kind: Generate, text: &str) -> Result<(), String> {
    let window = shell.focused_window(ctx);
    start(shell, ctx, &Prompt { app, kind, window, text: text.into(), error: None })
}

fn title(kind: Generate) -> &'static str {
    match kind {
        Generate::Fill => "Generative Fill",
        Generate::Image => "Generate Image",
    }
}

/// Once per frame: the apps' requests, the worker's news, and results into the apps.
pub fn logic(shell: &mut Shell, ctx: &Context) {
    let requests: Vec<(AppKind, Generate)> = shell.generation.requests.borrow_mut().drain(..).collect();
    if let Some(&(app, kind)) = requests.last() {
        let window = shell.focused_window(ctx);
        shell.generation.prompt = Some(Prompt { app, kind, window, text: String::new(), error: None });
    }
    let open: Vec<AppKind> = shell.apps.keys().copied().collect();
    if open != shell.generation.reserved_for {
        let names: Vec<&str> = open.iter().map(|k| k.name()).collect();
        shell.generation.comfy.set_reserve(comfy::reserve_for(&names));
        shell.generation.reserved_for = open;
    }
    for event in shell.generation.comfy.poll() {
        match event {
            Event::Stage(id, stage) => {
                if let Some(j) = shell.generation.jobs.iter_mut().find(|j| j.id == id) {
                    j.stage = stage;
                }
            }
            Event::Failed(id, e) => {
                if e == "Cancelled." {
                    shell.generation.jobs.retain(|j| j.id != id);
                } else if let Some(j) = shell.generation.jobs.iter_mut().find(|j| j.id == id) {
                    j.error = Some(e);
                }
            }
            Event::Done(id, img) => {
                let Some(at) = shell.generation.jobs.iter().position(|j| j.id == id) else { continue };
                let job = shell.generation.jobs.remove(at);
                let image = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                let placed = match shell.apps.get(&job.app).cloned() {
                    Some(slot) => slot.borrow_mut().with_app(ctx, |a| a.place_generated(job.document, job.rect, &image, job.mask.as_deref(), &job.name)),
                    None => Err(format!("{} was closed; the image was not placed.", job.app.name())),
                };
                if let Err(e) = placed {
                    shell.generation.jobs.push(Running { error: Some(e), ..job });
                }
                ctx.request_repaint();
            }
        }
    }
}

/// The dialogs and the progress of jobs in `viewport`.
pub fn window_ui(shell: &mut Shell, ctx: &Context, viewport: ViewportId) {
    if shell.generation.settings == Some(viewport) {
        let mut open = true;
        comfy::dialog::settings(&mut open, ctx, &mut shell.generation.comfy);
        if !open {
            shell.generation.settings = None;
        }
    }
    if shell.generation.prompt.as_ref().is_some_and(|p| p.window == viewport) {
        prompt_dialog(shell, ctx);
    }
    progress(shell, ctx, viewport);
}

fn prompt_dialog(shell: &mut Shell, ctx: &Context) {
    let c = ShellColors::home();
    let Some(p) = &mut shell.generation.prompt else { return };
    let model = &mut shell.generation.comfy.settings.model;
    let model_before = *model;
    let mut go = false;
    let mut settings = false;
    let resp = egui::Modal::new(Id::new("septet-generate")).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.label(RichText::new(title(p.kind)).font(theme::semibold(18.0)));
        ui.add_space(4.0);
        let hint = match p.kind {
            Generate::Fill => "What should fill the selection? Leave it empty to remove what is there.",
            Generate::Image => "Describe the image: what is in it, where, the light, the style.",
        };
        ui.label(RichText::new(hint).color(c.text_dim));
        ui.add_space(8.0);
        let edit = ui.add(egui::TextEdit::multiline(&mut p.text).desired_rows(4).desired_width(f32::INFINITY).hint_text(match p.kind {
            Generate::Fill => "a small wooden boat",
            Generate::Image => "A wide photograph of a lighthouse on a cliff at dusk…",
        }));
        if !edit.has_focus() && p.text.is_empty() && p.error.is_none() {
            edit.request_focus();
        }
        let enter = edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Quality");
            egui::ComboBox::from_id_salt("septet-generate-model").selected_text(model.label()).show_ui(ui, |ui| {
                for m in comfy::Model::ALL {
                    ui.selectable_value(model, m, m.label());
                }
            });
        });
        if let Some(e) = &p.error {
            ui.add_space(6.0);
            ui.label(RichText::new(e).color(ui.visuals().error_fg_color));
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.link("Settings…").clicked() {
                settings = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let can = p.kind == Generate::Fill || !p.text.trim().is_empty();
                if ui.add_enabled(can, egui::Button::new(RichText::new("Generate").color(c.on_accent)).fill(c.accent)).clicked() || (enter && can) {
                    go = true;
                }
                if ui.button("Cancel").clicked() {
                    ui.close();
                }
            });
        });
        ui.label(
            RichText::new("Runs on this computer with ComfyUI and Qwen Image 2.1. The result is a new layer; nothing else changes.")
                .size(11.5)
                .color(c.text_dim),
        );
    });
    if *model != model_before {
        shell.generation.comfy.apply_settings();
    }
    if settings {
        shell.generation.settings = shell.generation.prompt.as_ref().map(|p| p.window);
    }
    if resp.should_close() {
        shell.generation.prompt = None;
        return;
    }
    if go && let Some(p) = shell.generation.prompt.take() {
        if let Err(e) = start(shell, ctx, &p) {
            shell.generation.prompt = Some(Prompt { error: Some(e), ..p });
        }
    }
}

/// Take the area from the app and queue the job.
fn start(shell: &mut Shell, ctx: &Context, p: &Prompt) -> Result<(), String> {
    let slot = shell.apps.get(&p.app).cloned().ok_or_else(|| format!("{} is closed.", p.app.name()))?;
    let mut slot = slot.borrow_mut();
    // Context around the selection: a third of its size each side, so the model sees what the
    // fill has to continue.
    let bounds = slot.with_app(ctx, |a| a.agent_inspect(ctx, "selection", &json!({}))).and_then(Result::ok);
    let side = bounds.as_ref().and_then(|b| b["selectionBounds"].as_array().map(|r| r.iter().skip(2).filter_map(|v| v.as_u64()).max().unwrap_or(0)));
    let margin = side.map_or(128, |s| (s / 3).clamp(48, 640)) as u32;
    let source = slot.with_app(ctx, |a| a.generative_source(if p.kind == Generate::Fill { margin } else { 0 }))?;
    let model = shell.generation.comfy.settings.model;
    let seed = uuid::Uuid::new_v4().as_u64_pair().0 >> 12;
    let [_, _, w, h] = source.rect;
    let (task, mask) = match p.kind {
        Generate::Fill => {
            let mask = source.mask.ok_or("Make a selection first: Generative Fill fills it.")?;
            let img = fill_input(&source.image, &mask);
            let mask = soften(mask, w as usize, h as usize);
            let resolution = workflow::snap((f64::from(w) * f64::from(h)).sqrt().clamp(1024.0, 1440.0) as u32);
            (Task::Edit { prompt: workflow::fill_prompt(&p.text), images: vec![img], resolution }, Some(mask))
        }
        Generate::Image => {
            let (gw, gh) = workflow::size_for(w, h, 2048.0 * 1152.0);
            (Task::Generate { prompt: p.text.trim().to_owned(), width: gw, height: gh }, source.mask)
        }
    };
    let id = shell.generation.comfy.submit(Job { task, model, seed });
    let words: String = p.text.split_whitespace().collect::<Vec<_>>().join(" ");
    let short = if words.chars().count() > 48 { format!("{}…", words.chars().take(47).collect::<String>()) } else { words };
    let name = match (p.kind, short.is_empty()) {
        (Generate::Fill, true) => format!("Generative Fill (seed {seed})"),
        _ => format!("{short} (seed {seed})"),
    };
    shell.generation.jobs.push(Running {
        id,
        app: p.app,
        kind: p.kind,
        window: p.window,
        document: source.document,
        rect: source.rect,
        mask,
        name,
        stage: Stage::Queued,
        error: None,
    });
    Ok(())
}

/// The picture Generative Fill sends: the area as it shows (transparency over white), the
/// selection flat grey, at most 1536 px a side.
fn fill_input(image: &egui::ColorImage, mask: &[u8]) -> image::RgbaImage {
    let [w, h] = image.size;
    let mut out = image::RgbaImage::new(w as u32, h as u32);
    for (i, (px, c)) in out.pixels_mut().zip(&image.pixels).enumerate() {
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        let over = |v: u8| ((u32::from(v) * u32::from(a) + 255 * (255 - u32::from(a))) / 255) as u8;
        px.0 = if mask.get(i).copied().unwrap_or(0) >= 128 {
            let [r, g, b] = workflow::FILL_GREY;
            [r, g, b, 255]
        } else {
            [over(r), over(g), over(b), 255]
        };
    }
    let longest = w.max(h) as u32;
    if longest > 1536 {
        let s = 1536.0 / longest as f32;
        image::imageops::resize(&out, ((w as f32 * s) as u32).max(1), ((h as f32 * s) as u32).max(1), image::imageops::FilterType::Triangle)
    } else {
        out
    }
}

/// A hard-edged selection gets a slightly soft edge for the layer mask, so the new pixels don't
/// show a seam against the old ones (as Photoshop does). Soft selections stay as they are.
fn soften(mask: Vec<u8>, w: usize, h: usize) -> Vec<u8> {
    if mask.len() != w * h || mask.iter().any(|&m| m != 0 && m != 255) {
        return mask;
    }
    let r = (w.min(h) / 100).max(2);
    // Two box blurs are close to a Gaussian.
    let mut m: Vec<u16> = mask.iter().map(|&v| u16::from(v)).collect();
    for _ in 0..2 {
        for (len, stride, count, step) in [(w, 1, h, w), (h, w, w, 1)] {
            let mut line = vec![0u16; len];
            for i in 0..count {
                let base = i * step;
                for (j, out) in line.iter_mut().enumerate() {
                    let (a, b) = (j.saturating_sub(r), (j + r).min(len - 1));
                    let sum: u32 = (a..=b).map(|k| u32::from(m[base + k * stride])).sum();
                    *out = (sum / (b - a + 1) as u32) as u16;
                }
                for (j, v) in line.iter().enumerate() {
                    m[base + j * stride] = *v;
                }
            }
        }
    }
    m.into_iter().map(|v| v.min(255) as u8).collect()
}

/// The jobs started in this window: what they are doing, Cancel; failures until closed.
fn progress(shell: &mut Shell, ctx: &Context, viewport: ViewportId) {
    let c = ShellColors::home();
    let mut cancel = Vec::new();
    let mut dismiss = Vec::new();
    let jobs: Vec<&Running> = shell.generation.jobs.iter().filter(|j| j.window == viewport).collect();
    if jobs.is_empty() {
        return;
    }
    let screen = ctx.content_rect();
    egui::Area::new(Id::new("septet-generate-progress"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.right_bottom() - egui::vec2(356.0, 24.0))
        .pivot(egui::Align2::LEFT_BOTTOM)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(320.0);
                for j in &jobs {
                    ui.label(RichText::new(format!("{} in {}", title(j.kind), j.app.name())).font(theme::semibold(13.0)));
                    match &j.error {
                        Some(e) => {
                            ui.label(RichText::new(e).color(ui.visuals().error_fg_color));
                            if ui.button("Close").clicked() {
                                dismiss.push(j.id);
                            }
                        }
                        None => {
                            let (text, fraction) = match &j.stage {
                                Stage::Queued => ("Waiting…".to_owned(), None),
                                Stage::Busy(what) => (format!("{what}…"), None),
                                Stage::Step { done, of } => (format!("Step {done} of {of}"), Some(*done as f32 / (*of).max(1) as f32)),
                            };
                            ui.label(RichText::new(text).color(c.text_dim));
                            ui.horizontal(|ui| {
                                match fraction {
                                    Some(f) => ui.add(egui::ProgressBar::new(f).desired_width(230.0)),
                                    None => ui.add(egui::ProgressBar::new(0.0).animate(true).desired_width(230.0)),
                                };
                                if ui.button("Cancel").clicked() {
                                    cancel.push(j.id);
                                }
                            });
                        }
                    }
                    ui.add_space(6.0);
                }
            });
        });
    for id in cancel {
        shell.generation.comfy.cancel(id);
    }
    shell.generation.jobs.retain(|j| !dismiss.contains(&j.id));
    if shell.generation.busy() {
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fill_input_greys_the_selection_and_flattens_over_white() {
        let image = egui::ColorImage::new([2, 1], vec![egui::Color32::TRANSPARENT, egui::Color32::from_rgb(10, 20, 30)]);
        let out = fill_input(&image, &[0, 255]);
        assert_eq!(out.get_pixel(0, 0).0, [255, 255, 255, 255]);
        assert_eq!(out.get_pixel(1, 0).0, [128, 128, 128, 255]);
        let big = egui::ColorImage::new([3000, 1000], vec![egui::Color32::BLACK; 3_000_000]);
        assert_eq!(fill_input(&big, &[]).dimensions(), (1536, 512));
    }

    #[test]
    fn hard_selections_get_a_soft_edge_soft_ones_stay() {
        let (w, h) = (40, 20);
        let hard: Vec<u8> = (0..w * h).map(|i| if i % w < 20 { 255 } else { 0 }).collect();
        let soft = soften(hard.clone(), w, h);
        assert_eq!((soft[5 * w + 2], soft[5 * w + 37]), (255, 0), "far from the edge: unchanged");
        assert!(soft[5 * w + 19] > 0 && soft[5 * w + 19] < 255 && soft[5 * w + 20] > 0, "a ramp across the edge");
        let feathered: Vec<u8> = hard.iter().map(|&v| v / 2).collect();
        assert_eq!(soften(feathered.clone(), w, h), feathered);
    }
}

/// By hand, with a real ComfyUI and the models (`SEPTET_REAL_COMFY=http://127.0.0.1:8188`,
/// `SEPTET_REAL_IMAGE=<png>`, `SEPTET_REAL_OUT=<dir>`): Generative Fill adds into and removes
/// from an elliptic selection, Generate Image makes a picture. Look at the files it writes.
#[cfg(test)]
mod real {
    use super::*;
    use crate::comfy::{Model, Settings};

    #[test]
    #[ignore]
    fn generative_fill_and_generate_with_a_real_comfyui() {
        let (Some(address), Some(input), Some(out)) =
            (std::env::var_os("SEPTET_REAL_COMFY"), std::env::var_os("SEPTET_REAL_IMAGE"), std::env::var_os("SEPTET_REAL_OUT"))
        else {
            panic!("set SEPTET_REAL_COMFY, SEPTET_REAL_IMAGE and SEPTET_REAL_OUT");
        };
        let out = std::path::PathBuf::from(out);
        std::fs::create_dir_all(&out).unwrap();
        let src = image::open(input).unwrap().to_rgba8();
        let ctx = egui::Context::default();
        let settings = Settings { address: address.to_string_lossy().into_owned(), autostart: false, ..Default::default() };
        let mut comfy = Comfy::with_settings(&ctx, settings);
        // (name, prompt, selection centre and radii)
        for (name, prompt, (cx, cy, rx, ry)) in
            [("add", "a small green mint leaf lying on the white table", (640.0, 650.0, 90.0, 70.0)), ("remove", "", (390.0, 160.0, 230.0, 110.0))]
        {
            let (w, h) = src.dimensions();
            let mask: Vec<u8> = (0..h)
                .flat_map(|y| (0..w).map(move |x| (x, y)))
                .map(|(x, y)| if ((x as f32 - cx) / rx).powi(2) + ((y as f32 - cy) / ry).powi(2) <= 1.0 { 255 } else { 0 })
                .collect();
            let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], src.as_raw());
            let input = fill_input(&image, &mask);
            input.save(out.join(format!("{name}-input.png"))).unwrap();
            let task = Task::Edit { prompt: workflow::fill_prompt(prompt), images: vec![input], resolution: 1024 };
            let t = std::time::Instant::now();
            let id = comfy.submit(Job { task, model: Model::Turbo, seed: 42 });
            let result = wait(&comfy, id).unwrap();
            eprintln!("{name}: {:?} in {:.0} s", result.dimensions(), t.elapsed().as_secs_f32());
            // What Photocraft does with it: scaled to the area, kept only inside the selection.
            let scaled = image::imageops::resize(&result, w, h, image::imageops::FilterType::Lanczos3);
            let mut done = src.clone();
            for (i, (d, s)) in done.pixels_mut().zip(scaled.pixels()).enumerate() {
                let a = u32::from(mask[i]);
                for k in 0..3 {
                    d.0[k] = ((u32::from(s.0[k]) * a + u32::from(d.0[k]) * (255 - a)) / 255) as u8;
                }
            }
            result.save(out.join(format!("{name}-raw.png"))).unwrap();
            done.save(out.join(format!("{name}-done.png"))).unwrap();
        }
        let t = std::time::Instant::now();
        let id = comfy.submit(Job {
            task: Task::Generate {
                prompt: "A wide photograph of a red lighthouse on a green cliff above a calm sea at dusk, warm light.".into(),
                width: 1024,
                height: 576,
            },
            model: Model::Turbo,
            seed: 7,
        });
        let img = wait(&comfy, id).unwrap();
        eprintln!("generate: {:?} in {:.0} s", img.dimensions(), t.elapsed().as_secs_f32());
        img.save(out.join("generate.png")).unwrap();
        comfy.shutdown();
    }

    fn wait(comfy: &Comfy, id: JobId) -> Result<image::RgbaImage, String> {
        loop {
            for e in comfy.poll() {
                match e {
                    Event::Done(i, img) if i == id => return Ok(img),
                    Event::Failed(i, e) if i == id => return Err(e),
                    Event::Stage(_, s) => eprintln!("  {s:?}"),
                    _ => {}
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}
