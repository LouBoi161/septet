---
name: vectorcraft
description: Drive Vectorcraft directly with the app_* tools - make a document, draw shapes and live text, select by id, paint, align, group, boolean ops, layers, artboards, export into the workspace - with verified command ids and parameter formats, plus the views and render targets that work in Septet. Load before calling app_commands/app_execute/app_inspect/app_render with app "vectorcraft", or when deciding between writing an SVG and editing the open Vectorcraft document.
---

# Driving Vectorcraft

Vectorcraft has ~940 commands (engine plus UI). In Septet, `app_execute` runs them as their menu items do: the user
sees the change, and each command is one undo step. This file lists the commands you will need most often, checked
against the registry (`crates/engine/src/cmd/*`). For anything else, call `app_commands {app: "vectorcraft", filter: "…"}`:
its `params` text is the parameter format.

## SVG file or commands?

- **Write an SVG** (`septet:svg-graphics`) for new illustrations with many paths, curves, gradients or patterns.
  Then `septet_open` it in Vectorcraft, or `septet_place` it into the open document, where it lands as one group.
  This is faster and easier to review than hundreds of commands.
- **Drive commands** to edit the user's open document, for exact geometric work (align, distribute, Pathfinder,
  offset, outline stroke), for live shapes and live text the user will keep editing, and for exports.
- Mixing them is normal: place an SVG, then align it, recolour it or export it with commands.

## Start

- No document open: `file.new {width?, height?, units?, name?, artboards?, colorMode?}`. The defaults are 612 × 792
  pt. The size is in pt, or a string such as `"210 mm"`. `units` is the ruler unit (`"Pixels"`, `"Millimeters"`…).
  This command also starts the app. Do **not** use `file.newDialog`, which opens a dialog.
- Existing file: `septet_open {paths, app: "vectorcraft"}`. `app_inspect {what: "documents"}` lists the open
  documents. `document.activate {index}` switches between them.
- Geometry: every value is in **points**, whatever the ruler unit. **y points down.** The first artboard of a new
  document starts at (0, 0) (`app_inspect document` → `artboards[{x, y, width, height}]`).

## Ids and the selection

- Every object, group and layer has a numeric id. Ids stay the same across edits and undo.
- Creating commands return `{id}` and **select the new object**. Commands without `ids` act on the selection.
- To find ids:
  - `app_inspect {what: "document", depth?}` gives the layer tree (id, name, kind, bounds `{x,y,width,height}`,
    fill, stroke), plus `selection`, `currentLayer` and `artboards`.
  - `app_inspect {what: "find", params: {name?, kind?, text?}}`: `name` and `text` are substrings. `kind` is exact:
    `Path`, `Compound Path`, `Group`, `Clip Group`, `Type`, `Image`, `Symbol`, `Layer`. It returns
    `{matches: [{id, name, kind, path}], total}`.
  - `app_inspect {what: "object", id}` shows one object. `{what: "selection"}` shows what is selected and its bounds.
- To select: `select.set {ids: [..]}` replaces the selection. `select.add {ids}`, `select.toggle {id}|{ids}`,
  `select.none {}`, `select.all {}` and `select.allOnArtboard {artboard?}` also work. `select.key {id}` sets the key
  object for `object.align {to: "key"}`.
- Many paint, transparency, text and layer commands take `ids` (or `id`) directly, so you don't have to select first.

## The work loop

1. `app_commands {filter: "rectangle"}` when you are unsure of an id or its params (`enabled_only: true` shows only
   the commands that can run now).
2. `app_execute {app: "vectorcraft", command, params}`. Read the reply: `{ok, result}` or `{ok: false, error}`.
3. `app_render {app: "vectorcraft", target: "document"}` shows the whole artboard. `target: "object", id` shows one
   object cut out alone, and `target: "selection"` shows the selection. Look at it critically.
4. Fix it with another command, or take it back with `app_undo {steps?}`.
5. `command.batch {label?, commands: [{command, params}, …]}` runs several **engine** commands as ONE undo step and
   rolls them all back if one fails. Use it for multi-step edits the user should be able to undo at once.

## Commands (verified ids and params; `?` = optional)

| Task | Command | Params |
|---|---|---|
| Shapes | `shape.rectangle` | `{x, y, width, height, radius?}` → `{id}` |
| | `shape.ellipse` | `{x, y, width, height}` → `{id}` |
| | `shape.polygon` | `{cx, cy, radius, sides=6, rotation?: deg}` |
| | `shape.star` | `{cx, cy, radius1, radius2, points=5, rotation?}` |
| | `shape.line` | `{x1, y1, x2, y2}` |
| | `path.create` | `{anchors: [{x, y, in?: [x,y], out?: [x,y]}], closed?}` or `{d: "SVG path data"}` |
| Select | `select.set` / `select.add` / `select.none` / `select.all` | see above |
| Fill/stroke | `paint.setFill` | `{color?: "#rrggbb", none?: true, swatch?, gradient?: {kind?: linear\|radial, stops?: [{offset, color, opacity?}], angle?}, ids?}` |
| | `paint.setStroke` | same as `paint.setFill`, for the stroke |
| | `stroke.set` | `{weight?, cap?: butt\|round\|square, join?: miter\|round\|bevel, align?: center\|inside\|outside, dash?: [d,g…]\|null, startArrow?, endArrow?, ids?}` |
| | `transparency.set` | `{ids?, opacity?: 0..100, blend?: "Multiply"…}` |
| Name/hide/lock | `object.setProps` | `{ids?\|id?, name?, visible?, locked?, opacity?, blend?}` |
| Align | `object.align` | `{horizontal?: left\|center\|right, vertical?: top\|center\|bottom, to?: selection\|artboard\|key}` (acts on the selection) |
| | `object.distribute` | `{horizontal?: left\|center\|right, vertical?: top\|center\|bottom}` |
| | `object.distributeSpacing` | `{axis: horizontal\|vertical, spacing?: pt}` |
| Transform | `object.move` | `{dx, dy, copy?}` |
| | `object.rotate` | `{angle: deg counter-clockwise, absolute?, origin?: [x,y], copy?}` |
| | `object.scale` | `{sx: %, sy?: %, origin?, copy?, strokes?}` |
| | `object.reflect` | `{axis: "vertical"\|"horizontal"\|deg, origin?, copy?}` |
| | `object.setBounds` | `{x?, y?, width?, height?, reference?: 0..8, proportional?}` (Transform panel) |
| | `object.transform` | `{matrix: [a,b,c,d,e,f], copy?, ids?}` |
| | `edit.duplicate` | `{dx?, dy?}` · `edit.clear {ids?}` deletes |
| Arrange | `object.arrange.bringToFront` / `.bringForward` / `.sendBackward` / `.sendToBack` | `{}` |
| Group | `object.group` → `{id}` · `object.ungroup` · `object.compoundPath.make` · `object.clippingMask.make` (the top object clips) | `{}` |
| Pathfinder | `object.pathfinder.unite` / `.minusFront` / `.intersect` / `.exclude` / `.divide` / `.trim` / `.merge` / `.crop` / `.minusBack` | `{}` on the selection → `{ids}` |
| Paths | `object.path.outlineStroke` `{}` · `object.path.offsetPath` `{offset: pt, joins?}` · `object.path.simplify` `{tolerance?}` | |
| Text | `text.create` | `{x, y (baseline), text, size?: pt, font?: family, style?, color?: "#rrggbb", area?: {width, height}}` → `{id}` |
| | `text.setText` | `{id?\|ids?, text}` |
| | `text.setStyle` | `{ids?\|id?, font?, style?, size?, leading?: pt\|"auto", tracking?: 1/1000 em, justify?: left\|center\|right\|justifyAll, fill?}` |
| | `text.fontList` | `{family?}` → the available families, or one family's styles |
| | `type.createOutlines` | `{}` turns the selected text into outlines |
| Effects | `effect.apply` | `{effect: "stylize.dropShadow"\|"blur.gaussian"\|…, params?, ids?}` (`effect.list` gives the catalog) |
| Layers | `layer.new` | `{name?, top?, visible?, locked?}` → `{id}` (new art goes into the current layer) |
| | `layer.setCurrent` `{id}` · `layer.setProps` `{ids?\|id?, name?, visible?, locked?}` · `layer.delete` `{ids?\|id?}` | |
| | `layer.move` | `{ids, target, place?: above\|below\|inside}` |
| Artboards | `artboard.new` `{x?, y?, width?, height?, name?}` · `artboard.setProps` `{index, name?, x?, y?, width?, height?}` · `artboard.fitToArt` `{index?}` | index 0-based |
| Place file | `file.place` | `{path, link?: true, at?: [x,y] centre, rect?: [x,y,w,h]}` → `{ids}` |
| Export | `document.export` | `{path, format?: svg\|pdf\|png\|jpg\|webp\|tiff\|eps\|…, artboard?: 0, ppi?: 72, scale?, background?: transparent\|white\|"#rrggbb", selectedOnly?}` |
| | `document.exportSelection` | `{path, format?: png\|jpg\|webp\|svg\|pdf, scale?}` (cropped to the selection) |
| | `file.saveCopy` | `{path, format?: vectorcraft\|svg\|pdf\|ai}` writes a copy; the document keeps its own path |

Examples:

```json
{"app": "vectorcraft", "command": "file.new", "params": {"width": 800, "height": 600, "units": "Pixels", "name": "Badge"}}
{"app": "vectorcraft", "command": "command.batch", "params": {"label": "Badge base", "commands": [
  {"command": "shape.ellipse", "params": {"x": 250, "y": 150, "width": 300, "height": 300}},
  {"command": "paint.setFill", "params": {"color": "#1d3557"}},
  {"command": "paint.setStroke", "params": {"none": true}},
  {"command": "object.align", "params": {"horizontal": "center", "vertical": "center", "to": "artboard"}}]}}
{"app": "vectorcraft", "command": "select.set", "params": {"ids": [12, 15]}}
{"app": "vectorcraft", "command": "object.pathfinder.minusFront", "params": {}}
{"app": "vectorcraft", "command": "document.export", "params": {"path": "out/badge.png", "artboard": 0, "ppi": 144}}
```

The batch works because each new shape becomes the selection, so the paint and align steps act on it.

## Views and renders that work

- `app_inspect` `what`: `document` (`depth` default 3, `params.limit` children per level), `object`, `selection`,
  `find`, `history` (undo/redo labels), `documents`. There are no others.
- `app_render` `target`: `document` (alias `artboard`; `page` is the **1-based** artboard number, default the one in
  view), `object`/`layer` (`id`; shown alone, cropped, over a checkerboard), `selection`. `max_side` is at most 1568.
  `save_as` writes a transparent PNG into the workspace.
- Renders are made off-screen, so they work while the tab is hidden and don't depend on the user's zoom.

## Pitfalls in Septet

- A command that opens a dialog (it was missing params, e.g. `object.move {}`, `file.print`, `file.newDialog`)
  has its dialog closed and fails. Run it again with the params it lists.
- Paths are relative to the workspace. Exports and saves inside the workspace run at once. Paths outside it, saving
  without a `path` and `prefs.*` all ask the user. Don't call `document.export` without `path`, which returns
  base64. Prefer `file.saveCopy`/`document.export` to `file.saveAs`, which moves the user's document to a new path.
- `paint.setFill`/`paint.setStroke` also set the default for the next object you draw. With nothing selected and no
  `ids`, they change only that default. New text ignores it: give `color` to `text.create`.
- **Fonts**: Vectorcraft sees only installed fonts (and its bundled ones), not TTFs in the workspace. Check with
  `text.fontList {family: "Inter"}`. A missing family falls back silently. When it must look right, outline it
  (`type.createOutlines`) or ask the user to install the font.
- **SVG filters**: placed or opened SVGs keep only blur, drop shadow, inner/outer glow and feather. Other filters are
  dropped (see `septet:svg-graphics`). Use `effect.apply` for live effects instead.
- Placed images are *linked* by default (`link: true`). Don't move or delete the file afterwards.
- A document made by `file.new` may not be fitted in the window (seen once: zoomed in, left side cut off). This
  doesn't affect `app_render`. If the user's view looks wrong, run `view.fitArtboard {}`.
- If a command refuses an object, check whether it or its layer is locked or hidden (`app_inspect object`), and
  unlock it with `object.setProps {id, locked: false}` or `layer.setProps`. `app_render` leaves template layers out.
- Never quit or close documents. `app.quit`/`ui.*` are blocked anyway, and the user saves their own work.
