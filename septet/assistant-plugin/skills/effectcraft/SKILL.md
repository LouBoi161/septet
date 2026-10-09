---
name: effectcraft
description: Drive Effectcraft (Septet's After Effects-like motion graphics app) with the app_* tools - its 665 engine commands with JSON schemas (compositions, text/shape/solid/null/footage layers, transform keyframes and easing, effects, masks, parenting, precomps, markers, render queue, save), how to address comps, layers and property paths, and the inspect-execute-render loop over several times. Use when you build or change an animation, title, lower third or composite inside Effectcraft.
---

# Driving Effectcraft

Load `septet:septet-apps` first for the general rules. `septet:motion-lottie` covers writing Lottie JSON and
which files Effectcraft takes. This skill is about building and changing projects **inside Effectcraft** with
`app_execute`.

Every menu item, shortcut and panel gesture in Effectcraft is an engine command with JSON params. `app_commands
{app: "effectcraft", filter: "keyframe"}` lists them; with **12 results or fewer** each one carries its JSON schema,
so filter narrowly (`"prop.addKey"`, `"layer new"`). Or read one in full: `app_execute {command: "command.describe",
params: {command: "keys.setEase"}}`. **Unknown params are rejected** with the accepted list; read it and retry.

## 1. Start

- Open the user's project: `septet_open {paths: ["film.ecproj"]}` (`.ecproj` / `.ecprojx`). Images, SVG, PSD,
  PDF/AI and video are imported into the open project; with a comp open, `septet_place` adds them as layers.
- Or make a comp: `comp.new {name, width, height, frameRate, duration, background}` → `{"comp": id}`. It opens and
  becomes the active comp. Always pass `frameRate` (the default is 29.97) and `duration` in seconds (default 10).
- Footage by command: `file.import {paths: ["clip.mp4"], addToComp: true}`, or import and then
  `layer.addItem {item: id|name, time?, index?, position?}`. A Lottie you wrote: `file.importLottie {path: "anim.json"}`
  (new comp, opened).
- Orient yourself: `app_inspect {what: "document"}` (items, `activeComp`, current time), then `what: "comp"`.

## 2. Ids, paths and units

- **Comps**: an id or name (`comp` param, known to every command); left out = the active comp. `comp.open {comp}`
  switches. `app_render` draws the **active** comp, so open the one you want first.
- **Layers**: the id a create command returns (`{"layer": 7}`), `"#n"` (1 = top of the stack) or the name. Name
  layers when you create them (`name` param) and use the names. `layer` and `layers` are interchangeable.
- **Properties** by path: `transform/position`, `transform/scale`, `transform/rotation`, `transform/opacity`,
  `transform/anchor`, `text/sourceText`, `effects/#1/blurriness`, or `@uid`. Get the real paths from
  `app_inspect {what: "layer", id: "Title", depth: 3}` (depth 2 is the default and stops above effect params).
  One property: `app_inspect {what: "property", params: {layer: "Title", path: "transform/opacity", time: 1}}`.
- **Units**: times in seconds (keys snap to the comp's frames; key times are layer time, equal to comp time unless
  the layer is moved or stretched). Position in comp pixels `[x, y]`, y down, origin top-left. Scale `[100, 100]`
  in percent, rotation in degrees, opacity 0-100. Colours `"#rrggbb"` or `[r, g, b]` in 0-1.

## 3. Work loop

1. `app_commands {app: "effectcraft", filter: "…"}` → exact id and schema. Never guess an id or a param.
2. `app_execute` one command; keep the ids and `paths` it returns.
3. `app_render {app: "effectcraft", target: "frame", time: t}` at **several times** (start, mid-move, end, e.g. 0,
   0.5, 1, 3), plus `target: "layer", id: "Title"` for one layer alone on transparency. Use `max_side: 640` for checks.
4. Wrong? `app_undo {app: "effectcraft"}` (`steps` for several) or fix the value; check again with `app_inspect
   property` (it shows `keys`, `expression`, and `evaluated` / `expressionError`).

## 4. Commands

| Task | Command and params (`?` = optional) |
|---|---|
| New comp | `comp.new {name?, width?, height?, frameRate?, duration?, background?, pixelAspect?, open?}` |
| Comp settings | `comp.settings {comp?, name?, width?, height?, frameRate?, duration?, background?}`; `comp.workArea {start?, end?}` |
| Switch / read comp | `comp.open {comp}`; `comp.info {comp?}` |
| Text layer | `layer.newText {text?, name?, position?, box? [x,y,w,h], font?, style?, size?, fill?, stroke?, strokeWidth?, tracking?, leading?, justify?}` |
| Change text | `layer.setText {layer?, range?, text?, font?, size?, fill?, …}`; fonts: `text.fonts {query?}` |
| Text animation | `layer.applyTextPreset {layer?, preset: typewriter\|fadeUpCharacters\|bounceInWords\|trackingIn\|scramble\|blurIn\|jitter\|dropInLines}` |
| Shape layer | `layer.newShape {kind?: rect\|rounded\|ellipse\|star\|polygon\|none, name?, size? [w,h], fill?, stroke?, strokeWidth?, position?}` |
| Shape contents | `layer.addShapeItem {layer?, kind: group\|rect\|ellipse\|path\|fill\|stroke\|trim\|repeater\|…, group?}` → `{uid, path}` |
| Solid / adjustment | `layer.newSolid {name?, color?, width?, height?}`; `layer.newAdjustment {name?}` |
| Null, camera, light | `layer.newNull {name?}`; `layer.newCamera {name?, preset?, …}`; `layer.newLight {kind?, name?, …}` |
| Footage layer | `file.import {paths, addToComp?}`; `layer.addItem {item, time?, index?, position?}` |
| Set a value | `prop.set {layer?, path, value, time?}` (on an animated property `time` sets that key) |
| Keyframe | `prop.addKey {layer?, path, time?, value?}`; move/change: `keys.set {layer?, path, time, newTime?, value?}` |
| Expression | `prop.setExpression {layer?, path, expression?, enabled?}` |
| Transform shortcut | `layer.setTransform {layers?, prop: anchor\|position\|scale\|orientation\|rotation\|opacity, value}` |
| Select keys | `prop.select {layer?, path}` (selects all its keys); `keys.select {keys: [{layer, path, time}]}` |
| Easing (selected keys) | `keys.easyEase {which?: both\|in\|out}`; `keys.interpolation {interpolation?: linear\|bezier\|hold\|…}`; `keys.velocity {inSpeed?, inInfluence?, outSpeed?, outInfluence?}` |
| Easing (one key) | `keys.setEase {layer?, path, time, side: in\|out, speed?, influence?}` |
| Effects | `effect.list {filter?}` (ids, names, param ids); `effect.apply {effect: id\|name, layers?}` → `{paths: ["effects/#1"]}`; `effect.remove {layer?, effect}`; `effect.toggle {layer?, effect, value?}` |
| Layer styles | `layer.style.add {style: dropShadow\|outerGlow\|stroke\|…, layers?}` |
| Masks | `layer.addMask {layer?, shape?: rect\|ellipse\|rounded\|polygon\|star, rect? [x,y,w,h], mode?}`; `layer.mask.set {layer?, mask?, field: feather\|opacity\|expansion, value}`; `layer.mask.mode {layer?, mask?, mode}` |
| Track matte | `layer.setTrackMatte {layer?, matte: layer\|null, kind?: alpha\|alphaInverted\|luma\|lumaInverted}` |
| Parenting | `layer.setParent {layers?, parent: layer\|null}` (keeps the child where it is) |
| Precompose | `layer.precompose {layers?, name?, mode?: move\|leave, adjustDuration?, open?}` → `{comp, layer}` |
| Order, align | `layer.arrange {layers?, to: front\|forward\|backward\|back}`; `layer.align {edge, to?, layers?}`; `layer.centerAnchor {layers?}` |
| Timing | `layer.timing {layers?, op?, start?, in?, out?}`; `layer.sequence {layers?, overlap?, duration?}`; `layer.rename {layer?, name}` |
| Blend mode | `layer.setBlendMode {layers?, mode?: Normal\|Multiply\|Screen\|…}` |
| Markers | `comp.addMarker {time?, comment?}`; `layer.addMarker {layers?, time?, comment?}`; `markers.list {layer?}` |
| Current time | `time.set {time? \| frame?}` |
| Render a movie | `renderQueue.add {comp?, format?: h264\|hevc\|av1\|prores\|webm\|gif\|png…, output?, channels?, timeSpan?}`, then `renderQueue.render {}` |
| Still / Lottie | `comp.saveFrameAs {path (.png), time?}`; `file.exportLottie {comp?, path}` |
| Save project | `file.saveAs {path: "name.ecproj"}`; `file.save {path?}` |

### Example: a title that fades in

```json
{"command": "comp.new", "params": {"name": "Title", "width": 1920, "height": 1080, "frameRate": 30, "duration": 4, "background": "#101418"}}
{"command": "layer.newText", "params": {"text": "Hello", "name": "Title", "size": 140, "fill": "#ffffff", "position": [960, 540]}}
{"command": "prop.addKey", "params": {"layer": "Title", "path": "transform/opacity", "time": 0, "value": 0}}
{"command": "prop.addKey", "params": {"layer": "Title", "path": "transform/opacity", "time": 1, "value": 100}}
{"command": "prop.select", "params": {"layer": "Title", "path": "transform/opacity"}}
{"command": "keys.easyEase", "params": {}}
```

Then `app_render` frames at `time` 0, 0.5 and 1.5.

### Example: slide in with a blur that clears

```json
{"command": "prop.addKey", "params": {"layer": "Title", "path": "transform/position", "time": 0, "value": [600, 540]}}
{"command": "prop.addKey", "params": {"layer": "Title", "path": "transform/position", "time": 0.8, "value": [960, 540]}}
{"command": "keys.setEase", "params": {"layer": "Title", "path": "transform/position", "time": 0.8, "side": "in", "speed": 0, "influence": 80}}
{"command": "effect.apply", "params": {"effect": "Gaussian Blur", "layers": ["Title"]}}
{"command": "prop.addKey", "params": {"layer": "Title", "path": "effects/#1/blurriness", "time": 0, "value": 30}}
{"command": "prop.addKey", "params": {"layer": "Title", "path": "effects/#1/blurriness", "time": 0.8, "value": 0}}
```

Use the path `effect.apply` returns (`effects/#2` when the layer already had an effect).

### Example: a dot on a rotating rig, precomposed

```json
{"command": "layer.newNull", "params": {"name": "Rig"}}
{"command": "layer.newShape", "params": {"kind": "ellipse", "name": "Dot", "size": [120, 120], "fill": "#ff5a36", "position": [1260, 540]}}
{"command": "layer.setParent", "params": {"layers": ["Dot"], "parent": "Rig"}}
{"command": "prop.addKey", "params": {"layer": "Rig", "path": "transform/rotation", "time": 0, "value": 0}}
{"command": "prop.addKey", "params": {"layer": "Rig", "path": "transform/rotation", "time": 4, "value": 360}}
{"command": "layer.precompose", "params": {"layers": ["Rig", "Dot"], "name": "Dot Rig", "mode": "move"}}
```

### Example: deliver into the workspace

```json
{"command": "file.saveAs", "params": {"path": "title.ecproj"}}
{"command": "comp.saveFrameAs", "params": {"path": "title-poster.png", "time": 2}}
{"command": "renderQueue.add", "params": {"comp": "Title", "format": "h264", "output": "title.mp4"}}
{"command": "renderQueue.render", "params": {}}
```

## 5. Pitfalls

- **Never use the solo switch** (`layer.setSwitch {switch: "solo"}`) to look at one layer: `app_render target:
  "layer"` already renders it alone without touching the project. Don't hide layers (`video` switch) for a check either.
- New layers go **above the selected layer** (or on top) and become the selection. A background solid made after
  the text covers it: `layer.arrange {layers: ["BG"], to: "back"}`.
- Commands that act on "the selected layers/keys" fail with "select a layer first" unless you pass `layer`/`layers`
  (or `keys`). Easing commands without key params (`keys.easyEase`, `keys.interpolation`, `keys.velocity`) need
  `prop.select` or `keys.select` first.
- `path` in `prop.*`/`keys.*` is a property path, never a file. File params (`path` of save/export/import, `paths`,
  `output`) are relative to the workspace; writing outside it, `file.save` without `path`, and `prefs.*` ask the user.
- `prop.set` without `time` on an animated property writes a key at the current time; set `time` explicitly.
- Names must be unique: a duplicate comp name gets a number. Use the returned ids when names could collide.
- Renders (`renderQueue.render`) and analysis run as background jobs; the call waits at most two minutes, so render
  short spans (`timeSpan`) or check `renderQueue.list` / `app_inspect` afterwards.
- Several steps as ONE undo step: `app_execute` `engine.batch {steps: [{command, params}, …]}`; a later step can use
  an earlier result as `"$N.key"` (e.g. `"$1.layer"`, steps count from 1). Septet checks each step like a command
  of its own. `file.runScript` runs a script and asks the user first: prefer commands or a batch.
- `ui.*` and quit commands are blocked in Septet. Window-only actions (RAM preview, panels) don't help you; look with
  `app_render`.
