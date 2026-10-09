---
name: video-editing
description: Cut and assemble video in Septet - build edits as OTIO/FCPXML/EDL timelines that Filmcraft imports, add subtitles (SRT/VTT), inspect and prepare media with ffmpeg/ffprobe when installed, pull frames to look at, and make title graphics. Use when the user wants to edit, trim, assemble, subtitle, convert or analyse video/audio, or asks for a rough cut, montage, social clip or lower thirds.
---

# Video editing

## How Filmcraft takes files (verified in its code)

- `septet_open` / `septet_place` with media (mp4, mov, mkv, webm, mxf, wav, mp3, …) **imports them into the
  project's bin only**; nothing goes onto the timeline.
- `.srt` / `.vtt` (also scc, stl, ttml) → a new **caption track** in the active sequence (a sequence is created if
  none is open).
- `.otio`, `.fcpxml`, `.xml` (FCP7), `.edl`, `.aaf`, `.omf` → their sequences, bins and media are **merged into the
  current project** in one undo step; the first imported sequence becomes active. Media files that exist are linked
  and probed; missing ones stay offline.
- `.fcproj` opens a Filmcraft project. (`.filmcraft` is listed as a Filmcraft file type in Septet but only
  `.fcproj` is treated as a project.)
- Filmcraft renders and exports with its own built-in encoders (H.264, ProRes, DNxHR, GIF, PNG/TIFF
  sequences, WAV/AIFF; HEVC only with a hardware encoder). The user exports from Filmcraft; ffmpeg isn't needed for
  that.

**So, to build an edit for the user, write a timeline file (OTIO is the best choice) and `septet_open` it.**

## Workflow

1. Get the media into the workspace (or use files the user opened in Septet). Write timelines into the workspace;
   media paths in them can be absolute, or relative to the timeline file's folder, or `file://` URLs.
2. Know your media: duration, frame rate, resolution, audio. With ffprobe (`command -v ffprobe`, Bash, needs approval):
   `ffprobe -v error -show_entries format=duration:stream=codec_type,width,height,r_frame_rate -of json in.mp4`.
   Without it, ask the user for the clips' durations and frame rate.
3. Look at the content: extract frames and Read them (ffmpeg, below). Pick in/out points from what you see and
   hear (transcripts: if the user has subtitles, read them).
4. Write `edit.otio`, `septet_open` it, then `septet_state` to confirm. Report the import's warnings if the user sees
   any.
5. Iterate: write `edit-v2.otio` rather than editing the old one (each import adds a sequence; tell the user).

## OTIO template

Times are `RationalTime` (value in frames at `rate`). A clip's `source_range.start_time` is the in-point in the
media; `duration` is its length. Clips on a track play one after another; use `Gap.1` for black/silence.
Video and audio are separate tracks: to keep a clip's sound, add the same clip to an Audio track.

```json
{
  "OTIO_SCHEMA": "Timeline.1",
  "name": "Rough cut",
  "global_start_time": {"OTIO_SCHEMA": "RationalTime.1", "rate": 25, "value": 0},
  "tracks": {
    "OTIO_SCHEMA": "Stack.1",
    "children": [
      {
        "OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video",
        "children": [
          {
            "OTIO_SCHEMA": "Clip.2", "name": "Intro",
            "source_range": {"OTIO_SCHEMA": "TimeRange.1",
              "start_time": {"OTIO_SCHEMA": "RationalTime.1", "rate": 25, "value": 50},
              "duration":   {"OTIO_SCHEMA": "RationalTime.1", "rate": 25, "value": 125}},
            "media_references": {"DEFAULT_MEDIA": {"OTIO_SCHEMA": "ExternalReference.1", "target_url": "media/a.mp4"}},
            "active_media_reference_key": "DEFAULT_MEDIA"
          },
          {
            "OTIO_SCHEMA": "Transition.1", "name": "Cross Dissolve", "transition_type": "SMPTE_Dissolve",
            "in_offset":  {"OTIO_SCHEMA": "RationalTime.1", "rate": 25, "value": 12},
            "out_offset": {"OTIO_SCHEMA": "RationalTime.1", "rate": 25, "value": 12}
          },
          { "…": "next Clip.2 (b.mp4)" }
        ]
      },
      { "OTIO_SCHEMA": "Track.1", "name": "A1", "kind": "Audio", "children": [ "…same clips/gaps…" ] }
    ]
  }
}
```

- The sequence frame rate comes from `global_start_time.rate` (or the first clip); 23.976/29.97 are snapped to
  24000/1001 and 30000/1001. Sequence size defaults to 1920×1080.
- A transition needs handles: the media must extend `in_offset` beyond the outgoing clip's out-point and `out_offset`
  before the incoming clip's in-point.
- Understood transition names: Cross Dissolve (`SMPTE_Dissolve`), Constant Power, Constant Gain, Dip to Black,
  Wipe. Anything else becomes a dissolve.
- Speed: add `"effects": [{"OTIO_SCHEMA": "LinearTimeWarp.1", "time_scalar": 2.0}]` to a clip (negative =
  reverse, 0 = freeze; or `FreezeFrame.1`). Other OTIO effects are ignored.
- Black/colour/bars: a clip with `GeneratorReference.1` and `generator_kind` `SolidColor`
  (`"parameters": {"color": [r, g, b, a]}`, 0–1 floats), `SMPTEBars` or `black`.
- Markers: `"markers": [{"OTIO_SCHEMA": "Marker.2", "name": "…", "color": "RED", "marked_range": {TimeRange}}]`
  on a clip or on the Stack (sequence markers).
- Several tracks: more `Track.1` entries in the Stack; the first video track is V1 (bottom), later ones are on top.
- `ImageSequenceReference` is not read as a frame sequence: render image sequences to a movie first.

Alternatives: FCPXML 1.9–1.11 (titles, `sync-clip`, `mc-clip` are skipped) or CMX3600 EDL (no frame rate in the
file: Filmcraft assumes 24 fps, or 29.97 for drop-frame; split edits are ignored). OTIO is the most predictable.

## Subtitles

Write an SRT and `septet_open` it while the sequence is active:
```
1
00:00:01,000 --> 00:00:03,500
First line of dialogue

2
00:00:04,000 --> 00:00:06,200
Second line
```
Max ~42 characters per line, 2 lines, ≥ 1 s on screen, ~17 characters per second. Filmcraft can burn captions in
or write sidecars on export.

## ffmpeg recipes (only if `command -v ffmpeg` succeeds; each run needs approval)

```
ffmpeg -ss 00:00:12 -i in.mp4 -frames:v 1 -vf scale=640:-2 frames/at12s.png          # one frame to Read
ffmpeg -i in.mp4 -vf "fps=1/5,scale=480:-2,tile=4x3" -frames:v 1 frames/contact.png   # contact sheet, 1 frame / 5 s
ffmpeg -i in.mp4 -vf "select='gt(scene,0.3)',showinfo" -vsync vfr -f null - 2>&1 | grep pts_time   # scene cuts
ffmpeg -i in.mp4 -af silencedetect=n=-35dB:d=0.6 -f null - 2>&1 | grep silence        # pauses in speech
ffmpeg -ss 10 -to 25 -i in.mp4 -c copy clip.mp4                                        # fast trim (cuts on keyframes)
ffmpeg -i in.mov -c:v libx264 -crf 20 -preset medium -c:a aac -b:a 192k out.mp4        # convert
ffmpeg -i in.mp4 -vn -ac 1 -ar 16000 audio.wav                                         # audio for transcription
```

Prefer building timelines over rendering finished videos with ffmpeg: the user can still adjust a timeline in
Filmcraft. Don't overwrite source media.

## Titles and graphics

- Static titles, lower thirds, end cards: make a 1920×1080 SVG with a transparent background (`septet:svg-graphics`,
  `septet:typography`), render it to PNG with `septet_render size=1920 out=…`, and put the PNG on V2 in the OTIO
  (as a clip with a duration) or place it into Filmcraft's bin.
- Keep text inside the title-safe area (inner 90%), ≥ 40 px for body text at 1080p.
- Animated titles: `septet:motion-lottie` (Effectcraft), then render a movie there and bring it into Filmcraft.

## Pacing tips

Cut on action or on the beat; 2–4 s per shot for social montages, longer for interviews; J/L-cuts (audio leads
or trails) feel natural; start strong (the first 2 s decide on social media); 9:16 = 1080×1920 for Reels/Shorts.

## Related

`septet:septet-apps`, `septet:motion-lottie`, `septet:image-editing` (stills), `septet:color`.
