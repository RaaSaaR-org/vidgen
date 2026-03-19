# vidgen

An AI-agent-first video production pipeline that combines markdown-based project authoring, HTML scene rendering, offline TTS, and Model Context Protocol (MCP) integration into a single Rust binary.

## What it does

vidgen enables AI agents (and humans) to create complete videos for YouTube, Instagram Reels, and other platforms. The core pipeline:

1. **Parses** markdown scene files with YAML frontmatter (visual config) and body text (voiceover script)
2. **Renders** HTML/CSS scene templates in headless Chromium via CSS custom properties (`--frame`, `--progress`) and `Page.captureScreenshot` polling
3. **Synthesizes** voiceover with offline TTS (native/edge/piper) or cloud TTS (ElevenLabs)
4. **Encodes** final output via FFmpeg with platform-specific presets

AI agents interact via MCP tool calls — a complete 5-scene video can be created and rendered in 2 tool calls (~600 tokens).

## Installation

```bash
# Core (HTML scenes + video clips)
cargo install vidgen

# With website scroll capture
cargo install vidgen --features clipper

# With YouTube clip support
cargo install vidgen --features youtube

# Everything
cargo install vidgen --features clipper,youtube
```

Chromium and FFmpeg are auto-downloaded on first run. The YouTube feature auto-downloads the yt-dlp binary to `~/.vidgen/libs/`.

## Quick start

```bash
# Create a project from a preset (short, recap, educational)
vidgen init ./my-video --preset short

# Render a project
vidgen render ./my-video

# Quick render from text (alias: qr)
echo "Hello world" | vidgen qr -o hello.mp4
vidgen qr --text "Breaking news" -t lower-third -o news.mp4

# Preview all scenes as thumbnails
vidgen preview ./my-video --all

# Watch mode for live iteration
vidgen watch ./my-video

# Add assets
vidgen asset add ./photo.jpg -p ./my-video -c images
```

## Project structure

A vidgen project is a directory of human-readable, Git-friendly files:

```
my-video/
├── project.toml              # Project config (video, voice, theme settings)
├── scenes/
│   ├── 01-intro.md           # Scene files: YAML frontmatter + voiceover script
│   ├── 02-content.md
│   └── 03-outro.md
├── templates/
│   └── components/           # Custom HTML/CSS components (override built-ins)
├── styles/                   # CSS: variables, typography, animations
├── assets/
│   ├── clips/                # Video clips (from clip commands or manual)
│   ├── images/
│   ├── audio/
│   └── fonts/
├── output/                   # Rendered videos (gitignored)
└── .vidgen/                  # Cache (gitignored)
```

## Scene types

vidgen supports three scene types, all defined as `.md` files:

### HTML template scenes

Standard scenes rendered via Chromium. The template defines the visual, the body text becomes voiceover:

```yaml
---
template: title-card
duration: auto
props:
  title: "My Video Title"
  subtitle: "A subtitle"
voice:
  engine: edge
  voice: "de-DE-ConradNeural"
  speed: 1.1
---
This is the voiceover script. When duration is "auto",
the scene length is derived from the TTS audio length.
```

### Video clip scenes

External MP4 files (website captures, YouTube clips, screen recordings) used as scene visuals. Supports voiceover narration and source audio ducking:

```yaml
---
video_source: "@assets/clips/demo-website.mp4"
duration: auto
source_volume: 0.2
---
Here's what you see on screen. The original clip audio
plays at 20% while this voiceover narrates over it.
```

- `video_source` — path to MP4 file (`@assets/` prefix, relative path, or URL)
- `source_volume` — volume of clip's original audio (0.0 = mute, 1.0 = full, default: muted)
- `duration: auto` — uses the clip's actual length; fixed values trim the clip

### Sequence scenes

Multiple visuals with a single continuous voiceover. The narration spans all sub-scenes:

```yaml
---
template: sequence
duration: auto
sub_scenes:
  - template: title-card
    duration: 3
    props:
      title: "Welcome"
  - video_source: "@assets/clips/product-demo.mp4"
    duration: 4
    source_volume: 0.2
  - template: content-text
    duration: auto
    props:
      heading: "Key Features"
---
Welcome to our product. Here's a quick demo of what it can do.
And these are the key features we'll cover today.
```

- Sub-scenes can be HTML templates or video clips
- One sub-scene can have `duration: auto` to fill remaining voiceover time
- Source audio from video clips is ducked while voiceover plays
- The sequence outputs a single MP4 that participates in normal transitions

## Overlays (lower thirds)

Add info banners on top of any scene — show names, URLs, video titles, interview labels:

```yaml
---
video_source: "@assets/clips/interview.mp4"
duration: auto
overlay:
  text: "Jane Smith"
  subtext: "CEO, Acme Corp"
  style: modern
  position: bottom-left
  show_at: 0.5
  hide_at: 4.0
---
```

Overlays work on all scene types (HTML, video clips, sequence sub-scenes). They fade in/out smoothly and are rendered as transparent PNGs via Chromium, then composited via FFmpeg.

**Built-in styles:**

| Style | Look |
|-------|------|
| `modern` | Frosted glass with accent color bar (default) |
| `minimal` | Clean text on subtle dark background |
| `news` | Bold colored stripe, TV news style |
| `gradient` | Gradient background bar |

**Positions:** `bottom-left` (default), `bottom-right`, `top-left`, `top-right`

Overlays on sequence sub-scenes:

```yaml
sub_scenes:
  - video_source: "@assets/clips/demo.mp4"
    duration: 4
    overlay:
      text: "Product Demo"
      subtext: "myapp.com"
      style: minimal
```

## Video clip capture

Capture clips directly from websites or YouTube for use in scenes:

```bash
# Capture a scrolling website (requires --features clipper)
vidgen clip web https://example.com -p ./my-video -d 5 --scroll-speed 150

# Download + trim a YouTube clip (requires --features youtube)
vidgen clip youtube "https://youtu.be/..." -p ./my-video --from 10 --to 20

# Use the captured clip in a scene
# video_source: "@assets/clips/web-example-com.mp4"
```

## Built-in templates

| Template | Purpose |
|----------|---------|
| `title-card` | Full-screen title with animated entrance |
| `content-text` | Body text with heading and bullet points |
| `kinetic-text` | Word-by-word reveal synced to voiceover (fade/bounce/slide styles) |
| `slideshow` | Image carousel with cross-fade transitions |
| `quote-card` | Styled quote with attribution |
| `split-screen` | 2-4 panel comparison layout |
| `lower-third` | Name/title overlay |
| `caption-overlay` | Word-by-word caption overlay synced to audio |
| `cta-card` | End-screen call-to-action |

Custom templates go in `templates/components/` — the file stem becomes the template name and overrides built-ins.

## MCP server

vidgen exposes an MCP server (stdio transport) with 10 tools for AI agent integration:

```bash
vidgen mcp
```

| Tool | Purpose |
|------|---------|
| `create_project` | Create project with optional inline scenes (batch) |
| `add_scenes` | Batch-add scenes to existing project |
| `update_scene` | Modify a single scene's properties |
| `remove_scenes` | Remove scenes by index |
| `reorder_scenes` | Change scene order |
| `set_project_config` | Update project settings |
| `list_voices` | List available TTS voices |
| `preview_scene` | Generate a still frame preview |
| `render` | Start async video rendering |
| `get_project_status` | Get project info and render status |

## TTS engines

| Engine | Type | Notes |
|--------|------|-------|
| Native | Offline | Default. Uses macOS `say` / Linux `espeak-ng` |
| Edge | Offline | Microsoft Edge TTS via `edge-tts` CLI. High-quality neural voices |
| Piper | Offline | Fast local neural TTS via ONNX models. See [piper](https://github.com/rhasspy/piper) |
| ElevenLabs | Cloud | API key required (`ELEVEN_API_KEY`). Voice cloning support |

## Output formats

Supports multi-format rendering from a single project via CSS container queries:

- **Landscape** (1920x1080) — YouTube
- **Portrait** (1080x1920) — Instagram Reels, TikTok, YouTube Shorts
- **Square** (1080x1080)

Platform-specific encoding presets handle codec, bitrate, and file size constraints automatically.

## Debugging

```bash
# Verbose output (TTS details, encoding info)
vidgen render ./my-video -v

# Full debug (implies verbose + saves intermediate scene files)
vidgen render ./my-video --debug

# Custom debug output directory
vidgen render ./my-video --debug --debug-dir /tmp/vidgen-debug
```

Debug mode saves each per-scene MP4 to `output/debug/` (named by scene filename), making it easy to identify which scene has issues.

## Background music

Project-wide background music in `project.toml`:

```toml
[audio.background]
file = "@assets/audio/ambient.mp3"
volume = -12        # dB relative to voice
fade_in = 2.0       # seconds
fade_out = 3.0      # seconds
```

Per-scene music overrides the project default via `audio.music` in scene frontmatter.

## Asset references

- `@assets/...` — resolves to project `assets/` directory
- `./filename` — relative to scene file (for co-located assets)
- `{{theme.primary}}` — resolves to `project.toml` `[theme]` values
- `{{props.title}}` — resolves to scene frontmatter props
- HTTP/HTTPS URLs — auto-downloaded and cached in `assets/downloads/`

## Feature flags

| Feature | What it adds | Extra dependencies |
|---------|-------------|-------------------|
| `clipper` | `vidgen clip web` (website scroll capture) | None (uses existing Chromium) |
| `youtube` | `vidgen clip youtube` (YouTube download + trim) | `yt-dlp` crate (auto-downloads yt-dlp binary) |

## Examples

| Example | Description |
|---------|-------------|
| [`examples/minimal/`](examples/minimal/) | Bare-minimum 2-scene project |
| [`examples/intro/`](examples/intro/) | 7-scene intro video with multi-format output |
| [`examples/showcase/`](examples/showcase/) | 14 scenes demonstrating every template and feature |
| [`examples/features-test/`](examples/features-test/) | Tests emoji, per-scene voice, custom components |
| [`examples/video-clips/`](examples/video-clips/) | Video clips, sequences, website capture, YouTube integration |

```bash
vidgen render examples/minimal/
vidgen render examples/video-clips/
vidgen preview examples/showcase/ --all
```

## Design principles

- **Token efficiency first** — Batch MCP operations minimize AI agent token usage
- **Files as source of truth** — All state is human-readable (markdown, YAML, TOML, HTML, CSS)
- **Single binary** — `cargo install` gives you everything; external deps auto-download
- **Offline by default** — Ships with native/edge TTS; cloud TTS is opt-in
- **Web-native rendering** — Scenes are HTML/CSS with full CSS animations, SVG, Canvas support

## License

MIT
