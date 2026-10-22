# Plan: Aus sieben Apps werden zwölf

Stand: 2026-10-14. Status: **Phase 0 erledigt** (MR !1 am 2026-10-13 auf `main` gemergt), zweiter Abgleich
erledigt (MR !2 am 2026-10-14 gemergt), dritter Abgleich auf `upstream/2026-10-14`, **Phase A läuft** (Branch `phase-a`: A1–A5 und H0 fertig (A4 ohne Lightcraft und hohen Kontrast)); ComfyUI-Anbindung als Phase H eingeplant. Grundlage: Untersuchung der Upstream-Repos
(`storytold/wordcraft`, `deckcraft`, `gridcraft`, `cadcraft`, `soundcraft`, `craft-launcher`, alle v0.4.0 vom
2026-10-10) und des Septet-Codes.

## Entscheidungen des Users (2026-10-10)

| Thema | Entscheidung |
|---|---|
| Architektur | **Ein Programm**, Apps ausblendbar, Updates in der App, große Extras zum Nachladen. Keine installierbaren Module. |
| Name der Suite | **Tutti** (Markenprüfung DPMA/EUIPO und Domain steht noch aus) |
| Erscheinungsbild | **Vier Stufen + System**, dazu Akzentfarbe und UI-Größe |
| Start | **Fundament zuerst** (nach dem ersten Upstream-Abgleich, siehe Phase 0) |
| Upstream | ArtCraft-Updates sollen regelmäßig in die Suite kommen (siehe „Upstream-Updates“) |
| Sound-App | Name noch offen (nicht „Soundcraft“) |

## Ziele (User, 2026-10-10)

1. **WordCraft** (Word), **DeckCraft** (PowerPoint), **GridCraft** (Excel) und **CADCraft** (AutoCAD) kommen
   als Tabs in die Suite, so wie die sieben bisherigen Apps.
2. **SoundCraft** (Pro Tools) kommt auch dazu, wird aber zusammen mit dem User überarbeitet: zwei Modi,
   **Pro** (wie SoundCraft heute) und **Einfach** (an GarageBand angelehnt).
3. **Alles ist untereinander kompatibel:** Drag & Drop, Kopieren/Einfügen, „Senden an“ und Hin-und-zurück
   zwischen allen Apps. Das ist die Design- und Software-Philosophie der Suite.
4. **Claude steuert jede App** über das Chat-Panel. DeckCraft bekommt besonders gute Unterstützung, damit
   Claude Folien, Animationen und Übergänge leicht bauen und prüfen kann.
5. **Ein Erscheinungsbild für alle:** Stellt man Dunkel ein, sind alle Apps dunkel. Heute ist das nicht so.
6. **Neuer Name**, kreativ und nicht nach KI klingend. „Septet“ heißt sieben; mit zwölf Apps passt das nicht mehr.
7. Wunsch: ein Installer in der App (wie ArtCraft Launcher), jede App ein eigenes Repo, Apps in der App
   installieren und deinstallieren, damit der Build einfacher und schneller wird. → Prüfung siehe unten.

## Prüfung: Installer und eigene Repos pro App

### Ergebnis: Echte installierbare Apps gehen mit dieser Architektur nicht sinnvoll

Septet ist **ein Prozess**: Alle Apps teilen sich einen `egui::Context` und ein GPU-Gerät
(`septet/src/hosted.rs`, `Isolation`). Nur so funktionieren Tabs, abreißbare Fenster, Drag & Drop mit
Zielposition und das gemeinsame Chat-Panel.

| Weg | Was passiert | Bewertung |
|---|---|---|
| Apps als nachladbare Bibliotheken (Rust-dylib) | Rust hat keine stabile ABI. Jede App müsste mit exakt demselben Compiler und denselben egui/wgpu-Builds gebaut sein, sonst stürzt sie ab. Photocraft, Filmcraft, Effectcraft und CADCraft greifen direkt auf die GPU zu (`egui_wgpu`-Callbacks, eigene Texturen). | Sehr aufwendig, zerbrechlich, jedes Update bricht alles |
| Jede App ein eigenes Programm, Septet zeigt es an | Tabs, abreißbare Fenster, gemeinsame Schriften, Drop an die Mausposition, GPU-Texturen und Tastaturfokus gehen kaputt oder müssten pro Betriebssystem neu gebaut werden | Würde Septet zu einem Launcher machen, das „Ecosystem-Gefühl“ ginge verloren |
| **Eine App mit Cargo-Features pro App** | Beim Entwickeln baut man nur die App, an der man arbeitet. Die Veröffentlichung enthält alle. | **Empfohlen**, wenig Risiko |

Der ArtCraft Launcher (`craft-launcher`, MIT/Apache) installiert **eigenständige Programme** (DMG, MSI, deb, rpm,
AppImage). Er lädt keine Module in eine laufende App. Seine Bausteine sind trotzdem nützlich, siehe unten.

### Was den Build wirklich schneller und die Datei kleiner macht

1. **Schriften nur einmal einbauen (größter Hebel).** Sechs Apps betten jeweils das komplette craft-fonts-Paket
   ein (je ca. 41 MB, `build.rs` mit `include_bytes!`). Etwa **245 MB der 636 MB** sind doppelte Schriften.
   Lösung: Septet bettet sie einmal ein (oder lädt sie aus einem Ordner neben dem Programm) und gibt sie den
   Apps über einen Hook. Mit fünf weiteren Apps wären es sonst über 450 MB nur für Schriften.
2. **Cargo-Features pro App** (`septet` mit `photocraft`, `wordcraft` … als Features, alle standardmäßig an).
   `cargo run -p septet --no-default-features --features wordcraft` baut nur die Shell und WordCraft.
3. **CI auch bei Pushes und Merge Requests**, nicht nur bei Tags: pro App ein schneller Check-Job mit Cache.
   Heute gibt es nur den Release-Lauf (0.2.0: 2 h 47 min für alle Plattformen).
4. **Repos:** Das Monorepo bleibt, jede App liegt weiter per `git subtree` mit Historie darin. Upstream-Updates
   holt man pro App mit `git subtree pull`. Eigene Repos pro App würden den gemeinsamen `[patch]` für
   hayro/lopdf, den gemeinsamen Lockfile und Änderungen über mehrere Apps hinweg verkomplizieren und den
   Build nicht beschleunigen. Wer will, kann jede App zusätzlich als eigenes Repo auf GitLab spiegeln.

### Was vom Installer-Wunsch trotzdem umgesetzt wird

- **Apps ein- und ausblenden:** In den Einstellungen wählt man, welche Apps auf Home, in „+“ und in „Öffnen mit“
  erscheinen. Wer nur Office braucht, sieht nur Office.
- **Updates in der App:** Septet prüft die Releases und aktualisiert sich selbst. Grundlage sind
  `craft-launcher/crates/core` (Release-Auswahl pro OS und CPU) und `crates/native` (Download, SHA-256,
  Selbst-Update).
- **Inhaltspakete zum Nachladen:** Große Dinge, die nicht jeder braucht, werden in der App heruntergeladen und
  gelöscht: CJK- und Arabisch-Schriften, Instrumente und Loops für den Sound-Einfachmodus, Vorlagen.
  Das macht den Download kleiner, ohne die Architektur zu brechen.
- Optional später: **Editionen** als getrennte Downloads (z. B. „Office“ und „Studio“), gebaut aus denselben Features.

## Upstream-Updates von ArtCraft

ArtCraft veröffentlicht etwa alle zwei Tage neue Versionen. Seit unserem Import (8./9. Oktober) sind upstream
schon **72 bis 895 Commits pro App** dazugekommen. Photocraft ist z. B. von unserem Stand auf v0.6.0 gesprungen.
Unsere eigenen Änderungen pro App sind überschaubar: 38–59 Dateien, ca. 1.200–1.800 Zeilen.

Stand der Apps bei der Untersuchung:

| App | Unsere Basis | Upstream seitdem | Neueste Release |
|---|---|---|---|
| Photocraft | 2026-10-08 | 627 Commits | v0.6.0 |
| Vectorcraft | 2026-10-09 | 554 | v0.8.0 |
| Lightcraft | 2026-10-08 | 895 | v0.5.0 |
| Designcraft | 2026-10-08 | 72 | v0.5.0 |
| Pdfcraft | 2026-10-09 | 219 | v0.5.0 |
| Filmcraft | 2026-10-08 | 448 | v0.5.0 |
| Effectcraft | 2026-10-08 | 162 | v0.7.0 |

Vorgehen:

1. **Auf Releases abgleichen, nicht auf jeden Commit.** Upstream-`main` ändert sich stündlich; die Release-Tags
   sind getestete Stände. Pro App: `git subtree pull --prefix=<app> https://github.com/storytold/<app> <tag>`.
2. **Skript `scripts/sync-upstream.sh <app>|all [tag]`**: holt die neueste Release, merged sie in einen Branch
   `upstream/<datum>`, baut, führt die Tests und die Autotests (`tour`, `interop`, `content`, Claude-Werkzeuge)
   aus und listet Konflikte. Konflikte löst Claude, du prüfst.
3. **Automatisch nachsehen:** `scripts/sync-nightly.sh`, als systemd-User-Timer Di und Fr 03:17
   (`scripts/systemd/`, seit 2026-10-14 aktiv), macht den Abgleich in einem eigenen Worktree auf einem Branch
   und öffnet bei Erfolg einen **Draft-Merge-Request auf GitLab**; bei Konflikt oder Fehler nur eine
   Desktop-Meldung. Solange ein Sync-MR offen ist, setzt er aus. Nichts landet ohne deine Freigabe auf `main`.
4. **Konflikte klein halten:** Unsere Änderungen in den App-Ordnern kommen möglichst in **eigene Dateien**
   (`embed.rs`, `hosted.rs`, `appearance.rs`) statt verteilt in die großen Upstream-Dateien. Jede Änderung in
   einer Upstream-Datei wird in `docs/suite/upstream-patches.md` mit Grund notiert.
5. **Langfristig der beste Weg:** Die Einbettungs-Schnittstelle (Hosted-Modus, `set_appearance`, Daten-Ordner,
   geteilte Schriften) als Pull Request **an ArtCraft zurückgeben**. Nehmen sie das an, schrumpfen unsere
   Änderungen fast auf null, und Updates gehen ohne Konflikte. (Nach außen sichtbar: erst nach deinem OK.)
6. **Neue Apps** (WordCraft, DeckCraft, GridCraft, CADCraft, Sound) werden gleich von Anfang an so angelegt.

## Phasen

Größen: S = klein, M = mittel, L = groß, XL = sehr groß.

### Phase 0: Erster Upstream-Abgleich (erledigt)

Alle sieben Apps auf die neueste Release bringen, **bevor** das Fundament die App-Ordner anfasst. Sonst stapeln
sich die Konflikte. Dabei entsteht `scripts/sync-upstream.sh`. Größe: M (vor allem Lightcraft und Photocraft mit
vielen Upstream-Änderungen).

Erledigt 2026-10-10, gemergt 2026-10-13 (MR !1). Upstream war da schon weiter (Photocraft v0.8.2, Vectorcraft
v0.10.0, Lightcraft/Designcraft/Pdfcraft v0.7.1, Filmcraft v0.7.2, Effectcraft v0.9.2; die fünf neuen Apps
v0.6.x). Vor Phase A deshalb ein zweiter Abgleich mit `scripts/sync-upstream.sh all`: erledigt, gemergt
2026-10-14 (MR !2; Photocraft v0.8.2, Vectorcraft v0.10.0, Effectcraft v0.9.2, Filmcraft v0.7.2, die übrigen v0.7.1).
Dritter Abgleich am 2026-10-14 auf Basis von `phase-a` (Branch `upstream/2026-10-14`, Draft-MR nach MR !3):
Photocraft v0.9.0, Vectorcraft v0.11.0, Effectcraft v0.10.0, die übrigen v0.8.0. Dabei: Vectorcraft nutzt
hayro 0.8, daher Pdfcrafts JBIG2-Patch auf hayro-jbig2 0.3.1 übertragen (sonst ungepatcht für alle Apps).

### Phase A: Fundament (vor den neuen Apps)

| Schritt | Inhalt | Größe |
|---|---|---|
| A1 Schriften einmal | Hook in den Text-Crates der sechs Apps (`set_shared_fonts` o. ä.); `build.rs` bettet nicht mehr ein, wenn Septet es tut. Binary wird um ca. 200 MB kleiner. | M |
| A2 App-Register statt fester Listen | `AppKind`-Enum, die `match`-Blöcke in `hosted.rs`, `content.rs`, `bridge.rs`, `shell.rs`, `fonts.rs` und die Assistant-Texte werden aus einer Tabelle von App-Beschreibungen gespeist (Name, Farbe, Icon, Endungen, was die App annimmt und liefert). Gespeicherte Sitzungen mit fehlenden Apps werden still übersprungen. | M |
| A3 Cargo-Features pro App | baut auf A2 auf | S |
| A4 Ein Erscheinungsbild | siehe eigener Abschnitt | M (dazu L für Lightcraft und CADCraft) |
| A5 CI für Pushes und MRs | Check-Job pro App, Cache | S |

Stand 2026-10-14 (Branch `phase-a`, MR !3 als Draft): **A1–A5 und H0 erledigt**; von A4 fehlen nur Lightcraft
(helle Palette fehlt) und hoher Kontrast.
- A1: Crate `shared/craft-fonts` bettet jede Schrift einmal ein; die Apps verweisen mit dem Feature
  `shared-craft-fonts` darauf (Adresse, nicht Wert: sonst kopiert der Compiler die Bytes). Gemessen: jede Schrift
  einmal im Binary statt sechsmal; Test `fonts::tests::craft_fonts_are_embedded_once`.
- A2/A3: `kinds::APPS` (eine Zeile pro App, samt Einstiegspunkten und Assistant-Texten); Features
  `photocraft` … `effectcraft`, alle Standard. Unbekannte Apps in Sitzungen lesen sich als `AppKind::Unknown`.
- A5: `.github/workflows/check.yml` (GitHub, jeder Push) und `.gitlab-ci.yml` (GitLab: MRs, main, von Hand –
  kostet CI-Minuten, daher nicht bei jedem Push).
- H0: `septet/src/gpu.rs`; Autotest `gpu-lost`.
- A4 (Teil 1): `septet/src/appearance.rs` – Stufe Darkest/Dark/Light/Lightest, „Same as the desktop“ (XDG-Portal
  über `pdfcraft-platform`), „Each app's own“, UI-Größe 80–175 % (ein `zoom_factor` für alle). Dialog über Home ›
  Appearance und das Fenstermenü. `HostedApp::set_appearance(Option<u8>)` / `set_open_appearance`; in
  Vectorcraft, Designcraft (je 4 Stufen) und Pdfcraft (Dark/Light) über `hosted::set_brightness`/`set_theme`,
  nie in deren Einstellungen gespeichert; ihre Theme-Befehle öffnen Septets Dialog. Autotest `appearance`.
- A4 (Teil 2): auch Photocraft (vier Pro-Themes), Filmcraft, Effectcraft (setzt gehostet keinen Zoom mehr).
  Akzentfarbe: acht feste Farben, auf denen weißer Text lesbar bleibt; jede App färbt ihre Akzent-Tokens um
  (`hosted::with_accent`/`recolor`: Farbton des Akzents, Helligkeit und Sättigung relativ zum Theme-Akzent,
  Grautöne bleiben), Filmcraft über seine Highlight-Farbe. `set_appearance(level, accent)`.

### Phase B: Die vier Office/CAD-Apps einbauen

Für jede App dieselben Schritte wie bei den bisherigen sieben (`docs/embedding/phase1…5`):
Subtree hinzufügen, Embed-API (`apps/<app>/src/embed.rs`, `hosted::set_hosted`), Logos entfernen
(Markenlizenz), Daten-Ordner für den portablen Modus, Steuerkanal für Claude (`control::handle` in-process),
Skills für Claude, Autotest-Szenario, Theme-Anbindung.

| App | Größe | Besonderheiten |
|---|---|---|
| WordCraft | M | `zoom_with_keyboard` und Schriften sind global; Sprache liegt in einem `thread_local`; Vorlesen startet externe Programme; Layout pro Render teuer (cachen). 422 Befehle, Seiten-Render auf der CPU vorhanden. |
| DeckCraft | M | Engine setzt einen globalen Panic-Hook (`engine/src/guard.rs`), setzt jeden Frame `set_zoom_factor`, öffnet eigenen Audio-Ausgang, Vollbild und ein zweites Fenster für die Referentenansicht, schließt Fenster direkt. Alles im Hosted-Modus umleiten. 226 Befehle. |
| GridCraft | M–L | Kein Offscreen-Render für Tabellen. Abkürzung: GridCraft kann PDF; das PDF rendert Septet mit hayro, das schon im Binary ist. Fängt fallengelassene Dateien selbst ab (kollidiert mit „Platzieren“); statischer Textur-Cache. 313 Befehle. |
| CADCraft | S–M | Globales `install_io()` einmal aufrufen; GPU-Canvas über `egui_wgpu`-Callback; `undo` heißt dort nicht `edit.undo`; interaktive Befehle (Kommandozeile mit Nachfragen) für Claude sperren. 277 Befehle, PNG/SVG/PDF-Render vorhanden. |

Empfohlene Reihenfolge: **WordCraft → DeckCraft → GridCraft → CADCraft**.

### Phase C: Alles miteinander kompatibel

Jede App beschreibt in A2, welche Formate sie **liefert** und **annimmt**. Septet wählt bei Drag & Drop,
„Senden an“ und Einfügen automatisch das beste gemeinsame Format, wie heute schon zwischen den sieben Apps.

| Von → Nach | Format | Neu zu bauen |
|---|---|---|
| GridCraft → DeckCraft | Diagramm als **Daten** (`insert.chart`), bleibt bearbeitbar | Übergabe-Format (JSON) |
| GridCraft → WordCraft / DeckCraft | Tabelle als HTML/CSV | – |
| GridCraft → Pdfcraft | PDF | – |
| WordCraft → Designcraft | Text mit Formaten | Designcraft nimmt heute nur `txt`: RTF/HTML/DOCX-Import in Designcraft ergänzen |
| WordCraft → Pdfcraft / Photocraft | PDF / PNG | – |
| DeckCraft → Pdfcraft | PDF | – |
| DeckCraft → Filmcraft | Folien als PNG, später als Video | Video-Export für DeckCraft |
| Vectorcraft / Photocraft / Filmcraft → DeckCraft, WordCraft | SVG, PNG, MP4 | – |
| CADCraft → Vectorcraft / Designcraft / Pdfcraft / Photocraft | SVG, PDF, PNG | – |
| Vectorcraft → CADCraft | DXF (Vectorcraft kann DXF) | prüfen |
| Filmcraft ↔ Sound-App | Spuren als Stems + Referenzvideo hin, Mix zurück | siehe Phase F |

Später: reiche Zwischenablage (HTML/RTF) zwischen den Office-Apps, damit Einfügen Formatierung behält.

### Phase D: Claude in den neuen Apps, besonders DeckCraft

DeckCraft kann schon heute alles über Befehle: Folien, Layouts, Themes, über 150 Formen, Text, Diagramme,
SmartArt, 49 Übergänge (auch Morph) und 63 Animationen mit Reihenfolge, Start, Dauer und Verzögerung.
Damit Claude das **leicht** und **überprüfbar** macht, fehlt:

1. **Batch-Werkzeug** im Septet-MCP-Server: viele Befehle in einem Aufruf, ein Rückgängig-Schritt.
2. **Animationen und Übergänge als Bild prüfen:** einen Frame zum Zeitpunkt t rendern (`Timeline::state_at`
   und `RenderOpts.state` gibt es schon; Übergänge außer Morph brauchen einen kleinen CPU-Compositor), dazu
   ein Kontaktbogen „Folie 3, Animation in 6 Schritten“.
3. **Freie Bewegungspfade** (`path`-Parameter statt nur Vorlagen) und Parameter für Hervorhebungen
   (Drehwinkel, Farbe, Stärke).
4. **Export als Video oder GIF** (auch für die Übergabe an Filmcraft).
5. **Skills** für Claude: „Präsentation bauen“ (Gliederung → Folien → Design → Animation → Prüfen),
   „Diagramm aus GridCraft-Daten“, „Dokument formatieren“, „Tabelle auswerten“, „Zeichnung bemaßen“.
6. Alle vier Apps: `agent_commands`, `agent_execute`, `agent_inspect`, `agent_render` wie bei den bisherigen
   sieben; der System-Prompt und die Werkzeugbeschreibungen kommen aus dem App-Register (A2).

### Phase E: Sound-App, Pro-Modus

| Schritt | Inhalt | Größe |
|---|---|---|
| Einbetten wie sie ist | Embed-API; Theme liegt in einem globalen Flag; Wiedergabe stoppen, wenn der Tab verborgen ist; eigener Thread-Pool für den Mix (sonst Aussetzer, weil Filmcraft denselben rayon-Pool nutzt) | M |
| Fremde Plug-ins aus | CLAP/VST3/AU laufen im selben Prozess: Ein abstürzendes Plug-in würde die ganze Suite mitreißen. In der Suite erst einmal aus, später in einem eigenen Prozess. | S |
| Claude | 276 Befehle, `session.inspect`, Bounce mit LUFS; dazu Wellenform-Bild und Befehle auf Musiker-Ebene (siehe Phase F) | M |

### Phase F: Sound-App, Einfach-Modus (an GarageBand angelehnt), mit dem User zusammen

Technik: Die Sound-App trennt Oberfläche und Engine sauber. Der Einfach-Modus wird eine **zweite Oberfläche
auf derselben Sitzung**: Umschalten zwischen Pro und Einfach jederzeit, ohne Datenverlust, ein gemeinsames
Rückgängig. Regel: Einfach darf nichts erzeugen, was Pro nicht anzeigen kann.

| Stufe | Inhalt | Größe |
|---|---|---|
| F1 MVP | große Spurköpfe, Umschalter Pro/Einfach, Klang-Vorlagen für Synth und Drums, wenige große Regler pro Spur (wie „Smart Controls“), Aufnahme-Vorlagen (Stimme, Gitarre = fertige Effektketten), Schritt-Sequenzer als einfacher Drummer, Song exportieren | M–L |
| F2 | Live-MIDI-Eingang (fehlt in der Engine, Voraussetzung für alles Spielbare), Bildschirm-Klaviatur und Tastatur-Spielen, Sampler mit SoundFonts (`rustysynth`; Klänge z. B. GeneralUser GS, Salamander Piano CC-BY, VSCO-2 CE CC0) als Inhaltspaket, Loop-Browser mit selbst erzeugten MIDI-Loops (passen sich Tempo und Tonart an), Akkord-Streifen, Arpeggiator | L |
| F3 | Drummer mit Stilen, Audio-Loops mit Echtzeit-Stretching, Live Loops (Clip-Raster), Touch-Instrumente | XL |
| Filmcraft ↔ Sound | Filmcraft gibt Stems und Referenzvideo, die Sound-App baut die Sitzung, der Mix geht zurück; später AAF/OMF-Import (Filmcraft exportiert AAF/OMF schon) | S–M, AAF/OMF M–L |
| Claude | Befehle wie `simple.add_drummer {style}`, `simple.chords {progression, key}`, `simple.add_loop`, `midi.notes_add` (viele Noten auf einmal), damit „mach einen Lo-Fi-Beat“ in wenigen Schritten geht | M |

**Name:** „Soundcraft“ ist eine eingetragene Marke von Harman (Mischpulte, seit 1988). Die App braucht in der
Suite einen anderen Namen. Auch nichts, was nach „GarageBand“ klingt (Apple-Marke).

### Phase G: Release

Umbenennung (Name, Icon, Paketnamen, Datenordner mit Migration vom alten `septet`-Ordner), README, NOTICE für
fünf weitere Apps, Lizenzliste, Release-Notes, Version 0.3.0.

### Phase H: ComfyUI-Anbindung (Bilder erzeugen und bearbeiten mit Qwen Image 2.1)

Wunsch des Users (2026-10-13): Bilder in der Suite lokal erzeugen und bearbeiten (Qwen Image 2.1 über ComfyUI),
ohne dass die Suite abstürzt, wenn ComfyUI viel VRAM oder RAM braucht, und mit automatischem Entladen der Modelle.

**Ausgangslage auf diesem Rechner** (untersucht 2026-10-13):

| Was | Stand |
|---|---|
| Hardware | RTX 4070 Ti mit 12 GB VRAM, 30 GB RAM, 30 GB Swap |
| ComfyUI | Comfy Desktop v0.37.0 in `~/ComfyUI-Installs/ComfyUI/ComfyUI`, Port 8188, ohne VRAM-Flags. Modelle in `~/ComfyUI-Shared/models` |
| Qwen Image 2.1 | Diffusionsmodell int8 7,3 GB + Text-Encoder (Qwen3-VL 8B int8) 9,4 GB + VAE 0,7 GB = **17,3 GB**. Passt nicht gleichzeitig in 12 GB: ComfyUI lädt abwechselnd und hält den Rest im RAM. Kein Lightning-LoRA für 2.1, also 25 Schritte (ca. 30–60 s pro Bild) |
| Workflows | Generieren: `workflows_local/qwen_image_2_1.api.json` (Hermes-Skill, API-Format, 25 Schritte, cfg 1, euler/simple). Bearbeiten: wird in `~/.claude/skills/qwen-image/scripts/qwen_image.py` im Code gebaut (`TextEncodeQwenImage21` mit bis zu 10 Bildern und VAE). Das alte Edit-2509-Modell (20 GB) braucht die Suite nicht |
| Vorhandene Logik | `comfy_local.py` (Hermes): Ollama entladen, ComfyUI starten, `/free`, VRAM-Prüfung per `nvidia-smi` (mind. 7 GB frei), Upload, `/prompt`, `/history` abfragen, `/view` holen. Die Custom Node `vram_auto_swapper.py` entlädt Ollama vor jedem Lauf |
| Andere GPU-Nutzer | `bonsai.service` (llama-server, ca. 9 GB, startet bei Bedarf), Ollama (5 min keep-alive) |
| Septet selbst | Ein gemeinsames wgpu-Gerät für alle Apps. Photocraft reserviert bis 4 GiB VRAM (`RESIDENT_BUDGET`), Effectcraft bis ca. 2,3 GiB. **Kein** suite-weiter Handler für „GPU-Speicher voll“ oder „Gerät verloren“: Photocraft und Filmcraft setzen je einen, und wgpu behält nur den zuletzt gesetzten |

**Bewertung der vorhandenen Workflows:** Generieren und Bearbeiten passen und dienen als Vorlage. Für die Suite
fehlen eigene Varianten:
- **Feste Eingänge** (Prompt, Bilder, Maske, Größe, Seed, Schritte), damit Septet sie sicher befüllen kann,
  statt Knoten-Nummern zu raten.
- **Bearbeiten mit Maske** (Photocraft-Auswahl → „Generatives Füllen“). Qwen 2.1 nimmt keine Maske. Lösung:
  Ausschnitt um die Auswahl plus Rand ausschneiden, bearbeiten lassen, mit weicher Kante nur in der Auswahl
  zurücksetzen (das macht Photocraft, nicht ComfyUI). So ändert sich außerhalb der Auswahl kein Pixel.
- **Erweitern** (Leinwand vergrößern, Rand füllen) mit derselben Technik.
- Optional später: Hochskalieren, Varianten, schnellere Vorschau mit weniger Schritten (Qualität erst testen).

**Architektur:**
- ComfyUI bleibt ein **eigener Prozess**, die Suite spricht HTTP mit ihm. Nie im selben Prozess: Ein Absturz
  oder Speicherfehler in ComfyUI darf die Suite nicht mitreißen. Auch nicht mitliefern: ComfyUI ist GPL-3.0.
  Der User installiert ComfyUI und die Modelle selbst; die Suite findet sie (Comfy-Desktop-Einstellungen,
  `COMFYUI_MAIN`) oder fragt nach dem Pfad.
- Ein Modul `septet/src/comfy/` in der Shell (Client mit dem vorhandenen `ureq`, eigener Thread, eine Warteschlange,
  Abbrechen über `/interrupt`). Ergebnisse landen als PNG über das vorhandene `OpenRequest`/`place_paths` in den
  Apps. In den App-Ordnern ändert sich dafür nichts, außer bei Photocraft für die Maske.
- Die Workflows liegen als API-JSON-Vorlagen in der Suite. Eigene Workflows des Users kann man importieren, wenn
  er die Eingänge im Knotentitel markiert (z. B. `septet:prompt`, `septet:image1`). Beim Verbinden prüft die Suite
  per `/object_info`, ob alle Knoten da sind, und nennt fehlende.
- Lizenz von Qwen Image 2.1 für die Weitergabe der Ergebnisse vor dem Release prüfen.

**Damit die Suite nicht abstürzt (Speicher):**

| Maßnahme | Wirkung |
|---|---|
| **Suite-weiter GPU-Fehler-Handler** (ein `on_uncaptured_error` und ein Device-Lost-Callback in der Shell, die an alle Apps weiterreichen) | Ersetzt die Handler, die sich heute gegenseitig überschreiben. Bei „Speicher voll“: ComfyUI `/free`, erneut versuchen, sonst CPU-Weg der App. Bei „Gerät verloren“: Notsicherung aller Dokumente, dann Gerät neu anlegen oder sauber neu starten. **Hilft auch ohne ComfyUI, deshalb am besten schon in Phase A** |
| **Die Suite hat Vorrang** (User, 2026-10-13: „es muss genug RAM und VRAM frei sein, damit z. B. Photocraft flüssig läuft“). Die Reserve wird nicht fest eingestellt, sondern aus den Budgets der offenen Apps berechnet: `--reserve-vram` = Summe der GPU-Budgets (Photocraft `RESIDENT_BUDGET` 4 GiB + `FX_BUDGET` 1,5 GiB, Effectcraft usw.) + Shell + 1 GB Puffer. RAM: cgroup-Grenze für ComfyUI = Gesamt-RAM − aktueller Bedarf der Suite − 4 GB Puffer | Die Apps behalten ihre vollen Budgets, Photocraft bleibt flüssig. Preis: ComfyUI hat weniger Platz, lagert mehr aus und rechnet langsamer (vorher messen, bei 12 GB VRAM z. B. ca. 6 GB für ComfyUI). Braucht die Suite später mehr, als beim Start reserviert war (weitere App geöffnet), wird ComfyUI nach dem laufenden Bild entladen und mit neuer Reserve neu gestartet |
| **GPU-Druck an die Apps melden** (`HostedApp::set_gpu_pressure`) | Nur für Apps im Hintergrund: Verborgene Tabs halten Vorschau-Renderings an und geben Caches frei. Die sichtbare App wird nicht gebremst |
| **VRAM-Prüfung vor jedem Lauf** (NVML, unter Linux auch `nvidia-smi`) | Reicht der Platz nicht, sagt die Suite, wer ihn belegt (z. B. bonsai), statt zu starten. Andere Dienste beendet sie nicht von selbst |
| **RAM:** ComfyUI-Prozess unter Linux mit `oom_score_adj=1000` starten, optional in einem cgroup mit `MemoryHigh`/`MemoryMax` (`systemd-run --user --scope`) | Wird der RAM knapp, beendet das System ComfyUI statt der Suite; mit cgroup wird ComfyUI vorher gebremst. Windows: Job Object mit Speichergrenze |
| ComfyUI mit niedriger CPU-Priorität (`nice`) | Die Oberfläche bleibt flüssig |
| Immer nur ein Lauf gleichzeitig, mit Abbrechen und Zeitlimit | Kein Stapeln von Speicher |

**Automatisch entladen:**
- Nach einem einstellbaren Leerlauf (Standard 2 min) schickt die Suite `/free` mit `unload_models` und
  `free_memory`. Dann ist VRAM und RAM wieder frei; der nächste Lauf lädt neu (Kaltstart ca. 1–2 min).
- Sofort entladen, wenn eine GPU-hungrige Aufgabe startet (Filmcraft-Export, Effectcraft-Render) oder die
  Suite einen GPU-Fehler sieht.
- Hat die Suite ComfyUI selbst gestartet, beendet sie es nach längerem Leerlauf (Standard 10 min) und beim
  Schließen. Lief ComfyUI schon vorher (z. B. für den User selbst), nur `/free`, nie beenden.

**Starten (entschieden 2026-10-13: automatisch):** Läuft ComfyUI schon, nutzt die Suite es. Läuft es nicht,
startet die Suite es beim ersten Auftrag selbst (mit Reserve, Priorität und Speichergrenze von oben). Der User muss
nichts einstellen; in den Einstellungen gibt es nur einen Schalter, um das Selbst-Starten abzuschalten.
Ein schon laufendes ComfyUI wurde ohne die Reserve gestartet: Die Suite prüft dann vor jedem Auftrag den freien
Speicher und entlädt nach jedem Bild sofort.

**Bedienung:**
- Einstellungen „ComfyUI“: Adresse, Pfad, selbst starten ja/nein, VRAM-Reserve, Leerlaufzeiten, Workflows.
- Photocraft: „Bild erzeugen“ (neue Ebene), „Generatives Füllen“ auf die Auswahl, „Bearbeiten mit Anweisung“ auf
  die Ebene. Ergebnis immer als **neue Ebene**, das Original bleibt.
- Vectorcraft, Designcraft, DeckCraft, WordCraft: „Bild erzeugen“ und an der Stelle platzieren.
  Filmcraft/Effectcraft: Standbild in den Medien-Ordner.
- Fortschrittsanzeige mit Schritten und Abbrechen; Seed und Prompt werden in der Ebene gespeichert, damit man
  eine Variante wiederholen kann.
- Claude: Werkzeuge `image_generate` und `image_edit` im Septet-MCP-Server (`septet/src/assistant/tools.rs`),
  laufen wie `septet_render` auf einem eigenen Thread.

| Stufe | Inhalt | Größe |
|---|---|---|
| H0 | Suite-weiter GPU-Fehler-Handler und Notsicherung (am besten in Phase A) | M |
| H1 | Client, Start/Stopp von ComfyUI, VRAM-/RAM-Schutz, automatisches Entladen, Einstellungen | M |
| H2 | Workflow-Vorlagen: zuerst Bearbeiten mit Maske (für Generatives Füllen) und Erzeugen; danach Erweitern und Import eigener Workflows | M |
| H3 | **Zuerst Photocraft „Generatives Füllen“** (User, 2026-10-13), dann „Bild erzeugen“ als neue Ebene, danach die anderen Apps | M |
| H4 | Claude-Werkzeuge `image_generate`/`image_edit` | S |
| H5 | Autotest gegen einen Schein-ComfyUI-Server (ohne GPU); ein echter Lauf von Hand | S |

Reihenfolge: H0 mit Phase A, H1–H5 nach Phase A (vor oder parallel zu Phase B, berührt die neuen Apps kaum).
Zuerst nur Linux und NVIDIA; Windows und macOS (dort zählt der gemeinsame RAM) danach.

## Ein Erscheinungsbild für alle Apps (A4)

Heute: Keine App lässt sich von außen umstellen; jede speichert ihr eigenes Theme. Die Shell ist fest dunkel
(`main.rs:152`, `theme.rs:52`). Die Tab-Leiste folgt schon der Farbe der aktiven App.

| App | Varianten heute | Aufwand |
|---|---|---|
| Vectorcraft, Designcraft | 4 Helligkeitsstufen | S |
| WordCraft, DeckCraft, GridCraft | Hell/Dunkel | S |
| Pdfcraft | Hell/Dunkel/System | S–M |
| Photocraft | 5 (Pro, Pro Medium, Studio, Studio Light, Classic) | M (schreibt das Theme sonst in die eigenen Einstellungen zurück) |
| Filmcraft, Effectcraft | 3 Stufen | M (je über 200 feste Farben im Code prüfen) |
| Sound-App | Hell/Dunkel/System über ein globales Flag | M |
| Lightcraft, CADCraft | nur dunkel | L (helle Palette fehlt ganz) |

Umsetzung:
1. Ein gemeinsamer Typ `Appearance { stufe: Dunkelst | Dunkel | Hell | Hellst, akzent: Option<Farbe>,
   hoher_kontrast }`, in jeder App in `hosted.rs`. Jede App bildet die Stufe auf ihre eigenen Varianten ab;
   Apps mit nur zwei Varianten nehmen Dunkel für die beiden dunklen und Hell für die beiden hellen Stufen.
2. `HostedApp::set_appearance`. Solange eine App in Septet läuft, gilt das Septet-Theme vor dem eigenen und
   wird **nicht** in die Einstellungen der eigenständigen App geschrieben. Die Theme-Menüs der Apps verweisen
   auf die Septet-Einstellungen.
3. Septet bekommt einen Einstellungsdialog „Darstellung“: Stufe (oder „wie System“), Akzentfarbe (Standard: die
   Farbe jeder App), UI-Größe (ein gemeinsamer Zoom für alle Apps). Home und leere Fenster folgen dem Theme.
4. Reihenfolge: zuerst die S-Apps, dann die M-Apps; Lightcraft und CADCraft bleiben dunkel, bis ihre helle
   Palette fertig ist.

Stand 2026-10-14: Schritte 1–3 für die Shell und alle Apps außer Lightcraft umgesetzt, mit Akzentfarbe.
Abweichungen vom Plan: statt eines gemeinsamen Typs bekommt jede App Stufe (0–3, `None` = eigenes Theme) und
Akzent (`Option<egui::Color32>`) einzeln, damit die Apps keine neue Abhängigkeit brauchen; Standard ist „Dark“
und der Akzent jeder App. Die Akzente sind eine feste Auswahl statt eines Farbwählers (Kontrast zu weißem
Text). Offen: hoher Kontrast (Designcraft und Filmcraft haben ihn schon, die anderen nicht), Lightcraft (L).

## Namensideen für die Suite

Vor der Entscheidung Marken beim DPMA/EUIPO und Domains prüfen.

| Name | Idee |
|---|---|
| **Tutti** | Musikbegriff: die Stelle, an der das ganze Orchester spielt. Führt „Septet“ weiter, ohne Zahl. |
| **Bigband** | Aus dem Septett ist eine Bigband geworden. Verspielt, gleiche Bildwelt. |
| **Studiolo** | Das Arbeitszimmer der Renaissance voller Werkzeuge, Instrumente und Kunst (z. B. in Urbino). |
| **Werkbank** | Bodenständig, alles liegt griffbereit. Deutsch, funktioniert auch international. |
| **Potluck** | Jeder bringt etwas mit: passt zu einer Suite aus offenen Apps. |
| **Bricolage** | Basteln mit dem, was da ist. |

Ideen für die Sound-App: **Woodshed** (Musiker-Slang fürs Üben im stillen Kämmerlein), **Overdub**, **Takes**,
**Proberaum**.

Die übrigen App-Namen (Photocraft, WordCraft …) dürfen bleiben: Die Markenlizenz von ArtCraft gilt nur für den
Namen „ArtCraft“, seine Wortmarke und Logos.

## Offene Punkte

1. Name der Sound-App.
2. Markenprüfung und Domain für „Tutti“; danach Umbenennung der Repos auf GitLab und GitHub (nach außen sichtbar).
3. Ob die Einbettungs-Schnittstelle an ArtCraft zurückgegeben werden soll.
4. ComfyUI (Phase H): Leerlaufzeiten (2 min entladen, 10 min beenden) bestätigen, sobald es sich ausprobieren
   lässt. Entschieden 2026-10-13: automatisch starten bzw. ein laufendes nutzen; die Suite hat beim Speicher
   Vorrang; Generatives Füllen in Photocraft zuerst.
