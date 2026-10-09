---
name: typography
description: Choose, download and use fonts in Septet work - Google Fonts via septet_fetch into fonts/, font pairings, type scales, text in SVG, outlining text so it survives in Vectorcraft/Designcraft, and font licenses. Use whenever a design contains text, the user names a font, or a wordmark/headline/poster/layout needs type choices.
---

# Typography

## Where fonts come from

1. **Fonts already on the system**: fine for drafts, but check the family really exists (a missing family falls
   back silently in `septet_render`). On Linux `fc-list : family | sort -u` (Bash, needs approval) lists them.
2. **Google Fonts via `septet_fetch`** (preferred; all OFL/Apache, free for commercial use and embedding).
3. **Bundled with `septet:canvas-design`**: ~30 OFL fonts in that skill's `canvas-fonts/` folder (Outfit, Work Sans,
   Lora, Crimson Pro, IBM Plex, Instrument Sans/Serif, JetBrains Mono, Big Shoulders, …). Copying them into the
   workspace needs Bash (`cp`).

Never download fonts from other sites, and don't use commercial fonts the user hasn't provided.

## Downloading a Google Font

Fonts go into `fonts/` in the workspace. `septet_render` loads every font file under the workspace (TTF/OTF;
**not** WOFF/WOFF2).

**Route A, static TTF per weight (best for rendering):** the CSS API gives TTF links to Septet's downloader
(it sends a non-browser user agent; browsers get WOFF2).

```
septet_fetch url="https://fonts.googleapis.com/css2?family=Fraunces:wght@400;700&family=Inter:wght@400;600" path="fonts/fonts.css"
Read fonts/fonts.css          → one @font-face per weight, src: url(https://fonts.gstatic.com/…/xyz.ttf)
septet_fetch url="<that gstatic URL>" path="fonts/Fraunces-700.ttf"     (one call per weight)
```

Family names with spaces use `+` (`family=Playfair+Display:ital,wght@0,400;1,400`). If the CSS shows
`format('woff2')` instead of `truetype`, use route B.

**Route B, the source repo (gives the license file too):**

```
septet_fetch url="https://raw.githubusercontent.com/google/fonts/main/ofl/inter/OFL.txt" path="fonts/Inter-OFL.txt"
septet_fetch url="https://raw.githubusercontent.com/google/fonts/main/ofl/inter/Inter%5Bopsz,wght%5D.ttf" path="fonts/Inter-Variable.ttf"
```

The folder is the family name in lower case without spaces (`ofl/playfairdisplay/`, some are under `apache/` or
`ufl/`). File names are often **variable fonts** (`Family[wght].ttf`, brackets URL-encoded as `%5B`/`%5D`);
if you don't know the exact file name, download `…/<family>/METADATA.pb` first and Read it: it lists the files.
Variable fonts may only render their default weight in `septet_render`; prefer route A for specific weights, or
instance them with `text_to_path.py --axis wght=700`.

Always keep the license: fetch `OFL.txt` alongside the fonts when you deliver work that uses them.

## Making text survive in the apps

| Where | Which fonts it sees |
|---|---|
| `septet_render` | system fonts + all TTF/OTF in the workspace |
| Vectorcraft (SVG import) | installed system fonts and its bundled fonts only. `@font-face` is ignored. Only the first family in `font-family` counts. Unknown fonts fall back to Source Sans 3 |
| Designcraft | system fonts, its bundled fonts, and a **`Document Fonts`** folder next to the `.designcraft` file |
| Effectcraft / Photocraft | installed fonts |

So a font that is only in `fonts/` looks right in your preview but **not** in Vectorcraft. Options:

1. **Outline the text** (best for logos, wordmarks and headlines that won't be re-edited). If `python3` with fontTools
   is available (`python3 -c "import fontTools"`), run this skill's `text_to_path.py` (Bash, needs approval):
   ```
   python3 -I <skill-dir>/text_to_path.py fonts/Fraunces-700.ttf "Crumb" --size 120 --x 20 --y 140 --tracking 10 --fill "#3b2314"
   ```
   It prints a `<g>` of paths to paste into the SVG (or `--svg out.svg` writes a file). It applies advance widths,
   tracking and old-style `kern` tables only: check pairs like AV, To, Ty with `septet_render` and nudge glyphs by
   hand if needed. Keep the live-text version in a hidden layer or a second file so the wording can still be changed.
2. **Ask the user to install the font** (e.g. copy into `~/.local/share/fonts/` on Linux, double-click → Install on
   Windows/macOS). Never install it yourself without asking: it is outside the workspace.
3. Use a font that is already installed.

Tell the user which option you used.

## Text in SVG

```svg
<text x="400" y="300" text-anchor="middle" font-family="Fraunces" font-weight="700" font-size="96"
      letter-spacing="-1" fill="#1d1d1b">Crumb</text>
```

- `font-family` must match the family name inside the font file (the name in the Google CSS `font-family`).
  Name exactly one family. Use `font-weight`/`font-style` to choose the face.
- `y` is the baseline. `text-anchor="middle"` centres horizontally; for vertical centring, compute it from the
  cap height (~0.7 × font-size for most sans) rather than using `dominant-baseline` (support varies).
- Multi-line: one `<tspan x="…" dy="1.2em">` per line. SVG doesn't wrap text.
- Text inside a `clipPath` is ignored by Vectorcraft.

## Choosing type

- Pick fonts for the subject. Avoid the default trio Inter/Roboto/Arial unless the brief is purely neutral UI.
  `septet:frontend-design` and `septet:theme-factory` have more on aesthetic direction and ready-made theme pairs.
- **Pairing rules**: contrast in structure, harmony in proportion. One display face + one text face is enough; add
  a mono only for data/code. Same family in two weights is always safe.
- Good OFL pairs (all on Google Fonts):
  - Fraunces + Inter: warm editorial, food, craft brands
  - Playfair Display + Source Sans 3: classic, elegant
  - Space Grotesk + IBM Plex Sans: tech, product
  - DM Serif Display + DM Sans: modern magazine
  - Archivo Black + Archivo: bold posters, sport
  - Cormorant Garamond + Montserrat: luxury, weddings
  - Bricolage Grotesque + Instrument Serif: contemporary, playful-editorial
  - Syne + Manrope: art, events
- **Scale**: pick a ratio (1.25 for documents, 1.333–1.5 for posters) and derive sizes from the body size
  (e.g. 16 → 20 → 25 → 31 → 39 → 49). Fewer sizes look more designed.
- **Spacing**: body line-height 1.4–1.6, headings 1.0–1.2. Tighten large display type (letter-spacing −1% to −3%),
  track out small caps and all-caps labels (+5% to +12%). Measure 45–75 characters per line.
- **Logos/wordmarks**: try 3–4 typefaces side by side in one SVG and render it before deciding; adjust kerning by
  eye at the final size.
- Print: body text ≥ 9 pt, captions ≥ 7 pt. Screen: ≥ 16 px body. Check contrast (`septet:color`).

## Related

`septet:logo-and-icons`, `septet:svg-graphics`, `septet:print-and-pdf`, `septet:color`.
