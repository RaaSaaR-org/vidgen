use crate::config::{PlatformPreset, ThemeConfig};
use crate::error::{VidgenError, VidgenResult};
use crate::render::encoder::SceneEncoder;
use crate::render::frame_cache;
use crate::scene::Scene;
use crate::template::TemplateRegistry;
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
use chromiumoxide::page::ScreenshotParams;
use futures::StreamExt;
use std::path::Path;

/// Capture a single frame as PNG bytes. Launches a browser, renders the HTML,
/// injects CSS custom properties, takes a screenshot, and returns PNG data.
///
/// This is the shared helper used by both the `preview` CLI command and
/// the MCP `preview_scene` tool.
pub async fn capture_single_frame(
    html: &str,
    width: u32,
    height: u32,
    frame: u32,
    total_frames: u32,
) -> VidgenResult<Vec<u8>> {
    let (browser, handler_handle) = launch_browser(width, height).await?;

    let page = browser
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

    page.set_content(html)
        .await
        .map_err(|e| VidgenError::Browser(format!("Failed to set page content: {e}")))?;

    // Inject CSS custom properties
    let progress = if total_frames > 0 {
        frame as f64 / total_frames as f64
    } else {
        0.0
    };
    let js = format!(
        "document.documentElement.style.setProperty('--frame', '{}');\
         document.documentElement.style.setProperty('--total-frames', '{}');\
         document.documentElement.style.setProperty('--progress', '{}');\
         document.documentElement.style.setProperty('--content-progress', '{}');",
        frame, total_frames, progress, progress
    );
    page.evaluate(js)
        .await
        .map_err(|e| VidgenError::Browser(format!("JS injection failed: {e}")))?;

    let screenshot = page
        .screenshot(ScreenshotParams::builder().full_page(false).build())
        .await
        .map_err(|e| VidgenError::Browser(format!("Screenshot failed: {e}")))?;

    let _ = page.close().await;
    drop(browser);
    handler_handle.abort();

    Ok(screenshot)
}

/// Launch a headless Chromium browser instance.
pub async fn launch_browser(
    width: u32,
    height: u32,
) -> VidgenResult<(Browser, tokio::task::JoinHandle<()>)> {
    let config = BrowserConfig::builder()
        .window_size(width, height)
        .viewport(None) // We'll set viewport per-page via CDP
        .arg("--hide-scrollbars")
        .arg("--disable-gpu")
        .arg("--no-sandbox")
        .arg("--disable-dev-shm-usage")
        .build()
        .map_err(|e| VidgenError::Browser(format!("Failed to configure browser: {e}")))?;

    let (browser, mut handler) = Browser::launch(config)
        .await
        .map_err(|e| VidgenError::Browser(format!("Failed to launch browser: {e}")))?;

    // Spawn the browser handler as a background task
    let handle = tokio::spawn(async move {
        while let Some(event) = handler.next().await {
            if event.is_err() {
                break;
            }
        }
    });

    Ok((browser, handle))
}

/// Capture all frames for a scene: render HTML per frame, screenshot, pipe to encoder.
#[allow(clippy::too_many_arguments)]
pub async fn capture_scene_frames(
    browser: &Browser,
    scene: &Scene,
    scene_index: usize,
    registry: &TemplateRegistry<'_>,
    theme: &ThemeConfig,
    width: u32,
    height: u32,
    fps: u32,
    platform: &PlatformPreset,
    output_path: &Path,
    audio_path: Option<&Path>,
    music_path: Option<&Path>,
    music_volume: f64,
    effective_duration: f64,
    audio_delay_secs: f64,
    content_padding_after: f64,
) -> VidgenResult<std::path::PathBuf> {
    let total_frames = Scene::total_frames_for_duration(effective_duration, fps);

    // Create a new page (tab) for this scene
    let page = browser
        .new_page("about:blank")
        .await
        .map_err(|e| VidgenError::Browser(format!("Failed to create page: {e}")))?;

    // Set viewport size via CDP command
    page.execute(SetDeviceMetricsOverrideParams::new(
        width as i64,
        height as i64,
        1.0,   // device_scale_factor
        false, // mobile
    ))
    .await
    .map_err(|e| VidgenError::Browser(format!("Failed to set viewport: {e}")))?;

    // Render frame 0 to check if the scene is static
    let html_frame0 = registry.render_scene_html(scene, theme, width, height, 0, total_frames)?;
    let is_static = frame_cache::is_static_scene(&html_frame0);

    if is_static {
        // Static scene: capture one frame, loop it with FFmpeg
        eprintln!(
            "  Scene {}: static, 1 frame captured ({:.1}s)",
            scene_index + 1,
            effective_duration
        );

        page.set_content(&html_frame0)
            .await
            .map_err(|e| VidgenError::Browser(format!("Failed to set page content: {e}")))?;

        let screenshot = page
            .screenshot(ScreenshotParams::builder().full_page(false).build())
            .await
            .map_err(|e| VidgenError::Browser(format!("Screenshot failed: {e}")))?;

        let output = SceneEncoder::encode_static(
            output_path,
            fps,
            width,
            height,
            effective_duration,
            platform,
            audio_path,
            music_path,
            music_volume,
            audio_delay_secs,
            &screenshot,
        )?;

        let _ = page.close().await;
        return Ok(output);
    }

    // Animated scene: render every frame
    // Start the encoder for this scene
    let mut encoder = SceneEncoder::new(
        output_path,
        fps,
        width,
        height,
        platform,
        audio_path,
        music_path,
        music_volume,
        audio_delay_secs,
    )?;

    // Compute content-progress boundaries (voice window within full scene duration)
    let content_start_frame = audio_delay_secs * fps as f64;
    let content_end_frame = (effective_duration - content_padding_after) * fps as f64;

    eprintln!(
        "  Scene {}: {} frames ({:.1}s)",
        scene_index + 1,
        total_frames,
        effective_duration
    );

    for frame in 0..total_frames {
        // Render HTML with current frame number
        let html = if frame == 0 {
            html_frame0.clone()
        } else {
            registry.render_scene_html(scene, theme, width, height, frame, total_frames)?
        };

        // Load the HTML into the page
        page.set_content(&html)
            .await
            .map_err(|e| VidgenError::Browser(format!("Failed to set page content: {e}")))?;

        // Inject CSS custom properties via JavaScript for dynamic animation
        let content_range = content_end_frame - content_start_frame;
        let content_progress = if content_range > 0.0 {
            ((frame as f64 - content_start_frame) / content_range).clamp(0.0, 1.0)
        } else {
            frame as f64 / total_frames as f64
        };
        let js = format!(
            "document.documentElement.style.setProperty('--frame', '{}');\
             document.documentElement.style.setProperty('--total-frames', '{}');\
             document.documentElement.style.setProperty('--progress', '{}');\
             document.documentElement.style.setProperty('--content-progress', '{}');",
            frame,
            total_frames,
            frame as f64 / total_frames as f64,
            content_progress
        );
        page.evaluate(js)
            .await
            .map_err(|e| VidgenError::Browser(format!("JS injection failed: {e}")))?;

        // Take screenshot as PNG
        let screenshot = page
            .screenshot(ScreenshotParams::builder().full_page(false).build())
            .await
            .map_err(|e| VidgenError::Browser(format!("Screenshot failed: {e}")))?;

        // Pipe PNG bytes to encoder
        encoder.write_frame(&screenshot)?;

        // Progress reporting
        if (frame + 1) % 30 == 0 || frame + 1 == total_frames {
            eprint!(
                "\r    Frame {}/{} ({:.0}%)",
                frame + 1,
                total_frames,
                (frame + 1) as f64 / total_frames as f64 * 100.0
            );
        }
    }
    eprintln!(); // Newline after progress

    // Finalize encoding
    let output = encoder.finish()?;

    // Close the page
    let _ = page.close().await;

    Ok(output)
}
