use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputBundle {
    pub tool: ToolInfo,
    pub stats: Stats,
    pub documents: Vec<ParsedDocument>,
    pub errors: Vec<ParseError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stats {
    pub input_count: usize,
    pub parsed_ok: usize,
    pub parsed_err: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseError {
    pub path: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedDocument {
    pub source: SourceInfo,
    pub site: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<PageMeta>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub post: Option<PostMeta>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub release: Option<ReleaseMeta>,

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub spoiler_sections: Vec<SpoilerSection>,

    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub link_domain_counts: BTreeMap<String, u64>,

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub download_section_headings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceInfo {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_url: Option<String>,

    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub meta: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_id: Option<u64>,

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub categories: Vec<String>,

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub wp_tags: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_title: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_datetime: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub comments_count: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_number: Option<u64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_title_line: Option<String>,

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub genres_tags: Vec<String>,

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub companies: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub languages_raw: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_size_raw: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub repack_size_raw: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpoilerSection {
    pub title: String,
    pub text: String,
}
