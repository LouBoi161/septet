---
name: svg-graphics
description: Write clean, editable SVG by hand for illustrations, badges, diagrams, patterns, posters and UI graphics, check them with septet_render, and hand them to Vectorcraft. Use whenever you are about to write or change an .svg file, or need to know which SVG features survive Vectorcraft's importer (filters, masks, text, gradients).
---

# SVG graphics for Septet

SVG is the main way you draw in Septet. You write the file, look at it with `septet_render`, fix it, then
`septet_open` it in Vectorcraft (or `septet_place` it into another app's document). See `septet:septet-apps` for the
general workflow.

## Document skeleton

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="800" height="600" viewBox="0 0 800 600">
  <title>Short description</title>
  <defs>
    <!-- gradients, clipPaths, symbols -->
  </defs>
  <g id="background">…</g>
  <g id="artwork">…</g>
  <g id="text">…</g>
</svg>
```

- Always set `viewBox` **and** `width`/`height`. Vectorcraft treats 1 px as 1 pt; absolute units (`mm`, `in`) on
  the root keep their physical size, so for print artwork write `width="210mm" height="297mm" viewBox="0 0 210 297"`.
- **Top-level `<g id="…">` become layers in Vectorcraft.** Name them for the user (`data-name="Background"` or
  `inkscape:label` also works). `display="none"` comes in as a hidden layer.
- Keep coordinates on a sensible grid, round to 1–2 decimals. No `transform` soup: prefer plain coordinates;
  transforms are baked into the geometry on import anyway.
- Use `<symbol>` + `<use>` for repeated elements; Vectorcraft keeps them as symbols with instances (unless the
  `<use>` inherits paint, is clipped by its viewport, or has a filter: then it becomes plain art).
- One idea per path. Use `fill-rule="evenodd"` for holes, or draw the hole as a reversed subpath.

## What Vectorcraft keeps on import

Verified in Vectorcraft's importer (usvg-based, with its own text import):

| Feature | Result in Vectorcraft |
|---|---|
| paths, rect/circle/ellipse/line/polyline/polygon | editable paths (multi-subpath → compound path) |
| linear/radial gradients, `gradientTransform`, `href` inheritance | kept. Focal radius `fr` ignored; `spreadMethod` reflect/repeat expanded into stops |
| patterns | pattern swatch |
| clipPath (nested too) | clip group |
| mask | luminance opacity mask (alpha masks approximated; nested masks ignored) |
| opacity, fill/stroke-opacity, all `mix-blend-mode`s, `isolation` | kept |
| stroke width/cap/join/miterlimit/dasharray/dashoffset, `paint-order`, `vector-effect: non-scaling-stroke` | kept |
| markers | expanded into plain paths (no longer "arrowheads") |
| filters | **only** Gaussian blur, drop shadow (`feDropShadow` or the usual offset+blur+flood+composite+merge chain), outer glow, inner glow and feather become live effects. **Everything else is dropped** (feTurbulence, feMorphology, lighting, displacement, colour matrices on their own…) |
| `<image>` | PNG/JPEG/GIF/WebP data URIs embedded; relative file links stay linked (keep the file!) |
| `<text>`, `tspan`, `textPath`, letter/word-spacing, vertical writing-mode | live text. Only the **first** `font-family` is used; it must be a font installed on the system or bundled with the app. `@font-face` is **not** loaded |
| `<style>` CSS | works for shapes; for text only simple selectors (type, `.class`, `#id`, attribute, descendant/child) |
| `foreignObject` | dropped |

Consequences:
- Effects you can't express as the five filters above (grain, noise, textures) → bake them as shapes, or make a
  raster in Photocraft instead (`septet:image-editing`).
- `septet_render` (resvg) *does* render all standard filters, so the preview can look richer than the Vectorcraft
  import. If the user will edit in Vectorcraft, stick to the supported set.
- Fonts: `septet_render` sees fonts in the workspace, Vectorcraft only sees installed fonts. See
  `septet:typography` for what to do about that (outline the text, or ask the user to install the font).

## Verify loop

1. Write the SVG.
2. `septet_render` it (`size` 512–1024 is enough for checking; use 2048 for fine detail). Look at it critically:
   alignment, spacing, clipping at the edges, contrast, text actually rendered (missing fonts fall back silently).
3. Fix and render again. Two or three rounds are normal.
4. `septet_open` in Vectorcraft (or `septet_place` into an open document), then `septet_state` to confirm.

For a tiny-size check (icons, favicons), also render at `size: 64`.

## Craft tips

- **Geometry first**: build from circles, rounded rects and a few Bézier curves. Use arcs (`A`) for exact round
  shapes, cubic curves (`C`/`S`) for organic ones. Close shapes with `Z`.
- **Optical balance**: circles and pointed shapes need to be ~3–5% larger than squares to look the same size;
  center visually, not mathematically (triangles sit slightly right/low).
- **Stroke vs fill**: for artwork that will be scaled, prefer fills over strokes (strokes do not scale with
  `vector-effect: non-scaling-stroke`). For line icons, consistent `stroke-width`, `stroke-linecap="round"`,
  `stroke-linejoin="round"`.
- **Colours**: define a palette first (`septet:color`), use at most 3–5 colours plus neutrals. Put colours on
  `fill`/`stroke` attributes rather than CSS classes for maximum portability.
- **Gradients**: subtle (two close hues) looks premium; rainbow looks cheap. Use `gradientUnits="userSpaceOnUse"`
  when several shapes should share one gradient.
- **Patterns/generative**: when an SVG would need hundreds of elements, generate it with a script (Python or
  Node, if available: `command -v python3 node`) that writes the SVG, rather than typing it out. Running a script
  needs the user's approval via Bash. For true generative art, see `septet:algorithmic-art`.
- **Diagrams/charts**: align to a grid, label directly instead of legends, one accent colour for emphasis.

## Exports

You can't run Vectorcraft's export yourself; the user exports from Vectorcraft (SVG, PDF, PNG, JPG, WebP, TIFF,
EPS, AI, DXF, EMF…). If you need a PNG yourself, `septet_render` with `out: "name.png"` (max 2048 px). For PDF
from SVG see `septet:print-and-pdf`.

## Related skills

- `septet:logo-and-icons`: logos, marks, icon sets, Iconify downloads.
- `septet:typography`: fonts, text in SVG, outlining text.
- `septet:color`: palettes and contrast.
- `septet:canvas-design`: posters/art pieces with a design philosophy (bundled Anthropic skill).
- `septet:motion-lottie`: animating artwork in Effectcraft.
