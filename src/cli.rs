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
    version,
    after_help = "\x1b[1mExamples:\x1b[0m
  vidgen init ./my-video           Create a new project
  vidgen render ./my-video         Render project to MP4
  vidgen render ./my-video --fps 60 --quality high  High-quality render
  vidgen preview ./my-video --scene 2 --frame 50   Preview a specific frame
  vidgen watch ./my-video          Watch for changes and auto-preview
  echo \"Hello world\" | vidgen quickrender -o hello.mp4   Quick single-scene render"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Initialize a new video project
    Init {
        /// Path to create the project directory
        path: PathBuf,
    },
    /// Start an MCP server over stdio for AI agent integration
    Mcp,
    /// Render a video project to MP4
    Render {
        /// Path to the project directory
        path: PathBuf,

        /// Frames per second (overrides project.toml)
        #[arg(long)]
        fps: Option<u32>,

        /// Output quality: draft, standard, high
        #[arg(long)]
        quality: Option<String>,

        /// Comma-separated format names to render (e.g. "landscape,portrait")
        /// Only renders matching formats from [video.formats.*] in project.toml.
        /// If omitted, renders all formats (or the default single format).
        #[arg(long, value_delimiter = ',')]
        formats: Option<Vec<String>>,

        /// Comma-separated scene indices to render (0-based). If omitted, renders all scenes.
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
    },
    /// Preview a single frame of a scene as a PNG image
    Preview {
        /// Path to the project directory
        path: PathBuf,

        /// 0-based scene index to preview
        #[arg(long, short = 's', default_value_t = 0)]
        scene: usize,

        /// 0-based frame number within the scene
        #[arg(long, short = 'f', default_value_t = 0)]
        frame: u32,

        /// Output PNG file path (default: preview.png)
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
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
    #[command(alias = "qr")]
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
}
