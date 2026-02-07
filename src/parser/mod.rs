pub mod release_page;
pub mod util;

use crate::config::Config;
use crate::model::{OutputBundle, ParseError, ParsedDocument, Stats, ToolInfo};
use anyhow::{Context, Result};
use std::path::PathBuf;
use tracing::{info, instrument, warn};

#[instrument(level = "info", skip_all, fields(file_count = files.len()))]
pub fn parse_many(files: &[PathBuf], cfg: &Config) -> Result<OutputBundle> {
    let mut docs: Vec<ParsedDocument> = Vec::with_capacity(files.len());
    let mut errs: Vec<ParseError> = Vec::new();

    for p in files {
        match parse_one(p, cfg) {
            Ok(doc) => docs.push(doc),
            Err(err) => {
                warn!(path = %p.display(), error = %format!("{err:#}"), "parse failed");
                errs.push(ParseError {
                    path: p.display().to_string(),
                    error: format!("{err:#}"),
                });
            }
        }
    }

    let stats = Stats {
        input_count: files.len(),
        parsed_ok: docs.len(),
        parsed_err: errs.len(),
    };

    info!(?stats, "parse summary");

    Ok(OutputBundle {
        tool: ToolInfo {
            name: env!("CARGO_PKG_NAME").to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
        stats,
        documents: docs,
        errors: errs,
    })
}

#[instrument(level = "debug", skip_all, fields(path = %path.display()))]
fn parse_one(path: &PathBuf, cfg: &Config) -> Result<ParsedDocument> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let bytes_len = bytes.len() as u64;
    let sha256 = util::sha256_hex(&bytes);

    let html = String::from_utf8(bytes).context("input is not valid UTF-8")?;

    let is_wp_release = cfg.profile.wordpress_release_layout
        && html.contains("article id=\"post-")
        && html.contains("entry-content");

    let mut doc = if is_wp_release {
        release_page::parse_wordpress_release(&html, cfg).context("wordpress-release parse")?
    } else {
        release_page::parse_generic(&html, cfg).context("generic parse")?
    };

    doc.source.path = path.display().to_string();
    doc.source.bytes = bytes_len;
    doc.source.sha256 = sha256;
    doc.site = if is_wp_release {
        "wordpress_release".to_string()
    } else {
        "generic".to_string()
    };

    Ok(doc)
}
