#!/usr/bin/env python3
"""Turn a line of text into SVG outlines (a <g> of <path>s) with fontTools.

Usage:
  python3 -I text_to_path.py FONT.ttf "Text" [--size 96] [--x 0] [--y 0]
         [--tracking 0] [--axis wght=700 ...] [--fill "#111"] [--svg out.svg]

Prints a <g> element (or writes a whole SVG with --svg). `--y` is the baseline.
`--tracking` is in 1/1000 em (like letter-spacing in design apps). `--axis`
picks an instance of a variable font. Uses the font's pair kerning only when it
has an old-style `kern` table (GPOS kerning and ligatures are not applied), so
check the result with septet_render and adjust pairs by hand if needed.
Needs: fontTools (`python3 -c "import fontTools"`).
"""

import argparse
import html
import sys

from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("font")
    ap.add_argument("text")
    ap.add_argument("--size", type=float, default=96)
    ap.add_argument("--x", type=float, default=0)
    ap.add_argument("--y", type=float, default=0)
    ap.add_argument("--tracking", type=float, default=0)
    ap.add_argument("--axis", action="append", default=[])
    ap.add_argument("--fill", default="#000")
    ap.add_argument("--svg")
    a = ap.parse_args()

    font = TTFont(a.font)
    if a.axis and "fvar" in font:
        from fontTools.varLib import instancer

        loc = {k: float(v) for k, v in (s.split("=", 1) for s in a.axis)}
        font = instancer.instantiateVariableFont(font, loc)

    upm = font["head"].unitsPerEm
    scale = a.size / upm
    cmap = font.getBestCmap()
    glyphs = font.getGlyphSet()
    hmtx = font["hmtx"]
    kern = {}
    if "kern" in font:
        for t in font["kern"].kernTables:
            kern.update(getattr(t, "kernTable", {}))

    parts = []
    pen_x = 0.0
    prev = None
    for ch in a.text:
        name = cmap.get(ord(ch))
        if name is None:
            print(f"warning: {ch!r} is not in the font", file=sys.stderr)
            continue
        if prev is not None:
            pen_x += kern.get((prev, name), 0)
        pen = SVGPathPen(glyphs)
        # font units, y up → SVG user units, y down, baseline at a.y
        tp = TransformPen(pen, (scale, 0, 0, -scale, a.x + pen_x * scale, a.y))
        glyphs[name].draw(tp)
        d = pen.getCommands()
        if d:
            parts.append(f'<path d="{d}"/>')
        pen_x += hmtx[name][0] + a.tracking * upm / 1000
        prev = name

    width = pen_x * scale
    group = f'<g fill="{html.escape(a.fill)}" data-name="{html.escape(a.text)}">' + "".join(parts) + "</g>"
    if a.svg:
        asc = font["hhea"].ascent * scale
        desc = -font["hhea"].descent * scale
        w, h = a.x + width + a.x, a.y + desc
        with open(a.svg, "w", encoding="utf-8") as f:
            f.write(
                f'<svg xmlns="http://www.w3.org/2000/svg" width="{w:.1f}" height="{h:.1f}" '
                f'viewBox="0 0 {w:.1f} {h:.1f}">{group}</svg>\n'
            )
        print(f"wrote {a.svg} (text width {width:.1f}, ascent {asc:.1f}, descent {desc:.1f})")
    else:
        print(group)
        print(f"<!-- text width {width:.1f} -->")
    return 0


if __name__ == "__main__":
    sys.exit(main())
