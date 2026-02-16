# Rust CLI video production tool with MCP server integration

**An AI-agent-first video production pipeline that combines markdown-based project authoring, HTML scene rendering, offline TTS, and Model Context Protocol integration into a single Rust binary.** This tool enables AI agents to create complete videos for YouTube (16:9), Instagram Reels (9:16), and other platforms through token-efficient MCP tool calls, while keeping every project file human-readable and Git-friendly. The core pipeline renders HTML/CSS scenes in headless Chromium using deterministic frame capture, synthesizes voiceover with offline neural TTS, and encodes final output via FFmpeg — all orchestrated from a single `cargo install`-able CLI.

---

## 1. Executive summary and product vision

### The problem

Producing short-form video content today requires either expensive GUI editors (Premiere, DaVinci Resolve), React/Node.js frameworks (Remotion), or Python scripts (MoviePy, Manim). None of these are designed for AI agent workflows. An LLM that wants to create a video must juggle complex APIs, manage heavy runtimes, and spend thousands of tokens coordinating multi-step processes. Meanwhile, human editors cannot easily review or modify what the AI produced — project files are either binary blobs or deeply nested code.

### The vision

**vidgen** (working name) is a Rust CLI that exposes an MCP server so AI agents like Claude can produce videos with minimal tool calls. Projects live as plain markdown files in a Git repository. Each scene is a markdown file whose YAML frontmatter defines layout, timing, and voice settings, whose body text becomes the voiceover script, and whose visual presentation is an HTML/CSS template rendered in headless Chromium. A human can open any scene file in a text editor, change the script, and re-render with a single command.

### Design principles

- **Token efficiency first.** A single `create_project` tool call with an inline scenes array should produce a complete video. Batch operations over granular CRUD.
- **Files as the source of truth.** Every piece of state is a human-readable file on disk — markdown, YAML, TOML, HTML, CSS. No databases, no opaque binary formats.
- **Single binary distribution.** `cargo install vidgen` gives you everything except Chromium and FFmpeg, which are auto-downloaded on first run.
- **Offline by default, cloud when desired.** Ships with native platform TTS. ElevenLabs is opt-in via API key.
- **Web-native rendering.** Scenes are HTML/CSS — the most expressive, well-tooled visual authoring format that exists. Full CSS animations, SVG, Canvas, and custom fonts.

---

## 2. Technical architecture

### High-level pipeline

```
┌─────────────┐    ┌──────────────┐    ┌─────────────┐    ┌──────────────┐
│  MCP Client │───▶│  MCP Server  │───▶│  Project FS  │───▶│  Render      │
│  (Claude,   │    │  (rmcp/stdio)│    │  (markdown + │    │  Engine      │
│   Cursor)   │    │              │    │   HTML/CSS)  │    │              │
└─────────────┘    └──────────────┘    └──────────────┘    └──────┬───────┘
                                                                  │
                          ┌───────────────────────────────────────┤
                          ▼                                       ▼
                   ┌─────────────┐                      ┌──────────────┐
                   │  TTS Engine │                      │  Chromium    │
                   │  (native/   │                      │  (chromium-  │
                   │   edge/     │                      │   oxide +    │
                   │   cloud)    │                      │  Screenshot) │
                   └──────┬──────┘                      └──────┬───────┘
                          │                                     │
                          ▼                                     ▼
                   ┌─────────────┐                      ┌──────────────┐
                   │  WAV audio  │─────────────────────▶│  FFmpeg      │
                   │  per scene  │                      │  (sidecar/   │
                   └─────────────┘                      │   subprocess)│
                                                        └──────┬───────┘
                                                               │
                                                               ▼
                                                        ┌──────────────┐
                                                        │  MP4 output  │
                                                        │  (multi-fmt) │
                                                        └──────────────┘
```

### Rendering pipeline in detail

The rendering engine is the most critical subsystem. It uses the **frame-as-function** paradigm proven by Remotion: every frame is a deterministic render of an HTML page at a specific point in time.

**Step 1 — Scene preparation.** For each scene, the engine loads the scene's HTML template, injects the scene's props (title, subtitle, body text, images) via template variables, and writes a self-contained HTML file to a temp directory. Global CSS (variables, typography, animations) and component CSS are bundled inline.

**Step 2 — TTS synthesis.** The voiceover script (markdown body text) is sent to the TTS engine. The resulting WAV file determines the scene's actual duration if `duration: auto` is set in frontmatter. Audio metadata (duration, sample rate) is fed back to the scene config.

**Step 3 — Deterministic frame capture.** The engine launches headless Chromium via `chromiumoxide` with the `--run-all-compositor-stages-before-draw` flag. For each frame:

1. The engine injects CSS custom properties (`--frame`, `--total-frames`, `--progress`) into the page's `:root` element before each capture
2. Templates use these CSS custom properties to drive animations deterministically (e.g., `opacity: var(--progress)`)
3. A screenshot is captured as PNG bytes via `Page.captureScreenshot`
4. The PNG bytes are piped directly to FFmpeg's stdin via `image2pipe` — **no intermediate frame files on disk**

This approach achieves deterministic frame-by-frame rendering by driving all animation state through CSS custom properties rather than relying on real-time browser animation clocks. Each frame is a pure function of its `--progress` value.

**Step 4 — Audio mixing and encoding.** FFmpeg receives raw RGBA frames on stdin and encodes to H.264. In a second pass (or a single complex pipeline), per-scene voiceover WAVs, background music, and sound effects are mixed and muxed with the video stream. The `adelay` filter handles per-scene audio offset, and audio normalization ensures consistent volume.

**Step 5 — Multi-format output.** For each target format (landscape 1920×1080, portrait 1080×1920, square 1080×1080), the engine re-renders with the appropriate viewport dimensions and format-specific CSS (loaded via container queries or format-variant stylesheets). FFmpeg encoding settings are preset per platform.

### Concurrency model

The rendering pipeline uses **tokio** for async browser control and I/O, with a **producer-consumer channel** between frame capture and FFmpeg encoding. Multiple scenes can be rendered in parallel (each in a separate Chromium tab) and their encoded segments concatenated via FFmpeg's concat demuxer. For multi-format output, frames from a single render can be scaled and encoded to multiple outputs simultaneously.

---

## 3. Project and scene file structure

### Directory layout

```
my-video/
├── project.toml                     # Project configuration
├── scenes/
│   ├── 01-intro.md                  # Scene 1: frontmatter + script
│   ├── 02-problem.md                # Scene 2
│   ├── 03-solution.md               # Scene 3
│   ├── 04-demo/
│   │   ├── scene.md                 # Scene with co-located assets
│   │   └── screenshot.png
│   └── 05-outro.md
├── templates/
│   ├── base.html                    # Base HTML shell
│   ├── components/
│   │   ├── title-card.html          # Reusable visual components
│   │   ├── kinetic-text.html
│   │   ├── slideshow.html
│   │   ├── quote-card.html
│   │   ├── split-screen.html
│   │   ├── lower-third.html
│   │   └── cta-card.html
│   └── transitions/
│       ├── fade.css
│       ├── slide.css
│       └── zoom.css
├── styles/
│   ├── variables.css                # Theme: colors, fonts, spacing
│   ├── typography.css               # @font-face declarations
│   ├── animations.css               # Reusable @keyframes
│   └── format-portrait.css          # 9:16 overrides
├── assets/
│   ├── images/
│   ├── audio/
│   ├── fonts/
│   └── voiceover/                   # Generated TTS audio (gitignored)
├── output/                          # Rendered videos (gitignored)
└── .vidgen/                       # Cache: frames, temp files (gitignored)
```

### Project configuration: `project.toml`

TOML is chosen for the project-level config because it offers explicit typing, comments, and no whitespace sensitivity — critical for configuration that humans and AI agents both modify.

```toml
[project]
name = "How AI Agents Build Videos"
version = "1.0.0"

[video]
fps = 30
default_transition = "fade"
default_transition_duration = 0.5

[video.formats.landscape]
width = 1920
height = 1080
label = "YouTube"

[video.formats.portrait]
width = 1080
height = 1920
label = "Reels/Stories"

[voice]
engine = "native"                     # native | edge | elevenlabs
default_voice = "en_US-amy-medium"
speed = 1.0

[voice.elevenlabs]                    # Only needed if engine = elevenlabs
api_key_env = "ELEVENLABS_API_KEY"
voice_id = "21m00Tcm4TlvDq8ikWAM"

[theme]
primary = "#2563EB"
secondary = "#7C3AED"
background = "#0F172A"
text = "#F8FAFC"
font_heading = "Inter"
font_body = "Inter"

[output]
directory = "./output"
quality = "high"                      # draft | standard | high
```

### Scene file format: markdown + YAML frontmatter

Each scene is a markdown file where the **frontmatter defines the visual and timing configuration** and the **body text becomes the voiceover script**. This separation is the key design insight: AI agents write/modify the frontmatter and script independently, and humans can edit either in any text editor.

```markdown
---
template: title-card
duration: auto # Derived from TTS audio length
transition_in: fade
transition_out: slide-left

background:
  color: "{{theme.background}}"
  image: "@assets/images/gradient.png"

props:
  title: "How AI Agents Build Videos"
  subtitle: "A complete guide"
  title_animation: fade-up
  subtitle_delay: 0.5s

audio:
  music: "@assets/audio/ambient.mp3"
  music_volume: 0.15
---

Welcome to a complete guide on how AI agents can produce professional videos
using nothing but text files and a command line tool.

In the next five minutes, you'll see exactly how this works.
```

**Key frontmatter fields:**

| Field               | Type          | Purpose                                                              |
| ------------------- | ------------- | -------------------------------------------------------------------- |
| `template`          | string        | HTML component to render (`title-card`, `kinetic-text`, `slideshow`) |
| `duration`          | string/number | `auto` (from TTS length), or explicit seconds like `5s`              |
| `transition_in/out` | string        | Transition type: `fade`, `slide-left`, `zoom`, `wipe`, `none`        |
| `background`        | object        | Color, image, gradient, or video background                          |
| `props`             | object        | Template-specific variables injected into HTML                       |
| `audio`             | object        | Background music, sound effects, voice overrides                     |
| `voice`             | string        | Override project default voice for this scene                        |
| `format_overrides`  | object        | Per-format layout adjustments                                        |

**The `auto` duration strategy** is critical for the AI workflow. When `duration: auto`, the engine first generates TTS audio, measures its length, adds a configurable padding (default **0.5s** before and after), and uses that as the scene duration. This means the AI agent never needs to estimate timing — it just writes the script.

### Asset referencing conventions

- **`@assets/...`** resolves to the project's `assets/` directory
- **`./filename`** resolves relative to the scene file (for co-located assets in directory-style scenes)
- **`{{theme.primary}}`** resolves to values from `project.toml`'s `[theme]` section
- **`{{props.title}}`** resolves to the scene's frontmatter props

---

## 4. MCP tool schema design

The MCP server exposes **10 tools**, designed around two principles: **batch operations for common workflows** (create a whole project in one call) and **granular tools for targeted edits** (change one scene's text). This keeps the total tool definition under **4,000 tokens** — small enough to fit comfortably in any LLM's context.

### Tool inventory

| Tool                 | Purpose                                         | Token cost (input) |
| -------------------- | ----------------------------------------------- | ------------------ |
| `create_project`     | Create project with optional inline scenes      | Medium-High        |
| `add_scenes`         | Batch-add scenes to existing project            | Medium             |
| `update_scene`       | Modify a single scene's properties              | Low                |
| `remove_scenes`      | Remove scenes by index                          | Low                |
| `reorder_scenes`     | Change scene order                              | Low                |
| `set_project_config` | Update project settings (voice, theme, formats) | Low                |
| `list_voices`        | List available TTS voices                       | Low                |
| `preview_scene`      | Generate a still frame or short clip preview    | Low                |
| `render`             | Start video rendering (async)                   | Low                |
| `get_project_status` | Get project info, scene list, render status     | Low                |

### Core tool schemas

**`create_project`** — The "happy path" tool. An AI agent can create a complete, ready-to-render project in a single call:

```json
{
  "name": "create_project",
  "description": "Create a video project. Optionally include all scenes inline for single-call video creation. Returns project_id and file paths.",
  "inputSchema": {
    "type": "object",
    "required": ["name", "path"],
    "properties": {
      "name": { "type": "string", "description": "Project display name" },
      "path": {
        "type": "string",
        "description": "Directory path for project files"
      },
      "fps": { "type": "integer", "default": 30 },
      "voice": { "type": "string", "description": "Default TTS voice ID" },
      "theme": {
        "type": "object",
        "properties": {
          "primary": { "type": "string" },
          "background": { "type": "string" },
          "font": { "type": "string" }
        }
      },
      "formats": {
        "type": "array",
        "items": {
          "type": "string",
          "enum": ["landscape", "portrait", "square"]
        },
        "default": ["landscape"]
      },
      "scenes": {
        "type": "array",
        "description": "Inline scene definitions. Each becomes a markdown file.",
        "items": {
          "type": "object",
          "required": ["script"],
          "properties": {
            "template": { "type": "string", "default": "title-card" },
            "script": {
              "type": "string",
              "description": "Voiceover text (becomes markdown body)"
            },
            "duration": { "type": "string", "default": "auto" },
            "props": { "type": "object", "description": "Template variables" },
            "transition": {
              "type": "string",
              "enum": [
                "fade",
                "slide-left",
                "slide-right",
                "zoom",
                "wipe",
                "none"
              ],
              "default": "fade"
            },
            "background_image": { "type": "string" },
            "voice": { "type": "string" }
          }
        }
      }
    }
  }
}
```

**`add_scenes`** — Batch-add scenes to an existing project:

```json
{
  "name": "add_scenes",
  "description": "Add one or more scenes to a project. Appends by default, or insert at position.",
  "inputSchema": {
    "type": "object",
    "required": ["project_path", "scenes"],
    "properties": {
      "project_path": { "type": "string" },
      "insert_at": {
        "type": "integer",
        "description": "0-indexed insert position. Omit to append."
      },
      "scenes": {
        "type": "array",
        "minItems": 1,
        "items": { "$ref": "#/definitions/SceneInput" }
      }
    }
  }
}
```

**`update_scene`** — Surgical edit of a single scene. Only specified fields are modified:

```json
{
  "name": "update_scene",
  "description": "Update one scene. Only provided fields are changed; others preserved.",
  "inputSchema": {
    "type": "object",
    "required": ["project_path", "scene_index"],
    "properties": {
      "project_path": { "type": "string" },
      "scene_index": { "type": "integer" },
      "script": { "type": "string" },
      "template": { "type": "string" },
      "props": { "type": "object" },
      "duration": { "type": "string" },
      "transition": { "type": "string" },
      "voice": { "type": "string" }
    }
  }
}
```

**`render`** — Starts async rendering with progress reporting:

```json
{
  "name": "render",
  "description": "Render video for specified formats. Long-running; returns immediately with render_id. Progress reported via MCP notifications.",
  "inputSchema": {
    "type": "object",
    "required": ["project_path"],
    "properties": {
      "project_path": { "type": "string" },
      "formats": {
        "type": "array",
        "items": {
          "type": "string",
          "enum": ["landscape", "portrait", "square"]
        }
      },
      "quality": {
        "type": "string",
        "enum": ["draft", "standard", "high"],
        "default": "standard"
      },
      "scenes": {
        "type": "array",
        "items": { "type": "integer" },
        "description": "Render only specific scene indices. Omit for all."
      }
    }
  }
}
```

### MCP resources

Resources provide read-only context that AI agents can pull into their working memory:

```
vidgen://projects/{path}                → Project config + scene list summary
vidgen://projects/{path}/scenes/{index} → Full scene markdown content
vidgen://voices                         → Available TTS voices with language/gender
vidgen://templates                      → Available HTML templates with descriptions
vidgen://render/{render_id}             → Render progress (subscribable)
```

### MCP prompts

Two predefined prompts guide agents through common workflows:

- **`create_video_from_topic`** — Takes a topic and target audience, guides the agent to create a complete project
- **`adapt_video_format`** — Takes an existing landscape project and adapts it for portrait/vertical format

### Token efficiency analysis

A typical "create a 5-scene explainer video" workflow requires **2 tool calls**: one `create_project` with all scenes inline, one `render`. The `create_project` input is approximately **500-800 tokens** for 5 scenes. Compare this to a granular API that would need `create_project` + 5× `add_scene` + `set_voice` + `render` = **8 calls, ~1,200+ tokens** plus round-trip overhead. The batch design achieves roughly **60% token reduction** for the common case.

---

## 5. Component system design

### Template architecture

Each component is a self-contained HTML file with companion CSS. Templates use **Mustache-style `{{variable}}`** syntax for prop injection (processed by the Rust engine before loading into Chromium, not by a browser-side JS library). This keeps templates dead simple — no build step, no framework.

**Base HTML shell** (`templates/base.html`):

```html
<!DOCTYPE html>
<html>
  <head>
    <meta charset="utf-8" />
    <style>
      :root {
        --primary: {{theme.primary}};
        --secondary: {{theme.secondary}};
        --background: {{theme.background}};
        --text: {{theme.text}};
        --font-heading: '{{theme.font_heading}}', system-ui;
        --font-body: '{{theme.font_body}}', system-ui;
        --frame: {{currentFrame}};
        --total-frames: {{totalFrames}};
        --progress: calc(var(--frame) / var(--total-frames));
        --duration: {{duration}}s;
      }
      /* Format-specific imports */
      {{#if format_landscape}}@import 'format-landscape.css';{{/if}}
      {{#if format_portrait}}@import 'format-portrait.css';{{/if}}
    </style>
    <link rel="stylesheet" href="variables.css" />
    <link rel="stylesheet" href="typography.css" />
    <link rel="stylesheet" href="animations.css" />
    {{component_styles}}
  </head>
  <body>
    {{component_html}}
  </body>
</html>
```

### Built-in components

The tool ships with **9 core components** covering the most common video content patterns. Each component adapts to both landscape and portrait formats via CSS container queries.

**Title card** — Full-screen title with animated entrance. Props: `title`, `subtitle`, `title_animation` (fade-up, scale-in, typewriter), `subtitle_delay`. The most common component for intros and section headers.

**Kinetic text** — Words appear on screen synchronized to voiceover timing. The engine generates estimated word-level timestamps from TTS audio duration (proportional to character count per word) and injects them as CSS animation delays. Props: `text` (auto-populated from script), `style` (bounce, fade, slide), `highlight_color`. This is the signature component for short-form social content.

**Slideshow** — Image carousel with configurable transitions. Props: `images[]`, `transition` (crossfade, slide, zoom), `duration_per_image`, `ken_burns` (boolean — enables slow zoom/pan). The engine calculates per-image timing from the total scene duration.

**Quote card** — Styled quote with attribution. Props: `quote`, `author`, `background_style` (gradient, image, solid). Uses large serif typography with elegant entrance animations.

**Split screen** — 2-4 panel layout. Props: `panels[]` (each with `content_type`, `src`, `label`), `layout` (50-50, 33-67, grid). Uses CSS Grid with format-aware reflow — side-by-side in landscape, stacked in portrait.

**Lower third** — Name/title overlay at the bottom of the frame. Props: `name`, `title`, `accent_color`, `position` (left, center, right). Animates in from the side with a colored accent bar.

**Caption overlay** — Subtitle-style text synchronized to audio. Props: `captions` (array of `{text, start, end}` or auto-generated from TTS timestamps), `style` (outline, background-box, drop-shadow), `position`.

**CTA card** — End-screen call-to-action. Props: `heading`, `items[]` (each with `icon`, `text`, `url`), `qr_code_url`. Designed as a video outro with channel subscribe, website visit, or other CTAs.

### Multi-format adaptation strategy

Components use **CSS container queries** combined with format-specific style sheets. The rendering engine sets the viewport to the target resolution, and CSS adapts accordingly:

```css
.scene-container {
  container-type: size;
}

@container (aspect-ratio > 1) {
  /* Landscape: horizontal layouts, smaller text relative to width */
  .split-screen {
    grid-template-columns: 1fr 1fr;
  }
  .title-card h1 {
    font-size: clamp(3rem, 5vw, 6rem);
  }
}

@container (aspect-ratio < 1) {
  /* Portrait: vertical stacking, larger text relative to width */
  .split-screen {
    grid-template-rows: 1fr 1fr;
  }
  .title-card h1 {
    font-size: clamp(2.5rem, 8vw, 5rem);
  }
  .title-card .subtitle {
    margin-top: 2rem;
  }
}
```

A scene author can also provide explicit per-format overrides in frontmatter:

```yaml
format_overrides:
  portrait:
    props:
      title_animation: slide-up # Different animation for vertical
    background:
      image: "@assets/images/bg-portrait.png" # Different crop
```

### Custom components

Users create custom components by adding HTML + CSS files to `templates/components/`. The naming convention is the file stem: `my-widget.html` becomes `template: my-widget` in scene frontmatter. A component declares its expected props in an HTML comment at the top:

```html
<!-- props: title (string), items (array), accent_color (string, default: theme.primary) -->
<div class="my-widget" style="--accent: {{accent_color}}">
  <h2>{{title}}</h2>
  <ul>
    {{#each items}}
    <li class="animate-fade-in" style="animation-delay: {{@index}}00ms">
      {{this}}
    </li>
    {{/each}}
  </ul>
</div>
```

---

## 6. Recommended technology stack

### Core dependencies

| Layer                 | Crate / Tool                    | Purpose                                          | Rationale                                                                      |
| --------------------- | ------------------------------- | ------------------------------------------------ | ------------------------------------------------------------------------------ |
| **CLI framework**     | `clap` v4                       | Argument parsing, subcommands                    | Industry standard for Rust CLIs                                                |
| **Async runtime**     | `tokio`                         | Async I/O, browser control, concurrent rendering | Required by chromiumoxide and rmcp                                             |
| **MCP server**        | `rmcp` v0.15+                   | Official MCP Rust SDK, stdio transport           | Maintained under modelcontextprotocol org, macro-based tool definitions        |
| **Browser control**   | `chromiumoxide`                 | Headless Chromium via CDP                        | Full CDP access, async, auto-downloads Chromium                                |
| **FFmpeg**            | `ffmpeg-sidecar` or subprocess  | Video encoding, audio mixing                     | No GPL linking, auto-download, progress parsing                                |
| **TTS (offline)**     | native platform + `edge-tts`    | macOS `say`, Linux `espeak-ng`, Edge neural TTS  | Zero dependency. No model downloads                                            |
| **TTS (cloud)**       | `ureq`                          | ElevenLabs API                                   | Simple REST client, opt-in via API key                                         |
| **Markdown/YAML**     | `pulldown-cmark` + `serde_yaml` | Parse scene files                                | Mature, fast markdown + YAML parsing                                           |
| **TOML**              | `toml`                          | Parse project.toml                               | Standard Rust TOML crate                                                       |
| **Template engine**   | `handlebars` (v6)               | HTML template variable injection                 | Lightweight, Mustache-compatible                                               |
| **Serialization**     | `serde` + `schemars`            | JSON Schema generation for MCP tools             | schemars auto-generates schemas from Rust structs                              |
| **File watching**     | `notify`                        | Watch project files for changes                  | Enables live preview / auto-rebuild                                            |

### External dependencies (auto-managed)

- **Chromium** — Downloaded automatically via `chromiumoxide::BrowserFetcher` on first run (~150MB). Cached in `~/.vidgen/chromium/`.
- **FFmpeg** — Downloaded via `ffmpeg-sidecar`'s auto-download feature or detected from system PATH. Cached in `~/.vidgen/ffmpeg/`.
- **TTS** — Native platform TTS (macOS `say`, Linux `espeak-ng`) requires no downloads. Edge TTS requires `pip install edge-tts`. ElevenLabs requires API key.

### TTS engine abstraction

All TTS backends implement a unified trait, enabling seamless swapping:

```rust
pub trait TtsEngine: Send + Sync {
    /// Synthesize speech from text, return audio file and metadata
    fn synthesize(&self, text: &str, voice: &str, speed: f32) -> Result<SynthesisResult>;
    /// List available voices
    fn list_voices(&self) -> Result<Vec<VoiceInfo>>;
}

pub struct SynthesisResult {
    pub audio_path: PathBuf,
    pub duration_secs: f64,
    pub cached: bool,
    pub word_timestamps: Option<Vec<WordTimestamp>>,
}

pub struct WordTimestamp {
    pub word: String,
    pub start_ms: u64,
    pub end_ms: u64,
}
```

Implementations: `NativeTtsEngine` (macOS `say` / Linux `espeak-ng`), `EdgeTtsEngine` (via `edge-tts` CLI), `ElevenLabsTtsEngine` (via `ureq` HTTP).

---

## 7. Implementation phases

### Phase 1: Foundation (weeks 1–4)

**Goal:** Render a single scene from a markdown file to an MP4 video with TTS voiceover.

- Project file structure: parse `project.toml`, parse scene markdown with YAML frontmatter
- HTML template engine: load base.html, inject props, write to temp file
- Chromium rendering: launch headless browser, load HTML, capture frames via `beginFrame`
- FFmpeg encoding: pipe raw frames to FFmpeg, output H.264 MP4
- Native TTS integration: macOS `say` / Linux `espeak-ng` for voiceover synthesis
- Audio/video mux: combine rendered video with TTS audio via FFmpeg
- CLI: `vidgen render ./my-project/` renders all scenes and concatenates
- Ship with **title-card** and **kinetic-text** components only

**Milestone:** `echo "Hello world" | vidgen quickrender --voice en_US-amy-medium -o hello.mp4` produces a working video.

### Phase 2: MCP server and multi-scene (weeks 5–8)

**Goal:** AI agents can create and render multi-scene projects via MCP tools.

- MCP server via `rmcp` with stdio transport
- Implement all 10 tools (create_project, add_scenes, update_scene, remove_scenes, reorder_scenes, set_project_config, list_voices, preview_scene, render, get_project_status)
- MCP resources for project state and voice listing
- Scene concatenation with transitions (fade, slide, zoom via CSS animations during overlap frames)
- Background music mixing with volume ducking during voiceover
- `duration: auto` — derive scene length from TTS audio
- Progress reporting via MCP notifications during render
- All 8 built-in components implemented

**Milestone:** Claude creates a 5-scene explainer video in 2 tool calls and monitors rendering progress.

### Phase 3: Multi-format and quality (weeks 9–12)

**Goal:** Single project renders to multiple formats.

- Multi-format rendering: landscape (16:9), portrait (9:16), square (1:1)
- CSS container queries and format-specific stylesheets in all built-in components
- Platform-specific encoding presets (YouTube, Instagram Reels, WhatsApp, TikTok)
- `preview_scene` tool returns a still image (base64 or file path)
- `vidgen watch` command for live preview during manual editing

**Milestone:** One project produces both a YouTube landscape video and an Instagram Reel in a single render command.

### Phase 4: Custom components, subtitles, asset downloads, performance (weeks 13–16)

**Goal:** Production-grade features with custom components, subtitles, asset auto-download, and parallel rendering.

- Custom component loading from project's `templates/components/` directory
- `vidgen init` creates `templates/components/` directory with example template
- Subtitle/caption auto-generation with estimated word timestamps and SRT output
- Asset auto-download: URL references in scene frontmatter are downloaded with SHA-256 caching
- Performance: parallel scene rendering (bounded concurrency), frame caching for static scenes
- CLI flags: `--subtitles`, `--parallel`

**Milestone:** Full production pipeline with custom templates, subtitle output, and optimized rendering.

### Scope Exclusions

The following features are **permanently out of scope** for this project:

- **Kokoro TTS** (`kokoro-onnx`) — Excluded due to ONNX runtime complexity and model size. Native platform TTS and Edge TTS provide sufficient quality.
- **OpenAI TTS API** — Excluded to keep cloud TTS options focused. ElevenLabs covers the cloud TTS use case.
- **Voice cloning** (ElevenLabs or any provider) — Excluded due to ethical/legal complexity and limited value for the primary use case of programmatic video creation.

### Known Limitations and Intentional Deviations

- **`vidgen://render/{render_id}` resource** — PRD §4.2 specifies a subscribable render progress resource. This is not implemented because renders use synchronous execution with inline MCP progress notifications (`notifications/progress`) instead. Implementing subscribable render resources would require an async render tracking system (render IDs, background task state storage) that doesn't exist in the current architecture.

- **Frame capture uses `Page.captureScreenshot` with CSS custom properties instead of `HeadlessExperimental.beginFrame`** — `beginFrame` is not available on macOS Chromium builds. Instead, animation progress is driven by CSS custom properties (`--frame`, `--total-frames`, `--progress`) injected into the page before each screenshot. This achieves deterministic frame-by-frame rendering without requiring `beginFrame`.

- **Frames are PNG (not raw RGBA) piped via `image2pipe`** — Instead of raw RGBA bytes on stdin, screenshots are captured as PNG and piped to FFmpeg using the `image2pipe` input format. This avoids the complexity of raw pixel format negotiation while adding negligible overhead at typical video resolutions.

- **TTS uses native platform (`say`/`espeak-ng`) + Edge TTS + ElevenLabs instead of `piper-rs`/`kokoro-onnx`** — The PRD originally specified `piper-rs` + `ort` and `kokoro-onnx` for offline neural TTS. These were replaced with native platform TTS (zero-dependency), Microsoft Edge neural TTS (high quality, no API key), and ElevenLabs (premium cloud option). Kokoro TTS, OpenAI TTS API, and voice cloning are permanently excluded from scope.

- **Audio mixing done per-scene in FFmpeg encoder, not via second-pass `adelay`** — Instead of a two-pass pipeline (video first, then audio mixing with `adelay` offsets), voiceover and background music are mixed into each scene's MP4 during the per-scene encode step. Scenes are then concatenated via FFmpeg concat demuxer. This simplifies the pipeline and enables parallel scene rendering.

- **`hound`/`rubato`/`jsonschema` crates not used** — FFmpeg handles all audio format conversion and resampling (replacing `hound`/`rubato`). Schema validation uses `schemars` for MCP JSON Schema generation (replacing `jsonschema` runtime validation).

- **Templates are self-contained HTML (no `base.html` shell)** — Instead of a shared base HTML shell with CSS includes, each template is a complete standalone HTML document. This simplifies template authoring and eliminates cross-template dependencies. Transitions between scenes use FFmpeg `xfade` filter (not CSS transition files).

- **`content-text` is a bonus 9th template** — Not listed in the original PRD's 7 core + 1 slideshow template list. Added as a general-purpose content slide that fills a common use case between title-card and kinetic-text.

- **`caption-overlay` is a 10th template** — Added to support subtitle-style text overlays with progressive word reveal, complementing the kinetic-text template for different visual use cases.

---

## 8. Example workflows

### AI agent workflow: create a complete video

The following shows the complete MCP interaction for an AI agent creating a YouTube explainer video. **Two tool calls total.**

**Tool call 1: `create_project`**

```json
{
  "name": "Why Rust is Fast",
  "path": "/projects/rust-explainer",
  "formats": ["landscape", "portrait"],
  "voice": "en_US-amy-medium",
  "theme": { "primary": "#FF6B35", "background": "#1a1a2e", "font": "Inter" },
  "scenes": [
    {
      "template": "title-card",
      "script": "Why is Rust so fast? Let's break it down in under three minutes.",
      "props": {
        "title": "Why Rust Is Fast",
        "subtitle": "Explained in 3 minutes"
      }
    },
    {
      "template": "kinetic-text",
      "script": "Rust achieves C-level performance without a garbage collector. It uses a unique ownership system that tracks memory at compile time, eliminating runtime overhead entirely."
    },
    {
      "template": "split-screen",
      "script": "Compare this to languages with garbage collectors. Java, Go, and Python all pause execution periodically to clean up memory. Rust never pauses.",
      "props": {
        "panels": [
          {
            "label": "Rust",
            "content": "Zero-cost abstractions\nNo GC pauses\nPredictable performance"
          },
          {
            "label": "GC Languages",
            "content": "Runtime overhead\nPause-the-world GC\nVariable latency"
          }
        ]
      }
    },
    {
      "template": "kinetic-text",
      "script": "The result? Rust programs routinely match or beat C and C++ in benchmarks, while preventing entire categories of bugs at compile time."
    },
    {
      "template": "cta-card",
      "script": "If you found this helpful, subscribe for more systems programming deep dives.",
      "props": {
        "heading": "Want more?",
        "items": [
          "Subscribe for weekly videos",
          "Link to Rust docs in description"
        ]
      }
    }
  ]
}
```

**Response:**

```json
{
  "project_id": "rust-explainer",
  "path": "/projects/rust-explainer",
  "scenes_created": 5,
  "files": [
    "scenes/01-intro.md",
    "scenes/02-ownership.md",
    "scenes/03-comparison.md",
    "scenes/04-results.md",
    "scenes/05-outro.md"
  ],
  "status": "ready_to_render"
}
```

**Tool call 2: `render`**

```json
{
  "project_path": "/projects/rust-explainer",
  "formats": ["landscape", "portrait"],
  "quality": "high"
}
```

**Response:**

```json
{
  "render_id": "r_a1b2c3",
  "status": "rendering",
  "estimated_duration_secs": 45,
  "outputs": [
    "output/why-rust-is-fast-landscape.mp4",
    "output/why-rust-is-fast-portrait.mp4"
  ]
}
```

The AI agent created a complete 5-scene video project and initiated rendering in **exactly 2 tool calls** and approximately **600 input tokens**.

### Manual editing workflow: modify and re-render

A human editor reviews the AI-generated project and wants to improve scene 3:

**1. Open `scenes/03-comparison.md` in any text editor:**

```markdown
---
template: split-screen
duration: auto
transition_in: fade
transition_out: slide-left
props:
  panels:
    - label: "Rust"
      content: "Zero-cost abstractions\nNo GC pauses\nPredictable performance"
    - label: "GC Languages"
      content: "Runtime overhead\nPause-the-world GC\nVariable latency"
---

Compare this to languages with garbage collectors. Java, Go, and Python
all pause execution periodically to clean up memory. Rust never pauses.
```

**2. Edit the script and props directly:**

Change the body text to a better script, add a third panel, adjust the template. Save the file.

**3. Re-render from the command line:**

```bash
# Re-render just the changed scene (fast iteration)
vidgen render --scenes 3 ./projects/rust-explainer

# Or re-render everything
vidgen render ./projects/rust-explainer

# Preview a single scene as a still frame
vidgen preview --scene 3 ./projects/rust-explainer
```

**4. Watch mode for live iteration:**

```bash
vidgen watch ./projects/rust-explainer
# Monitors all .md, .html, .css files for changes
# Auto-generates a preview frame on save
# Full re-render triggered by explicit command
```

### Hybrid workflow: AI creates, human refines, AI adapts

This is the highest-value workflow the tool enables:

1. **AI creates** the initial project via MCP (2 tool calls)
2. **Human reviews** the generated markdown files, edits scripts for tone and accuracy
3. **Human runs** `vidgen render` to see the result
4. **Human asks AI** to adjust: "Change scene 2 to use a slideshow of 3 architecture diagrams"
5. **AI calls** `update_scene` with `scene_index: 1`, new template, and new props (1 tool call)
6. **Human re-renders** and approves

The markdown files serve as the collaboration contract between human and AI — both can read and write them, and `vidgen render` is the single source of truth for what the output looks like.

---

## 9. FFmpeg encoding presets by platform

The engine includes preset encoding configurations for each major platform, selected via the `formats` array in `project.toml` or the `render` tool:

| Platform        | Resolution | FPS | Codec      | CRF | Preset | Audio            | Flags                    |
| --------------- | ---------- | --- | ---------- | --- | ------ | ---------------- | ------------------------ |
| YouTube HD      | 1920×1080  | 30  | H.264 High | 18  | slow   | AAC 384k/48kHz   | `-movflags +faststart`   |
| YouTube 4K      | 3840×2160  | 30  | H.264 High | 18  | medium | AAC 384k/48kHz   | `-movflags +faststart`   |
| Instagram Reels | 1080×1920  | 30  | H.264 High | 20  | medium | AAC 128k/44.1kHz | max 4GB                  |
| TikTok          | 1080×1920  | 30  | H.264 High | 20  | medium | AAC 128k/44.1kHz |                          |
| WhatsApp        | 720×1280   | 30  | H.264 Main | 26  | fast   | AAC 96k/44.1kHz  | max 16MB, size-optimized |
| YouTube Shorts  | 1080×1920  | 30  | H.264 High | 20  | medium | AAC 256k/48kHz   | max 60s                  |
| Twitter/X       | 1920×1080  | 30  | H.264 High | 22  | medium | AAC 128k/44.1kHz | max 512MB                |

The `quality` parameter maps to CRF offsets: `draft` adds +6 CRF and uses `ultrafast` preset (for quick previews), `standard` uses the table values, `high` subtracts -2 CRF and uses `slow` preset.

---

## 10. Conclusion: what makes this approach novel

Three architectural decisions distinguish vidgen from existing tools.

**First, the markdown-as-scene abstraction eliminates an entire class of complexity.** Remotion requires writing React components. editly requires constructing JSON. MoviePy requires Python scripts. vidgen requires writing a paragraph of text with a few YAML settings. The body text _is_ the voiceover, the frontmatter _is_ the visual config, and the template _is_ the rendering engine. This makes projects genuinely human-editable — not just "technically text-based" like JSON configs that no one wants to hand-edit.

**Second, the MCP-first architecture creates a genuinely token-efficient AI workflow.** The batch `create_project` tool accepts an entire video definition in a single call. The `auto` duration strategy eliminates the need for AI agents to reason about timing. The structured response format gives agents exactly enough context to proceed. This design was shaped by analyzing how existing MCP servers like Pictory handle video creation workflows, then optimizing for the LLM context window.

**Third, HTML/CSS as the rendering surface is simultaneously the most expressive and most accessible visual authoring format available.** Every web developer already knows how to create components. CSS animations run deterministically via CSS custom property injection. Container queries enable genuine multi-format adaptation from a single template. And the Chromium rendering engine — despite its weight — handles typography, SVG, gradients, custom fonts, and complex layouts that no purpose-built Rust renderer could match without years of development. The key insight is that **rendering is a solved problem** (Chromium does it) — the value is in the orchestration layer that turns scenes into videos.

The combination of Rust's single-binary distribution, the `rmcp` MCP SDK, `chromiumoxide`'s full CDP access, and native platform TTS and Edge TTS makes this technically feasible today with a mature dependency chain. The implementation phases are scoped to deliver a working single-scene prototype in 4 weeks and a production-grade multi-format pipeline in 16 weeks.
