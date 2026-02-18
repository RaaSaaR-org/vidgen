use crate::config;
use crate::error::{VidgenError, VidgenResult};
use crate::scene;
use serde::Serialize;
use std::path::Path;

/// Structured result from rendering a single format.
#[derive(Serialize)]
pub struct RenderResult {
    pub output_path: String,
    pub format_name: String,
    pub scenes_rendered: usize,
    pub duration_secs: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle_path: Option<String>,
}

/// Programmatic render entry point. Returns structured results (one per format).
#[allow(clippy::too_many_arguments)]
pub async fn render_project(
    path: &Path,
    fps: Option<u32>,
    quality: Option<String>,
    formats: Option<Vec<String>>,
    scenes_filter: Option<Vec<usize>>,
    subtitles_override: Option<bool>,
    burn_in_override: Option<bool>,
    parallel_override: Option<usize>,
) -> VidgenResult<Vec<RenderResult>> {
    if !path.exists() {
        return Err(VidgenError::ProjectNotFound(path.to_path_buf()));
    }

    // Load config and validate
    let mut config = config::load_config(path)?;
    config.validate()?;

    // Apply overrides
    let fps = fps.unwrap_or(config.video.fps);
    let quality_name = quality.as_deref().unwrap_or(&config.output.quality);

    if let Some(subs) = subtitles_override {
        config.output.subtitles.enabled = subs;
    }
    if let Some(burn) = burn_in_override {
        config.output.subtitles.burn_in = burn;
        if burn {
            // burn-in implies subtitles enabled
            config.output.subtitles.enabled = true;
        }
    }
    if let Some(par) = parallel_override {
        config.video.parallel_scenes = Some(par);
    }

    // Load scenes, optionally filtering by index
    let all_scenes = scene::load_scenes(path)?;
    let scenes = if let Some(ref indices) = scenes_filter {
        all_scenes
            .into_iter()
            .enumerate()
            .filter(|(i, _)| indices.contains(i))
            .map(|(_, s)| s)
            .collect()
    } else {
        all_scenes
    };
    let scenes_rendered = scenes.len();

    // Resolve output directory (strip ./ prefix if present)
    let output_rel = config
        .output
        .directory
        .strip_prefix("./")
        .unwrap_or(&config.output.directory);
    let output_dir = path.join(output_rel);

    let format_filter = formats.as_deref();

    let format_outputs = crate::render::render_project(
        &config,
        &scenes,
        fps,
        quality_name,
        &output_dir,
        path,
        crate::render::RenderProgress::noop(),
        format_filter,
    )
    .await?;

    Ok(format_outputs
        .into_iter()
        .map(|fo| {
            let duration_secs: f64 = fo.effective_durations.iter().sum();
            RenderResult {
                output_path: fo.output_path.display().to_string(),
                format_name: fo.format_name,
                scenes_rendered,
                duration_secs,
                subtitle_path: fo.subtitle_path.map(|p| p.display().to_string()),
            }
        })
        .collect())
}

/// Programmatic render entry point with MCP progress reporting.
pub async fn render_project_with_progress(
    path: &Path,
    fps: Option<u32>,
    quality: Option<String>,
    formats: Option<Vec<String>>,
    scenes_filter: Option<Vec<usize>>,
    progress: crate::render::RenderProgress,
) -> VidgenResult<Vec<RenderResult>> {
    if !path.exists() {
        return Err(VidgenError::ProjectNotFound(path.to_path_buf()));
    }

    let config = config::load_config(path)?;
    config.validate()?;
    let fps = fps.unwrap_or(config.video.fps);
    let quality_name = quality.as_deref().unwrap_or(&config.output.quality);
    let all_scenes = scene::load_scenes(path)?;
    let scenes = if let Some(ref indices) = scenes_filter {
        all_scenes
            .into_iter()
            .enumerate()
            .filter(|(i, _)| indices.contains(i))
            .map(|(_, s)| s)
            .collect()
    } else {
        all_scenes
    };
    let scenes_rendered = scenes.len();

    let output_rel = config
        .output
        .directory
        .strip_prefix("./")
        .unwrap_or(&config.output.directory);
    let output_dir = path.join(output_rel);

    let format_filter = formats.as_deref();

    let format_outputs = crate::render::render_project(
        &config,
        &scenes,
        fps,
        quality_name,
        &output_dir,
        path,
        progress,
        format_filter,
    )
    .await?;

    Ok(format_outputs
        .into_iter()
        .map(|fo| {
            let duration_secs: f64 = fo.effective_durations.iter().sum();
            RenderResult {
                output_path: fo.output_path.display().to_string(),
                format_name: fo.format_name,
                scenes_rendered,
                duration_secs,
                subtitle_path: fo.subtitle_path.map(|p| p.display().to_string()),
            }
        })
        .collect())
}

/// CLI entry point — delegates to `render_project()`.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    path: &Path,
    fps: Option<u32>,
    quality: Option<String>,
    formats: Option<Vec<String>>,
    scenes: Option<Vec<usize>>,
    subtitles: bool,
    burn_in: bool,
    parallel: Option<usize>,
) -> VidgenResult<()> {
    let subtitles_override = if subtitles { Some(true) } else { None };
    let burn_in_override = if burn_in { Some(true) } else { None };
    let results = render_project(
        path,
        fps,
        quality,
        formats,
        scenes,
        subtitles_override,
        burn_in_override,
        parallel,
    )
    .await?;
    for r in &results {
        eprintln!(
            "  Format \"{}\": {} scenes, {:.1}s total → {}",
            r.format_name, r.scenes_rendered, r.duration_secs, r.output_path
        );
    }
    Ok(())
}
