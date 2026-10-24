//! Making and editing images with a local ComfyUI (Qwen Image 2.1), for every app in Septet
//! (`docs/suite/PLAN.md`, Phase H).
//!
//! ComfyUI stays a process of its own that Septet talks to over HTTP, so a crash or an
//! out-of-memory in it never takes Septet down (and it is GPL, Septet does not ship it). One
//! worker thread runs one job at a time: it uses a ComfyUI that is already running, or starts
//! the user's own installation (keeping graphics memory for Septet's apps, at the lowest CPU
//! priority, first in line for the kernel when memory runs out), checks the free graphics memory
//! and says who holds it, shows the steps, cancels, and unloads the models when idle. A ComfyUI
//! Septet started is ended after a longer idle time and when Septet quits; one that was already
//! running is only unloaded, after every job.

pub mod client;
pub mod dialog;
pub mod launch;
pub mod mock;
pub mod ollama;
pub mod workflow;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Child;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use client::Client;
pub use workflow::Model;

/// Settings › Image generation (saved as `comfy/settings.json` in Septet's settings folder).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Where ComfyUI answers (and where Septet starts it).
    pub address: String,
    /// Its `main.py`, when it is not found on its own.
    pub main: Option<PathBuf>,
    /// Start ComfyUI for a job when it is not running.
    pub autostart: bool,
    /// Graphics memory kept free for Septet's apps when Septet starts ComfyUI, in GB; `None`:
    /// worked out from the open apps.
    pub reserve_gb: Option<f32>,
    /// Unload the models after this many idle minutes.
    pub unload_after_min: u32,
    /// End a ComfyUI Septet started after this many idle minutes.
    pub stop_after_min: u32,
    pub model: Model,
    /// Before a job that doesn't fit, unload Ollama's models (it loads them again when asked).
    pub unload_ollama: bool,
    /// Where Ollama answers.
    pub ollama_address: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            address: "http://127.0.0.1:8188".into(),
            main: None,
            autostart: true,
            reserve_gb: None,
            unload_after_min: 2,
            stop_after_min: 10,
            model: Model::default(),
            unload_ollama: true,
            ollama_address: std::env::var("OLLAMA_HOST")
                .ok()
                .filter(|h| !h.is_empty())
                .map(|h| if h.contains("://") { h } else { format!("http://{h}") })
                .unwrap_or_else(|| "http://127.0.0.1:11434".into()),
        }
    }
}

impl Settings {
    fn file() -> Option<PathBuf> {
        crate::recent::config_dir().map(|d| d.join("comfy").join("settings.json"))
    }

    pub fn load() -> Self {
        Self::file().and_then(|f| std::fs::read(f).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(file) = Self::file() else { return };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(file, json);
        }
    }
}

/// What to make.
#[derive(Clone, Debug)]
pub enum Task {
    /// Text to image at this size (snapped to what the model takes).
    Generate { prompt: String, width: u32, height: u32 },
    /// Edit `images[0]` by the instruction `prompt` (the others are references); the result
    /// has image 1's aspect at about `resolution`² pixels.
    Edit { prompt: String, images: Vec<image::RgbaImage>, resolution: u32 },
}

#[derive(Clone, Debug)]
pub struct Job {
    pub task: Task,
    pub model: Model,
    pub seed: u64,
}

pub type JobId = u64;

/// Where a job is.
#[derive(Clone, Debug, PartialEq)]
pub enum Stage {
    Queued,
    /// Something without a step count: starting ComfyUI, loading the models, decoding.
    Busy(String),
    /// Sampling step `done` of `of`.
    Step {
        done: u32,
        of: u32,
    },
}

#[derive(Debug)]
pub enum Event {
    Stage(JobId, Stage),
    Done(JobId, image::RgbaImage),
    Failed(JobId, String),
}

/// Where a job's result goes: the UI's events, or a thread that waits for it ([`Handle::run`]).
type Reply = Option<Sender<Result<image::RgbaImage, String>>>;

enum Msg {
    Run(JobId, Job, Reply),
    Cancel(JobId),
    Settings(Settings),
    /// Graphics memory to keep for the apps, in GB (from the open apps).
    Reserve(f32),
    /// Something else needs the graphics card now: unload.
    Unload,
    Shutdown,
}

/// The UI's side of the worker.
pub struct Comfy {
    pub settings: Settings,
    tx: Sender<Msg>,
    rx: Receiver<Event>,
    next: JobId,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Comfy {
    pub fn new(ctx: &egui::Context) -> Self {
        Self::with_settings(ctx, Settings::load())
    }

    pub fn with_settings(ctx: &egui::Context, settings: Settings) -> Self {
        let (tx, worker_rx) = channel();
        let (worker_tx, rx) = channel();
        let ctx = ctx.clone();
        let s = settings.clone();
        let worker = std::thread::Builder::new()
            .name("septet-comfy".into())
            .spawn(move || Worker::new(s, worker_tx, ctx).run(worker_rx))
            .map_err(|e| eprintln!("septet: no image generation thread: {e}"))
            .ok();
        Comfy { settings, tx, rx, next: 1, worker }
    }

    pub fn submit(&mut self, job: Job) -> JobId {
        let id = self.next;
        self.next += 1;
        let _ = self.tx.send(Msg::Run(id, job, None));
        id
    }

    /// For other threads (Claude's tools): run jobs in the same queue.
    pub fn handle(&self) -> Handle {
        Handle { tx: self.tx.clone() }
    }

    pub fn cancel(&self, id: JobId) {
        let _ = self.tx.send(Msg::Cancel(id));
    }

    /// Save the settings and hand them to the worker.
    pub fn apply_settings(&self) {
        self.settings.save();
        let _ = self.tx.send(Msg::Settings(self.settings.clone()));
    }

    /// Graphics memory the open apps may need, in GB (`reserve_for`).
    pub fn set_reserve(&self, gb: f32) {
        let _ = self.tx.send(Msg::Reserve(gb));
    }

    /// Something else needs the graphics card now (a GPU error, a big render): unload.
    pub fn unload(&self) {
        let _ = self.tx.send(Msg::Unload);
    }

    pub fn poll(&self) -> Vec<Event> {
        self.rx.try_iter().collect()
    }

    /// End a ComfyUI Septet started (Septet quits). Waits a few seconds at most.
    pub fn shutdown(&mut self) {
        let _ = self.tx.send(Msg::Shutdown);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

/// Runs jobs from another thread, in the UI's queue.
#[derive(Clone)]
pub struct Handle {
    tx: Sender<Msg>,
}

impl Handle {
    /// Run `job` and wait for its image (minutes, the first time).
    pub fn run(&self, job: Job) -> Result<image::RgbaImage, String> {
        let (tx, rx) = channel();
        self.tx.send(Msg::Run(0, job, Some(tx))).map_err(|_| "Image generation has stopped.".to_owned())?;
        rx.recv().map_err(|_| "Image generation has stopped.".to_owned())?
    }
}

/// Graphics memory to keep for Septet with these apps open, in GB: what each app may hold on
/// the graphics card (Photocraft's tile and effect budgets, Effectcraft's frame caches, …) plus
/// the shell and a margin.
pub fn reserve_for(apps: &[&str]) -> f32 {
    let per_app: f32 = apps
        .iter()
        .map(|a| match *a {
            "Photocraft" => 5.5,
            "Effectcraft" => 2.3,
            "Filmcraft" | "Lightcraft" => 1.0,
            _ => 0.3,
        })
        .sum();
    1.5 + per_app
}

/// The smallest share of the card ComfyUI gets, whatever the apps reserve: below it, Qwen Image
/// does not run at all.
const MIN_COMFY_GB: f32 = 4.0;
/// Free graphics memory a job needs to start in a ComfyUI that has nothing loaded, in MiB.
const MIN_FREE_MIB: u64 = 4096;

struct Worker {
    settings: Settings,
    client: Client,
    tx: Sender<Event>,
    ctx: egui::Context,
    /// The ComfyUI Septet started, if it did.
    child: Option<Child>,
    /// When the last job in Septet's own ComfyUI ended.
    idle_since: Option<Instant>,
    /// Septet's own ComfyUI still holds the models of the last job.
    loaded: bool,
    reserve_gb: f32,
    pending: VecDeque<(JobId, Job, Reply)>,
    cancelled: Vec<JobId>,
    /// Graphs ComfyUI was checked to have every node and model for (by their class and file names).
    checked: Vec<String>,
    shutdown: bool,
}

impl Worker {
    fn new(settings: Settings, tx: Sender<Event>, ctx: egui::Context) -> Self {
        let client = Client::new(&settings.address);
        Worker {
            settings,
            client,
            tx,
            ctx,
            child: None,
            idle_since: None,
            loaded: false,
            reserve_gb: reserve_for(&[]),
            pending: VecDeque::new(),
            cancelled: Vec::new(),
            checked: Vec::new(),
            shutdown: false,
        }
    }

    fn send(&self, e: Event) {
        let _ = self.tx.send(e);
        self.ctx.request_repaint();
    }

    fn run(mut self, rx: Receiver<Msg>) {
        while !self.shutdown {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(m) => self.handle(m),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            while let Some((id, job, reply)) = self.pending.pop_front() {
                let result = if self.cancelled.contains(&id) { Err("Cancelled.".into()) } else { self.job(id, &job, &rx) };
                match reply {
                    Some(reply) => {
                        let _ = reply.send(result);
                    }
                    None => self.send(match result {
                        Ok(img) => Event::Done(id, img),
                        Err(e) => Event::Failed(id, e),
                    }),
                }
                self.after_job();
                if self.shutdown {
                    break;
                }
            }
            self.idle();
        }
        if let Some(child) = &mut self.child {
            launch::stop(child);
        }
    }

    fn handle(&mut self, m: Msg) {
        match m {
            Msg::Run(id, job, reply) => {
                if reply.is_none() {
                    self.send(Event::Stage(id, Stage::Queued));
                }
                self.pending.push_back((id, job, reply));
            }
            Msg::Cancel(id) => self.cancelled.push(id),
            Msg::Settings(s) => {
                if s.address != self.settings.address {
                    self.client = Client::new(&s.address);
                    self.checked.clear();
                }
                self.settings = s;
            }
            Msg::Reserve(gb) => self.reserve_gb = gb,
            Msg::Unload => self.unload(),
            Msg::Shutdown => {
                self.shutdown = true;
                for (id, _, reply) in std::mem::take(&mut self.pending) {
                    match reply {
                        Some(r) => {
                            let _ = r.send(Err("Septet is closing.".into()));
                        }
                        None => self.send(Event::Failed(id, "Septet is closing.".into())),
                    }
                }
            }
        }
    }

    /// Messages that arrive while a job runs: cancel it, queue the others.
    fn drain(&mut self, rx: &Receiver<Msg>) {
        while let Ok(m) = rx.try_recv() {
            self.handle(m);
        }
    }

    fn unload(&mut self) {
        if self.loaded {
            let _ = self.client.free();
            self.loaded = false;
        }
    }

    /// After a job: a ComfyUI the user runs is unloaded at once (it was started without Septet's
    /// reserve); Septet's own keeps the models until it has been idle a while.
    fn after_job(&mut self) {
        if self.child.is_none() {
            let _ = self.client.free();
        } else {
            self.idle_since = Some(Instant::now());
            self.loaded = true;
        }
    }

    fn idle(&mut self) {
        if let Some(child) = &mut self.child
            && matches!(child.try_wait(), Ok(Some(_)))
        {
            self.child = None;
        }
        let Some(since) = self.idle_since.filter(|_| self.child.is_some()) else {
            (self.idle_since, self.loaded) = (None, false);
            return;
        };
        let minutes = |m: u32| Duration::from_secs(u64::from(m.max(1)) * 60);
        if self.loaded && since.elapsed() >= minutes(self.settings.unload_after_min) {
            self.unload();
        }
        if since.elapsed() >= minutes(self.settings.stop_after_min) {
            if let Some(child) = &mut self.child {
                launch::stop(child);
            }
            (self.child, self.idle_since, self.loaded) = (None, None, false);
        }
    }

    fn stage(&self, id: JobId, s: Stage) {
        self.send(Event::Stage(id, s));
    }

    fn job(&mut self, id: JobId, job: &Job, rx: &Receiver<Msg>) -> Result<image::RgbaImage, String> {
        self.ensure_server(id, rx)?;
        let graph = match &job.task {
            Task::Generate { prompt, width, height } => workflow::generate(job.model, prompt, job.seed, *width, *height),
            Task::Edit { prompt, images, resolution } => {
                self.stage(id, Stage::Busy("Sending the image".into()));
                let mut names = Vec::new();
                for (i, img) in images.iter().enumerate() {
                    let mut png = Vec::new();
                    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| e.to_string())?;
                    // The same names every time: ComfyUI's input folder doesn't fill up.
                    names.push(self.client.upload_png(&format!("septet-input-{}.png", i + 1), &png)?);
                }
                workflow::edit(job.model, prompt, job.seed, &names, *resolution)
            }
        };
        self.check_graph(&graph)?;
        self.check_memory(id)?;
        let mut progress = self.client.progress();
        let prompt = self.client.queue(&graph)?;
        self.stage(id, Stage::Busy("Loading the model".into()));
        let started = Instant::now();
        let mut last_poll = Instant::now() - Duration::from_secs(1);
        loop {
            self.drain(rx);
            if self.cancelled.contains(&id) || self.shutdown {
                self.client.cancel(&prompt);
                return Err("Cancelled.".into());
            }
            if let Some(p) = &mut progress {
                match p.messages() {
                    Some(msgs) => {
                        for m in msgs.iter().filter(|m| m["data"]["prompt_id"] == prompt.as_str()) {
                            if let Some(s) = stage_of(m) {
                                self.stage(id, s);
                            }
                        }
                    }
                    None => progress = None,
                }
            } else {
                std::thread::sleep(Duration::from_millis(200));
            }
            if last_poll.elapsed() >= Duration::from_millis(700) {
                last_poll = Instant::now();
                if let Some(entry) = self.client.history(&prompt)? {
                    let files = Client::outputs(&entry)?;
                    let file = files.first().ok_or("ComfyUI finished without an image.")?;
                    let png = self.client.view(file)?;
                    return image::load_from_memory(&png).map(|i| i.to_rgba8()).map_err(|e| format!("The result is not an image: {e}"));
                }
            }
            if started.elapsed() > Duration::from_secs(30 * 60) {
                self.client.cancel(&prompt);
                return Err("ComfyUI took more than 30 minutes; the job was stopped.".into());
            }
        }
    }

    /// A ComfyUI to talk to: the one at the address, or one Septet starts.
    fn ensure_server(&mut self, id: JobId, rx: &Receiver<Msg>) -> Result<(), String> {
        if self.client.up() {
            return Ok(());
        }
        if let Some(child) = &mut self.child
            && matches!(child.try_wait(), Ok(Some(_)))
        {
            self.child = None;
        }
        if self.child.is_none() {
            if !self.settings.autostart {
                return Err(format!("ComfyUI is not running at {}. Start it, or let Septet start it (Settings › Image generation).", self.client.base()));
            }
            let install = launch::find(self.settings.main.as_deref()).ok_or(
                "Septet found no ComfyUI to start. Install ComfyUI (with the Qwen Image 2.1 models), or choose its main.py in Settings › Image generation.",
            )?;
            let port = launch::port(&self.settings.address).ok_or("The ComfyUI address needs a port, such as http://127.0.0.1:8188.")?;
            let total_gb = launch::gpu_memory().map(|((_, total), _)| total as f32 / 1024.0);
            let wanted = self.settings.reserve_gb.unwrap_or(self.reserve_gb);
            let reserve = total_gb.map_or(wanted, |t| wanted.min((t - MIN_COMFY_GB).max(0.5)));
            self.stage(id, Stage::Busy("Starting ComfyUI".into()));
            let log = crate::recent::data_dir("ComfyUI").map(|d| d.join("comfyui.log"));
            let child = launch::start(&install, port, reserve, log.as_deref()).map_err(|e| format!("ComfyUI did not start: {e}"))?;
            self.child = Some(child);
        }
        let since = Instant::now();
        while since.elapsed() < Duration::from_secs(300) {
            self.drain(rx);
            if self.cancelled.contains(&id) || self.shutdown {
                return Err("Cancelled.".into());
            }
            if let Some(child) = &mut self.child
                && let Ok(Some(status)) = child.try_wait()
            {
                self.child = None;
                let log = crate::recent::data_dir("ComfyUI").map(|d| d.join("comfyui.log"));
                let tail =
                    log.and_then(|l| std::fs::read_to_string(l).ok()).map(|t| t.lines().rev().take(3).collect::<Vec<_>>().join(" / ")).unwrap_or_default();
                return Err(format!("ComfyUI stopped while starting ({status}). {tail}"));
            }
            if self.client.up() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        Err("ComfyUI did not answer within five minutes of starting.".into())
    }

    fn check_graph(&mut self, graph: &Value) -> Result<(), String> {
        let key = graph_key(graph);
        if self.checked.contains(&key) {
            return Ok(());
        }
        let missing = self.client.missing(graph)?;
        if !missing.is_empty() {
            return Err(format!("ComfyUI lacks {}.", missing.join(", ")));
        }
        self.checked.push(key);
        Ok(())
    }

    /// Enough free graphics memory (as ComfyUI sees its card) when it has nothing loaded; else
    /// who holds it.
    fn check_memory(&self, id: JobId) -> Result<(), String> {
        if self.loaded {
            return Ok(());
        }
        let Some(mut free) = self.client.vram_free_mib() else { return Ok(()) };
        if free >= MIN_FREE_MIB {
            return Ok(());
        }
        // What ComfyUI itself still holds (from another client's job) it gives back first.
        let wait_free = |seconds: u64| {
            let since = Instant::now();
            loop {
                let free = self.client.vram_free_mib().unwrap_or(0);
                if free >= MIN_FREE_MIB || since.elapsed() >= Duration::from_secs(seconds) {
                    return free;
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        };
        self.stage(id, Stage::Busy("Making room on the graphics card".into()));
        let _ = self.client.free();
        free = wait_free(5);
        if free >= MIN_FREE_MIB {
            return Ok(());
        }
        let models = if self.settings.unload_ollama { ollama::loaded(&self.settings.ollama_address) } else { Vec::new() };
        if !models.is_empty() {
            self.stage(id, Stage::Busy("Unloading Ollama's models".into()));
            for m in &models {
                ollama::unload(&self.settings.ollama_address, m);
            }
            free = wait_free(20);
            if free >= MIN_FREE_MIB {
                return Ok(());
            }
        }
        let users = launch::gpu_memory().map(|(_, u)| u).unwrap_or_default();
        let others: Vec<String> = users
            .iter()
            .filter(|(n, m)| *m >= 200 && !n.starts_with("python") && !n.contains("septet"))
            .take(3)
            .map(|(n, m)| format!("{n} ({:.1} GB)", *m as f64 / 1024.0))
            .collect();
        let who = if others.is_empty() { String::new() } else { format!(" In use by {}.", others.join(", ")) };
        Err(format!("Only {:.1} GB of graphics memory are free; image generation needs about {} GB.{who}", free as f64 / 1024.0, MIN_FREE_MIB / 1024))
    }
}

/// What a ComfyUI WebSocket message says about the job.
fn stage_of(m: &Value) -> Option<Stage> {
    let d = &m["data"];
    match m["type"].as_str()? {
        "progress" => Some(Stage::Step { done: d["value"].as_u64()? as u32, of: d["max"].as_u64()? as u32 }),
        "executing" => Some(Stage::Busy(
            match d["node"].as_str()? {
                "1" | "2" | "3" | "11" => "Loading the model",
                "4" => "Reading the prompt",
                "7" | workflow::SAVE_NODE => "Finishing",
                _ => return None,
            }
            .into(),
        )),
        _ => None,
    }
}

/// The classes and files a graph needs, as one string.
fn graph_key(graph: &Value) -> String {
    let mut parts: Vec<String> = graph
        .as_object()
        .into_iter()
        .flatten()
        .flat_map(|(_, n)| {
            let class = n["class_type"].as_str().unwrap_or("").to_owned();
            let files =
                n["inputs"].as_object().into_iter().flatten().filter_map(|(_, v)| v.as_str().filter(|s| s.ends_with(".safetensors")).map(str::to_owned));
            std::iter::once(class).chain(files)
        })
        .collect();
    parts.sort();
    parts.dedup();
    parts.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_default() {
        let s = Settings { reserve_gb: Some(3.0), model: Model::Base, ..Default::default() };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        let partial: Settings = serde_json::from_str(r#"{"autostart": false}"#).unwrap();
        assert_eq!((partial.autostart, partial.unload_after_min, partial.address.as_str()), (false, 2, "http://127.0.0.1:8188"));
    }

    #[test]
    fn reserve_grows_with_the_apps() {
        assert_eq!(reserve_for(&[]), 1.5);
        assert!(reserve_for(&["Photocraft", "Vectorcraft"]) > 7.0);
    }

    #[test]
    fn progress_messages() {
        let m = serde_json::json!({"type": "progress", "data": {"value": 3, "max": 8, "prompt_id": "p"}});
        assert_eq!(stage_of(&m), Some(Stage::Step { done: 3, of: 8 }));
        let m = serde_json::json!({"type": "executing", "data": {"node": "1", "prompt_id": "p"}});
        assert_eq!(stage_of(&m), Some(Stage::Busy("Loading the model".into())));
        assert_eq!(stage_of(&serde_json::json!({"type": "status"})), None);
    }

    /// The whole way through a stand-in ComfyUI: queue, steps, result; cancel; a missing model.
    #[test]
    fn jobs_run_against_a_stand_in_server() {
        let server = mock::Server::start().unwrap();
        let ctx = egui::Context::default();
        let settings = Settings { address: server.address(), autostart: false, ollama_address: server.address(), ..Default::default() };
        let mut comfy = Comfy::with_settings(&ctx, settings);
        let id = comfy.submit(Job { task: Task::Generate { prompt: "a red square".into(), width: 512, height: 256 }, model: Model::Turbo, seed: 5 });
        let img = wait(&comfy, id).unwrap();
        assert_eq!(img.dimensions(), (512, 256));
        assert!(server.requests().iter().any(|r| r.starts_with("POST /free")), "an external ComfyUI is unloaded after the job");
        let edit = Job { task: Task::Edit { prompt: "x".into(), images: vec![image::RgbaImage::new(64, 32)], resolution: 1024 }, model: Model::Turbo, seed: 1 };
        let id = comfy.submit(edit);
        assert_eq!(wait(&comfy, id).unwrap().dimensions(), (64, 32), "the stand-in hands back the uploaded image");
        server.set_slow(true);
        let id = comfy.submit(Job { task: Task::Generate { prompt: "slow".into(), width: 512, height: 512 }, model: Model::Turbo, seed: 1 });
        std::thread::sleep(Duration::from_millis(300));
        comfy.cancel(id);
        assert_eq!(wait(&comfy, id).unwrap_err(), "Cancelled.");
        assert!(server.requests().iter().any(|r| r.starts_with("POST /interrupt")));
        server.set_slow(false);
        server.hide_model("qwen_image_2.1_int8_convrot.safetensors");
        let id = comfy.submit(Job { task: Task::Generate { prompt: "x".into(), width: 512, height: 512 }, model: Model::Base, seed: 1 });
        let err = wait(&comfy, id).unwrap_err();
        assert!(err.contains("qwen_image_2.1_int8_convrot.safetensors"), "{err}");
        let from_a_thread = comfy.handle();
        let img = std::thread::spawn(move || {
            from_a_thread.run(Job { task: Task::Generate { prompt: "x".into(), width: 320, height: 256 }, model: Model::Turbo, seed: 2 })
        });
        assert_eq!(img.join().unwrap().unwrap().dimensions(), (320, 256));
        server.set_vram_free_mib(1000);
        let id = comfy.submit(Job { task: Task::Generate { prompt: "x".into(), width: 512, height: 512 }, model: Model::Turbo, seed: 1 });
        assert!(wait(&comfy, id).unwrap_err().starts_with("Only 1.0 GB of graphics memory are free"));
        // Ollama holds the memory: its model is unloaded, then the job runs.
        server.set_ollama_model(Some("gemma"));
        let id = comfy.submit(Job { task: Task::Generate { prompt: "x".into(), width: 512, height: 512 }, model: Model::Turbo, seed: 1 });
        assert!(wait(&comfy, id).is_ok());
        assert!(server.requests().iter().any(|r| r == "POST /api/generate keep_alive=0 gemma"));
        comfy.shutdown();
    }

    fn wait(comfy: &Comfy, id: JobId) -> Result<image::RgbaImage, String> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(20) {
            for e in comfy.poll() {
                match e {
                    Event::Done(i, img) if i == id => return Ok(img),
                    Event::Failed(i, e) if i == id => return Err(e),
                    _ => {}
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Err("timed out".into())
    }
}
