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
use std::io::Write;
use std::path::Path;
use tracing::{debug, warn};

/// Write HTML to a temporary file and return the handle + file:// URL.
///
/// Using file:// navigation (instead of `set_content`) gives the page a file://
/// origin, enabling JavaScript `fetch()` for local assets — required for templates
/// that load 3D models, fonts, or other binary assets via JS (e.g. Three.js).
fn write_temp_html(html: &str) -> VidgenResult<(tempfile::NamedTempFile, String)> {
    let mut temp = tempfile::Builder::new()
        .prefix("vidgen_")
        .suffix(".html")
        .tempfile()
        .map_err(|e| VidgenError::Browser(format!("Failed to create temp file: {e}")))?;

    temp.write_all(html.as_bytes())
        .map_err(|e| VidgenError::Browser(format!("Failed to write temp file: {e}")))?;
    temp.flush()
        .map_err(|e| VidgenError::Browser(format!("Failed to flush temp file: {e}")))?;

    let url = format!("file://{}", temp.path().display());
    Ok((temp, url))
}

/// Wait for the page to be fully loaded and ready for screenshots.
///
/// Handles two concerns:
/// 1. Basic page load (`document.readyState === 'complete'`) — scripts from CDN etc.
/// 2. Async template readiness (`window.__VIDGEN_READY__`) — for templates that load
///    external resources asynchronously (e.g. Three.js loading a GLB model).
///
/// Templates opt into async readiness by setting `window.__VIDGEN_READY__ = false`
/// at startup, then `= true` once resources are loaded. Templates that don't set
/// this flag are considered ready as soon as the page loads.
async fn wait_for_page_ready(page: &chromiumoxide::Page) -> VidgenResult<()> {
    use std::time::{Duration, Instant};

    let start = Instant::now();
    let timeout = Duration::from_secs(30);

    loop {
        if start.elapsed() > timeout {
            warn!("Page readiness timeout (30s) — proceeding with render");
            return Ok(());
        }

        // Combined check: page loaded AND template ready (or no async flag set)
        match page
            .evaluate(
                "document.readyState === 'complete' && window.__VIDGEN_READY__ !== false",
            )
            .await
        {
            Ok(result) => {
                if result.into_value::<bool>().unwrap_or(false) {
                    return Ok(());
                }
            }
            Err(_) => {} // Page context not ready for JS yet (mid-navigation)
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

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

    // Write HTML to temp file so the page gets a file:// origin,
    // enabling JS fetch() for local assets (e.g., Three.js loading GLB models)
    let (_temp_file, file_url) = write_temp_html(html)?;

    let page = browser
        .new_page(&file_url)
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

    wait_for_page_ready(&page).await?;

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
    debug!("Launching headless browser ({}x{})", width, height);
    let config = BrowserConfig::builder()
        .window_size(width, height)
        .viewport(None) // We'll set viewport per-page via CDP
        .arg("--hide-scrollbars")
        // Note: --disable-gpu was removed to enable WebGL (Three.js, 3D models).
        // Headless Chromium uses SwiftShader (software) on Linux or the system
        // GPU on macOS — both produce deterministic screenshots.
        .arg("--no-sandbox")
        .arg("--disable-dev-shm-usage")
        .arg("--allow-file-access-from-files")
        .arg("--allow-file-access")
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
    project_path: Option<&Path>,
) -> VidgenResult<std::path::PathBuf> {
    let total_frames = Scene::total_frames_for_duration(effective_duration, fps);
    debug!(
        "capture_scene_frames: scene={}, frames={}, static=pending, duration={:.1}s",
        scene_index, total_frames, effective_duration
    );

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
    let html_frame0 = registry.render_scene_html(scene, theme, width, height, 0, total_frames, project_path)?;
    let is_static = frame_cache::is_static_scene(&html_frame0);

    // Load HTML via file:// URL (enables JS fetch for local assets like 3D models)
    let (_temp_file, file_url) = write_temp_html(&html_frame0)?;
    page.goto(&file_url)
        .await
        .map_err(|e| VidgenError::Browser(format!("Failed to navigate to scene HTML: {e}")))?;
    wait_for_page_ready(&page).await?;

    if is_static {
        // Static scene: capture one frame, pipe it N times to the encoder.
        // This avoids FFmpeg's `-loop 1` flag which hangs on Apple Silicon
        // when combined with a finite audio input.
        eprintln!(
            "  Scene {}: static, 1 frame captured ({:.1}s)",
            scene_index + 1,
            effective_duration
        );

        let screenshot = page
            .screenshot(ScreenshotParams::builder().full_page(false).build())
            .await
            .map_err(|e| VidgenError::Browser(format!("Screenshot failed: {e}")))?;

        let mut encoder = SceneEncoder::new(
            output_path, fps, width, height, platform,
            audio_path, music_path, music_volume, audio_delay_secs,
            Some(effective_duration),
        )?;
        for _ in 0..total_frames {
            encoder.write_frame(&screenshot)?;
        }
        let output = encoder.finish()?;

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
        Some(effective_duration),
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

    // HTML already loaded via page.goto() above — the template output is identical
    // across frames; only the CSS custom properties change (injected via JS below).

    for frame in 0..total_frames {
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

        // Progress reporting with visual bar
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
    eprintln!(); // Newline after progress

    // Finalize encoding
    let output = encoder.finish()?;

    // Close the page
    let _ = page.close().await;

    Ok(output)
}
