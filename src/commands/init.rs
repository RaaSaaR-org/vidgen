use crate::error::{VidgenError, VidgenResult};
use crate::scene::SceneDuration;
use colored::*;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Options for programmatic project creation (used by MCP and CLI).
pub struct CreateProjectOptions {
    pub path: PathBuf,
    pub name: Option<String>,
    pub fps: Option<u32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub quality: Option<String>,
    pub voice: Option<String>,
    pub formats: Option<Vec<String>>,
    pub theme: Option<ThemeOverrides>,
    pub scenes: Option<Vec<SceneInput>>,
}

/// Optional theme overrides for project creation.
pub struct ThemeOverrides {
    pub primary: Option<String>,
    pub secondary: Option<String>,
    pub background: Option<String>,
    pub text: Option<String>,
    pub font_heading: Option<String>,
    pub font_body: Option<String>,
}

/// A single scene to create, provided inline.
pub struct SceneInput {
    pub template: Option<String>,
    pub script: String,
    pub duration: Option<SceneDuration>,
    pub props: Option<HashMap<String, serde_json::Value>>,
    pub transition: Option<String>,
    pub voice: Option<String>,
    pub background: Option<String>,
}

/// Structured result from project creation.
#[derive(Serialize)]
pub struct CreateProjectResult {
    pub project_path: String,
    pub name: String,
    pub scenes_created: usize,
    pub files: Vec<String>,
    pub status: String,
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

/// Programmatic project creation. Returns structured result.
pub fn create_project(opts: &CreateProjectOptions) -> VidgenResult<CreateProjectResult> {
    let path = &opts.path;

    if path.join("project.toml").exists() {
        return Err(VidgenError::AlreadyInitialized(path.to_path_buf()));
    }

    // Create directory structure
    std::fs::create_dir_all(path.join("scenes"))?;
    std::fs::create_dir_all(path.join("output"))?;
    std::fs::create_dir_all(path.join("assets/images"))?;
    std::fs::create_dir_all(path.join("assets/audio"))?;
    std::fs::create_dir_all(path.join("assets/fonts"))?;
    std::fs::create_dir_all(path.join("assets/downloads"))?;
    std::fs::create_dir_all(path.join("templates/components"))?;

    // Derive project name
    let project_name = opts
        .name
        .as_deref()
        .or_else(|| path.file_name().and_then(|n| n.to_str()))
        .unwrap_or("my-video");

    // Build project.toml with overrides
    let fps = opts.fps.unwrap_or(30);
    let width = opts.width.unwrap_or(1920);
    let height = opts.height.unwrap_or(1080);
    let quality = opts.quality.as_deref().unwrap_or("standard");

    let theme = opts.theme.as_ref();
    let primary = theme
        .and_then(|t| t.primary.as_deref())
        .unwrap_or("#2563EB");
    let secondary = theme
        .and_then(|t| t.secondary.as_deref())
        .unwrap_or("#7C3AED");
    let background = theme
        .and_then(|t| t.background.as_deref())
        .unwrap_or("#0F172A");
    let text_color = theme.and_then(|t| t.text.as_deref()).unwrap_or("#F8FAFC");
    let font_heading = theme
        .and_then(|t| t.font_heading.as_deref())
        .unwrap_or("Inter");
    let font_body = theme
        .and_then(|t| t.font_body.as_deref())
        .unwrap_or("Inter");

    // Build voice section with optional default_voice
    let voice_section = if let Some(ref voice) = opts.voice {
        format!(
            r##"[voice]
engine = "native"
default_voice = "{voice}"
padding_before = 0.5
padding_after = 0.5
auto_fallback_duration = 3.0"##
        )
    } else {
        r##"[voice]
engine = "native"
padding_before = 0.5
padding_after = 0.5
auto_fallback_duration = 3.0"##
            .to_string()
    };

    // Build formats sections if specified
    let formats_section = if let Some(ref format_names) = opts.formats {
        let mut sections = String::new();
        for name in format_names {
            let (fw, fh) = match name.as_str() {
                "landscape" => (1920, 1080),
                "portrait" => (1080, 1920),
                "square" => (1080, 1080),
                other => {
                    eprintln!(
                        "{} Unknown format \"{other}\", skipping",
                        "warning:".yellow().bold()
                    );
                    continue;
                }
            };
            sections.push_str(&format!(
                "\n[video.formats.{name}]\nwidth = {fw}\nheight = {fh}\n"
            ));
        }
        sections
    } else {
        String::new()
    };

    let project_toml = format!(
        r##"[project]
name = "{project_name}"
version = "1.0.0"

[video]
fps = {fps}
width = {width}
height = {height}
{formats_section}
{voice_section}

[theme]
primary = "{primary}"
secondary = "{secondary}"
background = "{background}"
text = "{text_color}"
font_heading = "{font_heading}"
font_body = "{font_body}"

[output]
directory = "./output"
quality = "{quality}"
"##
    );
    std::fs::write(path.join("project.toml"), project_toml)?;

    let mut files = vec!["project.toml".to_string()];
    let scenes_created;

    // Write scenes
    if let Some(scenes) = &opts.scenes {
        scenes_created = scenes.len();
        for (i, scene_input) in scenes.iter().enumerate() {
            let template = scene_input.template.as_deref().unwrap_or("title-card");
            let filename = format!("{:02}-{}.md", i + 1, template);

            let mut frontmatter = String::new();
            frontmatter.push_str(&format!("template: {template}\n"));
            if let Some(ref dur) = scene_input.duration {
                frontmatter.push_str(&format!("duration: {}\n", format_duration_yaml(dur)));
            }
            if let Some(ref transition) = scene_input.transition {
                frontmatter.push_str(&format!("transition_in: {transition}\n"));
            }
            if let Some(ref voice) = scene_input.voice {
                frontmatter.push_str(&format!("voice: {voice}\n"));
            }
            if let Some(ref bg) = scene_input.background {
                frontmatter.push_str(&format!("background:\n  color: \"{bg}\"\n"));
            }
            if let Some(props) = &scene_input.props {
                if !props.is_empty() {
                    frontmatter.push_str("props:\n");
                    // Use serde_yml to serialize props as YAML
                    let props_yaml = serde_yml::to_string(props).unwrap_or_default();
                    // Indent each line under props:
                    for line in props_yaml.lines() {
                        frontmatter.push_str(&format!("  {line}\n"));
                    }
                }
            }

            let scene_content = format!("---\n{frontmatter}---\n\n{}\n", scene_input.script);
            std::fs::write(path.join("scenes").join(&filename), &scene_content)?;
            files.push(format!("scenes/{filename}"));
        }
    } else {
        // Default scene — uses auto duration
        scenes_created = 1;
        let scene = "---\ntemplate: title-card\nduration: auto\nprops:\n  title: \"Welcome\"\n  subtitle: \"Created with vidgen\"\n---\n\nThis is the intro scene. Replace this text with your voiceover script.\n";
        std::fs::write(path.join("scenes/01-intro.md"), scene)?;
        files.push("scenes/01-intro.md".to_string());
    }

    // Write example custom component
    let example_component = r#"<!-- Custom component example.
     Use this as a starting point for your own templates.
     Reference via template: custom-example in scene frontmatter.
     Props: title, body -->
<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body {
    width: {{width}}px; height: {{height}}px;
    background: {{effective_background}};
    color: {{theme_text}};
    font-family: '{{theme_font_body}}', sans-serif;
    display: flex; align-items: center; justify-content: center;
  }
  .scene-container {
    container-type: size;
    width: 100%; height: 100%;
    display: flex; align-items: center; justify-content: center;
    padding: 8%;
  }
  .content { text-align: center; }
  .content h1 {
    font-family: '{{theme_font_heading}}', sans-serif;
    font-size: 4vw; margin-bottom: 1em;
    color: {{theme_primary}};
  }
  .content p { font-size: 2vw; line-height: 1.6; opacity: 0.9; }
  @container (aspect-ratio < 1) {
    .content h1 { font-size: 6vw; }
    .content p { font-size: 3.5vw; }
  }
</style>
</head>
<body>
<div class="scene-container">
  <div class="content">
    <h1>{{title}}</h1>
    <p>{{body}}</p>
  </div>
</div>
</body>
</html>"#;
    std::fs::write(
        path.join("templates/components/custom-example.html"),
        example_component,
    )?;
    files.push("templates/components/custom-example.html".to_string());

    // Write .gitignore
    let gitignore = "output/\nassets/voiceover/\nassets/downloads/\n.vidgen/\n.env\n";
    std::fs::write(path.join(".gitignore"), gitignore)?;
    files.push(".gitignore".to_string());

    Ok(CreateProjectResult {
        project_path: path.display().to_string(),
        name: project_name.to_string(),
        scenes_created,
        files,
        status: "created".to_string(),
    })
}

/// CLI entry point — delegates to `create_project()`.
pub fn run(path: &Path) -> VidgenResult<()> {
    let opts = CreateProjectOptions {
        path: path.to_path_buf(),
        name: None,
        fps: None,
        width: None,
        height: None,
        quality: None,
        voice: None,
        formats: None,
        theme: None,
        scenes: None,
    };
    let result = create_project(&opts)?;

    eprintln!(
        "{} Created project at {}",
        "done:".green().bold(),
        result.project_path
    );
    for file in &result.files {
        eprintln!("  {file}");
    }
    eprintln!();
    eprintln!("Next: edit scenes in {}, then run:", "scenes/".cyan());
    eprintln!("  vidgen render {}", path.display());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_project_default() {
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().join("test-project");
        let opts = CreateProjectOptions {
            path: project_path.clone(),
            name: None,
            fps: None,
            width: None,
            height: None,
            quality: None,
            voice: None,
            formats: None,
            theme: None,
            scenes: None,
        };
        let result = create_project(&opts).unwrap();
        assert_eq!(result.name, "test-project");
        assert_eq!(result.scenes_created, 1);
        assert!(result.files.contains(&"scenes/01-intro.md".to_string()));
        assert!(project_path.join("project.toml").exists());
        assert!(project_path.join("scenes/01-intro.md").exists());

        // Verify config is parseable
        let config = crate::config::load_config(&project_path).unwrap();
        assert_eq!(config.video.fps, 30);
        assert_eq!(config.video.width, 1920);

        // Verify default scene has auto duration
        let scenes = crate::scene::load_scenes(&project_path).unwrap();
        assert_eq!(scenes[0].frontmatter.duration, SceneDuration::Auto);
    }

    #[test]
    fn test_create_project_with_overrides() {
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().join("custom-project");
        let opts = CreateProjectOptions {
            path: project_path.clone(),
            name: Some("My Custom Video".to_string()),
            fps: Some(60),
            width: Some(3840),
            height: Some(2160),
            quality: Some("high".to_string()),
            voice: None,
            formats: None,
            theme: Some(ThemeOverrides {
                primary: Some("#FF0000".to_string()),
                secondary: None,
                background: None,
                text: None,
                font_heading: None,
                font_body: None,
            }),
            scenes: None,
        };
        let result = create_project(&opts).unwrap();
        assert_eq!(result.name, "My Custom Video");

        let config = crate::config::load_config(&project_path).unwrap();
        assert_eq!(config.video.fps, 60);
        assert_eq!(config.video.width, 3840);
        assert_eq!(config.video.height, 2160);
        assert_eq!(config.output.quality, "high");
        assert_eq!(config.theme.primary, "#FF0000");
        // Non-overridden values keep defaults
        assert_eq!(config.theme.secondary, "#7C3AED");
    }

    #[test]
    fn test_create_project_with_inline_scenes() {
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().join("scenes-project");
        let mut props = HashMap::new();
        props.insert(
            "title".to_string(),
            serde_json::Value::String("Hello World".to_string()),
        );
        let opts = CreateProjectOptions {
            path: project_path.clone(),
            name: None,
            fps: None,
            width: None,
            height: None,
            quality: None,
            voice: None,
            formats: None,
            theme: None,
            scenes: Some(vec![
                SceneInput {
                    template: Some("title-card".to_string()),
                    script: "Welcome to the show.".to_string(),
                    duration: Some(SceneDuration::Fixed(3.0)),
                    props: Some(props),
                    transition: None,
                    voice: None,
                    background: None,
                },
                SceneInput {
                    template: Some("content-text".to_string()),
                    script: "Here is some content.".to_string(),
                    duration: None,
                    props: None,
                    transition: None,
                    voice: None,
                    background: None,
                },
            ]),
        };
        let result = create_project(&opts).unwrap();
        assert_eq!(result.scenes_created, 2);
        assert!(project_path.join("scenes/01-title-card.md").exists());
        assert!(project_path.join("scenes/02-content-text.md").exists());

        // Verify scene is parseable
        let content =
            std::fs::read_to_string(project_path.join("scenes/01-title-card.md")).unwrap();
        let scene = crate::scene::parse_scene(&content, Path::new("test.md")).unwrap();
        assert_eq!(scene.frontmatter.template, "title-card");
        assert_eq!(scene.frontmatter.duration, SceneDuration::Fixed(3.0));
        assert_eq!(
            scene.frontmatter.props.get("title").unwrap(),
            &serde_json::Value::String("Hello World".to_string())
        );
    }

    #[test]
    fn test_init_creates_template_dir() {
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().join("template-test");
        let opts = CreateProjectOptions {
            path: project_path.clone(),
            name: None,
            fps: None,
            width: None,
            height: None,
            quality: None,
            voice: None,
            formats: None,
            theme: None,
            scenes: None,
        };
        create_project(&opts).unwrap();
        assert!(project_path.join("templates/components").is_dir());
        assert!(project_path
            .join("templates/components/custom-example.html")
            .exists());
        assert!(project_path.join("assets/downloads").is_dir());
        // Verify gitignore includes downloads
        let gitignore = std::fs::read_to_string(project_path.join(".gitignore")).unwrap();
        assert!(gitignore.contains("assets/downloads/"));
    }

    #[test]
    fn test_create_project_already_exists() {
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().join("existing");
        // Create first
        let opts = CreateProjectOptions {
            path: project_path.clone(),
            name: None,
            fps: None,
            width: None,
            height: None,
            quality: None,
            voice: None,
            formats: None,
            theme: None,
            scenes: None,
        };
        create_project(&opts).unwrap();

        // Try again — should fail
        let result = create_project(&opts);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_project_with_voice() {
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().join("voice-project");
        let opts = CreateProjectOptions {
            path: project_path.clone(),
            name: Some("Voice Test".to_string()),
            fps: None,
            width: None,
            height: None,
            quality: None,
            voice: Some("en-US-JennyNeural".to_string()),
            formats: None,
            theme: None,
            scenes: None,
        };
        create_project(&opts).unwrap();

        let toml_content =
            std::fs::read_to_string(project_path.join("project.toml")).unwrap();
        assert!(toml_content.contains(r#"default_voice = "en-US-JennyNeural""#));

        let config = crate::config::load_config(&project_path).unwrap();
        assert_eq!(
            config.voice.default_voice.as_deref(),
            Some("en-US-JennyNeural")
        );
    }

    #[test]
    fn test_create_project_with_formats() {
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().join("formats-project");
        let opts = CreateProjectOptions {
            path: project_path.clone(),
            name: Some("Format Test".to_string()),
            fps: None,
            width: None,
            height: None,
            quality: None,
            voice: None,
            formats: Some(vec![
                "landscape".to_string(),
                "portrait".to_string(),
                "square".to_string(),
            ]),
            theme: None,
            scenes: None,
        };
        create_project(&opts).unwrap();

        let config = crate::config::load_config(&project_path).unwrap();
        let formats = config.video.formats.expect("formats should be set");
        assert!(formats.contains_key("landscape"));
        assert!(formats.contains_key("portrait"));
        assert!(formats.contains_key("square"));
        assert_eq!(formats["landscape"].width, 1920);
        assert_eq!(formats["landscape"].height, 1080);
        assert_eq!(formats["portrait"].width, 1080);
        assert_eq!(formats["portrait"].height, 1920);
        assert_eq!(formats["square"].width, 1080);
        assert_eq!(formats["square"].height, 1080);
    }

    #[test]
    fn test_create_project_with_scene_transition_and_voice() {
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().join("transition-project");
        let opts = CreateProjectOptions {
            path: project_path.clone(),
            name: None,
            fps: None,
            width: None,
            height: None,
            quality: None,
            voice: None,
            formats: None,
            theme: None,
            scenes: Some(vec![SceneInput {
                template: Some("title-card".to_string()),
                script: "Hello.".to_string(),
                duration: None,
                props: None,
                transition: Some("fade".to_string()),
                voice: Some("en-US-GuyNeural".to_string()),
                background: Some("#FF0000".to_string()),
            }]),
        };
        create_project(&opts).unwrap();

        let scenes = crate::scene::load_scenes(&project_path).unwrap();
        assert_eq!(scenes[0].frontmatter.transition_in.as_deref(), Some("fade"));
        assert_eq!(
            scenes[0].frontmatter.voice.as_deref(),
            Some("en-US-GuyNeural")
        );
        assert_eq!(
            scenes[0]
                .frontmatter
                .background
                .as_ref()
                .and_then(|bg| bg.color.as_deref()),
            Some("#FF0000")
        );
    }
}
