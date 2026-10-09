---
name: color
description: Build colour palettes, check contrast (WCAG), pick colours for screen vs print (RGB/CMYK), dark/light variants and data-visualisation colours for Septet work. Use when choosing or fixing colours for a logo, poster, layout, UI graphic, chart, video title or animation, or when the user asks whether colours work together or are readable.
---

# Colour

## Building a palette

1. **Start from the subject**, not from a colour wheel: what materials, places, eras, moods belong to it?
   (bakery → crust brown, flour cream, oven red; marine → deep navy, kelp, foam). Name the colours.
2. **Structure** (the 60-30-10 rule as a default):
   - 1 dominant neutral (background/paper): light or dark, slightly tinted towards the brand hue rather than pure
     `#fff`/`#000`
   - 1 primary brand colour
   - 1 accent for emphasis/calls to action (often complementary or split-complementary)
   - 2–3 neutrals for text and lines (tinted greys)
3. **Tints and shades**: derive lighter/darker steps of the primary by changing lightness in a perceptual space
   (OKLCH: keep hue and roughly chroma, vary L in steps of ~0.08). Avoid mixing with pure grey (looks dull).
4. Write the palette down once (a `palette.svg` swatch sheet with hex codes and names, rendered with
   `septet_render`) and reuse the exact hex values everywhere.

For ready-made font + colour themes, use `septet:theme-factory`. For chart colours, the built-in `dataviz` skill
(if available) has a validated palette method.

## Harmony shortcuts

- Analogous (hues within ~30°): calm, natural. Add one contrasting accent.
- Complementary (opposite hues): energetic; let one dominate, use the other sparingly.
- Monochrome + one accent: elegant, safe for brands.
- Muted/desaturated palettes look more premium than fully saturated ones; save full saturation for the accent.
- Same lightness, different hue = vibrates and is unreadable as text/background. Vary lightness.

## Contrast (accessibility)

WCAG 2 contrast ratio = (L1 + 0.05) / (L2 + 0.05) with relative luminance
L = 0.2126 R + 0.7152 G + 0.0722 B, where each channel c (0–1) is linearised:
c ≤ 0.04045 ? c/12.92 : ((c + 0.055)/1.055)^2.4.

| Use | Minimum |
|---|---|
| Body text | 4.5 : 1 (AAA: 7 : 1) |
| Large text (≥ 24 px, or ≥ 18.66 px bold) | 3 : 1 |
| Icons, chart lines, UI borders | 3 : 1 |

Calculate rather than guess. If `python3` is available, a one-liner is fine (Bash, needs approval); otherwise
compute by hand for the 2–3 critical pairs. Also check:
- Colour is never the only carrier of meaning (add labels, patterns, icons).
- Red/green pairs: distinguish them by lightness too (≈8% of men are red-green colour-blind).
- Text over photos: add a scrim (gradient or semi-transparent band) and check the worst spot.

## Screen vs print

- Septet's SVG/PNG work is **sRGB**. Write colours as `#rrggbb`.
- For print, very saturated RGB colours (bright blue, neon green, vivid orange) can't be reproduced in CMYK and
  will come out dull. Choose slightly muted versions from the start, and warn the user.
- Designcraft can export **PDF/X-4** with a built-in generic CMYK output intent, and keeps the ink values of CMYK
  TIFFs it places. For exact brand colours in print, the user needs to define CMYK/spot swatches in Designcraft;
  you can suggest values but can't guarantee them.
- Rich black for large areas in print: C60 M40 Y40 K100; body text in plain K100.
- Total ink coverage should stay under ~300% (uncoated paper: ~260%).

## Dark/light variants

- Don't just invert. On dark backgrounds, reduce saturation and raise lightness of brand colours a little, use
  off-white (`#f2f0ea`-ish) instead of pure white for text, and avoid pure black backgrounds (`#121212`-ish).
- Logos need a mono white version for dark backgrounds (`septet:logo-and-icons`).

## Video and motion

- Broadcast/video: keep text and graphics away from pure `#ffffff` and fully saturated reds for long on-screen
  times; mid-saturation looks better after compression.
- Lower thirds: a band behind the text with ≥ 4.5 : 1 contrast at the worst frame.

## Checking your result

Render with `septet_render` and look at the colours in context, not as isolated swatches. A quick greyscale test
reveals weak hierarchy: duplicate the SVG, wrap the art in a group with
`filter="url(#g)"` where `<filter id="g"><feColorMatrix type="saturate" values="0"/></filter>`, and render it
(preview only: Vectorcraft drops this filter on import).

## Related

`septet:typography`, `septet:svg-graphics`, `septet:print-and-pdf`, `septet:theme-factory`, `septet:frontend-design`.
