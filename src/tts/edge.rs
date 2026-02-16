use crate::error::{VidgenError, VidgenResult};
use crate::tts::{ffprobe_duration, SynthesisResult, TtsEngine, VoiceInfo};
use std::path::Path;
use std::process::Command;

/// TTS engine using Microsoft Edge's neural TTS via the `edge-tts` Python CLI.
///
/// Requires `pip install edge-tts` and internet access.
/// Provides 300+ high-quality neural voices for free, no API key.
pub struct EdgeTtsEngine;

impl EdgeTtsEngine {
    /// Create a new EdgeTtsEngine, verifying `edge-tts` is on PATH.
    pub fn new() -> VidgenResult<Self> {
        let check = Command::new("which")
            .arg("edge-tts")
            .output()
            .map_err(|e| VidgenError::Tts(format!("Failed to check for 'edge-tts': {e}")))?;

        if !check.status.success() {
            return Err(VidgenError::Tts(
                "edge-tts not found. Install with: pip install edge-tts".into(),
            ));
        }

        Ok(Self)
    }
}

/// Convert a speed multiplier to an edge-tts `--rate` string.
///
/// `1.0` → `"+0%"`, `1.2` → `"+20%"`, `0.8` → `"-20%"`.
fn speed_to_rate(speed: f32) -> String {
    let pct = ((speed - 1.0) * 100.0).round() as i32;
    if pct >= 0 {
        format!("+{pct}%")
    } else {
        format!("{pct}%")
    }
}

/// Default voice when none is specified.
const DEFAULT_VOICE: &str = "en-US-AriaNeural";

impl TtsEngine for EdgeTtsEngine {
    fn synthesize(
        &self,
        text: &str,
        voice: Option<&str>,
        speed: f32,
        output_path: &Path,
    ) -> VidgenResult<SynthesisResult> {
        let voice = voice.unwrap_or(DEFAULT_VOICE);
        let rate = speed_to_rate(speed);

        // edge-tts outputs MP3; write to a temp file then convert to WAV
        let mp3_path = output_path.with_extension("mp3");

        let output = Command::new("edge-tts")
            .args(["--voice", voice])
            .args(["--rate", &rate])
            .args(["--text", text])
            .arg("--write-media")
            .arg(&mp3_path)
            .output()
            .map_err(|e| VidgenError::Tts(format!("Failed to run 'edge-tts': {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(VidgenError::Tts(format!("'edge-tts' failed: {stderr}")));
        }

        // Convert MP3 → WAV via ffmpeg
        let ffmpeg_output = Command::new("ffmpeg")
            .args(["-y", "-i"])
            .arg(&mp3_path)
            .args(["-acodec", "pcm_s16le", "-ar", "22050"])
            .arg(output_path)
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
        let output = Command::new("edge-tts")
            .arg("--list-voices")
            .output()
            .map_err(|e| {
                VidgenError::Tts(format!("Failed to run 'edge-tts --list-voices': {e}"))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(VidgenError::Tts(format!(
                "'edge-tts --list-voices' failed: {stderr}"
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(parse_edge_voices(&stdout))
    }

    fn engine_name(&self) -> &str {
        "edge"
    }
}

/// Parse the block format output of `edge-tts --list-voices`.
///
/// Each voice is a block of `Key: Value` lines separated by blank lines:
/// ```text
/// Name: en-US-AriaNeural
/// Gender: Female
///
/// Name: en-US-GuyNeural
/// Gender: Male
/// ```
fn parse_edge_voices(output: &str) -> Vec<VoiceInfo> {
    let mut voices = Vec::new();
    let mut name = String::new();
    let mut gender = String::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            // End of a block — emit voice if we have a name
            if !name.is_empty() {
                let language = extract_language(&name);
                voices.push(VoiceInfo {
                    id: name.clone(),
                    name: name.clone(),
                    language,
                    gender: gender.to_lowercase(),
                    engine: "edge".into(),
                    available: true,
                    note: None,
                });
                name.clear();
                gender.clear();
            }
            continue;
        }

        if let Some(val) = line.strip_prefix("Name: ") {
            name = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("Gender: ") {
            gender = val.trim().to_string();
        }
    }

    // Handle last block (no trailing blank line)
    if !name.is_empty() {
        let language = extract_language(&name);
        voices.push(VoiceInfo {
            id: name.clone(),
            name: name.clone(),
            language,
            gender: gender.to_lowercase(),
            engine: "edge".into(),
            available: true,
            note: None,
        });
    }

    voices
}

/// Extract language from an edge-tts voice ID.
///
/// `"en-US-AriaNeural"` → `"en-US"` (first two hyphen-separated parts).
fn extract_language(voice_id: &str) -> String {
    let parts: Vec<&str> = voice_id.splitn(3, '-').collect();
    if parts.len() >= 2 {
        format!("{}-{}", parts[0], parts[1])
    } else {
        voice_id.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_speed_to_rate() {
        assert_eq!(speed_to_rate(1.0), "+0%");
        assert_eq!(speed_to_rate(1.2), "+20%");
        assert_eq!(speed_to_rate(0.8), "-20%");
        assert_eq!(speed_to_rate(1.5), "+50%");
        assert_eq!(speed_to_rate(0.5), "-50%");
        assert_eq!(speed_to_rate(2.0), "+100%");
    }

    #[test]
    fn test_parse_edge_voices() {
        let sample = "\
Name: en-US-AriaNeural
Gender: Female

Name: en-US-GuyNeural
Gender: Male

Name: ja-JP-NanamiNeural
Gender: Female
";
        let voices = parse_edge_voices(sample);
        assert_eq!(voices.len(), 3);

        assert_eq!(voices[0].id, "en-US-AriaNeural");
        assert_eq!(voices[0].language, "en-US");
        assert_eq!(voices[0].gender, "female");
        assert_eq!(voices[0].engine, "edge");

        assert_eq!(voices[1].id, "en-US-GuyNeural");
        assert_eq!(voices[1].gender, "male");

        assert_eq!(voices[2].id, "ja-JP-NanamiNeural");
        assert_eq!(voices[2].language, "ja-JP");
    }

    #[test]
    fn test_parse_edge_voices_empty() {
        let voices = parse_edge_voices("");
        assert!(voices.is_empty());

        let voices = parse_edge_voices("   \n\n  \n");
        assert!(voices.is_empty());
    }

    #[test]
    fn test_parse_edge_voices_no_trailing_newline() {
        let sample = "Name: en-GB-SoniaNeural\nGender: Female";
        let voices = parse_edge_voices(sample);
        assert_eq!(voices.len(), 1);
        assert_eq!(voices[0].id, "en-GB-SoniaNeural");
        assert_eq!(voices[0].language, "en-GB");
    }

    #[test]
    fn test_extract_language() {
        assert_eq!(extract_language("en-US-AriaNeural"), "en-US");
        assert_eq!(extract_language("ja-JP-NanamiNeural"), "ja-JP");
        assert_eq!(extract_language("zh-CN-XiaoxiaoNeural"), "zh-CN");
        assert_eq!(extract_language("unknown"), "unknown");
    }
}
