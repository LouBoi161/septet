---
name: pdfcraft
description: Drive Pdfcraft (Septet's Acrobat-like PDF app) with the app_* tools - its commands are the 132 automation tools with JSON schemas (pages, text, comments, redaction, forms, bookmarks, links, images, watermarks, headers/footers, OCR, optimize, export, save), how to target documents, pages and rectangles, and the inspect-execute-render loop. Use when you edit, review, fill, redact, reorganize or export a PDF open in Pdfcraft, or turn images/text into a PDF there.
---

# Driving Pdfcraft

Load `septet:septet-apps` first for the general rules. `septet:print-and-pdf` covers print specs and making PDFs
with CLI tools. This skill is about changing PDFs **inside Pdfcraft** with `app_execute`.

In Septet, Pdfcraft's commands are its **automation tools** (the same ones `pdfcraft-cli mcp` serves), not menu
ids. `app_commands {app: "pdfcraft", filter: "page"}` lists them with a description and, for up to 12 results,
the JSON schema. Unknown parameters are rejected and missing required ones fail, so read the schema when unsure.

## 1. Start

- Open the user's PDF: `septet_open {paths: ["in.pdf"]}`. Images or a `.txt` with `app: "pdfcraft"` become a new PDF.
- Or make one with a tool: `doc_create {from: "blank", pages, width, height}`, `{from: "images", paths: [...]}`
  or `{from: "text", path | text}`; `doc_open {path}`; `doc_combine {paths: [...], pages?, out?}`.
  Every new document gets a tab and comes to the front. New documents are unsaved: `doc_save {path}`.
- Without any open PDF, tools that need a document fail with "No PDF is open in Pdfcraft".

## 2. Targeting

- **doc**: an integer id (`app_inspect what: "documents"`). Leave it out to work on the PDF in front. Only tools
  whose schema *requires* `doc` get it filled in. `app_inspect`/`app_render` take the doc id as `id`.
- **Pages are 1-based** everywhere: `page`, `pages: [1, 3]`, `at`, `to`, `from`/`to`, and `app_render page`.
  Without `page`, `app_inspect page` and `app_render` use the page in view.
- **Geometry**: PDF points (1/72 in), origin at the **top-left of the page as displayed** (after rotation), y down.
  `rect` is two corners `[x0, y0, x1, y1]`, not width/height. `at` and `point`s are `[x, y]`. `text_find`,
  `form_fields`, `link_list`, `comment_list`, `text_lines`, `page_images` return rects in the same system.
  Exceptions: `doc_compare` and `form_detect_fields` report rects with the origin **bottom-left**; `text_edit`'s
  `dy` moves **up** when positive; `page_set_box margins` and `content_update crop` are `[left, bottom, right, top]`.
- Page size: `app_inspect document` (page sizes) or the `app_render` caption ("… 595 × 842 pt"). Render with
  `max_side` = the longer side in points (64-1568) and one pixel is one point, so you can read positions off it.
- **Other ids**: comments by `id` (string, from `comment_list`) or `page` + `index`; form fields by `name`
  (`field` in edit tools); links, added content, text lines/paragraphs and images by `page` + 1-based number from
  their list tool; bookmarks by `path`, an array of 1-based positions (`[2, 1]` = first child of the second).

## 3. Work loop

1. **Inspect**: `app_inspect {app: "pdfcraft", what}` with `document` (metadata, pages, bookmarks, comments,
   fields, links…), `documents`, `page` (`params: {page: n}`: its text), `comments`, `fields`, `bookmarks`,
   `links`, `history` (dirty, undo/redo available). For more use read-only tools: `text_find`, `text_lines`,
   `text_paragraphs`, `page_images`, `content_list`.
2. **Execute**: `app_execute {app: "pdfcraft", command: "<tool>", params: {...}}`. Changing tools return the
   document summary (`doc`, `pages`, `dirty`, `undo`, `redo`).
3. **Look**: `app_render {app: "pdfcraft", target: "page", page: n}`. It is the only render target.
4. **Fix**: `app_undo {app: "pdfcraft"}` (`steps` up to 50) undoes in the PDF in front. For another document run
   `edit_undo` / `edit_redo` with `doc`. Many destructive edits are undoable only until saved.
5. **Save** when the user wants the file: `doc_save {path: "out/name.pdf"}` (a full rewrite to a new file).

## 4. Tools by task (names and params from the schemas)

| Task | Tool | Key params (required **bold**) |
|---|---|---|
| Rotate pages | `page_rotate` | **degrees** (multiple of 90, + = clockwise), pages, subset all/even/odd, orientation |
| Delete / move | `page_delete`, `page_move` | **pages**; move: **pages**, **to** (position before the move) |
| Insert | `page_insert_blank`, `page_insert_file` | **at**, width, height / **path**, **at**, pages (of the source) |
| Duplicate / replace | `page_duplicate`, `page_replace` | **pages** / **pages**, **path**, from_pages |
| Extract / split | `page_extract`, `doc_split` | **pages**, out, open, separate + out_dir, delete / **out_dir**, every, before, bookmarks, max_mb |
| Merge files | `doc_combine` | **paths** (≥2), pages (`"1-3, 6"` or null per file), out, open |
| Crop / boxes | `page_set_box` | pages, box crop/trim/bleed/art/media, margins or rect |
| Page labels | `page_number` | **from**, **to**, style decimal/lower-roman/…, prefix, start |
| Read / find text | `text_extract`, `text_find` | pages / **query**, limit |
| Edit existing text | `text_lines`, `text_paragraphs`, `text_edit` | **page**, line or paragraph, text, font, size, color, align, dx, dy, width |
| Add text / image | `page_add_text`, `page_add_image` | **page**, **text**, at + width or rect, font, size, bold, color, align / **page**, **path**, rect |
| Change added items | `content_list`, `content_update`, `content_delete` | **page**, **index**, rect, text, size, color, rotate, crop, image |
| Existing images | `page_images`, `image_edit`, `image_save` | **page**, **image**, **action** move/rotate/flip_horizontal/flip_vertical/replace/delete, rect, quarters, path |
| Comments | `comment_add` | **page**, **type** (note, highlight, underline, strikeout, squiggly, replace, rectangle, oval, line, arrow, ink, textbox, stamp, polygon, cloud, polyline, callout, caret, attachment), find, all, quads, rect, at, from, to, points, contents, stamp, color, fill, opacity, width, author |
| Review comments | `comment_list`, `comment_edit`, `comment_reply`, `comment_set_status`, `comment_delete` | id or page + index; contents/color/rect/move; **text**; **status** accepted/rejected/cancelled/completed |
| Comment summary | `comments_summarize` | sort page/author/date/type, out, open |
| Redact | `redact_mark`, `redact_apply`, `redact_clear` | page + rect, or find, or pattern phone/email/credit-card/ssn/date, or whole_pages; pages, overlay, fill |
| Sanitize | `doc_hidden_info`, `doc_remove_hidden` | categories (metadata, attachments, comments, …) |
| Fill forms | `form_fields`, `form_fill`, `form_reset` | **values** {name: string / bool / [strings]}; fields |
| No-field forms | `fill_sign_add` | **page**, **type** text/check/cross/dot/line/date/signature/initials, **at**, text |
| Build forms | `form_detect_fields`, `form_add_field`, `form_set_props`, `form_delete_field` | pages, add / **page**, **type**, **rect**, name, options, group / **field** + properties |
| Bookmarks | `bookmark_list`, `bookmark_add`, `bookmark_rename`, `bookmark_move`, `bookmark_set_page`, `bookmark_delete` | **title**, **page**, parent, position / **path** |
| Links | `link_list`, `link_add`, `link_edit`, `link_delete`, `links_from_urls`, `links_remove` | **page**, **rect**, url or to_page, visible, color / **page**, **index** |
| Marks | `doc_watermark`, `doc_header_footer`, `doc_background`, `doc_remove_marks` | text or file, opacity, rotation, behind, pages / header_left…footer_right with `<<1>>`, `<<n>>`, `<<Page 1 of n>>` tokens, font_size, margins / color or file / **kind** |
| Stamps | `stamp_custom` | **page**, **path** (image or PDF), **at** (centre), file_page |
| OCR | `ocr_status`, `ocr_recognize` | pages, dpi, language (en), skip_text_pages |
| Size / standards | `doc_reduce`, `doc_optimize`, `pdfa_verify`, `pdfa_convert`, `doc_flatten` | **path** (writes a copy) / level 2b/3b / comments, fields |
| Security | `doc_protect`, `doc_unprotect` | open_password, permissions_password, printing, changes, copy (applied on the next save) |
| Metadata | `doc_set_info`, `accessibility_fix` | **key**, **value** (Title, Author…) / **rule** primary-language/title/tab-order, value |
| Export | `doc_export_images`, `doc_export_text`, `doc_export_office`, `doc_export_all_images`, `doc_export_data` | **folder**, format png/jpeg/tiff, dpi, pages / **path** (.txt; .docx/.html/.rtf; .xfdf/.fdf/.csv) |
| Save / close | `doc_save`, `doc_close` | path, full / discard_changes |

## 5. Examples (`app_execute` arguments unless marked)

Highlight every "Total" on page 2 and add a note:
```json
{"app": "pdfcraft", "command": "comment_add", "params": {"page": 2, "type": "highlight", "find": "Total", "all": true, "color": "yellow"}}
{"app": "pdfcraft", "command": "comment_add", "params": {"page": 2, "type": "note", "at": [520, 60], "contents": "Check the sum.", "author": "Claude"}}
```

Redact e-mail addresses, check, apply, save a copy:
```
app_execute {"app": "pdfcraft", "command": "redact_mark", "params": {"pattern": "email"}}
app_render  {"app": "pdfcraft", "target": "page", "page": 1}
app_execute {"app": "pdfcraft", "command": "redact_apply", "params": {}}
app_execute {"app": "pdfcraft", "command": "doc_save", "params": {"path": "out/redacted.pdf"}}
```

Fill a form (names from `app_inspect what: "fields"`):
```json
{"app": "pdfcraft", "command": "form_fill", "params": {"values": {"Name": "Ada Lovelace", "Date": "2026-10-09", "Agree": true}}}
```

Reorganize: rotate page 3, move page 5 to the front, insert an appendix before page 4, number the pages:
```json
{"app": "pdfcraft", "command": "page_rotate", "params": {"pages": [3], "degrees": 90}}
{"app": "pdfcraft", "command": "page_move", "params": {"pages": [5], "to": 1}}
{"app": "pdfcraft", "command": "page_insert_file", "params": {"path": "appendix.pdf", "at": 4}}
{"app": "pdfcraft", "command": "doc_header_footer", "params": {"footer_center": "<<Page 1 of n>>", "font_size": 9}}
```

## 6. Pitfalls

- **Paths**: Septet makes folder keys (`out_dir`, `folder`, `dir`) workspace-relative always, and `path`, `paths`,
  `file`, `out` when they look like files (an extension) or the tool saves/exports. Other keys (`image` of
  `content_update`, `id` of `sign_document`) and extension-less folder names in `path`/`paths` (`action_run`,
  `ocr_recognize_files`) are **not** resolved: pass **absolute** paths inside the workspace (`septet_state` shows it)
  for those.
- **Approvals**: Septet asks the user before `doc_print`, every `sign_*` tool except `sign_list`, `doc_save`
  without `path` (saving in place over the user's file), and any file outside the workspace. Prefer saving to a
  new workspace path; only overwrite the user's file when asked. Don't touch `js_enabled` (an app preference).
- `redact_apply`, `doc_remove_hidden`, `doc_flatten` and `page_delete` remove content for good once saved. Render
  the marked pages and get the user's go-ahead for sensitive redactions before applying and saving.
- `page_add_text` adds page content; `comment_add type: "textbox"` adds a comment. Use `text_edit` (with numbers
  from `text_paragraphs`/`text_lines`) to change existing text; it falls back to Helvetica for missing glyphs.
- Rotation directions differ: `page_rotate` and `image_edit quarters` are clockwise; `content_update rotate` and
  `doc_watermark rotation` are counter-clockwise.
- `doc_reduce`, `doc_optimize`, `doc_export_*`, `page_extract` with `out` write files and leave the open document
  unchanged. `page_extract` and `comments_summarize` without `out` open a new unsaved tab instead.
- OCR needs installed models: run `ocr_status` first, and tell the user if it is unavailable.
- `doc_protect`/`doc_unprotect` only take effect on the next `doc_save`. Never echo passwords back.
- `app_render` draws one page as it is now (unsaved edits included); it has no object or selection targets.
  Large `text_extract`/`comment_list` replies are cut at 40,000 characters: pass `pages` or `page`.
