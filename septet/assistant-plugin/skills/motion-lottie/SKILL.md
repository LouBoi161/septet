---
name: motion-lottie
description: Create motion graphics for Effectcraft - hand-written Lottie JSON animations (logo reveals, animated icons, lower thirds, loaders, kinetic type), what Effectcraft's Lottie importer supports, and how to bring SVG/PNG/video assets into Effectcraft. Use when the user wants something animated, a Lottie file, an animated logo/title, or motion graphics.
---

# Motion graphics and Lottie

## How files reach Effectcraft (verified in its code)

- **Lottie is imported only through Effectcraft's menu File ▸ Import ▸ Lottie…** (`.json` or `.lottie`).
  `septet_open`/`septet_place` do *not* import Lottie: a `.json` lands as a *data file* (for data-driven
  animation) and a `.lottie` isn't recognised. So: write `anim.json`, tell the user the path and ask them to use
  File ▸ Import ▸ Lottie…. The import creates a new composition plus an "<name> Assets" folder and opens it.
- `septet_open`/`septet_place` *do* work for footage: PNG, JPEG, TIFF, WebP, PSD, SVG, EXR, video (mp4, mov, webm,
  …). PSD and PDF/AI/EPS can come in as layered compositions. With a composition open, `septet_place` adds them
  as layers or to the Project panel.
- Projects: `.ecproj` / `.ecprojx`. (`.effectcraft`, `.ectemplate` and `.aep` are listed for Effectcraft in Septet but
  don't open as projects; After Effects `.aep` is not supported.)
- Effectcraft renders to H.264/HEVC/AV1 MP4, ProRes MOV, WebM, GIF, PNG/JPEG/TIFF/EXR sequences, and exports
  Lottie (File ▸ Export ▸ Lottie JSON…). The user does the rendering.

## You can't preview Lottie yourself

`septet_render` only renders SVG. Before writing the Lottie, design the **key poses as SVG** (start, middle,
end) in the same coordinate system (same width/height), render them and get them right. Then translate the
shapes and values into Lottie. After the user imports it, ask what they see (or how many warnings the toast showed).

## Lottie essentials

- Units: `fr` frames per second, `ip`/`op` in/out frame, `w`/`h` pixels (required). Colours are `[r, g, b, a]`
  in 0–1. Scale and opacity in percent. Rotation in degrees. y points down.
- Animatable property: static `{"a": 0, "k": value}`; animated `{"a": 1, "k": [keyframes]}`.
  Keyframe: `{"t": frame, "s": [value], "o": {"x": [..], "y": [..]}, "i": {"x": [..], "y": [..]}}`; the last
  keyframe only needs `t` and `s`. `o` = ease out of this key, `i` = ease into the next. `"h": 1` = hold.
  No `o`/`i` = linear.
- Easing presets (`o` on the first key, `i` on the same key for the way into the next):
  smooth decelerate `o:{x:[0.33],y:[0]}, i:{x:[0.2],y:[1]}`; ease-in-out `o:{x:[0.42],y:[0]}, i:{x:[0.58],y:[1]}`;
  accelerate (exits) `o:{x:[0.7],y:[0]}, i:{x:[0.84],y:[0]}`. Overshoot = an extra keyframe past the target value.
- Layer transform `ks`: `a` anchor, `p` position, `s` scale, `r` rotation, `o` opacity (3-value arrays for `a`,
  `p`, `s`: `[x, y, 0]`). Shapes are drawn relative to the anchor; put the anchor at the shape's centre to scale
  or rotate around it.
- Shape layer (`ty: 4`) `shapes`: a group `{"ty":"gr","it":[ …shapes…, …fill/stroke…, {"ty":"tr",…} ]}`; the
  group's transform `tr` must be the **last** item.

Minimal working file (a dot that pops in, 3 s at 30 fps):

```json
{"v":"5.7.4","fr":30,"ip":0,"op":90,"w":512,"h":512,"nm":"Pop","ddd":0,"assets":[],
 "layers":[{"ddd":0,"ind":1,"ty":4,"nm":"Dot","sr":1,"ip":0,"op":90,"st":0,"bm":0,"ao":0,
  "ks":{"o":{"a":0,"k":100},"r":{"a":0,"k":0},"p":{"a":0,"k":[256,256,0]},"a":{"a":0,"k":[0,0,0]},
        "s":{"a":1,"k":[{"t":0,"s":[0,0,100],"o":{"x":[0.33],"y":[0]},"i":{"x":[0.2],"y":[1]}},
                         {"t":18,"s":[110,110,100],"o":{"x":[0.33],"y":[0]},"i":{"x":[0.4],"y":[1]}},
                         {"t":26,"s":[100,100,100]}]}},
  "shapes":[{"ty":"gr","nm":"Dot","it":[
     {"ty":"el","p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[200,200]}},
     {"ty":"fl","c":{"a":0,"k":[0.91,0.34,0.25,1]},"o":{"a":0,"k":100},"r":1},
     {"ty":"tr","p":{"a":0,"k":[0,0]},"a":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}]}]}]}
```

Paths (`"ty":"sh"`): `"ks":{"a":0,"k":{"c":true,"v":[[x,y],…],"i":[[dx,dy],…],"o":[[dx,dy],…]}}`: vertices plus
in/out tangents *relative to each vertex* (`[0,0]` = corner). Convert SVG cubic Béziers: for segment P0→P3 with
controls P1, P2: `o` of P0 = P1−P0, `i` of P3 = P2−P3. Morphing paths need the same vertex count in every keyframe.

Line drawing / logo reveal: stroke (`"ty":"st"`, `w` width, `lc` cap 1 butt/2 round, `lj` join) + trim paths
(`{"ty":"tm","s":{"a":0,"k":0},"e":{animated 0→100},"o":{"a":0,"k":0},"m":1}`).

Write the JSON with `Write`, then check it is valid JSON (e.g. `python3 -m json.tool anim.json > /dev/null`, if
python3 exists; Bash needs approval). Keep `nm` names meaningful: they become layer names in Effectcraft.

## What Effectcraft's Lottie importer supports

| Supported | Ignored (usually with a warning) |
|---|---|
| Layers: precomp (0), solid (1), image (2), null (3), shape (4), text (5) | camera (13), audio and other layer types |
| Parenting (`parent` → `ind`), 3D (`ddd`), stretch (`sr`), auto-orient (`ao`), all blend modes | layer styles (`sy`); skew (`sk`/`sa`) silently dropped |
| Track mattes (`tt` alpha/luma, inverted; `tp` or the layer above) | |
| Masks (`masksProperties`): add/subtract/intersect/lighten/darken/difference, opacity, expansion, feather, invert | |
| Shapes: gr, rc, el, sr (star/polygon), sh, fl, st, gf, gs, tm, rp, rd, op (offset), pb (pucker), tw, zz, mm | other shape items |
| Effects: Gaussian Blur, Tint, Tritone, Drop Shadow, Fill, expression controls | all other effects |
| Keyframes: bezier easing, hold, spatial tangents (`to`/`ti`), legacy `e` values | |
| Time remap (`tm`) on precomp layers only | |
| Expressions (`x`) are kept as expressions | (they may not evaluate like After Effects) |
| Text: source-text keyframes, animators with range selectors, fonts via `fonts.list` | glyph shapes (`chars`), text on a path (`t.p`) |
| Images: data URIs (png/jpg/webp/gif/svg; saved next to the JSON) or files relative to the JSON | |
| Markers on the main composition | |

dotLottie: only uncompressed (stored) zips, only the first animation, images inside the zip are not extracted.
**Prefer plain `.json`.**

Text layers use installed fonts by family/style; for brand-exact type, convert the text to shape paths
(`septet:typography`, `text_to_path.py`, then translate the SVG path into `sh` items) or ask the user to install
the font.

## Motion design tips

- Duration: UI micro-animations 150–400 ms; logo reveals 1.5–3 s; lower thirds in 0.5 s, hold, out in 0.4 s.
- Ease everything (linear only for constant motion like rotation loops). Entrances decelerate, exits accelerate.
- Stagger elements by 2–4 frames instead of moving everything at once. Overshoot ~5–10% sparingly.
- Loops: first and last keyframe identical, `op` = loop length.
- Animate few properties well (position + opacity + scale) rather than everything.
- For generative motion (particles, flow fields) see `septet:algorithmic-art`; for colour/type choices
  `septet:color` and `septet:typography`.

## Other routes

- Static layered artwork for the user to animate: make an SVG (or a set of PNGs) with one element per file and
  `septet_open`/`septet_place` them into Effectcraft.
- Animated titles for Filmcraft: build in Effectcraft, render a movie, then
  bring it into Filmcraft (`septet:video-editing`).
