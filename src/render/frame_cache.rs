/// Check if a rendered HTML scene is static (doesn't use animation variables).
///
/// Static scenes render the same PNG for every frame, so we can capture
/// just one screenshot and tell FFmpeg to loop it, saving significant time.
pub fn is_static_scene(html: &str) -> bool {
    !html.contains("--frame") && !html.contains("--progress") && !html.contains("--total-frames")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_static_scene_with_animation() {
        let html = r#"<div style="opacity: calc(var(--progress) * 1)">Hello</div>"#;
        assert!(!is_static_scene(html));
    }

    #[test]
    fn test_is_static_scene_static() {
        let html = r#"<div style="color: red">Hello World</div>"#;
        assert!(is_static_scene(html));
    }

    #[test]
    fn test_is_static_scene_frame_var() {
        let html = r#"<span style="--word-delay: calc(var(--frame) / 30)">word</span>"#;
        assert!(!is_static_scene(html));
    }

    #[test]
    fn test_is_static_scene_total_frames_var() {
        let html = r#"<style>:root { --total-frames: 150; }</style>"#;
        assert!(!is_static_scene(html));
    }
}
