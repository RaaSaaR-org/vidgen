# vidgen Agent Guide

This document is designed as a skills reference for AI agents working with vidgen. It describes how to create videos programmatically using either the MCP server or the CLI.

## Overview

vidgen is a video production CLI that turns markdown scene files into MP4 videos. As an agent, you can:

- Create complete video projects from a topic or script
- Mix HTML-rendered slides with website captures and YouTube clips
- Add voiceover narration that spans across multiple visual scenes
- Render in multiple formats (landscape, portrait, square)

## Two ways to interact

### 1. MCP Server (recommended for agents)

Start with `vidgen mcp` (stdio transport). Available tools:

| Tool | What it does |
|------|-------------|
| `create_project` | Create a project with inline scenes in one call |
| `add_scenes` | Add scenes to an existing project |
| `update_scene` | Modify a scene's template, props, duration, etc. |
| `remove_scenes` | Remove scenes by index |
| `reorder_scenes` | Change scene order |
| `set_project_config` | Update project.toml settings |
| `list_voices` | List available TTS voices per engine |
| `preview_scene` | Get a PNG preview of a scene |
| `render` | Render the project to MP4 |
| `get_project_status` | Check project info, scene count, render state |
| `export_media` | Export scene as PNG/GIF/WebP (supports `progress` parameter) |
| `batch` | Execute multiple operations in one call (reduces round-trips) |
| `get_render_progress` | Poll render progress during long renders |

**Batch workflow (2 calls for a complete video):**
1. `create_project` with `scenes` array — creates project + all scenes at once
2. `render` — produces the final MP4

### 2. CLI (for shell-based agents)

```bash
# Get help on any command
vidgen --help
vidgen render --help
vidgen clip web --help

# Create a project
vidgen init ./my-video --preset short

# Platform presets
vidgen init ./my-video --preset youtube-short  # 1080x1920
vidgen init ./my-video --preset tiktok         # 1080x1920
vidgen init ./my-video --preset instagram-reel # 1080x1920

# Render
vidgen render ./my-video

# Quick one-off video from text
echo "Your script here" | vidgen qr -o output.mp4

# Debug a render (saves intermediate scene files)
vidgen render ./my-video --debug

# Export media (images, GIFs, audio, subtitles)
vidgen export ./my-video image --scene 0 --progress 0.5
vidgen export ./my-video gif --scene 0 --duration 3
vidgen export ./my-video audio --scene 0
vidgen export ./my-video subtitles

# Project management
vidgen info ./my-video          # Timing overview without rendering
vidgen validate ./my-video      # Check for issues before render
vidgen diff ./my-video          # What changed since last render
vidgen test ./my-video          # Visual regression testing
vidgen templates -p ./my-video  # Browse available templates

# Render options
vidgen render ./my-video --speed 1.2     # Faster voiceover
vidgen render ./my-video --crop 9:16     # Auto-crop aspect ratio
vidgen render ./my-video --gpu           # Hardware encoding
vidgen render ./my-video --no-cache      # Force full re-render
```

## Scene types reference

### HTML template scene

Uses a built-in or custom HTML template rendered via Chromium.

```yaml
---
template: title-card
duration: auto
props:
  title: "Welcome"
  subtitle: "to my video"
---
This text becomes the voiceover narration.
```

**Key fields:**
- `template` — built-in name or custom component filename (without .html)
- `duration` — `auto` (fits TTS length) or seconds (e.g., `5`)
- `props` — key-value pairs passed to the template
- `voice` — optional per-scene TTS override: `"voice-name"` or `{engine, voice, speed}`

**Built-in templates:** `title-card`, `content-text`, `kinetic-text`, `slideshow`, `quote-card`, `split-screen`, `lower-third`, `caption-overlay`, `cta-card`

### Video clip scene

Uses an external MP4 file as the visual. Supports voiceover + source audio ducking.

```yaml
---
video_source: "@assets/clips/demo.mp4"
duration: auto
source_volume: 0.2
---
Optional voiceover narration over the clip.
```

**Key fields:**
- `video_source` — path to MP4 (`@assets/clips/...`, relative path, or URL)
- `source_volume` — volume of clip's own audio: 0.0 (muted, default) to 1.0 (full)
- `duration` — `auto` (clip's actual length) or fixed seconds (trims clip)

### Sequence scene

Multiple visuals with one continuous voiceover. The narration spans all sub-scenes.

```yaml
---
template: sequence
duration: auto
sub_scenes:
  - template: content-text
    duration: 3
    props:
      heading: "Point One"
  - video_source: "@assets/clips/demo.mp4"
    duration: 4
    source_volume: 0.2
  - template: content-text
    duration: auto
    props:
      heading: "Summary"
---
First we'll discuss point one. Here's a demo of it in action.
And now let's summarize what we've seen.
```

**Rules:**
- At most one sub-scene can have `duration: auto` (fills remaining voiceover time)
- Each sub-scene needs either `template` or `video_source`
- Sub-scenes are joined with hard cuts (no transitions between them)
- The sequence itself can have transitions to adjacent scenes

## Overlays (lower thirds / info banners)

Add info banners on top of any scene. Works on HTML scenes, video clips, and sequence sub-scenes:

```yaml
---
video_source: "@assets/clips/interview.mp4"
duration: auto
overlay:
  text: "Jane Smith"
  subtext: "CEO, Acme Corp"
  style: modern         # modern, minimal, news, gradient
  position: bottom-left  # bottom-left, bottom-right, top-left, top-right
  show_at: 0.5           # seconds (default: 0.5)
  hide_at: 4.0           # seconds (default: scene_duration - 0.5)
---
```

On sequence sub-scenes:

```yaml
sub_scenes:
  - video_source: "@assets/clips/yt-clip.mp4"
    duration: 4
    overlay:
      text: "Rick Astley"
      subtext: "youtube.com"
      style: news
```

**Styles:**
- `modern` — frosted glass with accent color bar (default)
- `minimal` — clean text on dark background
- `news` — bold colored stripe, TV news style
- `gradient` — gradient background bar

**Best practices:**
- Use overlays to show sources (URLs, video titles) on clip scenes
- Use `news` style for breaking-news or interview name labels
- `show_at`/`hide_at` default to 0.5s after start / 0.5s before end
- Overlays fade in/out smoothly (0.3s alpha transition)

## Creating video clips

### Website scroll capture

Capture a scrolling website as an MP4 clip:

```bash
vidgen clip web "https://example.com" \
  -p ./my-video \
  -d 5 \
  --scroll-speed 150 \
  --fps 24 \
  -o "website-demo"
```

The clip is saved to `assets/clips/website-demo.mp4`. Use it in a scene:

```yaml
video_source: "@assets/clips/website-demo.mp4"
```

### YouTube clip

Download and trim a YouTube video:

```bash
vidgen clip youtube "https://www.youtube.com/watch?v=..." \
  -p ./my-video \
  --from 10 \
  --to 25 \
  -o "yt-highlight"
```

Output is always re-encoded to H.264+AAC for universal playback.

## Project configuration

`project.toml` controls project-wide settings:

```toml
[project]
name = "My Video"

[video]
fps = 30
default_transition = "fade"          # fade, slide-left, slide-right, zoom, wipe
default_transition_duration = 0.5

# Multi-format output (optional)
[video.formats.landscape]
width = 1920
height = 1080

[video.formats.portrait]
width = 1080
height = 1920
platform = "instagram-reels"

[voice]
engine = "edge"                      # native, edge, piper, elevenlabs
default_voice = "en-US-JennyNeural"
speed = 1.0
language = "de"                      # Language hint for multilingual TTS
normalize = true                     # Audio normalization (loudnorm)
padding_before = 0.5                 # silence before voiceover
padding_after = 0.5                  # silence after voiceover

[theme]
primary = "#007bff"
secondary = "#6c757d"
background = "#1a1a2e"
text = "#ffffff"
font_family = "Inter, system-ui, sans-serif"

[output]
directory = "output"
quality = "standard"                 # draft, standard, high

[audio.background]
file = "@assets/audio/ambient.mp3"
volume = -12                         # dB
fade_in = 2.0
fade_out = 3.0
```

## Export & Media Generation

vidgen can export individual scenes as images, GIFs, audio, or subtitles — useful for thumbnails, social previews, and asset extraction.

### MCP tools

- **`export_media`** — Export a scene as PNG, GIF, or WebP. Use the `progress` parameter (0.0–1.0) to capture a specific moment in the scene. Example: `progress: 0.5` captures mid-scene, useful for previewing animations or finding the best thumbnail frame.
- **`batch`** — Execute multiple operations in a single MCP call. Reduces round-trips when you need to export several scenes or combine exports with config changes. Each operation in the batch runs sequentially and returns individual results.

### CLI export commands

```bash
# Static image at a specific progress point
vidgen export ./my-video image --scene 0 --progress 0.5

# Animated GIF (palette-optimized, two-pass)
vidgen export ./my-video gif --scene 0 --duration 3

# Extract scene audio (TTS voiceover)
vidgen export ./my-video audio --scene 0

# Generate subtitle file for the entire project
vidgen export ./my-video subtitles
```

**Use cases for agents:**
- Generate thumbnail candidates by exporting at `progress: 0.0`, `0.5`, and `1.0`
- Create social media preview GIFs from key scenes
- Extract audio for transcription or review
- Use `batch` to export multiple formats in one call

## Quality Assurance

vidgen includes built-in tools for validating projects and catching issues before rendering.

### Validate

```bash
vidgen validate ./my-video
```

Checks for common issues:
- Config errors (invalid fps, dimensions, speed values)
- Missing or unknown templates
- Missing asset files (images, clips, fonts)
- WCAG contrast ratio warnings for text/background combinations
- Duration warnings for unusually short or long scenes

### Visual regression testing

```bash
# Create baseline snapshots (renders each scene at 3 progress points)
vidgen test ./my-video --update

# Compare against baselines (pixel-diff)
vidgen test ./my-video
```

Snapshots are stored in `.vidgen/snapshots/`. Run `--update` after intentional visual changes.

### Diff

```bash
vidgen diff ./my-video
```

Shows which scenes have changed since the last render by comparing TTS cache keys. Useful to understand what will be re-rendered.

### Quality report

After rendering, vidgen outputs a quality report including:
- Output bitrate and codec details
- Peak audio levels
- Final file size
- Per-scene render times

## Best practices for agents

### Duration

- **Always use `duration: auto`** for scenes with voiceover — lets TTS determine timing
- Only use fixed durations for scenes without voiceover, or when you need exact timing
- For sequence scenes, use `auto` on one sub-scene to absorb timing differences

### Scene pacing

- Keep voiceover scripts concise — 1-2 sentences per scene for short videos
- Use sequence scenes when you need visual variety under a longer narration
- Duck source audio (`source_volume: 0.1-0.3`) on video clips with voiceover

### Efficiency

- Use `batch` tool to reduce MCP round-trips when performing multiple operations
- Use `export_media` with `progress: 0.5` for mid-scene previews before committing to a full render
- Use `info` via MCP (`get_project_status`) to check timing before rendering
- Incremental rendering caches unchanged scenes — second renders are 10x faster

### Debugging

- Use `vidgen render ./project --debug` to save per-scene MP4s
- Check individual scene files in `output/debug/` to isolate audio/video issues
- Use `-v` for verbose output showing TTS durations, encoding details

### Templates

- `title-card` — opening/closing slides
- `content-text` — body content, explanations
- `kinetic-text` — engaging word-by-word reveals
- `quote-card` — testimonials, quotes
- `lower-third` — name overlays
- `cta-card` — end screens with call-to-action

### Getting more info

```bash
# List all commands
vidgen --help

# Detailed help for any command
vidgen render --help
vidgen clip web --help
vidgen clip youtube --help
vidgen qr --help

# List available TTS voices (via MCP or CLI)
vidgen mcp  # then use list_voices tool

# Check project status
vidgen mcp  # then use get_project_status tool
```

## Example: Complete video from scratch

```bash
# 1. Create project
vidgen init ./demo-video

# 2. Capture a website clip
vidgen clip web "https://myapp.com" -p ./demo-video -d 5 -o "app-demo"

# 3. Edit scenes (create scene files in scenes/)
# scenes/01-intro.md:
#   template: title-card, duration: auto, props: {title: "Demo"}
#   Script: "Welcome to this demo."
#
# scenes/02-demo.md:
#   video_source: "@assets/clips/app-demo.mp4", duration: auto
#   Script: "Here's the app in action."
#
# scenes/03-outro.md:
#   template: cta-card, duration: auto, props: {heading: "Try it!"}
#   Script: "Thanks for watching."

# 4. Render
vidgen render ./demo-video

# 5. Debug if needed
vidgen render ./demo-video --debug
```

## Example: MCP batch creation

```json
{
  "tool": "create_project",
  "arguments": {
    "path": "/tmp/quick-video",
    "name": "Quick Video",
    "voice_engine": "edge",
    "scenes": [
      {
        "template": "title-card",
        "props": {"title": "Hello World"},
        "script": "Welcome to this quick video."
      },
      {
        "template": "content-text",
        "props": {"heading": "Key Point", "body": "This is the main content."},
        "script": "Here's the most important thing to know."
      },
      {
        "template": "cta-card",
        "props": {"heading": "Thanks!", "button_text": "Subscribe"},
        "script": "Thanks for watching."
      }
    ]
  }
}
```

Then call `render` with `{"path": "/tmp/quick-video"}` to produce the MP4.
