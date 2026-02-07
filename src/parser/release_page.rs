use crate::config::Config;
use crate::model::{PageMeta, ParsedDocument, PostMeta, ReleaseMeta, SourceInfo, SpoilerSection};
use crate::parser::util::{bump_domain_count, normalize_ws};
use anyhow::Result;
use once_cell::sync::Lazy;
use regex::Regex;
use scraper::{Html, Selector};
use std::collections::BTreeMap;
use tracing::{debug, instrument, warn};
use url::Url;

static RE_POST_ID: Lazy<Regex> = Lazy::new(|| Regex::new(r"post-(\d+)").expect("valid regex"));
static RE_RELEASE_NO: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"#\s*(\d{1,6})").expect("valid regex"));
static RE_FIRST_INT: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d+)").expect("valid regex"));

#[instrument(level = "debug", skip_all)]
pub fn parse_wordpress_release(html: &str, cfg: &Config) -> Result<ParsedDocument> {
    let doc = Html::parse_document(html);

    let mut out = ParsedDocument {
        source: SourceInfo {
            path: String::new(),
            bytes: 0,
            sha256: String::new(),
        },
        site: "wordpress_release".to_string(),
        page: None,
        post: None,
        release: None,
        spoiler_sections: vec![],
        link_domain_counts: BTreeMap::new(),
        download_section_headings: vec![],
        torrent_file: None,
        torrent_file_names: vec![],
        torrent_file_links: vec![],
        magnet_links: vec![],
    };

    if cfg.scrape.page_title || cfg.scrape.canonical_url || cfg.scrape.meta_tags {
        let mut page = PageMeta {
            title: None,
            canonical_url: None,
            meta: BTreeMap::new(),
        };

        if cfg.scrape.page_title {
            page.title = select_text(&doc, "head > title");
        }
        if cfg.scrape.canonical_url {
            page.canonical_url = select_attr(&doc, "link[rel='canonical']", "href");
        }
        if cfg.scrape.meta_tags {
            page.meta = extract_meta_tags(&doc);
        }

        out.page = Some(page);
    }

    let mut post = PostMeta {
        post_id: None,
        categories: vec![],
        wp_tags: vec![],
        entry_title: None,
        entry_datetime: None,
        author: None,
        comments_count: None,
    };

    if cfg.scrape.post_id || cfg.scrape.wp_tags {
        if let Some(article) = select_attr(&doc, "article[id^='post-']", "id") {
            if cfg.scrape.post_id {
                if let Some(cap) = RE_POST_ID.captures(&article) {
                    post.post_id = cap.get(1).and_then(|m| m.as_str().parse::<u64>().ok());
                }
            }
        }

        if cfg.scrape.wp_tags {
            if let Some(class_attr) = select_attr(&doc, "article[id^='post-']", "class") {
                for tok in class_attr.split_whitespace() {
                    if let Some(tag) = tok.strip_prefix("tag-") {
                        post.wp_tags.push(tag.to_string());
                    }
                }
                post.wp_tags.sort();
                post.wp_tags.dedup();
            }
        }
    }

    if cfg.scrape.categories {
        post.categories = select_all_text(&doc, "span.cat-links a");
    }
    if cfg.scrape.entry_title {
        post.entry_title = select_text(&doc, "h1.entry-title");
    }
    if cfg.scrape.entry_datetime {
        post.entry_datetime = select_attr(&doc, "time.entry-date", "datetime")
            .or_else(|| select_text(&doc, "time.entry-date"));
    }
    if cfg.scrape.author {
        post.author = select_text(&doc, "span.author a");
    }
    if cfg.scrape.comments_count {
        let raw = select_text(&doc, "span.tolstoycomments-cc");
        post.comments_count = raw
            .as_deref()
            .and_then(|s| RE_FIRST_INT.captures(s))
            .and_then(|cap| cap.get(1))
            .and_then(|m| m.as_str().parse::<u64>().ok());
    }

    out.post = Some(post);

    let mut release = ReleaseMeta {
        release_number: None,
        game_title_line: None,
        genres_tags: vec![],
        companies: vec![],
        languages_raw: None,
        original_size_raw: None,
        repack_size_raw: None,
    };

    if cfg.scrape.game_title_line || cfg.scrape.release_number {
        let h3 = select_text(&doc, "div.entry-content > h3");
        if cfg.scrape.game_title_line {
            release.game_title_line = h3.clone();
        }
        if cfg.scrape.release_number {
            if let Some(value) = h3 {
                if let Some(cap) = RE_RELEASE_NO.captures(&value) {
                    release.release_number =
                        cap.get(1).and_then(|m| m.as_str().parse::<u64>().ok());
                }
            }
        }
    }

    if cfg.scrape.genres_tags
        || cfg.scrape.companies
        || cfg.scrape.languages
        || cfg.scrape.original_size
        || cfg.scrape.repack_size
    {
        if let Some(p_html) =
            find_first_paragraph_html_containing(&doc, "div.entry-content p", "Genres/Tags:")
        {
            if cfg.scrape.genres_tags {
                release.genres_tags =
                    extract_anchor_texts_from_fragment(&p_html, "a[href*='/tag/']");
            }

            let p_text = normalize_ws(&html_to_text(&p_html));
            if cfg.scrape.companies {
                if let Some(value) = capture_between_labels(
                    &p_text,
                    "Companies:",
                    &["Languages:", "Original Size:", "Repack Size:"],
                ) {
                    release.companies = split_csvish(&value);
                }
            }
            if cfg.scrape.languages {
                release.languages_raw = capture_between_labels(
                    &p_text,
                    "Languages:",
                    &["Original Size:", "Repack Size:"],
                );
            }
            if cfg.scrape.original_size {
                release.original_size_raw =
                    capture_between_labels(&p_text, "Original Size:", &["Repack Size:"]);
            }
            if cfg.scrape.repack_size {
                release.repack_size_raw = capture_between_labels(&p_text, "Repack Size:", &[]);
            }
        } else {
            warn!("could not find Genres/Tags paragraph; release metadata may be partial");
        }
    }

    out.release = Some(release);

    if cfg.scrape.spoiler_sections {
        out.spoiler_sections = extract_spoilers(&doc, &cfg.profile.spoiler_denylist);
    }

    if cfg.scrape.download_section_presence {
        out.download_section_headings = select_all_text(&doc, "div.entry-content > h3")
            .into_iter()
            .filter(|title| title.to_ascii_lowercase().contains("download mirrors"))
            .collect();
    }

    if cfg.links.domain_counts {
        out.link_domain_counts = extract_domain_counts(&doc, cfg.links.ignore_magnet);
    }

    if cfg.scrape.torrent_file
        || cfg.scrape.torrent_file_name
        || cfg.scrape.torrent_file_link
        || cfg.scrape.magnet
    {
        let extracted = extract_torrent_and_magnet(&doc);
        if cfg.scrape.torrent_file {
            out.torrent_file = Some(!extracted.torrent_file_links.is_empty());
        }
        if cfg.scrape.torrent_file_name {
            out.torrent_file_names = extracted.torrent_file_names;
        }
        if cfg.scrape.torrent_file_link {
            out.torrent_file_links = extracted.torrent_file_links;
        }
        if cfg.scrape.magnet {
            out.magnet_links = extracted.magnet_links;
        }
    }

    Ok(out)
}

#[instrument(level = "debug", skip_all)]
pub fn parse_generic(html: &str, cfg: &Config) -> Result<ParsedDocument> {
    let doc = Html::parse_document(html);

    let mut out = ParsedDocument {
        source: SourceInfo {
            path: String::new(),
            bytes: 0,
            sha256: String::new(),
        },
        site: "generic".to_string(),
        page: None,
        post: None,
        release: None,
        spoiler_sections: vec![],
        link_domain_counts: BTreeMap::new(),
        download_section_headings: vec![],
        torrent_file: None,
        torrent_file_names: vec![],
        torrent_file_links: vec![],
        magnet_links: vec![],
    };

    if cfg.scrape.page_title || cfg.scrape.canonical_url || cfg.scrape.meta_tags {
        let mut page = PageMeta {
            title: None,
            canonical_url: None,
            meta: BTreeMap::new(),
        };

        if cfg.scrape.page_title {
            page.title = select_text(&doc, "head > title");
        }
        if cfg.scrape.canonical_url {
            page.canonical_url = select_attr(&doc, "link[rel='canonical']", "href");
        }
        if cfg.scrape.meta_tags {
            page.meta = extract_meta_tags(&doc);
        }

        out.page = Some(page);
    }

    if cfg.links.domain_counts {
        out.link_domain_counts = extract_domain_counts(&doc, cfg.links.ignore_magnet);
    }

    if cfg.scrape.torrent_file
        || cfg.scrape.torrent_file_name
        || cfg.scrape.torrent_file_link
        || cfg.scrape.magnet
    {
        let extracted = extract_torrent_and_magnet(&doc);
        if cfg.scrape.torrent_file {
            out.torrent_file = Some(!extracted.torrent_file_links.is_empty());
        }
        if cfg.scrape.torrent_file_name {
            out.torrent_file_names = extracted.torrent_file_names;
        }
        if cfg.scrape.torrent_file_link {
            out.torrent_file_links = extracted.torrent_file_links;
        }
        if cfg.scrape.magnet {
            out.magnet_links = extracted.magnet_links;
        }
    }

    Ok(out)
}

struct TorrentMagnetExtract {
    torrent_file_names: Vec<String>,
    torrent_file_links: Vec<String>,
    magnet_links: Vec<String>,
}

fn select_text(doc: &Html, selector: &str) -> Option<String> {
    let selector = Selector::parse(selector).ok()?;
    doc.select(&selector)
        .next()
        .map(|e| normalize_ws(&e.text().collect::<Vec<_>>().join(" ")))
        .filter(|s| !s.is_empty())
}

fn select_all_text(doc: &Html, selector: &str) -> Vec<String> {
    let selector = match Selector::parse(selector) {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    doc.select(&selector)
        .filter_map(|e| {
            let s = normalize_ws(&e.text().collect::<Vec<_>>().join(" "));
            if s.is_empty() { None } else { Some(s) }
        })
        .collect()
}

fn select_attr(doc: &Html, selector: &str, attr: &str) -> Option<String> {
    let selector = Selector::parse(selector).ok()?;
    doc.select(&selector)
        .next()
        .and_then(|e| e.value().attr(attr))
        .map(|s| s.to_string())
}

fn extract_meta_tags(doc: &Html) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let selector = match Selector::parse("meta") {
        Ok(s) => s,
        Err(_) => return out,
    };

    for meta in doc.select(&selector) {
        let node = meta.value();
        let key = node
            .attr("property")
            .or_else(|| node.attr("name"))
            .map(|s| s.to_string());
        let content = node.attr("content").map(|s| s.to_string());

        if let (Some(k), Some(v)) = (key, content) {
            out.entry(k).or_insert(v);
        }
    }

    out
}

fn find_first_paragraph_html_containing(
    doc: &Html,
    selector: &str,
    needle: &str,
) -> Option<String> {
    let selector = Selector::parse(selector).ok()?;
    for p in doc.select(&selector) {
        let text = normalize_ws(&p.text().collect::<Vec<_>>().join(" "));
        if text.contains(needle) {
            return Some(p.inner_html());
        }
    }
    None
}

fn extract_anchor_texts_from_fragment(fragment_html: &str, selector: &str) -> Vec<String> {
    let fragment = Html::parse_fragment(fragment_html);
    let selector = match Selector::parse(selector) {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    let mut out: Vec<String> = fragment
        .select(&selector)
        .filter_map(|a| {
            let text = normalize_ws(&a.text().collect::<Vec<_>>().join(" "));
            if text.is_empty() { None } else { Some(text) }
        })
        .collect();

    out.sort();
    out.dedup();
    out
}

fn html_to_text(fragment_html: &str) -> String {
    let fragment = Html::parse_fragment(fragment_html);
    normalize_ws(&fragment.root_element().text().collect::<Vec<_>>().join(" "))
}

fn capture_between_labels(text: &str, label: &str, next_labels: &[&str]) -> Option<String> {
    let start = text.find(label)? + label.len();
    let mut end = text.len();

    for next in next_labels {
        if let Some(pos) = text[start..].find(next) {
            end = end.min(start + pos);
        }
    }

    let value = text[start..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn split_csvish(s: &str) -> Vec<String> {
    s.split(',')
        .map(|x| x.trim())
        .filter(|x| !x.is_empty())
        .map(|x| x.to_string())
        .collect()
}

fn extract_spoilers(doc: &Html, denylist: &[String]) -> Vec<SpoilerSection> {
    let spoiler_sel = match Selector::parse("div.entry-content div.su-spoiler") {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    let title_sel = Selector::parse("div.su-spoiler-title").ok();
    let content_sel = Selector::parse("div.su-spoiler-content").ok();

    let mut out = Vec::new();

    for sp in doc.select(&spoiler_sel) {
        let title = title_sel
            .as_ref()
            .and_then(|sel| sp.select(sel).next())
            .map(|e| normalize_ws(&e.text().collect::<Vec<_>>().join(" ")))
            .unwrap_or_default();

        let title_l = title.to_ascii_lowercase();
        let denied = denylist
            .iter()
            .any(|term| title_l.contains(&term.to_ascii_lowercase()));
        if denied {
            debug!(title = %title, "skipping spoiler due to denylist");
            continue;
        }

        let text = content_sel
            .as_ref()
            .and_then(|sel| sp.select(sel).next())
            .map(|e| normalize_ws(&e.text().collect::<Vec<_>>().join(" ")))
            .unwrap_or_default();

        if !title.is_empty() && !text.is_empty() {
            out.push(SpoilerSection { title, text });
        }
    }

    out
}

fn extract_domain_counts(doc: &Html, ignore_magnet: bool) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    let selector = match Selector::parse("a[href]") {
        Ok(s) => s,
        Err(_) => return out,
    };

    for a in doc.select(&selector) {
        if let Some(href) = a.value().attr("href") {
            let href_l = href.to_ascii_lowercase();
            if ignore_magnet && href_l.starts_with("magnet:") {
                continue;
            }

            if !(href_l.starts_with("http://") || href_l.starts_with("https://")) {
                continue;
            }

            if let Ok(url) = Url::parse(href) {
                if let Some(host) = url.host_str() {
                    bump_domain_count(&mut out, host);
                }
            }
        }
    }

    out
}

fn extract_torrent_and_magnet(doc: &Html) -> TorrentMagnetExtract {
    let mut names: Vec<String> = Vec::new();
    let mut torrent_links: Vec<String> = Vec::new();
    let mut magnet_links: Vec<String> = Vec::new();

    let selector = match Selector::parse("a[href]") {
        Ok(s) => s,
        Err(_) => {
            return TorrentMagnetExtract {
                torrent_file_names: names,
                torrent_file_links: torrent_links,
                magnet_links,
            };
        }
    };

    for a in doc.select(&selector) {
        let href = match a.value().attr("href") {
            Some(h) => h.trim(),
            None => continue,
        };
        let href_l = href.to_ascii_lowercase();
        let text = normalize_ws(&a.text().collect::<Vec<_>>().join(" "));

        if href_l.starts_with("magnet:") {
            magnet_links.push(href.to_string());
            continue;
        }

        let is_http = href_l.starts_with("http://") || href_l.starts_with("https://");
        let text_l = text.to_ascii_lowercase();
        let looks_torrent = href_l.contains(".torrent")
            || text_l.contains(".torrent")
            || text_l.contains("torrent file");
        if is_http && looks_torrent {
            torrent_links.push(href.to_string());
            if !text.is_empty() {
                names.push(text);
            }
        }
    }

    names.sort();
    names.dedup();
    torrent_links.sort();
    torrent_links.dedup();
    magnet_links.sort();
    magnet_links.dedup();

    TorrentMagnetExtract {
        torrent_file_names: names,
        torrent_file_links: torrent_links,
        magnet_links,
    }
}
