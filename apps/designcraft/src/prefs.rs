//! The persisted UI state (`ui.json`) and engine preferences (`prefs.json`) in the per-user
//! config folder (or the data root, [`designcraft_engine::user_dirs`]). `DESIGNCRAFT_NO_PREFS`
//! turns both off.

use designcraft_ui_egui::DesignApp;

/// `ui.json` in the per-user config folder (`prefs.json` sits beside it).
pub fn prefs_path() -> Option<std::path::PathBuf> {
    designcraft_engine::user_dirs::config_dir().map(|b| b.join("ui.json"))
}

pub fn load_prefs(app: &mut DesignApp) {
    if std::env::var_os("DESIGNCRAFT_NO_PREFS").is_some() {
        return;
    }
    if let Some(p) = prefs_path()
        && let Ok(bytes) = std::fs::read(&p)
        && let Ok(ui) = serde_json::from_slice::<designcraft_ui_egui::UiState>(&bytes)
    {
        app.ui = ui;
    }
    // Engine preferences (Preferences dialog, favourites…) live beside the UI state.
    if let Some(p) = prefs_path().map(|p| p.with_file_name("prefs.json"))
        && let Ok(bytes) = std::fs::read(&p)
        && let Ok(prefs) = serde_json::from_slice::<designcraft_engine::Prefs>(&bytes)
    {
        app.session.prefs = prefs;
    }
}

pub fn save_prefs(app: &DesignApp) {
    if std::env::var_os("DESIGNCRAFT_NO_PREFS").is_some() {
        return;
    }
    if let Some(p) = prefs_path() {
        let _ = std::fs::create_dir_all(p.parent().unwrap_or(std::path::Path::new(".")));
        if let Ok(bytes) = serde_json::to_vec_pretty(&app.ui) {
            let _ = std::fs::write(&p, bytes);
        }
        if let Ok(bytes) = serde_json::to_vec_pretty(&app.session.prefs) {
            let _ = std::fs::write(p.with_file_name("prefs.json"), bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_data_root_holds_the_preferences_and_recovery() {
        let standalone = (prefs_path(), designcraft_engine::recovery::default_dir());
        let root = std::env::temp_dir().join(format!("designcraft-data-root-{}", std::process::id())).join("DesignCraft");
        designcraft_ui_egui::hosted::set_data_root(Some(root.clone()));
        assert!(root.is_dir(), "the root is created");
        assert_eq!(prefs_path(), Some(root.join("ui.json")));
        assert_eq!(designcraft_engine::recovery::default_dir(), Some(root.join("Recovery")));
        // The preferences go there and come back from there.
        let mut app = DesignApp::new(designcraft_engine::Session::new(), designcraft_ui_egui::Services::default());
        app.ui.story_editor_size = 21.0;
        app.session.prefs.polygon_sides = 9;
        save_prefs(&app);
        assert!(root.join("ui.json").is_file() && root.join("prefs.json").is_file());
        let mut back = DesignApp::new(designcraft_engine::Session::new(), designcraft_ui_egui::Services::default());
        load_prefs(&mut back);
        assert_eq!((back.ui.story_editor_size, back.session.prefs.polygon_sides), (21.0, 9));
        designcraft_ui_egui::hosted::set_data_root(None);
        assert_eq!((prefs_path(), designcraft_engine::recovery::default_dir()), standalone);
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }
}
