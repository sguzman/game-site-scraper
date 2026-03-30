use crate::model;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
pub struct OutputBundleWithNulls {
    pub tool: model::ToolInfo,
    pub stats: model::Stats,
    pub documents: Vec<ParsedDocumentWithNulls>,
    pub errors: Vec<model::ParseError>,
}

impl From<&model::OutputBundle> for OutputBundleWithNulls {
    fn from(v: &model::OutputBundle) -> Self {
        Self {
            tool: v.tool.clone(),
            stats: v.stats.clone(),
            documents: v
                .documents
                .iter()
                .map(ParsedDocumentWithNulls::from)
                .collect(),
            errors: v.errors.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ParsedDocumentWithNulls {
    pub source: model::SourceInfo,
    pub site: String,

    pub page: Option<PageMetaWithNulls>,
    pub post: Option<PostMetaWithNulls>,
    pub release: Option<ReleaseMetaWithNulls>,

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub spoiler_sections: Vec<model::SpoilerSection>,

    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub link_domain_counts: BTreeMap<String, u64>,

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub download_section_headings: Vec<String>,

    pub torrent_file: Option<bool>,

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub torrent_file_names: Vec<String>,

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub torrent_file_links: Vec<String>,

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub magnet_links: Vec<String>,
}

impl From<&model::ParsedDocument> for ParsedDocumentWithNulls {
    fn from(v: &model::ParsedDocument) -> Self {
        Self {
            source: v.source.clone(),
            site: v.site.clone(),
            page: v.page.as_ref().map(PageMetaWithNulls::from),
            post: v.post.as_ref().map(PostMetaWithNulls::from),
            release: v.release.as_ref().map(ReleaseMetaWithNulls::from),
            spoiler_sections: v.spoiler_sections.clone(),
            link_domain_counts: v.link_domain_counts.clone(),
            download_section_headings: v.download_section_headings.clone(),
            torrent_file: v.torrent_file,
            torrent_file_names: v.torrent_file_names.clone(),
            torrent_file_links: v.torrent_file_links.clone(),
            magnet_links: v.magnet_links.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PageMetaWithNulls {
    pub title: Option<String>,
    pub canonical_url: Option<String>,

    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub meta: BTreeMap<String, String>,
}

impl From<&model::PageMeta> for PageMetaWithNulls {
    fn from(v: &model::PageMeta) -> Self {
        Self {
            title: v.title.clone(),
            canonical_url: v.canonical_url.clone(),
            meta: v.meta.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PostMetaWithNulls {
    pub post_id: Option<u64>,

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub categories: Vec<String>,

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub wp_tags: Vec<String>,

    pub entry_title: Option<String>,
    pub entry_datetime: Option<String>,
    pub author: Option<String>,
    pub comments_count: Option<u64>,
}

impl From<&model::PostMeta> for PostMetaWithNulls {
    fn from(v: &model::PostMeta) -> Self {
        Self {
            post_id: v.post_id,
            categories: v.categories.clone(),
            wp_tags: v.wp_tags.clone(),
            entry_title: v.entry_title.clone(),
            entry_datetime: v.entry_datetime.clone(),
            author: v.author.clone(),
            comments_count: v.comments_count,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ReleaseMetaWithNulls {
    pub release_number: Option<u64>,
    pub game_title_line: Option<String>,

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub genres_tags: Vec<String>,

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub companies: Vec<String>,

    pub languages_raw: Option<String>,
    pub original_size_raw: Option<String>,
    pub repack_size_raw: Option<String>,
}

impl From<&model::ReleaseMeta> for ReleaseMetaWithNulls {
    fn from(v: &model::ReleaseMeta) -> Self {
        Self {
            release_number: v.release_number,
            game_title_line: v.game_title_line.clone(),
            genres_tags: v.genres_tags.clone(),
            companies: v.companies.clone(),
            languages_raw: v.languages_raw.clone(),
            original_size_raw: v.original_size_raw.clone(),
            repack_size_raw: v.repack_size_raw.clone(),
        }
    }
}
