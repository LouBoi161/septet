//! DesignCraft's desktop app as a library: the pieces `main.rs` and an embedding host share
//! (platform services, preferences, the start-up of a [`DesignApp`]) and [`Embedded`], DesignCraft
//! as a tab inside another eframe app (Septet).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod embed;
pub mod prefs;
pub mod services;

pub use embed::Embedded;

use designcraft_engine::Session;
use designcraft_ui_egui::DesignApp;

/// The app as a plain launch builds it: a session that reopens what a crashed run left unsaved
/// (and keeps its recovery data current), the desktop services, and the saved preferences.
pub fn new_app() -> DesignApp {
    let mut session = Session::new();
    // Crash recovery: reopen what a previous run left unsaved, then keep it current.
    session.recovery_dir = designcraft_engine::recovery::default_dir();
    let recovered = session.execute("file.recovery.open", &serde_json::json!({})).ok();
    let mut app = DesignApp::new(session, services::services());
    if let Some(n) = recovered.as_ref().and_then(|r| r["opened"].as_array()).map(Vec::len).filter(|n| *n > 0) {
        app.status(format!("Recovered {n} unsaved document{} from the last session.", if n == 1 { "" } else { "s" }));
    }
    prefs::load_prefs(&mut app);
    app
}
