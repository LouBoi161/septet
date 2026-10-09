---
name: septet-apps
description: The hub for working inside Septet - which of the seven apps (Photocraft, Vectorcraft, Lightcraft, Designcraft, Pdfcraft, Filmcraft, Effectcraft) opens which files, the workspace rules, septet_open vs septet_place, the render-and-look verify loop, checking for CLI tools, and which septet skill to load next. Use at the start of any creative task in Septet, or when unsure which app or file format to use.
---

# Working in Septet

You run inside Septet, next to the user's apps. You create files in the **workspace** (your working directory) and
show them in the apps with the `septet_*` tools. The user watches the apps change, so keep chat replies short.

## The apps (verified in their code)

| App | Opens as a document | `septet_place` into an open document |
|---|---|---|
| **Vectorcraft** (vector) | SVG/SVGZ, PDF, AI, EPS, EMF/WMF, DXF (ASCII), PNG/JPEG/TIFF…, `.vectorcraft` | raster (linked), SVG/EPS/DXF/EMF → a group, PDF/AI page → a clipped group |
| **Photocraft** (raster) | PNG, JPEG, WebP, GIF, BMP, TGA, ICO, TIFF, PSD/PSB, EXR, HDR, HEIF, camera raw, `.pcraft`. **Not SVG or PDF** | raster/PSD → smart object layer |
| **Lightcraft** (raw library) | imports raw files, images and folders into its library; presets (XMP, .lrtemplate), `.cube` LUTs | same as open (import) |
| **Designcraft** (layout) | only `.designcraft` and `.idml` | images, SVG, PDF (first page), EPS (preview), text (`.txt .md .docx .rtf`), `.xlsx` as a table |
| **Pdfcraft** (PDF) | PDF; images and `.txt` become a new PDF | PDF → pages inserted after the current page; image → onto the current page |
| **Filmcraft** (video) | `.fcproj` projects; media → imported into the bin (not onto the timeline); `.srt/.vtt` → caption track; `.otio/.fcpxml/.xml/.edl/.aaf/.omf` → sequences merged into the project | same as open (bin) |
| **Effectcraft** (motion) | `.ecproj` projects; images, SVG, PSD, PDF/AI, video → imported into the project | footage → layers / Project panel. **Lottie only via its menu File ▸ Import ▸ Lottie…** |

Without `app`, `septet_open` picks the app by extension (the first app in the table that lists it in Septet's
list, which includes a few extensions an app then fails to open, e.g. `.indd`, `.epub`, `.dcraft` for
Designcraft; `.aep`, `.lottie` for Effectcraft; `.cdr` for Vectorcraft; AVIF/JPEG XL for Photocraft). Pass `app` to choose another app:
e.g. a PNG with `app: "pdfcraft"` makes a PDF; a PDF with `app: "vectorcraft"` opens it as artwork.

## Tools

- `septet_state`: what is open (windows, tabs, each app's document, unsaved changes) and the workspace path.
  Call it before `septet_place` and after open/place to confirm (apps update on the next frame).
- `septet_open {paths, app?}`: open files as documents; brings the app's tab to the front.
- `septet_place {paths, app}`: put files into the document already open in that app (like File ▸ Place). If the app
  has no document, most apps open the file instead. Use it to add your artwork to the user's work.
- `septet_activate {app}`: bring an app to the front (starts it).
- `septet_render {path, size?, out?}`: render an SVG from the workspace to PNG and **see** it. Uses system fonts
  and every TTF/OTF in the workspace. `out` also saves the PNG (max 2048 px).
- `septet_fetch {url, path}`: download into the workspace from Iconify (`api.iconify.design`) or Google Fonts
  (`fonts.googleapis.com`, `fonts.gstatic.com`, `raw.githubusercontent.com/google/fonts/`) only. No redirects,
  25 MB max. For anything else use WebFetch (needs approval).
- Built-ins: Read/Write/Edit/Glob/Grep work freely in the workspace. `Read` shows you images (PNG/JPEG/WebP/GIF)
  and PDFs. Bash, WebSearch and WebFetch ask the user each time, so use them only when they clearly help.

## Workspace rules

- Write everything into the workspace; use subfolders (`fonts/`, `icons/`, `out/`, `media/`).
- `septet_open`/`septet_place` accept workspace files and files the user opened in Septet. Other files must be copied
  into the workspace first (Bash `cp`, needs approval) or opened by the user.
- **Never overwrite or delete the user's files.** Write new versions (`poster-v2.svg`) instead of replacing
  earlier ones the user may be looking at.
- Vectorcraft and Designcraft *link* placed images: don't move or delete placed files afterwards.
- The user's apps hold unsaved work: never ask an app to close or discard anything. The user saves and exports.

## The verify loop (always)

1. Make the file (SVG, PDF, PNG, timeline, JSON…).
2. Look at it yourself: SVG → `septet_render`; PNG/JPEG/PDF → `Read`; video → extract a frame (`septet:video-editing`).
3. Critique honestly (alignment, spacing, contrast, text rendered with the right font, nothing cut off) and fix.
   Two or three rounds are normal for anything visual.
4. Then open/place it, check `septet_state`, and tell the user in one or two sentences what you made and where.

Never claim a result looks good without having looked at it.

## CLI tools: check, don't assume

Useful tools may or may not be installed: `ffmpeg`/`ffprobe`, `magick` (ImageMagick 7; `convert` on 6),
`rsvg-convert`, `qpdf`, `typst`, `weasyprint`, `pdftoppm`, `python3` (+ Pillow, fontTools), `node`.
- Check first in one command: `command -v ffmpeg magick rsvg-convert qpdf typst weasyprint python3` (Windows:
  `where`). Remember the answer for the conversation.
- If a tool is missing, use the app route (open in Septet and explain the menu steps) or another tool. Say what
  would have been easier if the user wants to install something.
- **Never install software (pip, npm, package managers) without asking the user first.**
- Run scripts you write from the workspace with `python3 -I script.py …`.

## Which skill next

| Task | Skill |
|---|---|
| Any SVG drawing, illustration, diagram | `septet:svg-graphics` |
| Logo, brand mark, app icon, icons from Iconify | `septet:logo-and-icons` |
| Fonts, text, wordmarks, Google Fonts | `septet:typography` |
| Palettes, contrast, print colour | `septet:color` |
| Print, PDF, multi-page documents, merging PDFs | `septet:print-and-pdf` |
| Photos and raster images | `septet:image-editing` |
| Video cuts, subtitles, ffmpeg | `septet:video-editing` |
| Animation, Lottie, Effectcraft | `septet:motion-lottie` |
| Poster/art piece with a design philosophy | `septet:canvas-design` |
| Generative/algorithmic art (p5.js) | `septet:algorithmic-art` |
| Apply a ready-made colour + font theme | `septet:theme-factory` |
| Distinctive visual direction for HTML/UI-like designs | `septet:frontend-design` |

## Style of work

- Prefer generating content as code (SVG, OTIO, Lottie JSON, typst/HTML, scripts) and opening it in the right app.
- Ask at most one or two questions when the brief is truly ambiguous (format, text content, brand colours);
  otherwise make sensible choices, show the result, and offer variations.
- Keep files editable: named layers/groups, live text kept alongside outlined text, sources next to exports.
