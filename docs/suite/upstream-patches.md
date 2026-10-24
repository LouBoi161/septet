# Unsere Änderungen in Upstream-Dateien

Was Septet in den Dateien der ArtCraft-Apps ändert, mit Grund. Bei Konflikten im Upstream-Abgleich
(`scripts/sync-upstream.sh`) hier nachsehen, was unsere Seite wollte. Neue Dateien (`embed.rs`, `hosted.rs`, `lib.rs`)
stehen nicht in Konflikten und sind nur kurz genannt.

Stand: 2026-10-24. Bisher nur WordCraft; die sieben älteren Apps folgen (ihre Änderungen stehen bis dahin in den
Merge-Nachrichten der Abgleiche und in `docs/embedding/`).

## WordCraft (seit v0.10.0, Phase B)

Neu: `apps/wordcraft/src/lib.rs` (Bibliothek `wordcraft_embed`), `apps/wordcraft/src/embed.rs` (`Embedded`),
`crates/ui-egui/src/hosted.rs`, `docs/brand/README.md`.

| Datei | Änderung | Grund |
|---|---|---|
| `apps/wordcraft/Cargo.toml` | `[lib] wordcraft_embed`, Abhängigkeit `wordcraft-render` | Septet nutzt das Paket als Bibliothek; Seiten-Render für Claude |
| `apps/wordcraft/src/main.rs` | `mod animations; clipboard; default_apps; file_dialogs; print; region;` → `use wordcraft_embed::{…}` | Die Module liegen jetzt in der Bibliothek, nicht doppelt kompiliert |
| `apps/wordcraft/build.rs` | keine Windows-Ressourcen bei `HOSTED_BUILD` | Icon/VERSIONINFO landeten sonst in `septet.exe` |
| `crates/fonts/Cargo.toml`, `build.rs` | Feature `shared-craft-fonts`, `suite_fonts::bytes_expr` | craft-fonts nur einmal im Binary (A1) |
| `crates/ui-egui/src/lib.rs` | `pub mod hosted;`; Theme: Host-Hell/Dunkel vor dem eigenen, `take_restyle`; kein `set_zoom_factor`, kein `set_fonts`, keine `options_mut` (Tastatur-Zoom, Quit-Shortcut) gehostet; `ui_is_dark` nimmt das Host-Theme; `ui.theme`/`ui.dark` und `view.interfaceTheme` (in `handle_request`) öffnen gehostet Septets Darstellungs-Dialog; `handle_request` öffnet bei `hosted::agent_call` keinen Dialog; `discards_document` ist `pub` | Embed-Vertrag (Schriften, Zoom, Eingabe-Optionen gehören dem Host), A4 (ein Erscheinungsbild), Phase 5 (Claude lässt keine Dialoge offen) |
| `crates/ui-egui/src/theme.rs` | `Tokens::get` und `apply` gehen durch `hosted::with_accent` | Septets Akzentfarbe und „Mehr Kontrast“ |
| `crates/ui-egui/src/chrome.rs` | Titelleiste: kein `StartDrag`/`Maximized` gehostet; Discord-Knopf gehostet weg | Das Fenster gehört Septet; keine Einladung in die Community der Originalautoren |
| `crates/ui-egui/src/backstage.rs` | „Discord community“ gehostet weg | wie oben |
| `crates/ui-egui/src/dialogs.rs` | Info-Dialog gehostet: „Based on WordCraft by the ArtCraft team“, ohne getartcraft.com/Discord | Markenlizenz (`docs/brand/LICENSE-brand.txt`): veränderte Version |
| `README.md`, `NOTICE`, `ATTRIBUTION.md`, `docs/brand/*.svg/png` | Logos gelöscht, Hinweis „modified version“ | Markenlizenz |

Bewusst nicht geändert: `Services` und `WordApp::new` (das Embed setzt `on_dirty` selbst), `control.rs` (der
Steuerkanal läuft gehostet nicht; `Embedded::agent_execute` ruft `WordApp::execute` direkt).
