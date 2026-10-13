//! Opening the library session at launch.

use lightcraft_engine::Session;
use lightcraft_ui_egui::panels::library_problem::LibraryProblem;

/// The library session with the folder face models are kept in (`<config>/models`).
pub fn open_session(in_memory: bool, dir: Option<std::path::PathBuf>, seed_demo: bool) -> (Session, Option<LibraryProblem>) {
    let (mut s, problem) = open_library_session(in_memory, dir, seed_demo);
    s.face_models_dir = lightcraft_engine::config::default_face_models_dir();
    // Control requests are answered on the UI thread: Add to Apple Photos must not wait there
    s.apple_photos_background = true;
    (s, problem)
}

/// The persistent library session, or with `--memory` an in-memory one (demo photos).
///
/// If the library can't be opened the session is empty and in memory — never seeded with the
/// demo photos, never written anywhere — and the problem is returned: the window then says so
/// and offers Try Again / Choose Another Library… / Continue Without Saving / Quit (issue #100).
fn open_library_session(in_memory: bool, dir: Option<std::path::PathBuf>, seed_demo: bool) -> (Session, Option<LibraryProblem>) {
    if in_memory {
        return (if seed_demo { Session::with_demo() } else { Session::new() }.with_fs().with_default_denoise_models().with_system_clock(), None);
    }
    let unopened = || Session::new().with_fs().with_default_denoise_models().with_system_clock();
    let Some(dir) = dir else {
        log::warn!("no library location (set --library or LIGHTCRAFT_LIBRARY)");
        return (unopened(), Some(LibraryProblem::new("", "There is no home folder to keep the library in. Choose a folder for it.")));
    };
    let t0 = std::time::Instant::now();
    let mut s = Session::new().with_fs().with_default_denoise_models().with_system_clock();
    match s.open_library(&dir, seed_demo) {
        Ok(r) => {
            let (replayed, torn) = (r.replayed, r.torn_bytes);
            log::info!(
                "library {}: {} photos, {replayed} log records replayed{} ({:.1} ms)",
                dir.display(),
                s.catalog.len(),
                if torn > 0 { ", torn tail repaired" } else { "" },
                t0.elapsed().as_secs_f64() * 1000.0
            );
            (s, None)
        }
        Err(e) => {
            log::error!("can't open library {}: {e}", dir.display());
            (unopened(), Some(LibraryProblem::new(dir.to_string_lossy(), e.to_string())))
        }
    }
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

#[cfg(test)]
mod tests {
    /// Issue #236: the window answers control requests on its UI thread, so the desktop session
    /// adds to Apple Photos on a worker (headless sessions wait instead).
    #[test]
    fn the_desktop_session_adds_to_apple_photos_in_the_background() {
        let (s, problem) = super::open_session(true, None, false);
        assert!(problem.is_none());
        assert!(s.apple_photos_background);
        assert!(!lightcraft_engine::Session::new().apple_photos_background);
    }
}
