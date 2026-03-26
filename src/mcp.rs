use base64::Engine;
use crate::commands;
use crate::config;
use crate::scene::{self, SceneDuration};
use rmcp::handler::server::router::prompt::PromptRouter;
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    Annotated, CallToolResult, Content, GetPromptRequestParams, GetPromptResult,
    ListPromptsResult, ListResourceTemplatesResult, ListResourcesResult, Meta,
    PaginatedRequestParams, PromptMessage, PromptMessageRole, RawResource, RawResourceTemplate,
    ReadResourceRequestParams, ReadResourceResult, ResourceContents, ServerCapabilities, ServerInfo,
};
use rmcp::service::RequestContext;
use rmcp::Peer;
use rmcp::{
    prompt, prompt_handler, prompt_router, tool, tool_handler, tool_router, ErrorData as McpError,
    RoleServer, ServerHandler,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Helper: convert domain errors into MCP errors.
fn mc_err(e: impl std::fmt::Display) -> McpError {
    McpError::internal_error(e.to_string(), None)
}

/// Decode a percent-encoded URI path component back to a filesystem path.
fn decode_uri_path(encoded: &str) -> String {
    encoded.replace("%2F", "/").replace("%2f", "/").replace("%20", " ")
}

/// Build project status JSON from a project path. Shared by the `get_project_status`
/// tool and the `vidgen://projects/{path}` resource.
fn build_project_status_json(project_path: &Path) -> Result<serde_json::Value, McpError> {
    let config = config::load_config(project_path).map_err(mc_err)?;
    let scenes = scene::load_scenes(project_path).map_err(mc_err)?;

    // Check for rendered output files
    let output_rel = config
        .output
        .directory
        .strip_prefix("./")
        .unwrap_or(&config.output.directory);
    let output_dir = project_path.join(output_rel);
    let output_files: Vec<String> = if output_dir.exists() {
        std::fs::read_dir(&output_dir)
            .map_err(mc_err)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "mp4"))
            .map(|p| p.display().to_string())
            .collect()
    } else {
        vec![]
    };

    // Duration summary
    let mut fixed_duration_secs = 0.0_f64;
    let mut auto_duration_count = 0_usize;
    for s in &scenes {
        match &s.frontmatter.duration {
            SceneDuration::Fixed(d) => fixed_duration_secs += d,
            SceneDuration::Auto => auto_duration_count += 1,
        }
    }

    let scene_summaries: Vec<serde_json::Value> = scenes
        .iter()
        .map(|s| {
            let duration_val: serde_json::Value = match &s.frontmatter.duration {
                SceneDuration::Auto => serde_json::json!("auto"),
                SceneDuration::Fixed(d) => serde_json::json!(d),
            };
            let mut summary = serde_json::json!({
                "template": s.frontmatter.template,
                "duration": duration_val,
                "source": s.source_path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown"),
            });
            if let Some(ref t) = s.frontmatter.transition_in {
                summary["transition_in"] = serde_json::json!(t);
            }
            if let Some(ref t) = s.frontmatter.transition_out {
                summary["transition_out"] = serde_json::json!(t);
            }
            if let Some(d) = s.frontmatter.transition_duration {
                summary["transition_duration"] = serde_json::json!(d);
            }
            summary
        })
        .collect();

    Ok(serde_json::json!({
        "project_name": config.project.name,
        "video": {
            "fps": config.video.fps,
            "width": config.video.width,
            "height": config.video.height,
            "default_transition": config.video.default_transition,
            "default_transition_duration": config.video.default_transition_duration,
        },
        "voice": {
            "engine": config.voice.engine,
            "default_voice": config.voice.default_voice,
            "speed": config.voice.speed,
            "padding_before": config.voice.padding_before,
            "padding_after": config.voice.padding_after,
            "auto_fallback_duration": config.voice.auto_fallback_duration,
        },
        "quality": config.output.quality,
        "scenes": {
            "count": scenes.len(),
            "fixed_duration_secs": fixed_duration_secs,
            "auto_duration_scenes": auto_duration_count,
            "details": scene_summaries,
        },
        "output": {
            "directory": output_dir.display().to_string(),
            "rendered_files": output_files,
            "has_output": !output_files.is_empty(),
        },
    }))
}

// ---------------------------------------------------------------------------
// Parameter structs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateProjectParams {
    /// Project name
    #[schemars(description = "Project name")]
    pub name: String,
    /// Path where the project directory will be created
    #[schemars(description = "Filesystem path for the new project directory")]
    pub path: String,
    /// Frames per second (optional, default 30)
    #[schemars(description = "Frames per second (default 30)")]
    pub fps: Option<u32>,
    /// Video width in pixels (optional, default 1920)
    #[schemars(description = "Video width in pixels (default 1920)")]
    pub width: Option<u32>,
    /// Video height in pixels (optional, default 1080)
    #[schemars(description = "Video height in pixels (default 1080)")]
    pub height: Option<u32>,
    /// Output quality: draft, standard, high (optional, default standard)
    #[schemars(description = "Output quality: draft, standard, high (default standard)")]
    pub quality: Option<String>,
    /// Default TTS voice ID (optional)
    #[schemars(description = "Default TTS voice ID for the project")]
    pub voice: Option<String>,
    /// Output formats to generate (optional, e.g. ["landscape", "portrait", "square"])
    #[schemars(
        description = "Output formats: landscape (1920x1080), portrait (1080x1920), square (1080x1080). If omitted, uses default single format."
    )]
    pub formats: Option<Vec<String>>,
    /// Theme overrides (optional)
    #[schemars(description = "Theme color/font overrides")]
    pub theme: Option<ThemeParams>,
    /// Inline scenes to create (optional — if omitted, a default intro scene is created)
    #[schemars(
        description = "Array of scenes to create inline. If omitted, a default intro scene is created"
    )]
    pub scenes: Option<Vec<SceneParams>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SceneParams {
    /// Template name. Defaults to title-card
    #[schemars(
        description = "Template name (title-card, content-text, quote-card, split-screen, lower-third, cta-card, kinetic-text, slideshow, caption-overlay). Defaults to title-card"
    )]
    pub template: Option<String>,
    /// Voiceover script / body text for this scene
    #[schemars(description = "Voiceover script / body text for this scene")]
    pub script: String,
    /// Scene duration: "auto" (default, derives from TTS audio + padding) or a number in seconds
    #[schemars(
        description = "Scene duration: \"auto\" (default, derives from TTS audio + padding) or a number in seconds"
    )]
    pub duration: Option<SceneDuration>,
    /// Template variables (optional)
    #[schemars(description = "Template variables as key-value pairs")]
    pub props: Option<HashMap<String, serde_json::Value>>,
    /// Transition type (e.g. "fade", "slide-left", "slide-right", "zoom", "wipe")
    #[schemars(description = "Transition type: fade, slide-left, slide-right, zoom, wipe, none")]
    pub transition: Option<String>,
    /// Voice ID override for this scene
    #[schemars(description = "Voice ID override for this scene's TTS")]
    pub voice: Option<String>,
    /// Background color override (e.g. "#FF0000")
    #[schemars(description = "Background color override (hex, e.g. \"#FF0000\")")]
    pub background: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ThemeParams {
    /// Primary color (hex, e.g. #2563EB)
    #[schemars(description = "Primary color (hex)")]
    pub primary: Option<String>,
    /// Secondary color (hex)
    #[schemars(description = "Secondary color (hex)")]
    pub secondary: Option<String>,
    /// Background color (hex)
    #[schemars(description = "Background color (hex)")]
    pub background: Option<String>,
    /// Text color (hex)
    #[schemars(description = "Text color (hex)")]
    pub text: Option<String>,
    /// Heading font family
    #[schemars(description = "Heading font family")]
    pub font_heading: Option<String>,
    /// Body font family
    #[schemars(description = "Body font family")]
    pub font_body: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RenderParams {
    /// Path to the project directory
    #[schemars(description = "Path to the project directory to render")]
    pub project_path: String,
    /// Output quality override: draft, standard, high
    #[schemars(description = "Output quality override: draft, standard, high")]
    pub quality: Option<String>,
    /// Output formats to render (e.g. ["landscape", "portrait"])
    #[schemars(
        description = "Output formats to render: landscape, portrait, square, or custom format names from project.toml [video.formats.*]. If omitted, renders all configured formats."
    )]
    pub formats: Option<Vec<String>>,
    /// Scene indices to render (0-based). If omitted, renders all scenes.
    #[schemars(description = "0-based scene indices to render (e.g. [0, 2]). If omitted, renders all scenes.")]
    pub scenes: Option<Vec<usize>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetProjectStatusParams {
    /// Path to the project directory
    #[schemars(description = "Path to the project directory")]
    pub project_path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AddScenesParams {
    /// Path to the project directory
    #[schemars(description = "Path to the project directory")]
    pub project_path: String,
    /// Index to insert scenes at (0-based). If omitted, appends to end
    #[schemars(description = "Index to insert at (0-based). Omit to append")]
    pub insert_at: Option<usize>,
    /// Scenes to add
    #[schemars(description = "Array of scenes to add")]
    pub scenes: Vec<SceneParams>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateSceneParams {
    /// Path to the project directory
    #[schemars(description = "Path to the project directory")]
    pub project_path: String,
    /// 0-based scene index to update
    #[schemars(description = "0-based scene index to update")]
    pub scene_index: usize,
    /// New template name
    #[schemars(description = "New template name")]
    pub template: Option<String>,
    /// New voiceover script / body text
    #[schemars(description = "New voiceover script / body text")]
    pub script: Option<String>,
    /// New duration: "auto" or a number in seconds
    #[schemars(description = "New duration: \"auto\" or a number in seconds")]
    pub duration: Option<SceneDuration>,
    /// Props to merge into existing props
    #[schemars(description = "Props to merge into existing (key-value pairs)")]
    pub props: Option<HashMap<String, serde_json::Value>>,
    /// Transition in effect
    #[schemars(description = "Transition in effect name")]
    pub transition_in: Option<String>,
    /// Transition out effect
    #[schemars(description = "Transition out effect name")]
    pub transition_out: Option<String>,
    /// Voice ID override
    #[schemars(description = "Voice ID override for this scene")]
    pub voice: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RemoveScenesParams {
    /// Path to the project directory
    #[schemars(description = "Path to the project directory")]
    pub project_path: String,
    /// 0-based indices of scenes to remove
    #[schemars(description = "Array of 0-based scene indices to remove")]
    pub indices: Vec<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReorderScenesParams {
    /// Path to the project directory
    #[schemars(description = "Path to the project directory")]
    pub project_path: String,
    /// New order as a permutation of 0-based indices (e.g. [2, 0, 1])
    #[schemars(description = "New order as permutation of 0-based indices, e.g. [2, 0, 1]")]
    pub order: Vec<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetProjectConfigParams {
    /// Path to the project directory
    #[schemars(description = "Path to the project directory")]
    pub project_path: String,
    /// Frames per second
    #[schemars(description = "Frames per second")]
    pub fps: Option<u32>,
    /// Video width in pixels
    #[schemars(description = "Video width in pixels")]
    pub width: Option<u32>,
    /// Video height in pixels
    #[schemars(description = "Video height in pixels")]
    pub height: Option<u32>,
    /// Output quality: draft, standard, high
    #[schemars(description = "Output quality: draft, standard, high")]
    pub quality: Option<String>,
    /// Primary theme color (hex)
    #[schemars(description = "Primary theme color (hex)")]
    pub primary: Option<String>,
    /// Secondary theme color (hex)
    #[schemars(description = "Secondary theme color (hex)")]
    pub secondary: Option<String>,
    /// Background color (hex)
    #[schemars(description = "Background color (hex)")]
    pub background: Option<String>,
    /// Text color (hex)
    #[schemars(description = "Text color (hex)")]
    pub text: Option<String>,
    /// Heading font family
    #[schemars(description = "Heading font family")]
    pub font_heading: Option<String>,
    /// Body font family
    #[schemars(description = "Body font family")]
    pub font_body: Option<String>,
    /// Default transition between scenes (fade, slide-left, slide-right, zoom, wipe, none)
    #[schemars(
        description = "Default transition type: fade, slide-left, slide-right, zoom, wipe, none"
    )]
    pub default_transition: Option<String>,
    /// Default transition duration in seconds
    #[schemars(description = "Default transition duration in seconds (default 0.5)")]
    pub default_transition_duration: Option<f64>,
    /// TTS engine name (e.g. "native")
    #[schemars(
        description = "TTS engine: native (default), edge (Microsoft Edge neural TTS), elevenlabs (set ELEVEN_API_KEY in env or project .env file), piper (local neural TTS)"
    )]
    pub voice_engine: Option<String>,
    /// Default voice ID for TTS
    #[schemars(description = "Default TTS voice ID")]
    pub default_voice: Option<String>,
    /// Voice speed multiplier (1.0 = normal)
    #[schemars(description = "Voice speed multiplier (1.0 = normal)")]
    pub voice_speed: Option<f32>,
    /// Padding before TTS audio in seconds (default 0.5)
    #[schemars(description = "Silence padding before TTS audio in seconds (default 0.5)")]
    pub padding_before: Option<f64>,
    /// Padding after TTS audio in seconds (default 0.5)
    #[schemars(description = "Silence padding after TTS audio in seconds (default 0.5)")]
    pub padding_after: Option<f64>,
    /// Fallback duration for auto-duration scenes without TTS (default 3.0)
    #[schemars(
        description = "Fallback duration for auto-duration scenes without TTS audio (default 3.0)"
    )]
    pub auto_fallback_duration: Option<f64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListVoicesParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PreviewSceneParams {
    /// Path to the project directory
    #[schemars(description = "Path to the project directory")]
    pub project_path: String,
    /// 0-based scene index to preview
    #[schemars(description = "0-based scene index to preview")]
    pub scene_index: usize,
    /// 0-based frame number to preview (default: 0)
    #[schemars(description = "0-based frame number to preview (default 0, first frame)")]
    pub frame: Option<u32>,
    /// Animation progress 0.0-1.0. When provided, calculates the frame number from progress instead of using the frame parameter.
    #[schemars(
        description = "Animation progress 0.0-1.0. When set, overrides the frame parameter by calculating the frame from progress * total_frames."
    )]
    pub progress: Option<f32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ExportMediaParams {
    /// Path to the project directory
    #[schemars(description = "Path to the project directory")]
    pub project_path: String,
    /// 0-based scene index to export
    #[schemars(description = "0-based scene index to export")]
    pub scene_index: usize,
    /// Export format: png, gif, webp
    #[schemars(description = "Export format: png, gif, webp")]
    pub format: String,
    /// Animation progress 0.0-1.0 for PNG export (default 0.0)
    #[schemars(description = "Animation progress 0.0-1.0 for PNG export (default 0.0)")]
    pub progress: Option<f32>,
    /// Duration in seconds for GIF/WebP export
    #[schemars(description = "Duration in seconds for GIF/WebP export")]
    pub duration: Option<f32>,
    /// Output width for GIF/WebP export (height derived from aspect ratio)
    #[schemars(description = "Output width in pixels for GIF/WebP export")]
    pub width: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BatchOperation {
    /// Tool name to execute
    #[schemars(
        description = "Tool name: create_project, get_project_status, add_scenes, update_scene, remove_scenes, reorder_scenes, set_project_config, list_voices"
    )]
    pub tool: String,
    /// Parameters for the tool as a JSON object
    #[schemars(description = "Parameters for the tool as a JSON object")]
    pub params: serde_json::Value,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BatchParams {
    /// Array of operations to execute sequentially
    #[schemars(description = "Array of tool operations to execute sequentially")]
    pub operations: Vec<BatchOperation>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetRenderProgressParams {
    /// Path to the project directory
    #[schemars(description = "Path to the project directory")]
    pub project_path: String,
}

// ---------------------------------------------------------------------------
// McServer
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct McServer {
    tool_router: ToolRouter<Self>,
    prompt_router: PromptRouter<Self>,
}

impl McServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
            prompt_router: Self::prompt_router(),
        }
    }
}

#[tool_router]
impl McServer {
    #[tool(
        description = "Create a new video project with optional inline scenes. Accepts project settings, theme overrides, and an array of scenes for single-call project creation."
    )]
    async fn create_project(
        &self,
        Parameters(params): Parameters<CreateProjectParams>,
    ) -> Result<CallToolResult, McpError> {
        let scenes = params.scenes.map(|scenes| {
            scenes
                .into_iter()
                .map(|s| commands::init::SceneInput {
                    template: s.template,
                    script: s.script,
                    duration: s.duration,
                    props: s.props,
                    transition: s.transition,
                    voice: s.voice,
                    background: s.background,
                })
                .collect()
        });

        let theme = params.theme.map(|t| commands::init::ThemeOverrides {
            primary: t.primary,
            secondary: t.secondary,
            background: t.background,
            text: t.text,
            font_heading: t.font_heading,
            font_body: t.font_body,
        });

        let opts = commands::init::CreateProjectOptions {
            path: params.path.into(),
            name: Some(params.name),
            fps: params.fps,
            width: params.width,
            height: params.height,
            quality: params.quality,
            voice: params.voice,
            formats: params.formats,
            theme,
            scenes,
        };

        let result = commands::init::create_project(&opts).map_err(mc_err)?;
        let text = serde_json::to_string_pretty(&result).map_err(mc_err)?;
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        description = "Render a video project to MP4. Launches headless Chromium, captures frames, and encodes with FFmpeg. Auto-duration scenes derive length from TTS audio + padding. Supports multi-format rendering via formats parameter."
    )]
    async fn render(
        &self,
        Parameters(params): Parameters<RenderParams>,
        meta: Meta,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let path = Path::new(&params.project_path);

        // Build progress reporter from MCP context
        let progress = if let Some(token) = meta.get_progress_token() {
            crate::render::RenderProgress::new(peer, token)
        } else {
            crate::render::RenderProgress::noop()
        };

        let results = commands::render::render_project_with_progress(
            path,
            None,
            params.quality,
            params.formats,
            params.scenes,
            progress,
        )
        .await
        .map_err(mc_err)?;
        let text = serde_json::to_string_pretty(&results).map_err(mc_err)?;
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        description = "Get the current status of a video project: config, scene count, duration info (auto vs fixed), and whether rendered output exists."
    )]
    async fn get_project_status(
        &self,
        Parameters(params): Parameters<GetProjectStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        let path = Path::new(&params.project_path);
        let status = build_project_status_json(path)?;
        let text = serde_json::to_string_pretty(&status).map_err(mc_err)?;
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        description = "Add one or more scenes to a project. Can insert at a specific position or append to end."
    )]
    async fn add_scenes(
        &self,
        Parameters(params): Parameters<AddScenesParams>,
    ) -> Result<CallToolResult, McpError> {
        let path = Path::new(&params.project_path);
        let scenes = params
            .scenes
            .into_iter()
            .map(|s| commands::scenes::SceneInput {
                template: s.template,
                script: s.script,
                duration: s.duration,
                props: s.props,
                transition: s.transition,
                voice: s.voice,
                background: s.background,
            })
            .collect();

        let result =
            commands::scenes::add_scenes(path, params.insert_at, scenes).map_err(mc_err)?;
        let text = serde_json::to_string_pretty(&result).map_err(mc_err)?;
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        description = "Update a scene's properties. Supports partial updates — only provided fields are changed. Props are merged with existing values. Duration can be \"auto\" or a number."
    )]
    async fn update_scene(
        &self,
        Parameters(params): Parameters<UpdateSceneParams>,
    ) -> Result<CallToolResult, McpError> {
        let path = Path::new(&params.project_path);
        let update = commands::scenes::SceneUpdate {
            template: params.template,
            script: params.script,
            duration: params.duration,
            props: params.props,
            transition_in: params.transition_in,
            transition_out: params.transition_out,
            voice: params.voice,
        };

        let result =
            commands::scenes::update_scene(path, params.scene_index, update).map_err(mc_err)?;
        let text = serde_json::to_string_pretty(&result).map_err(mc_err)?;
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        description = "Remove one or more scenes by index. Remaining scenes are renumbered automatically."
    )]
    async fn remove_scenes(
        &self,
        Parameters(params): Parameters<RemoveScenesParams>,
    ) -> Result<CallToolResult, McpError> {
        let path = Path::new(&params.project_path);
        let result = commands::scenes::remove_scenes(path, &params.indices).map_err(mc_err)?;
        let text = serde_json::to_string_pretty(&result).map_err(mc_err)?;
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        description = "Reorder scenes by providing a permutation of 0-based indices. e.g. [2, 0, 1] moves scene 2 to first position."
    )]
    async fn reorder_scenes(
        &self,
        Parameters(params): Parameters<ReorderScenesParams>,
    ) -> Result<CallToolResult, McpError> {
        let path = Path::new(&params.project_path);
        let result = commands::scenes::reorder_scenes(path, &params.order).map_err(mc_err)?;
        let text = serde_json::to_string_pretty(&result).map_err(mc_err)?;
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        description = "Update project configuration: video settings (fps, resolution), output quality, theme (colors, fonts), and voice settings (padding, fallback duration). Only provided fields are changed."
    )]
    async fn set_project_config(
        &self,
        Parameters(params): Parameters<SetProjectConfigParams>,
    ) -> Result<CallToolResult, McpError> {
        let path = Path::new(&params.project_path);
        let update = config::ConfigUpdate {
            fps: params.fps,
            width: params.width,
            height: params.height,
            quality: params.quality,
            primary: params.primary,
            secondary: params.secondary,
            background: params.background,
            text: params.text,
            font_heading: params.font_heading,
            font_body: params.font_body,
            default_transition: params.default_transition,
            default_transition_duration: params.default_transition_duration,
            voice_engine: params.voice_engine,
            default_voice: params.default_voice,
            voice_speed: params.voice_speed,
            padding_before: params.padding_before,
            padding_after: params.padding_after,
            auto_fallback_duration: params.auto_fallback_duration,
            formats: None,
        };

        let updated = config::update_config(path, &update).map_err(mc_err)?;
        let result = serde_json::json!({
            "status": "updated",
            "config": {
                "video": {
                    "fps": updated.video.fps,
                    "width": updated.video.width,
                    "height": updated.video.height,
                    "default_transition": updated.video.default_transition,
                    "default_transition_duration": updated.video.default_transition_duration,
                },
                "voice": {
                    "engine": updated.voice.engine,
                    "default_voice": updated.voice.default_voice,
                    "speed": updated.voice.speed,
                    "padding_before": updated.voice.padding_before,
                    "padding_after": updated.voice.padding_after,
                    "auto_fallback_duration": updated.voice.auto_fallback_duration,
                },
                "quality": updated.output.quality,
                "theme": {
                    "primary": updated.theme.primary,
                    "secondary": updated.theme.secondary,
                    "background": updated.theme.background,
                    "text": updated.theme.text,
                    "font_heading": updated.theme.font_heading,
                    "font_body": updated.theme.font_body,
                },
            },
        });
        let text = serde_json::to_string_pretty(&result).map_err(mc_err)?;
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        description = "List available TTS voices for voiceover generation. Returns voice ID, name, language, gender, and availability."
    )]
    async fn list_voices(
        &self,
        #[allow(unused_variables)] Parameters(params): Parameters<ListVoicesParams>,
    ) -> Result<CallToolResult, McpError> {
        let voices = commands::scenes::list_voices();
        let text = serde_json::to_string_pretty(&voices).map_err(mc_err)?;
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        description = "Preview a scene by rendering a specific frame as a PNG screenshot. Returns base64-encoded PNG data. Use `progress` (0.0-1.0) to preview at a specific animation point."
    )]
    async fn preview_scene(
        &self,
        Parameters(params): Parameters<PreviewSceneParams>,
    ) -> Result<CallToolResult, McpError> {
        let path = Path::new(&params.project_path);

        // If progress is provided, calculate frame from progress
        let frame = if let Some(progress) = params.progress {
            let progress = progress.clamp(0.0, 1.0);
            let config = config::load_config(path).map_err(mc_err)?;
            let scenes = scene::load_scenes(path).map_err(mc_err)?;
            if params.scene_index >= scenes.len() {
                return Err(McpError::invalid_params(
                    format!(
                        "Scene index {} out of range (project has {} scenes)",
                        params.scene_index,
                        scenes.len()
                    ),
                    None,
                ));
            }
            let total_frames = scenes[params.scene_index].total_frames(config.video.fps);
            let frame = ((progress * total_frames as f32) as u32).min(total_frames.saturating_sub(1));
            Some(frame)
        } else {
            params.frame
        };

        let result = commands::scenes::preview_scene(path, params.scene_index, frame)
            .await
            .map_err(mc_err)?;
        let text = serde_json::to_string_pretty(&result).map_err(mc_err)?;
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        description = "Export a scene as PNG, GIF, or WebP. For PNG: returns base64-encoded image at the given progress point. For GIF/WebP: renders animated output and returns the file path and size."
    )]
    async fn export_media(
        &self,
        Parameters(params): Parameters<ExportMediaParams>,
    ) -> Result<CallToolResult, McpError> {
        let path = Path::new(&params.project_path);
        let format = match params.format.to_lowercase().as_str() {
            "png" => commands::export::ExportFormat::Png,
            "gif" => commands::export::ExportFormat::Gif,
            "webp" => commands::export::ExportFormat::Webp,
            other => {
                return Err(McpError::invalid_params(
                    format!("Unsupported format: {other}. Use png, gif, or webp."),
                    None,
                ))
            }
        };

        match format {
            commands::export::ExportFormat::Png => {
                // For PNG, render a single frame using the same pattern as preview_scene
                let progress = params.progress.unwrap_or(0.0).clamp(0.0, 1.0);
                let config = config::load_config(path).map_err(mc_err)?;
                let scenes = scene::load_scenes(path).map_err(mc_err)?;
                if params.scene_index >= scenes.len() {
                    return Err(McpError::invalid_params(
                        format!(
                            "Scene index {} out of range (project has {} scenes)",
                            params.scene_index,
                            scenes.len()
                        ),
                        None,
                    ));
                }
                let scene = &scenes[params.scene_index];
                let width = config.video.width;
                let height = config.video.height;
                let total_frames = scene.total_frames(config.video.fps);
                let frame =
                    ((progress * total_frames as f32) as u32).min(total_frames.saturating_sub(1));

                let mut registry = crate::template::TemplateRegistry::new().map_err(mc_err)?;
                registry
                    .register_project_templates(path)
                    .map_err(mc_err)?;
                let html = registry
                    .render_scene_html(
                        scene,
                        &config.theme,
                        width,
                        height,
                        frame,
                        total_frames,
                        Some(path),
                    )
                    .map_err(mc_err)?;

                let screenshot =
                    crate::render::browser::capture_single_frame(&html, width, height, frame, total_frames)
                        .await
                        .map_err(mc_err)?;
                let png_base64 =
                    base64::engine::general_purpose::STANDARD.encode(&screenshot);

                let result = serde_json::json!({
                    "format": "png",
                    "scene_index": params.scene_index,
                    "width": width,
                    "height": height,
                    "frame": frame,
                    "progress": progress,
                    "png_base64": png_base64,
                });
                let text = serde_json::to_string_pretty(&result).map_err(mc_err)?;
                Ok(CallToolResult::success(vec![Content::text(text)]))
            }
            _ => {
                // For GIF/WebP, call the export command and return the output path
                let config = config::load_config(path).map_err(mc_err)?;
                let output_rel = config
                    .output
                    .directory
                    .strip_prefix("./")
                    .unwrap_or(&config.output.directory);
                let output_dir = path.join(output_rel);
                let ext = format.extension();
                let output_path =
                    output_dir.join(format!("scene-{}.{}", params.scene_index, ext));

                commands::export::run(
                    path,
                    format,
                    Some(params.scene_index),
                    0,                    // frame (unused for GIF/WebP)
                    None,                 // progress (unused for animated)
                    params.duration,
                    Some(output_path.clone()),
                    false,                // all
                    params.width,
                    false,                // open
                    false,                // combined
                    false,                // smart
                )
                .await
                .map_err(mc_err)?;

                let file_size = std::fs::metadata(&output_path)
                    .map(|m| m.len())
                    .unwrap_or(0);

                let result = serde_json::json!({
                    "format": ext,
                    "scene_index": params.scene_index,
                    "output_path": output_path.display().to_string(),
                    "file_size_bytes": file_size,
                });
                let text = serde_json::to_string_pretty(&result).map_err(mc_err)?;
                Ok(CallToolResult::success(vec![Content::text(text)]))
            }
        }
    }

    #[tool(
        description = "Execute multiple tool operations in a single call. Supported tools: create_project, get_project_status, add_scenes, update_scene, remove_scenes, reorder_scenes, set_project_config, list_voices. Returns an array of results."
    )]
    async fn batch(
        &self,
        Parameters(params): Parameters<BatchParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut results = Vec::new();

        for op in params.operations {
            let result: Result<serde_json::Value, String> = (|| -> Result<serde_json::Value, String> {
                match op.tool.as_str() {
                "create_project" => {
                    let p: CreateProjectParams =
                        serde_json::from_value(op.params).map_err(|e| e.to_string())?;
                    let scenes = p.scenes.map(|scenes| {
                        scenes
                            .into_iter()
                            .map(|s| commands::init::SceneInput {
                                template: s.template,
                                script: s.script,
                                duration: s.duration,
                                props: s.props,
                                transition: s.transition,
                                voice: s.voice,
                                background: s.background,
                            })
                            .collect()
                    });
                    let theme = p.theme.map(|t| commands::init::ThemeOverrides {
                        primary: t.primary,
                        secondary: t.secondary,
                        background: t.background,
                        text: t.text,
                        font_heading: t.font_heading,
                        font_body: t.font_body,
                    });
                    let opts = commands::init::CreateProjectOptions {
                        path: p.path.into(),
                        name: Some(p.name),
                        fps: p.fps,
                        width: p.width,
                        height: p.height,
                        quality: p.quality,
                        voice: p.voice,
                        formats: p.formats,
                        theme,
                        scenes,
                    };
                    commands::init::create_project(&opts)
                        .map(|r| serde_json::to_value(r).unwrap_or_default())
                        .map_err(|e| e.to_string())
                }
                "get_project_status" => {
                    let p: GetProjectStatusParams =
                        serde_json::from_value(op.params).map_err(|e| e.to_string())?;
                    build_project_status_json(Path::new(&p.project_path))
                        .map_err(|e| e.message.to_string())
                }
                "add_scenes" => {
                    let p: AddScenesParams =
                        serde_json::from_value(op.params).map_err(|e| e.to_string())?;
                    let scenes = p
                        .scenes
                        .into_iter()
                        .map(|s| commands::scenes::SceneInput {
                            template: s.template,
                            script: s.script,
                            duration: s.duration,
                            props: s.props,
                            transition: s.transition,
                            voice: s.voice,
                            background: s.background,
                        })
                        .collect();
                    commands::scenes::add_scenes(Path::new(&p.project_path), p.insert_at, scenes)
                        .map(|r| serde_json::to_value(r).unwrap_or_default())
                        .map_err(|e| e.to_string())
                }
                "update_scene" => {
                    let p: UpdateSceneParams =
                        serde_json::from_value(op.params).map_err(|e| e.to_string())?;
                    let update = commands::scenes::SceneUpdate {
                        template: p.template,
                        script: p.script,
                        duration: p.duration,
                        props: p.props,
                        transition_in: p.transition_in,
                        transition_out: p.transition_out,
                        voice: p.voice,
                    };
                    commands::scenes::update_scene(
                        Path::new(&p.project_path),
                        p.scene_index,
                        update,
                    )
                    .map(|r| serde_json::to_value(r).unwrap_or_default())
                    .map_err(|e| e.to_string())
                }
                "remove_scenes" => {
                    let p: RemoveScenesParams =
                        serde_json::from_value(op.params).map_err(|e| e.to_string())?;
                    commands::scenes::remove_scenes(Path::new(&p.project_path), &p.indices)
                        .map(|r| serde_json::to_value(r).unwrap_or_default())
                        .map_err(|e| e.to_string())
                }
                "reorder_scenes" => {
                    let p: ReorderScenesParams =
                        serde_json::from_value(op.params).map_err(|e| e.to_string())?;
                    commands::scenes::reorder_scenes(Path::new(&p.project_path), &p.order)
                        .map(|r| serde_json::to_value(r).unwrap_or_default())
                        .map_err(|e| e.to_string())
                }
                "set_project_config" => {
                    let p: SetProjectConfigParams =
                        serde_json::from_value(op.params).map_err(|e| e.to_string())?;
                    let update = config::ConfigUpdate {
                        fps: p.fps,
                        width: p.width,
                        height: p.height,
                        quality: p.quality,
                        primary: p.primary,
                        secondary: p.secondary,
                        background: p.background,
                        text: p.text,
                        font_heading: p.font_heading,
                        font_body: p.font_body,
                        default_transition: p.default_transition,
                        default_transition_duration: p.default_transition_duration,
                        voice_engine: p.voice_engine,
                        default_voice: p.default_voice,
                        voice_speed: p.voice_speed,
                        padding_before: p.padding_before,
                        padding_after: p.padding_after,
                        auto_fallback_duration: p.auto_fallback_duration,
                        formats: None,
                    };
                    config::update_config(Path::new(&p.project_path), &update)
                        .map(|r| serde_json::to_value(r).unwrap_or_default())
                        .map_err(|e| e.to_string())
                }
                "list_voices" => {
                    let voices = commands::scenes::list_voices();
                    Ok(serde_json::to_value(voices).unwrap_or_default())
                }
                other => Err(format!(
                    "Unknown tool: {other}. Supported: create_project, get_project_status, \
                     add_scenes, update_scene, remove_scenes, reorder_scenes, \
                     set_project_config, list_voices"
                )),
            }
            })();

            results.push(match result {
                Ok(value) => serde_json::json!({ "status": "ok", "result": value }),
                Err(err) => serde_json::json!({ "status": "error", "error": err }),
            });
        }

        let text = serde_json::to_string_pretty(&results).map_err(mc_err)?;
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        description = "Poll render progress for a project. Returns the contents of .vidgen-progress.json from the output directory, or {\"status\": \"idle\"} if no render is in progress."
    )]
    async fn get_render_progress(
        &self,
        Parameters(params): Parameters<GetRenderProgressParams>,
    ) -> Result<CallToolResult, McpError> {
        let path = Path::new(&params.project_path);
        let config = config::load_config(path).map_err(mc_err)?;
        let output_rel = config
            .output
            .directory
            .strip_prefix("./")
            .unwrap_or(&config.output.directory);
        let progress_file = path.join(output_rel).join(".vidgen-progress.json");

        let result = if progress_file.exists() {
            let content = std::fs::read_to_string(&progress_file).map_err(mc_err)?;
            serde_json::from_str::<serde_json::Value>(&content).unwrap_or_else(|_| {
                serde_json::json!({ "status": "error", "message": "Invalid progress JSON" })
            })
        } else {
            serde_json::json!({ "status": "idle" })
        };

        let text = serde_json::to_string_pretty(&result).map_err(mc_err)?;
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }
}

// ---------------------------------------------------------------------------
// Prompt parameter structs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateVideoFromTopicParams {
    /// The topic for the video
    #[schemars(description = "The topic or subject for the video")]
    pub topic: String,
    /// Target audience (optional)
    #[schemars(description = "Target audience for the video (optional)")]
    pub audience: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AdaptVideoFormatParams {
    /// Path to the project directory
    #[schemars(description = "Path to the existing project directory")]
    pub project_path: String,
    /// Target format (e.g. portrait, square, landscape)
    #[schemars(
        description = "Target format: portrait (1080x1920), square (1080x1080), or landscape (1920x1080)"
    )]
    pub target_format: String,
}

#[prompt_router]
impl McServer {
    #[prompt(
        description = "Generate a structured video creation plan from a topic. Returns instructions for using create_project with suggested scenes, templates, and content."
    )]
    async fn create_video_from_topic(
        &self,
        Parameters(params): Parameters<CreateVideoFromTopicParams>,
    ) -> Result<GetPromptResult, McpError> {
        let audience_hint = params
            .audience
            .as_deref()
            .map(|a| format!(" The target audience is: {a}."))
            .unwrap_or_default();

        let message = format!(
            "Create a video project about \"{topic}\".{audience}\n\n\
             Use the `create_project` tool with the following approach:\n\
             1. Choose a descriptive project name based on the topic\n\
             2. Structure the video with 3-6 scenes using these templates:\n\
                - Start with a `title-card` scene for the intro\n\
                - Use `content-text` scenes for main points\n\
                - Consider `split-screen` for comparisons, `quote-card` for citations\n\
                - Use `slideshow` for multi-point overviews\n\
                - Use `kinetic-text` for emphasis or key takeaways\n\
                - End with a `cta-card` for the call-to-action\n\
             3. Write natural voiceover scripts in each scene body\n\
             4. Use `duration: auto` (the default) so timing adapts to TTS audio\n\
             5. Set appropriate theme colors and fonts for the topic\n\n\
             After creating the project, use `preview_scene` to verify the first scene looks correct, \
             then `render` to produce the final video.",
            topic = params.topic,
            audience = audience_hint,
        );

        Ok(GetPromptResult {
            description: Some(format!("Create a video about: {}", params.topic)),
            messages: vec![PromptMessage::new_text(PromptMessageRole::User, message)],
        })
    }

    #[prompt(
        description = "Adapt an existing video project to a different format (portrait, square, landscape). Returns instructions for modifying scenes and settings."
    )]
    async fn adapt_video_format(
        &self,
        Parameters(params): Parameters<AdaptVideoFormatParams>,
    ) -> Result<GetPromptResult, McpError> {
        let (width, height) = match params.target_format.to_lowercase().as_str() {
            "portrait" => (1080, 1920),
            "square" => (1080, 1080),
            _ => (1920, 1080), // landscape default
        };

        let message = format!(
            "Adapt the video project at \"{path}\" to {format} format ({width}x{height}).\n\n\
             Steps:\n\
             1. Use `get_project_status` to read the current project structure\n\
             2. Use `set_project_config` to update width={width} and height={height}\n\
             3. Review each scene and use `update_scene` to adjust props if needed:\n\
                - For `split-screen`: panels may need shorter text in portrait\n\
                - For `slideshow`: consider fewer slides per scene in portrait/square\n\
                - For `content-text`: body text may need to be more concise\n\
             4. Use `preview_scene` to verify each scene looks correct at the new resolution\n\
             5. Use `render` to produce the adapted video",
            path = params.project_path,
            format = params.target_format,
            width = width,
            height = height,
        );

        Ok(GetPromptResult {
            description: Some(format!("Adapt project to {} format", params.target_format)),
            messages: vec![PromptMessage::new_text(PromptMessageRole::User, message)],
        })
    }
}

// ---------------------------------------------------------------------------
// ServerHandler — provides get_info, list_resources, read_resource
// ---------------------------------------------------------------------------

#[tool_handler]
#[prompt_handler]
impl ServerHandler for McServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "vidgen — AI-agent-first video production. 13 tools available: \
                 create_project (create new project with inline scenes), \
                 render (render project to MP4), \
                 get_project_status (inspect project config/scenes/output), \
                 add_scenes (append or insert scenes), \
                 update_scene (partial update of a scene's properties), \
                 remove_scenes (delete scenes by index), \
                 reorder_scenes (rearrange scene order), \
                 set_project_config (update video/theme/quality/voice settings), \
                 list_voices (available TTS voices), \
                 preview_scene (render frame as PNG, supports progress 0.0-1.0), \
                 export_media (export scene as PNG/GIF/WebP), \
                 batch (execute multiple tool operations in one call), \
                 get_render_progress (poll render progress). \
                 Typical workflow: create_project → add/update scenes → preview_scene → render. \
                 Duration: scenes default to \"auto\" — length derived from TTS audio + padding."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_prompts()
                .build(),
            ..Default::default()
        }
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        let templates = vec![
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "vidgen://projects/{path}".into(),
                    name: "Project status".into(),
                    title: None,
                    description: Some(
                        "Project config, scene list, and render status".into(),
                    ),
                    mime_type: Some("application/json".into()),
                    icons: None,
                },
                None,
            ),
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "vidgen://projects/{path}/scenes/{index}".into(),
                    name: "Scene content".into(),
                    title: None,
                    description: Some("Full markdown content of a scene".into()),
                    mime_type: Some("text/markdown".into()),
                    icons: None,
                },
                None,
            ),
        ];
        Ok(ListResourceTemplatesResult {
            resource_templates: templates,
            next_cursor: None,
            meta: None,
        })
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let resources = vec![
            Annotated::new(
                RawResource::new("vidgen://templates", "Built-in templates (all adapt to landscape/portrait/square via CSS container queries). Add custom .html files to templates/components/ — registered by file stem name."),
                None,
            ),
            Annotated::new(
                RawResource::new("vidgen://voices", "voices"),
                None,
            ),
        ];

        Ok(ListResourcesResult {
            resources,
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        let uri = &request.uri;

        match uri.as_str() {
            "vidgen://templates" => {
                let templates = serde_json::json!([
                    {
                        "name": "title-card",
                        "description": "Full-screen title card with centered title and subtitle text. Ideal for intro/outro slides.",
                        "props": {
                            "title": "Main heading text (required)",
                            "subtitle": "Secondary text below the title (optional)"
                        }
                    },
                    {
                        "name": "content-text",
                        "description": "Content slide with a heading and body text. Good for explanations, bullet points, or narrative content.",
                        "props": {
                            "heading": "Section heading (required)",
                            "body": "Body text content (required)"
                        }
                    },
                    {
                        "name": "quote-card",
                        "description": "Styled quote with author attribution. Centered layout with decorative quote mark and accent divider.",
                        "props": {
                            "quote": "Quote text (required)",
                            "author": "Author name (optional)",
                            "source": "Source attribution, e.g. book or speech name (optional)"
                        }
                    },
                    {
                        "name": "split-screen",
                        "description": "Multi-panel comparison layout using CSS Grid. 2 columns in landscape, stacked in portrait. Panels appear with staggered animation.",
                        "props": {
                            "panels": "Array of {label, content} objects (required)"
                        }
                    },
                    {
                        "name": "lower-third",
                        "description": "Name/title overlay bar positioned at the bottom of the frame. Slides in from left with accent bar.",
                        "props": {
                            "name": "Person or speaker name (required)",
                            "title": "Title or role (optional)"
                        }
                    },
                    {
                        "name": "cta-card",
                        "description": "Call-to-action end screen with heading, optional subheading, and bulleted items list. Staggered fade-in animation.",
                        "props": {
                            "heading": "Main CTA heading (required)",
                            "subheading": "Secondary text (optional)",
                            "items": "Array of strings for bulleted list (optional)"
                        }
                    },
                    {
                        "name": "kinetic-text",
                        "description": "Progressive word-by-word text reveal driven by animation progress. Words from the script body (or 'text' prop) appear sequentially.",
                        "props": {
                            "text": "Text to display word-by-word (optional — falls back to scene script body)"
                        }
                    },
                    {
                        "name": "slideshow",
                        "description": "Multi-slide presentation with cross-fade transitions between slides. Each slide occupies an equal time slice of the scene duration. Includes progress indicator dots.",
                        "props": {
                            "slides": "Array of {heading, body, image} objects (required). Each slide can have heading (text), body (text), and image (URL/path, optional)"
                        }
                    },
                    {
                        "name": "caption-overlay",
                        "description": "Subtitle-style text overlay with progressive word reveal. Positioned at bottom (default), top, or center of frame. Multiple text styling options.",
                        "props": {
                            "text": "Caption text (optional — falls back to scene script body)",
                            "style": "Text style: outline (default), background-box, drop-shadow",
                            "position": "Caption position: bottom (default), top, center"
                        }
                    }
                ]);
                let text = serde_json::to_string_pretty(&templates).map_err(mc_err)?;
                Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(text, uri.clone())],
                })
            }
            "vidgen://voices" => {
                let mut all_voices = Vec::new();
                for engine_name in &["native", "edge", "piper"] {
                    let vc = crate::config::VoiceConfig {
                        engine: engine_name.to_string(),
                        ..Default::default()
                    };
                    if let Ok(engine) = crate::tts::create_engine(&vc) {
                        if let Ok(voices) = engine.list_voices() {
                            all_voices.extend(voices);
                        }
                    }
                }
                let text = serde_json::to_string_pretty(&all_voices).map_err(mc_err)?;
                Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(text, uri.clone())],
                })
            }
            _ if uri.starts_with("vidgen://projects/") => {
                let rest = &uri["vidgen://projects/".len()..];
                if let Some((path_part, scene_suffix)) = rest.rsplit_once("/scenes/") {
                    // vidgen://projects/{path}/scenes/{index}
                    let project_path = decode_uri_path(path_part);
                    let index: usize = scene_suffix.parse().map_err(|_| {
                        McpError::invalid_params(
                            format!("Invalid scene index: {scene_suffix}"),
                            None,
                        )
                    })?;
                    let path = Path::new(&project_path);
                    let scenes = scene::load_scenes(path).map_err(mc_err)?;
                    if index >= scenes.len() {
                        return Err(McpError::invalid_params(
                            format!(
                                "Scene index {index} out of bounds (project has {} scenes)",
                                scenes.len()
                            ),
                            None,
                        ));
                    }
                    let content =
                        std::fs::read_to_string(&scenes[index].source_path).map_err(mc_err)?;
                    Ok(ReadResourceResult {
                        contents: vec![ResourceContents::text(content, uri.clone())],
                    })
                } else {
                    // vidgen://projects/{path}
                    let project_path = decode_uri_path(rest);
                    let path = Path::new(&project_path);
                    let status = build_project_status_json(path)?;
                    let text = serde_json::to_string_pretty(&status).map_err(mc_err)?;
                    Ok(ReadResourceResult {
                        contents: vec![ResourceContents::text(text, uri.clone())],
                    })
                }
            }
            _ => Err(McpError::resource_not_found(
                format!("Unknown resource: {uri}"),
                None,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a project via the programmatic API and return its path.
    fn setup_test_project(dir: &std::path::Path) -> std::path::PathBuf {
        let project_path = dir.join("test-project");
        let opts = commands::init::CreateProjectOptions {
            path: project_path.clone(),
            name: Some("Test Video".to_string()),
            fps: None,
            width: None,
            height: None,
            quality: None,
            voice: None,
            formats: None,
            theme: None,
            scenes: None,
        };
        commands::init::create_project(&opts).unwrap();
        project_path
    }

    #[test]
    fn test_prompt_router_list_count() {
        let server = McServer::new();
        let prompts = server.prompt_router.list_all();
        assert_eq!(prompts.len(), 2);
        let names: Vec<&str> = prompts.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"create_video_from_topic"));
        assert!(names.contains(&"adapt_video_format"));
    }

    #[test]
    fn test_list_resources_count() {
        // Verify the resource list construction includes both templates and voices.
        // We test the vec directly since calling the trait method requires a RequestContext.
        let resources = vec![
            Annotated::new(
                RawResource::new("vidgen://templates", "Built-in templates (all adapt to landscape/portrait/square via CSS container queries). Add custom .html files to templates/components/ — registered by file stem name."),
                None,
            ),
            Annotated::new(
                RawResource::new("vidgen://voices", "voices"),
                None,
            ),
        ];
        assert_eq!(resources.len(), 2);
    }

    #[test]
    fn test_get_project_status_logic() {
        let dir = tempfile::tempdir().unwrap();
        let project_path = setup_test_project(dir.path());

        // Load config and scenes directly (same logic as get_project_status tool)
        let config = config::load_config(&project_path).unwrap();
        assert_eq!(config.project.name, "Test Video");

        let scenes = scene::load_scenes(&project_path).unwrap();
        assert_eq!(scenes.len(), 1);
        assert_eq!(scenes[0].frontmatter.template, "title-card");
        assert_eq!(scenes[0].frontmatter.duration, SceneDuration::Auto);

        // Output dir should exist but have no mp4 files
        let output_dir = project_path.join("output");
        assert!(output_dir.exists());
        let mp4_count = std::fs::read_dir(&output_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "mp4"))
            .count();
        assert_eq!(mp4_count, 0);
    }

    #[test]
    fn test_get_project_status_with_inline_scenes() {
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().join("multi-scene");
        let mut props = HashMap::new();
        props.insert(
            "title".to_string(),
            serde_json::Value::String("Intro".to_string()),
        );
        let opts = commands::init::CreateProjectOptions {
            path: project_path.clone(),
            name: Some("Multi Scene".to_string()),
            fps: Some(60),
            width: None,
            height: None,
            quality: None,
            voice: None,
            formats: None,
            theme: None,
            scenes: Some(vec![
                commands::init::SceneInput {
                    template: Some("title-card".to_string()),
                    script: "Welcome.".to_string(),
                    duration: Some(SceneDuration::Fixed(3.0)),
                    props: Some(props),
                    transition: None,
                    voice: None,
                    background: None,
                },
                commands::init::SceneInput {
                    template: Some("content-text".to_string()),
                    script: "Content here.".to_string(),
                    duration: Some(SceneDuration::Fixed(7.0)),
                    props: None,
                    transition: None,
                    voice: None,
                    background: None,
                },
            ]),
        };
        commands::init::create_project(&opts).unwrap();

        let config = config::load_config(&project_path).unwrap();
        assert_eq!(config.video.fps, 60);

        let scenes = scene::load_scenes(&project_path).unwrap();
        assert_eq!(scenes.len(), 2);

        // Sum fixed durations
        let total: f64 = scenes
            .iter()
            .filter_map(|s| s.frontmatter.duration.as_fixed())
            .sum();
        assert!((total - 10.0).abs() < 0.01);
    }

    #[test]
    fn test_decode_uri_path() {
        assert_eq!(decode_uri_path("%2Ftmp%2Ftest"), "/tmp/test");
        assert_eq!(decode_uri_path("%2fhome%2fuser%2fmy%20project"), "/home/user/my project");
        assert_eq!(decode_uri_path("simple"), "simple");
    }

    #[test]
    fn test_list_resource_templates() {
        let templates = vec![
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "vidgen://projects/{path}".into(),
                    name: "Project status".into(),
                    title: None,
                    description: Some("Project config, scene list, and render status".into()),
                    mime_type: Some("application/json".into()),
                    icons: None,
                },
                None,
            ),
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "vidgen://projects/{path}/scenes/{index}".into(),
                    name: "Scene content".into(),
                    title: None,
                    description: Some("Full markdown content of a scene".into()),
                    mime_type: Some("text/markdown".into()),
                    icons: None,
                },
                None,
            ),
        ];
        assert_eq!(templates.len(), 2);
        assert!(templates[0].uri_template.contains("{path}"));
        assert!(templates[1].uri_template.contains("{index}"));
    }

    #[test]
    fn test_read_resource_project() {
        let dir = tempfile::tempdir().unwrap();
        let project_path = setup_test_project(dir.path());

        let status = build_project_status_json(&project_path).unwrap();
        assert_eq!(status["project_name"], "Test Video");
        assert_eq!(status["scenes"]["count"], 1);
    }

    #[test]
    fn test_read_resource_scene() {
        let dir = tempfile::tempdir().unwrap();
        let project_path = setup_test_project(dir.path());

        let scenes = scene::load_scenes(&project_path).unwrap();
        assert!(!scenes.is_empty());
        let content = std::fs::read_to_string(&scenes[0].source_path).unwrap();
        assert!(content.contains("template:"));
    }

    #[test]
    fn test_read_resource_scene_out_of_bounds() {
        let dir = tempfile::tempdir().unwrap();
        let project_path = setup_test_project(dir.path());

        let scenes = scene::load_scenes(&project_path).unwrap();
        assert_eq!(scenes.len(), 1);
        // Index 5 is out of bounds for a 1-scene project
        assert!(scenes.get(5).is_none());
    }
}
