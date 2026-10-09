---
name: filmcraft
description: Drive Filmcraft (Septet's Premiere-like video editor) with the app_* tools - its ~675 engine commands (new sequence, import, placing clips on the timeline, razor/trim/move/ripple delete, transitions, text titles, captions, audio gain, effects and Lumetri colour, markers, exports as background jobs), the ids of bin items, sequences, tracks and clips, the tick/seconds/frame/timecode time formats, and the execute-render-undo loop. Use when you cut, assemble or change an edit inside Filmcraft rather than writing a timeline file.
---

# Driving Filmcraft

Load `septet:septet-apps` first for the general rules. `septet:video-editing` covers writing OTIO/FCPXML/SRT
files, ffmpeg and pacing. This skill is about editing **inside Filmcraft** with `app_execute`.

Every menu item, shortcut and timeline gesture is an engine command, run through the menus' dispatcher (undoable,
answered at once). `app_commands {app: "filmcraft", filter: "transition"}` lists ids, each with its `params`
string (`?` = optional, `=x` = default) and `disabled_reason` when it can't run now. Never guess an id or a param:
what isn't in the params string is ignored.

## 1. Start

- Open a project: `septet_open {paths: ["edit.fcproj"]}`. There is always a project (an untitled one at least).
- Media: `septet_open` / `septet_place` of mp4, mov, wav, png … **only adds items to the bin**. So does
  `file.import {paths: ["media/a.mp4", "media/b.mp4"], bin?}` → `{"items": [4, 5], "errors": [...]}`. Nothing
  goes onto the timeline until you run `timeline.place`.
- New sequence: `file.newSequence {name, width=1920, height=1080, fps=23.976, sampleRate=48000, video=3,
  audio=3}` → `{"sequence": id}`. It opens and becomes the active sequence. **Always pass `fps`** (the default is
  23.976) to match the footage. Change it later with `sequence.settings {width?, height?, fps?, name?}`.
  `file.newSequenceFromClip {items: [id]}` makes one that matches a clip and holds it.
- Other sequences: `sequence.open {item: id}` makes one active. Commands, `inspect sequence` and `render frame`
  work on the **active** sequence.
- Generated items for the bin: `file.newColorMatte {color: "#rrggbb", seconds}`, `file.newBlackVideo {seconds}`.

## 2. Ids and times

- `app_inspect {app: "filmcraft", what: "document"}`: bins and items (`id`, `name`, `type`, `duration`) and
  `activeSequence`. Project item ids are the `item` params; a sequence is an item too.
- `what: "sequence"` (optional `id`): `settings`, `video` / `audio` track lists (V1 first, each with `id`,
  `name` and `items`), `markers`, `selection`, `playhead`, and `ticksPerSecond`. Each timeline clip has `clip` (its
  id), `item` (its bin item), `start`, `end`, `duration`, `sourceIn`, `sourceOut` (ticks), `startFrame`,
  `endFrame`, `link` (picture and sound of one placement share it), `effects`, plus `transitions` per track.
- `what: "clip", id`: one clip. `what: "selection"`: editor state (selection, playhead, In/Out). `what: "history"`.
- **Tracks**: `"V1"`, `"V2"`, `"A1"` (1-based, V1 is the bottom track) or a track id. An unknown track is an
  error, never another track. Exception: `graphics.newText` / `graphics.newFromFile` take `track` as a **0-based
  index** (0 = V1).
- **Time**: 254 016 000 000 ticks per second. Where a command's params say `"time":ticks` and the command reads
  times the usual way (`playhead.set`, `timeline.place`, `timeline.razor`, `markers.add`, `captions.add`,
  `sequence.addEdit`, `sequence.closeGap`, `graphics.newText`), give **one** of: `time` (integer ticks), `frame`
  (integer, at the sequence's rate), `seconds` (float) or `timecode` (`"00:00:10:00"`, separators `:;.,`; bare
  digits read as HHMMSSFF; `"+15"` / `"-1.00"` = relative to the playhead). The first present wins in that order.
- **Ticks only**: `timeline.move` `moves[].time`, `timeline.place` `sourceIn` / `duration`, `markers.addRange`
  `duration`, `effects.*Keyframe` `mediaTime`. Deltas: `delta` (ticks) or `deltaFrames`. Compute ticks as
  `round(seconds * 254016000000)`; `time` must be an integer (a float `time` is ignored → the playhead is used).
- Commands that work on the selection take `clips: [id]` (or `clip`) instead and leave the selection alone;
  project-panel commands take `items: [id]`.

## 3. Work loop

1. `app_commands {app: "filmcraft", filter: "…"}` → exact id and params.
2. `app_inspect what: "sequence"` → clip and track ids.
3. `app_execute` one command; keep the ids it returns (`{"clips": [..]}`, `{"clip": id}`, `{"sequence": id}`).
4. `app_render {app: "filmcraft", target: "frame", time: t}` at a few times: just after each cut, mid-transition,
   on titles. `target: "clip", id` shows one clip alone (at `time`, else the playhead if inside it, else its
   middle); `target: "item", id` a bin item; `selection` the first selected clip. `max_side: 640` for checks.
   `time` here is seconds. Frames are drawn on black like the Program monitor, with captions.
5. Wrong? `app_undo {app: "filmcraft"}` (`steps`), then inspect again.

## 4. Commands

| Task | Command and params |
|---|---|
| Place a bin item | `timeline.place {item, track?:"V1"\|"A1"\|id, audioTrack?:"A1", time\|frame\|seconds\|timecode (else playhead), insert:bool=false, sourceIn?:ticks, duration?:ticks}` → `{"clips": [video, audio]}`. Picture and linked sound; an audio track as `track` = sound only |
| Source In/Out of an item | `project.setMarks {item, in?:ticks\|null, out?:ticks\|null}` (used by later `timeline.place`) |
| Move clips | `timeline.move {moves: [{clip, track:"V1"\|id, time:ticks}], insert:bool, linked?:bool}` (linked sound follows) |
| Razor / add edit | `timeline.razor {time\|seconds…, track?, clip?}` (no track = all unlocked tracks); `sequence.addEdit {time?}` at targeted tracks |
| Trim an edge | `timeline.trim {clip, edge:"in\|out", mode:"regular\|ripple", delta:ticks\|deltaFrames}` |
| Roll / slip / slide | `timeline.roll {left, right, delta\|deltaFrames}`; `timeline.slip {clip, delta\|deltaFrames}`; `timeline.slide {clip, delta\|deltaFrames}` |
| Delete | `edit.rippleDelete {clips?}` (closes the gap); `edit.clear {clips?}` (leaves it); `sequence.closeGap {track, time…}` |
| Select | `timeline.select {clips: [id], add:bool, toggle:bool}`; `edit.deselectAll {}` |
| Speed | `clip.speedDuration {clips?, speed:percent=100, reverse:bool, ripple:bool, interpolation?:"frameSampling\|frameBlending\|opticalFlow"}`; `clip.frameHold {clips?, time?}` |
| Video transition | `sequence.applyVideoTransition {clip?, edge?:"in"\|"out", effect?:"cross_dissolve"\|"dip_to_black"\|"dip_to_white"\|"film_dissolve"\|…, frames?, params?, reverse?}`; edit later with `sequence.setTransition {transition, params?, reverse?, reset?}` |
| Audio transition | `sequence.applyAudioTransition {clip?, edge?, effect?:"constant_power"\|"constant_gain"\|"exponential_fade", frames?}` |
| List effects | `effects.list {kind?:"Video"\|"Audio"\|"VideoTransition"\|"AudioTransition", folder?, detail?:bool}` (`detail` adds param info) |
| Title text | `graphics.newText {text, time\|frame (start), seconds=5 (duration!), size=100, position?:[x,y], box?:[w,h], font?, fontStyle?, track?:index, clip?, newClip?}` → `{"clip", "layer"}`; it lands above the clips there |
| Edit title | `graphics.setText {clip?, layer?, text}`; shapes `graphics.newRectangle {position?, size?:[w,h], clip?}`; image/video above: `graphics.newFromFile {path, time?, track?}` |
| Captions | `captions.add {text?, time\|seconds (start), durationSeconds=3, track?:"C1"}` (makes a caption track if none); `captions.setText {caption?, text?}`; `captions.setStyle {track:"C1", font?, size?, color?, align?, anchor?, …}`; `captions.import {path}`; `captions.list {}` |
| Audio level | `clip.audioGain {clips?, mode?:"set\|adjust\|normalizeMax\|normalizeAll", db}` (no `mode`: adjust); track: `timeline.setTrack {track:"A1", volumeDb?, pan?, muted?, solo?, locked?, enabled?, name?}` |
| Loudness / ducking | `essentialSound.setType {clips?, type:"dialogue\|music\|sfx\|ambience"}`, `essentialSound.autoMatch {clips?, target?}`, `essentialSound.generateDucking {clips?}` |
| Apply effect | `effects.apply {clips?, effect:"gaussian_blur"\|"Gaussian Blur"\|"lumetri"\|…}` |
| Set a parameter | `effects.setParam {clip, effect:"motion"\|"opacity"\|"lumetri"\|index, param, value, time?}`; motion: `position` [x,y], `scale` (%), `rotation`, `anchor`; opacity: `opacity` (%); lumetri: `exposure`, `contrast`, `temperature`, `tint`, `saturation`, `highlights`, `shadows`, `whites`, `blacks`, `vibrance`, `look` (choice index 0-8, 0 = None) |
| Keyframes | `effects.addKeyframe {clip, effect, param, time?}`; `effects.toggleAnimation {clip, effect, param}`; `effects.setKeyframe {clip, effect, param, mediaTime, value?}` |
| Frame fit | `clip.fitToFrame {clips?}`, `clip.fillFrame {clips?}` |
| Colour presets | `lumetri.presets {folder?}`, `lumetri.applyPreset {name, clips?}` |
| Markers | `markers.add {time\|seconds…, name?, comment?, color?:"Green"\|"Red"…, durationFrames?}`; `markers.edit {marker, …}`; In/Out: `markers.markIn {time?}`, `markers.markOut {time?}` |
| Playhead | `playhead.set {time\|frame\|seconds\|timecode}` |
| Save | `file.saveAs {path: "edit.fcproj"}`; `file.save {path?}` |
| Export movie | `file.exportMedia {path, format?:"h264\|hevc\|prores\|dnxhr\|gif\|png\|wav\|…", preset?, width?, height?, fps?, quality?, range?:"entire\|inOut\|workArea\|custom", startSeconds?, endSeconds?, burnCaptions?, captionSidecar?:"srt\|vtt", proresProfile?}` → `{job, path}`; presets: `export.presets.list {query?}` |
| Still / interchange | `file.exportFrame {path?, format?:"png\|tiff\|bmp"}` (at the playhead); `file.exportOtio`, `file.exportFcpxml`, `file.exportEdl` (see `app_commands`) |
| Jobs | `jobs.list {}` (`progress`, `etaSeconds`, `finished`, `result`); `jobs.cancel {job}` |

## 5. Worked examples

Two clips back to back on V1/A1 in a new 25 fps sequence (bin items 4 and 5 from `file.import`):
```json
{"command": "file.newSequence", "params": {"name": "Cut 1", "fps": 25}}
{"command": "timeline.place", "params": {"item": 4, "track": "V1", "seconds": 0, "insert": false}}
```
→ `{"clips": [12, 13]}`. `app_inspect what: "sequence"` → clip 12 `endFrame: 250`. Then the second clip,
starting 2 s into its media and 4 s long (ticks):
```json
{"command": "timeline.place", "params": {"item": 5, "track": "V1", "frame": 250, "insert": false,
 "sourceIn": 508032000000, "duration": 1016064000000}}
```
A 1 s cross dissolve centred on the cut between them (the out edge of clip 12, the next clip touching it):
```json
{"command": "sequence.applyVideoTransition", "params": {"clip": 12, "edge": "out", "effect": "cross_dissolve", "frames": 25}}
```
Then `app_render target: "frame"` at 9.5, 10 and 10.5 s. A title over the first 4 s, low in the frame:
```json
{"command": "graphics.newText", "params": {"text": "Summer 2026", "frame": 0, "seconds": 4, "size": 120, "position": [960, 900]}}
```
Export into the workspace as a background job, then poll:
```json
{"command": "file.exportMedia", "params": {"path": "renders/cut1.mp4", "format": "h264"}}
{"command": "jobs.list"}
```

## 6. Pitfalls

- **Dialogs and file pickers don't happen.** A command that would open a dialog (Audio Gain, Speed/Duration,
  New Sequence… without params) is closed again and fails: pass its params. One that would open a file picker fails
  with "pass the path": give `path` / `paths`. Paths are relative to the workspace.
- **Exports are jobs.** `file.exportMedia` returns `{job, path}` at once (`export.queue.add` queues into the
  Export queue, `start: true` runs it); poll `jobs.list`
  until `finished` (check `result` for an `error`), then tell the user the path. Don't pass `wait: true`: it
  blocks the app until the encode ends. Exporting or saving outside the workspace asks the user; `file.save` with
  no `path` asks too.
- **Extension follows the format.** The default format is H.264 (`.mp4`); `"path": "out.mov"` without `format:
  "prores"` writes `out.mov.mp4`. Match `format` and extension, or give a folder (`"renders/"`) and get the
  sequence's name.
- **`seconds` means different things.** In `graphics.newText` / `graphics.newShape` it is the **duration**, and it
  also counts as the start time when no `time` / `frame` is given: always pass `frame` or `time` for the start. In
  `captions.add`, `seconds` is the start and `durationSeconds` the length. `file.newColorMatte {seconds}` is the
  length.
- **Linked clips.** `timeline.place` puts picture and sound as two linked clips; razor, trim, ripple delete and move
  act on both (`timeline.move linked: false` moves one). A still image gets the default still duration.
- **Transitions need an edit point.** Without `clip`, `applyVideoTransition` looks for a cut within 3 frames of the
  playhead on targeted tracks ("no edit point at the playhead…"): pass `clip` and `edge`. Applying one where one is
  replaces it.
- **Placing over clips overwrites** them (`insert: false`); `insert: true` pushes later clips right. Read
  `start`/`end` before choosing a time so clips don't overlap by accident.
- Don't drive playback (`playback.*`) or dynamic trims (`trim.shuttle`): they run on the UI clock. Use
  `playhead.set` and renders. Settings (`prefs.*`) ask the user.
