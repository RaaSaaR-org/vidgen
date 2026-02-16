pub mod cache;
pub mod edge;
pub mod elevenlabs;
pub mod native;
pub mod timestamps;

use crate::config::VoiceConfig;
use crate::error::{VidgenError, VidgenResult};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Result of a TTS synthesis call.
#[derive(Debug)]
pub struct SynthesisResult {
    pub audio_path: PathBuf,
    pub duration_secs: f64,
    pub cached: bool,
    /// Word-level timestamps, if the engine provides them.
    /// When `None`, the caller can use `timestamps::estimate_word_timestamps()` as a fallback.
    #[allow(dead_code)]
    pub word_timestamps: Option<Vec<timestamps::WordTimestamp>>,
}

/// Voice metadata returned by `list_voices()`.
#[derive(Debug, Serialize)]
pub struct VoiceInfo {
    pub id: String,
    pub name: String,
    pub language: String,
    pub gender: String,
    pub engine: String,
    pub available: bool,
    pub note: Option<String>,
}

/// Trait for pluggable TTS backends.
///
/// Implementations are synchronous — subprocess calls are blocking but short,
/// and synthesis runs before browser launch so nothing is starved.
pub trait TtsEngine: Send + Sync {
    fn synthesize(
        &self,
        text: &str,
        voice: Option<&str>,
        speed: f32,
        output_path: &Path,
    ) -> VidgenResult<SynthesisResult>;

    fn list_voices(&self) -> VidgenResult<Vec<VoiceInfo>>;

    fn engine_name(&self) -> &str;
}

/// Factory: create a TTS engine from project voice config.
pub fn create_engine(config: &VoiceConfig) -> VidgenResult<Box<dyn TtsEngine>> {
    match config.engine.as_str() {
        "native" => {
            let engine = native::NativeTtsEngine::new()?;
            Ok(Box::new(engine))
        }
        "edge" => {
            let engine = edge::EdgeTtsEngine::new()?;
            Ok(Box::new(engine))
        }
        "elevenlabs" => {
            let engine = elevenlabs::ElevenLabsTtsEngine::new()?;
            Ok(Box::new(engine))
        }
        other => Err(VidgenError::Tts(format!(
            "Unknown TTS engine: '{other}'. Supported: native, edge, elevenlabs"
        ))),
    }
}

/// Query audio duration via ffprobe. Returns seconds.
pub fn ffprobe_duration(path: &Path) -> VidgenResult<f64> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(path.as_os_str())
        .output()
        .map_err(|e| VidgenError::Tts(format!("Failed to run ffprobe: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VidgenError::Tts(format!("ffprobe failed: {stderr}")));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .trim()
        .parse::<f64>()
        .map_err(|e| VidgenError::Tts(format!("Failed to parse ffprobe duration: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_engine_native() {
        let config = VoiceConfig {
            engine: "native".into(),
            default_voice: None,
            speed: 1.0,
            ..Default::default()
        };
        // On macOS this should succeed (say is built-in); on Linux it depends on espeak-ng.
        // We test the factory dispatch, not the engine availability.
        let result = create_engine(&config);
        // Just check the factory doesn't panic; availability depends on platform
        if let Ok(engine) = &result {
            assert_eq!(engine.engine_name(), "native");
        }
    }

    #[test]
    fn test_create_engine_edge() {
        let config = VoiceConfig {
            engine: "edge".into(),
            default_voice: None,
            speed: 1.0,
            ..Default::default()
        };
        let result = create_engine(&config);
        // edge-tts may or may not be installed; test the factory dispatch
        if let Ok(engine) = &result {
            assert_eq!(engine.engine_name(), "edge");
        }
    }

    #[test]
    fn test_create_engine_elevenlabs_no_key() {
        // Ensure env var is unset
        let prev = std::env::var("ELEVEN_API_KEY").ok();
        std::env::remove_var("ELEVEN_API_KEY");

        let config = VoiceConfig {
            engine: "elevenlabs".into(),
            default_voice: None,
            speed: 1.0,
            ..Default::default()
        };
        let result = create_engine(&config);
        assert!(result.is_err());

        // Restore if it was set
        if let Some(val) = prev {
            std::env::set_var("ELEVEN_API_KEY", val);
        }
    }

    #[test]
    fn test_create_engine_unknown() {
        let config = VoiceConfig {
            engine: "nonexistent".into(),
            default_voice: None,
            speed: 1.0,
            ..Default::default()
        };
        let result = create_engine(&config);
        assert!(result.is_err());
    }
}
