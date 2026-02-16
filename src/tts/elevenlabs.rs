use crate::error::{VidgenError, VidgenResult};
use crate::tts::{ffprobe_duration, SynthesisResult, TtsEngine, VoiceInfo};
use std::path::Path;
use std::process::Command;

const API_BASE: &str = "https://api.elevenlabs.io/v1";
const DEFAULT_VOICE_ID: &str = "21m00Tcm4TlvDq8ikWAM"; // Rachel
const DEFAULT_MODEL_ID: &str = "eleven_multilingual_v2";

/// TTS engine using ElevenLabs cloud API.
///
/// Requires `ELEVEN_API_KEY` environment variable.
/// Returns premium neural voices via REST API (`POST /v1/text-to-speech/{voice_id}`).
#[derive(Debug)]
pub struct ElevenLabsTtsEngine {
    api_key: String,
}

impl ElevenLabsTtsEngine {
    pub fn new() -> VidgenResult<Self> {
        let api_key = std::env::var("ELEVEN_API_KEY").map_err(|_| {
            VidgenError::Tts(
                "ELEVEN_API_KEY env var not set. Get your API key from https://elevenlabs.io"
                    .into(),
            )
        })?;

        if api_key.is_empty() {
            return Err(VidgenError::Tts("ELEVEN_API_KEY env var is empty".into()));
        }

        Ok(Self { api_key })
    }
}

impl TtsEngine for ElevenLabsTtsEngine {
    fn synthesize(
        &self,
        text: &str,
        voice: Option<&str>,
        speed: f32,
        output_path: &Path,
    ) -> VidgenResult<SynthesisResult> {
        let voice_id = voice.unwrap_or(DEFAULT_VOICE_ID);
        let url = format!("{API_BASE}/text-to-speech/{voice_id}?output_format=mp3_44100_128");

        let body = serde_json::json!({
            "text": text,
            "model_id": DEFAULT_MODEL_ID,
        });

        let response = ureq::post(&url)
            .header("xi-api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .send(body.to_string().as_bytes())
            .map_err(|e| VidgenError::Tts(format!("ElevenLabs API request failed: {e}")))?;

        let bytes = response
            .into_body()
            .read_to_vec()
            .map_err(|e| VidgenError::Tts(format!("Failed to read ElevenLabs response: {e}")))?;

        // Write MP3 to temp file, then convert to WAV
        let mp3_path = output_path.with_extension("mp3");
        std::fs::write(&mp3_path, &bytes)
            .map_err(|e| VidgenError::Tts(format!("Failed to write MP3: {e}")))?;

        // Build ffmpeg args: MP3→WAV, optionally with atempo for speed
        let mut ffmpeg_args: Vec<String> =
            vec!["-y".into(), "-i".into(), mp3_path.display().to_string()];

        if (speed - 1.0).abs() > 0.01 {
            let clamped = speed.clamp(0.5, 100.0);
            ffmpeg_args.extend(["-af".into(), format!("atempo={clamped}")]);
        }

        ffmpeg_args.extend([
            "-acodec".into(),
            "pcm_s16le".into(),
            "-ar".into(),
            "22050".into(),
            output_path.display().to_string(),
        ]);

        let ffmpeg_output = Command::new("ffmpeg")
            .args(&ffmpeg_args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output()
            .map_err(|e| VidgenError::Tts(format!("Failed to convert MP3→WAV: {e}")))?;

        // Clean up intermediate MP3
        let _ = std::fs::remove_file(&mp3_path);

        if !ffmpeg_output.status.success() {
            let stderr = String::from_utf8_lossy(&ffmpeg_output.stderr);
            return Err(VidgenError::Tts(format!(
                "FFmpeg MP3→WAV conversion failed: {stderr}"
            )));
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
        let url = format!("{API_BASE}/voices");

        let response = ureq::get(&url)
            .header("xi-api-key", &self.api_key)
            .call()
            .map_err(|e| VidgenError::Tts(format!("ElevenLabs list voices failed: {e}")))?;

        let body = response
            .into_body()
            .read_to_string()
            .map_err(|e| VidgenError::Tts(format!("Failed to read voices response: {e}")))?;

        Ok(parse_voices_response(&body))
    }

    fn engine_name(&self) -> &str {
        "elevenlabs"
    }
}

/// Parsed labels from an ElevenLabs voice entry.
#[derive(serde::Deserialize, Default)]
struct VoiceLabels {
    language: Option<String>,
    gender: Option<String>,
}

/// A single voice entry from the ElevenLabs API.
#[derive(serde::Deserialize)]
struct ApiVoice {
    voice_id: String,
    name: String,
    labels: Option<VoiceLabels>,
}

/// Top-level response from `GET /v1/voices`.
#[derive(serde::Deserialize)]
struct VoicesResponse {
    voices: Vec<ApiVoice>,
}

/// Parse the JSON response from ElevenLabs `GET /v1/voices` into `VoiceInfo` entries.
fn parse_voices_response(json: &str) -> Vec<VoiceInfo> {
    let parsed: VoicesResponse = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    parsed
        .voices
        .into_iter()
        .map(|v| {
            let labels = v.labels.unwrap_or_default();
            VoiceInfo {
                id: v.voice_id,
                name: v.name,
                language: labels.language.unwrap_or_else(|| "en".into()),
                gender: labels.gender.unwrap_or_default(),
                engine: "elevenlabs".into(),
                available: true,
                note: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_voices_response() {
        let json = r#"{
            "voices": [
                {
                    "voice_id": "abc123",
                    "name": "Rachel",
                    "labels": {"language": "en", "gender": "female"}
                },
                {
                    "voice_id": "def456",
                    "name": "Adam",
                    "labels": {"language": "en", "gender": "male"}
                }
            ]
        }"#;

        let voices = parse_voices_response(json);
        assert_eq!(voices.len(), 2);

        assert_eq!(voices[0].id, "abc123");
        assert_eq!(voices[0].name, "Rachel");
        assert_eq!(voices[0].language, "en");
        assert_eq!(voices[0].gender, "female");
        assert_eq!(voices[0].engine, "elevenlabs");

        assert_eq!(voices[1].id, "def456");
        assert_eq!(voices[1].name, "Adam");
        assert_eq!(voices[1].gender, "male");
    }

    #[test]
    fn test_parse_voices_response_missing_labels() {
        let json = r#"{
            "voices": [
                {
                    "voice_id": "xyz",
                    "name": "Custom Voice",
                    "labels": null
                }
            ]
        }"#;

        let voices = parse_voices_response(json);
        assert_eq!(voices.len(), 1);
        assert_eq!(voices[0].id, "xyz");
        assert_eq!(voices[0].language, "en"); // default
        assert_eq!(voices[0].gender, ""); // default
    }

    #[test]
    fn test_parse_voices_response_empty() {
        let json = r#"{"voices": []}"#;
        let voices = parse_voices_response(json);
        assert!(voices.is_empty());
    }

    #[test]
    fn test_parse_voices_response_invalid_json() {
        let voices = parse_voices_response("not json at all");
        assert!(voices.is_empty());
    }

    #[test]
    fn test_new_missing_env_var() {
        // Temporarily ensure the env var is unset
        let prev = std::env::var("ELEVEN_API_KEY").ok();
        std::env::remove_var("ELEVEN_API_KEY");

        let result = ElevenLabsTtsEngine::new();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("ELEVEN_API_KEY"),
            "Error should mention ELEVEN_API_KEY, got: {err_msg}"
        );

        // Restore if it was set
        if let Some(val) = prev {
            std::env::set_var("ELEVEN_API_KEY", val);
        }
    }
}
