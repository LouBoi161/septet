# Unsere Änderungen in Upstream-Dateien

Was Septet in den Dateien der ArtCraft-Apps ändert, mit Grund. Bei Konflikten im Upstream-Abgleich
(`scripts/sync-upstream.sh`) hier nachsehen, was unsere Seite wollte. Neue Dateien (`embed.rs`, `hosted.rs`, `lib.rs`)
stehen nicht in Konflikten und sind nur kurz genannt.

Stand: 2026-10-25. Bisher WordCraft und DeckCraft; die sieben älteren Apps folgen (ihre Änderungen stehen bis dahin in den
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

## DeckCraft (seit v0.11.1, Phase B)

Neu: `apps/deckcraft/src/lib.rs` (Bibliothek `deckcraft_embed`), `apps/deckcraft/src/embed.rs` (`Embedded`),
`crates/ui-egui/src/hosted.rs`, `docs/brand/README.md`.

| Datei | Änderung | Grund |
|---|---|---|
| `apps/deckcraft/Cargo.toml` | `[lib] deckcraft_embed`, Abhängigkeiten `deckcraft-render`, `deckcraft-model` | Septet nutzt das Paket als Bibliothek; Folien-Render für Claude |
| `apps/deckcraft/src/main.rs` | `mod audio; mod pasteboard;` → `use deckcraft_embed::{audio, pasteboard};` | Die Module liegen jetzt in der Bibliothek |
| `apps/deckcraft/build.rs` | keine Windows-Ressourcen bei `HOSTED_BUILD` | Icon/VERSIONINFO landeten sonst in `septet.exe` |
| `crates/fonts/Cargo.toml`, `build.rs` | Feature `shared-craft-fonts`, `suite_fonts::bytes_expr` | craft-fonts nur einmal im Binary (A1) |
| `crates/ui-egui/src/lib.rs` | `pub mod hosted;`; `logic`: kein `set_zoom_factor`, kein `install_fonts_with`, keine eigenen Datei-Drops gehostet, `hosted::take_restyle`, Theme aus `is_dark()`; neu `SlideApp::is_dark()`; `view.dark` öffnet gehostet Septets Darstellung; `app.links` gehostet nur GitHub; `show::ui` → `show::ui_or_window` | Embed-Vertrag (Schriften, Zoom, Drops gehören dem Host), A4, Vollbild |
| `crates/ui-egui/src/show.rs` | neu `ui_or_window` (gehostet eigenes Vollbild-Fenster, Hinweis im Tab); `Fullscreen(true/false)` gehostet nicht | Septets Fenster trägt andere Tabs |
| `crates/ui-egui/src/theme.rs` | `Tokens::for_brightness` und `get` gehen durch `hosted::with_accent` | Septets Akzentfarbe und „Mehr Kontrast“ |
| `crates/ui-egui/src/uifonts.rs` | `apply` installiert gehostet nichts | Schriften gehören dem Host |
| `crates/ui-egui/src/ribbon.rs` | Titelleiste: kein `StartDrag`/`Maximized` gehostet; Dark-Mode-Knöpfe lesen `is_dark()`; „Learn More“ gehostet → GitHub | Fenster gehört Septet; A4; Markenlizenz |
| `crates/ui-egui/src/backstage.rs` | Discord-Links gehostet weg; Darstellung liest `is_dark()` | Markenlizenz; A4 |
| `crates/ui-egui/src/dialogs.rs` | Info-Dialog gehostet: „Based on DeckCraft by the ArtCraft team“, ohne getartcraft.com/Discord; Einstellungen: `is_dark()`, Regler „Interface size“ gehostet weg | Markenlizenz; Zoom gehört dem Host |
| `README.md`, `NOTICE`, `ATTRIBUTION.md`, `docs/brand/*.svg/png` | Logos gelöscht, Hinweis „modified version“, Zeile für `docs/brand/README.md` | Markenlizenz (`cargo xtask assets` prüft `docs/brand/`) |
| `crates/ui-egui/src/lib.rs` (2026-10-25 abends) | `SlideApp::owns_widget`/`hosted_rect`: gefremdeter Fokus (Septets Chat-Feld) wird beim Textbearbeiten nicht weggenommen; `show_window`, `restore_show_window`, `show_covers_window` (2026-10-26: Septet blendet die Tableiste aus), `replay_pick`; `hosted::Replayable` in `pick_and_open`, `pick_and_run`, `pick_and_reuse`, `save_as_dialog`, `export_as`, `hosted::NoDefer` in `save_all` | Fokus-Bug; Show ruckelte im eigenen Fenster (verdecktes Elternfenster wird von GNOME gedrosselt) → Show jetzt im Septet-Fenster; Dateiwähler blockierten Septet („reagiert nicht“) |
| `crates/ui-egui/src/show.rs` | `ui_or_window`: gehostet Septets Fenster auf Vollbild, Show als `Area` (Foreground) darüber; `restore_fullscreen` | wie oben |
| `crates/ui-egui/src/ribbon.rs` | Hell/Dunkel-Knopf in der Titelleiste und „Dark Mode“ unter Ansicht gehostet weg | ein Darstellungs-Knopf in Septets Leiste |
| `apps/deckcraft/src/pickers.rs` (neu), `Cargo.toml` (`pollster`) | Dateiwähler gehostet nebenläufig (`rfd::AsyncFileDialog`, Elternfenster), Aktion läuft mit der Antwort erneut | Septet blieb stehen, solange ein Wähler offen war |
| `crates/ui-egui/src/show.rs` (2026-10-25 nachts) | `presenter_window`: gehostet als Vollbild auf einem anderen Bildschirm (`with_monitor(hosted::other_screen())`), mit nur einem Bildschirm keine Referentenansicht; `hosted::set_other_screen` (Septet setzt es jedes Bild über `Embedded::set_other_screen`) | Show lief mit 1 Bild/s: die Referentenansicht (ein sofortiges Viewport) lag hinter der Vollbild-Show, GNOME/XWayland nimmt einem verdeckten Fenster nur etwa ein Bild pro Sekunde ab, und egui zeichnet sofortige Viewports innerhalb des Bilds des Hauptfensters – jedes Bild wartete ~1 s |
| `crates/ui-egui/src/show.rs` (ebenso, nicht nur gehostet) | Während eines Morph-Übergangs wird das (unsichtbare) Folienbild nicht in jedem Bild neu gerendert | Folien, deren erste Animation mit dem Übergang startet, renderten pro Bild Morph und Animation (2560×1440: ~55 statt ~36 ms) |

## Darstellungs-Knopf: in allen Apps gehostet ausgeblendet (2026-10-25)

Septet hat einen Darstellungs-Knopf in der Tab-Leiste neben Claude; die eigenen Hell/Dunkel-Knöpfe der Apps sind gehostet
weg (`if !crate::hosted::is_hosted()` um den Block). Menü- und Einstellungs-Einträge bleiben (sie öffnen gehostet Septets
Dialog).

| Datei | Was ausgeblendet ist |
|---|---|
| `effectcraft/crates/ui-egui/src/header.rs` | Appearance-Mode-Knopf (Monitor/Sonne/Mond) mit Kontextmenü |
| `filmcraft/crates/ui-egui/src/header.rs` | Appearance-Mode-Knopf mit Kontextmenü |
| `lightcraft/crates/ui-egui/src/panels/topbar.rs` | Appearance-Mode-Knopf mit Kontextmenü |
| `photocraft/crates/ui-egui/src/panels.rs` | Appearance-Mode-Knopf in der Titelleiste |
| `pdfcraft/crates/ui-egui/src/chrome.rs` | „Display theme“-Knopf mit Kontextmenü |
| `wordcraft/crates/ui-egui/src/ribbon.rs` | „Interface Theme“ unter Ansicht › Dark Mode („Switch Modes“ bleibt: betrifft die Seitenansicht) |
| `deckcraft/crates/ui-egui/src/ribbon.rs` | Titelleisten-Knopf und Ansicht › „Dark Mode“ |

Bewusst nicht geändert: `engine/src/guard.rs` (der Panic-Hook ruft den vorherigen auf), `control.rs` (läuft gehostet
nicht; `Embedded::agent_execute` ruft `SlideApp::run` direkt), `Services` (das Embed schaltet Dateiwähler für Claude ab).
