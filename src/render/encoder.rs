use crate::config::{PlatformPreset, VideoConfig};
use crate::error::{VidgenError, VidgenResult};
use crate::scene::Scene;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread::JoinHandle;
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// Transition types
// ---------------------------------------------------------------------------

/// Supported transition types for scene-to-scene blending via FFmpeg xfade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionType {
    Fade,
    SlideLeft,
    SlideRight,
    Zoom,
    Wipe,
    None,
}

impl TransitionType {
    /// Parse a transition name from scene frontmatter / config strings.
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "fade" => Self::Fade,
            "slide-left" | "slideleft" | "slide_left" => Self::SlideLeft,
            "slide-right" | "slideright" | "slide_right" => Self::SlideRight,
            "zoom" => Self::Zoom,
            "wipe" | "wipeleft" | "wipe-left" => Self::Wipe,
            "none" | "" => Self::None,
            other => {
                warn!("Unknown transition \"{other}\", defaulting to fade");
                Self::Fade
            }
        }
    }

    /// Return the FFmpeg xfade transition name.
    pub fn ffmpeg_name(&self) -> &'static str {
        match self {
            Self::Fade => "fade",
            Self::SlideLeft => "slideleft",
            Self::SlideRight => "slideright",
            Self::Zoom => "smoothup",
            Self::Wipe => "wipeleft",
            Self::None => "fade", // used with tiny duration for instant cut
        }
    }
}

/// A resolved transition between two adjacent scenes.
#[derive(Debug, Clone)]
pub struct SceneTransition {
    pub transition_type: TransitionType,
    pub duration: f64,
}

/// Resolve the transition between scene N (out) and scene N+1 (in).
///
/// Priority: scene_out.transition_out > scene_in.transition_in > config default > None.
/// Duration: scene-level transition_duration (if set on either scene, preferring out),
/// else config default_transition_duration.
pub fn resolve_transition(
    scene_out: &Scene,
    scene_in: &Scene,
    video_config: &VideoConfig,
) -> Option<SceneTransition> {
    // Determine the transition type string
    let transition_name = scene_out
        .frontmatter
        .transition_out
        .as_deref()
        .or(scene_in.frontmatter.transition_in.as_deref())
        .or(video_config.default_transition.as_deref());

    let transition_name = transition_name?;

    let transition_type = TransitionType::from_str(transition_name);
    if transition_type == TransitionType::None {
        return None;
    }

    // Determine duration: prefer scene_out's duration, then scene_in's, then config default
    let duration = scene_out
        .frontmatter
        .transition_duration
        .or(scene_in.frontmatter.transition_duration)
        .unwrap_or(video_config.default_transition_duration);

    Some(SceneTransition {
        transition_type,
        duration,
    })
}

/// Encodes PNG frames piped to stdin into an MP4 file.
pub struct SceneEncoder {
    child: Child,
    output_path: PathBuf,
    stderr_handle: Option<JoinHandle<String>>,
}

impl SceneEncoder {
    /// Spawn an FFmpeg process that accepts PNG frames on stdin.
    /// If `audio_path` is provided (TTS voice), the audio file is muxed into the output.
    /// If `music_path` is provided, the music file is mixed in at the given volume.
    /// When both are present, they are combined via `amix`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        output_path: &Path,
        fps: u32,
        width: u32,
        height: u32,
        platform: &PlatformPreset,
        audio_path: Option<&Path>,
        music_path: Option<&Path>,
        music_volume: f64,
        audio_delay_secs: f64,
        effective_duration: Option<f64>,
    ) -> VidgenResult<Self> {
        let mut cmd = Command::new("ffmpeg");
        cmd.args([
            "-y", // Overwrite output
            "-f",
            "image2pipe", // Input format: piped images
            "-vcodec",
            "png", // Input codec
            "-framerate",
            &fps.to_string(), // Input framerate
            "-s",
            &format!("{width}x{height}"), // Input size
            "-i",
            "-", // Read from stdin
        ]);

        // Add audio inputs: voice first (input 1), then music (input 2)
        let has_voice = audio_path.is_some();
        let has_music = music_path.is_some();

        if let Some(audio) = audio_path {
            cmd.args(["-i"]).arg(audio.as_os_str());
        }
        if let Some(music) = music_path {
            cmd.args(["-i"]).arg(music.as_os_str());
        }

        cmd.args([
            "-c:v",
            "libx264", // H.264 codec
            "-pix_fmt",
            "yuv420p", // Pixel format for compatibility
            "-crf",
            &platform.crf.to_string(), // Quality
            "-preset",
            platform.preset, // Speed/quality tradeoff
            "-movflags",
            "+faststart", // Web-optimized
        ]);

        // Audio mixing: voice + music, only voice, only music, or none
        // When audio_delay_secs > 0, insert an adelay filter to shift the voice track
        let delay_ms = (audio_delay_secs * 1000.0).round() as u64;
        match (has_voice, has_music) {
            (true, true) => {
                // Voice is input 1, music is input 2
                let voice_chain = if delay_ms > 0 {
                    format!("[1:a]adelay={delay_ms}|{delay_ms},volume=1.0[voice]")
                } else {
                    "[1:a]volume=1.0[voice]".to_string()
                };
                let filter = format!(
                    "{voice_chain};[2:a]volume={music_volume:.2}[music];\
                     [voice][music]amix=inputs=2:duration=first:dropout_transition=2:normalize=0[aout]"
                );
                cmd.args(["-filter_complex", &filter, "-map", "0:v", "-map", "[aout]"]);
                cmd.args([
                    "-c:a", "aac", "-ac", "2",
                    "-b:a", platform.audio_bitrate,
                    "-ar", &platform.audio_samplerate.to_string(),
                ]);
            }
            (true, false) => {
                if delay_ms > 0 {
                    cmd.args(["-af", &format!("adelay={delay_ms}|{delay_ms}")]);
                }
                cmd.args([
                    "-c:a", "aac", "-ac", "2",
                    "-b:a", platform.audio_bitrate,
                    "-ar", &platform.audio_samplerate.to_string(),
                ]);
            }
            (false, true) => {
                // Music only is input 1
                let filter = format!("[1:a]volume={music_volume:.2}[aout]");
                cmd.args(["-filter_complex", &filter, "-map", "0:v", "-map", "[aout]"]);
                cmd.args([
                    "-c:a", "aac", "-ac", "2",
                    "-b:a", platform.audio_bitrate,
                    "-ar", &platform.audio_samplerate.to_string(),
                ]);
            }
            (false, false) => {}
        }

        // Force exact output duration to match the video frames.
        // This prevents TTS audio longer than the scene from extending the MP4.
        if let Some(dur) = effective_duration {
            cmd.args(["-t", &format!("{dur:.3}")]);
        }

        cmd.arg(output_path.as_os_str());
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::piped());

        debug!(
            "Spawning FFmpeg encoder: {}x{} @ {}fps, crf={}",
            width, height, fps, platform.crf
        );

        let mut child = cmd
            .spawn()
            .map_err(|e| VidgenError::Ffmpeg(format!("Failed to spawn ffmpeg: {e}")))?;

        // Drain stderr in a background thread to prevent pipe deadlock
        let stderr_handle = child.stderr.take().map(|mut stderr| {
            std::thread::spawn(move || {
                let mut buf = String::new();
                let _ = stderr.read_to_string(&mut buf);
                buf
            })
        });

        Ok(Self {
            child,
            output_path: output_path.to_path_buf(),
            stderr_handle,
        })
    }

    /// Write a single PNG frame to FFmpeg's stdin.
    pub fn write_frame(&mut self, png_data: &[u8]) -> VidgenResult<()> {
        let stdin = self
            .child
            .stdin
            .as_mut()
            .ok_or_else(|| VidgenError::Ffmpeg("FFmpeg stdin closed".into()))?;

        stdin
            .write_all(png_data)
            .map_err(|e| VidgenError::Ffmpeg(format!("Failed to write frame: {e}")))?;

        Ok(())
    }

    /// Close stdin and wait for FFmpeg to finish encoding.
    pub fn finish(mut self) -> VidgenResult<PathBuf> {
        // Drop stdin to signal EOF
        drop(self.child.stdin.take());

        let status = self
            .child
            .wait()
            .map_err(|e| VidgenError::Ffmpeg(format!("FFmpeg wait failed: {e}")))?;

        // Collect stderr from background drain thread
        let stderr_output = self
            .stderr_handle
            .take()
            .and_then(|h| h.join().ok())
            .unwrap_or_default();

        if !status.success() {
            let last_line = stderr_output
                .lines()
                .last()
                .unwrap_or("unknown error");
            return Err(VidgenError::Ffmpeg(format!(
                "FFmpeg encoding failed (exit {}): {}",
                status, last_line
            )));
        }

        Ok(self.output_path)
    }
}

/// Concatenate multiple MP4 files using FFmpeg's concat filter.
/// Uses the filter (not demuxer) to properly handle audio timestamp continuity
/// across scene boundaries. Re-encodes both video and audio for seamless output.
pub fn concat_scenes(scene_files: &[PathBuf], output_path: &Path) -> VidgenResult<()> {
    if scene_files.len() == 1 {
        std::fs::copy(&scene_files[0], output_path)?;
        return Ok(());
    }

    let n = scene_files.len();
    let any_audio = scene_files.iter().any(|f| has_audio_stream(f));

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y");

    // Add all input files
    for file in scene_files {
        cmd.args(["-i"]).arg(file.as_os_str());
    }

    // Build concat filter — normalize all streams to identical format first.
    // This prevents DTS/PTS timestamp mismatches between HTML-rendered scenes
    // (from SceneEncoder) and video clip scenes (from prepare_video_clip),
    // which have different timebases and can cause truncated output.
    let mut filter_parts: Vec<String> = Vec::new();

    for (i, file) in scene_files.iter().enumerate() {
        // Normalize video: consistent fps, pixel format, timebase, and reset PTS
        filter_parts.push(format!(
            "[{i}:v]fps=30,format=yuv420p,setpts=PTS-STARTPTS[v{i}]"
        ));

        if any_audio && !has_audio_stream(file) {
            // Generate silence for this input to match its video duration
            let dur = probe_video_duration(file).unwrap_or(3.0);
            filter_parts.push(format!(
                "anullsrc=cl=stereo:r=44100[silence{i}];[silence{i}]atrim=0:{dur:.3},asetpts=PTS-STARTPTS[a{i}]"
            ));
        } else if any_audio {
            filter_parts.push(format!(
                "[{i}:a]aformat=sample_rates=44100:channel_layouts=stereo,asetpts=PTS-STARTPTS[a{i}]"
            ));
        }
    }

    // Concat filter with normalized inputs
    let inputs: String = (0..n)
        .map(|i| {
            if any_audio {
                format!("[v{i}][a{i}]")
            } else {
                format!("[v{i}]")
            }
        })
        .collect();

    let a_flag = if any_audio { 1 } else { 0 };
    filter_parts.push(format!(
        "{inputs}concat=n={n}:v=1:a={a_flag}[vout]{}",
        if any_audio { "[aout]" } else { "" }
    ));

    let filter = filter_parts.join(";");
    cmd.args(["-filter_complex", &filter, "-map", "[vout]"]);

    if any_audio {
        cmd.args([
            "-map", "[aout]",
            "-c:a", "aac", "-ac", "2", "-ar", "44100", "-b:a", "192k",
        ]);
    }

    cmd.args([
        "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "23", "-preset", "fast",
        "-movflags", "+faststart",
    ]);
    cmd.arg(output_path.as_os_str());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());

    let output = cmd
        .output()
        .map_err(|e| VidgenError::Ffmpeg(format!("Failed to spawn ffmpeg concat: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VidgenError::Ffmpeg(format!(
            "FFmpeg concat failed: {}",
            stderr.lines().last().unwrap_or("unknown error")
        )));
    }

    Ok(())
}

/// Check if a media file has an audio stream.
fn has_audio_stream(path: &Path) -> bool {
    Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a",
            "-show_entries",
            "stream=codec_type",
            "-of",
            "csv=p=0",
        ])
        .arg(path.as_os_str())
        .output()
        .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false)
}

/// Concatenate scene MP4 files with optional xfade transitions between them.
///
/// - Single scene → just copy
/// - No transitions → delegate to fast `concat_scenes()` (no re-encode)
/// - Has transitions → build FFmpeg xfade filter graph and re-encode
pub fn concat_scenes_with_transitions(
    scene_files: &[PathBuf],
    scene_durations: &[f64],
    transitions: &[Option<SceneTransition>],
    output_path: &Path,
    platform: &PlatformPreset,
) -> VidgenResult<()> {
    debug!(
        "Concatenating {} scenes to {}",
        scene_files.len(),
        output_path.display()
    );
    if scene_files.len() == 1 {
        std::fs::copy(&scene_files[0], output_path)?;
        return Ok(());
    }

    // Check if there are any actual transitions
    let has_transitions = transitions.iter().any(|t| t.is_some());
    if !has_transitions {
        return concat_scenes(scene_files, output_path);
    }

    // BUG-001: xfade transitions produce truncated output when mixing
    // HTML-rendered scenes with video clip scenes (prepare_video_clip output).
    // Detect mixed scene types by checking if any scene was produced by
    // prepare_video_clip (which adds an overlay post-process or has different
    // internal structure). For safety, check actual file sizes — clip scenes
    // tend to be much larger than HTML-rendered scenes.
    // The concat filter path works correctly, so use it as fallback.
    // TODO: fix xfade with mixed scene types
    if scene_files.len() > 1 {
        let sizes: Vec<u64> = scene_files.iter()
            .map(|f| std::fs::metadata(f).map(|m| m.len()).unwrap_or(0))
            .collect();
        let max_size = sizes.iter().max().unwrap_or(&0);
        let min_size = sizes.iter().min().unwrap_or(&0);
        // If largest scene is >10x smallest, likely mixed HTML + clip scenes
        if *min_size > 0 && *max_size > min_size * 10 {
            warn!("Mixed scene types detected — using hard cuts instead of transitions (BUG-001 workaround)");
            return concat_scenes(scene_files, output_path);
        }
    }

    // Check which scene files have audio streams
    let has_audio: Vec<bool> = scene_files.iter().map(|f| has_audio_stream(f)).collect();
    let any_audio = has_audio.iter().any(|&a| a);

    // Normalize all video inputs to prevent DTS/PTS mismatches between
    // HTML-rendered scenes and video clip scenes (different timebases).
    let n = scene_files.len();
    let mut filter_parts: Vec<String> = Vec::new();

    for i in 0..n {
        filter_parts.push(format!(
            "[{i}:v]fps=30,format=yuv420p,setpts=PTS-STARTPTS[vin{i}]"
        ));
    }

    // Build FFmpeg xfade filter graph for video
    let mut offset = 0.0_f64;

    for i in 0..n - 1 {
        let trans = &transitions[i];
        let (trans_name, trans_dur) = match trans {
            Some(t) => (t.transition_type.ffmpeg_name(), t.duration),
            None => ("fade", 0.001), // instant cut
        };

        if i == 0 {
            offset = scene_durations[0] - trans_dur;
        } else {
            offset += scene_durations[i] - trans_dur;
        }

        let offset_val = offset.max(0.0);

        let input_a = if i == 0 {
            "[vin0]".to_string()
        } else {
            format!("[xv{i}]")
        };
        let input_b = format!("[vin{}]", i + 1);
        let output_label = if i == n - 2 {
            "[vout]".to_string()
        } else {
            format!("[xv{}]", i + 1)
        };

        filter_parts.push(format!(
            "{input_a}{input_b}xfade=transition={trans_name}:duration={trans_dur:.3}:offset={offset_val:.3}{output_label}"
        ));
    }

    // Build audio filter chain if any scenes have audio
    if any_audio {
        // For scenes without audio, generate silence matching the scene duration.
        // Use anullsrc → atrim to produce a silent segment, then normalize all
        // audio streams to the same format before crossfading.
        // Use platform sample rate for consistency with per-scene encoding.
        let sr = platform.audio_samplerate;
        for (i, (&has, dur)) in has_audio.iter().zip(scene_durations.iter()).enumerate() {
            if !has {
                filter_parts.push(format!(
                    "anullsrc=cl=stereo:r={sr}[silence{i}];[silence{i}]atrim=0:{dur:.3},asetpts=PTS-STARTPTS[sa{i}]"
                ));
            } else {
                // Normalize existing audio to consistent format
                filter_parts.push(format!(
                    "[{i}:a]aformat=sample_rates={sr}:channel_layouts=stereo,asetpts=PTS-STARTPTS[sa{i}]"
                ));
            }
        }

        // Build acrossfade chain for audio
        for (i, trans) in transitions.iter().enumerate().take(n - 1) {
            let trans_dur = match trans {
                Some(t) => t.duration,
                None => 0.001,
            };

            let input_a = if i == 0 {
                "[sa0]".to_string()
            } else {
                format!("[a{i}]")
            };
            let input_b = format!("[sa{}]", i + 1);
            let output_label = if i == n - 2 {
                "[aout]".to_string()
            } else {
                format!("[a{}]", i + 1)
            };

            filter_parts.push(format!(
                "{input_a}{input_b}acrossfade=d={trans_dur:.3}:c1=tri:c2=tri{output_label}"
            ));
        }
    }

    let filter_graph = filter_parts.join(";");

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y");

    // Add all input files
    for file in scene_files {
        cmd.args(["-i"]).arg(file.as_os_str());
    }

    cmd.args(["-filter_complex", &filter_graph, "-map", "[vout]"]);

    if any_audio {
        cmd.args(["-map", "[aout]"]);
    }

    cmd.args([
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
        "-crf",
        &platform.crf.to_string(),
        "-preset",
        platform.preset,
        "-movflags",
        "+faststart",
    ]);

    if any_audio {
        cmd.args([
            "-c:a", "aac", "-ac", "2",
            "-b:a", platform.audio_bitrate,
            "-ar", &platform.audio_samplerate.to_string(),
        ]);
    }

    cmd.arg(output_path.as_os_str());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());

    let output = cmd
        .output()
        .map_err(|e| VidgenError::Ffmpeg(format!("Failed to spawn ffmpeg xfade: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VidgenError::Ffmpeg(format!(
            "FFmpeg xfade concat failed: {}",
            stderr.lines().last().unwrap_or("unknown error")
        )));
    }

    Ok(())
}

/// Probe the duration of a video file in seconds using ffprobe.
pub fn probe_video_duration(path: &Path) -> VidgenResult<f64> {
    let output = Command::new("ffprobe")
        .args([
            "-v", "error",
            "-show_entries", "format=duration",
            "-of", "csv=p=0",
        ])
        .arg(path.as_os_str())
        .output()
        .map_err(|e| VidgenError::Ffmpeg(format!("Failed to run ffprobe: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VidgenError::Ffmpeg(format!(
            "ffprobe failed for {}: {}",
            path.display(),
            stderr.lines().last().unwrap_or("unknown error")
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .trim()
        .parse::<f64>()
        .map_err(|_| VidgenError::Ffmpeg(format!(
            "Could not parse duration from ffprobe output: '{}'",
            stdout.trim()
        )))
}

/// Re-encode an external video clip to match the target format dimensions and codec.
///
/// Scales the video to fit within `width x height` (with padding if aspect ratios differ),
/// trims to `duration` seconds if specified, and encodes with the given platform preset.
/// Optionally mixes in voice audio and/or background music.
#[allow(clippy::too_many_arguments)]
pub fn prepare_video_clip(
    source_path: &Path,
    output_path: &Path,
    width: u32,
    height: u32,
    fps: u32,
    duration: Option<f64>,
    platform: &PlatformPreset,
    audio_path: Option<&Path>,
    music_path: Option<&Path>,
    music_volume: f64,
    audio_delay_secs: f64,
    source_volume: f64,
) -> VidgenResult<PathBuf> {
    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y");

    // Input: source video (input 0)
    cmd.args(["-i"]).arg(source_path.as_os_str());

    // Optional trim
    if let Some(dur) = duration {
        cmd.args(["-t", &format!("{dur:.3}")]);
    }

    // Additional audio inputs
    let has_voice = audio_path.is_some();
    let has_music = music_path.is_some();
    let has_source_audio = source_volume > 0.0 && has_audio_stream(source_path);

    if let Some(audio) = audio_path {
        cmd.args(["-i"]).arg(audio.as_os_str()); // input 1 (if voice)
    }
    if let Some(music) = music_path {
        cmd.args(["-i"]).arg(music.as_os_str()); // input 2 (if voice+music) or 1 (if music only)
    }

    // Video filter: scale + pad to target dimensions + force fps for xfade compat
    let vf = format!(
        "fps={fps},scale={width}:{height}:force_original_aspect_ratio=decrease,\
         pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:black"
    );

    // Build filter graph based on audio sources:
    // - source audio from the clip (ducked to source_volume)
    // - voice audio from TTS
    // - background music
    let delay_ms = (audio_delay_secs * 1000.0).round() as u64;

    // Input indices: 0=source video/audio, then voice, then music
    let voice_idx = if has_voice { 1 } else { 0 };
    let music_idx = if has_voice && has_music { 2 } else if has_music { 1 } else { 0 };

    // Helper: build voice filter chain with optional delay
    let voice_chain = |label: &str| -> String {
        if delay_ms > 0 {
            format!("[{voice_idx}:a]adelay={delay_ms}|{delay_ms},volume=1.0{label}")
        } else {
            format!("[{voice_idx}:a]volume=1.0{label}")
        }
    };

    // Collect audio streams to mix
    let mut filter_parts: Vec<String> = vec![format!("[0:v]{vf}[vout]")];
    let mut mix_labels: Vec<String> = Vec::new();

    if has_voice {
        filter_parts.push(voice_chain("[voice]"));
        mix_labels.push("[voice]".into());
    }
    if has_source_audio {
        filter_parts.push(format!("[0:a]volume={source_volume:.2}[src]"));
        mix_labels.push("[src]".into());
    }
    if has_music {
        filter_parts.push(format!("[{music_idx}:a]volume={music_volume:.2}[music]"));
        mix_labels.push("[music]".into());
    }

    let has_any_audio = !mix_labels.is_empty();

    if mix_labels.len() > 1 {
        // Mix multiple audio streams
        let inputs = mix_labels.len();
        let labels = mix_labels.join("");
        filter_parts.push(format!(
            "{labels}amix=inputs={inputs}:duration=longest:dropout_transition=2:normalize=0[aout]"
        ));
        let filter = filter_parts.join(";");
        cmd.args(["-filter_complex", &filter, "-map", "[vout]", "-map", "[aout]"]);
    } else if mix_labels.len() == 1 {
        // Single audio stream — rename to [aout]
        // Replace the last label with [aout]
        let last = filter_parts.last_mut().unwrap();
        let label = &mix_labels[0];
        *last = last.replace(label, "[aout]");
        let filter = filter_parts.join(";");
        cmd.args(["-filter_complex", &filter, "-map", "[vout]", "-map", "[aout]"]);
    } else {
        // No audio at all
        cmd.args(["-vf", &vf, "-map", "0:v"]);
    }

    cmd.args([
        "-c:v", "libx264",
        "-pix_fmt", "yuv420p",
        "-crf", &platform.crf.to_string(),
        "-preset", platform.preset,
        "-movflags", "+faststart",
    ]);

    if has_any_audio {
        cmd.args([
            "-c:a", "aac", "-ac", "2",
            "-b:a", platform.audio_bitrate,
            "-ar", &platform.audio_samplerate.to_string(),
        ]);
    }

    cmd.arg(output_path.as_os_str());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());

    debug!(
        "Preparing video clip: {} → {} ({}x{})",
        source_path.display(),
        output_path.display(),
        width,
        height
    );

    let output = cmd
        .output()
        .map_err(|e| VidgenError::Ffmpeg(format!("Failed to spawn ffmpeg for video clip: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VidgenError::Ffmpeg(format!(
            "FFmpeg video clip encoding failed: {}",
            stderr.lines().last().unwrap_or("unknown error")
        )));
    }

    Ok(output_path.to_path_buf())
}

/// Mix voiceover and/or music onto a video file (post-process).
/// The video may already have audio (e.g., from source clips in a sequence).
/// Renames the original to a temp file, re-encodes with audio mix, then removes the temp.
#[allow(clippy::too_many_arguments)]
pub fn mix_audio_onto_video(
    video_path: &Path,
    voice_path: Option<&Path>,
    music_path: Option<&Path>,
    music_volume: f64,
    voice_delay_secs: f64,
    platform: &PlatformPreset,
) -> VidgenResult<()> {
    if voice_path.is_none() && music_path.is_none() {
        return Ok(());
    }

    let tmp_path = video_path.with_extension("mix-tmp.mp4");
    std::fs::rename(video_path, &tmp_path)?;

    let has_existing_audio = has_audio_stream(&tmp_path);
    let has_voice = voice_path.is_some();
    let has_music = music_path.is_some();
    let delay_ms = (voice_delay_secs * 1000.0).round() as u64;

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y");
    cmd.args(["-i"]).arg(&tmp_path); // input 0: video (+ existing audio)

    let mut next_input = 1;
    let voice_idx = if has_voice {
        cmd.args(["-i"]).arg(voice_path.unwrap().as_os_str());
        let idx = next_input;
        next_input += 1;
        Some(idx)
    } else {
        None
    };
    let music_idx = if has_music {
        cmd.args(["-i"]).arg(music_path.unwrap().as_os_str());
        let idx = next_input;
        Some(idx)
    } else {
        None
    };

    // Build audio mix filter
    let mut filter_parts: Vec<String> = Vec::new();
    let mut mix_labels: Vec<String> = Vec::new();

    if has_existing_audio {
        filter_parts.push("[0:a]volume=1.0[existing]".into());
        mix_labels.push("[existing]".into());
    }
    if let Some(vi) = voice_idx {
        if delay_ms > 0 {
            filter_parts.push(format!("[{vi}:a]adelay={delay_ms}|{delay_ms},volume=1.0[voice]"));
        } else {
            filter_parts.push(format!("[{vi}:a]volume=1.0[voice]"));
        }
        mix_labels.push("[voice]".into());
    }
    if let Some(mi) = music_idx {
        filter_parts.push(format!("[{mi}:a]volume={music_volume:.2}[music]"));
        mix_labels.push("[music]".into());
    }

    let inputs = mix_labels.len();
    let labels = mix_labels.join("");
    filter_parts.push(format!(
        "{labels}amix=inputs={inputs}:duration=longest:dropout_transition=2:normalize=0[aout]"
    ));

    let filter = filter_parts.join(";");
    cmd.args(["-filter_complex", &filter, "-map", "0:v", "-map", "[aout]"]);
    cmd.args([
        "-c:v", "copy", // keep video as-is
        "-c:a", "aac", "-ac", "2",
        "-b:a", platform.audio_bitrate,
        "-ar", &platform.audio_samplerate.to_string(),
    ]);
    cmd.arg(video_path.as_os_str());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());

    let output = cmd
        .output()
        .map_err(|e| VidgenError::Ffmpeg(format!("Failed to mix audio: {e}")))?;

    let _ = std::fs::remove_file(&tmp_path);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VidgenError::Ffmpeg(format!(
            "FFmpeg audio mix failed: {}",
            stderr.lines().last().unwrap_or("unknown error")
        )));
    }

    Ok(())
}

/// Burn subtitles into a video file via FFmpeg's `subtitles` filter (post-process).
/// Renames the original video to a temp file, re-encodes with subtitles, then removes the temp.
pub fn burn_in_subtitles(video_path: &Path, srt_path: &Path) -> VidgenResult<()> {
    let tmp_path = video_path.with_extension("tmp.mp4");
    std::fs::rename(video_path, &tmp_path)?;

    // Escape path for FFmpeg subtitles filter (backslashes and colons need escaping)
    let srt_escaped = srt_path
        .display()
        .to_string()
        .replace('\\', "/")
        .replace(':', "\\:");

    let subtitle_filter = format!(
        "subtitles=filename='{}':force_style='FontSize=24,PrimaryColour=&H00FFFFFF,Alignment=2'",
        srt_escaped
    );

    let output = Command::new("ffmpeg")
        .args(["-y", "-i"])
        .arg(&tmp_path)
        .args(["-vf", &subtitle_filter, "-c:a", "copy"])
        .arg(video_path)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| VidgenError::Ffmpeg(format!("Failed to spawn ffmpeg burn-in: {e}")))?;

    // Remove temp file regardless of success
    let _ = std::fs::remove_file(&tmp_path);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VidgenError::Ffmpeg(format!(
            "FFmpeg subtitle burn-in failed: {}",
            stderr.lines().last().unwrap_or("unknown error")
        )));
    }

    Ok(())
}

/// Apply audio fade-in and/or fade-out to a video file (post-process).
/// Used for project-wide background music fades.
pub fn apply_audio_fades(
    video_path: &Path,
    total_duration: f64,
    fade_in: f64,
    fade_out: f64,
) -> VidgenResult<()> {
    if fade_in <= 0.0 && fade_out <= 0.0 {
        return Ok(());
    }

    let tmp_path = video_path.with_extension("fade-tmp.mp4");
    std::fs::rename(video_path, &tmp_path)?;

    let mut filter_parts = Vec::new();
    if fade_in > 0.0 {
        filter_parts.push(format!("afade=t=in:st=0:d={fade_in:.2}"));
    }
    if fade_out > 0.0 {
        let start = (total_duration - fade_out).max(0.0);
        filter_parts.push(format!("afade=t=out:st={start:.2}:d={fade_out:.2}"));
    }
    let af = filter_parts.join(",");

    let output = Command::new("ffmpeg")
        .args(["-y", "-i"])
        .arg(tmp_path.as_os_str())
        .args(["-c:v", "copy", "-af", &af])
        .arg(video_path.as_os_str())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| VidgenError::Ffmpeg(format!("Failed to spawn ffmpeg fade: {e}")))?;

    let _ = std::fs::remove_file(&tmp_path);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VidgenError::Ffmpeg(format!(
            "FFmpeg audio fade failed: {}",
            stderr.lines().last().unwrap_or("unknown error")
        )));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::VideoConfig;
    use crate::scene::{parse_scene, Scene};
    use std::path::Path;

    fn make_scene(content: &str) -> Scene {
        parse_scene(content, Path::new("test.md")).unwrap()
    }

    #[test]
    fn test_transition_type_from_str() {
        assert_eq!(TransitionType::from_str("fade"), TransitionType::Fade);
        assert_eq!(TransitionType::from_str("Fade"), TransitionType::Fade);
        assert_eq!(
            TransitionType::from_str("slide-left"),
            TransitionType::SlideLeft
        );
        assert_eq!(
            TransitionType::from_str("slide_left"),
            TransitionType::SlideLeft
        );
        assert_eq!(
            TransitionType::from_str("slideright"),
            TransitionType::SlideRight
        );
        assert_eq!(TransitionType::from_str("zoom"), TransitionType::Zoom);
        assert_eq!(TransitionType::from_str("wipe"), TransitionType::Wipe);
        assert_eq!(TransitionType::from_str("none"), TransitionType::None);
        assert_eq!(TransitionType::from_str(""), TransitionType::None);
        assert_eq!(TransitionType::from_str("unknown"), TransitionType::Fade);
    }

    #[test]
    fn test_ffmpeg_name_mapping() {
        assert_eq!(TransitionType::Fade.ffmpeg_name(), "fade");
        assert_eq!(TransitionType::SlideLeft.ffmpeg_name(), "slideleft");
        assert_eq!(TransitionType::SlideRight.ffmpeg_name(), "slideright");
        assert_eq!(TransitionType::Wipe.ffmpeg_name(), "wipeleft");
    }

    #[test]
    fn test_resolve_transition_scene_out_priority() {
        let scene_out = make_scene("---\ntemplate: title-card\ntransition_out: slide-left\n---\nA");
        let scene_in = make_scene("---\ntemplate: title-card\ntransition_in: fade\n---\nB");
        let config = VideoConfig::default();

        let result = resolve_transition(&scene_out, &scene_in, &config).unwrap();
        assert_eq!(result.transition_type, TransitionType::SlideLeft);
    }

    #[test]
    fn test_resolve_transition_scene_in_fallback() {
        let scene_out = make_scene("---\ntemplate: title-card\n---\nA");
        let scene_in = make_scene("---\ntemplate: title-card\ntransition_in: zoom\n---\nB");
        let config = VideoConfig::default();

        let result = resolve_transition(&scene_out, &scene_in, &config).unwrap();
        assert_eq!(result.transition_type, TransitionType::Zoom);
    }

    #[test]
    fn test_resolve_transition_config_default() {
        let scene_out = make_scene("---\ntemplate: title-card\n---\nA");
        let scene_in = make_scene("---\ntemplate: title-card\n---\nB");
        let config = VideoConfig {
            default_transition: Some("wipe".into()),
            default_transition_duration: 0.75,
            ..Default::default()
        };

        let result = resolve_transition(&scene_out, &scene_in, &config).unwrap();
        assert_eq!(result.transition_type, TransitionType::Wipe);
        assert!((result.duration - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn test_resolve_transition_none_when_no_config() {
        let scene_out = make_scene("---\ntemplate: title-card\n---\nA");
        let scene_in = make_scene("---\ntemplate: title-card\n---\nB");
        let config = VideoConfig::default(); // no default_transition

        assert!(resolve_transition(&scene_out, &scene_in, &config).is_none());
    }

    #[test]
    fn test_resolve_transition_explicit_none() {
        let scene_out = make_scene("---\ntemplate: title-card\ntransition_out: none\n---\nA");
        let scene_in = make_scene("---\ntemplate: title-card\n---\nB");
        let config = VideoConfig {
            default_transition: Some("fade".into()),
            ..Default::default()
        };

        // scene_out says "none" explicitly → should return None even though config has a default
        assert!(resolve_transition(&scene_out, &scene_in, &config).is_none());
    }

    #[test]
    fn test_resolve_transition_scene_duration_override() {
        let scene_out = make_scene(
            "---\ntemplate: title-card\ntransition_out: fade\ntransition_duration: 1.5\n---\nA",
        );
        let scene_in = make_scene("---\ntemplate: title-card\n---\nB");
        let config = VideoConfig::default();

        let result = resolve_transition(&scene_out, &scene_in, &config).unwrap();
        assert!((result.duration - 1.5).abs() < f64::EPSILON);
    }
}
