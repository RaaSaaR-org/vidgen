use crate::config;
use crate::error::VidgenResult;
use crate::scene;
use crate::tts;
use colored::*;
use std::path::Path;

pub async fn run(project_path: &Path) -> VidgenResult<()> {
    let config = config::load_config(project_path)?;
    config.validate()?;

    let scenes = scene::load_scenes(project_path)?;

    // Load .env from project directory (if present) so TTS API keys are available
    let _ = dotenvy::from_path(project_path.join(".env"));

    // Create TTS engine (optional — if it fails, we show "?" for auto durations)
    let tts_engine = tts::create_engine(&config.voice).ok();

    // Synthesize TTS for each scene to get accurate durations
    let temp_dir = tempfile::tempdir()?;
    let mut tts_durations: Vec<Option<f64>> = Vec::new();

    for (i, s) in scenes.iter().enumerate() {
        let script = s.script.trim();
        if script.is_empty() || tts_engine.is_none() {
            tts_durations.push(None);
            continue;
        }

        let wav_path = temp_dir.path().join(format!("scene-{i:03}.wav"));

        // Determine per-scene voice/speed overrides (same logic as render/mod.rs)
        let scene_voice_cfg = s.frontmatter.voice.as_ref();
        let scene_engine_override = scene_voice_cfg.and_then(|v| v.engine.as_deref());
        let voice = scene_voice_cfg
            .and_then(|v| v.voice_name())
            .or(config.voice.default_voice.as_deref());
        let speed = scene_voice_cfg
            .and_then(|v| v.speed)
            .unwrap_or(config.voice.speed);

        // Use a per-scene engine if the scene overrides the engine
        let scene_engine: Option<Box<dyn tts::TtsEngine>> =
            if let Some(engine_name) = scene_engine_override {
                let mut voice_cfg = config.voice.clone();
                voice_cfg.engine = engine_name.to_string();
                tts::create_engine(&voice_cfg).ok()
            } else {
                None
            };
        let effective_engine: &dyn tts::TtsEngine = scene_engine
            .as_deref()
            .unwrap_or_else(|| tts_engine.as_ref().unwrap().as_ref());

        match tts::cache::synthesize_cached_with_options(
            effective_engine,
            script,
            voice,
            speed,
            &wav_path,
            project_path,
            false,
        ) {
            Ok(result) => tts_durations.push(Some(result.duration_secs)),
            Err(_) => tts_durations.push(None),
        }
    }

    // Print project header
    println!("\n{} {}", "Project:".bold(), config.project.name);
    println!(
        "{} {}x{} @ {}fps",
        "Format: ".bold(),
        config.video.width,
        config.video.height,
        config.video.fps
    );
    println!(
        "{} {} (speed: {})",
        "Voice:  ".bold(),
        config.voice.engine,
        config.voice.speed
    );

    if let Some(ref bg) = config.audio.background {
        println!("{} {} ({}dB)", "Music:  ".bold(), bg.file, bg.volume);
    }

    // Print scene list
    println!("\n{} ({}):", "Scenes".bold(), scenes.len());

    let mut total_duration: f64 = 0.0;
    let mut all_resolved = true;

    for (i, s) in scenes.iter().enumerate() {
        let tts_dur = tts_durations[i];
        let duration = s.frontmatter.duration.resolve(
            tts_dur,
            config.voice.padding_before,
            config.voice.padding_after,
            config.voice.auto_fallback_duration,
        );

        // Determine if we could resolve the duration
        let dur_str = if s.frontmatter.duration.is_auto()
            && tts_dur.is_none()
            && s.script.trim().is_empty()
        {
            // Auto duration with no script — use fallback, mark with ?
            all_resolved = false;
            format!("{:.1}s?", duration)
        } else if s.frontmatter.duration.is_auto() && tts_dur.is_none() {
            // Auto duration with script but TTS failed
            all_resolved = false;
            "?".to_string()
        } else {
            format!("{:.1}s", duration)
        };

        total_duration += duration;

        let scene_name = s
            .source_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("scene-{}", i + 1));

        let template_info = if s.is_video_clip() {
            format!(
                "(clip: {})",
                s.frontmatter.video_source.as_deref().unwrap_or("?")
            )
        } else if s.frontmatter.sub_scenes.is_some() {
            "(sequence)".to_string()
        } else if !s.frontmatter.template.is_empty() {
            format!("(template: {})", s.frontmatter.template)
        } else {
            String::new()
        };

        println!(
            "  {:02} {:<16} {:>6}  {}",
            i + 1,
            scene_name,
            dur_str,
            template_info.dimmed()
        );
    }

    // Print total
    let total_mins = (total_duration / 60.0).floor() as u32;
    let total_remaining_secs = total_duration - (total_mins as f64 * 60.0);
    println!("  {}", "─".repeat(40));
    let qualifier = if all_resolved { "" } else { " (estimated)" };
    if total_mins > 0 {
        println!(
            "  Total: {:.1}s ({}m {:.0}s){}",
            total_duration, total_mins, total_remaining_secs, qualifier
        );
    } else {
        println!("  Total: {:.1}s{}", total_duration, qualifier);
    }
    println!();

    Ok(())
}
