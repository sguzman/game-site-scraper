use crate::cli::{MeilisearchArgs, MeilisearchModeArg};
use crate::config::{Config, MeilisearchMode, MeilisearchSettingsConfig};
use crate::fs;
use crate::model::ParsedDocument;
use crate::parser;
use anyhow::{anyhow, bail, Context, Result};
use meilisearch_sdk::client::Client;
use meilisearch_sdk::errors::{Error as MeiliError, ErrorCode, ErrorType};
use meilisearch_sdk::settings::Settings;
use meilisearch_sdk::task_info::TaskInfo;
use meilisearch_sdk::tasks::Task;
use serde::Serialize;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, instrument, warn};

const TASK_POLL_INTERVAL_MS: u64 = 200;
const RETRY_ATTEMPTS: usize = 3;
const RETRY_BASE_DELAY_MS: u64 = 250;

#[derive(Debug, Clone)]
struct ResolvedMeilisearch {
    host: String,
    api_key: Option<String>,
    index_uid: String,
    primary_key: String,
    batch_size: usize,
    timeout_secs: u64,
    mode: MeilisearchMode,
    apply_settings_on_existing: bool,
    settings: MeilisearchSettingsConfig,
    dry_run: bool,
}

#[derive(Debug, Clone, Serialize)]
struct MeiliDocument {
    id: String,
    title: String,
    poster: Option<String>,
    site: String,
    source_path: String,
    canonical_url: Option<String>,
    categories: Vec<String>,
    wp_tags: Vec<String>,
    genres: Vec<String>,
    companies: Vec<String>,
    languages_raw: Option<String>,
    original_size_raw: Option<String>,
    repack_size_raw: Option<String>,
    release_number: Option<u64>,
    entry_datetime: Option<String>,
    author: Option<String>,
    torrent_file: Option<bool>,
}

pub fn run(args: &MeilisearchArgs, cfg: &Config) -> Result<()> {
    let settings = resolve_settings(args, cfg)?;

    let files = fs::collect_html_inputs(&args.inputs, args.recursive, args.follow_symlinks)
        .context("collect inputs")?;

    if files.is_empty() {
        warn!("no input HTML files found");
        return Ok(());
    }

    info!(count = files.len(), "collected input HTML files");

    let bundle = parser::parse_many(&files, cfg).context("parse inputs")?;

    if !bundle.errors.is_empty() {
        warn!(
            error_count = bundle.errors.len(),
            "some documents failed to parse; continuing with parsed documents"
        );
    }

    let documents: Vec<MeiliDocument> = bundle.documents.iter().map(map_document).collect();

    if documents.is_empty() {
        warn!("no parsed documents to index");
        return Ok(());
    }

    info!(
        count = documents.len(),
        parsed_ok = bundle.stats.parsed_ok,
        parsed_err = bundle.stats.parsed_err,
        "prepared documents for indexing"
    );

    if settings.dry_run {
        info!(
            parsed_ok = bundle.stats.parsed_ok,
            parsed_err = bundle.stats.parsed_err,
            indexed_count = documents.len(),
            "dry run enabled; skipping indexing"
        );
        return Ok(());
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?;

    runtime.block_on(async { run_indexing(&settings, &documents).await })?;

    info!(
        parsed_ok = bundle.stats.parsed_ok,
        parsed_err = bundle.stats.parsed_err,
        indexed_count = documents.len(),
        "meilisearch summary"
    );

    Ok(())
}

fn resolve_settings(args: &MeilisearchArgs, cfg: &Config) -> Result<ResolvedMeilisearch> {
    let mut settings = ResolvedMeilisearch {
        host: cfg.meilisearch.host.clone(),
        api_key: cfg.meilisearch.api_key.clone(),
        index_uid: cfg.meilisearch.index_uid.clone(),
        primary_key: cfg.meilisearch.primary_key.clone(),
        batch_size: cfg.meilisearch.batch_size,
        timeout_secs: cfg.meilisearch.timeout_secs,
        mode: cfg.meilisearch.mode,
        apply_settings_on_existing: cfg.meilisearch.apply_settings_on_existing,
        settings: cfg.meilisearch.settings.clone(),
        dry_run: false,
    };

    if let Some(host) = &args.host {
        debug!(override_value = %host, "overriding meilisearch host");
        settings.host = host.clone();
    }
    if let Some(index) = &args.index {
        debug!(override_value = %index, "overriding meilisearch index uid");
        settings.index_uid = index.clone();
    }
    if let Some(api_key) = &args.api_key {
        debug!("overriding meilisearch api key");
        settings.api_key = Some(api_key.clone());
    }
    if let Some(primary_key) = &args.primary_key {
        debug!(override_value = %primary_key, "overriding meilisearch primary key");
        settings.primary_key = primary_key.clone();
    }
    if let Some(batch_size) = args.batch_size {
        debug!(override_value = batch_size, "overriding meilisearch batch size");
        settings.batch_size = batch_size;
    }
    if let Some(timeout_secs) = args.timeout_secs {
        debug!(override_value = timeout_secs, "overriding meilisearch timeout");
        settings.timeout_secs = timeout_secs;
    }
    if args.apply_settings {
        settings.apply_settings_on_existing = true;
        debug!("overriding apply_settings_on_existing to true");
    }
    if args.dry_run {
        settings.dry_run = true;
        debug!("dry run enabled via CLI");
    }
    if let Some(mode) = args.mode {
        settings.mode = match mode {
            MeilisearchModeArg::Upsert => MeilisearchMode::Upsert,
            MeilisearchModeArg::CleanInsert => MeilisearchMode::CleanInsert,
        };
        debug!(override_value = ?settings.mode, "overriding meilisearch mode");
    }

    settings.api_key = settings
        .api_key
        .and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });

    validate_settings(&settings)?;

    info!(
        host = %settings.host,
        index_uid = %settings.index_uid,
        mode = ?settings.mode,
        batch_size = settings.batch_size,
        timeout_secs = settings.timeout_secs,
        primary_key = %settings.primary_key,
        apply_settings_on_existing = settings.apply_settings_on_existing,
        dry_run = settings.dry_run,
        api_key_set = settings.api_key.is_some(),
        "resolved meilisearch config"
    );

    Ok(settings)
}

fn validate_settings(settings: &ResolvedMeilisearch) -> Result<()> {
    if settings.host.trim().is_empty() {
        bail!("meilisearch host is required");
    }
    if settings.index_uid.trim().is_empty() {
        bail!("meilisearch index UID is required");
    }
    if settings.primary_key.trim().is_empty() {
        bail!("meilisearch primary key is required");
    }
    if settings.batch_size == 0 {
        bail!("meilisearch batch size must be greater than zero");
    }
    validate_attr_list("displayed_attributes", &settings.settings.displayed_attributes)?;
    validate_attr_list("searchable_attributes", &settings.settings.searchable_attributes)?;
    validate_attr_list("filterable_attributes", &settings.settings.filterable_attributes)?;
    validate_attr_list("sortable_attributes", &settings.settings.sortable_attributes)?;
    Ok(())
}

#[instrument(level = "info", skip_all, fields(index_uid = %settings.index_uid, mode = ?settings.mode))]
async fn run_indexing(settings: &ResolvedMeilisearch, documents: &[MeiliDocument]) -> Result<()> {
    let client = Client::new(&settings.host, settings.api_key.as_deref())
        .map_err(|err| anyhow!("create meilisearch client: {err}"))?;

    match settings.mode {
        MeilisearchMode::Upsert => {
            ensure_index(&client, settings).await?;
        }
        MeilisearchMode::CleanInsert => {
            clean_insert_index(&client, settings).await?;
        }
    }

    let index = client.index(&settings.index_uid);
    submit_batches(&client, &index, settings, documents).await?;
    info!(indexed_count = documents.len(), "completed indexing");

    Ok(())
}

async fn ensure_index(client: &Client, settings: &ResolvedMeilisearch) -> Result<()> {
    if index_exists(client, &settings.index_uid).await? {
        info!(index_uid = %settings.index_uid, "index already exists");
        if settings.apply_settings_on_existing {
            let index = client.index(&settings.index_uid);
            apply_settings(client, &index, settings).await?;
        }
        return Ok(());
    }

    info!(index_uid = %settings.index_uid, "creating index");
    let task = retry_meili("create_index", || async {
        client
            .create_index(&settings.index_uid, Some(&settings.primary_key))
            .await
    })
    .await?;

    wait_for_task(client, task, settings, "create_index").await?;
    let index = client.index(&settings.index_uid);
    apply_settings(client, &index, settings).await?;
    Ok(())
}

async fn clean_insert_index(client: &Client, settings: &ResolvedMeilisearch) -> Result<()> {
    if index_exists(client, &settings.index_uid).await? {
        info!(index_uid = %settings.index_uid, "deleting existing index");
        let task = retry_meili("delete_index", || async {
            client.delete_index(&settings.index_uid).await
        })
        .await?;
        wait_for_task(client, task, settings, "delete_index").await?;
    } else {
        info!(index_uid = %settings.index_uid, "index not found; skipping delete");
    }

    info!(index_uid = %settings.index_uid, "creating index");
    let task = retry_meili("create_index", || async {
        client
            .create_index(&settings.index_uid, Some(&settings.primary_key))
            .await
    })
    .await?;
    wait_for_task(client, task, settings, "create_index").await?;
    let index = client.index(&settings.index_uid);
    apply_settings(client, &index, settings).await?;

    Ok(())
}

async fn submit_batches(
    client: &Client,
    index: &meilisearch_sdk::indexes::Index,
    settings: &ResolvedMeilisearch,
    documents: &[MeiliDocument],
) -> Result<()> {
    let total = documents.len();
    let mut batch_num = 0usize;

    for chunk in documents.chunks(settings.batch_size) {
        batch_num += 1;
        let batch_size = chunk.len();
        let batch_start = Instant::now();
        info!(
            batch = batch_num,
            batch_size,
            total,
            "submitting document batch"
        );

        let task = retry_meili("add_documents", || async {
            index
                .add_documents(chunk, Some(&settings.primary_key))
                .await
        })
        .await?;

        info!(
            batch = batch_num,
            task_uid = task.get_task_uid(),
            "submitted document batch"
        );

        wait_for_task(
            client,
            task,
            settings,
            &format!("batch_{batch_num}"),
        )
        .await?;

        info!(
            batch = batch_num,
            batch_size,
            elapsed_ms = batch_start.elapsed().as_millis(),
            "batch completed"
        );
    }

    Ok(())
}

async fn apply_settings(
    client: &Client,
    index: &meilisearch_sdk::indexes::Index,
    settings: &ResolvedMeilisearch,
) -> Result<()> {
    let settings_payload = build_index_settings(&settings.settings);
    info!(index_uid = %settings.index_uid, "applying index settings");
    let task = retry_meili("set_settings", || async {
        index.set_settings(&settings_payload).await
    })
    .await?;
    wait_for_task(client, task, settings, "set_settings").await?;
    Ok(())
}

async fn index_exists(client: &Client, uid: &str) -> Result<bool> {
    match client.get_index(uid).await {
        Ok(_) => Ok(true),
        Err(err) if is_index_not_found(&err) => Ok(false),
        Err(err) => Err(err.into()),
    }
}

fn is_index_not_found(err: &MeiliError) -> bool {
    match err {
        MeiliError::Meilisearch(error) => error.error_code == ErrorCode::IndexNotFound,
        MeiliError::MeilisearchCommunication(error) => error.status_code == 404,
        _ => false,
    }
}

async fn wait_for_task(
    client: &Client,
    task: TaskInfo,
    settings: &ResolvedMeilisearch,
    label: &str,
) -> Result<Task> {
    let task_uid = task.get_task_uid();
    let start = Instant::now();
    let interval = Some(Duration::from_millis(TASK_POLL_INTERVAL_MS));
    let timeout = if settings.timeout_secs == 0 {
        None
    } else {
        Some(Duration::from_secs(settings.timeout_secs))
    };

    info!(task_uid, label, "waiting for task completion");
    let status = task
        .wait_for_completion(client, interval, timeout)
        .await
        .with_context(|| format!("wait for task {task_uid}"))?;

    let elapsed_ms = start.elapsed().as_millis();
    if matches!(status, Task::Succeeded { .. }) {
        info!(task_uid, elapsed_ms, label, "task succeeded");
        return Ok(status);
    }

    if matches!(status, Task::Failed { .. }) {
        error!(task_uid, elapsed_ms, label, task = ?status, "task failed");
        bail!("task {task_uid} failed");
    }

    warn!(task_uid, elapsed_ms, label, task = ?status, "task completed");
    Ok(status)
}

async fn retry_meili<F, Fut, T>(label: &str, mut op: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = std::result::Result<T, MeiliError>>,
{
    let mut attempt = 0usize;
    loop {
        attempt += 1;
        match op().await {
            Ok(value) => return Ok(value),
            Err(err) if attempt < RETRY_ATTEMPTS && is_retryable(&err) => {
                let delay = retry_delay(attempt);
                warn!(
                    attempt,
                    delay_ms = delay.as_millis(),
                    label,
                    error = %err,
                    "meilisearch request failed; retrying"
                );
                tokio::time::sleep(delay).await;
            }
            Err(err) => {
                error!(attempt, label, error = %err, "meilisearch request failed");
                return Err(err.into());
            }
        }
    }
}

fn is_retryable(err: &MeiliError) -> bool {
    match err {
        MeiliError::HttpError(err) => {
            if err.is_timeout() || err.is_connect() {
                return true;
            }
            match err.status() {
                Some(status) => status.is_server_error() || status.as_u16() == 429,
                None => true,
            }
        }
        MeiliError::MeilisearchCommunication(err) => {
            err.status_code == 429 || err.status_code >= 500
        }
        MeiliError::Meilisearch(err) => err.error_type == ErrorType::Internal,
        _ => false,
    }
}

fn retry_delay(attempt: usize) -> Duration {
    let base = RETRY_BASE_DELAY_MS.saturating_mul(2_u64.saturating_pow(attempt as u32 - 1));
    let jitter = jitter_ms();
    Duration::from_millis(base.min(5_000).saturating_add(jitter))
}

fn build_index_settings(settings: &MeilisearchSettingsConfig) -> Settings {
    Settings::new()
        .with_displayed_attributes(settings.displayed_attributes.clone())
        .with_searchable_attributes(settings.searchable_attributes.clone())
        .with_filterable_attributes(settings.filterable_attributes.clone())
        .with_sortable_attributes(settings.sortable_attributes.clone())
}

fn validate_attr_list(label: &str, values: &[String]) -> Result<()> {
    if values.is_empty() {
        bail!("meilisearch settings {label} cannot be empty");
    }
    let mut seen = std::collections::BTreeSet::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            bail!("meilisearch settings {label} contains empty value");
        }
        if !seen.insert(trimmed.to_ascii_lowercase()) {
            bail!("meilisearch settings {label} contains duplicate value: {trimmed}");
        }
    }
    Ok(())
}

fn jitter_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    now.subsec_millis() as u64 % 200
}

fn map_document(doc: &ParsedDocument) -> MeiliDocument {
    let title = doc
        .release
        .as_ref()
        .and_then(|r| r.game_title_line.clone())
        .or_else(|| doc.post.as_ref().and_then(|p| p.entry_title.clone()))
        .or_else(|| doc.page.as_ref().and_then(|p| p.title.clone()))
        .unwrap_or_default();

    MeiliDocument {
        id: doc.source.sha256.clone(),
        title,
        poster: doc.poster.clone(),
        site: doc.site.clone(),
        source_path: doc.source.path.clone(),
        canonical_url: doc.page.as_ref().and_then(|p| p.canonical_url.clone()),
        categories: doc
            .post
            .as_ref()
            .map(|p| p.categories.clone())
            .unwrap_or_default(),
        wp_tags: doc
            .post
            .as_ref()
            .map(|p| p.wp_tags.clone())
            .unwrap_or_default(),
        genres: doc
            .release
            .as_ref()
            .map(|r| r.genres_tags.clone())
            .unwrap_or_default(),
        companies: doc
            .release
            .as_ref()
            .map(|r| r.companies.clone())
            .unwrap_or_default(),
        languages_raw: doc
            .release
            .as_ref()
            .and_then(|r| r.languages_raw.clone()),
        original_size_raw: doc
            .release
            .as_ref()
            .and_then(|r| r.original_size_raw.clone()),
        repack_size_raw: doc
            .release
            .as_ref()
            .and_then(|r| r.repack_size_raw.clone()),
        release_number: doc
            .release
            .as_ref()
            .and_then(|r| r.release_number),
        entry_datetime: doc
            .post
            .as_ref()
            .and_then(|p| p.entry_datetime.clone()),
        author: doc.post.as_ref().and_then(|p| p.author.clone()),
        torrent_file: doc.torrent_file,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ParsedDocument, ReleaseMeta, SourceInfo};

    #[test]
    fn map_document_uses_source_sha256_for_id() {
        let doc = ParsedDocument {
            source: SourceInfo {
                path: "tmp/sample.html".to_string(),
                bytes: 0,
                sha256: "abc123".to_string(),
            },
            site: "generic".to_string(),
            poster: None,
            page: None,
            post: None,
            release: Some(ReleaseMeta {
                release_number: None,
                game_title_line: Some("Example".to_string()),
                genres_tags: vec![],
                companies: vec![],
                languages_raw: None,
                original_size_raw: None,
                repack_size_raw: None,
            }),
            spoiler_sections: vec![],
            link_domain_counts: Default::default(),
            download_section_headings: vec![],
            torrent_file: None,
            torrent_file_names: vec![],
            torrent_file_links: vec![],
            magnet_links: vec![],
        };

        let mapped = map_document(&doc);
        assert_eq!(mapped.id, "abc123");
    }
}
