---
name: lightcraft
description: Drive Lightcraft (raw photo library and develop) directly with the app_* tools - import photos, find and select them by id, rate/flag/label, set develop sliders (exposure, white balance, tone, HSL, grading, detail), crop and straighten, auto settings, presets and LUT profiles, masks, copy/paste/sync settings across photos, export into the workspace - with verified command ids, slider ids and parameter formats, plus the views and render targets that work in Septet. Load before calling app_commands/app_execute/app_inspect/app_render with app "lightcraft", or when the user wants photos developed, graded or culled.
---

# Driving Lightcraft

Lightcraft has 373 commands (239 engine + 134 UI). `app_execute` runs them as their menu items do: the user sees the
change, and each command is one undo step. Edits are **non-destructive**: they live in the library, never in the
original files. The ids and params below were checked against the registry (`lightcraft/crates/engine/src/cmd/*`,
slider ids in `crates/develop/src/controls.rs`). For anything else, use `app_commands {app: "lightcraft", filter: "…"}`.
Its `params` text gives the parameter format. UI commands (`view.*`, `panel.*`, `dialog.*`, `file.*`) take no params.
For pixel work (retouching, layers, text), export the photo and use Photocraft (`septet:image-editing`).

## Start: get photos into the library

- **Many files or a folder**: `septet_open {paths, app: "lightcraft"}`. This imports them in the background, in
  `add` mode (the files are referenced where they are, not copied), and shows the grid. Presets and `.cube` files
  are imported as presets/profiles. Then find the new photos:
  `app_inspect {what: "photos", params: {sort: {key: "importDate", ascending: false}, limit: 20}}`.
- **A few files, with options**: `library.import {paths: [..], mode?: add|copy, album?: albumId, albumName?, preset?:
  presetId, keywords?: [..]}`. It returns `{imported: [ids], duplicates, failed}`. It runs while you wait, so don't
  use it for big folders. **Never use `mode: "move"`**: it deletes the source files.
- Do not use `file.addPhotos`/`file.addFolder`. Without paths they would open a file picker, which Septet refuses.
  With paths they open the import review dialog for the user.
- Paths are relative to the workspace. Files outside the workspace need the user's approval.

## Ids, selection and the active photo

- Photos have numeric ids (`id` in `photos`, `fileName`, `rating`, `flag`, `label`, `edited`…). Masks have small
  numeric ids inside a photo (`develop` → `masks[].id`).
- `library.select {ids: [..], active?: id, mode?: replace|add|toggle|range}`. The **active** photo is `active`, or
  else the first id. **Most develop, crop and mask commands act on the active photo only.** Select the photo first.
- `photo.*`, `develop.reset`, `develop.paste`, `preset.apply`, `develop.quickAdjust` and `develop.set` take `ids`
  directly. Without `ids`, `photo.*` acts on the selection.
- Views (`app_inspect what`): `document` (view, filter, sort, selection, undo label, plus `stats`), `photos` (params
  `filter`, `sort`, `offset`, `limit` (default 50); without a filter: the current view), `photo` (`id`, else the
  active photo: metadata, albums, history, stack), `develop` (all settings, including `masks`), `controls` (every
  slider with `min`/`max`/`default`/`value` for the active photo; `params.section` such as `light`, `color`,
  `mixer`, `grading`, `effects`, `detail`, `optics`, `geometry`), `albums`, `history`.
- Filter (for `photos` and `library.filter`): `{text?, rating?, ratingOp?: atLeast|exactly|atMost, flag?:
  pick|reject|none, label?: red|yellow|green|blue|purple, kind?: image|raw|video, edited?, date?: "2026-04", keyword?,
  camera?, lens?, album?: albumId}`. `text` searches file names, titles, keywords and camera names.

## Develop sliders (`develop.set` control ids, range, default)

| Area | Control ids |
|---|---|
| White balance | `wb.temp` 2000..50000 K (higher = warmer), `wb.tint` -150..150 (+ = magenta) |
| Light | `light.exposure` -5..5 EV, `light.contrast`, `light.highlights`, `light.shadows`, `light.whites`, `light.blacks` (-100..100) |
| Presence | `color.vibrance`, `color.saturation`, `effects.texture`, `effects.clarity`, `effects.dehaze` (-100..100) |
| Parametric curve | `curve.highlights`, `curve.lights`, `curve.darks`, `curve.shadows` (-100..100) |
| HSL mixer | `mixer.<band>.hue/.sat/.lum` (-100..100), band = red, orange, yellow, green, aqua, blue, purple, magenta |
| B&W mix | `bw.<band>` (-100..100), only with the B&W treatment |
| Color grading | `grading.<shadows/midtones/highlights/global>.hue` 0..360, `.sat` 0..100, `.lum` -100..100; `grading.blending` 0..100 (50), `grading.balance` |
| Vignette, grain | `vignette.amount` -100..100, `vignette.midpoint`, `vignette.feather` (50); `grain.amount`, `grain.size` (25), `grain.roughness` (50) 0..100 |
| Detail | `detail.sharpenAmount` 0..150, `detail.sharpenRadius` 0.5..3 (1), `detail.sharpenMasking`, `detail.nrLuminance`, `detail.nrColor` 0..100 |
| Optics, geometry | `optics.distortion`, `optics.vignetting`; `geometry.vertical`, `geometry.horizontal`, `geometry.rotate` -10..10, `geometry.scale` 50..150 (100) |
| Profile, crop | `profile.amount` 0..200 (100); `crop.angle` -45..45 (straighten) |

Values are clamped to the range. Setting `wb.temp`/`wb.tint` switches white balance to Custom. Read the current
value first (`controls`): temp is absolute Kelvin, so change it from there (for example, +400 K for a little warmth).

## The work loop

1. `app_inspect {what: "photo"}` / `{what: "develop"}` to see the current state; `app_render {target: "photo", id}`
   to look at it.
2. Set the sliders with **one** `develop.set {values: {..}}`. This is one undo step.
3. `app_render {app: "lightcraft", target: "photo", id}` (developed and cropped) and compare it with
   `target: "before"` (the same photo unedited). For masks use `target: "mask"`, and give `mask: <maskId>` and
   `view: "color"` (tinted over the photo; the default is white on black) as **top-level** arguments next to
   `target`. Look critically: clipped highlights, colour casts, halos, oversaturated skin.
4. Fix it with another `develop.set`, or take it back with `app_undo {app: "lightcraft", steps?}`. The undo stack is
   the library's, shared with the user: undo only your own steps.

## Commands (verified; `?` = optional)

| Task | Command and params |
|---|---|
| Find / view | `library.filter {..Filter}`, `library.clearFilter {}`, `library.sort {key?: captureDate\|importDate\|editDate\|fileName\|rating, ascending?}`, `library.source {kind: all\|recentlyAdded\|album\|picks, id?: albumId}` |
| Select | `library.select {ids, active?, mode?}`, `library.selectAll {}`, `library.selectNone {}`, `library.selectBy {flag?, rating?, ratingOp?: gte\|eq\|lte, label?}`, `library.next {}` |
| Cull | `photo.rate {rating: 0..5, ids?}`, `photo.flag {flag: pick\|reject\|none, ids?}`, `photo.label {label: red\|yellow\|green\|blue\|purple\|none, ids?}`, `photo.analyze {ids?, rejectBelow?, pickBest?}` (focus scores) |
| Metadata | `photo.setMeta {ids?, title?, caption?, creator?, copyright?, keywords?, addKeywords?, removeKeywords?}` |
| Sliders | `develop.set {control, value}` or `{values: {id: n, ..}, ids?: [..]}`; `develop.adjust {control, delta}` (active photo); `develop.quickAdjust {control, delta, ids?}` (each photo from its own value) |
| Auto | `develop.auto {}` (auto tone and presence), `develop.wb {mode: asShot\|auto\|daylight\|cloudy\|shade\|tungsten\|fluorescent\|flash\|custom, temp?, tint?}`, `develop.autoBwMix {}` |
| Treatment, profile | `develop.treatment {bw?: bool}` (toggles without `bw`), `develop.profile {id, amount?: 0..200}` (ids from `profiles.list {}`) |
| Tone curve | `develop.curve {channel: master\|red\|green\|blue, points: [[x,y],..]}` (0..1), `curve.reset {channel?}`, `curve.applyPreset {name}` (`curve.presets {}`) |
| Crop, rotate | `crop.set {rect?: [x0,y0,x1,y1] (0..1), angle?}`, `crop.aspect {aspect: "original"\|"free"\|"1x1"\|"4x5"\|"2x3"\|"16x9"\|[w,h]}`, `crop.straighten {angle}`, `crop.autoStraighten {}`, `crop.reset {}`, `geometry.upright {mode: off\|auto\|level\|vertical\|full}`, `photo.rotateLeft/rotateRight {ids?}` |
| Reset | `develop.reset {ids?}`, `develop.resetSection {section: light\|curve\|color\|mixer\|grading\|effects\|vignette\|grain\|detail\|optics\|geometry}`, `develop.resetControl {control}` |
| Presets, LUTs | `presets.list {}` → `preset.apply {id, amount?: 0..200, ids?}`; `preset.create {name, group?, groups?}`; `preset.import {paths}` (XMP, .lrtemplate, .lcpreset, .lmp); `profile.import {paths}` (.cube LUTs become profiles, then `develop.profile {id: "lut:…"}`) |
| Masks | `mask.add {kind: linear\|radial\|sky\|subject\|background\|luminanceRange\|colorRange\|brush, name?, ..shape}` (returns the mask, now selected), then `mask.adjust {id?, values: {exposure (-4..4), contrast, highlights, shadows, whites, blacks, temp, tint, texture, clarity, dehaze, saturation, ..}}`; `mask.invert {id?}`, `mask.delete {id?}`, `mask.brushStroke {id?, points, size?, erase?}` |
| Retouch | `spot.findDust {sensitivity?}`, `spot.add {mode?: remove\|heal\|clone, points: [[x,y]], size?}`, `redeye.add {center, rx, ry}` |
| Copy, sync | `develop.copy {groups?}` → `develop.paste {ids?, groups?}`; `develop.sync {groups?}` (active photo → all selected); `develop.matchExposure {ids?}` |
| Versions | `version.create {name?}` (snapshot before big changes), `version.restore {index}`, `history.restore {index}` |
| Albums | `album.create {name, addSelected?: bool}`, `album.addPhotos {id, ids?}`, `photo.virtualCopy {ids?, name?}` |
| Export | `app.export {dir: "out", ids?, format?: jpg\|png\|tiff\|webp\|avif, longEdge?: px (0 = full size), quality?: 1..100, colorSpace?: srgb\|displayP3\|adobeRgb}` → `{files}` |

Mask shapes (coordinates normalized 0..1, y down): linear `start: [x,y], end: [x,y]` (full effect at `start`);
radial `center: [x,y], rx, ry, angle?, feather?: 0..100, invert?`; luminanceRange `lo, hi` (0..1). `sky`,
`subject` and `background` need no model. `object` and `prompt` need the SAM 3 model (3.4 GB): only offer it, and
never run `segment.model.download` without the user's yes. Settings groups (for `groups`): `whiteBalance`, `light`,
`toneCurve`, `color`, `colorMixer`, `colorGrading`, `effects`, `vignette`, `grain`, `detail`, `optics`, `geometry`,
`crop`, `masks`, `profile`, `treatment`, `calibration`.

## Examples

Brighten and warm the active photo, check it against the original, keep the change:
```json
{"app":"lightcraft","command":"library.select","params":{"ids":[42]}}
{"app":"lightcraft","what":"controls","params":{"section":"color"}}
{"app":"lightcraft","command":"develop.set","params":{"values":{"light.exposure":0.6,"light.shadows":25,"light.highlights":-30,"wb.temp":5900,"color.vibrance":15}}}
{"app":"lightcraft","target":"photo","id":42}   then   {"app":"lightcraft","target":"before","id":42}
```
Same look on several photos: edit one, then `develop.copy {groups: ["light","whiteBalance","color"]}` and
`develop.paste {ids: [43,44,45]}`. Or skip the clipboard with
`develop.set {values: {"light.exposure": 0.3}, ids: [43,44,45]}` (the same absolute value on every photo) or
`develop.quickAdjust {control: "light.exposure", delta: 0.3, ids: [..]}` (relative).

Darken a bright sky: `mask.add {kind: "linear", start: [0.5, 0.0], end: [0.5, 0.45], name: "Sky"}` →
`mask.adjust {values: {exposure: -0.7, highlights: -40, dehaze: 10}}` → `app_render {target: "mask", mask: <id>,
view: "color"}` to check the coverage.

Picks to the workspace: `app_inspect {what: "photos", params: {filter: {flag: "pick"}}}` →
`app.export {dir: "out/picks", ids: [..], format: "jpg", longEdge: 2048, quality: 85}`.

## Pitfalls in Septet

- **The library is the user's own photo library** (outside portable mode, it is the same library as standalone
  LightCraft). Never delete, move or rename photos, files or folders unless the user explicitly asks: not
  `photo.delete`, `photo.deletePermanently`, `library.import {mode: "move"}`, `photo.rename`, `folder.rename`,
  `folder.move`, `keyword.delete`, `album.delete`, `history.clear`. Don't re-rate or re-flag photos the user didn't
  point you to.
- **Catalog lock**: if standalone LightCraft (or another Septet) has the library open, Lightcraft starts in a
  temporary session that saves nothing (`app_execute library.info {}` → `persistent: false`). Tell the user, and don't do real work
  there.
- Commands that would open a dialog or a file picker, show a file in the file manager or open another program fail
  in Septet with an error that says why. Pass the paths they list instead. Don't use `dialog.*`,
  `photo.editInExternal` or `photo.editExternal` (it writes a TIFF next to the original). To edit pixels, use
  `app.export` into the workspace, then `septet_open` the file in Photocraft.
- `app.export` without `dir`/`path` asks the user and falls back to the last export folder: always give a workspace
  `dir`. `photo.saveMetadataToFile` writes XMP sidecars next to the originals: only do it on request.
- Develop edits apply to the **active** photo: after `library.select`, check `document` → `selection.active` before a
  series of edits. `controls` shows the active photo's values.
- `app_render` makes a preview (default 1024 px). For full-quality files use `app.export`, not `save_as`.
- Raws that can only be shown from their embedded JPEG are marked `previewOnly`. Their edits are approximate, so
  tell the user.
