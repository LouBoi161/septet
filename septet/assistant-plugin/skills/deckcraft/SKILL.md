---
name: deckcraft
description: Drive Deckcraft (Septet's PowerPoint-like presentation app) directly with the app_* tools - start a deck with file.new (themes Harbor, Ember, Meadow, Nocturne… and starter templates), add slides by layout with title and body, fill placeholders, shapes, text boxes, pictures, charts, tables, SmartArt and icons, themes, colour variants and backgrounds, transitions (fade, push, morph…) and entrance/emphasis/exit/path animations with timing and text builds, speaker notes, slide shows, save as .pptx/.deckcraft and export PDF/PNG/GIF into the workspace - with verified command ids and parameter formats, the slide, id and point model, and the views and renders (one slide, or the whole deck as a grid) that work in Septet. Load before calling app_commands/app_execute/app_inspect/app_render with app "deckcraft", or when making or changing a presentation, slides, a pitch deck or a talk in Septet.
---

# Driving Deckcraft

Load `septet:septet-apps` first for the general rules. Deckcraft has ~330 commands. `app_execute` runs them like the
ribbon does (the user sees each change, and each change is one undo step). The ids and parameters below were checked
against the registry (`deckcraft/crates/engine/src/cmd/*`) and run. For anything else, call
`app_commands {app: "deckcraft", filter: "…"}`: each command's `params` text is its exact format (`?` = optional,
`→` = what it answers).

## Start

- Deckcraft starts on its start screen with **no presentation open**; until one is open, every view, render and
  editing command fails ("no presentation is open").
- **`file.new {theme?, template?, size?: [w, h], blank?: true}`** makes a presentation with one empty title slide and
  shows it. Themes: `DeckCraft`, `Harbor`, `Ember`, `Meadow`, `Nocturne` (dark), `Paper`, `Coral Reef`, `Slate`.
  Starter templates (full example decks you then rewrite): `Briefing`, `Lesson`, `Pitch`, `Field Notes`,
  `Quarterly`, `Evening Talk`.
- Deckcraft keeps **several presentations open** and shows one: `file.new` and `septet_open` (pptx, potx, ppsx,
  deckcraft) add one and make it current. `app_inspect {what: "documents"}` lists them; `window.next {index}`
  switches. Commands, views and renders act on the current one.
- `file.close` and `file.revert` **refuse** while the presentation has unsaved changes. Ask the user: save first, or
  pass `"force": true` only when the user agreed to discard them.
- Commands that only open a dialog or a file picker (`app.saveAsDialog`, `app.insertPictureDialog`,
  `app.preferences`, `shape.sizePosition {}` without keys…) fail and say so; use the engine command with parameters
  (`file.saveAs {path}`, `insert.picture {path}`, `shape.setBounds {…}`).

## The model (read this)

- **Slides are 960 × 540 pt** (16:9; `design.slideSize` changes it). Positions and sizes are points, x to the right,
  y down; a `rect` is `[x, y, width, height]`.
- **Slide indices start at 0** everywhere (`index`, `slide`, `at`, `app_render slide`). `slide.new` inserts after the
  current slide and makes the new one current; `slide.go {index}` changes the current slide.
- **Shape ids** are numbers from `slide.inspect` or from the command that made the shape (`shape.insert` → `{id}`).
  They are looked up **on the current slide only**: `slide.go` first when you work on another slide.
- A command that adds a shape selects it. `format.*`, `shape.fill`, `shape.line`, `animation.add`… act on the
  selection unless you pass `ids: [..]` (or `id`). `edit.select {ids}` selects others.
- **Text**: `text.set {id, text}` replaces a shape's or placeholder's whole text (`\n` = new paragraph, which also
  makes bullets in a content placeholder). `slide.new` takes `title` and `body` directly. To format part of a text:
  `text.edit {id}` → `text.select {anchor: [para, char], caret: [para, char]}` (0-based, characters) → `format.*` →
  `text.exit`. `format.*` on a selected shape (not editing) formats all its text.
- Colours: `"#RRGGBB"` or theme colours `accent1`…`accent6`, `tx1`, `bg1`, `tx2`, `bg2`. Prefer theme colours: they
  follow `design.theme` and `design.variant`.

## Work loop

1. **Inspect**: `app_inspect {app: "deckcraft", what}`:
   - `document`: slides with titles, layouts and shape counts, the selection, the slide size and the theme.
   - `slide` (`params: {index}`; the current one by default): each shape's `id`, `kind`, `name`, `placeholder`,
     `box` [x, y, w, h], `text`, `fill`, plus the slide's `animations`, `transition` and `notes`.
   - `shape` (`params: {id}`), `text` (`params: {id}`), `selection` (selection and formatting at it),
     `animations` (`params: {slide}`), `comments`, `history`, `documents`, `show`.
2. **Execute** one command at a time. Read the error: it names the wrong or missing parameter.
3. **Look**: `app_render {app: "deckcraft", target: "slide", slide: n}` draws a slide as the show shows it (the
   final state, after all its animations). `app_render {target: "deck"}` draws every slide in one grid image (up to
   24, `from` for more; hidden slides dimmed): use it to check a whole deck's consistency in one look.
4. **Fix**: `app_undo {app: "deckcraft", steps?}`. `edit.redo {}` redoes.

## Commands by task

| Task | Commands |
|---|---|
| Slides | `slide.new {layout?, title?, body?, at?}` (layouts: `title`, `titleAndContent`, `sectionHeader`, `twoContent`, `comparison`, `titleOnly`, `blank`, `contentWithCaption`, `pictureWithCaption`) · `slide.layout {layout, index?}` · `slide.duplicate {index?}` · `slide.delete {index?}` · `slide.move {from?, to}` · `slide.go {index}` · `slide.hide {index?, hidden?}` · `slide.notes {text, index?}` (speaker notes) · `section.add {name?, at?}` · `slide.fromOutline {text, markdown?: true}` (a Markdown outline becomes slides: headings → slides, list items → bullets) |
| Shapes | `shape.insert {preset, rect, text?}` → `{id}` (presets: `rect`, `roundRect`, `ellipse`, `triangle`, `rightArrow`, `star5`, `chevron`, `hexagon`, … see `shape.presets`) · `insert.textBox {rect?, text?}` · `insert.icon {name, rect?}` (Lucide icons, see `insert.icons`) · `shape.connect {from, to}` · `shape.freeform {points, closed?, smooth?}` · `insert.wordArt {text?}` |
| Look of a shape | `shape.fill {color \| gradient: {stops: [[0, c], [1, c]], angle?} \| none: true \| transparency?, ids?}` · `shape.line {color?, width?, dash?, none?, ids?}` · `shape.effects {shadow?: outer\|inner\|none, glow?, softEdges?, reflection?}` · `shape.quickStyle {index: 0..41}` · `shape.change {preset}` |
| Place | `shape.setBounds {x, y, w, h, ids?}` · `shape.move {dx?, dy?} \| {x?, y?}` · `shape.resize {w?, h?, lockAspect?}` · `shape.rotate {deg}` · `arrange.align {edge: left\|center\|right\|top\|middle\|bottom, to?: slide\|selection, ids?}` · `arrange.distribute {dir: horizontal\|vertical}` · `arrange.bringToFront` / `sendToBack` · `arrange.group {ids}` / `ungroup` · `edit.duplicate` · `edit.delete {ids?}` |
| Text | `text.set {id, text}` · `format.size {size}` · `format.font {family}` · `format.bold` / `italic` / `underline {on?}` · `format.color {color}` · `format.align {align: left\|center\|right\|justify}` · `format.anchor {anchor: top\|middle\|bottom}` · `format.bullets {on?, char?}` · `format.numbering {on?, scheme?}` · `format.lineSpacing {lines}` · `format.paragraph {spaceBefore?, spaceAfter?, indentLeft?}` · `format.autofit {mode: none\|shrink\|resize}` · `text.replace {text, with}` (every slide) · `text.find {text}` |
| Pictures, media | `insert.picture {path, rect?}` (workspace path; png, jpg, gif, webp, svg) · `picture.crop {left?, top?, right?, bottom?}` (fractions) · `picture.style {style}` · `picture.removeBackground` · `insert.video {path, rect?}` / `insert.audio {path}` · `media.options {autoplay?, loop?}` |
| Charts, tables, diagrams | `insert.chart {type: column\|bar\|line\|pie\|doughnut\|area\|scatter\|stackedColumn…, rect?, categories, series: [{name, values}]}` → `{id}` · `chart.data {…, id?}` · `chart.options {title?: text\|null, legend?: b\|t\|l\|r\|null, dataLabels?, gridlines?}` · `insert.table {rows, cols, rect?}` then `text.set {id, cell: [r, c], text}` · `table.style {style: medium2-accent1\|light1-accent2\|…}` · `insert.smartArt {kind: list\|process\|cycle\|hierarchy\|pyramid\|matrix, items: ["a", "\tsub", "b"]}` · `smartArt.style` / `smartArt.colors` |
| Design | `design.theme {name}` · `design.variant {index: 0..3}` (colour variants, see `design.variants`) · `design.colors {name}` (colour schemes: Harbor, Ember, Meadow, Nocturne, Paper, Coral Reef, Slate, Grayscale, Ocean, Sunset, Forest, Berry) · `design.fonts {name}` (Inter, Classic, Geometric, Editorial, Humanist, Friendly, Neutral) · `design.background {color \| gradient \| style: 1..12, all?: true}` · `design.ideas {slide?}` → `design.applyIdea {id}` · `design.headerFooter {slideNumber?, footer?, footerText?, date?}` · `design.slideSize {preset: widescreen\|standard}` |
| Transitions | `transition.set {kind, option?, duration?: ms, index?}` (kinds: `fade`, `push`, `wipe`, `split`, `reveal`, `cover`, `uncover`, `morph`, `zoom`, `cube`, `flip`, `gallery`, `vortex`, … see `transition.list` for each kind's `options`) · `transition.applyAll` (the current slide's to every slide) · `transition.timing {duration?, onClick?, after?: ms, index?}` (auto-advance) |
| Animations | `animation.add {effect, class?, option?, start?: onClick\|withPrevious\|afterPrevious, duration?: ms, delay?: ms, ids?}` (adds; `animation.set` replaces the shape's) · entrance: `appear`, `fade`, `fly`, `float`, `wipe`, `zoom`, `grow`, `bounce`, `rise`… · emphasis: `pulse`, `spin`, `growShrink`, `teeter`, `colorPulse`… · exit: `fadeOut`, `flyOut`, `zoomOut`, `disappear`… · motion paths: `lines`, `arcs`, `turns`, `loops`, `zigzag`… (see `animation.list` for each effect's `options`) · `animation.options {textBuild: asOne\|byParagraph\|byWord\|byLetter, option?, index}` · `animation.timing {index, start?, duration?, delay?, repeat?}` · `animation.move {index, to}` · `animation.remove {index}` · `animation.clear` |
| Slide show | `show.start {from?}` (full screen in a window of its own; `show.end` ends it) · `show.setup {type?: speaker\|browsed\|kiosk, loop?, useTimings?}` · `show.customShow {name, slides}` |
| Save, export | `file.saveAs {path: "out/talk.pptx"}` (pptx or deckcraft by extension) · `file.save {}` · `file.export {path, format?: pdf\|png\|jpeg\|pptx\|gif\|outline, all?: true (png/jpeg: every slide, files numbered from 1), slide?, scale?, layout?: slides\|notes\|handouts}` · `file.properties {title?, author?}` |

## Example: a three-slide talk with a chart, an animated list and transitions

Shorthand: `app_execute <command> <params>` means `app_execute {"app": "deckcraft", "command": …, "params": …}`.
The ids (258…265) are what a fresh presentation gave; take yours from the answers and `slide.inspect`.

```
app_execute file.new {"theme": "Harbor"}
app_inspect {"app": "deckcraft", "what": "slide"}                → Title 1 = 258, Subtitle 2 = 259
app_execute text.set {"id": 258, "text": "Quarterly Review"}
app_execute text.set {"id": 259, "text": "Q3 2026 · Sales team"}
app_execute slide.new {"layout": "titleAndContent", "title": "Highlights", "body": "Revenue up 12%\nThree new markets\nTeam grew to 40"}
app_inspect {"app": "deckcraft", "what": "slide"}                → Content Placeholder 2 = 262
app_execute animation.add {"effect": "fly", "option": "fromBottom", "ids": [262]}
app_execute animation.options {"textBuild": "byParagraph", "index": 0}      (one bullet per click)
app_execute slide.new {"layout": "titleOnly", "title": "Revenue by region"}
app_execute insert.chart {"type": "column", "rect": [80, 140, 800, 360], "categories": ["North", "South", "West"], "series": [{"name": "Q3", "values": [18, 9, 4]}]}
app_execute chart.options {"title": null, "legend": null, "dataLabels": true}
app_execute transition.set {"kind": "push", "duration": 600}
app_execute transition.applyAll {}
app_render  {"app": "deckcraft", "target": "deck"}
app_execute file.saveAs {"path": "out/quarterly.pptx"}
app_execute file.export {"path": "out/quarterly.pdf"}
```

## Checking animations and transitions

A render shows each slide with every shape in place, without animation states (an exit-animated shape still
shows), so check the build order with
`app_inspect {what: "animations", params: {slide}}`: each entry has `shape`, `effect`, `class`, `start`,
`durationMs`, `delayMs` and `textBuild`, in play order (`index` 0, 1, …). `slide.inspect` shows the slide's
`transition`. For a slide whose shapes appear one after another, make sure the first entry starts `onClick` (or
`afterPrevious` to run on its own) and the rest `withPrevious`/`afterPrevious` as intended. To let the user watch
it, `show.start {from: n}` (they end it with Esc, or you with `show.end`).

## Pitfalls

- `text.set` on a placeholder keeps the layout's formatting; on a content placeholder each `\n` line becomes a
  bullet. For text without bullets use `insert.textBox` or `format.bullets {on: false}`.
- `shape.insert` needs `rect`. Keep shapes inside 0…960 × 0…540, and leave room under a title (titles sit at about
  y 29–133 on title-and-content slides; check with `slide.inspect`).
- Ids belong to the current slide: after `slide.new`, ids from an earlier slide fail until `slide.go` back.
- `animation.add` without `ids` animates the selection, which is the last shape you made, not necessarily the one you
  mean: pass `ids`.
- `file.export` with `all: true` writes `name1.png`, `name2.png`, … (numbered from 1, while slide indices start at
  0). `file.save {}` without `path`, saving outside the workspace, and `app.print` ask the user first. Write a new
  file rather than saving over the user's.
- `septet_place` into an open presentation puts pictures, audio and video on the **current** slide (centred and scaled
  to fit the slide); move and size them with `shape.setBounds` afterwards.
