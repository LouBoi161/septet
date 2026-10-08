//! Opening the library session at launch.

use lightcraft_engine::Session;
use lightcraft_ui_egui::panels::library_problem::LibraryProblem;

use crate::prefs::config_dir;

/// The persistent library session, or with `--memory` an in-memory one (demo photos).
///
/// If the library can't be opened the session is empty and in memory — never seeded with the
/// demo photos, never written anywhere — and the problem is returned: the window then says so
/// and offers Try Again / Choose Another Library… / Continue Without Saving / Quit (issue #100).
pub fn open_session(in_memory: bool, dir: Option<std::path::PathBuf>, seed_demo: bool) -> (Session, Option<LibraryProblem>) {
    if in_memory {
        return (if seed_demo { Session::with_demo() } else { Session::new() }.with_fs().with_system_clock(), None);
    }
    let unopened = || Session::new().with_fs().with_system_clock();
    let Some(dir) = dir else {
        eprintln!("lightcraft: no library location (set --library or LIGHTCRAFT_LIBRARY)");
        return (unopened(), Some(LibraryProblem::new("", "There is no home folder to keep the library in. Choose a folder for it.")));
    };
    let t0 = std::time::Instant::now();
    let mut s = Session::new().with_fs().with_system_clock();
    match s.open_library(&dir, seed_demo) {
        Ok(r) => {
            let (replayed, torn) = (r.replayed, r.torn_bytes);
            eprintln!(
                "lightcraft: library {}: {} photos, {replayed} log records replayed{} ({:.1} ms)",
                dir.display(),
                s.catalog.len(),
                if torn > 0 { ", torn tail repaired" } else { "" },
                t0.elapsed().as_secs_f64() * 1000.0
            );
            (s, None)
        }
        Err(e) => {
            eprintln!("lightcraft: can't open library {}: {e}", dir.display());
            (unopened(), Some(LibraryProblem::new(dir.to_string_lossy(), e.to_string())))
        }
    }
}

/// AI masks: the SAM 3 checkpoint (facebook/sam3) in <config>/models/sam3, or LIGHTCRAFT_SAM3_DIR
/// (never required: without it, AI masks offer to download it; see docs/ai-masks.md), and the
/// user's own download locations, one base URL per line (LIGHTCRAFT_SAM3_MIRRORS too).
pub fn configure_segmenter(session: &mut Session) {
    session.segmenter.dir =
        std::env::var_os("LIGHTCRAFT_SAM3_DIR").map(std::path::PathBuf::from).or_else(|| config_dir().map(|d| d.join("models").join("sam3")));
    session.segmenter.mirrors_file = config_dir().map(|d| d.join("models").join("sam3-mirrors.txt"));
}

/// The library to open without `--library`: the one last opened from Settings (unless
/// `LIGHTCRAFT_LIBRARY` says otherwise), else the default location.
pub fn library_dir(prefs: Option<&lightcraft_ui_egui::UiState>) -> Option<std::path::PathBuf> {
    prefs
        .map(|u| u.settings.library_path.clone())
        .filter(|p| !p.is_empty() && std::env::var_os("LIGHTCRAFT_LIBRARY").is_none())
        .map(Into::into)
        .or_else(lightcraft_engine::library::default_dir)
}
