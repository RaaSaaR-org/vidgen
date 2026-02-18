use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum VidgenError {
    #[error("Project not found: {0}")]
    ProjectNotFound(PathBuf),

    #[error("Config file not found: {0}")]
    ConfigNotFound(PathBuf),

    #[error("Failed to parse config: {0}")]
    ConfigParse(String),

    #[error("Scene file error in {path}: {message}")]
    SceneParse { path: PathBuf, message: String },

    #[error("No scenes found in {0}")]
    NoScenes(PathBuf),

    #[error("Template not found: {0}")]
    TemplateNotFound(String),

    #[error("Template render error: {0}")]
    TemplateRender(String),

    #[error("Browser error: {0}")]
    Browser(String),

    #[error("FFmpeg error: {0}")]
    Ffmpeg(String),

    #[error("Scene index out of range: {index} (project has {count} scenes)")]
    SceneIndexOutOfRange { index: usize, count: usize },

    #[error("Invalid scene order: {0}")]
    InvalidSceneOrder(String),

    #[error("Already initialized: {0} already exists")]
    AlreadyInitialized(PathBuf),

    #[error("TTS error: {0}")]
    Tts(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

impl VidgenError {
    /// Return an actionable hint for the user, if applicable.
    pub fn hint(&self) -> Option<String> {
        match self {
            VidgenError::ProjectNotFound(_) => Some(
                "Run 'vidgen init <path>' to create a new project, or check the path.".into(),
            ),
            VidgenError::ConfigNotFound(_) => Some(
                "A valid project needs a project.toml file. Run 'vidgen init' to create one.".into(),
            ),
            VidgenError::NoScenes(_) => Some(
                "Add .md files to the scenes/ directory. Run 'vidgen init' for a starter project."
                    .into(),
            ),
            VidgenError::ConfigParse(msg) => {
                if msg.contains("missing field") {
                    Some("Ensure your project.toml has a [project] section with at least 'name'. Run 'vidgen init' for a valid example.".into())
                } else {
                    Some("Check project.toml syntax. Run 'vidgen init <path>' to generate a valid example config.".into())
                }
            }
            VidgenError::SceneParse { message, .. } => {
                if message.contains("template") {
                    Some("Built-in templates: title-card, content-text, quote-card, split-screen, lower-third, cta-card, kinetic-text, slideshow, caption-overlay. Custom templates go in templates/components/.".into())
                } else if message.contains("frontmatter") || message.contains("---") {
                    Some("Scene files need YAML frontmatter between --- delimiters at the top of the file.".into())
                } else {
                    Some("Check YAML syntax in the scene frontmatter. Keys must be properly indented and values properly quoted.".into())
                }
            }
            VidgenError::TemplateNotFound(_) => Some(
                "Built-in templates: title-card, content-text, quote-card, split-screen, lower-third, cta-card, kinetic-text, slideshow, caption-overlay. Custom templates go in templates/components/.".into(),
            ),
            VidgenError::Browser(_) => Some(
                "Ensure Chromium/Chrome is installed, or let chromiumoxide download it automatically."
                    .into(),
            ),
            VidgenError::Ffmpeg(_) => Some(
                "Ensure FFmpeg is installed and on your PATH. Install via: brew install ffmpeg (macOS) or apt install ffmpeg (Linux).".into(),
            ),
            VidgenError::SceneIndexOutOfRange { .. } => Some(
                "Scene indices are 0-based. Use get_project_status to see available scenes.".into(),
            ),
            VidgenError::InvalidSceneOrder(_) => Some(
                "Provide a complete permutation of scene indices (0-based).".into(),
            ),
            VidgenError::AlreadyInitialized(_) => Some(
                "Use a different path, or delete the existing project first.".into(),
            ),
            VidgenError::Tts(_) => Some(
                "Ensure a TTS engine is available. macOS: 'say' (built-in). Linux: install espeak-ng. For neural voices: pip install edge-tts. For local neural TTS: install piper (https://github.com/rhasspy/piper). For ElevenLabs: set ELEVEN_API_KEY env var or add it to .env in your project".into(),
            ),
            _ => None,
        }
    }
}

pub type VidgenResult<T> = Result<T, VidgenError>;
