use crate::config;
use crate::error::VidgenResult;
use crate::scene::{self, Scene, SceneDuration};
use crate::template::TemplateRegistry;
use colored::*;
use std::path::Path;

struct ValidationResult {
    errors: Vec<String>,
    warnings: Vec<String>,
}

impl ValidationResult {
    fn new() -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn error(&mut self, msg: impl Into<String>) {
        self.errors.push(msg.into());
    }

    fn warning(&mut self, msg: impl Into<String>) {
        self.warnings.push(msg.into());
    }
}

pub fn run(project_path: &Path) -> VidgenResult<()> {
    let project_name = project_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    eprintln!("Validating \"{}\"...", project_name);

    let mut result = ValidationResult::new();

    // 1. Config loads
    let config = match config::load_config(project_path) {
        Ok(cfg) => {
            if let Err(e) = cfg.validate() {
                eprintln!("  {} Config: {}", "\u{2717}".red(), e);
                result.error(format!("Config validation: {e}"));
                None
            } else {
                eprintln!("  {} Config valid", "\u{2713}".green());
                Some(cfg)
            }
        }
        Err(e) => {
            eprintln!("  {} Config: {}", "\u{2717}".red(), e);
            result.error(format!("Config load: {e}"));
            None
        }
    };

    // 2. Scenes load
    let scenes = match scene::load_scenes(project_path) {
        Ok(scenes) => {
            eprintln!("  {} {} scenes loaded", "\u{2713}".green(), scenes.len());
            Some(scenes)
        }
        Err(e) => {
            eprintln!("  {} Scenes: {}", "\u{2717}".red(), e);
            result.error(format!("Scene load: {e}"));
            None
        }
    };

    // 3. Templates exist
    if let Some(ref scenes) = scenes {
        check_templates(project_path, scenes, &mut result);
    }

    // 4. Assets referenced
    if let Some(ref scenes) = scenes {
        check_asset_references(project_path, scenes, &mut result);
    }

    // 5. Background music
    if let Some(ref cfg) = config {
        check_background_music(project_path, cfg, &mut result);
    }

    // 6. Duration warnings
    if let Some(ref scenes) = scenes {
        check_duration_warnings(scenes, &mut result);
    }

    // 7. Font check
    if let Some(ref scenes) = scenes {
        check_fonts(project_path, scenes, &mut result);
    }

    // 8. Contrast check
    if let Some(ref cfg) = config {
        check_contrast(&cfg.theme, &mut result);
    }

    // Summary
    let errors = result.errors.len();
    let warnings = result.warnings.len();
    eprintln!();
    if errors == 0 && warnings == 0 {
        eprintln!("  {}: no issues found", "Result".green().bold());
    } else {
        eprintln!(
            "  {}: {} error(s), {} warning(s)",
            "Result".cyan().bold(),
            if errors > 0 {
                format!("{errors}").red().bold().to_string()
            } else {
                "0".to_string()
            },
            if warnings > 0 {
                format!("{warnings}").yellow().bold().to_string()
            } else {
                "0".to_string()
            },
        );
    }

    Ok(())
}

fn check_templates(project_path: &Path, scenes: &[Scene], result: &mut ValidationResult) {
    let registry = match TemplateRegistry::new() {
        Ok(mut reg) => {
            let _ = reg.register_project_templates(project_path);
            reg
        }
        Err(e) => {
            eprintln!("  {} Template registry: {}", "\u{2717}".red(), e);
            result.error(format!("Template registry: {e}"));
            return;
        }
    };

    let mut all_found = true;
    for scene in scenes {
        // Skip video-clip and sequence-only scenes (no template needed)
        if scene.is_video_clip() || scene.is_sequence() {
            continue;
        }
        let template_name = &scene.frontmatter.template;
        if template_name.is_empty() {
            continue;
        }
        if !registry.has_template(template_name) {
            let scene_name = scene
                .source_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");
            eprintln!(
                "  {} Template not found: \"{}\" (scene {})",
                "\u{2717}".red(),
                template_name,
                scene_name
            );
            result.error(format!(
                "Template \"{}\" not found (scene {})",
                template_name, scene_name
            ));
            all_found = false;
        }
    }
    if all_found {
        eprintln!("  {} All templates found", "\u{2713}".green());
    }
}

fn check_asset_references(project_path: &Path, scenes: &[Scene], result: &mut ValidationResult) {
    let mut all_found = true;
    for scene in scenes {
        let scene_name = scene
            .source_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        // Check props for @assets/ references
        for (key, value) in &scene.frontmatter.props {
            check_asset_value(project_path, value, scene_name, key, result, &mut all_found);
        }

        // Check video_source
        if let Some(ref src) = scene.frontmatter.video_source {
            if let Some(suffix) = src.strip_prefix("@assets/") {
                let path = project_path.join("assets").join(suffix);
                if !path.exists() {
                    eprintln!(
                        "  {} Asset not found: {} (scene {}, video_source)",
                        "\u{2717}".red(),
                        path.display(),
                        scene_name
                    );
                    result.error(format!(
                        "Asset not found: {} (scene {}, video_source)",
                        path.display(),
                        scene_name
                    ));
                    all_found = false;
                }
            }
        }

        // Check background image
        if let Some(ref bg) = scene.frontmatter.background {
            if let Some(ref img) = bg.image {
                if let Some(suffix) = img.strip_prefix("@assets/") {
                    let path = project_path.join("assets").join(suffix);
                    if !path.exists() {
                        eprintln!(
                            "  {} Asset not found: {} (scene {}, background.image)",
                            "\u{2717}".red(),
                            path.display(),
                            scene_name
                        );
                        result.error(format!(
                            "Asset not found: {} (scene {}, background.image)",
                            path.display(),
                            scene_name
                        ));
                        all_found = false;
                    }
                }
            }
        }
    }
    if all_found {
        eprintln!("  {} All asset references valid", "\u{2713}".green());
    }
}

fn check_asset_value(
    project_path: &Path,
    value: &serde_json::Value,
    scene_name: &str,
    context: &str,
    result: &mut ValidationResult,
    all_found: &mut bool,
) {
    match value {
        serde_json::Value::String(s) => {
            if let Some(suffix) = s.strip_prefix("@assets/") {
                let path = project_path.join("assets").join(suffix);
                if !path.exists() {
                    eprintln!(
                        "  {} Asset not found: {} (scene {}, props.{})",
                        "\u{2717}".red(),
                        path.display(),
                        scene_name,
                        context
                    );
                    result.error(format!(
                        "Asset not found: {} (scene {}, props.{})",
                        path.display(),
                        scene_name,
                        context
                    ));
                    *all_found = false;
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                check_asset_value(project_path, item, scene_name, context, result, all_found);
            }
        }
        serde_json::Value::Object(obj) => {
            for (k, v) in obj {
                check_asset_value(
                    project_path,
                    v,
                    scene_name,
                    &format!("{context}.{k}"),
                    result,
                    all_found,
                );
            }
        }
        _ => {}
    }
}

fn check_background_music(
    project_path: &Path,
    config: &config::ProjectConfig,
    result: &mut ValidationResult,
) {
    if let Some(ref bg) = config.audio.background {
        let resolved = scene::resolve_asset_path(&bg.file, project_path);
        if resolved.exists() {
            let filename = resolved
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(&bg.file);
            eprintln!("  {} Background music: {}", "\u{2713}".green(), filename);
        } else {
            eprintln!(
                "  {} Background music not found: {}",
                "\u{2717}".red(),
                resolved.display()
            );
            result.error(format!(
                "Background music not found: {}",
                resolved.display()
            ));
        }
    }
}

fn check_duration_warnings(scenes: &[Scene], result: &mut ValidationResult) {
    for (i, scene) in scenes.iter().enumerate() {
        if let SceneDuration::Fixed(duration) = &scene.frontmatter.duration {
            let script = scene.script.trim();
            if !script.is_empty() {
                let word_count = script.split_whitespace().count();
                if word_count > 10 {
                    let scene_name = scene
                        .source_path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown");
                    eprintln!(
                        "  {} Scene {:02} ({}): fixed duration {:.0}s with {} words (may cut off voiceover)",
                        "\u{26A0}".yellow(),
                        i + 1,
                        scene_name,
                        duration,
                        word_count
                    );
                    result.warning(format!(
                        "Scene {:02} ({}): fixed duration {:.0}s with {} words",
                        i + 1,
                        scene_name,
                        duration,
                        word_count
                    ));
                }
            }
        }
    }
}

fn check_fonts(project_path: &Path, scenes: &[Scene], result: &mut ValidationResult) {
    let mut checked_fonts: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut all_found = true;

    // Collect template HTML files to scan for font references
    let components_dir = project_path.join("templates").join("components");
    let mut html_files: Vec<std::path::PathBuf> = Vec::new();
    if components_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&components_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "html") {
                    html_files.push(path);
                }
            }
        }
    }

    // Also check styles directory
    let styles_dir = project_path.join("styles");
    if styles_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&styles_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path
                    .extension()
                    .is_some_and(|ext| ext == "css" || ext == "html")
                {
                    html_files.push(path);
                }
            }
        }
    }

    // Also scan scene props for template names and check those template files
    for scene in scenes {
        let template_name = &scene.frontmatter.template;
        if !template_name.is_empty() {
            let template_file = components_dir.join(format!("{template_name}.html"));
            if template_file.exists() && !html_files.contains(&template_file) {
                html_files.push(template_file);
            }
        }
    }

    for html_file in &html_files {
        let content = match std::fs::read_to_string(html_file) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Find all file:/// URLs that look like font paths
        for cap in find_file_urls(&content) {
            if checked_fonts.contains(&cap) {
                continue;
            }
            checked_fonts.insert(cap.clone());

            let font_path = Path::new(&cap);
            if !font_path.exists() {
                eprintln!("  {} Font not found: {}", "\u{2717}".red(), cap);
                result.error(format!("Font not found: {cap}"));
                all_found = false;
            }
        }
    }

    if all_found && !checked_fonts.is_empty() {
        eprintln!(
            "  {} All fonts found ({} checked)",
            "\u{2713}".green(),
            checked_fonts.len()
        );
    }
}

/// Extract `file:///...` URLs from HTML/CSS content (typically font references).
fn find_file_urls(content: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let prefix = "file:///";
    let mut search_from = 0;
    while let Some(start) = content[search_from..].find(prefix) {
        let abs_start = search_from + start;
        // Find the end of the URL (quote, paren, whitespace, or semicolon)
        let url_start = abs_start + prefix.len();
        let end = content[url_start..]
            .find(['"', '\'', ')', ' ', ';'])
            .map(|e| url_start + e)
            .unwrap_or(content.len());
        let url_path = &content[abs_start + "file://".len()..end];
        // Only include paths that look like font files
        let lower = url_path.to_lowercase();
        if lower.ends_with(".ttf")
            || lower.ends_with(".otf")
            || lower.ends_with(".woff")
            || lower.ends_with(".woff2")
        {
            urls.push(url_path.to_string());
        }
        search_from = end;
    }
    urls
}

// ---------------------------------------------------------------------------
// Accessibility: WCAG contrast ratio checks
// ---------------------------------------------------------------------------

fn check_contrast(theme: &config::ThemeConfig, result: &mut ValidationResult) {
    let bg = parse_hex_color(&theme.background);
    let text = parse_hex_color(&theme.text);
    let primary = parse_hex_color(&theme.primary);

    let mut warnings = Vec::new();

    let ratio_text = contrast_ratio(bg, text);
    if ratio_text < 4.5 {
        warnings.push(format!(
            "Text on background: {:.1}:1 (minimum 4.5:1)",
            ratio_text
        ));
    }

    let ratio_primary = contrast_ratio(bg, primary);
    if ratio_primary < 3.0 {
        warnings.push(format!(
            "Primary on background: {:.1}:1 (minimum 3.0:1 for large text)",
            ratio_primary
        ));
    }

    if warnings.is_empty() {
        eprintln!(
            "  {} Contrast ratios OK (text {:.1}:1, primary {:.1}:1)",
            "\u{2713}".green(),
            ratio_text,
            ratio_primary
        );
    } else {
        for w in &warnings {
            eprintln!("  {} Contrast: {}", "\u{26A0}".yellow(), w);
            result.warning(format!("Contrast: {w}"));
        }
    }
}

fn parse_hex_color(hex: &str) -> (f64, f64, f64) {
    let hex = hex.trim_start_matches('#');
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0) as f64 / 255.0;
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0) as f64 / 255.0;
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0) as f64 / 255.0;
    (r, g, b)
}

fn relative_luminance(r: f64, g: f64, b: f64) -> f64 {
    let r = if r <= 0.03928 {
        r / 12.92
    } else {
        ((r + 0.055) / 1.055).powf(2.4)
    };
    let g = if g <= 0.03928 {
        g / 12.92
    } else {
        ((g + 0.055) / 1.055).powf(2.4)
    };
    let b = if b <= 0.03928 {
        b / 12.92
    } else {
        ((b + 0.055) / 1.055).powf(2.4)
    };
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn contrast_ratio(c1: (f64, f64, f64), c2: (f64, f64, f64)) -> f64 {
    let l1 = relative_luminance(c1.0, c1.1, c1.2);
    let l2 = relative_luminance(c2.0, c2.1, c2.2);
    let (lighter, darker) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
    (lighter + 0.05) / (darker + 0.05)
}
