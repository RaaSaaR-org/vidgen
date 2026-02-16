use crate::config;
use crate::error::{VidgenError, VidgenResult};
use crate::render::browser::capture_single_frame;
use crate::scene;
use crate::template::TemplateRegistry;
use colored::*;
use std::path::{Path, PathBuf};

/// Run the preview command: render a single frame of a scene as a PNG file.
pub async fn run(
    project_path: &Path,
    scene_index: usize,
    frame: u32,
    output: Option<PathBuf>,
) -> VidgenResult<()> {
    let cfg = config::load_config(project_path)?;
    let scenes = scene::load_scenes(project_path)?;
    let count = scenes.len();

    if scene_index >= count {
        return Err(VidgenError::SceneIndexOutOfRange {
            index: scene_index,
            count,
        });
    }

    let scene = &scenes[scene_index];
    let width = cfg.video.width;
    let height = cfg.video.height;
    let fps = cfg.video.fps;
    let total_frames = scene.total_frames(fps);

    if scene.frontmatter.duration.is_auto() {
        eprintln!(
            "{} Scene {} has auto duration — using {:.1}s fallback for preview (TTS not run in preview mode)",
            "preview:".yellow().bold(),
            scene_index,
            cfg.voice.auto_fallback_duration
        );
    }

    if frame >= total_frames {
        return Err(VidgenError::Other(format!(
            "Frame {frame} out of range (scene has {total_frames} frames, 0-indexed)"
        )));
    }

    eprintln!(
        "{} Previewing scene {} frame {}/{}...",
        "preview:".cyan().bold(),
        scene_index,
        frame,
        total_frames
    );

    let mut registry = TemplateRegistry::new()?;
    registry.register_project_templates(project_path)?;
    let html = registry.render_scene_html(scene, &cfg.theme, width, height, frame, total_frames)?;

    let png_data = capture_single_frame(&html, width, height, frame, total_frames).await?;

    let output_path = output.unwrap_or_else(|| PathBuf::from("preview.png"));
    std::fs::write(&output_path, &png_data)?;

    eprintln!(
        "{} Saved preview to {} ({}x{}, scene {} frame {})",
        "done:".green().bold(),
        output_path.display(),
        width,
        height,
        scene_index,
        frame
    );

    Ok(())
}
