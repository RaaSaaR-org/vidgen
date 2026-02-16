use crate::config;
use crate::error::{VidgenError, VidgenResult};
use crate::render::browser::capture_single_frame;
use crate::scene::{self, SceneDuration};
use crate::template::TemplateRegistry;
use base64::Engine;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Shared utilities
// ---------------------------------------------------------------------------

/// Return sorted `.md` file paths from the `scenes/` directory.
pub fn scene_file_paths(project_path: &Path) -> VidgenResult<Vec<PathBuf>> {
    let scenes_dir = project_path.join("scenes");
    if !scenes_dir.exists() {
        return Ok(vec![]);
    }
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&scenes_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "md"))
        .collect();
    entries.sort();
    Ok(entries)
}

/// Extract the template slug from a scene filename: `01-title-card.md` → `title-card`.
fn extract_scene_slug(path: &Path) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("scene");
    // Strip the leading numeric prefix (e.g., "01-")
    if let Some(pos) = stem.find('-') {
        let after = &stem[pos + 1..];
        if !after.is_empty() {
            return after.to_string();
        }
    }
    stem.to_string()
}

/// Two-phase rename of scene files to sequential numbering.
/// `files_with_slugs` is (slug, original_path) in desired order.
fn renumber_scene_files(
    scenes_dir: &Path,
    files_with_slugs: &[(String, PathBuf)],
) -> VidgenResult<Vec<PathBuf>> {
    // Phase 1: rename all to temp names to avoid collisions
    let mut temp_paths = Vec::new();
    for (i, (_slug, original)) in files_with_slugs.iter().enumerate() {
        let tmp = scenes_dir.join(format!("__tmp_{i:04}.md"));
        std::fs::rename(original, &tmp)?;
        temp_paths.push(tmp);
    }

    // Phase 2: rename to final sequential names
    let mut final_paths = Vec::new();
    for (i, (slug, _)) in files_with_slugs.iter().enumerate() {
        let final_name = format!("{:02}-{slug}.md", i + 1);
        let final_path = scenes_dir.join(&final_name);
        std::fs::rename(&temp_paths[i], &final_path)?;
        final_paths.push(final_path);
    }

    Ok(final_paths)
}

/// Format a SceneDuration for YAML frontmatter output.
fn format_duration_yaml(duration: &SceneDuration) -> String {
    match duration {
        SceneDuration::Auto => "auto".to_string(),
        SceneDuration::Fixed(d) => {
            if *d == d.floor() {
                format!("{}", *d as i64)
            } else {
                format!("{d}")
            }
        }
    }
}

/// Write a scene input to a file as markdown with YAML frontmatter.
#[allow(clippy::too_many_arguments)]
fn write_scene_input_to_file(
    template: &str,
    script: &str,
    duration: Option<&SceneDuration>,
    props: &Option<HashMap<String, serde_json::Value>>,
    transition: Option<&str>,
    voice: Option<&str>,
    background: Option<&str>,
    path: &Path,
) -> VidgenResult<()> {
    let mut frontmatter = String::new();
    frontmatter.push_str(&format!("template: {template}\n"));
    if let Some(dur) = duration {
        frontmatter.push_str(&format!("duration: {}\n", format_duration_yaml(dur)));
    }
    if let Some(t) = transition {
        frontmatter.push_str(&format!("transition_in: {t}\n"));
    }
    if let Some(v) = voice {
        frontmatter.push_str(&format!("voice: {v}\n"));
    }
    if let Some(bg) = background {
        frontmatter.push_str(&format!("background:\n  color: \"{bg}\"\n"));
    }
    if let Some(props) = props {
        if !props.is_empty() {
            frontmatter.push_str("props:\n");
            let props_yaml = serde_yml::to_string(props).unwrap_or_default();
            for line in props_yaml.lines() {
                frontmatter.push_str(&format!("  {line}\n"));
            }
        }
    }
    let content = format!("---\n{frontmatter}---\n\n{script}\n");
    std::fs::write(path, content)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Scene input struct (reused from init, but self-contained here for MCP)
// ---------------------------------------------------------------------------

/// A scene to add, with all fields optional except script.
pub struct SceneInput {
    pub template: Option<String>,
    pub script: String,
    pub duration: Option<SceneDuration>,
    pub props: Option<HashMap<String, serde_json::Value>>,
    pub transition: Option<String>,
    pub voice: Option<String>,
    pub background: Option<String>,
}

// ---------------------------------------------------------------------------
// add_scenes
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct AddScenesResult {
    pub scenes_added: usize,
    pub total_scenes: usize,
    pub files: Vec<String>,
}

pub fn add_scenes(
    project_path: &Path,
    insert_at: Option<usize>,
    scenes: Vec<SceneInput>,
) -> VidgenResult<AddScenesResult> {
    let scenes_dir = project_path.join("scenes");
    std::fs::create_dir_all(&scenes_dir)?;

    let existing = scene_file_paths(project_path)?;
    let count = existing.len();
    let insert_pos = insert_at.unwrap_or(count);

    if insert_pos > count {
        return Err(VidgenError::SceneIndexOutOfRange {
            index: insert_pos,
            count,
        });
    }

    // Write new scene files to temp names first
    let mut new_paths = Vec::new();
    for (i, input) in scenes.iter().enumerate() {
        let template = input.template.as_deref().unwrap_or("title-card");
        let tmp_path = scenes_dir.join(format!("__new_{i:04}.md"));
        write_scene_input_to_file(
            template,
            &input.script,
            input.duration.as_ref(),
            &input.props,
            input.transition.as_deref(),
            input.voice.as_deref(),
            input.background.as_deref(),
            &tmp_path,
        )?;
        new_paths.push((template.to_string(), tmp_path));
    }

    // Build combined file list: existing[..insert_pos] + new + existing[insert_pos..]
    let mut combined: Vec<(String, PathBuf)> = Vec::new();
    for path in &existing[..insert_pos] {
        combined.push((extract_scene_slug(path), path.clone()));
    }
    for (template, path) in &new_paths {
        combined.push((template.clone(), path.clone()));
    }
    for path in &existing[insert_pos..] {
        combined.push((extract_scene_slug(path), path.clone()));
    }

    let final_paths = renumber_scene_files(&scenes_dir, &combined)?;

    let files: Vec<String> = final_paths
        .iter()
        .map(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string()
        })
        .collect();

    Ok(AddScenesResult {
        scenes_added: scenes.len(),
        total_scenes: final_paths.len(),
        files,
    })
}

// ---------------------------------------------------------------------------
// update_scene
// ---------------------------------------------------------------------------

/// Partial update for a scene. All fields optional — only non-None fields are applied.
pub struct SceneUpdate {
    pub template: Option<String>,
    pub script: Option<String>,
    pub duration: Option<SceneDuration>,
    pub props: Option<HashMap<String, serde_json::Value>>,
    pub transition_in: Option<String>,
    pub transition_out: Option<String>,
    pub voice: Option<String>,
}

#[derive(Serialize)]
pub struct UpdateSceneResult {
    pub scene_index: usize,
    pub file: String,
    pub fields_updated: Vec<String>,
}

pub fn update_scene(
    project_path: &Path,
    scene_index: usize,
    update: SceneUpdate,
) -> VidgenResult<UpdateSceneResult> {
    let paths = scene_file_paths(project_path)?;
    let count = paths.len();
    if scene_index >= count {
        return Err(VidgenError::SceneIndexOutOfRange {
            index: scene_index,
            count,
        });
    }

    let path = &paths[scene_index];
    let content = std::fs::read_to_string(path)?;
    let mut scene = scene::parse_scene(&content, path)?;
    let mut fields_updated = Vec::new();

    if let Some(ref template) = update.template {
        scene.frontmatter.template = template.clone();
        fields_updated.push("template".to_string());
    }
    if let Some(ref script) = update.script {
        scene.script = script.clone();
        fields_updated.push("script".to_string());
    }
    if let Some(ref duration) = update.duration {
        scene.frontmatter.duration = duration.clone();
        fields_updated.push("duration".to_string());
    }
    if let Some(ref props) = update.props {
        // Merge semantics: new props are merged into existing
        for (key, value) in props {
            scene.frontmatter.props.insert(key.clone(), value.clone());
        }
        fields_updated.push("props".to_string());
    }
    if let Some(ref transition_in) = update.transition_in {
        scene.frontmatter.transition_in = Some(transition_in.clone());
        fields_updated.push("transition_in".to_string());
    }
    if let Some(ref transition_out) = update.transition_out {
        scene.frontmatter.transition_out = Some(transition_out.clone());
        fields_updated.push("transition_out".to_string());
    }
    if let Some(ref voice) = update.voice {
        scene.frontmatter.voice = Some(voice.clone());
        fields_updated.push("voice".to_string());
    }

    scene::write_scene(&scene, path)?;

    // If template changed, rename the file to match the new slug
    let mut final_path = path.clone();
    if update.template.is_some() {
        let scenes_dir = project_path.join("scenes");
        let paths = scene_file_paths(project_path)?;
        let slugs: Vec<(String, PathBuf)> = paths
            .iter()
            .map(|p| (extract_scene_slug(p), p.clone()))
            .collect();
        // Re-derive slug for the updated scene
        let mut new_slugs = slugs;
        new_slugs[scene_index].0 = scene.frontmatter.template.clone();
        let final_paths = renumber_scene_files(&scenes_dir, &new_slugs)?;
        final_path = final_paths[scene_index].clone();
    }

    let file = final_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    Ok(UpdateSceneResult {
        scene_index,
        file,
        fields_updated,
    })
}

// ---------------------------------------------------------------------------
// remove_scenes
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct RemoveScenesResult {
    pub scenes_removed: usize,
    pub remaining_scenes: usize,
    pub files: Vec<String>,
}

pub fn remove_scenes(project_path: &Path, indices: &[usize]) -> VidgenResult<RemoveScenesResult> {
    let paths = scene_file_paths(project_path)?;
    let count = paths.len();

    // Validate all indices
    for &idx in indices {
        if idx >= count {
            return Err(VidgenError::SceneIndexOutOfRange { index: idx, count });
        }
    }

    let scenes_dir = project_path.join("scenes");

    // Delete the files at the given indices
    let mut to_remove: Vec<usize> = indices.to_vec();
    to_remove.sort_unstable();
    to_remove.dedup();

    for &idx in to_remove.iter().rev() {
        std::fs::remove_file(&paths[idx])?;
    }

    // Collect remaining files with their slugs
    let remaining: Vec<(String, PathBuf)> = paths
        .iter()
        .enumerate()
        .filter(|(i, _)| !to_remove.contains(i))
        .map(|(_, p)| (extract_scene_slug(p), p.clone()))
        .collect();

    let final_paths = if remaining.is_empty() {
        vec![]
    } else {
        renumber_scene_files(&scenes_dir, &remaining)?
    };

    let files: Vec<String> = final_paths
        .iter()
        .map(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string()
        })
        .collect();

    Ok(RemoveScenesResult {
        scenes_removed: to_remove.len(),
        remaining_scenes: final_paths.len(),
        files,
    })
}

// ---------------------------------------------------------------------------
// reorder_scenes
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ReorderScenesResult {
    pub total_scenes: usize,
    pub files: Vec<String>,
}

pub fn reorder_scenes(project_path: &Path, order: &[usize]) -> VidgenResult<ReorderScenesResult> {
    let paths = scene_file_paths(project_path)?;
    let count = paths.len();

    // Validate: order must be a permutation of 0..count
    if order.len() != count {
        return Err(VidgenError::InvalidSceneOrder(format!(
            "expected {} indices, got {}",
            count,
            order.len()
        )));
    }
    let mut seen = vec![false; count];
    for &idx in order {
        if idx >= count {
            return Err(VidgenError::SceneIndexOutOfRange { index: idx, count });
        }
        if seen[idx] {
            return Err(VidgenError::InvalidSceneOrder(format!(
                "duplicate index: {idx}"
            )));
        }
        seen[idx] = true;
    }

    let scenes_dir = project_path.join("scenes");

    // Apply permutation
    let reordered: Vec<(String, PathBuf)> = order
        .iter()
        .map(|&idx| (extract_scene_slug(&paths[idx]), paths[idx].clone()))
        .collect();

    let final_paths = renumber_scene_files(&scenes_dir, &reordered)?;

    let files: Vec<String> = final_paths
        .iter()
        .map(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string()
        })
        .collect();

    Ok(ReorderScenesResult {
        total_scenes: final_paths.len(),
        files,
    })
}

// ---------------------------------------------------------------------------
// list_voices
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct VoiceInfo {
    pub id: String,
    pub name: String,
    pub language: String,
    pub gender: String,
    pub engine: String,
    pub available: bool,
    pub note: Option<String>,
}

pub fn list_voices() -> Vec<VoiceInfo> {
    let mut all_voices = Vec::new();

    // Query each known engine; skip engines that aren't installed
    for engine_name in &["native", "edge", "elevenlabs"] {
        let voice_config = crate::config::VoiceConfig {
            engine: (*engine_name).to_string(),
            ..crate::config::VoiceConfig::default()
        };
        if let Ok(engine) = crate::tts::create_engine(&voice_config) {
            if let Ok(tts_voices) = engine.list_voices() {
                all_voices.extend(tts_voices.into_iter().map(|v| VoiceInfo {
                    id: v.id,
                    name: v.name,
                    language: v.language,
                    gender: v.gender,
                    engine: v.engine,
                    available: v.available,
                    note: v.note,
                }));
            }
        }
    }

    if all_voices.is_empty() {
        // Fallback: return stub data if no engine is available
        let note = Some("No TTS engine available. macOS: 'say' (built-in). Linux: install espeak-ng. For neural voices: pip install edge-tts".to_string());
        vec![VoiceInfo {
            id: "unavailable".into(),
            name: "No TTS engine".into(),
            language: "n/a".into(),
            gender: "n/a".into(),
            engine: "none".into(),
            available: false,
            note,
        }]
    } else {
        all_voices
    }
}

// ---------------------------------------------------------------------------
// preview_scene
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct PreviewResult {
    pub scene_index: usize,
    pub width: u32,
    pub height: u32,
    pub png_base64: String,
}

pub async fn preview_scene(
    project_path: &Path,
    scene_index: usize,
    frame: Option<u32>,
) -> VidgenResult<PreviewResult> {
    let cfg = config::load_config(project_path)?;
    let scenes = scene::load_scenes(project_path)?;
    let count = scenes.len();

    if scene_index >= count {
        return Err(VidgenError::SceneIndexOutOfRange {
            index: scene_index,
            count,
        });
    }

    let scene = &scenes[scene_index];
    let width = cfg.video.width;
    let height = cfg.video.height;
    let total_frames = scene.total_frames(cfg.video.fps);
    let frame = frame.unwrap_or(0);

    if frame >= total_frames {
        return Err(VidgenError::Other(format!(
            "Frame {frame} out of range (scene has {total_frames} frames, 0-indexed)"
        )));
    }

    let mut registry = TemplateRegistry::new()?;
    registry.register_project_templates(project_path)?;
    let html = registry.render_scene_html(scene, &cfg.theme, width, height, frame, total_frames)?;

    let screenshot = capture_single_frame(&html, width, height, frame, total_frames).await?;
    let png_base64 = base64::engine::general_purpose::STANDARD.encode(&screenshot);

    Ok(PreviewResult {
        scene_index,
        width,
        height,
        png_base64,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::init::{self, CreateProjectOptions};

    /// Create a test project with the given scenes.
    fn setup_project(dir: &Path, scenes: Vec<init::SceneInput>) -> PathBuf {
        let project_path = dir.join("test-project");
        let opts = CreateProjectOptions {
            path: project_path.clone(),
            name: Some("Test".to_string()),
            fps: None,
            width: None,
            height: None,
            quality: None,
            voice: None,
            formats: None,
            theme: None,
            scenes: Some(scenes),
        };
        init::create_project(&opts).unwrap();
        project_path
    }

    fn make_scene(template: &str, script: &str) -> init::SceneInput {
        init::SceneInput {
            template: Some(template.to_string()),
            script: script.to_string(),
            duration: Some(SceneDuration::Fixed(5.0)),
            props: None,
            transition: None,
            voice: None,
            background: None,
        }
    }

    #[test]
    fn test_add_scenes_append() {
        let dir = tempfile::tempdir().unwrap();
        let project = setup_project(dir.path(), vec![make_scene("title-card", "Scene 1")]);

        let result = add_scenes(
            &project,
            None,
            vec![SceneInput {
                template: Some("content-text".to_string()),
                script: "Scene 2".to_string(),
                duration: Some(SceneDuration::Fixed(3.0)),
                props: None,
                transition: None,
                voice: None,
                background: None,
            }],
        )
        .unwrap();

        assert_eq!(result.scenes_added, 1);
        assert_eq!(result.total_scenes, 2);
        assert_eq!(result.files, vec!["01-title-card.md", "02-content-text.md"]);

        // Verify content
        let scenes = scene::load_scenes(&project).unwrap();
        assert_eq!(scenes[1].script, "Scene 2");
    }

    #[test]
    fn test_add_scenes_insert() {
        let dir = tempfile::tempdir().unwrap();
        let project = setup_project(
            dir.path(),
            vec![
                make_scene("title-card", "First"),
                make_scene("content-text", "Last"),
            ],
        );

        let result = add_scenes(
            &project,
            Some(1), // insert between first and last
            vec![SceneInput {
                template: Some("title-card".to_string()),
                script: "Middle".to_string(),
                duration: None,
                props: None,
                transition: None,
                voice: None,
                background: None,
            }],
        )
        .unwrap();

        assert_eq!(result.total_scenes, 3);

        let scenes = scene::load_scenes(&project).unwrap();
        assert_eq!(scenes[0].script, "First");
        assert_eq!(scenes[1].script, "Middle");
        assert_eq!(scenes[2].script, "Last");
    }

    #[test]
    fn test_update_scene_partial() {
        let dir = tempfile::tempdir().unwrap();
        let project = setup_project(
            dir.path(),
            vec![make_scene("title-card", "Original script")],
        );

        let result = update_scene(
            &project,
            0,
            SceneUpdate {
                template: None,
                script: Some("Updated script".to_string()),
                duration: Some(SceneDuration::Fixed(10.0)),
                props: None,
                transition_in: None,
                transition_out: None,
                voice: None,
            },
        )
        .unwrap();

        assert_eq!(result.scene_index, 0);
        assert!(result.fields_updated.contains(&"script".to_string()));
        assert!(result.fields_updated.contains(&"duration".to_string()));

        let scenes = scene::load_scenes(&project).unwrap();
        assert_eq!(scenes[0].script, "Updated script");
        assert_eq!(scenes[0].frontmatter.duration, SceneDuration::Fixed(10.0));
        assert_eq!(scenes[0].frontmatter.template, "title-card"); // unchanged
    }

    #[test]
    fn test_update_scene_to_auto_duration() {
        let dir = tempfile::tempdir().unwrap();
        let project = setup_project(
            dir.path(),
            vec![make_scene("title-card", "Original script")],
        );

        update_scene(
            &project,
            0,
            SceneUpdate {
                template: None,
                script: None,
                duration: Some(SceneDuration::Auto),
                props: None,
                transition_in: None,
                transition_out: None,
                voice: None,
            },
        )
        .unwrap();

        let scenes = scene::load_scenes(&project).unwrap();
        assert_eq!(scenes[0].frontmatter.duration, SceneDuration::Auto);
    }

    #[test]
    fn test_update_scene_props_merge() {
        let dir = tempfile::tempdir().unwrap();
        let mut initial_props = HashMap::new();
        initial_props.insert(
            "title".to_string(),
            serde_json::Value::String("Hello".into()),
        );
        initial_props.insert(
            "subtitle".to_string(),
            serde_json::Value::String("World".into()),
        );

        let project = setup_project(
            dir.path(),
            vec![init::SceneInput {
                template: Some("title-card".to_string()),
                script: "Script".to_string(),
                duration: Some(SceneDuration::Fixed(5.0)),
                props: Some(initial_props),
                transition: None,
                voice: None,
                background: None,
            }],
        );

        // Update: change title, add new prop, subtitle should remain
        let mut new_props = HashMap::new();
        new_props.insert(
            "title".to_string(),
            serde_json::Value::String("Updated".into()),
        );
        new_props.insert("extra".to_string(), serde_json::Value::String("New".into()));

        update_scene(
            &project,
            0,
            SceneUpdate {
                template: None,
                script: None,
                duration: None,
                props: Some(new_props),
                transition_in: None,
                transition_out: None,
                voice: None,
            },
        )
        .unwrap();

        let scenes = scene::load_scenes(&project).unwrap();
        let props = &scenes[0].frontmatter.props;
        assert_eq!(
            props.get("title").unwrap(),
            &serde_json::Value::String("Updated".into())
        );
        assert_eq!(
            props.get("subtitle").unwrap(),
            &serde_json::Value::String("World".into())
        );
        assert_eq!(
            props.get("extra").unwrap(),
            &serde_json::Value::String("New".into())
        );
    }

    #[test]
    fn test_remove_scenes() {
        let dir = tempfile::tempdir().unwrap();
        let project = setup_project(
            dir.path(),
            vec![
                make_scene("title-card", "First"),
                make_scene("content-text", "Second"),
                make_scene("title-card", "Third"),
            ],
        );

        let result = remove_scenes(&project, &[1]).unwrap();
        assert_eq!(result.scenes_removed, 1);
        assert_eq!(result.remaining_scenes, 2);

        let scenes = scene::load_scenes(&project).unwrap();
        assert_eq!(scenes.len(), 2);
        assert_eq!(scenes[0].script, "First");
        assert_eq!(scenes[1].script, "Third");
    }

    #[test]
    fn test_remove_scenes_multiple() {
        let dir = tempfile::tempdir().unwrap();
        let project = setup_project(
            dir.path(),
            vec![
                make_scene("title-card", "A"),
                make_scene("content-text", "B"),
                make_scene("title-card", "C"),
                make_scene("content-text", "D"),
            ],
        );

        let result = remove_scenes(&project, &[0, 2]).unwrap();
        assert_eq!(result.scenes_removed, 2);
        assert_eq!(result.remaining_scenes, 2);

        let scenes = scene::load_scenes(&project).unwrap();
        assert_eq!(scenes[0].script, "B");
        assert_eq!(scenes[1].script, "D");
    }

    #[test]
    fn test_reorder_scenes() {
        let dir = tempfile::tempdir().unwrap();
        let project = setup_project(
            dir.path(),
            vec![
                make_scene("title-card", "First"),
                make_scene("content-text", "Second"),
                make_scene("title-card", "Third"),
            ],
        );

        let result = reorder_scenes(&project, &[2, 0, 1]).unwrap();
        assert_eq!(result.total_scenes, 3);

        let scenes = scene::load_scenes(&project).unwrap();
        assert_eq!(scenes[0].script, "Third");
        assert_eq!(scenes[1].script, "First");
        assert_eq!(scenes[2].script, "Second");
    }

    #[test]
    fn test_reorder_invalid_permutation() {
        let dir = tempfile::tempdir().unwrap();
        let project = setup_project(
            dir.path(),
            vec![
                make_scene("title-card", "A"),
                make_scene("content-text", "B"),
            ],
        );

        // Wrong length
        let result = reorder_scenes(&project, &[0]);
        assert!(result.is_err());

        // Duplicate index
        let result = reorder_scenes(&project, &[0, 0]);
        assert!(result.is_err());

        // Out of range
        let result = reorder_scenes(&project, &[0, 5]);
        assert!(result.is_err());
    }

    #[test]
    fn test_scene_index_out_of_range() {
        let dir = tempfile::tempdir().unwrap();
        let project = setup_project(dir.path(), vec![make_scene("title-card", "Only scene")]);

        let result = update_scene(
            &project,
            5,
            SceneUpdate {
                template: None,
                script: Some("Nope".into()),
                duration: None,
                props: None,
                transition_in: None,
                transition_out: None,
                voice: None,
            },
        );
        assert!(result.is_err());
        match result {
            Err(VidgenError::SceneIndexOutOfRange { index, count }) => {
                assert_eq!(index, 5);
                assert_eq!(count, 1);
            }
            _ => panic!("Expected SceneIndexOutOfRange"),
        }
    }

    #[test]
    fn test_add_scene_with_transition_and_voice() {
        let dir = tempfile::tempdir().unwrap();
        let project = setup_project(dir.path(), vec![make_scene("title-card", "Scene 1")]);

        let result = add_scenes(
            &project,
            None,
            vec![SceneInput {
                template: Some("content-text".to_string()),
                script: "Scene 2".to_string(),
                duration: None,
                props: None,
                transition: Some("fade".to_string()),
                voice: Some("en-US-AriaNeural".to_string()),
                background: Some("#112233".to_string()),
            }],
        )
        .unwrap();

        assert_eq!(result.total_scenes, 2);

        let scenes = scene::load_scenes(&project).unwrap();
        assert_eq!(
            scenes[1].frontmatter.transition_in.as_deref(),
            Some("fade")
        );
        assert_eq!(
            scenes[1].frontmatter.voice.as_deref(),
            Some("en-US-AriaNeural")
        );
        assert_eq!(
            scenes[1]
                .frontmatter
                .background
                .as_ref()
                .and_then(|bg| bg.color.as_deref()),
            Some("#112233")
        );
    }

    #[test]
    fn test_list_voices() {
        let voices = list_voices();
        // On macOS/Linux with TTS available: real voices returned.
        // Otherwise: fallback stub with at least 1 entry.
        assert!(!voices.is_empty());
    }
}
