use crate::error::VidgenResult;
use crate::tts::{SynthesisResult, TtsEngine};
use sha2::{Digest, Sha256};
use std::path::Path;

/// Synthesize TTS with file-based caching.
///
/// Cache key is SHA-256 of `"{engine}\0{voice}\0{speed}\0{text}"`.
/// Cached audio is stored in `<project>/assets/voiceover/<hash>.wav`
/// with a `<hash>.json` sidecar containing duration metadata.
pub fn synthesize_cached(
    engine: &dyn TtsEngine,
    text: &str,
    voice: Option<&str>,
    speed: f32,
    output_path: &Path,
    project_path: &Path,
) -> VidgenResult<SynthesisResult> {
    let hash = cache_key(engine.engine_name(), voice, speed, text);
    let cache_dir = project_path.join("assets/voiceover");
    let cached_wav = cache_dir.join(format!("{hash}.wav"));
    let cached_json = cache_dir.join(format!("{hash}.json"));

    // Cache hit: both .wav and .json sidecar exist
    if cached_wav.exists() && cached_json.exists() {
        if let Some(duration_secs) = read_sidecar(&cached_json) {
            std::fs::copy(&cached_wav, output_path)?;
            return Ok(SynthesisResult {
                audio_path: output_path.to_path_buf(),
                duration_secs,
                cached: true,
                word_timestamps: None,
            });
        }
    }

    // Cache miss: synthesize, then populate cache
    let result = engine.synthesize(text, voice, speed, output_path)?;

    std::fs::create_dir_all(&cache_dir)?;
    std::fs::copy(output_path, &cached_wav)?;
    write_sidecar(
        &cached_json,
        result.duration_secs,
        engine.engine_name(),
        voice,
        text,
    );

    Ok(result)
}

/// Compute a deterministic cache key from all inputs that affect audio content.
fn cache_key(engine_name: &str, voice: Option<&str>, speed: f32, text: &str) -> String {
    let voice_str = voice.unwrap_or("");
    let input = format!("{engine_name}\0{voice_str}\0{speed}\0{text}");
    let digest = Sha256::digest(input.as_bytes());
    hex_encode(&digest)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Read duration from a JSON sidecar file. Returns `None` on any error.
fn read_sidecar(path: &Path) -> Option<f64> {
    let contents = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&contents).ok()?;
    v.get("duration_secs")?.as_f64()
}

/// Write a JSON sidecar with duration and metadata for human inspection.
fn write_sidecar(path: &Path, duration_secs: f64, engine: &str, voice: Option<&str>, text: &str) {
    let text_preview: String = text.chars().take(80).collect();
    let sidecar = serde_json::json!({
        "duration_secs": duration_secs,
        "engine": engine,
        "voice": voice.unwrap_or(""),
        "text_preview": text_preview,
    });
    let _ = std::fs::write(
        path,
        serde_json::to_string_pretty(&sidecar).unwrap_or_default(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_key_deterministic() {
        let a = cache_key("elevenlabs", Some("Rachel"), 1.0, "Hello world");
        let b = cache_key("elevenlabs", Some("Rachel"), 1.0, "Hello world");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64); // SHA-256 hex = 64 chars
    }

    #[test]
    fn test_cache_key_varies_on_text() {
        let a = cache_key("native", None, 1.0, "Hello");
        let b = cache_key("native", None, 1.0, "Goodbye");
        assert_ne!(a, b);
    }

    #[test]
    fn test_cache_key_varies_on_voice() {
        let a = cache_key("edge", Some("en-US-AriaNeural"), 1.0, "Hello");
        let b = cache_key("edge", Some("en-US-GuyNeural"), 1.0, "Hello");
        assert_ne!(a, b);
    }

    #[test]
    fn test_cache_key_varies_on_speed() {
        let a = cache_key("native", None, 1.0, "Hello");
        let b = cache_key("native", None, 1.5, "Hello");
        assert_ne!(a, b);
    }

    #[test]
    fn test_cache_key_varies_on_engine() {
        let a = cache_key("native", None, 1.0, "Hello");
        let b = cache_key("edge", None, 1.0, "Hello");
        assert_ne!(a, b);
    }

    #[test]
    fn test_sidecar_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.json");

        write_sidecar(
            &path,
            4.2,
            "elevenlabs",
            Some("Rachel"),
            "Getting started with vidgen is easy.",
        );

        let duration = read_sidecar(&path);
        assert_eq!(duration, Some(4.2));
    }

    #[test]
    fn test_read_sidecar_missing_file() {
        let result = read_sidecar(Path::new("/nonexistent/path.json"));
        assert_eq!(result, None);
    }

    #[test]
    fn test_read_sidecar_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "not json").unwrap();

        assert_eq!(read_sidecar(&path), None);
    }

    #[test]
    fn test_sidecar_text_preview_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("long.json");
        let long_text = "a".repeat(200);

        write_sidecar(&path, 1.0, "native", None, &long_text);

        let contents = std::fs::read_to_string(&path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&contents).unwrap();
        let preview = v["text_preview"].as_str().unwrap();
        assert_eq!(preview.len(), 80);
    }
}
