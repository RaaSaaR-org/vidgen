use crate::config;
use crate::error::{VidgenError, VidgenResult};
use crate::render::browser::capture_single_frame;
use crate::scene;
use crate::template::TemplateRegistry;
use colored::*;
use std::path::{Path, PathBuf};

/// Run the preview command: render a single frame (or all scenes / animated GIF).
pub async fn run(
    project_path: &Path,
    scene_index: usize,
    frame: u32,
    output: Option<PathBuf>,
    all: bool,
    gif: bool,
) -> VidgenResult<()> {
    let cfg = config::load_config(project_path)?;
    let scenes = scene::load_scenes(project_path)?;
    let count = scenes.len();

    let mut registry = TemplateRegistry::new()?;
    registry.register_project_templates(project_path)?;

    let width = cfg.video.width;
    let height = cfg.video.height;
    let fps = cfg.video.fps;

    if all {
        // --all: preview all scenes as numbered PNG thumbnails
        let output_dir = output
            .as_deref()
            .unwrap_or_else(|| Path::new("."));
        eprintln!(
            "{} Previewing all {} scenes...",
            "preview:".cyan().bold(),
            count
        );

        for (i, s) in scenes.iter().enumerate() {
            let total = s.total_frames(fps);
            let html = registry.render_scene_html(s, &cfg.theme, width, height, 0, total, Some(project_path))?;
            let png = capture_single_frame(&html, width, height, 0, total).await?;
            let filename = format!("preview-{:02}.png", i + 1);
            let path = output_dir.join(&filename);
            std::fs::write(&path, &png)?;
            eprintln!(
                "  Scene {}: {} ({})",
                i + 1,
                filename,
                s.frontmatter.template
            );
        }
        eprintln!(
            "{} Saved {} preview thumbnails",
            "done:".green().bold(),
            count
        );
        return Ok(());
    }

    if scene_index >= count {
        return Err(VidgenError::SceneIndexOutOfRange {
            index: scene_index,
            count,
        });
    }

    let s = &scenes[scene_index];
    let total_frames = s.total_frames(fps);

    if s.frontmatter.duration.is_auto() {
        eprintln!(
            "{} Scene {} has auto duration — using {:.1}s fallback for preview (TTS not run in preview mode)",
            "preview:".yellow().bold(),
            scene_index,
            cfg.voice.auto_fallback_duration
        );
    }

    if gif {
        // --gif: render multiple frames and assemble via FFmpeg into a GIF
        let gif_frames = total_frames.min(fps * 3); // cap at 3 seconds
        let step = if total_frames > gif_frames {
            total_frames / gif_frames
        } else {
            1
        };
        let output_path = output.unwrap_or_else(|| PathBuf::from("preview.gif"));

        eprintln!(
            "{} Generating GIF preview for scene {} ({} frames)...",
            "preview:".cyan().bold(),
            scene_index,
            gif_frames
        );

        // Create temp dir for frames
        let temp_dir = tempfile::tempdir()?;
        let mut frame_idx = 0u32;
        let mut f = 0u32;
        while f < total_frames && frame_idx < gif_frames {
            let html = registry.render_scene_html(s, &cfg.theme, width, height, f, total_frames, Some(project_path))?;
            let png = capture_single_frame(&html, width, height, f, total_frames).await?;
            let frame_path = temp_dir.path().join(format!("frame-{frame_idx:04}.png"));
            std::fs::write(&frame_path, &png)?;
            frame_idx += 1;
            f += step;
        }

        // Use FFmpeg to assemble GIF
        let input_pattern = temp_dir.path().join("frame-%04d.png");
        let gif_fps = (fps / step).max(1);
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-framerate", &gif_fps.to_string(),
                "-i", &input_pattern.to_string_lossy(),
                "-vf", &format!("scale={}:-1:flags=lanczos", width.min(480)),
                "-loop", "0",
                &output_path.to_string_lossy(),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| VidgenError::Ffmpeg(format!("Failed to run ffmpeg: {e}")))?;

        if !status.success() {
            return Err(VidgenError::Ffmpeg("FFmpeg GIF encoding failed".into()));
        }

        eprintln!(
            "{} Saved GIF preview to {} (scene {}, {} frames)",
            "done:".green().bold(),
            output_path.display(),
            scene_index,
            frame_idx
        );
        return Ok(());
    }

    // Single frame preview (original behavior)
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

    let html = registry.render_scene_html(s, &cfg.theme, width, height, frame, total_frames, Some(project_path))?;
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
