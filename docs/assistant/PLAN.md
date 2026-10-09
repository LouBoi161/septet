# Plan: Claude in Septet

Stand: 2026-10-09. Status: Phasen 0–2, 4 und 4b erledigt (Spike: `protocol-notes.md`); Phase 3 läuft (Spec und Pilot
Vectorcraft fertig, siehe „Stand Phase 3“).

## Ziel

Nutzer melden sich mit ihrem Claude-Konto an und nutzen ihr Claude-Abo (Pro/Max), um in Septet:

1. **Mit Code erzeugen**: Vektorgrafiken (SVG), Layouts, PDFs, Motion-Graphics (JSX/Lottie), Projektdateien.
2. **Video schneiden**: Filmcraft-Projekte über dessen ~650 Befehle bearbeiten, Frames rendern und prüfen.
3. **Die Apps normal bedienen**: über die vorhandenen Befehls- und UI-Schnittstellen und als letzte Stufe
   per „Computer Use", aber **nur innerhalb des Septet-Fensters/Tabs**, nie auf dem restlichen Desktop.

## Rechtlicher Rahmen (bestimmt die Architektur)

Quellen (geprüft 2026-10-08): <https://code.claude.com/docs/en/legal-and-compliance>,
<https://code.claude.com/docs/en/agent-sdk/overview>, <https://code.claude.com/docs/en/authentication>.

- **Verboten**: einen eigenen „Mit Claude anmelden"-OAuth-Flow bauen, Claude-Tokens lesen, speichern,
  weiterreichen oder erneuern (auch nicht `claude setup-token` einfügen lassen), Anfragen über fremde
  Abos leiten. Wörtlich: „developers may not collect, store, or intermediate Claude.ai credentials or
  session tokens — sign-in to a Claude account must complete through Anthropic's own flow."
- **Erlaubt**: „Nor does it prevent an end user from signing in to the unmodified Claude Code binary
  with their own Claude subscription." → Septet **startet das vom Nutzer selbst installierte und selbst
  angemeldete `claude`** als Unterprozess und fasst die Zugangsdaten nie an.
- `claude` **nicht** mitliefern oder verändern (sonst gelten die Commercial Terms).
- Kein „Claude Code"-Logo/-Branding in Septet; Formulierung: „Benötigt Claude Code, angemeldet mit deinem
  Claude-Abo". Keine Zusagen zu Limits.
- Graubereich: Ob Anthropic diese Art Einbindung (wie Zed sie macht) ausdrücklich gutheißt, ist nirgends
  schriftlich bestätigt. Vor dem Release optional beim Anthropic-Vertrieb nachfragen.
- Computer Use: Anthropic verlangt Einwilligung vor dem Aktivieren → einmaliger Zustimmungsdialog.

## Architektur

```
┌──────────────────────── Septet (ein Prozess) ────────────────────────┐
│  Chat-Panel (egui, rechts angedockt)                                  │
│        │ Nutzer-Nachrichten          ▲ Text/Tool-Events (Stream)      │
│        ▼                             │                                │
│  assistant::session  ── stdin/stdout (stream-json) ──► claude -p …   │◄─ vom Nutzer installiert
│        ▲                                                   │          │   und angemeldet
│        │ mpsc + ctx.request_repaint                        │ MCP (HTTP,
│  assistant::mcp  ◄──── 127.0.0.1:<zufällig>, Bearer-Token ─┘ loopback)
│        │                                                              │
│        ├─► Shell-Werkzeuge: Tabs, Öffnen, Platzieren, Screenshot,     │
│        │   synthetische Eingaben (Router)                            │
│        └─► App-Werkzeuge: ControlRequest → Embedded::set_control →    │
│            vorhandenes control::handle der jeweiligen App             │
└───────────────────────────────────────────────────────────────────────┘
```

### Warum so

- Alle 7 Apps haben bereits eine einheitliche Befehls-Registry (`Session::execute`, rückgängig machbar) und
  eine JSON-Steuer-API (`crates/ui-egui/src/control.rs`: `engine.execute`, `engine.commands`, `ui.inspect`,
  `ui.click/drag/key/type`, `ui.screenshot`, `app.open/save`). Im Embed-Modus ist sie nur abgeschaltet
  (Phase-1-Vertrag: „NO control/TCP/MCP servers"). **Wir binden sie in-process an, ohne TCP pro App.**
- Ein MCP-Server in Septet statt sieben (keine Port-Kollisionen wie Vectorcraft/Designcraft 7979, ein
  Token, eine Sicherheitslogik).
- MCP-Bildergebnisse sieht Claude direkt („When an MCP tool returns a PNG … Claude sees the image inline"),
  d. h. Screenshots/Render-Vorschauen funktionieren ohne eigenes Computer-Use-API-Tool.

### Claude-Prozess

Start (pro Unterhaltung ein langlebiger Prozess):

```
claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages
       --mcp-config <septet-mcp.json> --strict-mcp-config
       --tools "Read,Write,Edit,Glob,Grep,Bash"  --permission-prompt-tool mcp__septet__approve
       --allowedTools mcp__septet --permission-mode acceptEdits --disable-slash-commands
       --append-system-prompt-file <septet-system.md>
       --session-id <uuid>   (bzw. --resume <uuid> für alte Unterhaltungen)
       --setting-sources ""  (keine Nutzer-Hooks/Plugins/CLAUDE.md – Verhalten reproduzierbar)
       --model <aus Einstellungen: sonnet | opus | …>
cwd = <Datenordner>/assistant/workspace/<session>
env = Elternumgebung OHNE ANTHROPIC_API_KEY / ANTHROPIC_AUTH_TOKEN (sonst wird über API statt Abo abgerechnet),
      dazu MCP_TOOL_TIMEOUT=3600000 (sonst bricht `approve` nach ~60 s ab)
```

- **Kein** `--bare` (liest keine OAuth-Daten).
- `septet-mcp.json`: `{"mcpServers":{"septet":{"type":"http","url":"http://127.0.0.1:<port>/mcp","headers":{"Authorization":"Bearer <token>"}}}}`,
  im Session-Ordner mit Rechten 0600, beim Beenden gelöscht.
- Erkennung/Anmeldung: `claude` im PATH suchen (Windows: `claude.exe`/`claude.cmd`; zusätzlich typische
  Installationsorte `~/.local/bin`, `~/.claude/local`; Pfad in den Einstellungen überschreibbar),
  `claude --version`, `claude auth status` (JSON, `loggedIn`, `authMethod`).
  Nicht angemeldet → Knopf „Anmelden" startet `claude auth login` (Anthropics Browser-Flow); Septet pollt
  danach `auth status`. Nicht installiert → Link zur offiziellen Installationsanleitung.

## Entscheidungen (User, 2026-10-08)

| Thema | Entscheidung |
|---|---|
| Eingebaute Werkzeuge | Read/Write/Edit/Glob/Grep im Workspace frei; **Bash nur mit Freigabe pro Befehl** im Chat (über `approve`). Schreibzugriffe außerhalb des Workspace ebenfalls Freigabe. |
| API-Key-Modus | **Später als Phase 9**, hinter derselben `session`-Schnittstelle. Zuerst nur Abo über lokales Claude Code. |
| Tabwechsel | Claude **darf den Ziel-Tab selbst aktivieren** (sichtbar); nötig, weil nur der sichtbare Tab Befehle verarbeitet. |
| App-Steuerung (2026-10-09) | **Über ein sauberes Backend** (Befehls-Registry der Apps), nicht visuell per Computer Use. Claude soll die Leinwand **und jede Ebene einzeln** als Bild abrufen können. Siehe Phase 3. |
| MCP-Server der Apps (2026-10-09) | **Nicht direkt nutzen.** Jede App hat einen (`<app>-cli mcp`, stdio), aber Headless arbeitet auf einer eigenen Sitzung (nicht dem Dokument im Tab), Bridge braucht den im Embed-Modus abgeschalteten TCP-Steuerport (Vectorcraft und Designcraft beide 7979), dazu 7 CLI-Binaries und ~250 Werkzeuge. Stattdessen: ihre Werkzeugkataloge als Quelle für die App-Skills, ihr Render-Code als Vorlage für `agent_render`. Einbinden in-process (Vectorcrafts `Backend`-Trait ist austauschbar) nur, falls die generischen `app_*`-Werkzeuge nicht reichen. |

## Neue Module

| Datei | Inhalt |
|---|---|
| `septet/src/assistant/mod.rs` | `Assistant`-Zustand (Unterhaltungen, aktive Session, Kanäle), Drain in `Shell::logic` |
| `assistant/cli.rs` | `claude` finden, Version, `auth status`, `auth login`, Prozess starten/stoppen, Env säubern |
| `assistant/protocol.rs` | serde-Typen für stream-json (system/init, assistant, stream_event-Deltas, user, result, Bild-Blöcke); tolerant gegenüber unbekannten Feldern |
| `assistant/session.rs` | Lese-/Schreib-Thread pro Prozess, Abbrechen (Interrupt bzw. Kill), Neustart mit `--resume` |
| `assistant/mcp.rs` | Minimaler MCP-Server „Streamable HTTP" (nur JSON-Antworten, kein SSE nötig) auf eigenem Thread, Token-Prüfung, `initialize`/`tools/list`/`tools/call` |
| `assistant/tools.rs` | Werkzeugdefinitionen (JSON-Schema) und Dispatch → Shell-Befehle über Kanal, Antwort über `oneshot`-artigen `mpsc` |
| `assistant/computer.rs` | Screenshot des Tab-Bereichs, Skalierung, Koordinaten-Umrechnung, Eingabe-Injektion mit Wächter |
| `assistant/panel.rs` | Chat-UI: Verlauf, Streaming-Text, Werkzeug-Karten (aufklappbar), Bild-Vorschauen, Freigabe-Dialoge, Stop-Knopf, Modellwahl |
| `assistant/settings.rs` | Einstellungsseite (erste Settings-UI der Shell): Claude-Pfad, Status, Modell, Berechtigungen, Computer-Use-Einwilligung |

Neue Abhängigkeiten (klein): `tiny_http` (oder handgeschriebener HTTP/1.1-Parser), `uuid`, `base64`,
`getrandom` für das Token. Kein tokio nötig – die Shell arbeitet bisher ohne Async, das bleibt so.

## App-Seite: Steuerkanal in eingebettete Apps

Siehe **„Phase 3: Apps steuern und sehen (Detailplan)“** weiter unten. Kurz: Claude bedient die Apps über ihre
vorhandene Befehls-Registry, nicht per Computer Use. Es sieht seine Arbeit über Off-Screen-Renderings, die es auch
für einzelne Ebenen, Objekte und Clips geben soll. Ein Kanal je App, kein TCP.

## Werkzeuge für Claude (bewusst wenige und generisch)

Statt 7 × ~25 App-Werkzeugen ein schlanker Satz. Die App-Befehle sind über `commands`/`execute` erreichbar,
die Registry beschreibt sich selbst (IDs, Parameter, enabled).

| Werkzeug | Zweck |
|---|---|
| `septet_state` | Fenster, Tabs, aktive App, Dokumenttitel, ungespeichert ja/nein |
| `septet_open(paths, app?)` / `septet_place(path, app, at?)` | Datei öffnen bzw. ins aktive Dokument platzieren (`OpenRequest`) |
| `septet_activate(app)` | Tab zeigen/App starten |
| `app_commands` / `app_execute` / `app_inspect` / `app_render` / `app_undo` | Phase 3, Details im Detailplan unten |
| `ui_inspect(app)` / `ui_act(app, action)` | semantische UI-Steuerung über Widget-IDs (`ui.click`, `ui.set`, `ui.type`, Menü) |
| `computer(action, …)` | Pixel-Ebene, nur im Tab-Bereich (siehe unten) |
| `approve` | interne Permission-Prompt-Schnittstelle (`--permission-prompt-tool`) → Freigabe-Dialog im Panel |

Der System-Prompt (`septet-system.md`) erklärt: Reihenfolge Befehle > UI-Inspektion > Pixel-Klicks;
nach Änderungen per `app_render` prüfen; Dateien nur im Workspace anlegen; pro App kurze Hinweise
(Vectorcraft: SVG schreiben und platzieren, Filmcraft: `.fcproj`/Befehle, Effectcraft: JSX-Skripte via
`run_script`, Designcraft: Befehlsskripte, Pdfcraft: `doc_create`).

## Computer Use – nur in der App

- **Screenshot**: `ViewportCommand::Screenshot` über den Router, Ergebnis `Event::Screenshot` abfangen,
  auf `app_rect × pixels_per_point` zuschneiden (nur der Tab-Inhalt, keine Tab-Leiste, kein Chat-Panel,
  kein Desktop), auf ≤ 1568 px lange Kante skalieren, PNG → MCP-Bild. Skalierungsfaktor merken.
  (Für abgerissene Zusatzfenster liefert eframe keine Screenshots → dort nur `ui.screenshot` der App
  bzw. Computer Use nur im Hauptfenster; im Spike prüfen.)
- **Eingaben**: Koordinaten aus Screenshot-Pixeln zurückrechnen, auf `app_rect` begrenzen (außerhalb →
  Fehler an Claude), über `router.inject_events` einspeisen. Neuer Wächter im `input_hook`: injizierte
  Agent-Events werden nur zugestellt, wenn `shown(viewport).kind == ziel_app` (wie bei `inject_close`).
  Aktionen: `click`, `double_click`, `drag`, `scroll`, `type` (`Event::Text`), `key` (mit Modifiern),
  `move`, `screenshot`, `wait`.
- **Nie** Eingaben auf OS-Ebene (kein xdotool, keine Windows-SendInput) → kann das Fenster technisch nicht verlassen.
- Fallstricke: Synthetische Drags können Inhalts-Drags starten (`content.rs:47-59`), Strg+V fängt die Bridge
  (`bridge.rs:61-107`) → für Agent-Events markieren und dort überspringen. Echte Nutzereingaben während
  der Agent klickt: Panel zeigt „Claude bedient Vectorcraft – [Übernehmen]"; farbiger Rahmen um den Tab;
  `Esc` stoppt sofort.

## Chat-Panel (UX)

- Rechts angedockt, Breite ziehbar, ein-/ausblendbar über Knopf in der Tab-Leiste und `Strg+Umschalt+K`.
  In `shell.rs:435` wird `content` in `app_rect` + `chat_rect` geteilt; das Panel wird **außerhalb** des
  `Isolation`-Swaps gezeichnet.
- Tastatureingaben ins Chat-Feld dürfen die App nicht erreichen → im `input_hook` Tastatur-Events
  zurückhalten, solange das Chat-Feld Fokus hat. (Apps lesen `ctx.input` direkt.)
- Kontext-Chips: „aktueller Tab", „Auswahl", „Screenshot anhängen", Dateien per Drag & Drop ins Panel
  (werden als Bild-/Dateiblock mitgeschickt bzw. in den Workspace kopiert).
- Werkzeug-Karten mit Name, Parametern, Ergebnis/Bild; Freigabe-Dialoge inline (Erlauben einmal /
  für diese Unterhaltung / Ablehnen).
- Unterhaltungsliste (Session-IDs + Titel in `config_dir/assistant/sessions.json`), Fortsetzen per `--resume`.
- Statuszeile: angemeldet als (nur `authMethod`, keine Konto-Daten), Modell, ggf. Limit-Meldungen aus
  `result`-Events weiterreichen.
- Auf der Startseite eine Kachel „Mit Claude erstellen" als Einstieg.

## Sicherheit

- MCP nur auf 127.0.0.1, zufälliger Port, 256-bit-Token pro Prozessstart, Vergleich in konstanter Zeit
  (entspricht `pdfcraft/AGENTS.md:96-103`: opt-in, loopback, Token, nie standardmäßig an).
- Alle Werkzeug-Eingaben gelten als nicht vertrauenswürdig (wie in den AGENTS.md der Apps); Pfade auf
  Workspace + vom Nutzer geöffnete Dateien beschränkt; Speichern über bestehende Dateien und Export
  außerhalb des Workspace → Freigabe.
- Die Integration ist **standardmäßig aus**; erst die Einstellungsseite schaltet sie ein.
- Unterprozess wird beim Beenden von Septet sicher beendet (Kindprozess-Gruppe / Job-Objekt auf Windows).

## Phasen

| # | Inhalt | Ergebnis / Abnahme |
|---|---|---|
| 0 ✅ | **Spike** (Wegwerf-Prototyp, `septet/examples/claude_spike.rs`): `claude -p` stream-json mit Abo, MCP-HTTP-Server mit einem Werkzeug, Bildergebnis, `--permission-prompt-tool`, `--resume`, Abbrechen. Linux zuerst, dann Windows/macOS. | Protokoll-Notizen in `docs/assistant/protocol-notes.md`; offene Fragen geklärt (Bilder im stream-json-Input, Interrupt-Nachricht, Verhalten bei Limit). |
| 1 ✅ | `cli.rs`, `protocol.rs`, `session.rs` + Einstellungsseite (Erkennung, Anmelden, Modell) | Anmelde-Zustand korrekt auf allen 3 Plattformen; Unit-Tests für Protokoll-Parsing aus aufgezeichneten Streams. |
| 2 ✅ | `mcp.rs`, `tools.rs` mit Shell-Werkzeugen (`septet_*`) | Claude kann Tabs öffnen, SVG im Workspace schreiben und in Vectorcraft platzieren. |
| 3 | Apps steuern und sehen: `Embedded::control`/`render`, `HostedApp`, Werkzeuge `app_*`, Sperrliste (Detailplan unten) | Ändern, einzeln und gesamt rendern, Undo in allen 7 Apps. |
| 4 ✅ | Chat-Panel komplett (Streaming, Karten, Freigaben, Unterhaltungen, Eingabe-Abschirmung) | Bedienbar ohne Terminal. |
| 5 | Erzeugen per Code: System-Prompt + App-Hinweise, Workspace, Render-Prüfschleife; Vorlagen-Prompts („Logo als SVG", „Social-Media-Video aus Clips", „Lower Third animieren") | Beispiele in `docs/assistant/examples.md` reproduzierbar. |
| 6 | Videoschnitt: Filmcraft-Werkzeughinweise, Medien-Import, Frame-Kontrolle an Schnittpunkten, lange Exporte als Job mit Fortschritt (nicht blockierend) | Rohschnitt aus 5 Clips + Titel + Musik per Chat. |
| 7 | Computer Use im Tab (`computer.rs`, Router-Wächter, Overlay, Übernehmen/Esc, Einwilligung) | Klicks außerhalb des Tab-Bereichs nachweislich unmöglich (Test). |
| 8 | Tests & Doku: **Fake-`claude`** (kleines Test-Binary, spielt aufgezeichnete stream-json-Skripte ab und ruft MCP-Werkzeuge auf) → neues Autotest-Szenario `assistant` läuft in CI ohne Login; README-Abschnitt, NOTICE-Hinweis, RELEASE_NOTES | CI grün auf allen Plattformen. |

Stand nach Phase 2 (2026-10-09): Einstellungsdialog über „Claude…" auf der Startseite (Erkennung, Abo-Status,
Anmelden über `claude auth login`, Modell, Verbindungstest). Unterhaltung + MCP-Server + Werkzeuge
`septet_state/open/place/activate` + Freigabe-Dialog (vorläufig modal, Phase 4 macht es inline). Zusätzlich zum Plan:
`assistant/conversation.rs` (Transkript, Freigaben). Autotests: `SEPTET_AUTOTEST_SCENARIO=assistant` (Dialog +
Verbindungstest) und `assistant-tools` (Claude schreibt `logo.svg` und öffnet es in Vectorcraft; Modell per
`SEPTET_AUTOTEST_MODEL=haiku`). Beide brauchen ein angemeldetes Claude Code, laufen also nicht in der CI (→ Phase 8).
Noch nicht geprüft: Anmelde-Flow aus Septet heraus (User war schon angemeldet), Windows, macOS.

Stand nach Phase 4 (2026-10-09, vor Phase 3 gezogen): Panel rechts (`assistant/panel.rs`), Knopf in der Tab-Leiste,
`Strg+Umschalt+K`, „Claude" auf der Startseite. Streaming-Text mit kleinem Markdown-Renderer (`markdown.rs`),
aufklappbare Werkzeug-Karten (mit Bildern), Freigaben inline, Stop, Neue Unterhaltung, Verlauf (`history.rs`,
`conversations.json`, Fortsetzen per `--resume`; die alten Nachrichten zeigt das Panel dann nicht an), Plan-Auslastung
aus `rate_limit_event`. Tasten-Abschirmung: solange das Chat-Feld den Fokus hat, nimmt `shell.rs` Tastatur-Ereignisse
vor dem App-Aufruf heraus und danach wieder hinein. Autotest `assistant-panel` (tippt echte Tasten;
`SEPTET_AUTOTEST_PROMPT`, `SEPTET_AUTOTEST_APPROVE=allow`). **Noch offen aus Phase 4**: Kontext-Chips (Tab/Auswahl/
Screenshot anhängen), Dateien ins Panel ziehen, Kachel auf der Startseite, Zeiger-Ereignisse über dem Panel werden
nicht von der App ferngehalten (nur Tastatur).

Nach Phase 4 dazugekommen (2026-10-09, gebaut + Unit-Tests, **in der App noch nicht mit Claude getestet**):
- **Modell & Effort im Chat**: Chips unten im Eingabefeld (`panel.rs: composer_ui`), Liste in `mod.rs: MODELS`
  (haiku/sonnet/opus/fable, Aliase = immer neuestes Modell) und `EFFORTS` (low…max → `--effort`). Auch im
  Einstellungsdialog. Wechsel während einer Unterhaltung: `Assistant::ask` startet Claude Code mit `--resume` und
  neuen Flags neu (Einträge und Freigaben bleiben). Zu testen: Wechsel Haiku→Sonnet mitten im Chat.
- **Eingabefeld neu**: eine Box, wächst mit dem Text (1–8 Zeilen), runder Senden-/Stop-Knopf.
- **Neue Werkzeuge** (`assets.rs`): `septet_render` (SVG → PNG mit resvg, Bild geht an Claude zurück, Schriften
  aus dem Workspace werden geladen) und `septet_fetch` (Download nur von api.iconify.design, fonts.googleapis.com,
  fonts.gstatic.com, raw.githubusercontent.com/google/fonts/ in den Workspace; ureq + rustls + OS-Zertifikate wie
  Pdfcraft; keine Redirects; 25 MB). Laufen auf eigenem Thread. Zu testen: echter Download von Iconify.

## Phase 3: Apps steuern und sehen (Detailplan, nächster Schritt)

Stand der Recherche: 2026-10-09, drei Code-Durchsichten aller sieben Apps. Die Datei:Zeile-Angaben sind Startpunkte.
Vor dem Ändern nachlesen, denn die Zeilen verschieben sich.

### Ziel und Grundsätze

1. **Steuern ohne Computer Use.** Claude bedient jede App über deren Befehls-Registry, also dieselben Befehle wie
   Menüs und Shortcuts. Die Befehle sind rückgängig machbar und beschreiben sich selbst. Klicks auf Pixel bleiben die
   letzte Stufe (Phase 7).
2. **Sehen, was es tut.** Ein Werkzeug rendert ohne Fenster-Screenshot als PNG:
   - die ganze Leinwand, Seite oder Frame
   - **jede Ebene, jedes Objekt oder jeden Clip einzeln**, freigestellt mit transparentem Hintergrund
   
   Das Bild geht als MCP-Bild direkt an Claude. Fenster-Screenshots (`ui.screenshot`) sind in Septet unbrauchbar:
   Sie erfassen das ganze Septet-Fenster und brauchen einen gezeichneten Frame.
3. **Sauberes Backend.** Pro App ein In-Process-Kanal, kein TCP, kein eigener Server in der App. Das verlangt der
   Embed-Vertrag (`docs/embedding/phase1-embed-api.md:35`). Septets bestehender MCP-Server (`assistant/mcp.rs`) ist
   der einzige Server, und `assistant/tools.rs` leitet die Aufrufe weiter.
4. **Wenige, generische Werkzeuge** statt 7 × 30. Die Registry jeder App liefert die Details.

### Was die Apps schon haben

Alle sieben haben einen `ControlRequest { method, params, reply: Sender<Value> }` mit
`ControlRequest::new(method, params) -> (req, Receiver)`. Die Ausnahme ist Pdfcraft (siehe unten). Dazu kommen ein
Handler `handle(app, ctx, req) -> Outcome`, ein Builder `with_control(rx)` und `drain_control()` in `logic()`.
**Kein `embed.rs` reicht davon etwas durch**, und die Felder sind privat. Undo und Redo laufen überall über die
Befehle `edit.undo` / `edit.redo`.

| App | Handler / Anbindung | Registry (Selbstbeschreibung) | Zustand abfragen |
|---|---|---|---|
| Photocraft | `photocraft/crates/ui-egui/src/control.rs:137`, `with_control` lib.rs:601, `drain_control` lib.rs:903 | `engine.commands`; `engine/src/commands.rs:15` (`CommandSpec`), params als Text | `document.inspect` (`engine/src/inspect.rs:25`: Ebenenbaum, Auswahl, Verlauf) |
| Vectorcraft | `vectorcraft/crates/ui-egui/src/control.rs:124`, `with_control` lib.rs:434, `drain_control` lib.rs:573 | `engine.commands` (`all_commands` :66), params als Text | `document.inspect {depth, childLimit}`, `document.node {id}`, `document.find`; **nicht** `document.json` (riesig) |
| Lightcraft | `lightcraft/crates/ui-egui/src/control.rs:144`, `with_control` lib.rs:259, `drain_control` lib.rs:445 | `engine.commands`, params als Text | `photo.inspect`, `develop.get`, `catalog.query`, `history.list` |
| Designcraft | `designcraft/crates/ui-egui/src/control.rs:102`, `with_control` lib.rs:470, `drain_control` lib.rs:815 | `engine.commands` (`engine/src/cmd/mod.rs:69/84`), params als Text, `undoable` | `document.inspect` (`engine/src/cmd/inspect.rs:52`: Seiten, Items, Stories, Ebenen; groß) |
| Pdfcraft | **anders**: `attach_control(ctx) -> ControlClient` (`ui-egui/src/lib.rs:968`), hängt ein egui-Plugin und AccessKit an den **gemeinsamen** ctx | `ui.commands` (`engine/src/commands.rs:150`), nur IDs, **keine Parameter** | `ui.state`; ausführlicher `DocInfo` (`render/src/inspect.rs:35`) |
| Filmcraft | `filmcraft/crates/ui-egui/src/control.rs:84`, `with_control` lib.rs:428, `drain_control` lib.rs:1093 | `engine.commands` (~560 Befehle, `engine/src/commands.rs:2464`), params als Text | `project.inspect`, `sequence.inspect {item?}`, `state.inspect`, `history.list` |
| Effectcraft | `effectcraft/crates/ui-egui/src/control.rs:167`, `with_control` lib.rs:379, `drain_control` lib.rs:1233 | `command.list {schemas:true}` mit **echtem JSON-Schema**, `command.describe`; `engine.batch` (ein Undo-Schritt) | `project.summary`, `comp.info`, `layer.tree`, `prop.get`, `editor.state` |

Agenten-Doku der Apps, gut für Werkzeugbeschreibungen und den Skill:
- `<app>/AGENTS.md` und `<app>/docs/control-protocol.md`
- `filmcraft/docs/agents.md`, `effectcraft/docs/agents.md`
- `photocraft/book/src/automation/mcp.md`
- `pdfcraft/crates/automation/README.md` (~120 Werkzeuge mit Schemas, `crates/automation/src/tools.rs`)

### Rendern ohne Fenster (Kern von „sehen“)

| App | Ganzes Dokument | Einzeln (Ebene / Objekt / Clip) | Technik |
|---|---|---|---|
| Photocraft | `compose::thumbnail(doc, max_side)` / `flatten` (`photocraft/crates/compose/src/lib.rs:285/252`) | **`render_layer(&Layer, Rect) -> Buffer`** (:257), gilt auch für Gruppen | CPU |
| Vectorcraft | `Renderer::render_region_with(doc, rect, scale, white, &RenderOptions)` (`crates/render/src/lib.rs:562`) je Zeichenfläche | **`render_node_thumbnail(doc, NodeId, size)`** (:585); Ebenen sind auch Nodes; alternativ `RenderOptions.hidden` | CPU (vello_cpu) |
| Lightcraft | `Session::render_now(id, w, h)` (`engine/src/media.rs:933`) | Maske einzeln: `RenderJob::with_overlay(Overlay::Mask{..})` (media.rs:500) | GPU, sonst CPU |
| Designcraft | `Renderer::render_page(doc, cache, abs, scale, bleed, &opts)` (`crates/render/src/lib.rs:351`); `ui.render` liefert schon PNG | Item: alle anderen in `RenderOptions.hidden` + `paper:false`; Vorbild `render_objects()` in `apps/designcraft/src/embed.rs:373`; Ebene: Items anderer Ebenen ausblenden | CPU |
| Pdfcraft | `PageRenderer::new(bytes, cfg).render(RenderRequest{page, scale..})` (`crates/render/src/raster.rs:188/207`), Bytes aus `app.session` | OCG-Ebenen über `RenderConfig.layers`, `hide_comments`; einzelne Anmerkung: nicht vorhanden (bei Bedarf Ausschnitt um `rect` rendern) | CPU (hayro) |
| Filmcraft | `filmcraft_render::render_sequence(project, seq, t, opts, provider)` (`crates/render/src/lib.rs:91`); Vorbild `export_active` in embed.rs:238 | **`render_clip(project, seq, clip, t, ..)`** (:564), Projekt-Item: `render_item` (:615) | CPU, synchron |
| Effectcraft | `Session::render_rgba8_alpha(comp, t, max_side, transparent)` (`engine/src/lib.rs:879`), auch als Methode `render.frame` | kein fertiger Weg; `Renderer::layer_buf` (`render/src/lib.rs:1168`) nach außen führen. **Nicht** den Solo-Schalter setzen, das ist eine Projektänderung | GPU, sonst CPU |

Regeln für `app_render`:
- Längste Kante ≤ 1568 px (Standard 1024), PNG und transparent, wo es Sinn ergibt.
- Optional `save_as` in den Workspace, damit Claude Ergebnisse vergleichen oder an andere Apps geben kann.
- Große Renders nicht im UI-Thread: Auf dem UI-Thread nur den nötigen Zustand klonen oder snapshotten, dann auf
  einem Worker-Thread rendern, so wie es `septet_render`/`septet_fetch` in `tools.rs: drain` schon machen.
- Wo der Klon zu teuer ist, zuerst synchron rendern und messen.

### Das größte Hindernis: unsichtbare Tabs

Septet ruft `logic()`/`ui()` einer App nur auf, solange ihr Tab sichtbar ist (`septet/src/shell.rs` ≈ 469-487,
`window_ui`). Anfragen an `drain_control` bleiben sonst liegen, und `Retry`-Fristen laufen ab. Lösung in zwei Wegen:

1. **Direktaufruf (Standard für Befehle, Abfragen und Renders).** `Embedded` bekommt
   `fn control(&mut self, ctx, method, params) -> Value`. Es ruft den App-Handler `handle(&mut app, ctx, &req)`
   **synchron** auf, und zwar innerhalb von `AppSlot::with_app` + `Isolation`-Swap (`hosted.rs:209/242`). Das ist
   nötig, weil alle Apps einen ctx teilen und ihren eigenen `ctx.data` lesen.
   - `Outcome::Done(v)` → fertig.
   - Aufgeschobene Outcomes (`Retry`, `AfterInput`, `AfterJob`, `AwaitJobs`, `Screenshot`) → Fallback auf Weg 2.
   - Funktioniert, ohne dass der Tab sichtbar ist.
2. **Über die Warteschlange (für `ui.*`, Jobs und alles Aufgeschobene).** Den Tab aktivieren, die Anfrage über den
   per `with_control` angehängten Kanal schicken und mit Zeitlimit warten. Das bestehende `septet_activate` liefert
   die Logik.
   - Optional: Septet ruft für einen unsichtbaren Tab mit offenen Anfragen pro Frame nur `logic()` auf (ohne `ui()`).
     Vorher prüfen, ob die Apps das vertragen.

**Wann der Tab nach vorne kommt.** Laut User-Entscheidung vom 2026-10-08 darf Claude den Ziel-Tab sichtbar
aktivieren. Empfehlung:
- Bei **ändernden** Befehlen den Tab aktivieren, damit der User sieht, was passiert.
- Bei Abfragen und Renders den Tab nicht wechseln.

### Werkzeuge (MCP, in `assistant/tools.rs`)

| Werkzeug | Eingaben | Verhalten |
|---|---|---|
| `app_commands` | `app`, `filter?`, `enabled_only?` | Registry der App, normalisiert auf `{id, label, menu, params, enabled, disabled_reason?, undoable?}`. `params` ist JSON-Schema (Effectcraft) oder Text (andere). Filter Pflicht, wenn die Liste groß wird (Filmcraft ~560) |
| `app_execute` | `app`, `command`, `params?` | ein Befehl über `engine.execute`; Ergebnis kurz. Befehle, die Dialoge öffnen (`ui.menu.invoke` ohne params), blockieren |
| `app_batch` | `app`, `steps[]` | Effectcraft: `engine.batch` (ein Undo-Schritt); sonst nacheinander, Abbruch beim ersten Fehler |
| `app_inspect` | `app`, `what` (`document`, `selection`, `layer`, `node`, `sequence`, `comp`, `page`, `history`…), `id?`, `depth?` | auf die Abfrage der App abgebildet; **Antworten kürzen** (MCP-Grenze ~25 000 Tokens): `depth`/`childLimit` nutzen, lange Listen abschneiden mit Hinweis |
| `app_render` | `app`, `target` (`document`/`page`/`frame`/`layer`/`object`/`clip`/`mask`), `id?`, `page?`, `time?`, `max_side?`, `background?`, `save_as?` | PNG als MCP-Bild (wie `septet_render`) |
| `app_undo` | `app`, `steps?` | `edit.undo` / `edit.redo` |

- Jedes Werkzeug startet die App bei Bedarf (wie `septet_activate`).
- Antwortet eine App nicht, kommt ein klarer Fehler zurück („Vectorcraft has no document open; use septet_open“).
- `panel.rs: label()` bekommt lesbare Texte wie „Ran layer.duplicate in Photocraft“ oder „Looked at layer ‘Sky’“.

**Sperrliste, in Septet immer blockiert.** Diese Methoden wirken auf das Host-Fenster:
- `app.quit`, `ui.resize`, `ui.focus`, `ui.screenshot`
- `ui.gpu.simulateLoss`
- Fenster-/Workspace-Befehle wie `ui.window.*`

Zusätzlich über die Freigabe (`approve`):
- Speichern über bestehende Dateien des Users und Export außerhalb des Workspace
- `app.open` auf Pfade außerhalb von Workspace und zuletzt geöffneten Dateien

Speichern und Exportieren bleiben sonst Sache des Users.

### Besonderheiten je App

- **Pdfcraft:**
  - **Nicht** `attach_control` nutzen: Es hängt ein egui-Plugin und AccessKit an Septets gemeinsamen ctx.
  - Stattdessen in `embed.rs` direkt `state()`, `commands()`, `execute(id)` und `render_page()` auf Basis von
    `app.session` (pub, lib.rs:326) und `PageRenderer` anbieten.
  - Die Registry hat keine Parameter. Für parametrisierte Aktionen (Text, Kommentare, Seiten) prüfen, ob sich die
    Automation-Werkzeuge (`crates/automation`) auf die laufende Session umbiegen lassen. Heute besitzt `Automation`
    seine eigene Session.
- **Photocraft:**
  - Im Embed sind `services::native(None)` → `automation_read`/`write` = None (`embed.rs:59`). Darum schlagen
    `app.open`/`app.save` fehl. Öffnen weiter über `septet_open`.
  - `LayerId` ist nur innerhalb der Sitzung stabil (globaler Zähler).
- **Vectorcraft:** `NodeId` ist dauerhaft stabil. Für Ebenen und Objekte gibt es dieselbe Render-Funktion.
- **Designcraft:** `document.inspect` ist groß, ggf. pro Seite filtern. Ebenen als `LayerId`, Items als
  `ItemId` (stabil).
- **Filmcraft:**
  - Zeiten intern in Ticks (254 016 000 000 / s); Befehle akzeptieren auch Sekunden, Frames und Timecode.
  - `engine.execute` braucht den UI-Dispatcher; Renders sind CPU und synchron.
  - Lange Exporte als Job (`file.exportMedia`, `jobs.list`).
- **Effectcraft:**
  - Beste Registry (JSON-Schemas, `engine.batch` mit `$N.key`-Verweisen).
  - Eigenschaften über Pfade (`transform/position`); Layer per ID, `#n` oder Name.
  - `run_script` (JSX) prüfen, ob es sinnvoll freigegeben werden kann.
- **Lightcraft:**
  - Fotobibliothek statt Ebenen: „einzeln“ heißt hier Foto oder Maske.
  - `ui.render` ohne Pfad liefert kein Bild; darum `render_now` direkt nutzen.

### Umsetzung in Schritten

1. ✅ **Spec:** `docs/embedding/phase5-agent-control.md`. Statt `control`/`render` vier Methoden: `agent_commands`,
   `agent_execute` (Antwort über `Receiver`, auch aufgeschoben), `agent_inspect`, `agent_render` (Beschriftung + Job für
   einen Worker-Thread, liefert `egui::ColorImage`).
2. ✅ **Pilot Vectorcraft** (einfachste App):
   - `Embedded::control` und `Embedded::render`
   - `HostedApp` um `control`/`render` erweitern, im `hosted!`-Makro durchreichen (`hosted.rs:44`); Apps ohne
     Umsetzung bekommen einen Default „not supported“
   - `app_commands`/`app_execute`/`app_inspect`/`app_render`/`app_undo` in `tools.rs`
   - Autotest: Rechteck per Befehl anlegen, Objekt einzeln rendern, Undo

   **Stand Phase 3 (2026-10-09):** Vectorcraft fertig und getestet. Host: `assistant/apps.rs` (Werkzeuge, Warteliste für
   startende Apps und Freigaben, Pfadregeln, Hintergrund und PNG), `hosted.rs` (Trait-Methoden mit Default „kann noch
   nicht“, Makro-Arm `hosted!(…, agent)`), Freigabe-Karten und Beschriftungen in `panel.rs`, System-Prompt erwähnt die
   Werkzeuge. Vectorcraft (`apps/vectorcraft/src/embed.rs`): Registry normalisiert (939 Befehle), Befehle über
   `app.run`; öffnet ein Befehl einen Dialog, wird er wieder geschlossen und Claude bekommt einen Fehler; Ansichten
   `document`/`object`/`selection`/`find`/`history`/`documents`; Renders von Zeichenfläche, Objekt/Ebene (freigestellt
   über `fileio::objects_document`) und Auswahl. Autotest `SEPTET_AUTOTEST_SCENARIO=assistant-apps` (ruft die
   Werkzeuge direkt auf, ohne Claude, kein Login nötig): App starten per `file.new`, Formen, Abfragen und Renders bei
   verstecktem Tab, Dialog-Schutz, Sperrliste, Undo. Echter Chat mit Haiku: neues Dokument, Kreis und Quadrat, Farben,
   Prüf-Render, in 10 s ohne Fehlversuch.
   **Photocraft** fertig: Registry aus `command_specs()` (817 Befehle, mit Grund für „nicht verfügbar“), nur
   Engine-Befehle (öffnen nie einen Dialog), über `PhotocraftApp::control_now` (neu, auch `drain_control` nutzt es),
   damit Hintergrund-Jobs ihre Antwort später schicken; Ansichten `document`/`layer`/`selection`/`history`/`documents`;
   Renders: Komposit per `compose::thumbnail`, Ebenen allein und beschnitten über `content::layers_alone` + `trimmed`
   (wie beim Rausziehen). Autotest: `SEPTET_AUTOTEST_APP=photocraft` (Szenario jetzt pro App).
   **Designcraft** fertig: wie Vectorcraft über `app.run` (550 Befehle) mit Dialog-Schutz; zusätzlich wird ein Befehl,
   der einen Dateiauswahl-Dialog öffnen würde (`UiRequest::Pick`, z. B. `file.place` ohne `path`), abgefangen
   (`services.pick_open` vorübergehend ersetzt). Ansichten `document` (Seiten mit Top-Level-Items, `page` filtert,
   `depth`), `page`, `object`, `story` (ganzer Text), `selection`, `history`, `documents`. Renders: Seite (mit Papier),
   Objekt/Auswahl freigestellt (`objects_image`, auch für das Rausziehen), Ebene = Seite nur mit deren Items. Dokument
   und Satz-Cache sind `Arc`s, der Job rendert auf dem Worker. Claude kann jetzt Designcraft-Dokumente selbst anlegen
   (System-Prompt angepasst).
   **Effectcraft** fertig: 665 Engine-Befehle mit Text-Parametern und JSON-Schema (Septet lässt die Schemas weg, wenn
   mehr als 12 Befehle gelistet werden), über das neue `EffectcraftApp::control_now`. `drain_control` lief bisher nur
   mit TCP-Kanal; jetzt arbeitet es aufgeschobene Anfragen (`Retry`, Job-Wartende) auch ohne ab (Standalone mit
   `--control` geprüft). Ansichten `document`, `comp`, `layer` (Eigenschaftsbaum), `property`, `selection`, `history`.
   Renders über neues `Session::frame_job` (wie `thumbnail_job`, CPU, eigener Thread): Komposition über ihrem
   Hintergrund; Ebene(n) allein, indem in einer Kopie des Projekts die anderen Ebenen ausgeschaltet werden (Mattes,
   Kameras, Lichter bleiben; nie der Solo-Schalter). Septet schneidet transparente Ränder von Teil-Renders ab.
   Gefunden und behoben: Effectcrafts `path` ist ein Eigenschaftspfad (`transform/position`); Septets Pfadregel nimmt
   `path` jetzt nur als Datei, wenn er danach aussieht (absolut, `./`, `../`, `~`, Endung) oder der Befehl speichert.
   **Filmcraft** fertig: 675 Engine-Befehle; ausgeführt über `menus::invoke` (wie `engine.execute` im
   Steuerprotokoll, immer sofort fertig). Filmcraft hat ~15 einzelne Dialog-Felder statt einem: `open_dialogs` merkt
   sich, welche offen waren, und schließt neu geöffnete wieder; alle Datei-Dialog-Hooks (`pick_*`) und `open_path`
   werden für den Befehl durch Stubs ersetzt (Fehler „pass the path“). Ansichten `document`, `sequence` (mit
   `ticksPerSecond`), `clip`, `selection`, `history`. Renders: Sequenz-Frame (auf Schwarz, wie der Program Monitor),
   Clip allein (`render_clip`, Septet schneidet zu), Auswahl (erster ausgewählter Clip), Projekt-Item; der
   `PoolProvider` ist `Send`, gerendert wird auf dem Worker. Prompt: Clips per `timeline.place` statt nur OTIO.
   **Lightcraft** fertig: 373 Befehle über `app.run` (wie Vectorcraft), Dialog-Schutz (`ui.dialog`), Datei-Dialoge
   und „in anderem Programm öffnen“ abgefangen. Ansichten `document` (Bibliothek + Statistik), `photos` (Filter,
   Seite), `photo`, `develop`/`mask`, `controls` (Regler-IDs wie `light.exposure`), `albums`, `history`. Renders über
   `Session::render_job` (auf dem Worker): entwickeltes Foto, `before` (unbearbeitet), `mask` (weiß auf schwarz oder
   `view: color`). Test im Portable-Modus (eigene Demo-Bibliothek, nicht die des Users).
   **Hinweis Testumgebung (2026-10-09 ~12:40):** NVIDIA-Treiber und -Bibliotheken passen nicht mehr zusammen
   (Kernelmodul 615.71.09, Bibliotheken 615.78.08 nach einem Update) → neue Prozesse bekommen keine GPU, Septet stürzt
   auf `:99` in wgpu ab („Invalid surface“). Bis zum Neustart mit Software-GL testen: `WGPU_BACKEND=gl
   LIBGL_ALWAYS_SOFTWARE=1 __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json`.
   Beobachtet, offen: Ein per Befehl angelegtes Dokument war im Chat-Test nicht ganz eingepasst (Zoom zu groß, links
   abgeschnitten); vermutlich eine Embedding-Frage von Vectorcrafts `canvas::fit`, nicht der Werkzeuge.
3. **Photocraft, Designcraft, Effectcraft, Filmcraft**, dann **Lightcraft**, zuletzt **Pdfcraft** (Sonderweg).
4. **Warteschlangen-Weg** für aufgeschobene Outcomes und `ui.*`, inklusive Tab-Aktivierung und Zeitlimits.
5. **Skill und Prompt:**
   - `septet:septet-apps` um die `app_*`-Werkzeuge ergänzen: Prüfschleife „Befehl → `app_render` (Ebene/Ganzes) →
     korrigieren“, wichtige Befehle je App.
   - Pro App evtl. ein kurzer Skill (`septet:photocraft` …) mit den häufigsten Befehls-IDs aus den `AGENTS.md`.
   - System-Prompt in `tools.rs` kurz anpassen.
6. **Tests:**
   - Unit-Tests für die Normalisierung (Registry, Kürzen).
   - Autotest `assistant-apps`: pro App ein Befehl, eine Abfrage, ein Render von Dokument und Ebene. Nicht-leeres
     PNG prüfen und Screenshot anschauen.
   - Danach ein echter Chat mit Haiku (`SEPTET_AUTOTEST_MODEL=haiku`).

**Abnahme:** In jeder App kann Claude per Chat etwas ändern, sich das Ergebnis als Bild ansehen (gesamt und eine
Ebene, ein Objekt oder einen Clip einzeln) und es rückgängig machen. Das gilt auch dann, wenn der Tab gerade nicht
sichtbar ist (Abfragen und Renders).

## Phase 4b: Skills & Werkzeuge (erledigt 2026-10-09)

Recherche (2026-10-09, Lizenzen an den Quellen geprüft):
- **Anthropic-Skills** (github.com/anthropics/skills): `canvas-design`, `algorithmic-art`, `theme-factory`,
  `frontend-design` sind Apache-2.0 → **mitliefern** (unverändert, LICENSE.txt behalten, in NOTICE nennen;
  canvas-design bringt OFL-Schriften mit). `docx`, `pdf`, `pptx`, `xlsx` sind **proprietär → nicht mitliefern**.
  Rest (brand-guidelines, web-artifacts-builder, …) passt nicht.
- **Community**: nichts direkt übernehmen. Als Vorlage taugen kaankiziltug/logo-design-skill (MIT),
  Leonxlnx/taste-skill (MIT), lucifer1004/claude-skill-typst (MIT). Optional für Nutzer: heygen-com/hyperframes
  (Apache, HTML+GSAP → MP4). Nicht: remotion-dev/skills (keine Lizenz), OpenMontage (AGPL).
- **Fremde MCP-Server: keine** (wenig gepflegt, Supply-Chain-Risiko); stattdessen eigene Werkzeuge + Skills, die
  Iconify/Google Fonts/ffmpeg/magick/qpdf erklären.
- **Eingebaute Tools**: WebSearch und WebFetch zusätzlich freischalten (laufen über `approve`). Glob/Grep behalten.

Laden (selbst geprüft mit CLI 2.1.295): `--plugin-dir <ordner>` mit `.claude-plugin/plugin.json` + `skills/<name>/SKILL.md`,
Skills heißen dann `septet:<name>`. **`--disable-slash-commands` schaltet alle Skills ab → entfernen.**
`--setting-sources ""` bleibt (lädt unser Plugin, aber keine persönlichen Skills/Plugins des Nutzers). **Nicht**
`project` als Setting-Source nehmen: Claude könnte sonst im Workspace eine `.claude/settings.json` mit Hooks
anlegen und Freigaben umgehen. Eingebaute Claude-Code-Skills (dataviz, design, deep-research, …) bleiben sichtbar.

Stand im Repo:
- [x] `septet/assistant-plugin/` mit `.claude-plugin/plugin.json`, `NOTICE.md` (Commit-Hash der Quelle) und den
  4 Anthropic-Skills (5,8 MB, davon Schriften).
- [x] Eigene Skills (`assistant-plugin/skills/`): `septet-apps` (Drehscheibe: welche App was öffnet, Workspace,
  Prüfschleife, CLI-Tools prüfen), `svg-graphics` (was Vectorcrafts Import behält), `logo-and-icons` (Iconify-Suche/
  -Download, Lizenzen je Set gegen die Iconify-API geprüft), `typography` (Google Fonts per `septet_fetch`, liefert TTF;
  `text_to_path.py` wandelt Text mit fontTools in Pfade), `color`, `print-and-pdf`, `image-editing`, `video-editing`
  (OTIO-Vorlage gegen `interchange.rs` geprüft), `motion-lottie` (Lottie nur über Effectcrafts Menü, Feature-Tabelle aus
  `lottie.rs`).
- [x] Plugin im Binary: `build.rs` erzeugt `FILES`/`HASH` (FNV-1a), `assistant/extensions.rs` entpackt nach
  `data_dir("Assistant")/plugin-<hash>/` (atomar per rename, alte Versionen werden gelöscht).
- [x] `cli.rs`: `--disable-slash-commands` raus, `--tools` + `Skill,WebSearch,WebFetch`, je Plugin `--plugin-dir`
  und eine Regel `Read(//<plugin-ordner>/**)` in `--allowedTools` (sonst fragt jedes Lesen von Skill-Dateien nach;
  mit CLI 2.1.295 geprüft, auch die Gegenprobe ohne Regel). Windows-Form der Regel (`//C:/…`) ungetestet.
- [x] `panel.rs`: Beschriftungen für Skill/WebSearch/WebFetch/`septet_render`/`septet_fetch`/fremde MCP-Werkzeuge;
  `approval_key` für WebFetch nach Host (`conversation::host`).
- [x] System-Prompt (`tools.rs`) nennt die Skills und die wichtigsten App-Grenzen.
- [x] NOTICE.md + `scripts/third_party_licenses.py` (Apache-Skills, OFL-Schriften von canvas-design).
- [x] **Eigene Erweiterungen**, Einstellungsdialog → „Skills and extensions“ (zugeklappt):
  - eigene Skills in `config_dir/assistant/my-skills/skills/<name>/SKILL.md` (Plugin „my“, Skills `my:<name>`;
    `.claude-plugin/plugin.json` legt Septet an), Plugins in `config_dir/assistant/plugins/<plugin>/`; je ein
    `--plugin-dir`. Plugins mit Hooks werden markiert („runs commands without asking“).
  - MCP-Server: Liste (Name, Befehl oder http(s)-URL, an/aus) in `Settings::mcp_servers`, landet neben `septet` in der
    `--mcp-config`; ihre Werkzeuge stehen nicht in `--allowedTools`, laufen also über `approve`.
  - Opt-in „Also use my Claude Code settings, skills and plugins“ → `--setting-sources user` (mit Warnung). MCP-Server
    aus `~/.claude.json` werden auch dann nicht geladen (`--strict-mcp-config` bleibt, sonst könnte eine `.mcp.json`
    im Workspace mitkommen).
  - Änderungen gelten für neue Unterhaltungen (laufende werden nicht neu gestartet).
- [x] Test: Autotest `assistant-panel` mit Haiku und Prompt „Logo für Bäckerei Crumb“ (englisch): Skill-Aufrufe
  `logo-and-icons` + `septet-apps`, Iconify-Suche und -Download per `septet_fetch`, zweimal `septet_render`, Öffnen in
  Vectorcraft. Haiku nahm dabei trotz Skill-Hinweis „Georgia“ (nicht installiert → Vectorcraft markiert fehlende
  Schrift). Einstellungsdialog per Autotest `assistant` mit `SEPTET_CONFIG_DIR` (Testskill, Plugin mit Hooks, MCP-Liste).
- Offen: eigene MCP-Server und `own_setup` noch nicht mit einem echten Server/Gespräch durchgespielt; Windows/macOS.

Reihenfolge 0 → 1 → 2 liefert früh einen nutzbaren Kern (Claude erzeugt SVG und legt es in Vectorcraft ab);
3–7 bauen darauf auf und sind einzeln auslieferbar.

| 9 | API-Key-Modus (BYOK): eigene Agent-Schleife über die Messages API, gleiche Werkzeuge, Key im OS-Schlüsselbund | Optional, wenn Policy oder Nutzerwunsch es nötig machen. |

## Risiken / offene technische Punkte

- **stream-json-Steuerprotokoll** (Interrupt, Permission-Nachrichten) ist nicht öffentlich dokumentiert; das
  Agent SDK nutzt es intern. Abhilfe: nur dokumentierte Flags + `--permission-prompt-tool`; Abbrechen notfalls
  per Prozess-Kill + `--resume`. CLI-Version in den Einstellungen anzeigen, Mindestversion prüfen.
- **Kontextgröße**: Screenshots und große `inspect`-Antworten kosten viel; Antworten kürzen/paginieren,
  `MAX_MCP_OUTPUT_TOKENS` beachten (Standard 25 000).
- **Gemeinsame GPU / Hauptthread**: Rendern für Claude passiert im Frame der App; lange Renders über die
  vorhandenen Job-Systeme der Apps, nicht im UI-Thread.
- **Abgerissene Fenster**: kein eframe-Screenshot für Zusatzfenster (siehe `autotest.rs:261`).
- **Policy kann sich ändern** → Integration klar als „nutzt dein lokales Claude Code" kapseln; ein späterer
  API-Key-Modus (Messages API direkt) wäre ein zweites Backend hinter derselben `session`-Schnittstelle.
