use crate::error::{VidgenError, VidgenResult};
use crate::tts::{ffprobe_duration, SynthesisResult, TtsEngine, VoiceInfo};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// TTS engine using Piper, a fast local neural TTS.
///
/// Requires the `piper` binary on PATH and a downloaded ONNX voice model.
/// See <https://github.com/rhasspy/piper> for installation.
/// Piper outputs WAV directly — no ffmpeg conversion step needed.
pub struct PiperTtsEngine;

/// Default Piper voice model when none is specified.
const DEFAULT_MODEL: &str = "en_US-amy-medium";

impl PiperTtsEngine {
    /// Create a new PiperTtsEngine, verifying `piper` is on PATH.
    pub fn new() -> VidgenResult<Self> {
        let check = Command::new("which")
            .arg("piper")
            .output()
            .map_err(|e| VidgenError::Tts(format!("Failed to check for 'piper': {e}")))?;

        if !check.status.success() {
            return Err(VidgenError::Tts(
                "piper not found. Install from: https://github.com/rhasspy/piper/releases".into(),
            ));
        }

        Ok(Self)
    }
}

/// Convert a vidgen speed multiplier to piper's `--length-scale`.
///
/// Piper's length_scale is inverse: higher values = slower speech.
/// vidgen convention: higher speed = faster speech.
/// So: `length_scale = 1.0 / speed`.
fn speed_to_length_scale(speed: f32) -> f32 {
    1.0 / speed
}

impl TtsEngine for PiperTtsEngine {
    fn synthesize(
        &self,
        text: &str,
        voice: Option<&str>,
        speed: f32,
        output_path: &Path,
    ) -> VidgenResult<SynthesisResult> {
        let model = voice.unwrap_or(DEFAULT_MODEL);
        let length_scale = speed_to_length_scale(speed);

        // Piper reads text from stdin and writes WAV to --output_file
        let mut child = Command::new("piper")
            .args(["--model", model])
            .args(["--length-scale", &format!("{length_scale:.2}")])
            .arg("--output_file")
            .arg(output_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| VidgenError::Tts(format!("Failed to spawn 'piper': {e}")))?;

        // Write text to piper's stdin
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(text.as_bytes())
                .map_err(|e| VidgenError::Tts(format!("Failed to write to piper stdin: {e}")))?;
            // stdin is dropped here, closing the pipe
        }

        let output = child
            .wait_with_output()
            .map_err(|e| VidgenError::Tts(format!("Failed to wait for 'piper': {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(VidgenError::Tts(format!("'piper' failed: {stderr}")));
        }

        let duration_secs = ffprobe_duration(output_path)?;

        Ok(SynthesisResult {
            audio_path: output_path.to_path_buf(),
            duration_secs,
            cached: false,
            word_timestamps: None,
        })
    }

    fn list_voices(&self) -> VidgenResult<Vec<VoiceInfo>> {
        // Piper doesn't have a --list-voices command.
        // Models are downloaded separately from https://github.com/rhasspy/piper/blob/master/VOICES.md
        Ok(vec![VoiceInfo {
            id: DEFAULT_MODEL.into(),
            name: "Amy (US English, Medium)".into(),
            language: "en-US".into(),
            gender: "female".into(),
            engine: "piper".into(),
            available: true,
            note: Some(
                "Piper uses downloadable ONNX models. See https://github.com/rhasspy/piper/blob/master/VOICES.md for all available voices."
                    .into(),
            ),
        }])
    }

    fn engine_name(&self) -> &str {
        "piper"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_speed_to_length_scale_normal() {
        let scale = speed_to_length_scale(1.0);
        assert!((scale - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_speed_to_length_scale_faster() {
        // speed 2.0 → length_scale 0.5 (shorter duration = faster)
        let scale = speed_to_length_scale(2.0);
        assert!((scale - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_speed_to_length_scale_slower() {
        // speed 0.5 → length_scale 2.0 (longer duration = slower)
        let scale = speed_to_length_scale(0.5);
        assert!((scale - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_speed_to_length_scale_moderate_fast() {
        // speed 1.25 → length_scale 0.8
        let scale = speed_to_length_scale(1.25);
        assert!((scale - 0.8).abs() < 0.001);
    }

    #[test]
    fn test_speed_to_length_scale_moderate_slow() {
        // speed 0.8 → length_scale 1.25
        let scale = speed_to_length_scale(0.8);
        assert!((scale - 1.25).abs() < 0.001);
    }

    #[test]
    fn test_list_voices() {
        // list_voices doesn't require piper to be installed
        let engine = PiperTtsEngine;
        let voices = engine.list_voices().unwrap();
        assert_eq!(voices.len(), 1);
        assert_eq!(voices[0].id, "en_US-amy-medium");
        assert_eq!(voices[0].engine, "piper");
        assert!(voices[0].note.is_some());
    }

    #[test]
    fn test_engine_name() {
        let engine = PiperTtsEngine;
        assert_eq!(engine.engine_name(), "piper");
    }
}
