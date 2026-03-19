# Known Bugs

## BUG-001: Video truncated when concatenating HTML-rendered scenes with video clip scenes

**Status:** Mitigated (auto-detected, falls back to hard cuts)
**Severity:** Low — transitions silently downgraded to hard cuts for mixed projects
**Discovered:** 2026-03-19

### Problem

When a project mixes HTML-rendered scenes (template-based) with video clip scenes (`video_source`), the final concatenated MP4 is silently truncated. The output file's metadata may report the correct total duration, but actual playable content ends at or shortly after the first video clip scene. Everything after appears black or the player stops.

### Reproduction

1. Create a project with 11 scenes:
   - Scenes 1-3: HTML template scenes (kinetic-text, robot-showcase, split-screen)
   - Scene 4: `video_source: "@assets/clips/atlas.mp4"` (12s YouTube clip)
   - Scenes 5-11: Mix of HTML templates and another video clip

2. Render: `vidgen render ./project/ -v`

3. Result: Output MP4 reports ~74s duration but video content stops after ~25s (right after scene 4 ends). Frames extracted via `ffmpeg -ss 26 ...` return empty files.

### Root Cause Analysis

The concat step (in `render/encoder.rs` or equivalent) uses FFmpeg's concat filter to join per-scene MP4s. When mixing scenes from two different sources:

- **HTML-rendered scenes**: encoded by vidgen's FFmpeg encoder (Chromium PNG frames → image2pipe → H.264)
- **Video clip scenes**: processed by `prepare_video_clip()` which re-encodes the external clip to match target format

Even though both produce H.264/AAC with matching profiles (High, level 40, yuv420p, 30fps, AAC-LC 44100Hz stereo), there is a subtle incompatibility that causes the concat filter to produce a truncated output. The issue does NOT occur when all scenes are HTML-rendered, and does NOT occur when re-encoding during concat (`-c:v libx264` instead of stream copy).

**Key observations:**
- `ffprobe` on individual debug scene files: all look identical (codec, profile, level, pix_fmt, fps, sample_rate)
- `ffmpeg -f concat -safe 0 -i list.txt -c copy` produces truncated output
- `ffmpeg -f concat -safe 0 -i list.txt -c:v libx264 -crf 23 -c:a aac` produces correct full-length output
- Adding `transition_in: fade` on clip scenes makes the problem worse (total corruption)
- A `Non-monotonic DTS` warning appears during concat, suggesting timestamp discontinuities

### Likely Fix Areas

1. **`prepare_video_clip()` in the video clip engine**: The re-encoded clip may have slightly different timing metadata (DTS/PTS) than Chromium-rendered scenes. Ensure output matches exactly:
   - Same timebase (`1/15360` as used by Chromium scenes)
   - Monotonic DTS starting from 0
   - Matching GOP structure / keyframe interval

2. **Concat filter setup**: If using `-filter_complex "[0:v][0:a][1:v][1:a]...concat=n=N:v=1:a=1"`, ensure all input segments have compatible stream parameters. Consider adding explicit format normalization: `[v]format=yuv420p,fps=30,settb=1/15360[v]`

3. **Fallback to re-encode concat**: If `-c copy` concat continues to fail with mixed sources, fall back to re-encoding during concat. This is slower but guarantees correctness. Could be a `--safe-concat` flag or auto-detected when `video_source` scenes are present.

4. **Transition handling on clip scenes**: Transitions on `video_source` scenes cause additional breakage. The transition (xfade) filter likely fails when one input is a clip. Either disable transitions on clip scenes automatically, or ensure the xfade filter gets properly formatted inputs.

### Workaround (current)

```bash
# 1. Render with --debug to get per-scene MP4s
vidgen render ./project/ --debug -v

# 2. Build concat list from debug files
DEBUGDIR="./project/output/debug/default"
ls "$DEBUGDIR"/*.mp4 | sort | awk '{print "file \x27" $0 "\x27"}' > /tmp/scenes.txt

# 3. Re-encode concat (fixes the truncation)
ffmpeg -y -f concat -safe 0 -i /tmp/scenes.txt \
  -c:v libx264 -crf 23 -preset medium -pix_fmt yuv420p \
  -c:a aac -ar 44100 -ac 2 \
  ./project/output/final.mp4
```

Also: do NOT use `transition_in`/`transition_out` on `video_source` scenes.

### Test Case

A minimal reproduction would be:
```
scenes/
  01-intro.md    → template: title-card, duration: 3
  02-clip.md     → video_source: "@assets/clips/any.mp4", duration: auto
  03-outro.md    → template: title-card, duration: 3
```

Expected: ~3+clip+3 seconds of video
Actual: ~3 seconds of video, rest is black/truncated
