use crate::error::{VidgenError, VidgenResult};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProjectConfig {
    pub project: ProjectInfo,
    #[serde(default)]
    pub video: VideoConfig,
    #[serde(default)]
    pub voice: VoiceConfig,
    #[serde(default)]
    pub theme: ThemeConfig,
    #[serde(default)]
    pub output: OutputConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProjectInfo {
    pub name: String,
    #[serde(default = "default_version")]
    pub version: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VideoConfig {
    #[serde(default = "default_fps")]
    pub fps: u32,
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default = "default_height")]
    pub height: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_transition: Option<String>,
    #[serde(default = "default_transition_duration")]
    pub default_transition_duration: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formats: Option<BTreeMap<String, FormatConfig>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel_scenes: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FormatConfig {
    pub width: u32,
    pub height: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Platform encoding preset name (e.g., "youtube-hd", "instagram-reels")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ThemeConfig {
    #[serde(default = "default_primary")]
    pub primary: String,
    #[serde(default = "default_secondary")]
    pub secondary: String,
    #[serde(default = "default_background")]
    pub background: String,
    #[serde(default = "default_text")]
    pub text: String,
    #[serde(default = "default_font")]
    pub font_heading: String,
    #[serde(default = "default_font")]
    pub font_body: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VoiceConfig {
    #[serde(default = "default_voice_engine")]
    pub engine: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_voice: Option<String>,
    #[serde(default = "default_voice_speed")]
    pub speed: f32,
    #[serde(default = "default_padding_before")]
    pub padding_before: f64,
    #[serde(default = "default_padding_after")]
    pub padding_after: f64,
    #[serde(default = "default_auto_fallback")]
    pub auto_fallback_duration: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OutputConfig {
    #[serde(default = "default_output_dir")]
    pub directory: String,
    #[serde(default = "default_quality")]
    pub quality: String,
    #[serde(default)]
    pub subtitles: SubtitleConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubtitleConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_max_words")]
    pub max_words_per_line: usize,
    /// Burn subtitles into the video via FFmpeg (post-process step)
    #[serde(default)]
    pub burn_in: bool,
}

fn default_max_words() -> usize {
    6
}

impl Default for SubtitleConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_words_per_line: default_max_words(),
            burn_in: false,
        }
    }
}

// Defaults
fn default_version() -> String {
    "1.0.0".into()
}
fn default_fps() -> u32 {
    30
}
fn default_width() -> u32 {
    1920
}
fn default_height() -> u32 {
    1080
}
fn default_primary() -> String {
    "#2563EB".into()
}
fn default_secondary() -> String {
    "#7C3AED".into()
}
fn default_background() -> String {
    "#0F172A".into()
}
fn default_text() -> String {
    "#F8FAFC".into()
}
fn default_font() -> String {
    "Inter".into()
}
fn default_transition_duration() -> f64 {
    0.5
}
fn default_voice_engine() -> String {
    "native".into()
}
fn default_voice_speed() -> f32 {
    1.0
}
fn default_padding_before() -> f64 {
    0.5
}
fn default_padding_after() -> f64 {
    0.5
}
fn default_auto_fallback() -> f64 {
    3.0
}
fn default_output_dir() -> String {
    "./output".into()
}
fn default_quality() -> String {
    "standard".into()
}

impl Default for VideoConfig {
    fn default() -> Self {
        Self {
            fps: default_fps(),
            width: default_width(),
            height: default_height(),
            default_transition: None,
            default_transition_duration: default_transition_duration(),
            formats: None,
            parallel_scenes: None,
        }
    }
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            primary: default_primary(),
            secondary: default_secondary(),
            background: default_background(),
            text: default_text(),
            font_heading: default_font(),
            font_body: default_font(),
        }
    }
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            engine: default_voice_engine(),
            default_voice: None,
            speed: default_voice_speed(),
            padding_before: default_padding_before(),
            padding_after: default_padding_after(),
            auto_fallback_duration: default_auto_fallback(),
        }
    }
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            directory: default_output_dir(),
            quality: default_quality(),
            subtitles: SubtitleConfig::default(),
        }
    }
}

/// Quality settings mapped from quality string
pub struct QualityPreset {
    pub crf: u32,
    pub preset: &'static str,
}

impl QualityPreset {
    pub fn from_name(name: &str) -> Self {
        match name {
            "draft" => Self {
                crf: 28,
                preset: "ultrafast",
            },
            "high" => Self {
                crf: 18,
                preset: "slow",
            },
            _ => Self {
                crf: 23,
                preset: "medium",
            }, // standard
        }
    }
}

/// Full encoding parameters including audio settings, resolved from platform or quality.
pub struct PlatformPreset {
    pub crf: u32,
    pub preset: &'static str,
    pub audio_bitrate: &'static str,
    pub audio_samplerate: u32,
}

impl PlatformPreset {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "youtube-hd" => Self {
                crf: 18,
                preset: "slow",
                audio_bitrate: "384k",
                audio_samplerate: 48000,
            },
            "youtube-4k" => Self {
                crf: 18,
                preset: "medium",
                audio_bitrate: "384k",
                audio_samplerate: 48000,
            },
            "instagram-reels" => Self {
                crf: 20,
                preset: "medium",
                audio_bitrate: "128k",
                audio_samplerate: 44100,
            },
            "tiktok" => Self {
                crf: 20,
                preset: "medium",
                audio_bitrate: "128k",
                audio_samplerate: 44100,
            },
            "whatsapp" => Self {
                crf: 26,
                preset: "fast",
                audio_bitrate: "96k",
                audio_samplerate: 44100,
            },
            "youtube-shorts" => Self {
                crf: 20,
                preset: "medium",
                audio_bitrate: "256k",
                audio_samplerate: 48000,
            },
            "twitter" => Self {
                crf: 22,
                preset: "medium",
                audio_bitrate: "128k",
                audio_samplerate: 44100,
            },
            _ => return None,
        })
    }

    pub fn from_quality(quality: &QualityPreset) -> Self {
        Self {
            crf: quality.crf,
            preset: quality.preset,
            audio_bitrate: "128k",
            audio_samplerate: 44100,
        }
    }
}

/// Resolve encoding parameters from quality preset + optional platform name.
/// If platform is given, uses platform defaults with quality CRF offset (draft=+6, high=-2).
pub fn resolve_encoding(quality: &QualityPreset, platform: Option<&str>) -> PlatformPreset {
    match platform.and_then(PlatformPreset::from_name) {
        Some(mut p) => {
            // Apply quality-based CRF offset relative to "standard" (crf=23)
            let offset = quality.crf as i32 - 23;
            p.crf = (p.crf as i32 + offset).max(1) as u32;
            p
        }
        None => PlatformPreset::from_quality(quality),
    }
}

/// All-optional struct for partial config updates.
pub struct ConfigUpdate {
    pub fps: Option<u32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub quality: Option<String>,
    pub primary: Option<String>,
    pub secondary: Option<String>,
    pub background: Option<String>,
    pub text: Option<String>,
    pub font_heading: Option<String>,
    pub font_body: Option<String>,
    pub default_transition: Option<String>,
    pub default_transition_duration: Option<f64>,
    pub voice_engine: Option<String>,
    pub default_voice: Option<String>,
    pub voice_speed: Option<f32>,
    pub padding_before: Option<f64>,
    pub padding_after: Option<f64>,
    pub auto_fallback_duration: Option<f64>,
    pub formats: Option<BTreeMap<String, FormatConfig>>,
}

/// Save a `ProjectConfig` to `project_path/project.toml`.
pub fn save_config(project_path: &Path, config: &ProjectConfig) -> VidgenResult<()> {
    let toml_str =
        toml::to_string_pretty(config).map_err(|e| VidgenError::ConfigParse(e.to_string()))?;
    std::fs::write(project_path.join("project.toml"), toml_str)?;
    Ok(())
}

/// Load config, apply non-None fields from `update`, save, and return updated config.
pub fn update_config(project_path: &Path, update: &ConfigUpdate) -> VidgenResult<ProjectConfig> {
    let mut config = load_config(project_path)?;

    if let Some(fps) = update.fps {
        config.video.fps = fps;
    }
    if let Some(width) = update.width {
        config.video.width = width;
    }
    if let Some(height) = update.height {
        config.video.height = height;
    }
    if let Some(ref quality) = update.quality {
        config.output.quality = quality.clone();
    }
    if let Some(ref primary) = update.primary {
        config.theme.primary = primary.clone();
    }
    if let Some(ref secondary) = update.secondary {
        config.theme.secondary = secondary.clone();
    }
    if let Some(ref background) = update.background {
        config.theme.background = background.clone();
    }
    if let Some(ref text) = update.text {
        config.theme.text = text.clone();
    }
    if let Some(ref font_heading) = update.font_heading {
        config.theme.font_heading = font_heading.clone();
    }
    if let Some(ref font_body) = update.font_body {
        config.theme.font_body = font_body.clone();
    }
    if let Some(ref default_transition) = update.default_transition {
        config.video.default_transition = Some(default_transition.clone());
    }
    if let Some(duration) = update.default_transition_duration {
        config.video.default_transition_duration = duration;
    }
    if let Some(ref engine) = update.voice_engine {
        config.voice.engine = engine.clone();
    }
    if let Some(ref voice) = update.default_voice {
        config.voice.default_voice = Some(voice.clone());
    }
    if let Some(speed) = update.voice_speed {
        config.voice.speed = speed;
    }
    if let Some(padding) = update.padding_before {
        config.voice.padding_before = padding;
    }
    if let Some(padding) = update.padding_after {
        config.voice.padding_after = padding;
    }
    if let Some(fallback) = update.auto_fallback_duration {
        config.voice.auto_fallback_duration = fallback;
    }
    if let Some(ref formats) = update.formats {
        config.video.formats = Some(formats.clone());
    }

    save_config(project_path, &config)?;
    Ok(config)
}

pub fn load_config(project_path: &Path) -> VidgenResult<ProjectConfig> {
    let config_path = project_path.join("project.toml");
    if !config_path.exists() {
        return Err(VidgenError::ConfigNotFound(config_path));
    }
    let content = std::fs::read_to_string(&config_path)?;
    toml::from_str(&content).map_err(|e| VidgenError::ConfigParse(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_full_config() {
        let toml = r##"
[project]
name = "Test Video"
version = "1.0.0"

[video]
fps = 60
width = 3840
height = 2160

[theme]
primary = "#FF0000"
secondary = "#00FF00"
background = "#000000"
text = "#FFFFFF"
font_heading = "Roboto"
font_body = "Roboto"

[output]
directory = "./out"
quality = "high"
"##;
        let config: ProjectConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.project.name, "Test Video");
        assert_eq!(config.video.fps, 60);
        assert_eq!(config.video.width, 3840);
        assert_eq!(config.theme.primary, "#FF0000");
        assert_eq!(config.output.quality, "high");
    }

    #[test]
    fn test_parse_minimal_config() {
        let toml = r#"
[project]
name = "Minimal"
"#;
        let config: ProjectConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.project.name, "Minimal");
        assert_eq!(config.video.fps, 30);
        assert_eq!(config.video.width, 1920);
        assert_eq!(config.theme.primary, "#2563EB");
        assert_eq!(config.output.quality, "standard");
    }

    #[test]
    fn test_parse_invalid_toml() {
        let toml = "not valid toml [[[";
        let result = toml::from_str::<ProjectConfig>(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path();
        let config = ProjectConfig {
            project: ProjectInfo {
                name: "Roundtrip Test".into(),
                version: "1.0.0".into(),
            },
            video: VideoConfig {
                fps: 60,
                width: 3840,
                height: 2160,
                ..Default::default()
            },
            voice: VoiceConfig {
                engine: "native".into(),
                default_voice: Some("Samantha".into()),
                speed: 1.2,
                ..Default::default()
            },
            theme: ThemeConfig::default(),
            output: OutputConfig::default(),
        };
        save_config(project_path, &config).unwrap();
        let loaded = load_config(project_path).unwrap();
        assert_eq!(loaded.project.name, "Roundtrip Test");
        assert_eq!(loaded.video.fps, 60);
        assert_eq!(loaded.video.width, 3840);
        assert_eq!(loaded.voice.engine, "native");
        assert_eq!(loaded.voice.default_voice.as_deref(), Some("Samantha"));
        assert!((loaded.voice.speed - 1.2).abs() < f32::EPSILON);
    }

    #[test]
    fn test_update_config() {
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path();
        let config = ProjectConfig {
            project: ProjectInfo {
                name: "Update Test".into(),
                version: "1.0.0".into(),
            },
            video: VideoConfig::default(),
            voice: VoiceConfig::default(),
            theme: ThemeConfig::default(),
            output: OutputConfig::default(),
        };
        save_config(project_path, &config).unwrap();

        let update = ConfigUpdate {
            fps: Some(60),
            width: None,
            height: None,
            quality: Some("high".into()),
            primary: Some("#FF0000".into()),
            secondary: None,
            background: None,
            text: None,
            font_heading: None,
            font_body: None,
            default_transition: None,
            default_transition_duration: None,
            voice_engine: Some("native".into()),
            default_voice: Some("Alex".into()),
            voice_speed: None,
            padding_before: None,
            padding_after: None,
            auto_fallback_duration: None,
            formats: None,
        };
        let updated = update_config(project_path, &update).unwrap();
        assert_eq!(updated.video.fps, 60);
        assert_eq!(updated.video.width, 1920); // unchanged
        assert_eq!(updated.output.quality, "high");
        assert_eq!(updated.theme.primary, "#FF0000");
        assert_eq!(updated.theme.secondary, "#7C3AED"); // unchanged
        assert_eq!(updated.voice.engine, "native");
        assert_eq!(updated.voice.default_voice.as_deref(), Some("Alex"));
        assert!((updated.voice.speed - 1.0).abs() < f32::EPSILON); // unchanged
    }

    #[test]
    fn test_parse_config_with_transitions() {
        let toml = r##"
[project]
name = "Transition Test"

[video]
fps = 30
width = 1920
height = 1080
default_transition = "fade"
default_transition_duration = 0.75
"##;
        let config: ProjectConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.video.default_transition.as_deref(), Some("fade"));
        assert!((config.video.default_transition_duration - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_config_without_transitions_backward_compat() {
        let toml = r#"
[project]
name = "No Transitions"

[video]
fps = 30
"#;
        let config: ProjectConfig = toml::from_str(toml).unwrap();
        assert!(config.video.default_transition.is_none());
        assert!((config.video.default_transition_duration - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_update_config_transitions() {
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path();
        let config = ProjectConfig {
            project: ProjectInfo {
                name: "Trans Update".into(),
                version: "1.0.0".into(),
            },
            video: VideoConfig::default(),
            voice: VoiceConfig::default(),
            theme: ThemeConfig::default(),
            output: OutputConfig::default(),
        };
        save_config(project_path, &config).unwrap();

        let update = ConfigUpdate {
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
            default_transition: Some("slide-left".into()),
            default_transition_duration: Some(1.0),
            voice_engine: None,
            default_voice: None,
            voice_speed: None,
            padding_before: None,
            padding_after: None,
            auto_fallback_duration: None,
            formats: None,
        };
        let updated = update_config(project_path, &update).unwrap();
        assert_eq!(
            updated.video.default_transition.as_deref(),
            Some("slide-left")
        );
        assert!((updated.video.default_transition_duration - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_quality_presets() {
        let draft = QualityPreset::from_name("draft");
        assert_eq!(draft.crf, 28);
        assert_eq!(draft.preset, "ultrafast");

        let high = QualityPreset::from_name("high");
        assert_eq!(high.crf, 18);
        assert_eq!(high.preset, "slow");

        let standard = QualityPreset::from_name("standard");
        assert_eq!(standard.crf, 23);
    }

    #[test]
    fn test_parse_config_with_formats() {
        let toml = r##"
[project]
name = "Multi-format Test"

[video]
fps = 30
width = 1920
height = 1080

[video.formats.landscape]
width = 1920
height = 1080
label = "YouTube"

[video.formats.portrait]
width = 1080
height = 1920
label = "Reels"
platform = "instagram-reels"
"##;
        let config: ProjectConfig = toml::from_str(toml).unwrap();
        let formats = config.video.formats.as_ref().unwrap();
        assert_eq!(formats.len(), 2);

        let landscape = &formats["landscape"];
        assert_eq!(landscape.width, 1920);
        assert_eq!(landscape.height, 1080);
        assert_eq!(landscape.label.as_deref(), Some("YouTube"));
        assert!(landscape.platform.is_none());

        let portrait = &formats["portrait"];
        assert_eq!(portrait.width, 1080);
        assert_eq!(portrait.height, 1920);
        assert_eq!(portrait.label.as_deref(), Some("Reels"));
        assert_eq!(portrait.platform.as_deref(), Some("instagram-reels"));
    }

    #[test]
    fn test_parse_config_without_formats_backward_compat() {
        let toml = r#"
[project]
name = "No Formats"

[video]
fps = 30
width = 1920
height = 1080
"#;
        let config: ProjectConfig = toml::from_str(toml).unwrap();
        assert!(config.video.formats.is_none());
        assert_eq!(config.video.width, 1920);
        assert_eq!(config.video.height, 1080);
    }

    #[test]
    fn test_platform_preset_from_name() {
        let names = [
            ("youtube-hd", 18, "slow", "384k", 48000),
            ("youtube-4k", 18, "medium", "384k", 48000),
            ("instagram-reels", 20, "medium", "128k", 44100),
            ("tiktok", 20, "medium", "128k", 44100),
            ("whatsapp", 26, "fast", "96k", 44100),
            ("youtube-shorts", 20, "medium", "256k", 48000),
            ("twitter", 22, "medium", "128k", 44100),
        ];
        for (name, crf, preset, bitrate, samplerate) in names {
            let p = PlatformPreset::from_name(name).unwrap();
            assert_eq!(p.crf, crf, "CRF mismatch for {name}");
            assert_eq!(p.preset, preset, "preset mismatch for {name}");
            assert_eq!(p.audio_bitrate, bitrate, "bitrate mismatch for {name}");
            assert_eq!(
                p.audio_samplerate, samplerate,
                "samplerate mismatch for {name}"
            );
        }
        assert!(PlatformPreset::from_name("unknown").is_none());
    }

    #[test]
    fn test_parse_subtitle_config() {
        let toml = r##"
[project]
name = "Subtitle Test"

[output]
directory = "./output"
quality = "standard"

[output.subtitles]
enabled = true
max_words_per_line = 8
"##;
        let config: ProjectConfig = toml::from_str(toml).unwrap();
        assert!(config.output.subtitles.enabled);
        assert_eq!(config.output.subtitles.max_words_per_line, 8);
    }

    #[test]
    fn test_subtitle_config_defaults() {
        let toml = r#"
[project]
name = "No Subtitles"
"#;
        let config: ProjectConfig = toml::from_str(toml).unwrap();
        assert!(!config.output.subtitles.enabled);
        assert_eq!(config.output.subtitles.max_words_per_line, 6);
    }

    #[test]
    fn test_subtitle_burn_in_config() {
        let toml = r##"
[project]
name = "Burn-in Test"

[output.subtitles]
enabled = true
max_words_per_line = 8
burn_in = true
"##;
        let config: ProjectConfig = toml::from_str(toml).unwrap();
        assert!(config.output.subtitles.enabled);
        assert!(config.output.subtitles.burn_in);
        assert_eq!(config.output.subtitles.max_words_per_line, 8);
    }

    #[test]
    fn test_subtitle_burn_in_default_false() {
        let toml = r##"
[project]
name = "No Burn-in"

[output.subtitles]
enabled = true
"##;
        let config: ProjectConfig = toml::from_str(toml).unwrap();
        assert!(config.output.subtitles.enabled);
        assert!(!config.output.subtitles.burn_in);
    }

    #[test]
    fn test_parse_parallel_scenes_config() {
        let toml = r##"
[project]
name = "Parallel Test"

[video]
fps = 30
parallel_scenes = 4
"##;
        let config: ProjectConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.video.parallel_scenes, Some(4));
    }

    #[test]
    fn test_parallel_scenes_default() {
        let toml = r#"
[project]
name = "No Parallel"
"#;
        let config: ProjectConfig = toml::from_str(toml).unwrap();
        assert!(config.video.parallel_scenes.is_none());
    }

    #[test]
    fn test_platform_preset_quality_offset() {
        let standard = QualityPreset::from_name("standard");
        let p = resolve_encoding(&standard, Some("youtube-hd"));
        assert_eq!(p.crf, 18); // no offset for standard

        let draft = QualityPreset::from_name("draft");
        let p = resolve_encoding(&draft, Some("youtube-hd"));
        assert_eq!(p.crf, 23); // 18 + (28-23) = 23

        let high = QualityPreset::from_name("high");
        let p = resolve_encoding(&high, Some("youtube-hd"));
        assert_eq!(p.crf, 13); // 18 + (18-23) = 13

        // No platform: uses quality directly
        let p = resolve_encoding(&standard, None);
        assert_eq!(p.crf, 23);
        assert_eq!(p.audio_bitrate, "128k");
    }
}
