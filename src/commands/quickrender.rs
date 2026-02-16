use crate::commands;
use crate::error::VidgenResult;
use crate::scene::SceneDuration;
use colored::*;
use std::collections::HashMap;
use std::path::Path;

/// Run the quickrender command: create a temp project with a single scene and render it.
pub async fn run(
    text: &str,
    template: &str,
    output: &Path,
    voice: Option<&str>,
    quality: Option<&str>,
    props_json: Option<&str>,
) -> VidgenResult<()> {
    eprintln!(
        "{} Quick render: template={}, output={}",
        "quickrender:".cyan().bold(),
        template,
        output.display()
    );

    // Create a temp project directory
    let temp_dir = tempfile::tempdir()?;
    let project_path = temp_dir.path().join("quickrender-project");

    // Parse optional props JSON
    let props: Option<HashMap<String, serde_json::Value>> = match props_json {
        Some(json_str) => {
            let parsed: HashMap<String, serde_json::Value> = serde_json::from_str(json_str)
                .map_err(|e| {
                    crate::error::VidgenError::Other(format!("Invalid --props JSON: {e}"))
                })?;
            Some(parsed)
        }
        None => None,
    };

    // Create the temp project with a single auto-duration scene
    let scene = commands::init::SceneInput {
        template: Some(template.to_string()),
        script: text.to_string(),
        duration: Some(SceneDuration::Auto),
        props,
        transition: None,
        voice: None,
        background: None,
    };

    let opts = commands::init::CreateProjectOptions {
        path: project_path.clone(),
        name: Some("quickrender".to_string()),
        fps: None,
        width: None,
        height: None,
        quality: quality.map(|q| q.to_string()),
        voice: None,
        formats: None,
        theme: None,
        scenes: Some(vec![scene]),
    };

    commands::init::create_project(&opts)?;

    // If voice is specified, update the config
    if let Some(voice_str) = voice {
        let update = crate::config::ConfigUpdate {
            fps: None,
            width: None,
            height: None,
            quality: None,
            primary: None,
            secondary: None,
            background: None,
            text: None,
            font_heading: None,
            font_body: None,
            default_transition: None,
            default_transition_duration: None,
            voice_engine: None,
            default_voice: Some(voice_str.to_string()),
            voice_speed: None,
            padding_before: None,
            padding_after: None,
            auto_fallback_duration: None,
            formats: None,
        };
        crate::config::update_config(&project_path, &update)?;
    }

    // Render the project (single default format, no multi-format for quickrender)
    let results = commands::render::render_project(
        &project_path,
        None,
        quality.map(|q| q.to_string()),
        None,
        None,
        None,
        None,
        None,
    )
    .await?;

    let result = &results[0];

    // Copy the rendered output to the desired location
    let rendered_path = Path::new(&result.output_path);
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::copy(rendered_path, output)?;

    eprintln!(
        "{} Output: {} ({:.1}s, {} scene)",
        "done:".green().bold(),
        output.display(),
        result.duration_secs,
        result.scenes_rendered
    );

    Ok(())
}
