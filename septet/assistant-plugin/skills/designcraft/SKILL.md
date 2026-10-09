---
name: designcraft
description: Drive Designcraft (InDesign-like page layout) directly with the app_* tools - make a document with file.new, pages and parent pages, text/image/shape frames, place images and text files, story text, paragraph/character styles, fonts, swatches, align/arrange/group, layers, export PDF/PNG into the workspace - with verified command ids and parameter formats, plus the views and render targets that work in Septet. Load before calling app_commands/app_execute/app_inspect/app_render with app "designcraft", or when building a multi-page layout (flyer, menu, brochure, report, magazine) in Septet.
---

# Driving Designcraft

Designcraft has ~550 commands (engine plus UI). `app_execute` runs them as their menu items do: the user sees the
change, and each changing command is one undo step. The ids and parameter formats below were checked against the registry
(`crates/engine/src/cmd/*`). For anything else, call `app_commands {app: "designcraft", filter: "…"}`. Each command's
`params` text is its exact parameter format (`?` = optional, `|` = alternatives, `→` = what it returns).

## Start

- Designcraft opens only `.designcraft` and `.idml` files. Open an existing one with `septet_open {paths, app: "designcraft"}`
  or `app_execute file.open {path}`. To start a new layout, run **`file.new`** (it also starts the app):
  `{preset?: "Letter"|"A4"|…, width?, height?, pages?, facingPages?, columns?, gutter?, margins?: number|{top,bottom,inside,outside}, bleed?, title?}`.
  Presets: Letter, Legal, Tabloid, Letter - Half, A3, A4, A5, B5, Business Card, Postcard, Web 1920 × 1080,
  Web 1366 × 768, Web 1024 × 768, Mobile 1080 × 1920, Mobile 390 × 844, Tablet 1024 × 1366. With no preset the
  page is 612 × 792 pt with 36 pt margins and facing pages. `primaryTextFrame: true` (accepted, not listed) threads
  one margin-sized text frame through all pages.
- **`margins` as an object needs all four keys.** If one is missing, the whole object is silently ignored. Use a
  number for even margins. `bleed` is one number for all sides.
- Never run `app.newDocumentDialog`, `app.openDialog`, `app.placeDialog` or other `app.*Dialog` commands: they open
  dialogs, and Septet closes them again and fails the call.
- Save: `file.saveAs {path: "out/flyer.designcraft"}`, then `file.save {}`. `app_inspect {what: "documents"}` lists
  open documents, and `file.activate {index}` switches between them.

## Geometry, pages, spreads

- All values are **points**. y points down. Coordinates are **spread coordinates**: x = 0 is the left edge of the
  spread's leftmost page, and y = 0 is the top of the page.
- Facing pages (the default) put page 1 alone on spread 0, then pages 2–3 on spread 1, and so on. The right-hand
  page of a spread starts at x = page width. With `facingPages: false`, each page is its own spread.
  Check this in `app_inspect {what: "document"}`: `spreads[{index, pages[{index, name, bounds:[x0,0,x1,h], margins}], items}]`.
- `spread` params take the spread index (`1`) or `{"kind":"doc","index":1}`. Parent pages use
  `{"kind":"parent","index":0}` (A-Parent). If `spread` is missing, spread 0 is used.
- **Page numbering is mixed:** `app_inspect`/`app_render` `page`, `layout.pageSize`, `layout.section` and
  `file.place` `page` count from 1. Inspect output `pages[].index`, `layout.pages.insert/delete/applyParent/move`
  and `app.exportPng` `page` count from 0.

## Ids and the selection

- Items (frames, lines, groups) have numeric ItemIds. Stories, layers and assets have their own ids. Read them in
  `app_inspect document`: items have `{id, name, kind, layer, bounds:[x0,y0,x1,y1], fill, stroke, story?, asset?, children?}`.
  The document also lists `layers`, `stories[{id, frames, overset, preview}]`, `paragraphStyles`,
  `characterStyles`, `swatches`, `selection` and `activeLayer`.
- `frame.create` returns `{id}`, plus `story` for text frames. `file.place` returns `{id, asset}`. `layer.new` returns `{id}`.
- Commands that list `ids?` act on those items, even when nothing is selected. Pass `ids` whenever you can.
- Commands without `ids` (`type.char`, `type.para`, `style.paragraph.apply`, `style.character.apply`,
  `edit.duplicate`, `type.fillWithPlaceholder`) act on the selection. First run
  `selection.set {ids: [id], add?: bool}`. Selecting a text frame makes text formatting apply to its **whole story**.
  `edit.deselectAll {}` clears the selection. `selection.set` is not an undo step.
- **A text frame made by `frame.create` leaves a text caret at the end** (`caret` defaults to true). In that state
  `type.char` changes nothing, and `file.place` puts text files *into that story*. Pass `"caret": false`, or
  `selection.set` the frame, before formatting.

## Work loop

1. `app_inspect {what: "document"}` (`params: {page: n}` for one page). `app_commands {filter: "…"}` for unknown commands.
2. `app_execute`, one command at a time. Read the error: it names the missing or wrong parameter.
3. `app_render {target: "page", page: n}` to see the whole page on its paper. Then `{target: "object", id}` to see one item
   alone, cropped. Check text against `stories[].overset` (true = text doesn't fit; enlarge the frame or thread it).
4. `app_undo {app: "designcraft", steps?}` for mistakes. `app_inspect {what: "history"}` lists the undo labels.

## Commands (verified params)

| Task | Command and params |
|---|---|
| Pages | `layout.pages.insert {count?: 1, after?: page index (0-based, default last), parent?: "A"\|null}` · `layout.pages.delete {pages: [index]}` · `layout.pages.move {from, to}` · `layout.documentSetup {width?, height?, pages?: count, facingPages?, bleed?, slug?, adjustLayout?: bool}` · `layout.pageSize {pages: [1-based], width?, height?, preset?}` |
| Margins, guides | `layout.marginsAndColumns {pages?: [index], margins?, columns?, gutter?, adjustLayout?}` · `guide.add {orientation: horizontal\|vertical, position, spread?}` · `layout.createGuides {rows?, columns?, rowGutter?, columnGutter?, fitTo?: margins\|page}` |
| Parent pages | `layout.parents.new {prefix?, name?}` · `layout.pages.applyParent {pages: [index], parent: "A"\|null}` · items for a parent: `frame.create` with `spread: {"kind":"parent","index":0}` · `layout.section {page (1-based), startNumber?, style?: arabic\|upperRoman\|…, prefix?}` |
| Frames | `frame.create {spread?, rect: [x0,y0,x1,y1], shape?: rectangle\|ellipse\|polygon, content?: graphic\|text\|unassigned, sides?, text?, caret?: bool}`. The default `graphic` is an empty image frame with no stroke. Use `unassigned` for a plain shape (it gets a 1 pt black stroke). · `line.create {spread?, a: [x,y], b: [x,y]}` · `path.create`: see `app_commands` |
| Place files | `file.place {path, frame?: id, spread?, x?, y?, width?, pdfPage?, pdfCrop?, layoutPage?}`. An image goes into `frame`, or into a **selected** empty or image frame, or else into a new frame at x/y (default: the margin corner) with `width` (default ≤ 60 % of the page width). Text (`.txt .md .docx .rtf`) and `.xlsx` (table) go into the caret, `frame`, or a new frame from `page` (1-based) + `rect`, and also take `autoflow?: bool`, `removeStyles?`, `styleMap?`, `styleConflicts?` |
| Frame content | `object.fit {mode: fillProportionally\|fitProportionally\|fitContentToFrame\|centerContent\|fitFrameToContent, ids?}` · `object.fittingOptions {autoFit?, fitting?, align?: 0..8, crop?, ids?}` · `object.content {type: graphic\|text\|unassigned, ids?}` · `object.textFrameOptions {columns?, gutter?, inset?: n\|[t,l,b,r], verticalJustification?: top\|center\|bottom\|justify, autoSize?, ids?}` · `object.cornerOptions {shape: none\|rounded\|…, size, ids?}` · `object.textWrap {mode: none\|boundingBox\|contour\|jumpObject\|jumpToNextColumn, offset?, ids?}` |
| Story text | `story.setText {story, text}` (replaces all of it; `\n` starts a paragraph) · `story.replaceRange {story, start, end, text}` · `story.get {story? \| frame?}` · `find.change {find, change, grep?, scope?: document\|story\|selection, story?}` · `text.select {story, anchor, focus}` (UTF-8 byte offsets, as `find.find` reports them) then `text.insert {text}` · `type.fillWithPlaceholder {frame?}` |
| Character / paragraph | `type.char {attrs: {fontFamily?, fontStyle?, size?, leading?: {"kind":"auto"}\|{"kind":"points","value":14}, tracking?, hScale? (1.0 = 100 %), baselineShift?, fill?: swatch name, capitalization?: normal\|allCaps\|smallCaps, underline?, position?: superscript\|subscript}}` · `type.para {attrs: {align?: left\|center\|right\|leftJustified\|fullyJustified\|…, leftIndent?, firstLineIndent?, spaceBefore?, spaceAfter?, dropCapLines?, hyphenate?}}` (`null` clears an override) |
| Styles | `style.paragraph.create {name, basedOn?, nextStyle?, para?: {…type.para keys}, chars?: {…type.char keys}}` · `style.paragraph.edit {name, rename?, …}` · `style.paragraph.apply {name, clearOverrides?}` · `style.character.create {name, basedOn?, chars?}` · `style.character.apply {name}` · `style.object.create {name, fromSelection?, fill?, paragraphStyle?}` · `style.object.apply {name, ids?}` · `style.list {}`. The default paragraph style is `[Basic Paragraph]` |
| Fonts | `font.list {}` → families and styles (missing ones first) · `font.replace {family, style?, toFamily, toStyle?}`. Bundled: `Source Serif 4` (the default), `Source Sans 3`. Others must be installed system fonts, or sit in a `Document Fonts` folder next to the saved `.designcraft` (loaded on `file.open`) |
| Colour | `swatch.create {name?, color: "#rrggbb"\|{c,m,y,k}(0..100)\|[r,g,b], spot?}` → `{name}` · `object.fill {swatch, tint?: 0..1, ids?}` · `object.stroke {swatch?, weight?, align?, ids?, …}` · `object.color {color, target?: fill\|stroke, ids?}` (unnamed colour) · `object.gradient {kind?, stops?, angle?, ids?}` · `object.opacity {opacity: 0..1, blend?, ids?}` · `object.dropShadow {on?, distance?, angle?, size?, opacity?, ids?}`. Built-in swatches: `[None]`, `[Paper]`, `[Black]`, `[Registration]` |
| Position | `transform.move {dx, dy, copy?, ids?}` · `transform.set {x?, y?, width?, height?, rotation?, ref?: 0..8, ids?}` · `transform.rotate {angle (CCW), ids?}` · `transform.scale {sx, sy, ids?}` · `edit.duplicate {}` (selection) |
| Align, arrange, group | `object.align {edge: left\|hcenter\|right\|top\|vcenter\|bottom, to?: selection\|keyObject\|margins\|page\|spread, ids?}` · `object.distribute {axis: horizontal\|vertical, by?: centers\|spacing, spacing?, ids?}` · `object.arrange {to: front\|forward\|backward\|back, ids?}` · `object.group {ids?}` · `object.ungroup {ids?}` · `object.lock {ids?}` · `object.rename {id, name}` · `edit.clear {ids?}` (delete) |
| Layers | `layer.new {name?}` → `{id}` · `layer.activate {id}` (new frames go on the active layer) · `object.setLayer {layer: id, ids?}` · `layer.set {id, name?, visible?, locked?, printable?}` · `layer.move {id, to: index (0 = top)}` · `layer.delete {id}` |
| Tables, QR | `table.insert {rows?, cols?, headerRows?, width?}` (at the text caret) · `table.setCell {fill?, text?, …}` · `object.qrCode {type?: url\|text\|…, content, rect?, spread?}` |
| Export | `file.exportPdf {path, pages?: "1-3,5"\|[1,3], bleed?: bool, marks?: bool, standard?: "none"\|"x4"\|"a2b", spreads?, title?, author?}` · `app.exportPng {path, page?: 0-based, scale?: 2}` (params not listed; always pass `path`) · `file.exportIdml {path}` · `snippet.export {path}` (selection) |

## Examples

```json
{"app":"designcraft","command":"file.new","params":{"preset":"A4","pages":2,"facingPages":false,"margins":42,"bleed":8.5,"title":"Menu"}}
{"app":"designcraft","command":"frame.create","params":{"spread":0,"rect":[42,42,553,140],"content":"text","text":"Summer Menu\nFresh from the garden","caret":false}}
{"app":"designcraft","command":"type.char","params":{"attrs":{"fontFamily":"Source Sans 3","fontStyle":"Bold","size":36,"fill":"[Black]"}}}
{"app":"designcraft","command":"file.place","params":{"path":"media/hero.jpg","frame":12}}
```

The `type.char` call formats the frame from the call before it, which `caret: false` left selected. To fill an image frame:
`frame.create {"rect":[42,160,553,460]}` (graphic) → `{id: 12}`, then `file.place` with `frame: 12` fills it proportionally.
Then try `object.fit {"mode":"fillProportionally","ids":[12]}` if needed. For a style, run
`style.paragraph.create {"name":"Dish","para":{"spaceAfter":6},"chars":{"fontFamily":"Source Serif 4","size":11,"leading":{"kind":"points","value":14}}}`,
then `selection.set {ids:[frame]}` and `style.paragraph.apply {"name":"Dish"}`.

## Views and renders in Septet

- `app_inspect what`:
  - `document`: pages, layers, styles, swatches, and up to 50 stories. `params.page` (1-based) keeps one spread, and `depth` sets how far groups open.
  - `page`: one page in full (default: the page in view).
  - `object` (`id`): one item, also inside groups.
  - `story` (`id`): the full text, its frames and paragraph count.
  - `selection`, `history`, `documents`.
- `app_render target`:
  - `page` (`page`, 1-based; default: the page in view): on its paper.
  - `object` (`id`) or `selection`: alone, cropped, on a checkerboard.
  - `layer` (`id`, `page`): that page with only that layer's items, transparent.

  `save_as` also writes the picture as a PNG.

## Pitfalls

- **Paths:** relative paths resolve against the workspace. Placed images stay **linked** (`link`), so don't move or
  delete them. A command that would open a file picker (`file.place` without `path`) fails: pass `path`.
- A selected frame captures `file.place`, because an image replaces its content. Run `edit.deselectAll {}` first if you
  want a new frame.
- `object.fill` takes a **swatch name**, not a hex value. Create the swatch with `swatch.create` and use the `name` it
  returns (names are made unique), or use `object.color`.
- `object` renders and views see only document pages. Items on parent pages show up in `page` renders only.
- Long text: after `story.setText`, check `overset` in `app_inspect document` (or `story.get`). Fix it by making the frame
  bigger, using `object.fit {mode: "fitFrameToContent"}`, or placing the text with `autoflow: true`.
- Export into the workspace (`out/…`). Exports elsewhere ask the user. For print, use `file.exportPdf` with `bleed: true`,
  `marks: true` and `standard: "x4"` (see `septet:print-and-pdf`).
