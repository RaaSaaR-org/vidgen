use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Subcommand, Debug)]
pub enum AssetAction {
    /// Add an asset to the project (download URL or copy local file)
    Add {
        /// URL or local file path to add
        source: String,

        /// Project directory (default: current directory)
        #[arg(long, short = 'p', default_value = ".")]
        project: PathBuf,

        /// Asset category (determines subdirectory)
        #[arg(long, short = 'c', value_enum, default_value = "images")]
        category: AssetCategory,
    },
}

#[derive(Debug, Clone, ValueEnum)]
pub enum AssetCategory {
    Images,
    Audio,
    Fonts,
}

#[derive(Parser, Debug)]
#[command(
    name = "vidgen",
    about = "AI-agent-first video production CLI — render HTML/CSS scenes via headless Chromium + FFmpeg",
    long_about = "vidgen turns markdown files into videos. Write scenes as .md files with YAML\n\
                  frontmatter, and vidgen renders them through headless Chromium, synthesizes\n\
                  voiceover with TTS, and encodes via FFmpeg. AI agents interact through the\n\
                  built-in MCP server.",
    version,
    after_help = "\x1b[1mExamples:\x1b[0m
  \x1b[36mCreate & render:\x1b[0m
    vidgen init ./my-video                     Create a new project
    vidgen init ./my-video --preset short      Create a 9:16 vertical video project
    vidgen render ./my-video                   Render project to MP4
    vidgen render ./my-video --quality high     High-quality render

  \x1b[36mQuick one-liners:\x1b[0m
    echo \"Hello world\" | vidgen qr -o hello.mp4
    vidgen qr --text \"Breaking news\" -t lower-third -o news.mp4

  \x1b[36mPreview & iterate:\x1b[0m
    vidgen preview ./my-video --scene 2        Preview scene 2, frame 0
    vidgen preview ./my-video --all            Thumbnail all scenes
    vidgen watch ./my-video                    Auto-preview on file changes

  \x1b[36mVideo clips:\x1b[0m
    vidgen clip web https://example.com -p ./my-video -d 5
    vidgen clip youtube \"https://youtu.be/...\" -p ./my-video --from 10 --to 20

  \x1b[36mDebugging:\x1b[0m
    vidgen render ./my-video -v                Verbose output (TTS, encoding details)
    vidgen render ./my-video --debug           Full debug (saves intermediate scene files)
    vidgen render ./my-video --debug-dir /tmp/debug  Custom debug output directory

  \x1b[36mMCP server:\x1b[0m
    vidgen mcp                                 Start MCP server (stdio transport)

\x1b[1mDocumentation:\x1b[0m https://github.com/RaaSaaR-org/vidgen"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Enable verbose output (show TTS details, encoding info, durations)
    #[arg(global = true, long, short = 'v')]
    pub verbose: bool,

    /// Enable debug mode (implies --verbose, saves intermediate scene files)
    #[arg(global = true, long)]
    pub debug: bool,

    /// Directory to save intermediate files when --debug is enabled (default: ./output/debug/)
    #[arg(global = true, long)]
    pub debug_dir: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Initialize a new video project with scenes, templates, and config
    Init {
        /// Path to create the project directory
        path: PathBuf,

        /// Project preset: short (9:16 vertical), recap (16:9 landscape), educational (long-form)
        #[arg(long)]
        preset: Option<String>,
    },

    /// Render a video project to MP4
    #[command(long_about = "Render all scenes in a project to a final MP4 video.\n\n\
        The render pipeline: load scenes -> TTS synthesis -> resolve durations ->\n\
        render frames (Chromium) -> encode (FFmpeg) -> concatenate -> output MP4.\n\n\
        Scene types: HTML templates (rendered via Chromium), video clips (external MP4s),\n\
        and sequence scenes (multiple visuals with a single voiceover).")]
    Render {
        /// Path to the project directory
        path: PathBuf,

        /// Frames per second (overrides project.toml)
        #[arg(long)]
        fps: Option<u32>,

        /// Output quality: draft, standard, high
        #[arg(long, value_name = "LEVEL")]
        quality: Option<String>,

        /// Comma-separated format names to render (e.g. "landscape,portrait").
        /// Only renders matching formats from [video.formats.*] in project.toml
        #[arg(long, value_delimiter = ',')]
        formats: Option<Vec<String>>,

        /// Comma-separated scene indices to render (0-based)
        #[arg(long, value_delimiter = ',')]
        scenes: Option<Vec<usize>>,

        /// Generate SRT subtitle files alongside the video output
        #[arg(long)]
        subtitles: bool,

        /// Burn subtitles into the video (implies --subtitles)
        #[arg(long)]
        burn_in: bool,

        /// Maximum number of scenes to render in parallel (default: 4)
        #[arg(long)]
        parallel: Option<usize>,

        /// Force TTS regeneration, ignoring cached audio files
        #[arg(long)]
        force_tts: bool,

        /// Disable incremental rendering cache (force full re-render of all scenes)
        #[arg(long)]
        no_cache: bool,

        /// Use GPU hardware-accelerated encoding (auto-detects VideoToolbox, NVENC, VAAPI)
        #[arg(long)]
        gpu: bool,

        /// Voice speed override (1.0 = normal, overrides project.toml)
        #[arg(long)]
        speed: Option<f32>,

        /// Post-process crop to aspect ratio (e.g., "9:16", "1:1")
        #[arg(long)]
        crop: Option<String>,
    },

    /// Preview a single frame of a scene as a PNG image
    Preview {
        /// Path to the project directory
        path: PathBuf,

        /// Scene index to preview (0-based)
        #[arg(long, short = 's', default_value_t = 0)]
        scene: usize,

        /// Frame number within the scene (0-based)
        #[arg(long, short = 'f', default_value_t = 0)]
        frame: u32,

        /// Output PNG file path (default: preview.png)
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,

        /// Preview all scenes as numbered PNG thumbnails
        #[arg(long)]
        all: bool,

        /// Generate an animated GIF preview of the scene
        #[arg(long)]
        gif: bool,
    },

    /// Watch project files for changes and auto-preview or re-render
    Watch {
        /// Path to the project directory
        path: PathBuf,

        /// Full re-render mode instead of preview-only
        #[arg(long)]
        render: bool,

        /// Pin to a specific scene index for preview (default: detect changed scene)
        #[arg(long, short = 's')]
        scene: Option<usize>,
    },

    /// Manage project assets (images, audio, fonts)
    Asset {
        #[command(subcommand)]
        action: AssetAction,
    },

    /// Quick render: pipe text in, get an MP4 out (single auto-duration scene)
    #[command(
        alias = "qr",
        long_about = "Render a single scene from text input, without creating a full project.\n\
            Reads text from --text or stdin and produces an MP4 with voiceover.\n\n\
            Examples:\n\
            \x20 echo \"Hello world\" | vidgen qr -o hello.mp4\n\
            \x20 vidgen qr --text \"Breaking news\" -t lower-third -o news.mp4"
    )]
    QuickRender {
        /// Template name for the scene
        #[arg(long, short = 't', default_value = "title-card")]
        template: String,

        /// Output MP4 file path
        #[arg(long, short = 'o', default_value = "output.mp4")]
        output: PathBuf,

        /// Text/script for the scene (if omitted, reads from stdin)
        #[arg(long)]
        text: Option<String>,

        /// Voice ID for TTS
        #[arg(long)]
        voice: Option<String>,

        /// Output quality: draft, standard, high
        #[arg(long)]
        quality: Option<String>,

        /// Template props as JSON string (e.g. '{"title":"Hello"}')
        #[arg(long)]
        props: Option<String>,
    },

    /// List and preview available templates
    Templates {
        /// Project path (optional — shows project templates in addition to built-ins)
        #[arg(long, short = 'p')]
        project: Option<PathBuf>,

        /// Output directory for thumbnail previews
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
    },

    /// Export scenes as images (PNG), animated GIFs, WebP, MP4, audio, or subtitles
    Export {
        /// Path to the project directory
        path: PathBuf,
        #[command(subcommand)]
        format: ExportAction,
    },

    /// Show project info, scene list, and estimated timing (runs TTS but no rendering)
    Info {
        /// Path to the project directory
        path: PathBuf,
    },

    /// Validate project for common issues (missing templates, fonts, assets, timing, contrast)
    Validate {
        /// Path to the project directory
        path: PathBuf,
    },

    /// Show what changed since last render (text changes, duration differences)
    Diff {
        /// Path to the project directory
        path: PathBuf,
    },

    /// Run visual regression tests against stored snapshots
    Test {
        /// Path to the project directory
        path: PathBuf,
        /// Update reference snapshots instead of comparing
        #[arg(long)]
        update: bool,
    },

    /// Start an MCP server over stdio for AI agent integration
    #[command(long_about = "Start a Model Context Protocol (MCP) server on stdin/stdout.\n\
        AI agents (like Claude) connect via this transport to create and render videos\n\
        programmatically. The server exposes tools for project management, scene editing,\n\
        and rendering.")]
    Mcp,

    /// Capture video clips from websites or YouTube
    #[cfg(any(feature = "clipper", feature = "youtube"))]
    #[command(long_about = "Capture video clips for use in vidgen projects.\n\n\
        Clips are saved to assets/clips/ and can be referenced in scenes via:\n\
        \x20 video_source: \"@assets/clips/filename.mp4\"\n\n\
        Features:\n\
        \x20 clipper  - vidgen clip web (scrolling website capture via Chromium)\n\
        \x20 youtube  - vidgen clip youtube (download + trim via yt-dlp)")]
    Clip {
        #[command(subcommand)]
        action: ClipAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum ExportAction {
    /// Export as PNG image (single frame)
    #[command(alias = "png")]
    Image {
        #[arg(long, short = 's')]
        scene: Option<usize>,
        #[arg(long, short = 'f', default_value_t = 0)]
        frame: u32,
        #[arg(long, short = 'p')]
        progress: Option<f32>,
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        smart: bool,
        #[arg(long)]
        open: bool,
    },
    /// Export as animated GIF (looping)
    Gif {
        #[arg(long, short = 's')]
        scene: Option<usize>,
        #[arg(long, short = 'd', default_value_t = 3.0)]
        duration: f32,
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
        #[arg(long)]
        all: bool,
        #[arg(long, short = 'w')]
        width: Option<u32>,
        #[arg(long)]
        combined: bool,
        #[arg(long)]
        open: bool,
    },
    /// Export as animated WebP
    Webp {
        #[arg(long, short = 's')]
        scene: Option<usize>,
        #[arg(long, short = 'd', default_value_t = 3.0)]
        duration: f32,
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
        #[arg(long)]
        all: bool,
        #[arg(long, short = 'w')]
        width: Option<u32>,
        #[arg(long)]
        open: bool,
    },
    /// Export a single scene as standalone MP4
    Mp4 {
        #[arg(long, short = 's')]
        scene: Option<usize>,
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
        #[arg(long)]
        force_tts: bool,
    },
    /// Export voiceover audio only (WAV)
    Audio {
        #[arg(long, short = 's')]
        scene: Option<usize>,
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
    },
    /// Export subtitles as SRT file
    #[command(alias = "srt")]
    Subtitles {
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
    },
}

#[cfg(any(feature = "clipper", feature = "youtube"))]
#[derive(Subcommand, Debug)]
pub enum ClipAction {
    /// Capture a scrolling video of a website
    #[cfg(feature = "clipper")]
    #[command(long_about = "Navigate to a URL in headless Chromium, scroll the page, and capture\n\
        screenshots frame-by-frame into an MP4. The clip is saved to assets/clips/.\n\n\
        Example:\n\
        \x20 vidgen clip web https://example.com -p ./my-video -d 5 --scroll-speed 150")]
    Web {
        /// URL of the website to capture
        url: String,

        /// Project directory (clips saved to assets/clips/)
        #[arg(long, short = 'p', default_value = ".")]
        project: PathBuf,

        /// Duration of the scroll capture in seconds
        #[arg(long, short = 'd', default_value_t = 10.0)]
        duration: f64,

        /// Viewport width
        #[arg(long, default_value_t = 1920)]
        width: u32,

        /// Viewport height
        #[arg(long, default_value_t = 1080)]
        height: u32,

        /// Scroll speed in pixels per second
        #[arg(long, default_value_t = 200)]
        scroll_speed: u32,

        /// Output filename (saved in assets/clips/)
        #[arg(long, short = 'o')]
        output: Option<String>,

        /// Delay in seconds before starting scroll (let page load)
        #[arg(long, default_value_t = 2.0)]
        wait: f64,

        /// Frames per second for the capture
        #[arg(long, default_value_t = 30)]
        fps: u32,
    },

    /// Download and trim a clip from YouTube (auto-downloads yt-dlp binary on first run)
    #[cfg(feature = "youtube")]
    #[command(long_about = "Download a YouTube video and optionally trim it to a time range.\n\
        The yt-dlp binary is auto-downloaded on first use (~/.vidgen/libs/).\n\
        Output is always re-encoded to H.264+AAC for universal playback.\n\n\
        Example:\n\
        \x20 vidgen clip youtube \"https://youtu.be/dQw4w9WgXcQ\" -p ./my-video --from 10 --to 20")]
    Youtube {
        /// YouTube video URL
        url: String,

        /// Project directory (clips saved to assets/clips/)
        #[arg(long, short = 'p', default_value = ".")]
        project: PathBuf,

        /// Start time in seconds (e.g., 65.0 for 1:05)
        #[arg(long)]
        from: Option<f64>,

        /// End time in seconds
        #[arg(long)]
        to: Option<f64>,

        /// Output filename (saved in assets/clips/)
        #[arg(long, short = 'o')]
        output: Option<String>,
    },
}
