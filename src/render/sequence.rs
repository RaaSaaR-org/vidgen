use crate::config::{PlatformPreset, ThemeConfig};
use crate::error::{VidgenError, VidgenResult};
use crate::render::{browser, encoder};
use crate::scene::{Scene, SceneDuration, SceneFrontmatter};
use crate::template::TemplateRegistry;
use chromiumoxide::browser::Browser;
use std::path::{Path, PathBuf};
use tracing::debug;

/// Render a sequence scene: multiple visual sub-scenes with a single voiceover.
///
/// 1. Renders each sub-scene independently (HTML via Chromium, clips via FFmpeg)
/// 2. Concatenates sub-scene videos with hard cuts
/// 3. Mixes TTS voiceover + background music onto the result
#[allow(clippy::too_many_arguments)]
pub async fn render_sequence_scene(
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
    sub_durations: &[f64],
    audio_delay_secs: f64,
    project_path: &Path,
) -> VidgenResult<PathBuf> {
    let sub_scenes = scene.frontmatter.sub_scenes.as_ref().unwrap();
    let temp_dir = tempfile::tempdir()
        .map_err(|e| VidgenError::Other(format!("Failed to create temp dir: {e}")))?;

    eprintln!(
        "  Scene {}: sequence ({} sub-scenes, {:.1}s total)",
        scene_index + 1,
        sub_scenes.len(),
        sub_durations.iter().sum::<f64>(),
    );

    // Step 1: Render each sub-scene independently
    let mut sub_files: Vec<PathBuf> = Vec::new();

    for (j, (sub, &dur)) in sub_scenes.iter().zip(sub_durations.iter()).enumerate() {
        let sub_output = temp_dir.path().join(format!("sub-{j:03}.mp4"));

        if sub.is_video_clip() {
            // Video-clip sub-scene: re-encode with optional source audio
            let video_src = sub.video_source.as_ref().unwrap();
            let resolved = crate::scene::resolve_asset_path(video_src, project_path);
            let source_vol = sub.source_volume.unwrap_or(0.0);

            debug!(
                "Sequence sub-scene {}: video-clip ({:.1}s) from {}",
                j, dur, resolved.display()
            );
            eprintln!(
                "    Sub {}: video-clip ({:.1}s){}",
                j + 1,
                dur,
                if source_vol > 0.0 {
                    format!(" [source audio {:.0}%]", source_vol * 100.0)
                } else {
                    String::new()
                },
            );

            encoder::prepare_video_clip(
                &resolved,
                &sub_output,
                width,
                height,
                fps,
                Some(dur),
                platform,
                None,  // no voice on individual sub-scenes
                None,  // no music on individual sub-scenes
                0.0,
                0.0,
                source_vol,
            )?;
        } else {
            // HTML template sub-scene: render via Chromium
            let template = sub.template.as_deref().unwrap_or("content-text");

            debug!(
                "Sequence sub-scene {}: template '{}' ({:.1}s)",
                j, template, dur
            );
            eprintln!("    Sub {}: {} ({:.1}s)", j + 1, template, dur);

            // Create a temporary Scene struct for the sub-scene
            let tmp_scene = Scene {
                frontmatter: SceneFrontmatter {
                    template: template.to_string(),
                    duration: SceneDuration::Fixed(dur),
                    video_source: None,
                    source_volume: None,
                    sub_scenes: None,
                    props: sub.props.clone(),
                    background: sub.background.clone(),
                    transition_in: None,
                    transition_out: None,
                    transition_duration: None,
                    voice: None,
                    audio: None,
                    format_overrides: None,
                },
                script: String::new(), // no per-sub-scene voiceover
                source_path: scene.source_path.clone(),
            };

            browser::capture_scene_frames(
                browser,
                &tmp_scene,
                scene_index * 100 + j, // unique index for progress display
                registry,
                theme,
                width,
                height,
                fps,
                platform,
                &sub_output,
                None,  // no audio per sub-scene
                None,  // no music per sub-scene
                0.0,
                dur,
                0.0,
                0.0,
                Some(project_path),
            )
            .await?;
        }

        sub_files.push(sub_output);
    }

    // Step 2: Concatenate sub-scene videos with hard cuts (no transitions)
    let concat_path = temp_dir.path().join("sequence-concat.mp4");
    encoder::concat_scenes(&sub_files, &concat_path)?;

    // Step 3: Mix voiceover + music onto the concatenated video
    // Copy concat to final output, then mix audio in-place
    std::fs::copy(&concat_path, output_path)?;

    if audio_path.is_some() || music_path.is_some() {
        encoder::mix_audio_onto_video(
            output_path,
            audio_path,
            music_path,
            music_volume,
            audio_delay_secs,
            platform,
        )?;
    }

    Ok(output_path.to_path_buf())
}
