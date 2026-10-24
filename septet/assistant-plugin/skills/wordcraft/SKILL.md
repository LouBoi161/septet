---
name: wordcraft
description: Drive Wordcraft (Septet's Word-like word processor) directly with the app_* tools - start a document with file.new (blank, letter, resume, report templates), type paragraphs at the caret, select text, character and paragraph formatting, styles (Title, Heading 1, Normal), bullets and numbering, tables, pictures, page size and margins, headers, footers and page numbers, comments and tracked changes, find and replace, save as .docx and export PDF/PNG into the workspace - with verified command ids and parameter formats, the caret and position model, and the views and renders that work in Septet. Load before calling app_commands/app_execute/app_inspect/app_render with app "wordcraft", or when writing or formatting a document, letter, report, CV or memo in Septet.
---

# Driving Wordcraft

Load `septet:septet-apps` first for the general rules. Wordcraft has ~620 engine commands. `app_execute` runs them
like the ribbon does (the user sees each change, and each change is one undo step). The ids and parameters below were
checked against the registry (`wordcraft/crates/engine/src/cmd/*`). For anything else, call
`app_commands {app: "wordcraft", filter: "…"}`: each command's `params` text is its exact format (`?` = optional).

## Start

- Wordcraft shows **one document at a time**. It starts on the Start screen with **no document open**, and until
  one is open every view, render and editing command fails ("no document is open").
- Run **`file.new {template?: "blank"|"sample"|"letter"|"resume"|"report"}`** to start. `blank` uses the region's paper
  (A4 or Letter), so set the size you want. The templates contain placeholder text ("Your Name", "Dear Recipient,",
  "Report Title"…) that you replace (see Find and replace).
- Open a file: `septet_open {paths: ["draft.docx"]}` (docx, doc, rtf, odt, md, txt, html, tex). Opening **replaces**
  the open document. septet_open asks the user to save changes first. `app_execute` `file.new`, `file.open {path}`,
  `file.newFromTemplate` and `file.close` **refuse** while the document has unsaved changes. Then ask the user:
  save it first, or pass `"force": true` only when the user agreed to discard the changes.
- `ui.*` commands are blocked in Septet. Commands that
  open a dialog when their parameters are missing (`file.exportPdf {}`, `format.fontDialog`, `para.dialog`,
  `edit.findReplace`) fail and name the parameters to pass instead.

## The caret model (read this)

Commands work **at the caret or on the selection**, like keystrokes. Typing replaces the selection.
- `text.insert {text}` types at the caret. In `text`, **`\r` starts a new paragraph** and `\n` is a line break
  inside the paragraph. `text.newParagraph {}` is Enter. After a Title or Heading paragraph, Enter gives a Normal one.
- Multi-character `text.insert` skips AutoCorrect and smart quotes, so type “curly” quotes and – dashes yourself.
- Move: `caret.docStart`, `caret.docEnd`, `caret.left/right/up/down`, `caret.home/end`, `caret.wordLeft/wordRight`,
  `caret.paraUp/paraDown`. All of them take `{extend?: true}` to grow the selection.
- Select: `select.text {text, occurrence?}` (1-based, case-sensitive, within one paragraph, in the caret's story),
  `select.paragraph {}`, `select.word {}`, `select.all {}`, `select.collapse {}`, and
  `select.range {anchor: Pos, focus: Pos}` / `caret.set {pos: Pos}`.
- **Pos** is `{"story": "body", "path": [block], "off": n}` or just `{"block": n, "off": n}` in the body. A table
  cell's paragraph is `[table block, row, cell, paragraph]`, all 0-based. A header or footer is `"story": {"part": id}`.
  **`off` and `len` count UTF-8 bytes**, not characters: "é" is 2 and "—" is 3.
- Formatting with a bare caret inside a word formats the whole word. Between words or at the end, it sets the
  formatting for the next text you type: `format.bold {value: true}` → `text.insert` → `format.bold {value: false}`.

## Work loop

1. **Inspect**: `app_inspect {app: "wordcraft", what}`:
   - `document`: `blocks[{index, type, style, len, text, list, props, runs[{start, end, props}], objects}]`,
     with tables as `{index, type: "table", rows, cols, style, cells}`. It also has `parts` (headers, footers,
     comments), `selection`, `pages`, `words`, `sections`, `trackChanges`, `canUndo`. Pass `params: {text: false}`
     to leave the text out of a long document.
   - `text` (`params: {story}`), `paragraph` (`params: {path: [n]}`: its props plus the props resolved through its
     styles), `selection` (text and formatting), `pages` (count and `sizes_pt`), `history` (the undo list).
2. **Execute** one command at a time. Read the error, because it names the wrong or missing parameter.
3. **Look**: `app_render {app: "wordcraft", target: "page", page: n}` (1-based; the caption gives "Page n of N,
   w × h pt"). Render every page you changed. Text that spills onto an extra page shows up in `pages`.
4. **Fix**: `app_undo {app: "wordcraft", steps?}`. `edit.redo {}` redoes.

## Commands by task (all lengths in points: 72 pt = 1 in, 28.35 pt = 1 cm; colours `"RRGGBB"`)

| Task | Commands |
|---|---|
| Text | `text.insert {text}` · `text.newParagraph` · `text.lineBreak` · `text.pageBreak` (also `insert.pageBreak`) · `text.tab` · `text.insertTab` (a tab character, also in cells) · `text.backspace` · `text.delete` · `layout.break {kind: "page\|column\|nextPage\|continuous\|evenPage\|oddPage"}` |
| Character | `format.bold` / `format.italic` / `format.strikethrough {value?: bool}` (no value toggles) · `format.underline {value?, style?: "single\|double\|thick\|dotted\|…"}` · `format.font {name}` · `format.size {size}` · `format.color {color: "RRGGBB"\|"auto"}` · `format.highlight {color: "yellow\|brightGreen\|…\|none"}` · `format.superscript` · `format.subscript` · `format.allCaps` · `format.smallCaps` · `format.changeCase {mode?: "sentence\|lower\|upper\|title\|toggle"}` · `format.clear` · `format.set {props: CharProps}` |
| Paragraph | `para.align {value: "left\|center\|right\|justify"}` (or `para.alignCenter`…) · `para.spacing {before?, after?}` · `para.lineSpacing {value: 1.15}` \| `{atLeast: pt}` \| `{exactly: pt}` · `para.indents {left?, right?, firstLine?}` (negative = hanging) · `para.keepNext {value?}` · `para.pageBreakBefore {value?}` · `para.shading {color}` · `para.borders {kind: "bottom\|top\|all\|outside\|none…", width?, color?, style?}` · `para.tabs {set: [{pos, align?, leader?}]}` · `para.set {props: ParaProps}` |
| Styles | `para.style {style}` (name or id: "Title", "Subtitle", "Heading 1"/"Heading1"…"Heading 9", "Normal", "Quote", "Intense Quote", "No Spacing", "Caption") · `para.heading1/2/3` · `para.normal` · `styles.modify {style, chr?: CharProps, para?: ParaProps}` (restyles every paragraph in that style) · `styles.create {name, basedOn?, fromSelection?}` · `format.charStyle {style}` · `design.styleSet {name}` |
| Lists | `para.bullets {kind?: "bullet"\|one char, off?: true}` · `para.numbering {kind?: "numbered\|numberedParen\|upperLetter\|lowerLetter\|lowerRoman\|outline", off?}` · `para.listLevel {level: 0-8}` · `para.restartNumbering` · `list.multilevelPreset {preset: "headings\|outlineHeadings\|article\|chapter"}` (numbers the headings) |
| Tables | `insert.table {rows, cols, style?}` → `{table: [block]}`, caret in the first cell · `text.tab` moves to the next cell and selects its text (in the last cell it adds a row) · `table.style {style}` · `table.look {headerRow?, bandedRows?, firstColumn?, …}` · `table.insertRowBelow` / `insertRowAbove` / `insertColumnRight` / `insertColumnLeft` · `table.deleteRow` / `deleteColumn` / `deleteTable` · `table.merge` · `table.columnWidth {width}` · `table.autofit {mode: "contents\|window\|fixed"}` · `table.cellAlign {value: "topLeft\|…\|bottomRight"}` · `table.borders {kind, width?, color?}` · `table.shading {color}` · `table.fromText {separator?}` |
| Pictures, objects | `insert.picture {path, width?, alt?}` (inline at the caret, which moves after it) · `picture.size {width?, height?}` · `picture.altText {text}` · `insert.chart {type?, data?: {categories, series: [{name, values}]}, title?}` · `insert.shape {kind, width?, height?, fill?}` · `insert.textBox {text?, width?, height?}` · `insert.link {url, text?}` · `insert.symbol {char}` · `insert.equation {latex?}` |
| Page setup | `layout.size {name?: "A4\|Letter\|Legal\|…", width?, height?}` · `layout.orientation {value: "portrait\|landscape"}` · `layout.margins {preset?: "normal\|narrow\|moderate\|wide", top?, bottom?, left?, right?}` · `layout.columns {count, space?}` · `design.watermark {text?, remove?}` · `design.pageColor {color}` |
| Headers, footers | `insert.header {text?, preset?: "blank\|blankThree\|title"}` / `insert.footer {text?, preset?: "blank\|blankThree\|pageNumber"}` (here `\n` separates paragraphs) · `insert.pageNumber {position?: "top\|bottom\|current", align?: "left\|center\|right", format?: "x of y"}` · `insert.closeHeader` · `layout.differentFirstPage` · `layout.pageNumberFormat {format?, start?}` |
| Long documents | `references.toc {levels?, title?}` (`{}` updates it) · `insert.coverPage {title?, subtitle?, author?}` · `references.footnote {text?}` · `references.caption {label?, text?}` · `references.updateFields {all?}` |
| Find, replace | `edit.replaceAll {text, with, matchCase?, wholeWord?, wildcards?, regex?}` (in the caret's story; `^p` = paragraph mark, `^t` = tab) · `edit.find {text, …}` (selects the next match) · `edit.replace {text, with}` (replaces the selected match, then selects the next one) · `edit.goto {page?, paragraph?, bookmark?}` |
| Review | `review.trackChanges {value: true}` (without `value` it toggles) · `review.newComment {text}` (on the selection or the word at the caret) → `{id}` · `review.reply {id, text}` · `review.acceptAll` · `review.rejectAll` · `review.accept` / `review.reject` / `review.nextChange` · `review.wordCount` |
| Fonts | `font.list {all?: true}` (installed families) · `font.add {path}` (TTF/OTF/WOFF from the workspace, e.g. from `septet_fetch`; it stays in Wordcraft's added fonts) |
| Save, export | `file.saveAs {path: "out/letter.docx"}` (format from the extension: docx, odt, rtf, html, md, txt; use the exports for PDF and PNG) · `file.save {}` · `file.exportPdf {path, markup?}` · `file.exportPng {path, page?: 1-based, scale?: px per pt}` · `file.properties {title?, author?, subject?}` |

CharProps and ParaProps use the same field names as `document.paragraph` shows, e.g.
`{"font": "Georgia", "size": 18, "bold": true, "color": "1B1611"}` and
`{"spaceBefore": 12, "lineSpacing": {"rule": "multiple", "value": 1.15}, "keepNext": true}`. **Unknown field names are
silently ignored**, so check the result with `app_inspect paragraph`.

## Example: a one-page report with a list and a table, exported to PDF

Shorthand: `app_execute <command> <params>` means `app_execute {"app": "wordcraft", "command": …, "params": …}`.

```
app_execute file.new {"template": "blank"}
app_execute layout.size {"name": "A4"}
app_execute layout.margins {"top": 72, "bottom": 72, "left": 72, "right": 72}
app_execute text.insert {"text": "Quarterly Report"}
app_execute para.style {"style": "Title"}
app_execute text.newParagraph {}
app_execute text.insert {"text": "Summary"}
app_execute para.heading1 {}
app_execute text.newParagraph {}
app_execute text.insert {"text": "Sales grew 12% this quarter.\rCosts fell in every region."}
app_execute select.text {"text": "12%"}
app_execute format.bold {"value": true}
app_execute caret.docEnd {}
app_execute text.newParagraph {}
app_execute para.bullets {}
app_execute text.insert {"text": "North: +18%\rSouth: +9%\rWest: +4%"}
app_execute text.newParagraph {}
app_execute text.newParagraph {}
app_execute insert.table {"rows": 3, "cols": 3, "style": "GridTable4AccentBlue"}
app_execute text.insert {"text": "Region"}    then text.tab + text.insert for each further cell (no Tab after the last one)
app_execute caret.docEnd {}
app_execute insert.pageNumber {"position": "bottom", "align": "center"}
app_render  {"app": "wordcraft", "target": "page", "page": 1}
app_execute file.saveAs {"path": "out/report.docx"}
app_execute file.exportPdf {"path": "out/report.pdf"}
```

Pressing Enter twice ends the list, and the second Enter leaves an empty Normal paragraph. `insert.table` in an empty
paragraph puts the table **before** that paragraph, so `caret.docEnd` afterwards lands below the table.
The table styles are "Table Grid", "Plain Table 1", "Grid Table 1 Light", and "Grid Table 4 – Blue/Orange/Green/Purple"
and "List Table 3 – …" in the same colours. Their names contain an en dash, so pass ids such as `GridTable4AccentBlue`,
`ListTable3AccentGreen`, `GridTable1Light`, `PlainTable1` or `TableGrid`.

## Pitfalls

- **Replacing the document**: see Start; never pass `force` on your own. Write a new file (`out/letter-v2.docx`) rather than saving over the
  user's file. `file.save {}` without `path`, saves outside the workspace, and anything that prints
  (`file.printPdf`, `file.printMarkup`) ask the user first.
- **Headers and footers move the caret**: `insert.header`/`insert.footer` leave the caret in the header story.
  Run `insert.closeHeader {}` and then `caret.docEnd {}` before you type body text. `insert.pageNumber` with `top`
  or `bottom` **replaces** that header or footer's whole content, so put a footer text and a page number together
  with `insert.footer`, then `insert.editFooter {}`, `caret.docEnd`, and `insert.pageNumber {position: "current"}`.
- **Mixed numbering**: `app_render page`, `file.exportPng page` and `edit.goto page` count from 1. Pos paths,
  `insert.editHeader/editFooter {page}` and `table.draw` pages count from 0.
- `select.text` finds text only inside one paragraph. Text that runs across paragraphs needs `select.range`.
  `text.tab` at the start of a list item demotes the item one level, and in a table it moves to the next cell.
  Use `text.insertTab` for a real tab character.
- Fonts: the default styles ask for Aptos and Aptos Display, which are drawn with an open look-alike when they are not
  installed. An unknown `format.font` name is accepted but drawn with a fallback, so check `font.list` and render.
- `document.setText {text}` replaces the **whole body**. Use it only on your own new document.
- `review.trackChanges` without `value` toggles, which turns tracking **off** in a document saved with it on.
  `file.setAuthor {name}` renames the author of all later changes and comments, the user's too, so only change it when
  the user wants Claude's edits labelled.
- `septet_place` into an open document inserts at the caret: pictures as `insert.picture`, and docx/rtf/odt/md/txt/html
  as `insert.textFromFile`. Move the caret first.
