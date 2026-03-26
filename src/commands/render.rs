use crate::config;
use crate::error::{VidgenError, VidgenResult};
use crate::scene;
use colored::*;
use serde::Serialize;
use std::path::Path;
use std::process::Command as ProcessCommand;

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
    force_tts: bool,
    no_cache: bool,
    gpu: bool,
    speed: Option<f32>,
) -> VidgenResult<Vec<RenderResult>> {
    if !path.exists() {
        return Err(VidgenError::ProjectNotFound(path.to_path_buf()));
    }

    // Load config and validate
    let mut config = config::load_config(path)?;
    config.validate()?;

    // Apply overrides
    if let Some(s) = speed {
        config.voice.speed = s;
    }
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
        force_tts,
        no_cache,
        gpu,
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
        false, // MCP doesn't support force_tts yet
        false, // MCP doesn't support no_cache yet
        false, // MCP doesn't support gpu yet
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
    force_tts: bool,
    no_cache: bool,
    gpu: bool,
    speed: Option<f32>,
    crop: Option<&str>,
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
        force_tts,
        no_cache,
        gpu,
        speed,
    )
    .await?;
    for r in &results {
        eprintln!(
            "  Format \"{}\": {} scenes, {:.1}s total → {}",
            r.format_name, r.scenes_rendered, r.duration_secs, r.output_path
        );

        let video_path = std::path::Path::new(&r.output_path);

        // Apply crop if requested
        if let Some(aspect) = crop {
            if video_path.exists() {
                match crate::render::encoder::apply_crop(video_path, aspect) {
                    Ok(()) => eprintln!("  Cropped to {}", aspect),
                    Err(e) => eprintln!("  {} Crop failed: {}", "warning:".yellow().bold(), e),
                }
            }
        }

        // Print quality report for the output file
        if video_path.exists() {
            if let Err(e) = print_quality_report(video_path) {
                eprintln!(
                    "  {} Could not generate quality report: {}",
                    "warning:".yellow().bold(),
                    e
                );
            }
        }
    }
    Ok(())
}

/// Probe the rendered video file and print a quality report with key metrics.
fn print_quality_report(video_path: &Path) -> VidgenResult<()> {
    // Run ffprobe to get format and stream info as JSON
    let output = ProcessCommand::new("ffprobe")
        .args([
            "-v", "quiet",
            "-show_entries", "format=duration,size,bit_rate",
            "-show_entries", "stream=codec_type,codec_name,width,height,bit_rate,sample_rate,channels",
            "-of", "json",
        ])
        .arg(video_path)
        .output()
        .map_err(|e| VidgenError::Ffmpeg(format!("Failed to run ffprobe: {e}")))?;

    if !output.status.success() {
        return Err(VidgenError::Ffmpeg("ffprobe exited with non-zero status".into()));
    }

    let json_str = String::from_utf8_lossy(&output.stdout);
    let data: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| VidgenError::Ffmpeg(format!("Failed to parse ffprobe JSON: {e}")))?;

    // Extract format-level info
    let format = &data["format"];
    let duration_secs: f64 = format["duration"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let file_size: u64 = format["size"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    // Extract stream info
    let streams = data["streams"].as_array();
    let mut video_info: Option<String> = None;
    let mut audio_info: Option<String> = None;

    if let Some(streams) = streams {
        for stream in streams {
            let codec_type = stream["codec_type"].as_str().unwrap_or("");
            match codec_type {
                "video" => {
                    let w = stream["width"].as_u64().unwrap_or(0);
                    let h = stream["height"].as_u64().unwrap_or(0);
                    let codec = stream["codec_name"].as_str().unwrap_or("unknown");
                    let bitrate: u64 = stream["bit_rate"]
                        .as_str()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                    let bitrate_str = if bitrate > 0 {
                        format_bitrate(bitrate)
                    } else {
                        // Fall back to format-level bitrate
                        let fmt_br: u64 = format["bit_rate"]
                            .as_str()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0);
                        if fmt_br > 0 { format_bitrate(fmt_br) } else { "N/A".to_string() }
                    };
                    video_info = Some(format!("{}x{}, {}, {}", w, h, bitrate_str, codec));
                }
                "audio" => {
                    let codec = stream["codec_name"].as_str().unwrap_or("unknown");
                    let bitrate: u64 = stream["bit_rate"]
                        .as_str()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                    let sample_rate = stream["sample_rate"]
                        .as_str()
                        .and_then(|s| s.parse::<u32>().ok())
                        .unwrap_or(0);
                    let channels = stream["channels"].as_u64().unwrap_or(0);
                    let ch_str = match channels {
                        1 => "mono",
                        2 => "stereo",
                        _ => "multi",
                    };
                    let sr_str = if sample_rate > 0 {
                        format!("{:.1}kHz", sample_rate as f64 / 1000.0)
                    } else {
                        "N/A".to_string()
                    };
                    let br_str = if bitrate > 0 { format_bitrate(bitrate) } else { "N/A".to_string() };
                    audio_info = Some(format!("{}, {}, {} {}", br_str, codec, ch_str, sr_str));
                }
                _ => {}
            }
        }
    }

    // Format file size
    let size_str = format_file_size(file_size);

    // Detect audio peak level
    let peak_warning = detect_audio_peak(video_path);

    // Print the report
    eprintln!();
    eprintln!("{}", "Quality Report:".cyan().bold());
    if let Some(vi) = video_info {
        eprintln!("  Video: {}", vi);
    }
    if let Some(ai) = audio_info {
        eprintln!("  Audio: {}", ai);
    }
    eprintln!("  File:  {} ({:.1}s)", size_str, duration_secs);
    if let Some(peak) = peak_warning {
        eprintln!("  {} {}", "\u{26A0}".yellow(), peak);
    }

    Ok(())
}

/// Format a bitrate in bits/s to a human-readable string.
fn format_bitrate(bps: u64) -> String {
    if bps >= 1_000_000 {
        format!("{:.1} Mbps", bps as f64 / 1_000_000.0)
    } else if bps >= 1_000 {
        format!("{} kbps", bps / 1_000)
    } else {
        format!("{} bps", bps)
    }
}

/// Format a file size in bytes to a human-readable string.
fn format_file_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1_024 {
        format!("{:.1} KB", bytes as f64 / 1_024.0)
    } else {
        format!("{} B", bytes)
    }
}

/// Run ffmpeg volumedetect and return a warning string if audio peak is near clipping.
fn detect_audio_peak(video_path: &Path) -> Option<String> {
    let output = ProcessCommand::new("ffmpeg")
        .args(["-i"])
        .arg(video_path)
        .args(["-af", "volumedetect", "-f", "null", "-"])
        .output()
        .ok()?;

    // volumedetect outputs to stderr
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Parse max_volume line: "max_volume: -0.1 dB"
    for line in stderr.lines() {
        if let Some(pos) = line.find("max_volume:") {
            let rest = &line[pos + "max_volume:".len()..];
            let rest = rest.trim();
            if let Some(db_str) = rest.strip_suffix("dB").or_else(|| rest.strip_suffix("dB\r")) {
                if let Ok(db) = db_str.trim().parse::<f64>() {
                    if db > -1.0 {
                        return Some(format!("Audio peak: {:.1}dB (near clipping)", db));
                    }
                }
            }
        }
    }

    None
}
