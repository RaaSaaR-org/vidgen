# vidgen

An AI-agent-first video production pipeline that combines markdown-based project authoring, HTML scene rendering, offline TTS, and Model Context Protocol (MCP) integration into a single Rust binary.

## What it does

vidgen enables AI agents (and humans) to create complete videos for YouTube, Instagram Reels, and other platforms. The core pipeline:

1. **Parses** markdown scene files with YAML frontmatter (visual config) and body text (voiceover script)
2. **Renders** HTML/CSS scene templates in headless Chromium via CSS custom properties (`--frame`, `--progress`) and `Page.captureScreenshot` polling
3. **Synthesizes** voiceover with offline TTS (native/edge/piper) or cloud TTS (ElevenLabs)
4. **Encodes** final output via FFmpeg with platform-specific presets

AI agents interact via MCP tool calls — a complete 5-scene video can be created and rendered in 2 tool calls (~600 tokens).

### Debugging

Set the `RUST_LOG` environment variable to enable structured tracing output (written to stderr):

```bash
RUST_LOG=debug vidgen render ./my-project/
RUST_LOG=vidgen=trace vidgen render ./my-project/
```

## Installation

```bash
cargo install vidgen
```

Chromium and FFmpeg are auto-downloaded on first run.

## Quick start

```bash
# Create a project from a preset (short, recap, educational)
vidgen init ./my-video --preset short

# Render a project
vidgen render ./my-project/

# Preview all scenes as thumbnails
vidgen preview ./my-project/ --all

# Preview a single scene as animated GIF
vidgen preview ./my-project/ --scene 2 --gif

# Watch mode for live iteration
vidgen watch ./my-project/

# Quick render from stdin
echo "Hello world" | vidgen quickrender --voice en_US-amy-medium -o hello.mp4

# Add assets to a project
vidgen asset add ./photo.jpg -p ./my-project/ -c images
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
│   └── components/           # HTML/CSS visual components
├── styles/                   # CSS: variables, typography, animations
├── assets/                   # Images, audio, fonts
├── output/                   # Rendered videos (gitignored)
└── .vidgen/                  # Cache (gitignored)
```

## Scene file format

Each scene is a markdown file. The YAML frontmatter defines visuals and timing; the body text becomes the voiceover:

```markdown
---
template: title-card
duration: auto
transition_in: fade
props:
  title: "My Video Title 🚀"
  subtitle: "A subtitle"
  title_animation: fade-up
voice:
  engine: edge
  voice: "de-DE-ConradNeural"
  speed: 1.1
audio:
  music: "@assets/audio/ambient.mp3"
  music_volume: 0.15
---

This is the voiceover script. When duration is set to "auto",
the scene length is derived from the TTS audio length.
```

Per-scene voice config supports both simple (`voice: "en-US-JennyNeural"`) and structured form with engine/speed overrides. Emoji characters are automatically rendered via Twemoji CDN.

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
| `caption-overlay` | Word-by-word caption overlay synced to audio (outline/background-box/drop-shadow styles) |
| `cta-card` | End-screen call-to-action |

## MCP server

vidgen exposes an MCP server (stdio transport) with 10 tools for AI agent integration:

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

## Design principles

- **Token efficiency first** — Batch MCP operations minimize AI agent token usage
- **Files as source of truth** — All state is human-readable (markdown, YAML, TOML, HTML, CSS)
- **Single binary** — `cargo install` gives you everything; external deps auto-download
- **Offline by default** — Ships with native/edge TTS; cloud TTS is opt-in
- **Web-native rendering** — Scenes are HTML/CSS with full CSS animations, SVG, Canvas support

## Examples

The `examples/` directory contains four projects:

| Example | Description |
|---------|-------------|
| [`examples/minimal/`](examples/minimal/) | Bare-minimum 2-scene project — the simplest thing that works |
| [`examples/intro/`](examples/intro/) | 7-scene intro video with multi-format output (landscape, portrait, square) |
| [`examples/showcase/`](examples/showcase/) | 14 scenes demonstrating every built-in template, custom components, and features (subtitles, format overrides, parallel rendering) |
| [`examples/features-test/`](examples/features-test/) | 6 scenes testing emoji rendering, per-scene voice config, custom components, and asset management |

```bash
# Render the minimal example
vidgen render examples/minimal/

# Render the showcase in all three formats
vidgen render examples/showcase/

# Preview all scenes of the features test
vidgen preview examples/features-test/ --all
```

## Background music

Project-wide background music can be configured in `project.toml`:

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

## License

TBD
