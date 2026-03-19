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
        } => {
            commands::render::run(&path, fps, quality, formats, scenes, subtitles, burn_in, parallel, force_tts)
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
    }
}
