# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project overview

vidgen is an AI-agent-first video production CLI in Rust. It renders HTML/CSS scenes in headless Chromium, synthesizes voiceover with offline TTS, and encodes via FFmpeg. AI agents interact through an MCP server (stdio transport, via `rmcp`). Projects are markdown files with YAML frontmatter — human-readable and Git-friendly.

## Build commands

- **Build:** `cargo build`
- **Build with all features:** `cargo build --features clipper,youtube`
- **Run:** `cargo run`
- **Test:** `cargo test`
- **Test with all features:** `cargo test --features clipper,youtube`
- **Run single test:** `cargo test <test_name>`
- **Lint:** `cargo clippy --features clipper,youtube`
- **Format:** `cargo fmt`
- **Check (fast compile check):** `cargo check`

## Feature flags

- `clipper` — Enables `vidgen clip web` (website scroll capture via Chromium)
- `youtube` — Enables `vidgen clip youtube` (YouTube download via `yt-dlp` crate)
- Both features are optional; core vidgen works without them

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

- **CLI layer** — `clap` v4 with global flags (`-v`, `--debug`, `--debug-dir`). Subcommands: `init`, `render`, `preview`, `watch`, `quickrender` (alias `qr`), `asset add`, `mcp`, `clip web/youtube`
- **MCP server** — `rmcp` crate, stdio transport, 10 tools (see prd.md §4). Batch-first design: `create_project` accepts inline scenes array for single-call video creation
- **Scene parser** — `pulldown-cmark` + `serde_yaml` for markdown/frontmatter; `toml` for project.toml. Three scene types: HTML template, video clip (`video_source`), and sequence (`sub_scenes`)
- **Template engine** — `handlebars` (v6) for `{{variable}}` injection into HTML templates. 9 built-in templates with CSS `@container` queries for multi-format adaptation. No browser-side framework
- **Render engine** — `chromiumoxide` with `--allow-file-access-from-files` flags. Uses CSS custom properties (`--frame`, `--total-frames`, `--progress`) injected per frame + `Page.captureScreenshot` polling. PNG bytes piped to FFmpeg via `image2pipe` (no intermediate frame files). Loads HTML via `file://` temp files for JS fetch() support
- **Video clip engine** — `prepare_video_clip()` re-encodes external MP4s to match target format (fps, resolution, codec). Supports source audio ducking via `source_volume`, TTS voiceover mixing, and background music
- **Sequence engine** — `render/sequence.rs` renders sub-scenes independently, concatenates with hard cuts, then mixes a single TTS voiceover + music onto the result via `mix_audio_onto_video()`
- **TTS engine** — Trait-based abstraction (`TtsEngine` trait) with implementations: `NativeTtsEngine` (macOS `say` / Linux `espeak-ng`), `EdgeTtsEngine` (`edge-tts` CLI), `PiperTtsEngine` (local neural via ONNX), `ElevenLabsTtsEngine` (`ureq`). All return `SynthesisResult` (audio path + duration + optional `WordTimestamp`) for kinetic text sync
- **Audio/video encoding** — FFmpeg subprocess. Platform-specific presets (CRF, codec, bitrate). All audio normalized to stereo AAC 44100Hz (`-ac 2`). Scene concatenation uses FFmpeg concat filter (not demuxer) for seamless audio at scene boundaries
- **Concurrency** — tokio async runtime. Parallel scene rendering via `buffer_unordered` (separate Chromium tabs), concatenated via FFmpeg concat filter
- **Clipper** — `clip web`: Chromium scroll capture (frame-by-frame screenshots → FFmpeg). `clip youtube`: `yt-dlp` crate with auto-binary download, re-encodes to H.264+AAC

### Project file layout (user projects, not this repo)

```
my-video/
├── project.toml           # [project], [video], [voice], [theme], [output], [audio] sections
├── scenes/*.md            # YAML frontmatter + body (voiceover script)
├── templates/components/  # HTML + CSS components (file stem = template name)
├── styles/                # variables.css, typography.css, animations.css, format-portrait.css
├── assets/
│   ├── clips/             # Video clips (from clip commands or manual)
│   ├── images/, audio/, fonts/
│   └── voiceover/         # TTS cache (gitignored)
└── output/                # Rendered videos + debug/ (gitignored)
```

### Scene frontmatter key fields

**All scenes:** `duration` (auto/explicit), `transition_in`/`transition_out`, `transition_duration`, `voice` (string or `{engine, voice, speed}` struct), `audio` (music, volume), `format_overrides`

**HTML template scenes:** `template` (component name), `props` (template variables), `background`

**Video clip scenes:** `video_source` (path to MP4), `source_volume` (0.0-1.0, duck original audio)

**Sequence scenes:** `sub_scenes` (array of sub-scene objects with `template`/`video_source`, `duration`, `props`, `source_volume`, `overlay`)

**Overlays (all scene types + sub-scenes):** `overlay` object with `text`, `subtext`, `show_at`, `hide_at`, `style` (modern/minimal/news/gradient), `position` (bottom-left/bottom-right/top-left/top-right). Rendered as transparent PNG via Chromium, composited via FFmpeg overlay filter with alpha fade

### Asset reference conventions

- `@assets/...` → project assets/ dir
- `./filename` → relative to scene file
- `{{theme.*}}` → project.toml [theme] values
- `{{props.*}}` → scene frontmatter props
- HTTP/HTTPS URLs → auto-downloaded and cached

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
| YouTube (optional) | yt-dlp |

## Design constraints

- `duration: auto` is the default — scene length derived from TTS audio length + configurable padding (0.5s before/after). AI agents never estimate timing
- For fixed-duration scenes, `-t` on the FFmpeg output enforces exact duration (truncates audio if TTS is longer)
- Per-scene MP4 durations are probed via `ffprobe` for accurate xfade concat offsets (handles TTS/video duration mismatches)
- Templates use Mustache-style `{{variable}}` processed server-side in Rust, not in browser JS
- Multi-format adaptation uses CSS container queries — components adapt to landscape/portrait/square via `@container` rules
- External deps (Chromium, FFmpeg, TTS models, yt-dlp) auto-download on first run and cache in `~/.vidgen/`
- No intermediate frame files: PNG bytes piped from Chromium → FFmpeg `image2pipe` stdin. Static scenes pipe the same frame N times
- All audio output forced to stereo AAC 44100Hz (`-ac 2 -ar 44100`) for consistent concat
- Scene concatenation uses FFmpeg concat filter (not demuxer) to prevent audio gaps at scene boundaries
- `amix` filters use `normalize=0` to prevent volume reduction when mixing multiple audio streams
- Config validation runs at load time — rejects out-of-range fps, dimensions, speed, padding, and parallel_scenes values
- Animated frame rendering loads HTML via `file://` temp file, then updates CSS custom properties per frame via JS injection
- Structured logging via `tracing` crate. CLI flags: `-v` (info), `--debug` (debug + saves scene files). Disabled for MCP mode
- `--debug` saves per-scene MP4s to `output/debug/` named by scene filename for easy issue isolation
- Emoji detection auto-injects Twemoji CDN script into HTML
- Per-scene voice config: `SceneVoiceConfig` supports both string and `{engine, voice, speed}` struct
- Project-wide `[audio.background]` config with dB volume, fade_in, fade_out; per-scene audio.music overrides project default
- Video clip scenes support `source_volume` for ducking original audio while voiceover plays
- Sequence scenes allow a single voiceover to span multiple visual sub-scenes (HTML templates + video clips)
- Overlays rendered as RGBA PNGs via Chromium (`omit_background: true`), composited via FFmpeg `overlay` filter with `loop` + `fade` alpha animations. Applied as post-process on per-scene MP4s (after render, before concat)

## Known bugs

See `BUGS.md` for detailed bug reports with reproduction steps.

- **BUG-001 (High):** Concat truncates video when mixing HTML-rendered scenes with `video_source` clip scenes. The per-scene MP4s are individually correct, but the final concat output is silently truncated after the first clip scene. Root cause: likely DTS/PTS timestamp mismatch or timebase incompatibility between Chromium-rendered scenes and `prepare_video_clip()` output. Re-encoding during concat fixes it. Transitions on clip scenes make it worse.
