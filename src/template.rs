use crate::config::ThemeConfig;
use crate::error::{VidgenError, VidgenResult};
use crate::scene::Scene;
use handlebars::Handlebars;
use serde_json::json;
use std::path::Path;
use tracing::{debug, trace};

const TITLE_CARD_TEMPLATE: &str = include_str!("templates/title-card.html");
const CONTENT_TEXT_TEMPLATE: &str = include_str!("templates/content-text.html");
const QUOTE_CARD_TEMPLATE: &str = include_str!("templates/quote-card.html");
const LOWER_THIRD_TEMPLATE: &str = include_str!("templates/lower-third.html");
const CTA_CARD_TEMPLATE: &str = include_str!("templates/cta-card.html");
const SPLIT_SCREEN_TEMPLATE: &str = include_str!("templates/split-screen.html");
const KINETIC_TEXT_TEMPLATE: &str = include_str!("templates/kinetic-text.html");
const SLIDESHOW_TEMPLATE: &str = include_str!("templates/slideshow.html");
const CAPTION_OVERLAY_TEMPLATE: &str = include_str!("templates/caption-overlay.html");

pub struct TemplateRegistry<'a> {
    hbs: Handlebars<'a>,
}

impl<'a> TemplateRegistry<'a> {
    pub fn new() -> VidgenResult<Self> {
        let mut hbs = Handlebars::new();
        hbs.set_strict_mode(false); // Allow missing optional variables

        hbs.register_template_string("title-card", TITLE_CARD_TEMPLATE)
            .map_err(|e| VidgenError::TemplateRender(e.to_string()))?;
        hbs.register_template_string("content-text", CONTENT_TEXT_TEMPLATE)
            .map_err(|e| VidgenError::TemplateRender(e.to_string()))?;
        hbs.register_template_string("quote-card", QUOTE_CARD_TEMPLATE)
            .map_err(|e| VidgenError::TemplateRender(e.to_string()))?;
        hbs.register_template_string("lower-third", LOWER_THIRD_TEMPLATE)
            .map_err(|e| VidgenError::TemplateRender(e.to_string()))?;
        hbs.register_template_string("cta-card", CTA_CARD_TEMPLATE)
            .map_err(|e| VidgenError::TemplateRender(e.to_string()))?;
        hbs.register_template_string("split-screen", SPLIT_SCREEN_TEMPLATE)
            .map_err(|e| VidgenError::TemplateRender(e.to_string()))?;
        hbs.register_template_string("kinetic-text", KINETIC_TEXT_TEMPLATE)
            .map_err(|e| VidgenError::TemplateRender(e.to_string()))?;
        hbs.register_template_string("slideshow", SLIDESHOW_TEMPLATE)
            .map_err(|e| VidgenError::TemplateRender(e.to_string()))?;
        hbs.register_template_string("caption-overlay", CAPTION_OVERLAY_TEMPLATE)
            .map_err(|e| VidgenError::TemplateRender(e.to_string()))?;

        debug!("Template registry initialized with 9 built-in templates");
        Ok(Self { hbs })
    }

    /// Register project-local templates from `<project_path>/templates/components/*.html`.
    /// Project templates can override built-in templates.
    pub fn register_project_templates(&mut self, project_path: &Path) -> VidgenResult<()> {
        let components_dir = project_path.join("templates").join("components");
        if !components_dir.exists() {
            return Ok(());
        }
        let entries = std::fs::read_dir(&components_dir)?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "html") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    debug!("Registering project template: {}", stem);
                    let content = std::fs::read_to_string(&path)?;
                    self.hbs
                        .register_template_string(stem, &content)
                        .map_err(|e| {
                            VidgenError::TemplateRender(format!(
                                "Failed to register project template '{}': {}",
                                stem, e
                            ))
                        })?;
                }
            }
        }
        Ok(())
    }

    /// Render a scene to a full HTML document string.
    ///
    /// `frame` and `total_frames` are injected for CSS custom property animation.
    pub fn render_scene_html(
        &self,
        scene: &Scene,
        theme: &ThemeConfig,
        width: u32,
        height: u32,
        frame: u32,
        total_frames: u32,
    ) -> VidgenResult<String> {
        let template_name = &scene.frontmatter.template;
        trace!(
            "Rendering template '{}' frame {}/{}",
            template_name, frame, total_frames
        );

        if !self.hbs.has_template(template_name) {
            return Err(VidgenError::TemplateNotFound(template_name.clone()));
        }

        // Compute effective background: scene-level override or theme default
        let effective_bg = scene
            .frontmatter
            .background
            .as_ref()
            .and_then(|bg| bg.color.as_ref())
            .unwrap_or(&theme.background);

        // Build the data context — merge theme, frame info, dimensions, and scene props
        let mut data = json!({
            "frame": frame,
            "total_frames": total_frames,
            "width": width,
            "height": height,
            "theme_primary": &theme.primary,
            "theme_secondary": &theme.secondary,
            "theme_background": &theme.background,
            "effective_background": effective_bg,
            "theme_text": &theme.text,
            "theme_font_heading": &theme.font_heading,
            "theme_font_body": &theme.font_body,
            "script": &scene.script,
        });

        // Merge scene props into the top-level data
        if let Some(obj) = data.as_object_mut() {
            for (key, value) in &scene.frontmatter.props {
                obj.insert(key.clone(), value.clone());
            }
        }

        // Inject defaults for lower-third template
        if template_name == "lower-third" {
            if let Some(obj) = data.as_object_mut() {
                if !obj.contains_key("accent_color") {
                    obj.insert("accent_color".into(), json!(&theme.primary));
                }
                if !obj.contains_key("position") {
                    obj.insert("position".into(), json!("left"));
                }
            }
        }

        // Kinetic-text preprocessing: split text/script into individual word objects
        if template_name == "kinetic-text" {
            // Inject style default if not provided
            if let Some(obj) = data.as_object_mut() {
                if !obj.contains_key("style") {
                    obj.insert("style".into(), json!("fade"));
                }
            }
            let text = data
                .as_object()
                .and_then(|o| o.get("text").or(o.get("script")))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let words: Vec<serde_json::Value> = text
                .split_whitespace()
                .enumerate()
                .map(|(i, w)| json!({"word": w, "index": i}))
                .collect();
            let total_words = words.len();
            if let Some(obj) = data.as_object_mut() {
                obj.insert("words".into(), json!(words));
                obj.insert("total_words".into(), json!(total_words));
            }
        }

        // Caption-overlay preprocessing: split text/script into words (same as kinetic-text)
        if template_name == "caption-overlay" {
            // Inject defaults
            if let Some(obj) = data.as_object_mut() {
                if !obj.contains_key("style") {
                    obj.insert("style".into(), json!("outline"));
                }
                if !obj.contains_key("position") {
                    obj.insert("position".into(), json!("bottom"));
                }
            }
            let text = data
                .as_object()
                .and_then(|o| o.get("text").or(o.get("script")))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let words: Vec<serde_json::Value> = text
                .split_whitespace()
                .enumerate()
                .map(|(i, w)| json!({"word": w, "index": i}))
                .collect();
            let total_words = words.len();
            if let Some(obj) = data.as_object_mut() {
                obj.insert("words".into(), json!(words));
                obj.insert("total_words".into(), json!(total_words));
            }
        }

        // Slideshow preprocessing: inject slide indices and total_slides count
        if template_name == "slideshow" {
            let slides = data
                .as_object()
                .and_then(|o| o.get("slides"))
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let total_slides = slides.len().max(1);
            // Determine which slide is active based on the current frame progress
            let progress = if total_frames > 0 {
                frame as f64 / total_frames as f64
            } else {
                0.0
            };
            let active_index = ((progress * total_slides as f64) as usize).min(total_slides - 1);
            let indexed_slides: Vec<serde_json::Value> = slides
                .into_iter()
                .enumerate()
                .map(|(i, mut s)| {
                    if let Some(obj) = s.as_object_mut() {
                        obj.insert("index".into(), json!(i));
                        obj.insert("active".into(), json!(i == active_index));
                    }
                    s
                })
                .collect();
            if let Some(obj) = data.as_object_mut() {
                obj.insert("slides".into(), json!(indexed_slides));
                obj.insert("total_slides".into(), json!(total_slides));
            }
        }

        self.hbs
            .render(template_name, &data)
            .map_err(|e| VidgenError::TemplateRender(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::parse_scene;
    use std::path::Path;

    fn test_theme() -> ThemeConfig {
        ThemeConfig {
            primary: "#2563EB".into(),
            secondary: "#7C3AED".into(),
            background: "#0F172A".into(),
            text: "#F8FAFC".into(),
            font_heading: "Inter".into(),
            font_body: "Inter".into(),
        }
    }

    #[test]
    fn test_render_title_card() {
        let registry = TemplateRegistry::new().unwrap();
        let content = "---\ntemplate: title-card\nduration: 5\nprops:\n  title: \"Hello World\"\n  subtitle: \"Testing\"\n---\nScript.";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        let html = registry
            .render_scene_html(&scene, &test_theme(), 1920, 1080, 0, 150)
            .unwrap();
        assert!(html.contains("Hello World"));
        assert!(html.contains("Testing"));
        assert!(html.contains("1920px"));
        assert!(html.contains("1080px"));
        assert!(html.contains("#0F172A")); // background color
    }

    #[test]
    fn test_render_content_text() {
        let registry = TemplateRegistry::new().unwrap();
        let content = "---\ntemplate: content-text\nprops:\n  heading: \"Chapter 1\"\n  body: \"Some content here\"\n---\nVoiceover.";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        let html = registry
            .render_scene_html(&scene, &test_theme(), 1920, 1080, 75, 150)
            .unwrap();
        assert!(html.contains("Chapter 1"));
        assert!(html.contains("Some content here"));
    }

    #[test]
    fn test_render_quote_card() {
        let registry = TemplateRegistry::new().unwrap();
        let content = r#"---
template: quote-card
props:
  quote: "The only way to do great work is to love what you do."
  author: "Steve Jobs"
  source: "Stanford Commencement, 2005"
---
Voiceover."#;
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        let html = registry
            .render_scene_html(&scene, &test_theme(), 1920, 1080, 75, 150)
            .unwrap();
        assert!(html.contains("The only way to do great work"));
        assert!(html.contains("Steve Jobs"));
        assert!(html.contains("Stanford Commencement, 2005"));
        assert!(html.contains("&ldquo;")); // decorative quote mark
    }

    #[test]
    fn test_render_lower_third() {
        let registry = TemplateRegistry::new().unwrap();
        let content = "---\ntemplate: lower-third\nprops:\n  name: \"Jane Doe\"\n  title: \"CEO, Acme Corp\"\n---\nVoiceover.";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        let html = registry
            .render_scene_html(&scene, &test_theme(), 1920, 1080, 30, 150)
            .unwrap();
        assert!(html.contains("Jane Doe"));
        assert!(html.contains("CEO, Acme Corp"));
        assert!(html.contains("lower-third")); // class name
    }

    #[test]
    fn test_render_cta_card() {
        let registry = TemplateRegistry::new().unwrap();
        let content = r#"---
template: cta-card
props:
  heading: "Get Started Today"
  subheading: "Three easy steps"
  items:
    - "Sign up for free"
    - "Create your first project"
    - "Share with the world"
---
Voiceover."#;
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        let html = registry
            .render_scene_html(&scene, &test_theme(), 1920, 1080, 100, 150)
            .unwrap();
        assert!(html.contains("Get Started Today"));
        assert!(html.contains("Three easy steps"));
        assert!(html.contains("Sign up for free"));
        assert!(html.contains("Create your first project"));
        assert!(html.contains("Share with the world"));
    }

    #[test]
    fn test_render_kinetic_text() {
        let registry = TemplateRegistry::new().unwrap();
        let content =
            "---\ntemplate: kinetic-text\n---\nThe quick brown fox jumps over the lazy dog";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        let html = registry
            .render_scene_html(&scene, &test_theme(), 1920, 1080, 75, 150)
            .unwrap();
        // Each word should appear as an individual span
        assert!(html.contains(r#"<span class="word"#));
        assert!(html.contains(">The</span>"));
        assert!(html.contains(">quick</span>"));
        assert!(html.contains(">fox</span>"));
        assert!(html.contains(">dog</span>"));
        // total_words should be injected
        assert!(html.contains("--total-words: 9"));
    }

    #[test]
    fn test_render_kinetic_text_uses_text_prop() {
        let registry = TemplateRegistry::new().unwrap();
        let content = "---\ntemplate: kinetic-text\nprops:\n  text: \"Hello beautiful world\"\n---\nVoiceover.";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        let html = registry
            .render_scene_html(&scene, &test_theme(), 1920, 1080, 50, 150)
            .unwrap();
        // Should use the `text` prop over the script
        assert!(html.contains(">Hello</span>"));
        assert!(html.contains(">beautiful</span>"));
        assert!(html.contains(">world</span>"));
        assert!(html.contains("--total-words: 3"));
    }

    #[test]
    fn test_render_split_screen() {
        let registry = TemplateRegistry::new().unwrap();
        let content = r#"---
template: split-screen
props:
  panels:
    - label: "Before"
      content: "The old way of doing things"
    - label: "After"
      content: "The new, improved approach"
---
Voiceover."#;
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        let html = registry
            .render_scene_html(&scene, &test_theme(), 1920, 1080, 50, 150)
            .unwrap();
        assert!(html.contains("Before"));
        assert!(html.contains("The old way of doing things"));
        assert!(html.contains("After"));
        assert!(html.contains("The new, improved approach"));
        assert!(html.contains("panel-label")); // CSS class present
    }

    #[test]
    fn test_effective_background_default() {
        let registry = TemplateRegistry::new().unwrap();
        let content = "---\ntemplate: title-card\nprops:\n  title: \"Test\"\n---\nScript.";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        let theme = test_theme();
        let html = registry
            .render_scene_html(&scene, &theme, 1920, 1080, 0, 150)
            .unwrap();
        // Should use theme background when no scene-level override
        assert!(html.contains("#0F172A"));
    }

    #[test]
    fn test_effective_background_override() {
        let registry = TemplateRegistry::new().unwrap();
        let content = "---\ntemplate: title-card\nprops:\n  title: \"Test\"\nbackground:\n  color: \"#FF0000\"\n---\nScript.";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        let theme = test_theme();
        let html = registry
            .render_scene_html(&scene, &theme, 1920, 1080, 0, 150)
            .unwrap();
        // Should use the scene-level background override
        assert!(html.contains("#FF0000"));
        // Theme background should NOT appear in the body background
        // (it's still in the data as theme_background, but body uses effective_background)
    }

    #[test]
    fn test_register_project_templates() {
        let dir = tempfile::tempdir().unwrap();
        let components_dir = dir.path().join("templates").join("components");
        std::fs::create_dir_all(&components_dir).unwrap();

        // Write a custom template
        let custom_html = r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><style>
  body { width: {{width}}px; height: {{height}}px; background: {{effective_background}}; color: {{theme_text}}; }
</style></head>
<body><h1>{{custom_field}}</h1></body></html>"#;
        std::fs::write(components_dir.join("my-custom.html"), custom_html).unwrap();

        let mut registry = TemplateRegistry::new().unwrap();
        registry.register_project_templates(dir.path()).unwrap();

        let content =
            "---\ntemplate: my-custom\nprops:\n  custom_field: \"It works!\"\n---\nScript.";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        let html = registry
            .render_scene_html(&scene, &test_theme(), 1920, 1080, 0, 150)
            .unwrap();
        assert!(html.contains("It works!"));
    }

    #[test]
    fn test_project_template_overrides_builtin() {
        let dir = tempfile::tempdir().unwrap();
        let components_dir = dir.path().join("templates").join("components");
        std::fs::create_dir_all(&components_dir).unwrap();

        // Override the built-in "title-card" template with a custom one
        let custom_html = r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><style>
  body { width: {{width}}px; height: {{height}}px; background: {{effective_background}}; }
</style></head>
<body><div class="custom-override">{{title}}</div></body></html>"#;
        std::fs::write(components_dir.join("title-card.html"), custom_html).unwrap();

        let mut registry = TemplateRegistry::new().unwrap();
        registry.register_project_templates(dir.path()).unwrap();

        let content = "---\ntemplate: title-card\nprops:\n  title: \"Overridden!\"\n---\nScript.";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        let html = registry
            .render_scene_html(&scene, &test_theme(), 1920, 1080, 0, 150)
            .unwrap();
        // Should contain the custom override marker, not the built-in title-card content
        assert!(html.contains("custom-override"));
        assert!(html.contains("Overridden!"));
    }

    #[test]
    fn test_register_project_templates_no_dir() {
        let dir = tempfile::tempdir().unwrap();
        // No templates/components/ directory exists
        let mut registry = TemplateRegistry::new().unwrap();
        // Should not error
        registry.register_project_templates(dir.path()).unwrap();
    }

    #[test]
    fn test_render_slideshow() {
        let registry = TemplateRegistry::new().unwrap();
        let content = r#"---
template: slideshow
props:
  slides:
    - heading: "Slide One"
      body: "First slide content"
    - heading: "Slide Two"
      body: "Second slide content"
    - heading: "Slide Three"
      body: "Third slide content"
---
Voiceover."#;
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        let html = registry
            .render_scene_html(&scene, &test_theme(), 1920, 1080, 0, 150)
            .unwrap();
        assert!(html.contains("Slide One"));
        assert!(html.contains("First slide content"));
        assert!(html.contains("Slide Two"));
        assert!(html.contains("Third slide content"));
        assert!(html.contains("--total-slides: 3"));
        assert!(html.contains("slide-heading"));
    }

    #[test]
    fn test_render_slideshow_single_slide() {
        let registry = TemplateRegistry::new().unwrap();
        let content = r#"---
template: slideshow
props:
  slides:
    - heading: "Only Slide"
      body: "Solo content"
---
Voiceover."#;
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        let html = registry
            .render_scene_html(&scene, &test_theme(), 1920, 1080, 75, 150)
            .unwrap();
        assert!(html.contains("Only Slide"));
        assert!(html.contains("Solo content"));
        assert!(html.contains("--total-slides: 1"));
    }

    #[test]
    fn test_missing_template() {
        let registry = TemplateRegistry::new().unwrap();
        let content = "---\ntemplate: nonexistent\n---\nText.";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        let result = registry.render_scene_html(&scene, &test_theme(), 1920, 1080, 0, 150);
        assert!(result.is_err());
        if let Err(VidgenError::TemplateNotFound(name)) = result {
            assert_eq!(name, "nonexistent");
        } else {
            panic!("Expected TemplateNotFound error");
        }
    }

    #[test]
    fn test_template_contains_container_query() {
        let templates = [
            ("title-card", TITLE_CARD_TEMPLATE),
            ("split-screen", SPLIT_SCREEN_TEMPLATE),
            ("kinetic-text", KINETIC_TEXT_TEMPLATE),
            ("quote-card", QUOTE_CARD_TEMPLATE),
            ("lower-third", LOWER_THIRD_TEMPLATE),
            ("cta-card", CTA_CARD_TEMPLATE),
            ("content-text", CONTENT_TEXT_TEMPLATE),
            ("slideshow", SLIDESHOW_TEMPLATE),
            ("caption-overlay", CAPTION_OVERLAY_TEMPLATE),
        ];
        for (name, src) in templates {
            assert!(
                src.contains("container-type: size"),
                "Template {name} missing 'container-type: size'"
            );
            assert!(
                src.contains("@container"),
                "Template {name} missing '@container' query"
            );
            assert!(
                src.contains("scene-container"),
                "Template {name} missing '.scene-container' wrapper"
            );
        }
    }

    #[test]
    fn test_render_caption_overlay() {
        let registry = TemplateRegistry::new().unwrap();
        let content = "---\ntemplate: caption-overlay\nprops:\n  text: \"Hello beautiful world\"\n  style: background-box\n  position: top\n---\nVoiceover.";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        let html = registry
            .render_scene_html(&scene, &test_theme(), 1920, 1080, 50, 150)
            .unwrap();
        assert!(html.contains(">Hello</span>"));
        assert!(html.contains(">beautiful</span>"));
        assert!(html.contains(">world</span>"));
        assert!(html.contains("style-background-box"));
        assert!(html.contains("top"));
    }

    #[test]
    fn test_render_caption_overlay_script_fallback() {
        let registry = TemplateRegistry::new().unwrap();
        let content = "---\ntemplate: caption-overlay\n---\nThe quick brown fox";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        let html = registry
            .render_scene_html(&scene, &test_theme(), 1920, 1080, 50, 150)
            .unwrap();
        // Falls back to script text
        assert!(html.contains(">The</span>"));
        assert!(html.contains(">quick</span>"));
        assert!(html.contains(">brown</span>"));
        assert!(html.contains(">fox</span>"));
        // Default style and position
        assert!(html.contains("style-outline"));
        assert!(html.contains("bottom"));
    }

    #[test]
    fn test_render_kinetic_text_bounce_style() {
        let registry = TemplateRegistry::new().unwrap();
        let content = "---\ntemplate: kinetic-text\nprops:\n  style: bounce\n---\nWord one two";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        let html = registry
            .render_scene_html(&scene, &test_theme(), 1920, 1080, 50, 150)
            .unwrap();
        assert!(html.contains("bounce"));
    }

    #[test]
    fn test_render_lower_third_accent_color() {
        let registry = TemplateRegistry::new().unwrap();
        let content = "---\ntemplate: lower-third\nprops:\n  name: \"Jane\"\n  accent_color: \"#FF5500\"\n---\nVoiceover.";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        let html = registry
            .render_scene_html(&scene, &test_theme(), 1920, 1080, 30, 150)
            .unwrap();
        assert!(html.contains("#FF5500"));
    }

    #[test]
    fn test_render_lower_third_default_accent_color() {
        let registry = TemplateRegistry::new().unwrap();
        let content =
            "---\ntemplate: lower-third\nprops:\n  name: \"Jane\"\n---\nVoiceover.";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        let theme = test_theme();
        let html = registry
            .render_scene_html(&scene, &theme, 1920, 1080, 30, 150)
            .unwrap();
        // Should use theme primary as default accent_color
        assert!(html.contains(&theme.primary));
    }
}
