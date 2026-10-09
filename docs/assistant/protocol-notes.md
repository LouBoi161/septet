# Protokoll-Notizen (Phase-0-Spike)

Geprüft am 2026-10-08 mit Claude Code **2.1.295**, Linux, Abo-Anmeldung (`authMethod: "claude.ai"`), Modell
`haiku`. Aufzeichnungen: `septet/tests/fixtures/assistant/conversation.jsonl` (5 Runden) und `resume.jsonl`
(Pfade anonymisiert). Windows und macOS noch nicht geprüft.

## Start

```
claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages
       --mcp-config <datei> --strict-mcp-config --tools Read,Write,Edit,Glob,Grep,Bash
       --permission-prompt-tool mcp__septet__approve --setting-sources "" --model haiku
       --append-system-prompt <text> --session-id <uuid>      (bzw. --resume <uuid>)
```

- Umgebung ohne `ANTHROPIC_API_KEY`/`ANTHROPIC_AUTH_TOKEN` → `system/init.apiKeySource == "none"`, Abrechnung übers Abo.
- `stderr` blieb in allen Läufen leer.
- `--session-id <uuid>` wird übernommen; `--resume <uuid>` setzt die Unterhaltung im **gleichen** Workspace-Ordner
  fort (Claude Code speichert unter `~/.claude/projects/<cwd-mangled>/`, also cwd beim Fortsetzen gleich halten).
- `--setting-sources ""` lädt keine Nutzer-Einstellungen, aber eingebaute Skills/Slash-Commands bleiben
  sichtbar (`init.skills`). Für Phase 1 zusätzlich `--disable-slash-commands` setzen.
- `auth status` liefert auch E-Mail und Org → Septet zeigt nur `authMethod` und `subscriptionType`.

## Eingabe (stdin, eine JSON-Zeile pro Nachricht)

```json
{"type":"user","message":{"role":"user","content":"Text"},"parent_tool_use_id":null,"session_id":"<uuid>"}
{"type":"user","message":{"role":"user","content":[{"type":"text","text":"…"},
  {"type":"image","source":{"type":"base64","media_type":"image/png","data":"…"}}]}, …}
{"type":"control_request","request_id":"int-1","request":{"subtype":"interrupt"}}
```

- **Bilder in der Eingabe funktionieren** (Content-Blöcke wie in der Messages API).
- **Abbrechen**: `control_request`/`interrupt` → sofort `control_response` (`subtype: success`,
  `response.still_queued: []`), danach `user`-Text „[Request interrupted by user]" und `result` mit
  `subtype: "error_during_execution"`, `is_error: true`, `terminal_reason: "aborted_streaming"`.
  **Der Prozess bleibt am Leben**, die nächste Nachricht läuft normal. Kill + `--resume` nur als Notfall.

## Ausgabe (stdout, eine JSON-Zeile pro Ereignis)

| `type` | Bedeutung |
|---|---|
| `system`/`init` | **pro Runde** (nicht nur einmal): `session_id`, `model`, `tools`, `mcp_servers[{name,status}]`, `apiKeySource`, `claude_code_version`, `permissionMode`, `cwd` |
| `system`/`status` | `status: "requesting"` vor jedem API-Aufruf |
| `system`/`thinking_tokens` | geschätzte Denk-Tokens (Text der Denkblöcke ist leer) |
| `stream_event` | rohe API-Stream-Ereignisse in `event`: `message_start`, `content_block_start/delta/stop` (`text_delta`, `input_json_delta`, `thinking_delta`), `message_delta`, `message_stop` |
| `assistant` | fertige Nachricht pro Inhaltsblock (`text`, `thinking`, `tool_use{id,name,input}`) |
| `user` | Werkzeug-Ergebnisse (`tool_result{tool_use_id,content,is_error}`), dazu `tool_use_result` (rohes Ergebnis) |
| `rate_limit_event` | `rate_limit_info{status, rateLimitType: "five_hour", resetsAt, unifiedWindows.five_hour.utilization}` → für die Statuszeile |
| `control_response` | Antwort auf `control_request` |
| `result` | Ende der Runde: `subtype` (`success`/`error_during_execution`), `is_error`, `result` (Text), `num_turns`, `total_cost_usd`, `usage`, `permission_denials[]`, `terminal_reason` |

Parser muss unbekannte `type`s und Felder ignorieren (es gibt viele zusätzliche, z. B. `capabilities`).

## MCP (Streamable HTTP, Septet als Server)

- Claude Code schickt `POST /mcp` mit `Accept: application/json, text/event-stream`, Bearer-Token wie in der
  Konfiguration. **Reine `application/json`-Antworten reichen**; `GET /mcp` (SSE-Kanal) mit `405` beantworten
  wird akzeptiert. `DELETE` kam nicht.
- `initialize` mit `protocolVersion: "2025-11-25"` → dieselbe Version zurückgeben. Es kommen **zwei**
  `initialize` pro Prozess (zwei Verbindungen) – der Server muss mehrfach initialisierbar sein.
  `Mcp-Session-Id` im Antwort-Header wird danach mitgeschickt (optional).
- Benachrichtigungen (`notifications/initialized`, ohne `id`) → `202` ohne Inhalt.
- `tools/call` trägt `_meta.claudecode/toolUseId` → Zuordnung zur Werkzeug-Karte im Chat.
- **Bildergebnisse** (`{"type":"image","data":<b64>,"mimeType":"image/png"}`) sieht Claude direkt (rotes
  Testbild korrekt erkannt). Claude Code speichert sie zusätzlich unter `~/.claude/projects/…` und hängt
  einen Textblock „[Image: source: …]" an.
- Werkzeugname im Stream: `mcp__septet__<name>`. Das `approve`-Werkzeug taucht **nicht** in `init.tools` auf.

## Freigaben (`--permission-prompt-tool`)

- Aufruf: `tools/call` `approve` mit `{"tool_name","input","tool_use_id"}`.
- Antwort als Text-Block mit JSON: `{"behavior":"allow","updatedInput":<input>}` oder
  `{"behavior":"deny","message":"…"}`. Ablehnung erscheint als `tool_result` mit `is_error: true` und in
  `result.permission_denials`.
- **Wird auch für Septets eigene MCP-Werkzeuge aufgerufen** (Modus `default`). Daher in Phase 1:
  `--allowedTools mcp__septet` (alle Septet-Werkzeuge frei) und `--permission-mode acceptEdits` (Bearbeiten
  im Workspace frei, außerhalb → `approve`). `Bash` bleibt immer bei `approve`.
- Der `tools/call` für `approve` blockiert, bis Septet antwortet → der HTTP-Thread wartet auf den UI-Thread.
  **MCP-Werkzeuge haben ein Zeitlimit von ~60 s** („The operation timed out", Claude versucht es dann erneut).
  Mit `MCP_TOOL_TIMEOUT=3600000` in der Umgebung des Kindprozesses wartete `approve` 95 s problemlos. Das gilt
  auch für lange App-Renders; Septet setzt es immer.
- Geprüft mit `--allowedTools mcp__septet --permission-mode acceptEdits` (Spike 2/3): Septet-Werkzeuge und
  Write im Workspace laufen ohne `approve`; `Read /etc/hostname`, `cat /etc/hostname`, `ls ..` und Write nach
  `/tmp` gehen an `approve`. **Ausnahme**: Claude Code stuft pfadlose, rein lesende Befehle wie `echo hi` als
  sicher ein und führt sie ohne `approve` aus. Das ist für „Bash nur mit Freigabe" hinnehmbar (nichts außerhalb
  des Workspace wird berührt); wer es strenger will, bräuchte einen PreToolUse-Hook über `--settings`.
- `--disable-slash-commands` leert `init.skills` und `init.slash_commands` wie gewünscht.

## Offen

- Verhalten bei erreichtem Limit (nicht provozierbar; erwartet `result.is_error` + `rate_limit_event.status != "allowed"`).
- Windows (`claude.exe`/`claude.cmd`-Auflösung, Kindprozess beenden) und macOS.
