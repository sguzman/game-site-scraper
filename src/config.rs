use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{debug, instrument};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub output: OutputConfig,
    pub scrape: ScrapeConfig,
    pub links: LinkConfig,
    pub profile: ProfileConfig,
    pub meilisearch: MeilisearchConfig,
}

impl Config {
    #[instrument(level = "debug", skip_all, fields(path = path.map(|p| p.display().to_string())))]
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let mut cfg = Config::default();

        if let Some(path) = path {
            if path.exists() {
                debug!(path = %path.display(), "loading config");
                let raw = std::fs::read_to_string(path)
                    .with_context(|| format!("read config {}", path.display()))?;
                let parsed: Config = toml::from_str(&raw)
                    .with_context(|| format!("parse TOML {}", path.display()))?;
                cfg = parsed;
            } else {
                debug!(path = %path.display(), "config file not found; using defaults");
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
    pub ndjson: bool,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            pretty_json: true,
            include_nulls: false,
            ndjson: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapeConfig {
    pub page_title: bool,
    pub canonical_url: bool,
    pub meta_tags: bool,
    pub cover_image: bool,

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
    pub torrent_file: bool,
    pub torrent_file_name: bool,
    pub torrent_file_link: bool,
    pub magnet: bool,
}

impl Default for ScrapeConfig {
    fn default() -> Self {
        Self {
            page_title: true,
            canonical_url: true,
            meta_tags: true,
            cover_image: true,

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
            torrent_file: true,
            torrent_file_name: true,
            torrent_file_link: true,
            magnet: true,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeilisearchMode {
    Upsert,
    CleanInsert,
}

impl Default for MeilisearchMode {
    fn default() -> Self {
        Self::Upsert
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeilisearchConfig {
    pub host: String,
    pub api_key: Option<String>,
    pub index_uid: String,
    pub primary_key: String,
    pub batch_size: usize,
    pub timeout_secs: u64,
    pub mode: MeilisearchMode,
}

impl Default for MeilisearchConfig {
    fn default() -> Self {
        Self {
            host: "http://127.0.0.1:7700".to_string(),
            api_key: None,
            index_uid: "fitgirl-games".to_string(),
            primary_key: "id".to_string(),
            batch_size: 1000,
            timeout_secs: 120,
            mode: MeilisearchMode::Upsert,
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
ndjson = false

[scrape]
page_title = true
canonical_url = true
meta_tags = true
cover_image = true

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
torrent_file = true
torrent_file_name = true
torrent_file_link = true
magnet = true

[links]
domain_counts = true
ignore_magnet = true

[profile]
wordpress_release_layout = true
spoiler_denylist = ["click to show direct links", "direct links", "magnet", "torrent"]

[meilisearch]
host = "http://127.0.0.1:7700"
api_key = ""
index_uid = "fitgirl-games"
primary_key = "id"
batch_size = 1000
timeout_secs = 120
mode = "upsert"
"#;
