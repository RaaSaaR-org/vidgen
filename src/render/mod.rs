pub mod browser;
pub mod encoder;
pub mod frame_cache;

use crate::config::{resolve_encoding, ProjectConfig, QualityPreset};
use crate::error::VidgenResult;
use crate::render::encoder::{resolve_transition, SceneTransition};
use crate::scene::{Scene, SceneFrontmatter};
use crate::subtitle;
use crate::template::TemplateRegistry;
use crate::tts;
use colored::*;
use futures::stream::{self, StreamExt};
use rmcp::model::ProgressNotificationParam;
use rmcp::{Peer, RoleServer};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Render progress reporter. Sends MCP progress notifications when running
/// via the MCP server, or does nothing (noop) when running from the CLI.
pub struct RenderProgress {
    peer: Option<Peer<RoleServer>>,
    token: Option<rmcp::model::ProgressToken>,
}

impl RenderProgress {
    /// No-op progress reporter for CLI usage.
    pub fn noop() -> Self {
        Self {
            peer: None,
            token: None,
        }
    }

    /// Create a progress reporter that sends MCP notifications.
    pub fn new(peer: Peer<RoleServer>, token: rmcp::model::ProgressToken) -> Self {
        Self {
            peer: Some(peer),
            token: Some(token),
        }
    }

    /// Report progress. No-op if no peer/token is set.
    pub async fn report(&self, progress: f64, total: f64, message: &str) {
        if let (Some(peer), Some(token)) = (&self.peer, &self.token) {
            let _ = peer
                .notify_progress(ProgressNotificationParam {
                    progress_token: token.clone(),
                    progress,
                    total: Some(total),
                    message: Some(message.to_string()),
                })
                .await;
        }
    }
}

/// Output from rendering a single format.
#[derive(Debug, Clone, Serialize)]
pub struct FormatOutput {
    pub format_name: String,
    pub output_path: PathBuf,
    pub effective_durations: Vec<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle_path: Option<PathBuf>,
}

/// Apply format-specific overrides to a scene's frontmatter, returning a modified clone.
/// Merges `format_overrides[fmt_name].props` into the scene's props and replaces the
/// background if specified.
fn apply_format_overrides(scene: &Scene, fmt_name: &str) -> Scene {
    let overrides = scene
        .frontmatter
        .format_overrides
        .as_ref()
        .and_then(|fo| fo.get(fmt_name));

    match overrides {
        None => Scene {
            frontmatter: SceneFrontmatter {
                template: scene.frontmatter.template.clone(),
                duration: scene.frontmatter.duration.clone(),
                props: scene.frontmatter.props.clone(),
                background: scene.frontmatter.background.as_ref().map(|bg| {
                    crate::scene::BackgroundConfig {
                        color: bg.color.clone(),
                        image: bg.image.clone(),
                    }
                }),
                transition_in: scene.frontmatter.transition_in.clone(),
                transition_out: scene.frontmatter.transition_out.clone(),
                transition_duration: scene.frontmatter.transition_duration,
                voice: scene.frontmatter.voice.clone(),
                audio: scene.frontmatter.audio.clone(),
                format_overrides: scene.frontmatter.format_overrides.clone(),
            },
            script: scene.script.clone(),
            source_path: scene.source_path.clone(),
        },
        Some(fo) => {
            let mut props = scene.frontmatter.props.clone();
            if let Some(ref override_props) = fo.props {
                for (k, v) in override_props {
                    props.insert(k.clone(), v.clone());
                }
            }
            let background = fo
                .background
                .as_ref()
                .map(|bg| crate::scene::BackgroundConfig {
                    color: bg.color.clone(),
                    image: bg.image.clone(),
                })
                .or_else(|| {
                    scene.frontmatter.background.as_ref().map(|bg| {
                        crate::scene::BackgroundConfig {
                            color: bg.color.clone(),
                            image: bg.image.clone(),
                        }
                    })
                });

            Scene {
                frontmatter: SceneFrontmatter {
                    template: scene.frontmatter.template.clone(),
                    duration: scene.frontmatter.duration.clone(),
                    props,
                    background,
                    transition_in: scene.frontmatter.transition_in.clone(),
                    transition_out: scene.frontmatter.transition_out.clone(),
                    transition_duration: scene.frontmatter.transition_duration,
                    voice: scene.frontmatter.voice.clone(),
                    audio: scene.frontmatter.audio.clone(),
                    format_overrides: scene.frontmatter.format_overrides.clone(),
                },
                script: scene.script.clone(),
                source_path: scene.source_path.clone(),
            }
        }
    }
}

/// Resolve format list from config. Returns `(name, width, height, platform)` tuples.
fn resolve_formats(
    config: &ProjectConfig,
    format_filter: Option<&[String]>,
) -> Vec<(String, u32, u32, Option<String>)> {
    match &config.video.formats {
        Some(formats) => formats
            .iter()
            .filter(|(name, _)| {
                format_filter
                    .map(|f| f.iter().any(|n| n == *name))
                    .unwrap_or(true)
            })
            .map(|(name, fc)| (name.clone(), fc.width, fc.height, fc.platform.clone()))
            .collect(),
        None => vec![(
            "default".into(),
            config.video.width,
            config.video.height,
            None,
        )],
    }
}

/// Render a complete project: all scenes → per-scene MP4 → concatenated output.
/// Supports multi-format: renders once per format (different viewport/encoding).
#[allow(clippy::too_many_arguments)]
pub async fn render_project(
    config: &ProjectConfig,
    scenes: &[Scene],
    fps: u32,
    quality_name: &str,
    output_dir: &Path,
    project_path: &Path,
    progress: RenderProgress,
    format_filter: Option<&[String]>,
) -> VidgenResult<Vec<FormatOutput>> {
    let quality = QualityPreset::from_name(quality_name);
    let mut registry = TemplateRegistry::new()?;
    registry.register_project_templates(project_path)?;

    let formats = resolve_formats(config, format_filter);

    eprintln!(
        "{} Rendering \"{}\" — {} scene(s), {} format(s), @ {}fps, quality={}",
        "render:".cyan().bold(),
        config.project.name,
        scenes.len(),
        formats.len(),
        fps,
        quality_name,
    );

    // Create output directory
    std::fs::create_dir_all(output_dir)?;

    // Create a temp directory for intermediate scene files
    let temp_dir = tempfile::tempdir()?;

    // Load .env from project directory (if present) so keys like ELEVEN_API_KEY are available
    let _ = dotenvy::from_path(project_path.join(".env"));

    // TTS synthesis pass — runs once (format-independent)
    let tts_engine = match tts::create_engine(&config.voice) {
        Ok(engine) => {
            eprintln!(
                "{} TTS engine: {}",
                "render:".cyan().bold(),
                engine.engine_name()
            );
            Some(engine)
        }
        Err(e) => {
            eprintln!(
                "{} TTS unavailable ({}), skipping voiceover",
                "render:".cyan().bold(),
                e
            );
            None
        }
    };

    let mut audio_paths: Vec<Option<PathBuf>> = Vec::new();
    let mut tts_durations: Vec<Option<f64>> = Vec::new();
    for (i, scene) in scenes.iter().enumerate() {
        let script = scene.script.trim();
        if script.is_empty() || tts_engine.is_none() {
            audio_paths.push(None);
            tts_durations.push(None);
            continue;
        }
        let engine = tts_engine.as_ref().unwrap();
        let wav_path = temp_dir.path().join(format!("scene-{i:03}.wav"));
        let voice = scene
            .frontmatter
            .voice
            .as_deref()
            .or(config.voice.default_voice.as_deref());
        match tts::cache::synthesize_cached(
            engine.as_ref(),
            script,
            voice,
            config.voice.speed,
            &wav_path,
            project_path,
        ) {
            Ok(result) => {
                let tag = if result.cached { " (cached)" } else { "" };
                eprintln!(
                    "  TTS scene {}: {:.1}s audio{}",
                    i + 1,
                    result.duration_secs,
                    tag
                );
                tts_durations.push(Some(result.duration_secs));
                audio_paths.push(Some(result.audio_path));
            }
            Err(e) => {
                eprintln!("  TTS scene {}: failed ({}), skipping audio", i + 1, e);
                audio_paths.push(None);
                tts_durations.push(None);
            }
        }
    }

    // Duration resolution pass — runs once (format-independent)
    let effective_durations: Vec<f64> = scenes
        .iter()
        .enumerate()
        .map(|(i, scene)| {
            scene.frontmatter.duration.resolve(
                tts_durations[i],
                config.voice.padding_before,
                config.voice.padding_after,
                config.voice.auto_fallback_duration,
            )
        })
        .collect();

    for (i, (scene, &dur)) in scenes.iter().zip(effective_durations.iter()).enumerate() {
        if scene.frontmatter.duration.is_auto() {
            let source = if tts_durations[i].is_some() {
                "TTS + padding"
            } else {
                "fallback"
            };
            eprintln!(
                "  Scene {}: duration auto → {:.1}s ({})",
                i + 1,
                dur,
                source
            );
        }
    }

    // Resolve transitions between adjacent scenes (format-independent)
    let transitions: Vec<Option<SceneTransition>> = if scenes.len() > 1 {
        (0..scenes.len() - 1)
            .map(|i| resolve_transition(&scenes[i], &scenes[i + 1], &config.video))
            .collect()
    } else {
        vec![]
    };
    let has_transitions = transitions.iter().any(|t| t.is_some());

    // Determine project slug for output filenames
    let project_slug = config
        .project
        .name
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric(), "-")
        .trim_matches('-')
        .to_string();

    // Progress tracking across all formats
    let total_formats = formats.len();
    let steps_per_format = scenes.len() * 2 + 1; // capture per scene + concat
    let total_steps = (scenes.len() + total_formats * steps_per_format) as f64; // TTS + per-format work
    progress
        .report(scenes.len() as f64, total_steps, "TTS synthesis complete")
        .await;

    let mut results: Vec<FormatOutput> = Vec::new();

    // Per-format render loop
    for (fmt_idx, (fmt_name, width, height, platform_name)) in formats.iter().enumerate() {
        let platform = resolve_encoding(&quality, platform_name.as_deref());

        eprintln!(
            "{} Format \"{}\": {}x{}{}",
            "render:".cyan().bold(),
            fmt_name,
            width,
            height,
            platform_name
                .as_ref()
                .map(|p| format!(" (platform: {p})"))
                .unwrap_or_default(),
        );

        // Launch browser for this format's dimensions
        eprintln!("{} Launching browser...", "render:".cyan().bold());
        let (browser, handler_handle) = browser::launch_browser(*width, *height).await?;

        // Render each scene for this format
        let fmt_temp_dir = temp_dir.path().join(fmt_name);
        std::fs::create_dir_all(&fmt_temp_dir)?;

        // Apply per-format overrides to scenes
        let fmt_scenes: Vec<Scene> = scenes
            .iter()
            .map(|s| apply_format_overrides(s, fmt_name))
            .collect();

        // Pre-compute per-scene data (output paths, audio paths, music paths)
        let scene_prep: Vec<_> = fmt_scenes
            .iter()
            .enumerate()
            .map(|(i, scene)| {
                let scene_output = fmt_temp_dir.join(format!("scene-{i:03}.mp4"));
                let audio = audio_paths[i].clone();
                let music = scene
                    .frontmatter
                    .audio
                    .as_ref()
                    .and_then(|a| a.music.as_deref())
                    .map(|m| crate::scene::resolve_asset_path(m, project_path));
                let music_volume = scene
                    .frontmatter
                    .audio
                    .as_ref()
                    .and_then(|a| a.music_volume)
                    .unwrap_or(0.3);
                (scene_output, audio, music, music_volume)
            })
            .collect();

        let max_parallel = config.video.parallel_scenes.unwrap_or(4);
        if max_parallel > 1 && scenes.len() > 1 {
            eprintln!(
                "{} Parallel scene rendering (max {} concurrent)",
                "render:".cyan().bold(),
                max_parallel,
            );
        }

        // Create references to shared data (references are Copy, safe for async move)
        let browser_ref = &browser;
        let registry_ref = &registry;
        let theme_ref = &config.theme;
        let platform_ref = &platform;
        let durations_ref = &effective_durations;
        let prep_ref = &scene_prep;
        let scenes_ref = &fmt_scenes;

        // Render scenes concurrently with bounded parallelism
        let scene_results: Vec<_> = stream::iter(0..scenes.len())
            .map(|i| async move {
                let scene = &scenes_ref[i];
                let scene_output = &prep_ref[i].0;
                let audio = &prep_ref[i].1;
                let music = &prep_ref[i].2;
                let music_volume = prep_ref[i].3;
                let dur = durations_ref[i];
                let path = browser::capture_scene_frames(
                    browser_ref,
                    scene,
                    i,
                    registry_ref,
                    theme_ref,
                    *width,
                    *height,
                    fps,
                    platform_ref,
                    scene_output,
                    audio.as_deref(),
                    music.as_deref(),
                    music_volume,
                    dur,
                )
                .await?;
                Ok::<_, crate::error::VidgenError>((i, path, dur))
            })
            .buffer_unordered(max_parallel)
            .collect()
            .await;

        // Collect results in scene order
        let mut scene_files: Vec<PathBuf> = vec![PathBuf::new(); scenes.len()];
        let mut scene_durs: Vec<f64> = vec![0.0; scenes.len()];
        for result in scene_results {
            let (i, path, dur) = result?;
            scene_files[i] = path;
            scene_durs[i] = dur;

            // Progress: scene captured
            let done = scenes.len() as f64 + (fmt_idx * steps_per_format + i + 1) as f64;
            progress
                .report(
                    done,
                    total_steps,
                    &format!("Scene {} captured ({})", i + 1, fmt_name),
                )
                .await;
        }

        // Close browser for this format
        drop(browser);
        handler_handle.abort();

        // Output filename: slug-format.mp4 (or just slug.mp4 if single format)
        let output_path = if total_formats == 1 && *fmt_name == "default" {
            output_dir.join(format!("{project_slug}.mp4"))
        } else {
            output_dir.join(format!("{project_slug}-{fmt_name}.mp4"))
        };

        // Concatenate scenes
        if scene_files.len() > 1 {
            if has_transitions {
                eprintln!(
                    "{} Concatenating {} scenes with transitions...",
                    "render:".cyan().bold(),
                    scene_files.len()
                );
            } else {
                eprintln!(
                    "{} Concatenating {} scenes...",
                    "render:".cyan().bold(),
                    scene_files.len()
                );
            }
        }
        encoder::concat_scenes_with_transitions(
            &scene_files,
            &scene_durs,
            &transitions,
            &output_path,
            &platform,
        )?;

        eprintln!(
            "{} Output: {}",
            "done:".green().bold(),
            output_path.display()
        );

        // Generate subtitles if enabled
        let subtitle_path = if config.output.subtitles.enabled {
            let mut all_words = Vec::new();
            let mut scene_offset = 0.0_f64;

            for (i, scene) in scenes.iter().enumerate() {
                let script = scene.script.trim();
                if !script.is_empty() && tts_durations[i].is_some() {
                    let words =
                        tts::timestamps::estimate_word_timestamps(script, effective_durations[i]);
                    for mut w in words {
                        w.start_secs += scene_offset;
                        w.end_secs += scene_offset;
                        all_words.push(w);
                    }
                }
                scene_offset += effective_durations[i];
            }

            if !all_words.is_empty() {
                let entries = subtitle::group_into_subtitles(
                    &all_words,
                    config.output.subtitles.max_words_per_line,
                );
                let srt_content = subtitle::to_srt(&entries);
                let srt_path = output_path.with_extension("srt");
                std::fs::write(&srt_path, &srt_content)?;
                eprintln!(
                    "{} Subtitles: {}",
                    "done:".green().bold(),
                    srt_path.display()
                );
                Some(srt_path)
            } else {
                None
            }
        } else {
            None
        };

        // Burn subtitles into video if requested
        if config.output.subtitles.burn_in {
            if let Some(ref srt_path) = subtitle_path {
                eprintln!(
                    "{} Burning subtitles into video...",
                    "render:".cyan().bold()
                );
                encoder::burn_in_subtitles(&output_path, srt_path)?;
                eprintln!(
                    "{} Subtitles burned in: {}",
                    "done:".green().bold(),
                    output_path.display()
                );
            }
        }

        results.push(FormatOutput {
            format_name: fmt_name.clone(),
            output_path,
            effective_durations: effective_durations.clone(),
            subtitle_path,
        });

        // Progress: format complete
        let done = scenes.len() as f64 + ((fmt_idx + 1) * steps_per_format) as f64;
        progress
            .report(
                done,
                total_steps,
                &format!("Format \"{}\" complete", fmt_name),
            )
            .await;
    }

    // Progress: all formats complete
    progress
        .report(total_steps, total_steps, "Render complete")
        .await;

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_render_progress_noop() {
        let progress = RenderProgress::noop();
        // Should not panic — no peer/token, so report is a no-op
        progress.report(1.0, 10.0, "test step").await;
        progress.report(10.0, 10.0, "done").await;
    }

    #[test]
    fn test_resolve_formats_with_formats() {
        use crate::config::*;
        use std::collections::BTreeMap;
        let mut formats = BTreeMap::new();
        formats.insert(
            "landscape".into(),
            FormatConfig {
                width: 1920,
                height: 1080,
                label: Some("YouTube".into()),
                platform: None,
            },
        );
        formats.insert(
            "portrait".into(),
            FormatConfig {
                width: 1080,
                height: 1920,
                label: Some("Reels".into()),
                platform: Some("instagram-reels".into()),
            },
        );
        let config = ProjectConfig {
            project: ProjectInfo {
                name: "Test".into(),
                version: "1.0.0".into(),
            },
            video: VideoConfig {
                formats: Some(formats),
                ..Default::default()
            },
            voice: VoiceConfig::default(),
            theme: ThemeConfig::default(),
            output: OutputConfig::default(),
        };
        let result = resolve_formats(&config, None);
        assert_eq!(result.len(), 2);
        // BTreeMap → alphabetical: landscape, portrait
        assert_eq!(result[0].0, "landscape");
        assert_eq!(result[0].1, 1920);
        assert_eq!(result[0].2, 1080);
        assert!(result[0].3.is_none());
        assert_eq!(result[1].0, "portrait");
        assert_eq!(result[1].1, 1080);
        assert_eq!(result[1].2, 1920);
        assert_eq!(result[1].3.as_deref(), Some("instagram-reels"));
    }

    #[test]
    fn test_resolve_formats_without_formats() {
        use crate::config::*;
        let config = ProjectConfig {
            project: ProjectInfo {
                name: "Test".into(),
                version: "1.0.0".into(),
            },
            video: VideoConfig::default(),
            voice: VoiceConfig::default(),
            theme: ThemeConfig::default(),
            output: OutputConfig::default(),
        };
        let result = resolve_formats(&config, None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "default");
        assert_eq!(result[0].1, 1920);
        assert_eq!(result[0].2, 1080);
    }

    #[test]
    fn test_resolve_formats_with_filter() {
        use crate::config::*;
        use std::collections::BTreeMap;
        let mut formats = BTreeMap::new();
        formats.insert(
            "landscape".into(),
            FormatConfig {
                width: 1920,
                height: 1080,
                label: None,
                platform: None,
            },
        );
        formats.insert(
            "portrait".into(),
            FormatConfig {
                width: 1080,
                height: 1920,
                label: None,
                platform: None,
            },
        );
        formats.insert(
            "square".into(),
            FormatConfig {
                width: 1080,
                height: 1080,
                label: None,
                platform: None,
            },
        );
        let config = ProjectConfig {
            project: ProjectInfo {
                name: "Test".into(),
                version: "1.0.0".into(),
            },
            video: VideoConfig {
                formats: Some(formats),
                ..Default::default()
            },
            voice: VoiceConfig::default(),
            theme: ThemeConfig::default(),
            output: OutputConfig::default(),
        };
        let filter = vec!["portrait".into(), "square".into()];
        let result = resolve_formats(&config, Some(&filter));
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "portrait");
        assert_eq!(result[1].0, "square");
    }
}
