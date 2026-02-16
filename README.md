# vidgen

An AI-agent-first video production pipeline that combines markdown-based project authoring, HTML scene rendering, offline TTS, and Model Context Protocol (MCP) integration into a single Rust binary.

## What it does

vidgen enables AI agents (and humans) to create complete videos for YouTube, Instagram Reels, and other platforms. The core pipeline:

1. **Parses** markdown scene files with YAML frontmatter (visual config) and body text (voiceover script)
2. **Renders** HTML/CSS scene templates in headless Chromium via CSS custom properties (`--frame`, `--progress`) and `Page.captureScreenshot` polling
3. **Synthesizes** voiceover with offline TTS (native/edge) or cloud TTS (ElevenLabs)
4. **Encodes** final output via FFmpeg with platform-specific presets

AI agents interact via MCP tool calls — a complete 5-scene video can be created and rendered in 2 tool calls (~600 tokens).

## Installation

```bash
cargo install vidgen
```

Chromium and FFmpeg are auto-downloaded on first run.

## Quick start

```bash
# Render a project
vidgen render ./my-project/

# Preview a single scene
vidgen preview --scene 3 ./my-project/

# Watch mode for live iteration
vidgen watch ./my-project/

# Quick render from stdin
echo "Hello world" | vidgen quickrender --voice en_US-amy-medium -o hello.mp4
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
└── .vidforge/                # Cache (gitignored)
```

## Scene file format

Each scene is a markdown file. The YAML frontmatter defines visuals and timing; the body text becomes the voiceover:

```markdown
---
template: title-card
duration: auto
transition_in: fade
props:
  title: "My Video Title"
  subtitle: "A subtitle"
  title_animation: fade-up
audio:
  music: "@assets/audio/ambient.mp3"
  music_volume: 0.15
---

This is the voiceover script. When duration is set to "auto",
the scene length is derived from the TTS audio length.
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

## Asset references

- `@assets/...` — resolves to project `assets/` directory
- `./filename` — relative to scene file (for co-located assets)
- `{{theme.primary}}` — resolves to `project.toml` `[theme]` values
- `{{props.title}}` — resolves to scene frontmatter props

## License

TBD
