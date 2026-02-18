# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project overview

vidgen is an AI-agent-first video production CLI in Rust. It renders HTML/CSS scenes in headless Chromium, synthesizes voiceover with offline TTS, and encodes via FFmpeg. AI agents interact through an MCP server (stdio transport, via `rmcp`). Projects are markdown files with YAML frontmatter — human-readable and Git-friendly.

## Build commands

- **Build:** `cargo build`
- **Run:** `cargo run`
- **Test:** `cargo test`
- **Run single test:** `cargo test <test_name>`
- **Lint:** `cargo clippy`
- **Format:** `cargo fmt`
- **Check (fast compile check):** `cargo check`

## Architecture

### Pipeline flow

```
MCP Client → MCP Server (rmcp/stdio) → Project FS (markdown + HTML/CSS) → Render Engine
                                                                              ├→ TTS Engine → WAV per scene ─┐
                                                                              └→ Chromium (Screenshot) ──────┤
                                                                                                             ↓
                                                                                                     FFmpeg → MP4
```

### Key subsystems

- **CLI layer** — `clap` v4 subcommands: `init`, `render`, `preview`, `watch`, `quickrender` (alias `qr`), `mcp`
- **MCP server** — `rmcp` crate, stdio transport, 10 tools (see prd.md §4). Batch-first design: `create_project` accepts inline scenes array for single-call video creation
- **Scene parser** — `pulldown-cmark` + `serde_yaml` for markdown/frontmatter; `toml` for project.toml
- **Template engine** — `handlebars` (v6) for `{{variable}}` injection into HTML templates. 9 built-in templates with CSS `@container` queries for multi-format adaptation. No browser-side framework
- **Render engine** — `chromiumoxide` with `--run-all-compositor-stages-before-draw` flag. Uses CSS custom properties (`--frame`, `--total-frames`, `--progress`) injected per frame + `Page.captureScreenshot` polling (no `HeadlessExperimental.beginFrame` on macOS). PNG bytes piped to FFmpeg via `image2pipe` (no intermediate frame files)
- **TTS engine** — Trait-based abstraction (`TtsEngine` trait) with implementations: `NativeTtsEngine` (macOS `say` / Linux `espeak-ng`), `EdgeTtsEngine` (`edge-tts` CLI), `PiperTtsEngine` (local neural via ONNX), `ElevenLabsTtsEngine` (`ureq`). All return `SynthesisResult` (audio path + duration + optional `WordTimestamp`) for kinetic text sync
- **Audio/video encoding** — `ffmpeg-sidecar` or subprocess. Platform-specific presets (CRF, codec, bitrate). `adelay` filter for per-scene audio offset. Multi-format: re-renders with different viewport dimensions
- **Concurrency** — tokio async runtime. Producer-consumer channel between frame capture and FFmpeg. Parallel scene rendering (separate Chromium tabs), concatenated via FFmpeg concat demuxer

### Project file layout (user projects, not this repo)

```
my-video/
├── project.toml           # [project], [video], [voice], [theme], [output] sections
├── scenes/*.md            # YAML frontmatter (template, duration, props, audio) + body (voiceover script)
├── templates/components/  # HTML + CSS components (file stem = template name)
├── styles/                # variables.css, typography.css, animations.css, format-portrait.css
├── assets/                # images/, audio/, fonts/, voiceover/ (gitignored)
└── output/                # Rendered videos (gitignored)
```

### Scene frontmatter key fields

`template` (component name), `duration` (auto/explicit), `transition_in`/`transition_out`, `background`, `props` (template variables), `audio` (music, volume), `voice` (override), `format_overrides`

### Asset reference conventions

- `@assets/...` → project assets/ dir
- `./filename` → relative to scene file
- `{{theme.*}}` → project.toml [theme] values
- `{{props.*}}` → scene frontmatter props

## Key crate dependencies

| Purpose | Crate |
|---------|-------|
| CLI | clap v4 |
| Async | tokio |
| MCP | rmcp |
| Browser | chromiumoxide (0.8, tokio-runtime) |
| FFmpeg | ffmpeg-sidecar (2.0) |
| TTS (cloud) | ureq |
| Markdown | pulldown-cmark |
| YAML | serde_yml (0.0.12) |
| TOML | toml |
| Templates | handlebars (6) |
| Serialization | serde + schemars |
| Logging | tracing + tracing-subscriber |
| File watching | notify + notify-debouncer-mini |
| Hashing | sha2 |

## Design constraints

- `duration: auto` is the default — scene length derived from TTS audio length + configurable padding (0.5s before/after). AI agents never estimate timing
- Templates use Mustache-style `{{variable}}` processed server-side in Rust, not in browser JS
- Multi-format adaptation uses CSS container queries — components adapt to landscape/portrait/square via `@container` rules
- External deps (Chromium, FFmpeg, TTS models) auto-download on first run and cache in `~/.vidgen/`
- No intermediate frame files: PNG bytes piped from Chromium `captureScreenshot` → FFmpeg `image2pipe` stdin. Static scenes (no `--frame`/`--progress` vars) pipe the same single frame N times rather than re-capturing
- Word-by-word reveal in `kinetic-text` and `caption-overlay` uses CSS `--reveal` variable with `max-width` collapse (hidden words have zero width via `overflow: hidden`)
- Config validation runs at load time — rejects out-of-range fps, dimensions, speed, padding, and parallel_scenes values
- Animated frame rendering loads HTML into the page once, then updates CSS custom properties per frame via JS injection (no repeated `set_content` calls)
- Structured logging via `tracing` crate, gated on `RUST_LOG` env var, writes to stderr. Disabled for MCP mode to avoid corrupting stdio JSON
