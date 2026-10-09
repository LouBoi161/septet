---
name: photocraft
description: Drive Photocraft (Septet's layered raster editor) directly with the app_* tools - make or open a document, add pixel, shape, type, fill and adjustment layers, select, filter, transform, resize/crop, apply layer styles and masks, and save copies or export layers into the workspace. Load before calling app_commands/app_execute/app_inspect/app_render with app "photocraft".
---

# Driving Photocraft

Photocraft runs its **engine commands** for you (817 of them, ids like Photoshop's menus). They never open a
dialog: you pass the parameters instead. Each one is a normal undoable step the user sees. For file formats,
ImageMagick and when to use Photocraft at all, see `septet:image-editing`; for the workspace rules, `septet:septet-apps`.

## 1. Get a document

Every command except `file.new` needs an open document (otherwise: "No document is open in Photocraft").
- Existing image: copy it into the workspace if needed, then `septet_open {paths: ["photo.jpg"], app: "photocraft"}`.
  Photocraft opens raster formats, PSD/PSB, TIFF, raw, `.pcraft`; **not SVG or PDF** (render those to PNG first).
- New canvas: `app_execute` `file.new` `{"width":u32=1920,"height":u32=1080,"mode":"rgb|gray|cmyk|lab"="rgb","depth":8|16|32=8,"background":"white|black|backgroundColor|transparent|#rrggbb"="white","resolution":ppi=72,"name":str}`.
  `transparent` gives one empty pixel layer instead of a Background.
- Add a file as a layer: `septet_place` (leaves it in Free Transform for the user to commit with Enter), or
  `file.placeEmbedded` `{"path":str,"scale":%?,"fit":bool=true,"center":[x,y]?}`, which places it at once and returns `{layer, bounds}`.
- Several open documents: `app_inspect what="documents"`, switch with `document.activate {"document":index}`.
  Commands act on the active document.

## 2. Layer ids

- `app_inspect what="document"` gives size, mode, depth, `activeLayer`, `selectedLayers`, `hasSelection`,
  `selectionBounds`, the last 10 history steps and `layers`: a tree, **topmost first**, each with
  `id` (a number), `name`, `kind` (Pixel, Type, Shape, Fill, Adjustment, Group, Smart Object…), `visible`, `opacity` (0..1),
  `blend`, `clipped`, `hasMask`, `bounds` `[x, y, width, height]` (null for layers without pixels). `depth` (default 3) cuts groups.
- `what="layer"` with `id` (no id: the active layer); `what="selection"`: selected layers + pixel-selection bounds.
- Commands that create a layer return its id: `{"layer": 7}` (`layer.new.layer`, `shape.create`, `type.create`,
  `layer.newAdjustmentLayer.*`, `layer.newFillLayer.*`, `file.placeEmbedded`, `layer.stampVisible`). The new layer becomes active.
- Ids are numbers, valid only in this document while it stays open. Reopening a file gives new ids; flatten,
  merge and similar commands produce or remove layers. Re-inspect after those and never reuse old ids.

## 3. The work loop

1. `app_commands {app:"photocraft", filter:"blur"}` to find ids and the exact `params` text (filter words must all
   match id, label or menu; `enabled_only: true` shows only what can run now, else `disabled_reason` tells why).
2. `app_execute {app:"photocraft", command, params}`. Most commands with a `"layer":id?` param act on the active
   layer when it is left out. **Filters and `image.adjustments.*` have no layer param: they change the active
   pixel layer** (and the selection, if any), so `layer.select` it first. Long filters run as background jobs; the
   call waits for them.
3. `app_render {app:"photocraft", target:"document"}` to look at the composite; `target:"layer", id` shows one
   layer alone, trimmed to its content (over a checkerboard). Check every visible change.
4. Wrong? `app_undo {app:"photocraft", steps:n}` and try again. Prefer non-destructive work: adjustment and fill
   layers, shape and type layers, masks, layer styles, so the user can still edit it.

## 4. Commands (params as the registry gives them)

Colours are `"#rrggbb"`. Coordinates are document pixels from the top left.

| Task | Command | Params |
|---|---|---|
| New pixel layer / group | `layer.new.layer`, `layer.new.group` | `{"name":str?}` |
| Select layer(s) | `layer.select` | `{"layer":id,"mode":"replace\|toggle\|range\|add"="replace"}` |
| Layer properties | `layer.setProps` | `{"layer":id?,"name":str?,"visible":bool?,"opacity":0..1?,"fill":0..1?,"blend":"Multiply\|…"?,"clipped":bool?,"locked":bool?}` |
| Move | `layer.translate` | `{"layer":id?,"dx":i32,"dy":i32}` |
| Reorder | `layer.moveTo` | `{"layer":id?,"target":id,"position":"above\|below\|into"="above"}` |
| Duplicate / delete | `layer.duplicate`, `layer.delete` | `{"layer":id?}` (no layer: every selected layer) |
| Group / rename | `layer.groupLayers`, `layer.renameLayer` | `{"layer":id?,"name":str?}` / `{"layer":id?,"name":str}` |
| Merge / flatten | `layer.mergeDown`, `layer.stampVisible`, `layer.flattenImage` | `{"layer":id?}` / `{}` / `{}` |
| Clipping mask | `layer.createClippingMask` | `{"layer":id?}` (clips to the layer below) |
| Align to canvas | `layer.align.horizontalCenters`, `.verticalCenters`, `.leftEdges`, … | `{"to":"auto\|layers\|selection\|canvas"="auto"}` (acts on the selected layers) |
| Select rect / ellipse | `select.rect` | `{"x":i32,"y":i32,"width":u32,"height":u32,"mode":"replace\|add\|subtract\|intersect"="replace","ellipse":bool=false,"antiAlias":bool=true,"feather":px=0}` |
| Polygon selection | `select.lasso` | `{"points":[[x,y],…],"mode":…,"antiAlias":bool=true,"feather":px=0}` |
| Magic wand | `select.magicWand` | `{"x":px,"y":px,"tolerance":0..255=32,"contiguous":bool=true,"sampleAllLayers":bool=false,"mode":…}` |
| Subject / object | `select.subject`, `select.object` | `{"sampleAllLayers":bool=true,"mode":…}` / `{"rect":[x,y,w,h],"mode":…}` |
| All / none / invert | `select.all`, `select.deselect`, `select.inverse` | `{}` |
| Modify selection | `select.modify.feather`, `.expand`, `.contract` | `{"radius":…}` |
| Fill | `edit.fill` | `{"contents":"foreground\|background\|color\|contentAware\|pattern\|…"="color","color":"#rrggbb","opacity":0..100=100,"mode":"normal\|multiply\|…"}` (inside the selection, else the whole layer) |
| Delete pixels | `edit.clear` | `{}` (in the selection) |
| Stroke selection | `edit.stroke` | `{"width":1..250=1,"color":"#rrggbb","location":"inside\|center\|outside"="center","opacity":0..100=100}` |
| Gradient | `paint.gradient` | `{"from":[x,y],"to":[x,y],"style":"linear\|radial\|angle\|reflected\|diamond"="linear","colors":["#rrggbb",…]?,"opacity":1..100=100}` |
| Brush stroke | `paint.stroke` | `{"points":[[x,y],…],"size":px?,"hardness":0..1?,"opacity":0..1?,"color":"#rrggbb"?,"erase":bool?}` |
| Layer mask | `layer.layerMask.revealAll`, `.hideAll`, `.revealSelection`, `.apply`, `.delete` | `{"layer":id?}` |
| Remove background | `layer.removeBackground` | `{"layer":id?,"sampleAllLayers":bool=false,"refine":bool=true}` (adds a mask) |
| Adjustment layer | `layer.newAdjustmentLayer.brightnessContrast` | `{"brightness":-150..150=0,"contrast":-50..100=0}` |
| | `layer.newAdjustmentLayer.hueSaturation` | `{"hue":-180..180=0,"saturation":-100..100=0,"lightness":-100..100=0,"colorize":bool=false}` |
| | `layer.newAdjustmentLayer.curves` | `{"points":[[in,out],…]}` in 0..255, 2..19 points; per channel `red`/`green`/`blue` |
| | `layer.newAdjustmentLayer.levels` | `{"inBlack":0..253=0,"gamma":0.01..9.99=1,"inWhite":2..255=255,"outBlack":0..255=0,"outWhite":0..255=255}` |
| | `.exposure`, `.vibrance`, `.blackWhite` | `{"exposure":-20..20,"offset","gamma"}` / `{"vibrance":-100..100,"saturation":-100..100}` / `{"reds":…,"tint":bool,"tintColor":"#rrggbb"}` |
| Change an adjustment | `layer.setAdjustment` | `{"layer":id?, …params of that adjustment kind}` |
| Same, destructive | `image.adjustments.<kind>` (same params), `image.autoTone`, `image.autoContrast`, `image.adjustments.desaturate` | on the active pixel layer |
| Fill layer | `layer.newFillLayer.solidColor`, `.gradient` | `{"color":"#rrggbb"}` / `{"from":"#rrggbb","to":"#rrggbb","angle":deg=90,"style":"linear\|radial\|…"}` |
| Blur / sharpen | `filter.blur.gaussianBlur`, `filter.sharpen.unsharpMask` | `{"radius":0.1..1000=1}` / `{"amount":1..500=50,"radius":0.1..1000=1,"threshold":0..255=0}` |
| More filters | `filter.blur.motionBlur`, `filter.noise.addNoise`, `filter.noise.reduceNoise`, `filter.other.highPass` | `{"angle","distance"}` / `{"amount":0.1..400=12.5,"distribution":"uniform\|gaussian","monochromatic":bool}` / `{"strength":0..10=6,…}` / `{"radius":0.1..1000=10}` |
| Transform layer | `edit.transform` | `{"layer":id?,"rect":[x0,y0,x1,y1]?,"quad":[[x,y]×4]? (new corners, clockwise from top-left),"matrix":[a,b,c,d,e,f]?}` |
| Flip / rotate canvas | `image.imageRotation.90cw`, `.90ccw`, `.180`, `.flipCanvasHorizontal`; `image.rotation.arbitrary` | `{}`; `{"angle":deg,"direction":"cw\|ccw"}` |
| Image size | `image.imageSize` | `{"width":px,"height":px,"resolution":ppi,"resample":"bicubic\|…\|lanczos\|preserveDetails"}` (one side keeps the ratio) |
| Canvas size | `image.canvasSize` | `{"width":px,"height":px,"relative":bool=false,"anchor":"center\|topLeft\|…","extensionColor":"background\|white\|transparent\|#rrggbb"}` |
| Crop / trim | `image.crop`, `image.trim` | `{"x":px,"y":px,"width":px,"height":px}` / `{"basedOn":"transparent\|topLeft\|bottomRight"}` |
| Text | `type.create` | `{"x":px,"y":px (baseline of point text),"text":str,"box":[x,y,w,h]? (paragraph),"align":"left\|center\|right…","font":str,"size":pt=12,"color":"#rrggbb","name":str?}` |
| Edit / style text | `type.edit`, `type.setStyle` | `{"layer":id?,"text":str?}` / `{"layer":id?,"range":[start,end]?,"font","weight":100..900,"size","color","tracking","leading","align",…}` |
| Fonts | `type.fonts` | `{"family":str?}`: use names from here for `font` |
| Shape | `shape.create` | `{"kind":"rect\|roundedRect\|ellipse\|polygon\|star\|line\|path","rect":[x,y,w,h],"radii":r,"sides":3..100,"from":[x,y],"to":[x,y],"fill":"#rrggbb"\|null,"stroke":{"width":px,"color":"#rrggbb","align":"inside\|center\|outside"}\|null,"name":str?}` |
| Edit shape | `shape.edit` | `{"layer":id?,"rect":…?,"fill":…?,"stroke":…?,"move":[dx,dy]?}` |
| Layer styles | `layer.layerStyle.dropShadow`, `.stroke`, `.outerGlow`, `.colorOverlay`, `.clear` | `{"color":"#rrggbb","opacity":0..100=75,"angle":deg=120,"distance":px=5,"size":px=5,"layer":id}` / `{"size":px=3,"position":"outside\|inside\|center","color":"#rrggbb"}`; `.clear` `{"layer":id}` |
| Smart object | `layer.smartObjects.convertToSmartObject` | `{"layer":id?}` |
| Save a copy | `file.saveACopy` | `{"path":str (format from the extension),"quality":0..12? (JPEG),"layers":bool=true}` |
| Export a layer | `layer.exportAs` | `{"layer":id?,"path":text,"scale":1..1000=100}` (format from the extension; errors on an empty layer) |

`file.saveACopy` writes PNG, JPEG, WebP, TIFF, PSD and `.pcraft` (not SVG); flat formats flatten (the result's
`warnings` say so). Use `.psd` or `.pcraft` to keep layers. It never changes the document's own file.

## 5. Worked calls

```json
{"app":"photocraft","command":"file.new","params":{"width":1080,"height":1350,"background":"#f4efe6","name":"Poster"}}
{"app":"photocraft","command":"type.create","params":{"x":540,"y":300,"text":"SUMMER SALE","align":"center","font":"DejaVu Sans","size":96,"color":"#1d3557"}}
{"app":"photocraft","command":"layer.layerStyle.dropShadow","params":{"layer":3,"distance":8,"size":12,"opacity":40,"color":"#000000"}}
```

(`type.create` returned `{"layer":3,…}`; take the id from the result. `font` must be a family from `type.fonts`.)

Blur only the background of a photo: inspect → `layer.select {"layer":2}` → `select.subject {}` → `select.inverse {}` →
`{"command":"filter.blur.gaussianBlur","params":{"radius":8}}` → `select.deselect {}` → render.

Warm, non-destructive grade, then a flat JPEG into the workspace:

```json
{"app":"photocraft","command":"layer.newAdjustmentLayer.curves","params":{"points":[[0,10],[128,140],[255,250]]}}
{"app":"photocraft","command":"layer.newAdjustmentLayer.hueSaturation","params":{"saturation":15}}
{"app":"photocraft","command":"file.saveACopy","params":{"path":"out/photo-graded.jpg","quality":10}}
```

Scale a layer to double size from its frame: `{"command":"edit.transform","params":{"layer":3,"quad":[[100,100],[500,100],[500,300],[100,300]]}}`
when its content frame is 100,100 to 300,200 (`bounds` `[100,100,200,100]`).

## 6. Views, renders, pitfalls

- `app_inspect` views: `document`, `layer` (`id`), `selection`, `history` (with `canUndo`/`canRedo`), `documents`.
- `app_render` targets: `document` (the composite), `layer` (`id`, else the active one; alone and trimmed),
  `selection` (= the **selected layers** together, not the pixel selection). `max_side` defaults to 1024; `save_as`
  writes a PNG into the workspace.
- There is no `file.open`/`file.save` here: open with `septet_open`; write results with `file.saveACopy` or
  `layer.exportAs` and a workspace path (relative paths are in the workspace). The user saves the original
  document themselves. A save or export without `path`, or outside the workspace, asks the user. Don't use
  `file.export.quickExport` for a JPEG: it writes in the Export Preferences format (PNG) whatever the extension.
- Never run `file.close`, `file.closeAll`, `file.revert` or `prefs.*` (settings ask the user anyway).
- "not available right now: filters need a pixel layer": the active layer is a type/shape/adjustment layer.
  Select a pixel layer, or rasterize a copy (`layer.duplicate`, then `layer.rasterize.layer`).
- While a Free Transform box is open (after `septet_place`), `app_undo` steps through the box, not the document.
- Commands returning `null` (moves, selections, `layer.setProps`) succeeded; verify with inspect or render.
- Many params are optional; if a command rejects params, re-read its `params` text with `app_commands` rather than guessing.
