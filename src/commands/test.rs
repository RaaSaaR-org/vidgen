use crate::config;
use crate::error::VidgenResult;
use crate::render;
use crate::scene;
use crate::template::TemplateRegistry;
use colored::*;
use std::path::Path;

/// Progress points at which to capture snapshots for each scene.
const PROGRESS_POINTS: [f32; 3] = [0.2, 0.5, 0.8];

/// Maximum allowed pixel difference percentage before a test is considered failing.
const DIFF_THRESHOLD: f64 = 1.0;

/// Compare two PNG byte slices and return the percentage of differing bytes.
fn png_diff_percent(a: &[u8], b: &[u8]) -> f64 {
    if a.len() != b.len() {
        return 100.0;
    }
    let different = a.iter().zip(b.iter()).filter(|(x, y)| x != y).count();
    (different as f64 / a.len() as f64) * 100.0
}

pub async fn run(project_path: &Path, update: bool) -> VidgenResult<()> {
    let cfg = config::load_config(project_path)?;
    let scenes = scene::load_scenes(project_path)?;
    let mut registry = TemplateRegistry::new()?;
    registry.register_project_templates(project_path)?;

    let snapshot_dir = project_path.join(".vidgen").join("snapshots");
    let snapshots_exist = snapshot_dir.exists();
    let should_update = update || !snapshots_exist;

    let project_name = project_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    let total_snapshots = scenes.len() * PROGRESS_POINTS.len();
    eprintln!(
        "Testing \"{}\" ({} scenes, {} snapshots)...",
        project_name,
        scenes.len(),
        total_snapshots
    );

    if should_update {
        std::fs::create_dir_all(&snapshot_dir)?;
    }

    let width = cfg.video.width;
    let height = cfg.video.height;
    let fps = cfg.video.fps;

    let mut total_pass = 0usize;
    let mut total_fail = 0usize;

    for (i, scene_obj) in scenes.iter().enumerate() {
        let scene_name = scene_obj
            .source_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        // Strip leading number prefix for display (e.g., "01-hook" -> "hook")
        let display_name = scene_name
            .split_once('-')
            .map(|(_, rest)| rest)
            .unwrap_or(scene_name);

        let total_frames = scene_obj.total_frames(fps);

        let mut frames_match = 0usize;
        let frames_total = PROGRESS_POINTS.len();
        let mut worst_diff = 0.0f64;

        for &progress in PROGRESS_POINTS.iter() {
            let frame =
                ((progress * total_frames as f32) as u32).min(total_frames.saturating_sub(1));

            let html = registry.render_scene_html(
                scene_obj,
                &cfg.theme,
                width,
                height,
                frame,
                total_frames,
                Some(project_path),
            )?;

            let png_data =
                render::browser::capture_single_frame(&html, width, height, frame, total_frames)
                    .await?;

            let snapshot_file = snapshot_dir.join(format!(
                "{:02}-{}-{}.png",
                i,
                scene_name,
                (progress * 100.0) as u32
            ));

            if should_update {
                std::fs::write(&snapshot_file, &png_data)?;
                frames_match += 1;
            } else {
                // Compare against reference
                match std::fs::read(&snapshot_file) {
                    Ok(reference) => {
                        let diff = png_diff_percent(&reference, &png_data);
                        if diff <= DIFF_THRESHOLD {
                            frames_match += 1;
                        } else if diff > worst_diff {
                            worst_diff = diff;
                        }
                    }
                    Err(_) => {
                        // Reference file missing — count as fail
                        worst_diff = 100.0;
                    }
                }
            }
        }

        if should_update {
            eprintln!(
                "  {:02} {:<16} {} saved ({}/{} frames)",
                i + 1,
                display_name,
                "\u{2713}".green(),
                frames_match,
                frames_total
            );
            total_pass += 1;
        } else if frames_match == frames_total {
            eprintln!(
                "  {:02} {:<16} {} pass ({}/{} frames match)",
                i + 1,
                display_name,
                "\u{2713}".green(),
                frames_match,
                frames_total
            );
            total_pass += 1;
        } else {
            eprintln!(
                "  {:02} {:<16} {} FAIL (frame differs by {:.0}%)",
                i + 1,
                display_name,
                "\u{2717}".red(),
                worst_diff
            );
            total_fail += 1;
        }
    }

    eprintln!();
    if should_update {
        eprintln!(
            "  {}: {} reference snapshots saved",
            "Result".green().bold(),
            total_snapshots
        );
    } else if total_fail == 0 {
        eprintln!("  {}: {} pass, 0 fail", "Result".green().bold(), total_pass);
    } else {
        eprintln!(
            "  {}: {} pass, {} fail",
            "Result".cyan().bold(),
            total_pass,
            format!("{total_fail}").red().bold()
        );
        eprintln!();
        eprintln!("  Run with --update to accept current output as new reference.");
    }

    Ok(())
}
