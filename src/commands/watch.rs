use crate::error::{VidgenError, VidgenResult};
use colored::*;
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

/// Relevant file extensions for triggering rebuilds.
const WATCH_EXTENSIONS: &[&str] = &["md", "html", "css", "toml"];

/// Run the watch command: monitor project files and auto-preview or re-render on change.
pub async fn run(
    project_path: &Path,
    full_render: bool,
    fixed_scene: Option<usize>,
) -> VidgenResult<()> {
    let project_path = project_path
        .canonicalize()
        .map_err(|_| VidgenError::ProjectNotFound(project_path.to_path_buf()))?;

    // Verify the project exists
    if !project_path.join("project.toml").exists() {
        return Err(VidgenError::ConfigNotFound(
            project_path.join("project.toml"),
        ));
    }

    let mode = if full_render { "render" } else { "preview" };
    eprintln!(
        "{} Watching {} (mode: {mode})... press Ctrl+C to stop",
        "watch:".cyan().bold(),
        project_path.display(),
    );

    let (tx, rx) = mpsc::channel();

    let mut debouncer = new_debouncer(Duration::from_millis(500), tx)
        .map_err(|e| VidgenError::Other(format!("Failed to create file watcher: {e}")))?;

    debouncer
        .watcher()
        .watch(project_path.as_ref(), notify::RecursiveMode::Recursive)
        .map_err(|e| VidgenError::Other(format!("Failed to watch directory: {e}")))?;

    // Keep watcher alive
    let _debouncer = debouncer;

    loop {
        match rx.recv() {
            Ok(Ok(events)) => {
                // Filter to relevant file changes
                let relevant: Vec<_> = events
                    .iter()
                    .filter(|e| e.kind == DebouncedEventKind::Any)
                    .filter(|e| {
                        let path = &e.path;
                        // Skip output directory
                        if path.starts_with(project_path.join("output")) {
                            return false;
                        }
                        // Skip hidden files and temp files
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            if name.starts_with('.') || name.starts_with("__tmp_") {
                                return false;
                            }
                        }
                        // Check extension
                        path.extension()
                            .and_then(|ext| ext.to_str())
                            .is_some_and(|ext| WATCH_EXTENSIONS.contains(&ext))
                    })
                    .collect();

                if relevant.is_empty() {
                    continue;
                }

                let changed_files: Vec<String> = relevant
                    .iter()
                    .filter_map(|e| {
                        e.path
                            .strip_prefix(&project_path)
                            .ok()
                            .map(|p| p.display().to_string())
                    })
                    .collect();

                eprintln!(
                    "\n{} Change detected: {}",
                    "watch:".cyan().bold(),
                    changed_files.join(", ")
                );

                if full_render {
                    // Full render mode
                    match crate::commands::render::run(&project_path, None, None, None, None, false, false, None)
                        .await
                    {
                        Ok(()) => {}
                        Err(e) => {
                            eprintln!("{} Render failed: {}", "watch:".red().bold(), e);
                        }
                    }
                } else {
                    // Preview mode: detect which scene changed, or use fixed_scene
                    let scene_index = fixed_scene
                        .unwrap_or_else(|| detect_changed_scene(&relevant, &project_path));

                    let output_dir = project_path.join("output");
                    std::fs::create_dir_all(&output_dir).ok();
                    let output_path = output_dir.join(format!("preview-scene-{scene_index}.png"));

                    match crate::commands::preview::run(
                        &project_path,
                        scene_index,
                        0,
                        Some(output_path),
                    )
                    .await
                    {
                        Ok(()) => {}
                        Err(e) => {
                            eprintln!("{} Preview failed: {}", "watch:".red().bold(), e);
                        }
                    }
                }

                eprintln!("{} Watching for changes...", "watch:".cyan().bold());
            }
            Ok(Err(e)) => {
                eprintln!("{} Watcher error: {:?}", "watch:".red().bold(), e);
            }
            Err(e) => {
                // Channel closed
                return Err(VidgenError::Other(format!("Watcher channel closed: {e}")));
            }
        }
    }
}

/// Try to detect which scene was changed based on the file paths.
/// Returns the 0-based scene index, defaulting to 0.
fn detect_changed_scene(
    events: &[&notify_debouncer_mini::DebouncedEvent],
    project_path: &Path,
) -> usize {
    let scenes_dir = project_path.join("scenes");

    for event in events {
        if event.path.starts_with(&scenes_dir) {
            if let Some(name) = event.path.file_stem().and_then(|n| n.to_str()) {
                // Parse the leading number from filenames like "03-title-card"
                if let Some(num_str) = name.split('-').next() {
                    if let Ok(num) = num_str.parse::<usize>() {
                        // Scene files are 1-indexed (01-xxx.md), convert to 0-based
                        return num.saturating_sub(1);
                    }
                }
            }
        }
    }

    0
}
