use crate::cli::ClipAction;
use crate::error::{VidgenError, VidgenResult};
use colored::*;
use std::path::Path;

pub async fn run(action: ClipAction) -> VidgenResult<()> {
    match action {
        #[cfg(feature = "clipper")]
        ClipAction::Web {
            url,
            project,
            duration,
            width,
            height,
            scroll_speed,
            output,
            wait,
            fps,
        } => {
            capture_web(
                &url,
                &project,
                duration,
                width,
                height,
                scroll_speed,
                output.as_deref(),
                wait,
                fps,
            )
            .await
        }
        #[cfg(feature = "youtube")]
        ClipAction::Youtube {
            url,
            project,
            from,
            to,
            output,
        } => download_youtube(&url, &project, from, to, output.as_deref()).await,
    }
}

// ---------------------------------------------------------------------------
// Web capture (feature = "clipper")
// ---------------------------------------------------------------------------

/// Capture a scrolling website as a video clip.
///
/// Launches headless Chromium, navigates to the URL, waits for the page to load,
/// then scrolls while capturing screenshots frame-by-frame piped to FFmpeg.
#[cfg(feature = "clipper")]
#[allow(clippy::too_many_arguments)]
async fn capture_web(
    url: &str,
    project_path: &Path,
    duration: f64,
    width: u32,
    height: u32,
    scroll_speed: u32,
    output_name: Option<&str>,
    wait_secs: f64,
    fps: u32,
) -> VidgenResult<()> {
    use crate::render::browser;
    use crate::render::encoder::SceneEncoder;
    use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
    use chromiumoxide::page::ScreenshotParams;

    let clips_dir = project_path.join("assets/clips");
    std::fs::create_dir_all(&clips_dir)?;

    let filename = make_web_filename(url, output_name);
    let output_path = clips_dir.join(&filename);

    eprintln!(
        "{} Capturing website: {}",
        "clip:".cyan().bold(),
        url
    );
    eprintln!(
        "{} {}x{}, {:.1}s, {}fps, scroll {}px/s",
        "clip:".cyan().bold(),
        width, height, duration, fps, scroll_speed,
    );

    // Launch browser
    let (browser_instance, handler_handle) = browser::launch_browser(width, height).await?;

    let page = browser_instance
        .new_page("about:blank")
        .await
        .map_err(|e| VidgenError::Browser(format!("Failed to create page: {e}")))?;

    page.execute(SetDeviceMetricsOverrideParams::new(
        width as i64,
        height as i64,
        1.0,
        false,
    ))
    .await
    .map_err(|e| VidgenError::Browser(format!("Failed to set viewport: {e}")))?;

    // Navigate to URL
    eprintln!("{} Loading page...", "clip:".cyan().bold());
    page.goto(url)
        .await
        .map_err(|e| VidgenError::Browser(format!("Failed to navigate to {url}: {e}")))?;

    // Wait for page to load
    if wait_secs > 0.0 {
        eprintln!(
            "{} Waiting {:.1}s for page to load...",
            "clip:".cyan().bold(),
            wait_secs,
        );
        tokio::time::sleep(tokio::time::Duration::from_secs_f64(wait_secs)).await;
    }

    // Start encoder
    let quality = crate::config::QualityPreset::from_name("standard");
    let platform = crate::config::resolve_encoding(&quality, Some("youtube-hd"));
    let total_frames = (duration * fps as f64).ceil() as u32;
    let pixels_per_frame = scroll_speed as f64 / fps as f64;

    let mut encoder = SceneEncoder::new(
        &output_path, fps, width, height, &platform,
        None, None, 0.0, 0.0, None, false,
    )?;

    eprintln!(
        "{} Capturing {} frames...",
        "clip:".cyan().bold(),
        total_frames,
    );

    for frame in 0..total_frames {
        let scroll_y = (frame as f64 * pixels_per_frame) as u64;
        let js = format!("window.scrollTo(0, {scroll_y});");
        page.evaluate(js)
            .await
            .map_err(|e| VidgenError::Browser(format!("Scroll failed: {e}")))?;

        tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;

        let screenshot = page
            .screenshot(ScreenshotParams::builder().full_page(false).build())
            .await
            .map_err(|e| VidgenError::Browser(format!("Screenshot failed: {e}")))?;

        encoder.write_frame(&screenshot)?;

        if (frame + 1) % 30 == 0 || frame + 1 == total_frames {
            let pct = (frame + 1) as f64 / total_frames as f64;
            let bar_width = 20;
            let filled = (pct * bar_width as f64) as usize;
            let empty = bar_width - filled;
            eprint!(
                "\r    [{}{}] {:.0}% ({}/{})",
                "█".repeat(filled),
                "░".repeat(empty),
                pct * 100.0,
                frame + 1,
                total_frames,
            );
        }
    }
    eprintln!();

    let output = encoder.finish()?;

    let _ = page.close().await;
    drop(browser_instance);
    handler_handle.abort();

    eprintln!("{} Saved: {}", "done:".green().bold(), output.display());
    eprintln!(
        "{} Use in a scene with: video_source: \"@assets/clips/{}\"",
        "hint:".yellow().bold(),
        filename,
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// YouTube download (feature = "youtube")
// ---------------------------------------------------------------------------

/// Download a YouTube video clip and optionally trim it.
///
/// Uses the `yt-dlp` crate which auto-downloads the yt-dlp binary on first run.
/// Supports time-range trimming via FFmpeg.
#[cfg(feature = "youtube")]
async fn download_youtube(
    url: &str,
    project_path: &Path,
    from: Option<f64>,
    to: Option<f64>,
    output_name: Option<&str>,
) -> VidgenResult<()> {
    use yt_dlp::Downloader;

    let clips_dir = project_path.join("assets/clips");
    std::fs::create_dir_all(&clips_dir)?;

    // Store yt-dlp and ffmpeg binaries in ~/.vidgen/libs/
    let libs_dir = libs_path()?;

    eprintln!(
        "{} Fetching video info: {}",
        "clip:".cyan().bold(),
        url,
    );

    // Install yt-dlp binary if not cached
    let installer = yt_dlp::client::deps::LibraryInstaller::new(libs_dir.clone());

    let yt_dlp_bin = if libs_dir.join("yt-dlp").exists() || libs_dir.join("yt-dlp_macos").exists() {
        // Find existing binary
        let candidates = ["yt-dlp", "yt-dlp_macos", "yt-dlp.exe"];
        candidates.iter()
            .map(|c| libs_dir.join(c))
            .find(|p| p.exists())
            .unwrap_or_else(|| libs_dir.join("yt-dlp"))
    } else {
        eprintln!("{} Downloading yt-dlp binary (first run)...", "clip:".cyan().bold());
        installer
            .install_youtube(None)
            .await
            .map_err(|e| VidgenError::Other(format!("Failed to install yt-dlp: {e}")))?
    };

    // Use system FFmpeg (vidgen already requires it)
    let ffmpeg_path = which_ffmpeg()?;

    let libraries = yt_dlp::client::deps::Libraries::new(yt_dlp_bin, ffmpeg_path);
    let downloader = Downloader::builder(libraries, &clips_dir)
        .build()
        .await
        .map_err(|e| VidgenError::Other(format!("Failed to initialize yt-dlp: {e}")))?;

    // Fetch video metadata
    let video = downloader
        .fetch_video_infos(url)
        .await
        .map_err(|e| VidgenError::Other(format!("Failed to fetch video info: {e}")))?;

    let title = &video.title;
    let duration_secs = video.duration.unwrap_or(0) as f64;

    eprintln!(
        "{} \"{}\" ({:.0}s)",
        "clip:".cyan().bold(),
        title,
        duration_secs,
    );

    // Generate filename
    let filename = match output_name {
        Some(name) => {
            if name.ends_with(".mp4") {
                name.to_string()
            } else {
                format!("{name}.mp4")
            }
        }
        None => make_youtube_filename(title),
    };

    let final_path = clips_dir.join(&filename);

    // Use a temp dir for the raw download (yt-dlp creates intermediate files)
    let temp_dir = tempfile::tempdir()
        .map_err(|e| VidgenError::Other(format!("Failed to create temp dir: {e}")))?;
    let raw_path = temp_dir.path().join("download.mp4");

    eprintln!("{} Downloading...", "clip:".cyan().bold());

    // Download best quality
    downloader
        .download_video_to_path(&video, &raw_path)
        .await
        .map_err(|e| VidgenError::Other(format!("YouTube download failed: {e}")))?;

    // Verify download succeeded
    let raw_size = std::fs::metadata(&raw_path)
        .map(|m| m.len())
        .unwrap_or(0);
    if raw_size == 0 {
        return Err(VidgenError::Other(
            "Download produced an empty file. The video may be restricted or unavailable.".into(),
        ));
    }

    eprintln!(
        "{} Downloaded {:.1}MB",
        "clip:".cyan().bold(),
        raw_size as f64 / 1_048_576.0,
    );

    // Re-encode to H.264+AAC for universal playback (yt-dlp often outputs VP9/Opus)
    // Also trim if from/to specified
    let from_secs = from.unwrap_or(0.0);
    let to_secs = to.unwrap_or(duration_secs);
    let needs_trim = from.is_some() || to.is_some();

    if needs_trim {
        eprintln!(
            "{} Trimming + encoding H.264: {:.1}s - {:.1}s ({:.1}s clip)",
            "clip:".cyan().bold(),
            from_secs, to_secs, to_secs - from_secs,
        );
    } else {
        eprintln!("{} Encoding to H.264...", "clip:".cyan().bold());
    }

    reencode_to_h264(&raw_path, &final_path, if needs_trim { Some((from_secs, to_secs)) } else { None })?;

    eprintln!("{} Saved: {}", "done:".green().bold(), final_path.display());
    eprintln!(
        "{} Use in a scene with: video_source: \"@assets/clips/{}\"",
        "hint:".yellow().bold(),
        filename,
    );

    Ok(())
}

/// Re-encode a video to H.264+AAC for universal playback, with optional trim.
#[cfg(feature = "youtube")]
fn reencode_to_h264(
    input: &Path,
    output: &Path,
    trim: Option<(f64, f64)>,
) -> VidgenResult<()> {
    use std::process::{Command, Stdio};

    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-y", "-i"]).arg(input);

    if let Some((from, to)) = trim {
        cmd.args(["-ss", &format!("{from:.3}")]);
        cmd.args(["-to", &format!("{to:.3}")]);
    }

    cmd.args([
        "-c:v", "libx264",
        "-pix_fmt", "yuv420p",
        "-crf", "18",
        "-preset", "medium",
        "-c:a", "aac",
        "-b:a", "192k",
        "-movflags", "+faststart",
    ]);
    cmd.arg(output);
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());

    let result = cmd
        .output()
        .map_err(|e| VidgenError::Ffmpeg(format!("Failed to run ffmpeg: {e}")))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(VidgenError::Ffmpeg(format!(
            "FFmpeg encode failed: {}",
            stderr.lines().last().unwrap_or("unknown error")
        )));
    }

    Ok(())
}

/// Resolve the ~/.vidgen/libs/ directory for cached binaries.
#[cfg(feature = "youtube")]
fn libs_path() -> VidgenResult<std::path::PathBuf> {
    let home = std::env::var("HOME")
        .map_err(|_| VidgenError::Other("Could not determine HOME directory".into()))?;
    let path = std::path::PathBuf::from(home).join(".vidgen/libs");
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

/// Find the system FFmpeg binary.
#[cfg(feature = "youtube")]
fn which_ffmpeg() -> VidgenResult<std::path::PathBuf> {
    let output = std::process::Command::new("which")
        .arg("ffmpeg")
        .output()
        .map_err(|e| VidgenError::Ffmpeg(format!("Failed to find ffmpeg: {e}")))?;
    if !output.status.success() {
        return Err(VidgenError::Ffmpeg(
            "FFmpeg not found on PATH. Install via: brew install ffmpeg".into(),
        ));
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(std::path::PathBuf::from(path))
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

#[cfg(feature = "clipper")]
fn make_web_filename(url: &str, output_name: Option<&str>) -> String {
    match output_name {
        Some(name) => {
            if name.ends_with(".mp4") {
                name.to_string()
            } else {
                format!("{name}.mp4")
            }
        }
        None => {
            let slug = url
                .replace("https://", "")
                .replace("http://", "")
                .replace(|c: char| !c.is_alphanumeric(), "-")
                .trim_matches('-')
                .chars()
                .take(50)
                .collect::<String>();
            format!("web-{slug}.mp4")
        }
    }
}

#[cfg(feature = "youtube")]
fn make_youtube_filename(title: &str) -> String {
    let slug = title
        .replace(|c: char| !c.is_alphanumeric() && c != ' ', "")
        .replace(' ', "-")
        .to_lowercase()
        .chars()
        .take(50)
        .collect::<String>();
    format!("yt-{slug}.mp4")
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "clipper")]
    #[test]
    fn test_web_filename_generation() {
        let filename = super::make_web_filename("https://example.com/some-page?q=test", None);
        assert!(filename.starts_with("web-"));
        assert!(filename.ends_with(".mp4"));
        assert!(!filename.contains("https"));
    }

    #[cfg(feature = "clipper")]
    #[test]
    fn test_web_filename_custom() {
        assert_eq!(super::make_web_filename("https://x.com", Some("my-clip")), "my-clip.mp4");
        assert_eq!(super::make_web_filename("https://x.com", Some("my-clip.mp4")), "my-clip.mp4");
    }

    #[cfg(feature = "youtube")]
    #[test]
    fn test_youtube_filename_generation() {
        assert_eq!(
            super::make_youtube_filename("My Cool Video! (2024)"),
            "yt-my-cool-video-2024.mp4"
        );
    }
}
