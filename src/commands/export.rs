use crate::config;
use crate::error::{VidgenError, VidgenResult};
use crate::render::browser::capture_single_frame;
use crate::scene;
use crate::template::TemplateRegistry;
use colored::*;
use std::path::{Path, PathBuf};

/// Export format for media output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExportFormat {
    Png,
    Gif,
    Webp,
}

impl ExportFormat {
    pub fn extension(&self) -> &str {
        match self {
            Self::Png => "png",
            Self::Gif => "gif",
            Self::Webp => "webp",
        }
    }
}

/// Open a file with the platform's default viewer.
fn open_file(path: &Path) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

/// Calculate visual "weight" of a PNG image.
/// Counts bytes above a threshold to estimate how much visible (non-background) content exists.
/// Scenes with more visible content have more non-black pixels in the PNG data.
fn image_weight(png_data: &[u8]) -> usize {
    png_data.iter().filter(|&&b| b > 0x30).count()
}

/// Render a combined GIF that samples frames from every scene.
#[allow(clippy::too_many_arguments)]
async fn render_combined_gif(
    registry: &TemplateRegistry<'_>,
    scenes: &[scene::Scene],
    theme: &crate::config::ThemeConfig,
    width: u32,
    height: u32,
    fps: u32,
    duration_secs: f32,
    output_path: &Path,
    width_override: Option<u32>,
    project_path: &Path,
) -> VidgenResult<()> {
    let scene_count = scenes.len();
    if scene_count == 0 {
        return Err(VidgenError::Other("No scenes to export".into()));
    }

    let total_target_frames = (duration_secs * fps as f32) as u32;
    let frames_per_scene = (total_target_frames / scene_count as u32).max(1);

    let temp_dir = tempfile::tempdir()?;
    let mut global_frame_idx = 0u32;

    for (i, s) in scenes.iter().enumerate() {
        let total_frames = s.total_frames(fps);
        if total_frames == 0 {
            continue;
        }

        for j in 0..frames_per_scene {
            // Evenly space frames across this scene
            let f = if frames_per_scene > 1 {
                (j as f64 / (frames_per_scene - 1) as f64 * (total_frames.saturating_sub(1)) as f64)
                    as u32
            } else {
                total_frames / 2
            };
            let f = f.min(total_frames.saturating_sub(1));

            let html = registry.render_scene_html(
                s,
                theme,
                width,
                height,
                f,
                total_frames,
                Some(project_path),
            )?;
            let png = capture_single_frame(&html, width, height, f, total_frames).await?;
            let frame_path = temp_dir
                .path()
                .join(format!("frame-{global_frame_idx:04}.png"));
            std::fs::write(&frame_path, &png)?;
            global_frame_idx += 1;
        }

        eprintln!("  Scene {}: {} frames sampled", i + 1, frames_per_scene);
    }

    if global_frame_idx == 0 {
        return Err(VidgenError::Other(
            "No frames rendered for combined GIF".into(),
        ));
    }

    // Use FFmpeg two-pass palette-optimized GIF encoding
    let input_pattern = temp_dir.path().join("frame-%04d.png");
    let out_fps = (global_frame_idx as f32 / duration_secs).round().max(1.0) as u32;
    let scale_width = width_override.unwrap_or(width.min(640));
    let scale_filter = format!("scale={}:-1:flags=lanczos", scale_width);

    // Pass 1: generate optimal palette
    let palette_path = temp_dir.path().join("palette.png");
    let palette_status = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-framerate",
            &out_fps.to_string(),
            "-i",
            &input_pattern.to_string_lossy(),
            "-vf",
            &format!("{scale_filter},palettegen=stats_mode=diff"),
        ])
        .arg(palette_path.as_os_str())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| VidgenError::Ffmpeg(format!("FFmpeg palettegen failed: {e}")))?;

    if !palette_status.success() {
        return Err(VidgenError::Ffmpeg(
            "FFmpeg palette generation failed".into(),
        ));
    }

    // Pass 2: encode GIF using palette
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-framerate", &out_fps.to_string(),
            "-i", &input_pattern.to_string_lossy(),
            "-i", &palette_path.to_string_lossy(),
            "-lavfi", &format!(
                "{scale_filter}[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle"
            ),
            "-loop", "0",
        ])
        .arg(output_path.as_os_str())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| VidgenError::Ffmpeg(format!("FFmpeg GIF encode failed: {e}")))?;

    if !status.success() {
        return Err(VidgenError::Ffmpeg(
            "FFmpeg combined GIF encoding failed".into(),
        ));
    }

    Ok(())
}

/// Run the export command: generate images, GIFs, or WebP from scenes.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    project_path: &Path,
    format: ExportFormat,
    scene_index: Option<usize>,
    frame: u32,
    progress: Option<f32>,
    duration: Option<f32>,
    output: Option<PathBuf>,
    all: bool,
    width_override: Option<u32>,
    open: bool,
    combined: bool,
    smart: bool,
) -> VidgenResult<()> {
    let cfg = config::load_config(project_path)?;
    let scenes = scene::load_scenes(project_path)?;
    let count = scenes.len();

    let mut registry = TemplateRegistry::new()?;
    registry.register_project_templates(project_path)?;

    let width = cfg.video.width;
    let height = cfg.video.height;
    let fps = cfg.video.fps;

    // Default output directory: project_path/output/
    let default_output_dir = project_path.join(cfg.output.directory.trim_start_matches("./"));
    std::fs::create_dir_all(&default_output_dir)?;

    // Combined GIF: sample frames from all scenes into a single GIF
    if combined && format == ExportFormat::Gif {
        let dur = duration.unwrap_or(3.0);
        let output_path = output.unwrap_or_else(|| default_output_dir.join("combined.gif"));

        eprintln!(
            "{} Rendering combined GIF from {} scenes ({:.1}s)...",
            "export:".cyan().bold(),
            count,
            dur
        );

        render_combined_gif(
            &registry,
            &scenes,
            &cfg.theme,
            width,
            height,
            fps,
            dur,
            &output_path,
            width_override,
            project_path,
        )
        .await?;

        let file_size = std::fs::metadata(&output_path)
            .map(|m| m.len())
            .unwrap_or(0);
        let size_str = if file_size > 1_048_576 {
            format!("{:.1}MB", file_size as f64 / 1_048_576.0)
        } else {
            format!("{:.0}KB", file_size as f64 / 1024.0)
        };

        eprintln!(
            "{} Saved {} ({}, {:.1}s, {} scenes)",
            "done:".green().bold(),
            output_path.display(),
            size_str,
            dur,
            count
        );

        if open {
            open_file(&output_path);
        }

        return Ok(());
    }

    // Export all scenes
    if all {
        let output_dir = output.as_deref().unwrap_or(&default_output_dir);

        eprintln!(
            "{} Exporting all {} scenes as {}...",
            "export:".cyan().bold(),
            count,
            format.extension().to_uppercase()
        );

        for (i, s) in scenes.iter().enumerate() {
            let total = s.total_frames(fps);
            // Use midpoint frame for static exports, or progress override
            let f = if let Some(p) = progress {
                ((p.clamp(0.0, 1.0) * total as f32) as u32).min(total.saturating_sub(1))
            } else {
                total / 2 // midpoint by default
            };

            match format {
                ExportFormat::Png => {
                    let png = if smart {
                        // Smart mode: render 5 candidate frames, pick the one with most visual content
                        let candidates = [0.1f32, 0.3, 0.5, 0.7, 0.9];
                        let mut best_png = Vec::new();
                        let mut best_weight = 0usize;
                        for &p in &candidates {
                            let cf = ((p * total as f32) as u32).min(total.saturating_sub(1));
                            let html = registry.render_scene_html(
                                s,
                                &cfg.theme,
                                width,
                                height,
                                cf,
                                total,
                                Some(project_path),
                            )?;
                            let candidate =
                                capture_single_frame(&html, width, height, cf, total).await?;
                            let w = image_weight(&candidate);
                            if w > best_weight {
                                best_weight = w;
                                best_png = candidate;
                            }
                        }
                        best_png
                    } else {
                        let html = registry.render_scene_html(
                            s,
                            &cfg.theme,
                            width,
                            height,
                            f,
                            total,
                            Some(project_path),
                        )?;
                        capture_single_frame(&html, width, height, f, total).await?
                    };
                    let filename = format!("export-{:02}.png", i + 1);
                    let path = output_dir.join(&filename);
                    std::fs::write(&path, &png)?;
                    eprintln!(
                        "  Scene {}: {}{}",
                        i + 1,
                        filename,
                        if smart { " (smart)" } else { "" }
                    );
                }
                ExportFormat::Gif | ExportFormat::Webp => {
                    let dur = duration.unwrap_or(3.0);
                    let filename = format!("export-{:02}.{}", i + 1, format.extension());
                    let path = output_dir.join(&filename);
                    render_animated(
                        &registry,
                        s,
                        &cfg.theme,
                        width,
                        height,
                        fps,
                        dur,
                        &path,
                        format,
                        width_override,
                        project_path,
                    )
                    .await?;
                    eprintln!("  Scene {}: {}", i + 1, filename);
                }
            }
        }

        eprintln!("{} Exported {} files", "done:".green().bold(), count);
        return Ok(());
    }

    // Single scene export
    let idx = scene_index.unwrap_or(0);
    if idx >= count {
        return Err(VidgenError::SceneIndexOutOfRange { index: idx, count });
    }

    let s = &scenes[idx];
    let total_frames = s.total_frames(fps);

    match format {
        ExportFormat::Png => {
            let (png_data, f) = if smart {
                // Smart mode: render 5 candidate frames, pick the one with most visual content
                eprintln!(
                    "{} Exporting scene {} as PNG (smart mode)...",
                    "export:".cyan().bold(),
                    idx
                );
                let candidates = [0.1f32, 0.3, 0.5, 0.7, 0.9];
                let mut best_png = Vec::new();
                let mut best_weight = 0usize;
                let mut best_frame = 0u32;
                for &p in &candidates {
                    let cf = ((p * total_frames as f32) as u32).min(total_frames.saturating_sub(1));
                    let html = registry.render_scene_html(
                        s,
                        &cfg.theme,
                        width,
                        height,
                        cf,
                        total_frames,
                        Some(project_path),
                    )?;
                    let candidate =
                        capture_single_frame(&html, width, height, cf, total_frames).await?;
                    let w = image_weight(&candidate);
                    if w > best_weight {
                        best_weight = w;
                        best_png = candidate;
                        best_frame = cf;
                    }
                }
                (best_png, best_frame)
            } else {
                let f = if let Some(p) = progress {
                    ((p.clamp(0.0, 1.0) * total_frames as f32) as u32)
                        .min(total_frames.saturating_sub(1))
                } else {
                    frame
                };

                if f >= total_frames {
                    return Err(VidgenError::Other(format!(
                        "Frame {f} out of range (scene has {total_frames} frames)"
                    )));
                }

                eprintln!(
                    "{} Exporting scene {} frame {} as PNG...",
                    "export:".cyan().bold(),
                    idx,
                    f
                );

                let html = registry.render_scene_html(
                    s,
                    &cfg.theme,
                    width,
                    height,
                    f,
                    total_frames,
                    Some(project_path),
                )?;
                let data = capture_single_frame(&html, width, height, f, total_frames).await?;
                (data, f)
            };

            let output_path =
                output.unwrap_or_else(|| default_output_dir.join(format!("scene-{idx:02}.png")));
            std::fs::write(&output_path, &png_data)?;

            eprintln!(
                "{} Saved {} ({}x{}, scene {} frame {})",
                "done:".green().bold(),
                output_path.display(),
                width,
                height,
                idx,
                f
            );

            if open {
                open_file(&output_path);
            }
        }

        ExportFormat::Gif | ExportFormat::Webp => {
            let dur = duration.unwrap_or(3.0);
            let ext = format.extension();
            let output_path =
                output.unwrap_or_else(|| default_output_dir.join(format!("scene-{idx:02}.{ext}")));

            eprintln!(
                "{} Exporting scene {} as {} ({:.1}s)...",
                "export:".cyan().bold(),
                idx,
                ext.to_uppercase(),
                dur
            );

            render_animated(
                &registry,
                s,
                &cfg.theme,
                width,
                height,
                fps,
                dur,
                &output_path,
                format,
                width_override,
                project_path,
            )
            .await?;

            let file_size = std::fs::metadata(&output_path)
                .map(|m| m.len())
                .unwrap_or(0);
            let size_str = if file_size > 1_048_576 {
                format!("{:.1}MB", file_size as f64 / 1_048_576.0)
            } else {
                format!("{:.0}KB", file_size as f64 / 1024.0)
            };

            eprintln!(
                "{} Saved {} ({}, {:.1}s, scene {})",
                "done:".green().bold(),
                output_path.display(),
                size_str,
                dur,
                idx
            );

            if open {
                open_file(&output_path);
            }
        }
    }

    Ok(())
}

/// Export voiceover audio (WAV) for one or all scenes.
pub async fn run_audio(
    project_path: &Path,
    scene_index: Option<usize>,
    output: Option<PathBuf>,
) -> VidgenResult<()> {
    let cfg = config::load_config(project_path)?;
    let scenes = scene::load_scenes(project_path)?;
    let count = scenes.len();

    // Load .env from project directory (if present) so keys like ELEVEN_API_KEY are available
    let _ = dotenvy::from_path(project_path.join(".env"));

    // Create TTS engine
    let tts_engine = crate::tts::create_engine(&cfg.voice)?;
    eprintln!(
        "{} TTS engine: {}",
        "export:".cyan().bold(),
        tts_engine.engine_name()
    );

    let default_output_dir = project_path.join(cfg.output.directory.trim_start_matches("./"));
    let audio_output_dir = default_output_dir.join("audio");
    std::fs::create_dir_all(&audio_output_dir)?;

    let temp_dir = tempfile::tempdir()?;

    let indices: Vec<usize> = if let Some(idx) = scene_index {
        if idx >= count {
            return Err(VidgenError::SceneIndexOutOfRange { index: idx, count });
        }
        vec![idx]
    } else {
        (0..count).collect()
    };

    let mut exported = 0;
    for &i in &indices {
        let scene = &scenes[i];
        let script = scene.script.trim();
        if script.is_empty() {
            eprintln!("  Scene {}: no script, skipping", i + 1);
            continue;
        }

        let wav_path = temp_dir.path().join(format!("scene-{i:03}.wav"));

        // Determine per-scene engine/voice/speed overrides
        let scene_voice_cfg = scene.frontmatter.voice.as_ref();
        let voice = scene_voice_cfg
            .and_then(|v| v.voice_name())
            .or(cfg.voice.default_voice.as_deref());
        let speed = scene_voice_cfg
            .and_then(|v| v.speed)
            .unwrap_or(cfg.voice.speed);

        match crate::tts::cache::synthesize_cached_with_options(
            tts_engine.as_ref(),
            script,
            voice,
            speed,
            &wav_path,
            project_path,
            false,
        ) {
            Ok(result) => {
                let tag = if result.cached { " (cached)" } else { "" };

                // Determine destination path
                let dest = if indices.len() == 1 {
                    // Single scene: use output path directly if given
                    output.clone().unwrap_or_else(|| {
                        let fallback_name = format!("scene-{:02}", i + 1);
                        let scene_name = scene
                            .source_path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or(&fallback_name);
                        audio_output_dir.join(format!("{scene_name}.wav"))
                    })
                } else {
                    // Multiple scenes: save to output dir
                    let out_dir = output.as_deref().unwrap_or(&audio_output_dir);
                    let fallback_name = format!("scene-{:02}", i + 1);
                    let scene_name = scene
                        .source_path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or(&fallback_name);
                    out_dir.join(format!("{scene_name}.wav"))
                };

                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(&result.audio_path, &dest)?;
                eprintln!(
                    "  Scene {}: {:.1}s audio{} -> {}",
                    i + 1,
                    result.duration_secs,
                    tag,
                    dest.display()
                );
                exported += 1;
            }
            Err(e) => {
                eprintln!("  Scene {}: TTS failed ({}), skipping", i + 1, e);
            }
        }
    }

    eprintln!(
        "{} Exported {} audio file(s)",
        "done:".green().bold(),
        exported
    );

    Ok(())
}

/// Export subtitles as SRT file, with one entry per scene based on TTS durations.
pub async fn run_subtitles(project_path: &Path, output: Option<PathBuf>) -> VidgenResult<()> {
    let cfg = config::load_config(project_path)?;
    let scenes = scene::load_scenes(project_path)?;

    // Load .env from project directory (if present) so keys like ELEVEN_API_KEY are available
    let _ = dotenvy::from_path(project_path.join(".env"));

    // Create TTS engine to get durations
    let tts_engine = crate::tts::create_engine(&cfg.voice)?;
    eprintln!(
        "{} TTS engine: {}",
        "export:".cyan().bold(),
        tts_engine.engine_name()
    );

    let temp_dir = tempfile::tempdir()?;
    let mut entries = Vec::new();
    let mut current_time: f64 = 0.0;
    let mut index = 1usize;

    for (i, s) in scenes.iter().enumerate() {
        let script = s.script.trim();
        if script.is_empty() {
            // For scenes with no script, use their fixed duration as a gap
            if let scene::SceneDuration::Fixed(d) = &s.frontmatter.duration {
                current_time += *d;
            }
            continue;
        }

        let wav_path = temp_dir.path().join(format!("scene-{i:03}.wav"));

        let scene_voice_cfg = s.frontmatter.voice.as_ref();
        let voice = scene_voice_cfg
            .and_then(|v| v.voice_name())
            .or(cfg.voice.default_voice.as_deref());
        let speed = scene_voice_cfg
            .and_then(|v| v.speed)
            .unwrap_or(cfg.voice.speed);

        match crate::tts::cache::synthesize_cached_with_options(
            tts_engine.as_ref(),
            script,
            voice,
            speed,
            &wav_path,
            project_path,
            false,
        ) {
            Ok(result) => {
                let start = current_time;
                let end = current_time + result.duration_secs;

                entries.push(crate::subtitle::SubtitleEntry {
                    index,
                    start_secs: start,
                    end_secs: end,
                    text: script.to_string(),
                });

                index += 1;
                current_time = end;
            }
            Err(e) => {
                eprintln!("  Scene {}: TTS failed ({}), skipping", i + 1, e);
                // If fixed duration, advance time anyway
                if let scene::SceneDuration::Fixed(d) = &s.frontmatter.duration {
                    current_time += *d;
                }
            }
        }
    }

    let srt_content = crate::subtitle::to_srt(&entries);

    let default_output_dir = project_path.join(cfg.output.directory.trim_start_matches("./"));
    std::fs::create_dir_all(&default_output_dir)?;
    let output_path = output.unwrap_or_else(|| default_output_dir.join("subtitles.srt"));

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&output_path, &srt_content)?;

    eprintln!(
        "{} Saved {} subtitle entries to {}",
        "done:".green().bold(),
        entries.len(),
        output_path.display()
    );

    Ok(())
}

/// Render an animated GIF or WebP from a scene.
async fn render_animated(
    registry: &TemplateRegistry<'_>,
    scene: &scene::Scene,
    theme: &config::ThemeConfig,
    width: u32,
    height: u32,
    fps: u32,
    duration_secs: f32,
    output_path: &Path,
    format: ExportFormat,
    width_override: Option<u32>,
    project_path: &Path,
) -> VidgenResult<()> {
    let total_frames = scene.total_frames(fps);
    let target_frames = ((duration_secs * fps as f32) as u32).min(total_frames);
    let step = if total_frames > target_frames {
        total_frames / target_frames
    } else {
        1
    };

    // Render frames to temp dir
    let temp_dir = tempfile::tempdir()?;
    let mut frame_idx = 0u32;
    let mut f = 0u32;
    while f < total_frames && frame_idx < target_frames {
        let html = registry.render_scene_html(
            scene,
            theme,
            width,
            height,
            f,
            total_frames,
            Some(project_path),
        )?;
        let png = capture_single_frame(&html, width, height, f, total_frames).await?;
        let frame_path = temp_dir.path().join(format!("frame-{frame_idx:04}.png"));
        std::fs::write(&frame_path, &png)?;
        frame_idx += 1;
        f += step;
    }

    let input_pattern = temp_dir.path().join("frame-%04d.png");
    let out_fps = (fps / step).max(1);
    let scale_width = width_override.unwrap_or(width.min(640));

    let status = match format {
        ExportFormat::Gif => {
            // Two-pass GIF encoding with palette optimization for better quality
            let palette_path = temp_dir.path().join("palette.png");
            let scale_filter = format!("scale={}:-1:flags=lanczos", scale_width);

            // Pass 1: generate optimal palette
            let palette_status = std::process::Command::new("ffmpeg")
                .args([
                    "-y",
                    "-framerate",
                    &out_fps.to_string(),
                    "-i",
                    &input_pattern.to_string_lossy(),
                    "-vf",
                    &format!("{scale_filter},palettegen=stats_mode=diff"),
                ])
                .arg(palette_path.as_os_str())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map_err(|e| VidgenError::Ffmpeg(format!("FFmpeg palettegen failed: {e}")))?;

            if !palette_status.success() {
                return Err(VidgenError::Ffmpeg(
                    "FFmpeg palette generation failed".into(),
                ));
            }

            // Pass 2: encode GIF using palette
            std::process::Command::new("ffmpeg")
                .args([
                    "-y",
                    "-framerate", &out_fps.to_string(),
                    "-i", &input_pattern.to_string_lossy(),
                    "-i", &palette_path.to_string_lossy(),
                    "-lavfi", &format!(
                        "{scale_filter}[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle"
                    ),
                    "-loop", "0",
                ])
                .arg(output_path.as_os_str())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map_err(|e| VidgenError::Ffmpeg(format!("FFmpeg GIF encode failed: {e}")))?
        }
        ExportFormat::Webp => {
            // Try libwebp_anim first, fall back to APNG if unavailable
            let result = std::process::Command::new("ffmpeg")
                .args([
                    "-y",
                    "-framerate",
                    &out_fps.to_string(),
                    "-i",
                    &input_pattern.to_string_lossy(),
                    "-vf",
                    &format!("scale={}:-1:flags=lanczos", scale_width),
                    "-vcodec",
                    "libwebp_anim",
                    "-lossless",
                    "0",
                    "-compression_level",
                    "4",
                    "-q:v",
                    "80",
                    "-loop",
                    "0",
                ])
                .arg(output_path.as_os_str())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();

            match result {
                Ok(s) if s.success() => s,
                _ => {
                    // Fallback: encode as APNG (animated PNG, widely supported)
                    let apng_path = output_path.with_extension("apng");
                    let s = std::process::Command::new("ffmpeg")
                        .args([
                            "-y",
                            "-framerate",
                            &out_fps.to_string(),
                            "-i",
                            &input_pattern.to_string_lossy(),
                            "-vf",
                            &format!("scale={}:-1:flags=lanczos", scale_width),
                            "-f",
                            "apng",
                            "-plays",
                            "0",
                        ])
                        .arg(apng_path.as_os_str())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status()
                        .map_err(|e| {
                            VidgenError::Ffmpeg(format!("FFmpeg APNG encode failed: {e}"))
                        })?;
                    if s.success() {
                        std::fs::rename(&apng_path, output_path)?;
                    }
                    s
                }
            }
        }
        ExportFormat::Png => unreachable!(),
    };

    if !status.success() {
        return Err(VidgenError::Ffmpeg(format!(
            "FFmpeg {} encoding failed",
            format.extension().to_uppercase()
        )));
    }

    Ok(())
}
