# Plan: Claude in Septet

Stand: 2026-10-09. Status: Phasen 0–2, 4 und 4b erledigt (Spike: `protocol-notes.md`); als Nächstes Phase 3.

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

- Jede App bekommt in `apps/<app>/src/embed.rs` eine Methode `set_control(rx: Receiver<ControlRequest>)`, die
  das vorhandene `with_control(rx)` aufruft (Pdfcraft: Gegenstück zu `attach_control`). Ein Kanal pro App,
  ohne TCP. `docs/embedding/phase1-embed-api.md` um „Phase 5: Steuerkanal" ergänzen.
- `hosted.rs`: `HostedApp` bekommt `fn control(&mut self, method: &str, params: Value) -> Receiver<Value>`.
  Die vier verschiedenen Antworttypen (`Value`, `ControlResponse`, `Reply` …) werden im bestehenden
  `hosted_app!`-Makro auf `serde_json::Value` normalisiert.
- **Wichtig**: Pro Fenster läuft nur die App des sichtbaren Tabs (`shell.rs:438-456`, `app.logic` →
  `drain_control`). Darum aktiviert der Dispatcher vor jedem App-Aufruf den Ziel-Tab (oder öffnet die App)
  und wartet mit Zeitlimit auf die Antwort. Nebeneffekt, gewollt: Man sieht, woran Claude gerade arbeitet.
- `app.quit` und Ähnliches werden in Septet blockiert (Allowlist der Methoden pro App).

## Werkzeuge für Claude (bewusst wenige und generisch)

Statt 7 × ~25 App-Werkzeugen ein schlanker Satz. Die App-Befehle sind über `commands`/`execute` erreichbar,
die Registry beschreibt sich selbst (IDs, Parameter, enabled).

| Werkzeug | Zweck |
|---|---|
| `septet_state` | Fenster, Tabs, aktive App, Dokumenttitel, ungespeichert ja/nein |
| `septet_open(paths, app?)` / `septet_place(path, app, at?)` | Datei öffnen bzw. ins aktive Dokument platzieren (`OpenRequest`) |
| `septet_activate(app)` | Tab zeigen/App starten |
| `app_commands(app, filter?)` | Befehlsliste mit Parametern (gefiltert, damit es klein bleibt) |
| `app_execute(app, command, params)` / `app_batch(app, steps)` | Befehle ausführen (rückgängig machbar) |
| `app_inspect(app, what)` | Dokument-/Projekt-/Ebenen-/Sequenz-Zustand |
| `app_render(app, …)` | Vorschau als PNG (Canvas, Seite, Frame bei Zeit t) → Claude prüft sein Ergebnis |
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
| 3 | Steuerkanal in alle 7 Apps (`set_control`, `HostedApp::control`, Makro), Tab-Aktivierung, Methoden-Allowlist | `app_commands/execute/inspect/render` gegen alle 7 Apps. |
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
