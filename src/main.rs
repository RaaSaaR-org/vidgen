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

    // Initialize tracing for non-MCP commands, gated on RUST_LOG env var
    if !matches!(cli.command, Command::Mcp) && std::env::var("RUST_LOG").is_ok() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_writer(std::io::stderr)
            .try_init();
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
        Command::Init { path } => commands::init::run(&path),
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
        } => {
            commands::render::run(&path, fps, quality, formats, scenes, subtitles, burn_in, parallel)
                .await
        }
        Command::Preview {
            path,
            scene,
            frame,
            output,
        } => commands::preview::run(&path, scene, frame, output).await,
        Command::Watch {
            path,
            render,
            scene,
        } => commands::watch::run(&path, render, scene).await,
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
