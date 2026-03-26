mod cli;
mod commands;
mod config;
mod error;
mod mcp;
mod render;
mod scene;
mod subtitle;
mod template;
mod tts;

use clap::Parser;
use cli::{Cli, Command};
use colored::*;
use error::VidgenResult;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Initialize tracing based on CLI flags (not for MCP — would corrupt stdio JSON)
    if !matches!(cli.command, Command::Mcp) {
        let log_level = if cli.debug {
            Some("debug")
        } else if cli.verbose {
            Some("info")
        } else {
            // Respect RUST_LOG env var as fallback
            std::env::var("RUST_LOG").ok().map(|_| "")
        };

        if let Some(level) = log_level {
            let filter = if level.is_empty() {
                // RUST_LOG env var is set — use it directly
                tracing_subscriber::EnvFilter::from_default_env()
            } else {
                // CLI flag — set vidgen-specific level
                tracing_subscriber::EnvFilter::new(format!("vidgen={level}"))
            };

            let _ = tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(std::io::stderr)
                .try_init();
        }
    }

    // Export debug settings as env vars so the render pipeline can access them
    if cli.debug {
        std::env::set_var("VIDGEN_DEBUG", "1");
    }
    if let Some(ref dir) = cli.debug_dir {
        std::env::set_var("VIDGEN_DEBUG_DIR", dir.as_os_str());
    }

    if let Err(e) = run(cli).await {
        eprintln!("{} {}", "error:".red().bold(), e);
        if let Some(hint) = e.hint() {
            eprintln!("{} {}", "hint:".yellow().bold(), hint);
        }
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> VidgenResult<()> {
    match cli.command {
        Command::Init { path, preset } => commands::init::run(&path, preset.as_deref()),
        Command::Asset { action } => {
            match action {
                cli::AssetAction::Add {
                    source,
                    project,
                    category,
                } => commands::asset::add(&source, &project, &category),
            }
        }
        Command::Templates { project, output } => {
            commands::templates::run(project.as_deref(), output.as_deref()).await
        }
        Command::Mcp => commands::mcp::run().await,
        Command::Render {
            path,
            fps,
            quality,
            formats,
            scenes,
            subtitles,
            burn_in,
            parallel,
            force_tts,
            no_cache,
            gpu,
            speed,
            crop,
        } => {
            commands::render::run(&path, fps, quality, formats, scenes, subtitles, burn_in, parallel, force_tts, no_cache, gpu, speed, crop.as_deref())
                .await
        }
        Command::Preview {
            path,
            scene,
            frame,
            output,
            all,
            gif,
        } => commands::preview::run(&path, scene, frame, output, all, gif).await,
        Command::Watch {
            path,
            render,
            scene,
        } => commands::watch::run(&path, render, scene).await,
        #[cfg(any(feature = "clipper", feature = "youtube"))]
        Command::Clip { action } => commands::clip::run(action).await,
        Command::QuickRender {
            template,
            output,
            text,
            voice,
            quality,
            props,
        } => {
            // Get text from --text arg or stdin
            let text = match text {
                Some(t) => t,
                None => {
                    use std::io::IsTerminal;
                    if std::io::stdin().is_terminal() {
                        return Err(error::VidgenError::Other(
                            "No text provided. Use --text or pipe text via stdin.".into(),
                        ));
                    }
                    use std::io::Read;
                    let mut buf = String::new();
                    std::io::stdin().read_to_string(&mut buf)?;
                    buf
                }
            };
            commands::quickrender::run(
                &text,
                &template,
                &output,
                voice.as_deref(),
                quality.as_deref(),
                props.as_deref(),
            )
            .await
        }
        Command::Export { path, format } => {
            use cli::ExportAction;
            use commands::export::ExportFormat;
            match format {
                ExportAction::Image { scene, frame, progress, output, all, smart, open } => {
                    commands::export::run(
                        &path, ExportFormat::Png, scene, frame, progress, None, output, all, None, false, smart, open,
                    ).await
                }
                ExportAction::Gif { scene, duration, output, all, width, combined, open } => {
                    commands::export::run(
                        &path, ExportFormat::Gif, scene, 0, None, Some(duration), output, all, width, combined, false, open,
                    ).await
                }
                ExportAction::Webp { scene, duration, output, all, width, open } => {
                    commands::export::run(
                        &path, ExportFormat::Webp, scene, 0, None, Some(duration), output, all, width, false, false, open,
                    ).await
                }
                ExportAction::Mp4 { scene, output, force_tts } => {
                    let idx = scene.unwrap_or(0);
                    commands::render::run(
                        &path, None, None, None, Some(vec![idx]), false, false, None, force_tts, false, false, None, None,
                    ).await?;
                    if let Some(output_path) = output {
                        let cfg = config::load_config(&path)?;
                        let output_rel = cfg.output.directory.strip_prefix("./").unwrap_or(&cfg.output.directory);
                        let output_dir = path.join(output_rel);
                        let project_slug = cfg.project.name
                            .to_lowercase()
                            .replace(|c: char| !c.is_alphanumeric(), "-")
                            .trim_matches('-')
                            .to_string();
                        let rendered = output_dir.join(format!("{project_slug}.mp4"));
                        if rendered.exists() {
                            if let Some(parent) = output_path.parent() {
                                std::fs::create_dir_all(parent)?;
                            }
                            std::fs::rename(&rendered, &output_path)?;
                        }
                    }
                    Ok(())
                }
                ExportAction::Audio { scene, output } => {
                    commands::export::run_audio(&path, scene, output).await
                }
                ExportAction::Subtitles { output } => {
                    commands::export::run_subtitles(&path, output).await
                }
            }
        }
        Command::Info { path } => commands::info::run(&path).await,
        Command::Validate { path } => commands::validate::run(&path),
        Command::Diff { path } => commands::diff::run(&path).await,
        Command::Test { path, update } => commands::test::run(&path, update).await,
    }
}
