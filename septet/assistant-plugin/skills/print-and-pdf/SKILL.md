---
name: print-and-pdf
description: Make print-ready artwork and PDFs in Septet - page sizes, bleed, safe margins, resolution, multi-page documents (flyers, posters, business cards, menus, reports, invoices) generated with typst/weasyprint/rsvg-convert when installed, merging/splitting with qpdf, and handing results to Pdfcraft or Designcraft. Use when the user wants something printed, a PDF, a multi-page document, or wants to edit/combine existing PDFs.
---

# Print and PDF

## Formats (trim size)

| Format | mm | pt (1 pt = 1/72 in = 0.3528 mm) |
|---|---|---|
| A4 | 210 × 297 | 595.28 × 841.89 |
| A5 | 148 × 210 | 419.53 × 595.28 |
| A3 | 297 × 420 | 841.89 × 1190.55 |
| A6 postcard | 105 × 148 | 297.64 × 419.53 |
| US Letter | 215.9 × 279.4 | 612 × 792 |
| US Tabloid | 279.4 × 431.8 | 792 × 1224 |
| Business card EU / US | 85 × 55 / 88.9 × 50.8 | 240.94 × 155.91 / 252 × 144 |
| DL flyer | 99 × 210 | 280.63 × 595.28 |
| Poster A2 / A1 | 420 × 594 / 594 × 841 | |

Ask for the format if it isn't obvious; default to A4 in Europe, Letter in the US.

## Print rules

- **Bleed**: 3 mm on every side (US: 0.125 in = 3.175 mm). Background colours/images run to the bleed edge.
- **Safe zone**: keep text and logos ≥ 3–5 mm inside the trim (business cards: 4 mm, books: more on the gutter).
- **Resolution**: images ≥ 300 ppi at final size (posters seen from afar: 150 ppi is fine). A 2048 px PNG from
  `septet_render` covers only ~17 cm at 300 ppi, so keep artwork vector as long as possible.
- **Text**: ≥ 6 pt for fine print, 9–11 pt for body. Hairlines ≥ 0.25 pt.
- **Colour**: see `septet:color` (sRGB → CMYK shifts, rich black, ink limit).
- Fonts must be embedded in the PDF (all tools below do this) or outlined.

## SVG artwork with bleed

Make the SVG the size *including* bleed, in mm, and draw a guide layer you remove or hide before delivery:

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="216mm" height="303mm" viewBox="0 0 216 303">
  <g id="background"><rect width="216" height="303" fill="#f4efe6"/></g>
  <g id="content">…  <!-- trim box is x 3…213, y 3…300; safe area x 8…208, y 8…295 --></g>
  <g id="guides" display="none"><rect x="3" y="3" width="210" height="297" fill="none" stroke="#f0f" stroke-width="0.2"/></g>
</svg>
```

Check with `septet_render` (temporarily show the guides). Vectorcraft keeps the mm size and the layers.

## Which tool for which job

First check what exists, then choose. Run e.g.
`command -v typst weasyprint rsvg-convert qpdf magick pdftoppm python3` (Bash, needs approval). Never install
anything without asking the user; if nothing fits, use the in-app route.

| Job | Without CLI tools | With CLI tools |
|---|---|---|
| Single-page artwork (poster, flyer, card) | SVG → Vectorcraft → user exports PDF | `rsvg-convert -f pdf -o out.pdf in.svg` |
| Multi-page, text-heavy document (report, menu, invoice, CV) | SVG pages → Designcraft (see below) | `typst compile --font-path fonts doc.typ out.pdf` or `weasyprint doc.html out.pdf` |
| Merge / split / reorder / rotate PDFs | Pdfcraft (`septet_open`, then `septet_place` more PDFs: their pages are inserted) | `qpdf --empty --pages a.pdf b.pdf -- out.pdf`; `qpdf in.pdf --pages . 1-3 -- part.pdf`; `qpdf in.pdf out.pdf --rotate=+90:2` |
| Images → PDF | `septet_open` the image with `app: "pdfcraft"` (images become a new PDF) | `magick a.jpg b.jpg out.pdf` |
| Check a PDF | `Read` the PDF (you can read PDFs directly), or open it in Pdfcraft | `pdftoppm -png -r 72 -f 1 -l 1 in.pdf page` then Read the PNG |

Notes:
- **typst** is the best generator for structured documents (precise layout, good typography, fast). Set the page:
  `#set page(width: 216mm, height: 303mm, margin: 15mm)` (bleed included) or `#set page("a4", margin: 20mm)`.
  `--font-path fonts` makes downloaded fonts available (`septet:typography`).
- **weasyprint** renders HTML/CSS: use `@page { size: A4; margin: 20mm; }`, `@font-face` with `src: url(fonts/X.ttf)`
  (relative to the HTML file). For bleed: `@page { size: 216mm 303mm; bleed: 3mm; marks: crop }`.
  Good when the user wants HTML-style design; see `septet:frontend-design`.
- **rsvg-convert** uses installed fonts only (not `fonts/` in the workspace, no `@font-face`). Outline text first
  or use installed fonts.
- Always look at the result (Read the PDF) before telling the user it's done.

## Septet apps

- **Pdfcraft** opens PDFs (and turns images and `.txt` files into a new PDF). `septet_place` into an open PDF
  inserts a PDF's pages after the current page, or puts an image on the current page. The user can then do
  forms, signing, OCR, redaction, compression, PDF/A, passwords, page numbers, watermarks, headers/footers from its
  menus; tell them where if they ask.
- **Designcraft** (page layout) opens only `.designcraft` and `.idml` documents; you can't create a `.designcraft`
  file yourself. Workflow: ask the user to create a document (File > New, with the format, pages, margins and
  bleed you recommend), then `septet_place` your SVG/PDF/PNG artwork or text (`.txt`, `.md`, `.docx`, `.rtf`) into
  it. A placed PDF uses its first page by default. Fonts: Designcraft also loads a `Document Fonts` folder next to
  the `.designcraft` file. Designcraft exports PDF with bleed, crop marks and **PDF/X-4**, which is the right
  route for professional print.
- **Vectorcraft** opens and exports PDF too (single artwork, multiple artboards).

## Deliverables

For print jobs, give the user: the PDF (with bleed), a note of trim size and bleed, and the source (SVG/typst/HTML)
so it can be edited. For commercial printing, recommend exporting PDF/X-4 from Designcraft and checking the
printer's spec sheet (bleed, colour profile, file naming).

## Related

`septet:svg-graphics`, `septet:typography`, `septet:color`, `septet:canvas-design` (poster/art pieces as PDF).
