use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub output: OutputConfig,
    pub scrape: ScrapeConfig,
    pub links: LinkConfig,
    pub profile: ProfileConfig,
}

impl Config {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let mut cfg = Config::default();

        if let Some(path) = path {
            if path.exists() {
                let raw = std::fs::read_to_string(path)
                    .with_context(|| format!("read config {}", path.display()))?;
                let parsed: Config = toml::from_str(&raw)
                    .with_context(|| format!("parse TOML {}", path.display()))?;
                cfg = parsed;
            }
        }

        Ok(cfg)
    }

    pub fn to_pretty_toml(&self) -> Result<String> {
        toml::to_string_pretty(self).context("serialize config to TOML")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    pub pretty_json: bool,
    pub include_nulls: bool,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            pretty_json: true,
            include_nulls: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapeConfig {
    pub page_title: bool,
    pub canonical_url: bool,
    pub meta_tags: bool,

    pub post_id: bool,
    pub categories: bool,
    pub wp_tags: bool,

    pub entry_title: bool,
    pub entry_datetime: bool,
    pub author: bool,
    pub comments_count: bool,

    pub release_number: bool,
    pub game_title_line: bool,
    pub genres_tags: bool,
    pub companies: bool,
    pub languages: bool,
    pub original_size: bool,
    pub repack_size: bool,

    pub spoiler_sections: bool,
    pub download_section_presence: bool,
}

impl Default for ScrapeConfig {
    fn default() -> Self {
        Self {
            page_title: true,
            canonical_url: true,
            meta_tags: true,

            post_id: true,
            categories: true,
            wp_tags: true,

            entry_title: true,
            entry_datetime: true,
            author: true,
            comments_count: true,

            release_number: true,
            game_title_line: true,
            genres_tags: true,
            companies: true,
            languages: true,
            original_size: true,
            repack_size: true,

            spoiler_sections: true,
            download_section_presence: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkConfig {
    pub domain_counts: bool,
    pub ignore_magnet: bool,
}

impl Default for LinkConfig {
    fn default() -> Self {
        Self {
            domain_counts: true,
            ignore_magnet: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileConfig {
    pub wordpress_release_layout: bool,
    pub spoiler_denylist: Vec<String>,
}

impl Default for ProfileConfig {
    fn default() -> Self {
        Self {
            wordpress_release_layout: true,
            spoiler_denylist: vec![
                "click to show direct links".into(),
                "direct links".into(),
                "magnet".into(),
                "torrent".into(),
            ],
        }
    }
}

pub fn write_default_config(path: &PathBuf) -> Result<()> {
    std::fs::write(path, DEFAULT_CONFIG_TOML).context("write default config template")?;
    Ok(())
}

const DEFAULT_CONFIG_TOML: &str = r#"# game-scraper configuration
# Field toggles let you control exactly what is extracted.

[output]
pretty_json = true
include_nulls = false

[scrape]
page_title = true
canonical_url = true
meta_tags = true

post_id = true
categories = true
wp_tags = true

entry_title = true
entry_datetime = true
author = true
comments_count = true

release_number = true
game_title_line = true
genres_tags = true
companies = true
languages = true
original_size = true
repack_size = true

spoiler_sections = true
download_section_presence = true

[links]
domain_counts = true
ignore_magnet = true

[profile]
wordpress_release_layout = true
spoiler_denylist = ["click to show direct links", "direct links", "magnet", "torrent"]
"#;
