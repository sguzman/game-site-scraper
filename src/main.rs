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
            let use_ndjson = args.ndjson || cfg.output.ndjson;

            match &args.output {
                Some(path) => {
                    let mut out = std::io::BufWriter::new(
                        std::fs::File::create(path)
                            .with_context(|| format!("create output {}", path.display()))?,
                    );
                    write_output(
                        &mut out,
                        &bundle,
                        args.pretty || cfg.output.pretty_json,
                        use_ndjson,
                    )?;
                    out.flush()?;
                    info!(path = %path.display(), ndjson = use_ndjson, "wrote output");
                }
                None => {
                    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
                    write_output(
                        &mut out,
                        &bundle,
                        args.pretty || cfg.output.pretty_json,
                        use_ndjson,
                    )?;
                    out.flush()?;
                }
            }
        }
    }

    Ok(())
}

fn write_output<W: Write>(
    out: &mut W,
    bundle: &model::OutputBundle,
    pretty_json: bool,
    ndjson: bool,
) -> Result<()> {
    if ndjson {
        for doc in &bundle.documents {
            let line = serde_json::to_string(doc)?;
            out.write_all(line.as_bytes())?;
            out.write_all(b"\n")?;
        }
        for err in &bundle.errors {
            let line = serde_json::json!({
                "type": "error",
                "data": err
            })
            .to_string();
            out.write_all(line.as_bytes())?;
            out.write_all(b"\n")?;
        }
        let summary = serde_json::json!({
            "type": "summary",
            "data": &bundle.stats
        })
        .to_string();
        out.write_all(summary.as_bytes())?;
        out.write_all(b"\n")?;
        return Ok(());
    }

    let json = if pretty_json {
        serde_json::to_string_pretty(bundle)?
    } else {
        serde_json::to_string(bundle)?
    };
    out.write_all(json.as_bytes())?;
    out.write_all(b"\n")?;
    Ok(())
}
