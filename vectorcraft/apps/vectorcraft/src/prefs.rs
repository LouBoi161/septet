//! The UI preferences file (`ui.json`): where it lives, reading it at startup and writing it when
//! the app quits.

use std::path::{Path, PathBuf};

use vectorcraft_engine::cmd::fileio;
use vectorcraft_ui_egui::{UiState, VectorcraftApp};

use crate::prefs_dir;

/// The folder VectorCraft keeps what it stores per user in (the UI preferences, the User Defined
/// swatch and graphic style libraries, the Fonts folder, Data Recovery copies, logs): the data
/// root when the host set one (`hosted::data_root`, a portable install), else the folder of
/// [`prefs_dir::prefs_path_for`] (`VECTORCRAFT_CONFIG_DIR`, or ~/Library/Application
/// Support/VectorCraft on macOS, %APPDATA%\VectorCraft on Windows, $XDG_CONFIG_HOME or
/// ~/.config/vectorcraft on Linux).
pub fn data_dir() -> Option<PathBuf> {
    vectorcraft_ui_egui::hosted::data_root().or_else(|| Some(prefs_dir::prefs_path_for("VectorCraft", "vectorcraft")?.parent()?.to_path_buf()))
}

/// Where UI preferences live: `ui.json` in [`data_dir`].
pub fn prefs_path() -> Option<PathBuf> {
    Some(data_dir()?.join("ui.json"))
}

/// The same place under the project's former name (DrawCraft): read once if there are no
/// VectorCraft preferences yet, so settings survive the rename. Not with a data root, which
/// holds everything the app reads.
fn legacy_prefs_path() -> Option<PathBuf> {
    if vectorcraft_ui_egui::hosted::data_root().is_some() {
        return None;
    }
    prefs_dir::prefs_path_for("DrawCraft", "drawcraft")
}

/// Have the font scans read VectorCraft's own Fonts folder in [`data_dir`], which
/// `text.addFontFiles` copies font files into ([`prefs_dir::install_fonts`] without a data root).
pub fn install_fonts() {
    match vectorcraft_ui_egui::hosted::data_root() {
        Some(root) => vectorcraft_text::set_app_font_dir(root.join("Fonts")),
        None => prefs_dir::install_fonts(),
    }
}

/// The folder `name` in [`data_dir`], for what the user keeps: none in a run without preferences
/// (`VECTORCRAFT_NO_PREFS`), which keeps nothing. Without a data root it is
/// [`prefs_dir`]'s folder of the same name, which `vectorcraft-cli` reads too.
fn store_dir(name: &str) -> Option<PathBuf> {
    if !prefs_enabled() {
        return None;
    }
    Some(data_dir()?.join(name))
}

/// Register the ICC profiles `color.loadProfile` kept in the `Color Profiles` folder (#2174), and
/// keep the ones loaded from now on there. Before anything names a profile.
pub fn attach_profile_store(session: &mut vectorcraft_engine::Session) {
    if let Some(dir) = store_dir("Color Profiles") {
        session.attach_profile_store(dir);
    }
}

/// Install the plug-ins Install Plug-in kept in the `Plug-ins` folder (#2541), and keep the ones
/// installed from now on there.
pub fn attach_plugin_store(session: &mut vectorcraft_engine::Session) {
    if let Some(dir) = store_dir("Plug-ins") {
        session.attach_plugin_store(dir);
    }
}

/// Take the saved PDF presets from the `PDF Presets` folder (#2168), and keep the ones saved from
/// now on there. After the preferences were restored: the folder is the one place the presets live.
pub fn attach_pdf_preset_store(session: &mut vectorcraft_engine::Session) {
    if let Some(dir) = store_dir("PDF Presets") {
        session.attach_pdf_preset_store(dir);
    }
}

/// Runs without preferences (`VECTORCRAFT_NO_PREFS`, agents' test runs) neither read nor write them.
pub fn prefs_enabled() -> bool {
    std::env::var_os("VECTORCRAFT_NO_PREFS").is_none()
}

/// The saved UI preferences, read before the window opens (they hold its size and position).
pub fn read_prefs() -> Option<UiState> {
    if !prefs_enabled() {
        return None;
    }
    let bytes = prefs_path().and_then(|p| std::fs::read(p).ok()).or_else(|| legacy_prefs_path().and_then(|p| std::fs::read(p).ok()))?;
    decode_saved_prefs(&bytes)
}

/// Recover only optional persisted docking metadata. Runtime UiState and command decoding
/// remains strict, and an unreadable workspace layout cannot discard unrelated preferences.
fn decode_saved_prefs(bytes: &[u8]) -> Option<UiState> {
    use serde_json::{Map, Value, json};

    fn recover(object: &mut Map<String, Value>, hidden_key: &str) {
        if let Some(layout) = object.get("docking")
            && serde_json::from_value::<UiState>(json!({"docking": layout})).is_err()
        {
            object.remove("docking");
        }
        if let Some(hidden) = object.get_mut(hidden_key) {
            if let Value::Object(entries) = hidden {
                entries.retain(|panel, location| serde_json::from_value::<UiState>(json!({"docking_hidden": {panel: location}})).is_ok());
            } else {
                object.remove(hidden_key);
            }
        }
    }

    let mut value: Value = serde_json::from_slice(bytes).ok()?;
    let object = value.as_object_mut()?;
    recover(object, "docking_hidden");
    // Saved workspaces and the layout changes kept for each workspace carry the same layouts.
    for key in ["custom_workspaces", "workspace_changes"] {
        if let Some(Value::Array(workspaces)) = object.get_mut(key) {
            for workspace in workspaces {
                if let Some(object) = workspace.as_object_mut() {
                    recover(object, "dockingHidden");
                }
            }
        }
    }
    serde_json::from_value(value).ok()
}

pub fn load_prefs(app: &mut VectorcraftApp, saved: Option<UiState>) {
    if !prefs_enabled() {
        return;
    }
    if let Some(ui) = saved {
        app.ui = ui.sanitized();
    }
    vectorcraft_ui_egui::prefs_dialog::restore(app);
}

pub fn save_prefs(app: &VectorcraftApp) {
    if !prefs_enabled() {
        return;
    }
    if let Some(p) = prefs_path() {
        let _ = std::fs::create_dir_all(p.parent().unwrap_or(Path::new(".")));
        let mut ui = app.ui.clone();
        ui.engine_prefs = app.session.prefs.to_json();
        if let Ok(bytes) = serde_json::to_vec_pretty(&ui) {
            // Preferences are best effort: a failed write keeps the previous file.
            let _ = fileio::write_atomic(&p, &bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreadable_saved_docking_keeps_preferences_and_every_workspace() {
        use serde_json::json;
        let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.run("window.panel.float", json!({"panel":"layers"})).unwrap();
        app.ui.toolbar = false;
        app.ui.engine_prefs = json!({"interfaceLanguage":"fr", "uiBrightness":"light"});
        app.ui.custom_workspaces =
            vec![vectorcraft_ui_egui::workspaces::capture(&app.ui, "Future"), vectorcraft_ui_egui::workspaces::capture(&app.ui, "Valid")];
        app.ui.workspace_changes = vec![vectorcraft_ui_egui::workspaces::capture(&app.ui, "Future")];
        let original = serde_json::to_value(&app.ui).unwrap();
        let valid_location = original["docking_hidden"]["layers"].clone();
        assert!(valid_location.is_object());
        for invalid_layout in [json!({"root":{"FutureNode":{}},"floating":[]}), json!(false)] {
            for invalid_hidden in [json!({"layers":{"placement":{"FuturePlacement":{}}},"swatches":valid_location}), json!(["malformed"])] {
                let mut raw = original.clone();
                raw["docking"] = invalid_layout.clone();
                raw["docking_hidden"] = invalid_hidden.clone();
                raw["custom_workspaces"][0]["docking"] = invalid_layout.clone();
                raw["custom_workspaces"][0]["dockingHidden"] = invalid_hidden.clone();
                raw["workspace_changes"][0]["docking"] = invalid_layout.clone();
                raw["workspace_changes"][0]["dockingHidden"] = invalid_hidden.clone();
                assert!(serde_json::from_value::<UiState>(raw.clone()).is_err(), "runtime serde stays strict");
                let loaded = decode_saved_prefs(&serde_json::to_vec(&raw).unwrap()).unwrap();
                assert_eq!(loaded.engine_prefs, app.ui.engine_prefs);
                assert!(!loaded.toolbar);
                assert!(loaded.docking.is_none());
                assert!(!loaded.docking_hidden.contains_key("layers"));
                if invalid_hidden.is_object() {
                    assert_eq!(serde_json::to_value(loaded.docking_hidden.get("swatches")).unwrap(), valid_location);
                } else {
                    assert!(loaded.docking_hidden.is_empty());
                }
                assert_eq!(loaded.custom_workspaces.len(), 2);
                assert_eq!(loaded.custom_workspaces[0].name, "Future");
                assert!(loaded.custom_workspaces[0].docking.is_none());
                assert!(!loaded.custom_workspaces[0].docking_hidden.contains_key("layers"));
                assert_eq!(serde_json::to_value(&loaded.custom_workspaces[1]).unwrap(), original["custom_workspaces"][1]);
                assert_eq!(loaded.workspace_changes.len(), 1, "a remembered layout is recovered, not the whole file lost");
                assert!(loaded.workspace_changes[0].docking.is_none());
            }
        }
        let roundtrip = decode_saved_prefs(&serde_json::to_vec(&original).unwrap()).unwrap();
        assert_eq!(serde_json::to_value(roundtrip).unwrap(), original);
        assert!(
            decode_saved_prefs(&serde_json::to_vec(&json!({"toolbar":"invalid"})).unwrap()).is_none(),
            "unrelated fields retain normal strict parsing"
        );
    }
}
