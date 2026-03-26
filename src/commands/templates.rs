use crate::error::VidgenResult;
use crate::render::browser::capture_single_frame;
use crate::scene::{Scene, SceneDuration, SceneFrontmatter};
use crate::template::TemplateRegistry;
use colored::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Built-in templates — these are always available.
const BUILTIN_TEMPLATES: &[&str] = &[
    "title-card",
    "content-text",
    "quote-card",
    "lower-third",
    "cta-card",
    "split-screen",
    "kinetic-text",
    "slideshow",
    "caption-overlay",
];

/// List available templates and optionally render preview thumbnails.
pub async fn run(project_path: Option<&Path>, output_dir: Option<&Path>) -> VidgenResult<()> {
    let mut registry = TemplateRegistry::new()?;

    // Register project templates if a project path is provided
    if let Some(pp) = project_path {
        registry.register_project_templates(pp)?;
    }

    let names = registry.template_names();

    eprintln!(
        "{} {} template(s) available\n",
        "templates:".cyan().bold(),
        names.len()
    );

    // Determine output directory for thumbnails
    let thumb_dir = if let Some(dir) = output_dir {
        dir.to_path_buf()
    } else {
        std::env::temp_dir().join("vidgen-template-previews")
    };
    std::fs::create_dir_all(&thumb_dir)?;

    // Default theme for preview rendering
    let theme = crate::config::ThemeConfig::default();
    let width: u32 = 640;
    let height: u32 = 360;

    for name in &names {
        let is_builtin = BUILTIN_TEMPLATES.contains(&name.as_str());
        let label = if is_builtin { "built-in" } else { "project" };

        // Build a minimal scene with default props for this template
        let scene = build_preview_scene(name);
        let total_frames = 100u32;
        let mid_frame = 50u32; // 50% progress

        match registry.render_scene_html(
            &scene,
            &theme,
            width,
            height,
            mid_frame,
            total_frames,
            project_path,
        ) {
            Ok(html) => {
                match capture_single_frame(&html, width, height, mid_frame, total_frames).await {
                    Ok(png_data) => {
                        let thumb_path = thumb_dir.join(format!("{name}.png"));
                        std::fs::write(&thumb_path, &png_data)?;
                        eprintln!(
                            "  {} {} [{}] -> {}",
                            "OK".green().bold(),
                            name,
                            label,
                            thumb_path.display()
                        );
                    }
                    Err(e) => {
                        eprintln!(
                            "  {} {} [{}] (preview failed: {})",
                            "!!".yellow().bold(),
                            name,
                            label,
                            e
                        );
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "  {} {} [{}] (render failed: {})",
                    "!!".yellow().bold(),
                    name,
                    label,
                    e
                );
            }
        }
    }

    eprintln!("\n  Thumbnails saved to: {}", thumb_dir.display());

    Ok(())
}

/// Build a minimal Scene with sensible default props for thumbnail preview.
fn build_preview_scene(template_name: &str) -> Scene {
    let mut props: HashMap<String, serde_json::Value> = HashMap::new();

    match template_name {
        "title-card" => {
            props.insert("title".into(), serde_json::json!("Title Card"));
            props.insert("subtitle".into(), serde_json::json!("Preview"));
        }
        "content-text" => {
            props.insert("heading".into(), serde_json::json!("Content Heading"));
            props.insert(
                "body".into(),
                serde_json::json!("This is a content text template with a sample body paragraph."),
            );
        }
        "quote-card" => {
            props.insert(
                "quote".into(),
                serde_json::json!("The best way to predict the future is to create it."),
            );
            props.insert("author".into(), serde_json::json!("Preview Author"));
        }
        "lower-third" => {
            props.insert("name".into(), serde_json::json!("Speaker Name"));
            props.insert("title".into(), serde_json::json!("Speaker Title"));
        }
        "cta-card" => {
            props.insert("heading".into(), serde_json::json!("Call to Action"));
            props.insert(
                "items".into(),
                serde_json::json!(["Item 1", "Item 2", "Item 3"]),
            );
        }
        "split-screen" => {
            props.insert(
                "panels".into(),
                serde_json::json!([
                    {"label": "Left Panel", "content": "Left content"},
                    {"label": "Right Panel", "content": "Right content"}
                ]),
            );
        }
        "kinetic-text" => {
            props.insert(
                "text".into(),
                serde_json::json!("Kinetic text preview words"),
            );
        }
        "slideshow" => {
            props.insert(
                "slides".into(),
                serde_json::json!([
                    {"title": "Slide 1", "body": "First slide"},
                    {"title": "Slide 2", "body": "Second slide"}
                ]),
            );
        }
        "caption-overlay" => {
            props.insert(
                "text".into(),
                serde_json::json!("Caption overlay preview text"),
            );
        }
        _ => {
            // Unknown template — provide generic title prop
            props.insert("title".into(), serde_json::json!(template_name));
        }
    }

    Scene {
        frontmatter: SceneFrontmatter {
            template: template_name.to_string(),
            duration: SceneDuration::Fixed(5.0),
            video_source: None,
            source_volume: None,
            sub_scenes: None,
            overlay: None,
            props,
            background: None,
            transition_in: None,
            transition_out: None,
            transition_duration: None,
            voice: None,
            audio: None,
            format_overrides: None,
        },
        script: String::new(),
        source_path: PathBuf::from("preview.md"),
    }
}
