---
name: logo-and-icons
description: Design logos, brand marks, wordmarks, app icons, favicons and consistent icon sets as SVG, and find/download free icons from Iconify (with per-set license checks) via septet_fetch. Use when the user asks for a logo, brand mark, emblem, badge, app/website icon, pictogram, or "an icon of X".
---

# Logos and icons

Work as SVG (see `septet:svg-graphics` for SVG rules and the render loop, `septet:typography` for wordmarks,
`septet:color` for palettes). Finish in Vectorcraft with `septet_open`.

## Logo process

1. **Brief** (ask only what you can't infer): name, what the business does, audience, 2–3 adjectives,
   colours to use/avoid, where it will be used (signage, app icon, embroidery → simplicity matters).
2. **Concepts**: make 3–4 *different* directions in one SVG side by side (e.g. wordmark, monogram, pictorial mark,
   emblem/badge), each in a 300×300 cell with its name underneath. Render it and show it. Directions should differ in
   idea, not just colour.
3. **Refine** the chosen one: geometry, optical balance, kerning of the wordmark, spacing between mark and name.
4. **Deliver** a set of files in `logo/`:
   - `logo-primary.svg` (mark + name, horizontal), `logo-stacked.svg`, `logo-mark.svg` (symbol only)
   - `logo-mono-black.svg`, `logo-mono-white.svg` (white on a transparent background; render it on a dark
     background rectangle to check, then remove the rectangle)
   - PNG previews via `septet_render … out: "logo/logo-primary.png"` if useful.
   Then `septet_open` the primary file in Vectorcraft.

## What makes a mark work

- **One idea**. A bakery logo is not bread + wheat + chef hat + rolling pin. Merge two ideas at most (negative
  space tricks: a letter whose counter is a loaf).
- **Works at 16 px and in one colour.** Render at `size: 64` and look at it; if it turns into mush, simplify.
  Check the mono version before adding colour.
- **Geometric construction**: build from circles, squares and consistent radii; reuse the same corner radius
  and stroke weight everywhere. Snap to a grid (e.g. 24 or 48 units).
- **Wordmarks**: pick a typeface with character (not the default sans), then customise: tighten tracking, adjust one
  or two letterforms, cut a ligature. Outline the text in the final files (`septet:typography`) so Vectorcraft and
  other machines show the right font. Keep a live-text copy.
- **Clear space** around the mark ≈ the height of a key letter (e.g. the x-height or cap height).
- Avoid: gradients as the only distinguishing feature, thin hairlines, tiny text in emblems, clip-art look,
  stock "swoosh"/globe/lightbulb clichés, imitating existing brands' logos.

## App icons and favicons

- App icon: 1024×1024 artboard, the content inside a ~820 px safe area, background shape included (rounded
  square, radius ≈ 22% of the size, or as the platform demands). Bold silhouette, max. 2–3 colours, no text.
- Favicon: a dedicated simplified version on a 32×32 or 16×16 grid; render at `size: 64`.
- PNG sizes: `septet_render` with `size` 1024, 512, 256… and `out`. For `.ico`, if ImageMagick exists
  (`command -v magick`): `magick icon-16.png icon-32.png icon-48.png favicon.ico` (Bash, needs approval).

## Icon sets you draw yourself

Consistency over detail: same grid (24×24 viewBox), same stroke width (1.5 or 2), same `stroke-linecap`/
`linejoin` (round), same corner radius, 2 px padding inside the box. Put them as `<symbol id="icon-name">` in one
file for a sheet and render the sheet to compare them side by side.

## Iconify: ready-made icons (via `septet_fetch`)

Over 200 open icon sets, all as SVG. `septet_fetch` saves to a file; Read it afterwards.

**Search** (JSON; `limit` is at least 32):
```
septet_fetch url="https://api.iconify.design/search?query=bread&limit=32&prefixes=mdi,tabler,ph,lucide" path="icons/search-bread.json"
Read icons/search-bread.json
```
The result has `icons` (as `prefix:name`) and `collections`, with each set's `license` (`spdx`). Leave out
`prefixes` to search everything. Use English keywords.

**Download** one icon:
```
septet_fetch url="https://api.iconify.design/ph/bread-bold.svg" path="icons/bread.svg"
```
Query options: `color=%23b5651d` (URL-encoded `#`; replaces `currentColor`), `height=64` (sets the width/height;
the `viewBox` stays the set's own grid, e.g. 24 for `mdi`, 256 for `ph`), `rotate=90deg`, `flip=horizontal`. Without `color` the icon uses
`currentColor`, which renders black. To combine an icon with your art, Read the downloaded file and copy its
`<path>` data into your SVG (inside a `<g transform="translate(..) scale(..)">` or a `<symbol>` with the
icon's `viewBox`). Several icons at once: `https://api.iconify.design/<prefix>.json?icons=a,b,c` (JSON with
`body` strings).

**Licenses vary by set. Check before using.** The license is in the search result's `collections`, or here:
`https://api.iconify.design/collection?prefix=<prefix>&info=true` (look at `info.license`). All sets:
`https://api.iconify.design/collections`.

Prefer these (checked on the Iconify API, 2026-10):

| Set (prefix) | License | Style |
|---|---|---|
| Material Symbols (`material-symbols`), Material Design Icons (`mdi`) | Apache-2.0 | filled/outlined UI |
| Tabler (`tabler`), Phosphor (`ph`), Heroicons (`heroicons`), Iconoir (`iconoir`), Bootstrap (`bi`) | MIT | line/fill UI |
| Lucide (`lucide`) | ISC | line UI |
| Remix (`ri`), Carbon (`carbon`), MingCute (`mingcute`), IconPark (`icon-park-outline`) | Apache-2.0 | UI |
| Noto Emoji (`noto`), Fluent Emoji (`fluent-emoji`) | Apache-2.0 / MIT | colour emoji |
| Simple Icons (`simple-icons`), SVG Logos (`logos`) | CC0-1.0 | brand logos (trademarks still apply!) |
| Flags (`flag`, `circle-flags`) | MIT | flags |

Needs attribution: CC-BY sets (e.g. Font Awesome `fa6-*`, `solar`, `twemoji`, `game-icons`, `streamline`). Tell the user
the credit line (set name, author, license link) if you use them. **Avoid** for commercial work: CC-BY-SA
(`openmoji`, `entypo`, `typcn`, `arcticons`), GPL (`dashicons`, `icomoon-free`, `gridicons`), and non-commercial
sets (`cbi`, `ps`). When unsure, fetch the collection info and Read it rather than guessing.

**Icons are not logos.** A stock icon as a company's main mark can't be trademarked and looks generic. Use Iconify
icons for UI, infographics, menus, signage and as a *starting point* you redraw and make your own; brand logos
from `simple-icons`/`logos` only to refer to that brand (e.g. "find us on …"), never altered.

## Related

`septet:canvas-design` for poster-like brand visuals, `septet:theme-factory` for applying a palette/font theme to a
whole set of materials, `septet:motion-lottie` to animate a logo.
