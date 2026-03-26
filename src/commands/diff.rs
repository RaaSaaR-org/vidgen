use crate::config;
use crate::error::VidgenResult;
use crate::scene;
use crate::tts;
use colored::*;
use std::path::Path;

/// Run the diff command: compare current scene text with cached TTS audio.
pub async fn run(project_path: &Path) -> VidgenResult<()> {
    let cfg = config::load_config(project_path)?;
    let scenes = scene::load_scenes(project_path)?;

    let cache_dir = project_path.join("assets/voiceover");

    let project_slug = cfg
        .project
        .name
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric(), "-")
        .trim_matches('-')
        .to_string();

    eprintln!(
        "{}",
        format!("Diff for \"{}\":", project_slug).cyan().bold()
    );

    let mut changed_count = 0;
    let mut unchanged_count = 0;

    for (i, s) in scenes.iter().enumerate() {
        let scene_name = s
            .source_path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        let script = s.script.trim();
        if script.is_empty() {
            eprintln!(
                "  {:02} {:<20} {} no voiceover",
                i + 1,
                scene_name,
                "-".dimmed()
            );
            continue;
        }

        // Determine effective engine/voice/speed for this scene
        let scene_voice_cfg = s.frontmatter.voice.as_ref();
        let engine_name = scene_voice_cfg
            .and_then(|v| v.engine.as_deref())
            .unwrap_or(&cfg.voice.engine);
        let voice = scene_voice_cfg
            .and_then(|v| v.voice_name())
            .or(cfg.voice.default_voice.as_deref());
        let speed = scene_voice_cfg
            .and_then(|v| v.speed)
            .unwrap_or(cfg.voice.speed);

        // Compute the cache key (same logic as tts/cache.rs)
        let hash = tts::cache::cache_key(
            engine_name,
            voice,
            speed,
            script,
        );

        let cached_wav = cache_dir.join(format!("{hash}.wav"));
        let cached_json = cache_dir.join(format!("{hash}.json"));

        if cached_wav.exists() && cached_json.exists() {
            // Scene is unchanged — read duration from sidecar
            let duration = tts::cache::read_sidecar(&cached_json).unwrap_or(0.0);
            eprintln!(
                "  {:02} {:<20} {} unchanged ({:.1}s)",
                i + 1,
                scene_name,
                "\u{2713}".green(),
                duration,
            );
            unchanged_count += 1;
        } else {
            // Scene text has changed since last render
            // Try to find any existing cached file to show previous duration
            let was_str = if cached_json.exists() {
                if let Some(dur) = tts::cache::read_sidecar(&cached_json) {
                    format!("was {:.1}s, ", dur)
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            eprintln!(
                "  {:02} {:<20} {} CHANGED ({}re-render needed)",
                i + 1,
                scene_name,
                "\u{2717}".red(),
                was_str,
            );
            changed_count += 1;
        }
    }

    eprintln!();
    if changed_count == 0 {
        eprintln!(
            "{} All {} scenes unchanged — no re-render needed",
            "diff:".green().bold(),
            unchanged_count,
        );
    } else {
        eprintln!(
            "{} {} changed, {} unchanged — render to update",
            "diff:".yellow().bold(),
            changed_count,
            unchanged_count,
        );
    }

    Ok(())
}
