use crate::error::{VidgenError, VidgenResult};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// Scene duration: either automatically derived from TTS audio length + padding,
/// or a fixed number of seconds.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum SceneDuration {
    /// Duration derived from TTS audio length + padding (PRD default).
    #[default]
    Auto,
    /// Explicit duration in seconds.
    Fixed(f64),
}

impl SceneDuration {
    /// Resolve the effective duration in seconds.
    ///
    /// - `Auto` with TTS: `tts_duration + padding_before + padding_after`
    /// - `Auto` without TTS: `fallback`
    /// - `Fixed(d)`: `d`
    pub fn resolve(
        &self,
        tts_duration: Option<f64>,
        padding_before: f64,
        padding_after: f64,
        fallback: f64,
    ) -> f64 {
        match self {
            SceneDuration::Auto => match tts_duration {
                Some(d) => d + padding_before + padding_after,
                None => fallback,
            },
            SceneDuration::Fixed(d) => *d,
        }
    }

    pub fn is_auto(&self) -> bool {
        matches!(self, SceneDuration::Auto)
    }

    #[allow(dead_code)]
    pub fn as_fixed(&self) -> Option<f64> {
        match self {
            SceneDuration::Fixed(d) => Some(*d),
            SceneDuration::Auto => None,
        }
    }
}

impl Serialize for SceneDuration {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            SceneDuration::Auto => serializer.serialize_str("auto"),
            SceneDuration::Fixed(d) => serializer.serialize_f64(*d),
        }
    }
}

impl<'de> Deserialize<'de> for SceneDuration {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SceneDurationVisitor;

        impl<'de> Visitor<'de> for SceneDurationVisitor {
            type Value = SceneDuration;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("\"auto\" or a number (integer or float)")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<SceneDuration, E> {
                if value.eq_ignore_ascii_case("auto") {
                    Ok(SceneDuration::Auto)
                } else if let Some(num_str) = value.strip_suffix('s') {
                    // Support "5s" or "2.5s" suffix notation
                    num_str
                        .trim()
                        .parse::<f64>()
                        .map(SceneDuration::Fixed)
                        .map_err(|_| de::Error::invalid_value(de::Unexpected::Str(value), &self))
                } else {
                    // Try parsing as a bare number
                    value
                        .parse::<f64>()
                        .map(SceneDuration::Fixed)
                        .map_err(|_| de::Error::invalid_value(de::Unexpected::Str(value), &self))
                }
            }

            fn visit_f64<E: de::Error>(self, value: f64) -> Result<SceneDuration, E> {
                Ok(SceneDuration::Fixed(value))
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<SceneDuration, E> {
                Ok(SceneDuration::Fixed(value as f64))
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<SceneDuration, E> {
                Ok(SceneDuration::Fixed(value as f64))
            }
        }

        deserializer.deserialize_any(SceneDurationVisitor)
    }
}

impl schemars::JsonSchema for SceneDuration {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "SceneDuration".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "oneOf": [
                { "type": "string", "enum": ["auto"] },
                { "type": "number" }
            ],
            "description": "Scene duration: \"auto\" (derive from TTS audio + padding) or a number in seconds"
        }))
        .unwrap()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SceneAudioConfig {
    /// Path to a background music file (supports @assets/ prefix)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub music: Option<String>,
    /// Music volume from 0.0 to 1.0 (default 0.3)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub music_volume: Option<f64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SceneFrontmatter {
    pub template: String,
    #[serde(default)]
    pub duration: SceneDuration,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub props: HashMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<BackgroundConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition_in: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition_out: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition_duration: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<SceneAudioConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format_overrides: Option<HashMap<String, FormatOverride>>,
}

/// Per-format overrides that can be applied to a scene when rendering a specific format.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct FormatOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub props: Option<HashMap<String, serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<BackgroundConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BackgroundConfig {
    pub color: Option<String>,
    pub image: Option<String>,
}

#[derive(Debug)]
pub struct Scene {
    pub frontmatter: SceneFrontmatter,
    pub script: String,
    pub source_path: PathBuf,
}

impl Scene {
    /// Compute total frames for a given effective duration (in seconds).
    pub fn total_frames_for_duration(effective_duration: f64, fps: u32) -> u32 {
        (effective_duration * fps as f64).ceil() as u32
    }

    /// Compute total frames using the scene's own duration.
    /// For `Auto` duration, uses `fallback` seconds (for preview context without TTS).
    pub fn total_frames(&self, fps: u32) -> u32 {
        let effective = match &self.frontmatter.duration {
            SceneDuration::Fixed(d) => *d,
            SceneDuration::Auto => 3.0, // preview fallback
        };
        Self::total_frames_for_duration(effective, fps)
    }
}

/// Split a markdown file into YAML frontmatter and body text.
/// Expects `---` delimiters.
fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }
    // Skip the opening ---
    let after_open = &trimmed[3..];
    let close_pos = after_open.find("\n---")?;
    let yaml = &after_open[..close_pos];
    let body = &after_open[close_pos + 4..]; // skip \n---
                                             // Strip leading newline from body
    let body = body.strip_prefix('\n').unwrap_or(body);
    Some((yaml.trim(), body.trim()))
}

pub fn parse_scene(content: &str, path: &Path) -> VidgenResult<Scene> {
    let (yaml, body) = split_frontmatter(content).ok_or_else(|| VidgenError::SceneParse {
        path: path.to_path_buf(),
        message: "Missing YAML frontmatter (expected --- delimiters)".into(),
    })?;

    let frontmatter: SceneFrontmatter =
        serde_yml::from_str(yaml).map_err(|e| VidgenError::SceneParse {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;

    Ok(Scene {
        frontmatter,
        script: body.to_string(),
        source_path: path.to_path_buf(),
    })
}

/// Write a scene back to a markdown file (frontmatter + script body).
pub fn write_scene(scene: &Scene, path: &Path) -> VidgenResult<()> {
    let yaml = serde_yml::to_string(&scene.frontmatter).map_err(|e| VidgenError::SceneParse {
        path: path.to_path_buf(),
        message: format!("Failed to serialize frontmatter: {e}"),
    })?;
    let content = format!("---\n{}---\n\n{}\n", yaml, scene.script);
    std::fs::write(path, content)?;
    Ok(())
}

/// Load all scenes from a project's scenes/ directory, sorted by filename.
pub fn load_scenes(project_path: &Path) -> VidgenResult<Vec<Scene>> {
    let scenes_dir = project_path.join("scenes");
    if !scenes_dir.exists() {
        return Err(VidgenError::NoScenes(scenes_dir));
    }

    let mut entries: Vec<PathBuf> = std::fs::read_dir(&scenes_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "md"))
        .collect();

    entries.sort();

    if entries.is_empty() {
        return Err(VidgenError::NoScenes(scenes_dir));
    }

    let mut scenes = Vec::new();
    for path in entries {
        let content = std::fs::read_to_string(&path)?;
        scenes.push(parse_scene(&content, &path)?);
    }
    Ok(scenes)
}

/// Check if a string looks like an HTTP/HTTPS URL.
pub fn is_url(raw: &str) -> bool {
    raw.starts_with("http://") || raw.starts_with("https://")
}

/// Compute a deterministic SHA-256 cache key for a URL.
pub fn url_cache_key(url: &str) -> String {
    let digest = Sha256::digest(url.as_bytes());
    let mut s = String::with_capacity(digest.len() * 2);
    for b in &digest {
        use std::fmt::Write;
        write!(s, "{b:02x}").unwrap();
    }
    s
}

/// Extract file extension from a URL, defaulting to `.bin`.
fn url_extension(url: &str) -> &str {
    // Strip query string and fragment
    let path = url.split('?').next().unwrap_or(url);
    let path = path.split('#').next().unwrap_or(path);
    if let Some(dot_pos) = path.rfind('.') {
        let ext = &path[dot_pos + 1..];
        // Only return short, reasonable extensions
        if ext.len() <= 10 && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
            return ext;
        }
    }
    "bin"
}

/// Download a URL to a cached location. Returns the local path.
/// Uses SHA-256 hash of the URL as filename, preserving the original extension.
pub fn download_asset(url: &str, project_path: &Path) -> VidgenResult<PathBuf> {
    let hash = url_cache_key(url);
    let ext = url_extension(url);
    let download_dir = project_path.join("assets/downloads");
    std::fs::create_dir_all(&download_dir)?;
    let target = download_dir.join(format!("{hash}.{ext}"));

    // Cache hit
    if target.exists() {
        return Ok(target);
    }

    // Download
    let response = ureq::get(url)
        .call()
        .map_err(|e| VidgenError::Other(format!("Failed to download asset {url}: {e}")))?;

    let mut reader = response.into_body().into_reader();
    let mut file = std::fs::File::create(&target)?;
    std::io::copy(&mut reader, &mut file)?;

    Ok(target)
}

/// Resolve an asset path reference.
///
/// - `@assets/...` → `project_path/assets/...`
/// - `http://` or `https://` → download and cache in `assets/downloads/`
/// - Anything else → treated as relative to `project_path`
pub fn resolve_asset_path(raw: &str, project_path: &Path) -> PathBuf {
    if let Some(suffix) = raw.strip_prefix("@assets/") {
        project_path.join("assets").join(suffix)
    } else if is_url(raw) {
        match download_asset(raw, project_path) {
            Ok(path) => path,
            Err(e) => {
                eprintln!("Warning: failed to download asset {raw}: {e}");
                project_path.join(raw)
            }
        }
    } else {
        project_path.join(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_frontmatter_basic() {
        let content = "---\ntemplate: title-card\n---\nHello world";
        let (yaml, body) = split_frontmatter(content).unwrap();
        assert_eq!(yaml, "template: title-card");
        assert_eq!(body, "Hello world");
    }

    #[test]
    fn test_split_frontmatter_missing() {
        let content = "No frontmatter here";
        assert!(split_frontmatter(content).is_none());
    }

    #[test]
    fn test_split_frontmatter_multiline() {
        let content = "---\ntemplate: title-card\nduration: 10\nprops:\n  title: Hello\n---\n\nBody text here.\n\nMore text.";
        let (yaml, body) = split_frontmatter(content).unwrap();
        assert!(yaml.contains("template: title-card"));
        assert!(yaml.contains("duration: 10"));
        assert!(body.contains("Body text here."));
        assert!(body.contains("More text."));
    }

    #[test]
    fn test_parse_scene_basic() {
        let content = "---\ntemplate: title-card\nduration: 5\nprops:\n  title: \"Welcome\"\n---\nScript text here.";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        assert_eq!(scene.frontmatter.template, "title-card");
        assert_eq!(scene.frontmatter.duration, SceneDuration::Fixed(5.0));
        assert_eq!(scene.script, "Script text here.");
        assert_eq!(
            scene.frontmatter.props.get("title").unwrap(),
            &serde_json::Value::String("Welcome".into())
        );
    }

    #[test]
    fn test_parse_scene_defaults() {
        let content = "---\ntemplate: content-text\n---\nJust a script.";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        assert_eq!(scene.frontmatter.duration, SceneDuration::Auto); // new default
        assert!(scene.frontmatter.props.is_empty());
    }

    #[test]
    fn test_parse_scene_duration_auto() {
        let content = "---\ntemplate: title-card\nduration: auto\n---\nScript.";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        assert_eq!(scene.frontmatter.duration, SceneDuration::Auto);
        assert!(scene.frontmatter.duration.is_auto());
    }

    #[test]
    fn test_parse_scene_duration_integer() {
        let content = "---\ntemplate: title-card\nduration: 10\n---\nScript.";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        assert_eq!(scene.frontmatter.duration, SceneDuration::Fixed(10.0));
    }

    #[test]
    fn test_parse_scene_duration_float() {
        let content = "---\ntemplate: title-card\nduration: 3.5\n---\nScript.";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        assert_eq!(scene.frontmatter.duration, SceneDuration::Fixed(3.5));
    }

    #[test]
    fn test_scene_duration_resolve_auto_with_tts() {
        let d = SceneDuration::Auto;
        let effective = d.resolve(Some(5.0), 0.5, 0.5, 3.0);
        assert!((effective - 6.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_scene_duration_resolve_auto_without_tts() {
        let d = SceneDuration::Auto;
        let effective = d.resolve(None, 0.5, 0.5, 3.0);
        assert!((effective - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_scene_duration_resolve_fixed() {
        let d = SceneDuration::Fixed(7.0);
        let effective = d.resolve(Some(5.0), 0.5, 0.5, 3.0);
        assert!((effective - 7.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_scene_roundtrip() {
        let content = "---\ntemplate: title-card\nduration: 5\nprops:\n  title: \"Hello\"\n---\n\nScript text.";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("roundtrip.md");
        write_scene(&scene, &path).unwrap();

        let reloaded_content = std::fs::read_to_string(&path).unwrap();
        let reloaded = parse_scene(&reloaded_content, &path).unwrap();
        assert_eq!(reloaded.frontmatter.template, "title-card");
        assert_eq!(reloaded.frontmatter.duration, SceneDuration::Fixed(5.0));
        assert_eq!(
            reloaded.frontmatter.props.get("title").unwrap(),
            &serde_json::Value::String("Hello".into())
        );
        assert_eq!(reloaded.script, "Script text.");
    }

    #[test]
    fn test_scene_roundtrip_auto_duration() {
        let content = "---\ntemplate: title-card\nduration: auto\nprops:\n  title: \"Hello\"\n---\n\nScript text.";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("roundtrip-auto.md");
        write_scene(&scene, &path).unwrap();

        let reloaded_content = std::fs::read_to_string(&path).unwrap();
        let reloaded = parse_scene(&reloaded_content, &path).unwrap();
        assert_eq!(reloaded.frontmatter.duration, SceneDuration::Auto);
    }

    #[test]
    fn test_new_fields_parse() {
        let content = "---\ntemplate: title-card\ntransition_in: fade\ntransition_out: slide\ntransition_duration: 0.75\nvoice: en_US-male\n---\nText.";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        assert_eq!(scene.frontmatter.transition_in.as_deref(), Some("fade"));
        assert_eq!(scene.frontmatter.transition_out.as_deref(), Some("slide"));
        assert_eq!(scene.frontmatter.transition_duration, Some(0.75));
        assert_eq!(scene.frontmatter.voice.as_deref(), Some("en_US-male"));
    }

    #[test]
    fn test_transition_duration_omitted() {
        let content = "---\ntemplate: title-card\n---\nText.";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        assert!(scene.frontmatter.transition_duration.is_none());
    }

    #[test]
    fn test_total_frames_fixed() {
        let content = "---\ntemplate: title-card\nduration: 3\n---\nText";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        assert_eq!(scene.total_frames(30), 90);
        assert_eq!(scene.total_frames(60), 180);
    }

    #[test]
    fn test_total_frames_auto_uses_fallback() {
        let content = "---\ntemplate: title-card\nduration: auto\n---\nText";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        // Auto uses 3.0s fallback in total_frames()
        assert_eq!(scene.total_frames(30), 90);
    }

    #[test]
    fn test_total_frames_for_duration() {
        assert_eq!(Scene::total_frames_for_duration(5.0, 30), 150);
        assert_eq!(Scene::total_frames_for_duration(2.5, 60), 150);
    }

    #[test]
    fn test_parse_scene_with_audio() {
        let content = r#"---
template: title-card
audio:
  music: "@assets/audio/bg.mp3"
  music_volume: 0.2
---
Script."#;
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        let audio = scene.frontmatter.audio.as_ref().unwrap();
        assert_eq!(audio.music.as_deref(), Some("@assets/audio/bg.mp3"));
        assert_eq!(audio.music_volume, Some(0.2));
    }

    #[test]
    fn test_parse_scene_format_overrides() {
        let content = r##"---
template: title-card
props:
  title: "Default Title"
format_overrides:
  portrait:
    props:
      title: "Portrait Title"
    background:
      color: "#112233"
---
Script."##;
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        let overrides = scene.frontmatter.format_overrides.as_ref().unwrap();
        assert!(overrides.contains_key("portrait"));
        let portrait = &overrides["portrait"];
        assert_eq!(
            portrait.props.as_ref().unwrap().get("title").unwrap(),
            &serde_json::Value::String("Portrait Title".into())
        );
        assert_eq!(
            portrait.background.as_ref().unwrap().color.as_deref(),
            Some("#112233")
        );
    }

    #[test]
    fn test_format_override_roundtrip() {
        let content = r#"---
template: title-card
props:
  title: "Hello"
format_overrides:
  square:
    props:
      title: "Square Hello"
---
Script."#;
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("override-roundtrip.md");
        write_scene(&scene, &path).unwrap();

        let reloaded = std::fs::read_to_string(&path).unwrap();
        let reloaded_scene = parse_scene(&reloaded, &path).unwrap();
        let overrides = reloaded_scene
            .frontmatter
            .format_overrides
            .as_ref()
            .unwrap();
        assert_eq!(
            overrides["square"]
                .props
                .as_ref()
                .unwrap()
                .get("title")
                .unwrap(),
            &serde_json::Value::String("Square Hello".into())
        );
    }

    #[test]
    fn test_parse_scene_duration_with_s_suffix() {
        let content = "---\ntemplate: title-card\nduration: 5s\n---\nScript.";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        assert_eq!(scene.frontmatter.duration, SceneDuration::Fixed(5.0));
    }

    #[test]
    fn test_parse_scene_duration_with_s_suffix_float() {
        let content = "---\ntemplate: title-card\nduration: 2.5s\n---\nScript.";
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        assert_eq!(scene.frontmatter.duration, SceneDuration::Fixed(2.5));
    }

    #[test]
    fn test_resolve_asset_path() {
        let project = Path::new("/projects/my-video");
        assert_eq!(
            resolve_asset_path("@assets/audio/bg.mp3", project),
            PathBuf::from("/projects/my-video/assets/audio/bg.mp3")
        );
        assert_eq!(
            resolve_asset_path("music/track.mp3", project),
            PathBuf::from("/projects/my-video/music/track.mp3")
        );
    }

    #[test]
    fn test_resolve_asset_path_url_detection() {
        assert!(is_url("http://example.com/image.png"));
        assert!(is_url("https://cdn.example.com/audio/track.mp3"));
        assert!(!is_url("@assets/audio/bg.mp3"));
        assert!(!is_url("music/track.mp3"));
        assert!(!is_url("relative/path.png"));
    }

    #[test]
    fn test_download_cache_key() {
        let key1 = url_cache_key("https://example.com/image.png");
        let key2 = url_cache_key("https://example.com/image.png");
        let key3 = url_cache_key("https://example.com/other.png");
        assert_eq!(key1, key2); // deterministic
        assert_ne!(key1, key3); // different URLs differ
        assert_eq!(key1.len(), 64); // SHA-256 hex is 64 chars
    }

    #[test]
    fn test_resolve_asset_path_backwards_compat() {
        let project = Path::new("/projects/test");
        // @assets/ prefix still works
        assert_eq!(
            resolve_asset_path("@assets/fonts/Inter.ttf", project),
            PathBuf::from("/projects/test/assets/fonts/Inter.ttf")
        );
        // Relative paths still work
        assert_eq!(
            resolve_asset_path("styles/main.css", project),
            PathBuf::from("/projects/test/styles/main.css")
        );
    }

    #[test]
    fn test_url_extension_extraction() {
        assert_eq!(url_extension("https://example.com/file.mp3"), "mp3");
        assert_eq!(url_extension("https://example.com/file.png?v=2"), "png");
        assert_eq!(url_extension("https://example.com/noext"), "bin");
        assert_eq!(
            url_extension("https://example.com/path/image.jpg#fragment"),
            "jpg"
        );
    }

    #[test]
    fn test_scene_audio_roundtrip() {
        let content = r#"---
template: title-card
audio:
  music: "@assets/audio/bg.mp3"
  music_volume: 0.5
---
Script text."#;
        let scene = parse_scene(content, Path::new("test.md")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audio-roundtrip.md");
        write_scene(&scene, &path).unwrap();

        let reloaded_content = std::fs::read_to_string(&path).unwrap();
        let reloaded = parse_scene(&reloaded_content, &path).unwrap();
        let audio = reloaded.frontmatter.audio.as_ref().unwrap();
        assert_eq!(audio.music.as_deref(), Some("@assets/audio/bg.mp3"));
        assert_eq!(audio.music_volume, Some(0.5));
    }
}
