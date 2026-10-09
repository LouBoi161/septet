---
name: image-editing
description: Edit and generate raster images (photos, PNG/JPEG/WebP) in Septet - resize, crop, convert, batch process, compose, add text/watermarks with ImageMagick or Python/Pillow when installed, and hand results to Photocraft (layers, retouching) or Lightcraft (raw photos). Use when the user wants to change photos or bitmaps, make thumbnails/social images, convert formats, or bring vector art into Photocraft.
---

# Image editing

## Which app

- **Photocraft**: layered raster editing. Opens PNG, JPEG, WebP, GIF, BMP, TGA, ICO, PNM, QOI, TIFF (incl.
  layered), PSD/PSB (layers, masks, adjustment layers), EXR, HDR, HEIF/HEIC and camera raw (AVIF and JPEG XL are listed for Photocraft in Septet but are not decoded; convert them first), plus its
  own `.pcraft`. **It does not open or place SVG or PDF.** `septet_place` into an open document adds a raster
  file as a **smart object layer**, centred and fitted, in Free Transform.
- **Lightcraft**: raw photo library and development. `septet_open`/`septet_place` *import* files (or folders)
  into its library; it does not edit pixels destructively. Presets: XMP (Camera Raw), `.lrtemplate`, Luminar
  `.lmp`; `.cube` LUTs become creative profiles. For raw photos (`.dng`, `.cr3`, `.nef`, `.arw`…), send the user
  there.
- Vector art → **Vectorcraft** (`septet:svg-graphics`).

## Rules

- **Never overwrite the user's originals.** Write results to new files in the workspace (`out/…`). Files outside
  the workspace must be copied in before `septet_open`/`septet_place` can use them (or opened by the user).
- You can **look** at PNG/JPEG/WebP/GIF files with `Read`. Do it before and after every edit.
- Check tools first: `command -v magick convert identify python3` and `python3 -c "import PIL; print(PIL.__version__)"`
  (Bash, needs approval). `magick` is ImageMagick 7; on ImageMagick 6 use `convert`/`identify` instead. If
  nothing is available, do the work in Photocraft: open the image and tell the user the menu steps, or generate the
  graphic as SVG and convert it with `septet_render`.
- Never install software without asking.

## SVG → raster (for Photocraft)

- `septet_render path="art.svg" size=2048 out="art.png"`: longest side up to 2048 px, transparent background
  unless the SVG draws one.
- Bigger: `rsvg-convert -w 4000 art.svg -o art.png` or `magick -density 300 -background none art.svg art.png`
  (if available).
- Then `septet_place` the PNG into Photocraft (`app: "photocraft"`), or `septet_open` it as a new document.

## ImageMagick recipes (`magick`, IM7)

```
magick identify -format "%wx%h %[colorspace] %[size]\n" in.jpg          # inspect
magick in.jpg -resize 1600x1600\> -quality 85 out.jpg                    # fit within, only shrink
magick in.jpg -resize 1080x1080^ -gravity center -extent 1080x1080 sq.jpg # fill + crop to square
magick in.png -trim +repage out.png                                       # trim borders
magick in.jpg -auto-orient -strip out.jpg                                 # apply EXIF rotation, drop metadata
magick in.png -background white -alpha remove -alpha off out.jpg          # flatten transparency
magick in.jpg -colorspace Gray out.jpg
magick in.jpg -modulate 105,110,100 -unsharp 0x0.75+0.75+0.008 out.jpg   # a bit brighter/more saturated, sharpen
magick base.jpg logo.png -gravity southeast -geometry +40+40 -composite out.jpg   # watermark/overlay
magick in.jpg -quality 82 out.webp                                        # convert
magick a.png b.png c.png +append strip.png                                # side by side (-append: stacked)
magick montage *.jpg -tile 4x -geometry 300x300+8+8 contact.jpg           # contact sheet
magick mogrify -path out -resize 1200x1200\> *.jpg                       # batch (writes into out/)
```

For text on images, prefer making an SVG overlay (`septet:typography`) and compositing the rendered PNG: you get
real typographic control and can check it with `septet_render`.

## Pillow (Python) when there is no ImageMagick

Write a small script in the workspace (e.g. `tools/resize.py`) and run it with `python3 -I tools/resize.py`
(Bash, needs approval):

```python
from PIL import Image, ImageOps
im = ImageOps.exif_transpose(Image.open("in.jpg"))
im.thumbnail((1600, 1600))                      # in place, keeps aspect ratio
im.save("out/in-1600.jpg", quality=85, optimize=True)
# square crop: ImageOps.fit(im, (1080, 1080)); paste with alpha: base.paste(logo, (x, y), logo)
```

## Common sizes

| Use | Pixels |
|---|---|
| Instagram post / portrait / story | 1080 × 1080 / 1080 × 1350 / 1080 × 1920 |
| Open Graph / link preview | 1200 × 630 |
| YouTube thumbnail | 1280 × 720 |
| Website hero | 1920–2560 wide, JPEG/WebP quality 75–85 |
| Print | final size in mm ÷ 25.4 × 300 |

## Generated images

Backgrounds, patterns, gradients, textures: write them as SVG (or a script, see `septet:algorithmic-art`) and
render; for poster-like pieces see `septet:canvas-design`. You can't paint photo-realistic content; say so instead
of faking it.

## Handing over

Open the result with `septet_open` (Photocraft) or place it into the user's open document with `septet_place`.
Tell the user which files you created. Retouching (healing, masking, frequency separation…) is done by the user in
Photocraft; you can explain the steps.

## Related

`septet:svg-graphics`, `septet:color`, `septet:print-and-pdf`, `septet:video-editing` (stills from video).
