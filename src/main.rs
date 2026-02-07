#![forbid(unsafe_code)]

mod cli;
mod config;
mod fs;
mod model;
mod parser;

use anyhow::{Context, Result};
use clap::Parser;
use std::io::Write;
use tracing::{info, warn};

fn main() -> Result<()> {
    let cli = cli::Cli::parse();

    cli::init_tracing(&cli).context("init tracing")?;

    match &cli.command {
        cli::Command::InitConfig(args) => {
            config::write_default_config(&args.path)
                .with_context(|| format!("write default config to {}", args.path.display()))?;
            info!(path = %args.path.display(), "wrote default config");
        }
        cli::Command::PrintConfig(args) => {
            let cfg = config::Config::load(cli.config.as_deref())?;
            let toml = cfg.to_pretty_toml()?;
            if let Some(path) = &args.output {
                std::fs::write(path, toml)
                    .with_context(|| format!("write config to {}", path.display()))?;
                info!(path = %path.display(), "wrote effective config");
            } else {
                print!("{toml}");
            }
        }
        cli::Command::Completions(args) => {
            cli::print_completions(args.shell);
        }
        cli::Command::Parse(args) => {
            let cfg = config::Config::load(cli.config.as_deref())?;
            let files = fs::collect_html_inputs(&args.inputs, args.recursive, args.follow_symlinks)
                .context("collect inputs")?;

            if files.is_empty() {
                warn!("no input HTML files found");
            } else {
                info!(count = files.len(), "collected input HTML files");
            }

            let bundle = parser::parse_many(&files, &cfg).context("parse inputs")?;
            let json = if args.pretty || cfg.output.pretty_json {
                serde_json::to_string_pretty(&bundle)?
            } else {
                serde_json::to_string(&bundle)?
            };

            match &args.output {
                Some(path) => {
                    std::fs::write(path, json)
                        .with_context(|| format!("write output to {}", path.display()))?;
                    info!(path = %path.display(), "wrote JSON output");
                }
                None => {
                    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
                    out.write_all(json.as_bytes())?;
                    out.write_all(b"\n")?;
                    out.flush()?;
                }
            }
        }
    }

    Ok(())
}
