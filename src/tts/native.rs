use crate::error::{VidgenError, VidgenResult};
use crate::tts::{ffprobe_duration, SynthesisResult, TtsEngine, VoiceInfo};
use std::path::Path;
use std::process::Command;

/// TTS engine that shells out to platform-native commands:
/// - macOS: `say` (built-in)
/// - Linux: `espeak-ng`
pub struct NativeTtsEngine {
    platform: Platform,
}

#[derive(Debug, Clone, Copy)]
enum Platform {
    MacOS,
    Linux,
}

impl NativeTtsEngine {
    /// Create a new NativeTtsEngine, verifying the platform command is available.
    pub fn new() -> VidgenResult<Self> {
        let platform = if cfg!(target_os = "macos") {
            Platform::MacOS
        } else {
            Platform::Linux
        };

        // Verify the command exists
        let cmd = match platform {
            Platform::MacOS => "say",
            Platform::Linux => "espeak-ng",
        };

        let check = Command::new("which")
            .arg(cmd)
            .output()
            .map_err(|e| VidgenError::Tts(format!("Failed to check for '{cmd}': {e}")))?;

        if !check.status.success() {
            return Err(VidgenError::Tts(format!(
                "TTS command '{cmd}' not found on this system"
            )));
        }

        Ok(Self { platform })
    }

    /// Convert speech rate multiplier (1.0 = normal) to platform-specific rate value.
    fn platform_rate(&self, speed: f32) -> String {
        match self.platform {
            // macOS `say -r`: words per minute, ~200 is normal
            Platform::MacOS => ((speed * 200.0) as u32).to_string(),
            // espeak-ng `-s`: words per minute, ~175 is normal
            Platform::Linux => ((speed * 175.0) as u32).to_string(),
        }
    }
}

impl TtsEngine for NativeTtsEngine {
    fn synthesize(
        &self,
        text: &str,
        voice: Option<&str>,
        speed: f32,
        output_path: &Path,
    ) -> VidgenResult<SynthesisResult> {
        let rate = self.platform_rate(speed);

        match self.platform {
            Platform::MacOS => synthesize_macos(text, voice, &rate, output_path),
            Platform::Linux => synthesize_linux(text, voice, &rate, output_path),
        }
    }

    fn list_voices(&self) -> VidgenResult<Vec<VoiceInfo>> {
        match self.platform {
            Platform::MacOS => list_voices_macos(),
            Platform::Linux => list_voices_linux(),
        }
    }

    fn engine_name(&self) -> &str {
        "native"
    }
}

// ---------------------------------------------------------------------------
// macOS: `say`
// ---------------------------------------------------------------------------

fn synthesize_macos(
    text: &str,
    voice: Option<&str>,
    rate: &str,
    output_path: &Path,
) -> VidgenResult<SynthesisResult> {
    // say outputs AIFF; we convert to WAV via ffmpeg
    let aiff_path = output_path.with_extension("aiff");

    let mut cmd = Command::new("say");
    if let Some(v) = voice {
        cmd.args(["-v", v]);
    }
    cmd.args(["-r", rate, "-o"]);
    cmd.arg(&aiff_path);
    cmd.arg("--");
    cmd.arg(text);

    let output = cmd
        .output()
        .map_err(|e| VidgenError::Tts(format!("Failed to run 'say': {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VidgenError::Tts(format!("'say' failed: {stderr}")));
    }

    // Convert AIFF → WAV via ffmpeg
    let ffmpeg_output = Command::new("ffmpeg")
        .args(["-y", "-i"])
        .arg(&aiff_path)
        .args(["-acodec", "pcm_s16le", "-ar", "22050"])
        .arg(output_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| VidgenError::Tts(format!("Failed to convert AIFF→WAV: {e}")))?;

    // Clean up intermediate AIFF
    let _ = std::fs::remove_file(&aiff_path);

    if !ffmpeg_output.status.success() {
        let stderr = String::from_utf8_lossy(&ffmpeg_output.stderr);
        return Err(VidgenError::Tts(format!(
            "FFmpeg AIFF→WAV conversion failed: {stderr}"
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

fn list_voices_macos() -> VidgenResult<Vec<VoiceInfo>> {
    let output = Command::new("say")
        .arg("-v")
        .arg("?")
        .output()
        .map_err(|e| VidgenError::Tts(format!("Failed to run 'say -v ?': {e}")))?;

    if !output.status.success() {
        return Err(VidgenError::Tts("'say -v ?' failed".into()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut voices = Vec::new();

    for line in stdout.lines() {
        // Format: "Name                language  # comment"
        // e.g.: "Samantha             en_US    # ..."
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Split on multiple spaces to get name and the rest
        let parts: Vec<&str> = line.splitn(2, "  ").collect();
        if parts.len() < 2 {
            continue;
        }

        let name = parts[0].trim().to_string();
        let rest = parts[1].trim();

        // Extract language code (before #)
        let lang = rest
            .split_whitespace()
            .next()
            .unwrap_or("unknown")
            .to_string();

        voices.push(VoiceInfo {
            id: name.clone(),
            name: name.clone(),
            language: lang,
            gender: String::new(), // macOS say doesn't expose gender
            engine: "native".into(),
            available: true,
            note: None,
        });
    }

    Ok(voices)
}

// ---------------------------------------------------------------------------
// Linux: `espeak-ng`
// ---------------------------------------------------------------------------

fn synthesize_linux(
    text: &str,
    voice: Option<&str>,
    rate: &str,
    output_path: &Path,
) -> VidgenResult<SynthesisResult> {
    let mut cmd = Command::new("espeak-ng");
    if let Some(v) = voice {
        cmd.args(["-v", v]);
    }
    cmd.args(["-s", rate, "-w"]);
    cmd.arg(output_path);
    cmd.arg("--");
    cmd.arg(text);

    let output = cmd
        .output()
        .map_err(|e| VidgenError::Tts(format!("Failed to run 'espeak-ng': {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VidgenError::Tts(format!("'espeak-ng' failed: {stderr}")));
    }

    let duration_secs = ffprobe_duration(output_path)?;

    Ok(SynthesisResult {
        audio_path: output_path.to_path_buf(),
        duration_secs,
        cached: false,
        word_timestamps: None,
    })
}

fn list_voices_linux() -> VidgenResult<Vec<VoiceInfo>> {
    let output = Command::new("espeak-ng")
        .arg("--voices")
        .output()
        .map_err(|e| VidgenError::Tts(format!("Failed to run 'espeak-ng --voices': {e}")))?;

    if !output.status.success() {
        return Err(VidgenError::Tts("'espeak-ng --voices' failed".into()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut voices = Vec::new();

    for line in stdout.lines().skip(1) {
        // Header: Pty Language       Age/Gender VoiceName          File          Other Languages
        // Data:   5  af             M  Afrikaans                   ...
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 {
            continue;
        }

        let language = fields[1].to_string();
        let gender = match fields[2] {
            "M" => "male".to_string(),
            "F" => "female".to_string(),
            _ => String::new(),
        };
        let name = fields[3].to_string();

        voices.push(VoiceInfo {
            id: language.clone(),
            name,
            language,
            gender,
            engine: "native".into(),
            available: true,
            note: None,
        });
    }

    Ok(voices)
}
