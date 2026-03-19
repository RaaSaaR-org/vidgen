use crate::config::{PlatformPreset, ThemeConfig};
use crate::error::{VidgenError, VidgenResult};
use crate::scene::OverlayConfig;
use chromiumoxide::browser::Browser;
use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
use chromiumoxide::page::ScreenshotParams;
use std::path::Path;
use std::process::{Command, Stdio};
use tracing::debug;

/// Render an overlay PNG (transparent background) via Chromium, then composite
/// it onto a video file via FFmpeg with fade-in/fade-out timing.
#[allow(clippy::too_many_arguments)]
pub async fn apply_overlay(
    browser: &Browser,
    video_path: &Path,
    overlay: &OverlayConfig,
    theme: &ThemeConfig,
    width: u32,
    height: u32,
    scene_duration: f64,
    platform: &PlatformPreset,
) -> VidgenResult<()> {
    let show_at = overlay.show_at.unwrap_or(0.5);
    let hide_at = overlay.hide_at.unwrap_or((scene_duration - 0.5).max(show_at + 0.5));
    let fade_dur = 0.3; // animation duration for enter/exit

    debug!(
        "Rendering overlay: '{}' @ {:.1}s-{:.1}s ({})",
        overlay.text, show_at, hide_at, overlay.style
    );

    // Step 1: Render overlay to transparent PNG via Chromium
    let html = build_overlay_html(overlay, theme, width, height);
    let png_data = render_overlay_png(browser, &html, width, height).await?;

    // Step 2: Write PNG to temp file (also save to debug dir if enabled)
    let temp_dir = tempfile::tempdir()
        .map_err(|e| VidgenError::Other(format!("Failed to create temp dir: {e}")))?;
    let overlay_png = temp_dir.path().join("overlay.png");
    std::fs::write(&overlay_png, &png_data)?;

    if std::env::var("VIDGEN_DEBUG").is_ok() {
        let debug_dir = std::path::PathBuf::from("/tmp/vidgen-debug");
        let _ = std::fs::create_dir_all(&debug_dir);
        let dest = debug_dir.join(format!("overlay-{}.png", overlay.text.chars().take(20).collect::<String>().replace(|c: char| !c.is_alphanumeric(), "-")));
        let _ = std::fs::copy(&overlay_png, &dest);
        debug!("Overlay PNG saved to {}", dest.display());
    }

    // Step 3: Composite onto video via FFmpeg
    composite_overlay(video_path, &overlay_png, show_at, hide_at, fade_dur, platform)?;

    eprintln!(
        "    Overlay: \"{}\" ({:.1}s-{:.1}s, {})",
        overlay.text, show_at, hide_at, overlay.style
    );

    Ok(())
}

/// Render the overlay HTML to a transparent PNG via Chromium.
async fn render_overlay_png(
    browser: &Browser,
    html: &str,
    width: u32,
    height: u32,
) -> VidgenResult<Vec<u8>> {
    let page = browser
        .new_page("about:blank")
        .await
        .map_err(|e| VidgenError::Browser(format!("Failed to create overlay page: {e}")))?;

    page.execute(SetDeviceMetricsOverrideParams::new(
        width as i64,
        height as i64,
        1.0,
        false,
    ))
    .await
    .map_err(|e| VidgenError::Browser(format!("Failed to set viewport: {e}")))?;

    page.set_content(html)
        .await
        .map_err(|e| VidgenError::Browser(format!("Failed to set overlay HTML: {e}")))?;

    // Small delay to let CSS render
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let screenshot = page
        .screenshot(
            ScreenshotParams::builder()
                .full_page(false)
                .omit_background(true) // transparent background
                .build(),
        )
        .await
        .map_err(|e| VidgenError::Browser(format!("Overlay screenshot failed: {e}")))?;

    let _ = page.close().await;
    Ok(screenshot)
}

/// Composite a PNG overlay onto a video with fade-in/fade-out timing.
fn composite_overlay(
    video_path: &Path,
    overlay_png: &Path,
    show_at: f64,
    hide_at: f64,
    fade_dur: f64,
    platform: &PlatformPreset,
) -> VidgenResult<()> {
    let tmp_path = video_path.with_extension("overlay-tmp.mp4");
    std::fs::rename(video_path, &tmp_path)?;

    // FFmpeg filter: overlay the PNG with fade-in/fade-out and timing
    // The overlay PNG is a still image — loop it for the video duration,
    // apply alpha fades, then composite with enable timing.
    let overlay_dur = hide_at - show_at;
    let fade_out_start = (overlay_dur - fade_dur).max(0.0);
    let filter = format!(
        "[1:v]loop=loop=-1:size=1:start=0,setpts=PTS-STARTPTS,\
         fade=in:st=0:d={fade_dur:.2}:alpha=1,\
         fade=out:st={fade_out_start:.2}:d={fade_dur:.2}:alpha=1,\
         format=rgba[ovl];\
         [0:v][ovl]overlay=0:0:shortest=1:enable='between(t,{show_at:.3},{hide_at:.3})'",
    );

    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-y", "-i"])
        .arg(&tmp_path)
        .args(["-i"])
        .arg(overlay_png)
        .args(["-filter_complex", &filter, "-map", "0:a?", "-c:a", "copy"])
        .args([
            "-c:v", "libx264",
            "-pix_fmt", "yuv420p",
            "-crf", &platform.crf.to_string(),
            "-preset", "fast",
            "-movflags", "+faststart",
        ])
        .arg(video_path)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let output = cmd
        .output()
        .map_err(|e| VidgenError::Ffmpeg(format!("Failed to composite overlay: {e}")))?;

    let _ = std::fs::remove_file(&tmp_path);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VidgenError::Ffmpeg(format!(
            "FFmpeg overlay composite failed: {}",
            stderr.lines().last().unwrap_or("unknown error")
        )));
    }

    Ok(())
}

/// Build the overlay HTML with transparent background and the selected style.
fn build_overlay_html(
    overlay: &OverlayConfig,
    theme: &ThemeConfig,
    width: u32,
    height: u32,
) -> String {
    let primary = &theme.primary;
    let text_color = &theme.text;
    let font = &theme.font_body;

    let subtext_html = overlay.subtext.as_ref()
        .map(|s| format!(r#"<div class="subtext">{s}</div>"#))
        .unwrap_or_default();

    let (pos_x, pos_y) = match overlay.position.as_str() {
        "bottom-right" => ("right: 40px", "bottom: 60px"),
        "top-left" => ("left: 40px", "top: 60px"),
        "top-right" => ("right: 40px", "top: 60px"),
        _ => ("left: 40px", "bottom: 60px"), // bottom-left default
    };

    let style_css = match overlay.style.as_str() {
        "minimal" => format!(r#"
            .overlay {{
                background: rgba(0, 0, 0, 0.6);
                padding: 12px 24px;
                border-radius: 6px;
            }}
            .text {{ color: {text_color}; font-size: 28px; font-weight: 600; }}
            .subtext {{ color: rgba(255,255,255,0.7); font-size: 18px; margin-top: 4px; }}
        "#),
        "news" => format!(r#"
            .overlay {{
                background: linear-gradient(90deg, {primary} 0%, {primary}dd 100%);
                padding: 14px 28px;
                border-left: 5px solid {text_color};
            }}
            .text {{ color: {text_color}; font-size: 30px; font-weight: 700; text-transform: uppercase; letter-spacing: 1px; }}
            .subtext {{ color: rgba(255,255,255,0.9); font-size: 18px; margin-top: 4px; font-weight: 400; }}
        "#),
        "gradient" => format!(r#"
            .overlay {{
                background: linear-gradient(135deg, {primary}ee 0%, {primary}88 50%, transparent 100%);
                padding: 16px 32px 16px 24px;
                border-radius: 0 8px 8px 0;
                min-width: 300px;
            }}
            .text {{ color: {text_color}; font-size: 28px; font-weight: 600; }}
            .subtext {{ color: rgba(255,255,255,0.8); font-size: 17px; margin-top: 4px; }}
        "#),
        _ => format!(r#"
            .overlay {{
                background: rgba(0, 0, 0, 0.45);
                backdrop-filter: blur(12px);
                -webkit-backdrop-filter: blur(12px);
                padding: 14px 28px;
                border-radius: 8px;
                border-left: 4px solid {primary};
            }}
            .text {{ color: {text_color}; font-size: 28px; font-weight: 600; }}
            .subtext {{ color: rgba(255,255,255,0.7); font-size: 18px; margin-top: 4px; }}
        "#), // modern (default)
    };

    format!(
        r##"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  html, body {{
    width: {width}px;
    height: {height}px;
    background: transparent;
    font-family: {font};
    overflow: hidden;
  }}
  .container {{
    position: absolute;
    {pos_x};
    {pos_y};
    max-width: 60%;
  }}
  {style_css}
</style>
</head>
<body>
  <div class="container">
    <div class="overlay">
      <div class="text">{text}</div>
      {subtext_html}
    </div>
  </div>
</body>
</html>"##,
        width = width,
        height = height,
        font = font,
        pos_x = pos_x,
        pos_y = pos_y,
        style_css = style_css,
        text = overlay.text,
        subtext_html = subtext_html,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ThemeConfig;
    use crate::scene::OverlayConfig;

    fn default_theme() -> ThemeConfig {
        ThemeConfig::default()
    }

    fn make_overlay(text: &str, style: &str) -> OverlayConfig {
        OverlayConfig {
            text: text.into(),
            subtext: Some("subtitle".into()),
            show_at: None,
            hide_at: None,
            style: style.into(),
            position: "bottom-left".into(),
        }
    }

    #[test]
    fn test_build_overlay_html_modern() {
        let html = build_overlay_html(&make_overlay("Test", "modern"), &default_theme(), 1920, 1080);
        assert!(html.contains("Test"));
        assert!(html.contains("subtitle"));
        assert!(html.contains("backdrop-filter"));
        assert!(html.contains("background: transparent"));
    }

    #[test]
    fn test_build_overlay_html_news() {
        let html = build_overlay_html(&make_overlay("Breaking", "news"), &default_theme(), 1920, 1080);
        assert!(html.contains("text-transform: uppercase"));
    }

    #[test]
    fn test_build_overlay_html_minimal() {
        let html = build_overlay_html(&make_overlay("Info", "minimal"), &default_theme(), 1920, 1080);
        assert!(html.contains("border-radius: 6px"));
    }

    #[test]
    fn test_build_overlay_html_positions() {
        let mut ov = make_overlay("Test", "modern");

        ov.position = "bottom-left".into();
        let html = build_overlay_html(&ov, &default_theme(), 1920, 1080);
        assert!(html.contains("left: 40px"));
        assert!(html.contains("bottom: 60px"));

        ov.position = "top-right".into();
        let html = build_overlay_html(&ov, &default_theme(), 1920, 1080);
        assert!(html.contains("right: 40px"));
        assert!(html.contains("top: 60px"));
    }

    #[test]
    fn test_build_overlay_no_subtext() {
        let ov = OverlayConfig {
            text: "Solo".into(),
            subtext: None,
            show_at: None,
            hide_at: None,
            style: "minimal".into(),
            position: "bottom-left".into(),
        };
        let html = build_overlay_html(&ov, &default_theme(), 1920, 1080);
        assert!(html.contains("Solo"));
        assert!(!html.contains(r#"class="subtext">"#)); // no subtext div rendered
    }
}
