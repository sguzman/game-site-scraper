use crate::cli::{MeilisearchArgs, MeilisearchIdStrategyArg, MeilisearchModeArg};
use crate::config::{
    Config, MeilisearchIdStrategy, MeilisearchMode, MeilisearchSettingsConfig,
};
use crate::fs;
use crate::model::{OutputBundle, ParsedDocument, Stats};
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
use url::Url;

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
    id_strategy: MeilisearchIdStrategy,
    apply_settings_on_existing: bool,
    settings: MeilisearchSettingsConfig,
    dry_run: bool,
    stats_only: bool,
    settings_only: bool,
    sample: Option<usize>,
    fail_fast: bool,
    max_in_flight: usize,
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

    if settings.settings_only {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("build tokio runtime")?;
        runtime.block_on(async { apply_settings_only(&settings).await })?;
        return Ok(());
    }

    let input = collect_input(args, cfg)?;

    if input.documents.is_empty() {
        if settings.stats_only || settings.dry_run {
            info!(
                parsed_ok = input.stats.parsed_ok,
                parsed_err = input.stats.parsed_err,
                indexed_count = 0,
                "no documents parsed"
            );
            return Ok(());
        }
        warn!("no parsed documents to index");
        return Ok(());
    }

    info!(
        count = input.documents.len(),
        parsed_ok = input.stats.parsed_ok,
        parsed_err = input.stats.parsed_err,
        "prepared documents for indexing"
    );

    if settings.stats_only {
        info!(
            parsed_ok = input.stats.parsed_ok,
            parsed_err = input.stats.parsed_err,
            indexed_count = input.documents.len(),
            "stats-only enabled; skipping indexing"
        );
        if let Some(sample_size) = settings.sample {
            let sample_size = sample_size.min(input.documents.len());
            let samples: Vec<MeiliDocument> = input
                .documents
                .iter()
                .take(sample_size)
                .map(|doc| map_document(doc, settings.id_strategy))
                .collect();
            for (idx, doc) in samples.iter().enumerate() {
                info!(sample_index = idx + 1, document = ?doc, "sample document");
            }
        }
        return Ok(());
    }

    let documents: Vec<MeiliDocument> = input
        .documents
        .iter()
        .map(|doc| map_document(doc, settings.id_strategy))
        .collect();

    if settings.dry_run {
        info!(
            parsed_ok = input.stats.parsed_ok,
            parsed_err = input.stats.parsed_err,
            indexed_count = documents.len(),
            "dry run enabled; skipping indexing"
        );
        if let Some(sample_size) = settings.sample {
            let sample_size = sample_size.min(documents.len());
            for (idx, doc) in documents.iter().take(sample_size).enumerate() {
                info!(sample_index = idx + 1, document = ?doc, "sample document");
            }
        }
        return Ok(());
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?;

    runtime.block_on(async { run_indexing(&settings, &documents).await })?;

    info!(
        parsed_ok = input.stats.parsed_ok,
        parsed_err = input.stats.parsed_err,
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
        id_strategy: cfg.meilisearch.id_strategy,
        apply_settings_on_existing: cfg.meilisearch.apply_settings_on_existing,
        settings: cfg.meilisearch.settings.clone(),
        dry_run: false,
        stats_only: false,
        settings_only: false,
        sample: None,
        fail_fast: false,
        max_in_flight: 1,
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
    if let Some(strategy) = args.id_strategy {
        settings.id_strategy = match strategy {
            MeilisearchIdStrategyArg::Sha256 => MeilisearchIdStrategy::Sha256,
            MeilisearchIdStrategyArg::CanonicalUrl => MeilisearchIdStrategy::CanonicalUrl,
            MeilisearchIdStrategyArg::TitleSlug => MeilisearchIdStrategy::TitleSlug,
        };
        debug!(override_value = ?settings.id_strategy, "overriding id strategy");
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
    if args.stats_only {
        settings.stats_only = true;
        debug!("stats-only enabled via CLI");
    }
    if args.settings_only {
        settings.settings_only = true;
        debug!("settings-only enabled via CLI");
    }
    if let Some(sample) = args.sample {
        settings.sample = Some(sample);
        debug!(override_value = sample, "setting sample size");
    }
    if args.fail_fast {
        settings.fail_fast = true;
        debug!("fail-fast enabled via CLI");
    }
    if let Some(max_in_flight) = args.max_in_flight {
        settings.max_in_flight = max_in_flight;
        debug!(override_value = max_in_flight, "setting max_in_flight");
    }
    if let Some(settings_file) = &args.settings_file {
        settings.settings = load_settings_file(settings_file)?;
        debug!(
            path = %settings_file.display(),
            "loaded meilisearch settings file"
        );
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
        id_strategy = ?settings.id_strategy,
        apply_settings_on_existing = settings.apply_settings_on_existing,
        dry_run = settings.dry_run,
        stats_only = settings.stats_only,
        settings_only = settings.settings_only,
        sample = settings.sample.unwrap_or(0),
        fail_fast = settings.fail_fast,
        max_in_flight = settings.max_in_flight,
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
    if settings.max_in_flight == 0 {
        bail!("meilisearch max_in_flight must be greater than zero");
    }
    if settings.settings_only && settings.dry_run {
        bail!("cannot combine --settings-only with --dry-run");
    }
    if settings.settings_only && settings.stats_only {
        bail!("cannot combine --settings-only with --stats-only");
    }
    validate_attr_list(
        "displayed_attributes",
        &settings.settings.displayed_attributes,
        false,
    )?;
    validate_attr_list(
        "searchable_attributes",
        &settings.settings.searchable_attributes,
        false,
    )?;
    validate_attr_list(
        "filterable_attributes",
        &settings.settings.filterable_attributes,
        false,
    )?;
    validate_attr_list(
        "sortable_attributes",
        &settings.settings.sortable_attributes,
        true,
    )?;
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
    if settings.max_in_flight <= 1 {
        let mut batch_num = 0usize;
        for chunk in documents.chunks(settings.batch_size) {
            batch_num += 1;
            submit_one_batch(client, index, settings, batch_num, total, chunk.to_vec()).await?;
        }
        return Ok(());
    }

    let mut join_set = tokio::task::JoinSet::new();
    let mut batch_num = 0usize;
    let mut submitted = 0usize;
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(settings.max_in_flight));
    let fail_fast = settings.fail_fast;

    for chunk in documents.chunks(settings.batch_size) {
        batch_num += 1;
        let permit = semaphore.clone().acquire_owned().await?;
        let chunk_vec = chunk.to_vec();
        let client = client.clone();
        let index = index.clone();
        let settings_for_task = settings.clone();
        let total = total;

        join_set.spawn(async move {
            let _permit = permit;
            submit_one_batch(&client, &index, &settings_for_task, batch_num, total, chunk_vec).await
        });
        submitted += 1;

        if fail_fast {
            if let Some(result) = join_set.join_next().await {
                result??;
            }
        }
    }

    let mut completed = 0usize;
    while let Some(result) = join_set.join_next().await {
        completed += 1;
        match result {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                if fail_fast {
                    join_set.abort_all();
                    return Err(err);
                }
                error!(error = %err, "batch failed");
            }
            Err(err) => {
                if fail_fast {
                    join_set.abort_all();
                    return Err(err.into());
                }
                error!(error = %err, "batch task failed");
            }
        }
        debug!(completed, submitted, "batch completion progress");
    }

    Ok(())
}

async fn submit_one_batch(
    client: &Client,
    index: &meilisearch_sdk::indexes::Index,
    settings: &ResolvedMeilisearch,
    batch_num: usize,
    total: usize,
    chunk: Vec<MeiliDocument>,
) -> Result<()> {
    let batch_size = chunk.len();
    let batch_start = Instant::now();
    let batch_first = chunk.first().map(|doc| doc.id.as_str()).unwrap_or("");
    let batch_last = chunk.last().map(|doc| doc.id.as_str()).unwrap_or("");
    info!(
        batch = batch_num,
        batch_size,
        total,
        first_id = batch_first,
        last_id = batch_last,
        id_strategy = ?settings.id_strategy,
        "submitting document batch"
    );

    let task = retry_meili("add_documents", || async {
        index
            .add_documents(&chunk, Some(&settings.primary_key))
            .await
    })
    .await?;

    info!(
        batch = batch_num,
        task_uid = task.get_task_uid(),
        "submitted document batch"
    );

    wait_for_task(client, task, settings, &format!("batch_{batch_num}")).await?;

    info!(
        batch = batch_num,
        batch_size,
        elapsed_ms = batch_start.elapsed().as_millis(),
        "batch completed"
    );
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

    if let Task::Failed { content } = &status {
        error!(
            task_uid,
            elapsed_ms,
            label,
            error_code = ?content.error.error_code,
            error_type = ?content.error.error_type,
            error_message = %content.error.error_message,
            error_link = %content.error.error_link,
            "task failed"
        );
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

fn validate_attr_list(label: &str, values: &[String], allow_empty: bool) -> Result<()> {
    if values.is_empty() && !allow_empty {
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

fn load_settings_file(path: &std::path::Path) -> Result<MeilisearchSettingsConfig> {
    #[derive(serde::Deserialize)]
    struct SettingsFile {
        meilisearch: Option<SettingsFileMeili>,
    }

    #[derive(serde::Deserialize)]
    struct SettingsFileMeili {
        settings: Option<MeilisearchSettingsConfig>,
    }

    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("read settings file {}", path.display()))?;
    let parsed: SettingsFile = toml::from_str(&raw)
        .with_context(|| format!("parse settings TOML {}", path.display()))?;
    parsed
        .meilisearch
        .and_then(|m| m.settings)
        .ok_or_else(|| anyhow!("settings file missing [meilisearch.settings]"))
}

fn jitter_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    now.subsec_millis() as u64 % 200
}

fn normalize_canonical_url(raw: &str) -> Option<String> {
    let mut url = Url::parse(raw).ok()?;
    url.set_fragment(None);
    let path = url.path().to_string();
    if path.len() > 1 && path.ends_with('/') {
        let trimmed = path.trim_end_matches('/');
        url.set_path(trimmed);
    }
    Some(url.to_string())
}

fn slugify_title(title: &str) -> Option<String> {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn map_document(doc: &ParsedDocument, strategy: MeilisearchIdStrategy) -> MeiliDocument {
    let title = doc
        .release
        .as_ref()
        .and_then(|r| r.game_title_line.clone())
        .or_else(|| doc.post.as_ref().and_then(|p| p.entry_title.clone()))
        .or_else(|| doc.page.as_ref().and_then(|p| p.title.clone()))
        .unwrap_or_default();
    let mut id = match strategy {
        MeilisearchIdStrategy::Sha256 => Some(doc.source.sha256.clone()),
        MeilisearchIdStrategy::CanonicalUrl => doc
            .page
            .as_ref()
            .and_then(|page| page.canonical_url.as_deref())
            .and_then(normalize_canonical_url),
        MeilisearchIdStrategy::TitleSlug => slugify_title(
            doc.release
                .as_ref()
                .and_then(|r| r.game_title_line.as_deref())
                .or_else(|| doc.post.as_ref().and_then(|p| p.entry_title.as_deref()))
                .or_else(|| doc.page.as_ref().and_then(|p| p.title.as_deref()))
                .unwrap_or(""),
        ),
    };

    if id.is_none() {
        debug!(
            strategy = ?strategy,
            fallback_id = %doc.source.sha256,
            "id strategy fallback to sha256"
        );
        id = Some(doc.source.sha256.clone());
    }

    let poster = doc
        .poster
        .as_deref()
        .and_then(validate_poster_url)
        .map(|s| s.to_string());

    MeiliDocument {
        id: id.unwrap_or_else(|| doc.source.sha256.clone()),
        title,
        poster,
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

fn validate_poster_url(url: &str) -> Option<&str> {
    let url = url.trim();
    if url.starts_with("http://") || url.starts_with("https://") {
        Some(url)
    } else {
        None
    }
}

struct ParsedInput {
    documents: Vec<ParsedDocument>,
    stats: Stats,
}

fn collect_input(args: &MeilisearchArgs, cfg: &Config) -> Result<ParsedInput> {
    if args.from_json.is_some() && args.from_ndjson.is_some() {
        bail!("cannot use --from-json and --from-ndjson together");
    }

    if let Some(path) = &args.from_json {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("read json input {}", path.display()))?;
        if let Ok(bundle) = serde_json::from_str::<OutputBundle>(&raw) {
            if !bundle.errors.is_empty() {
                warn!(
                    error_count = bundle.errors.len(),
                    "json input includes parse errors"
                );
            }
            return Ok(ParsedInput {
                documents: bundle.documents,
                stats: bundle.stats,
            });
        }
        let docs: Vec<ParsedDocument> =
            serde_json::from_str(&raw).context("parse json input as documents")?;
        let stats = Stats {
            input_count: docs.len(),
            parsed_ok: docs.len(),
            parsed_err: 0,
        };
        return Ok(ParsedInput { documents: docs, stats });
    }

    if let Some(path) = &args.from_ndjson {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("read ndjson input {}", path.display()))?;
        let mut docs = Vec::new();
        let mut err_count = 0usize;
        let mut summary_stats: Option<Stats> = None;
        for (line_no, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let value: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(err) => {
                    warn!(line = line_no + 1, error = %err, "invalid ndjson line");
                    err_count += 1;
                    continue;
                }
            };
            if let Some(kind) = value.get("type").and_then(|v| v.as_str()) {
                if kind == "error" {
                    err_count += 1;
                    continue;
                }
                if kind == "summary" {
                    if let Some(data) = value.get("data") {
                        match serde_json::from_value::<Stats>(data.clone()) {
                            Ok(stats) => summary_stats = Some(stats),
                            Err(err) => {
                                warn!(line = line_no + 1, error = %err, "invalid summary stats");
                                err_count += 1;
                            }
                        }
                    }
                    continue;
                }
            }
            match serde_json::from_value::<ParsedDocument>(value) {
                Ok(doc) => docs.push(doc),
                Err(err) => {
                    warn!(line = line_no + 1, error = %err, "invalid document in ndjson");
                    err_count += 1;
                }
            }
        }
        let stats = summary_stats.unwrap_or(Stats {
            input_count: docs.len() + err_count,
            parsed_ok: docs.len(),
            parsed_err: err_count,
        });
        return Ok(ParsedInput {
            documents: docs,
            stats,
        });
    }

    let files = fs::collect_html_inputs(&args.inputs, args.recursive, args.follow_symlinks)
        .context("collect inputs")?;

    if files.is_empty() {
        warn!("no input HTML files found");
        return Ok(ParsedInput {
            documents: Vec::new(),
            stats: Stats {
                input_count: 0,
                parsed_ok: 0,
                parsed_err: 0,
            },
        });
    }

    info!(count = files.len(), "collected input HTML files");

    let bundle = parser::parse_many(&files, cfg).context("parse inputs")?;
    if !bundle.errors.is_empty() {
        warn!(
            error_count = bundle.errors.len(),
            "some documents failed to parse; continuing with parsed documents"
        );
    }

    Ok(ParsedInput {
        documents: bundle.documents,
        stats: bundle.stats,
    })
}

async fn apply_settings_only(settings: &ResolvedMeilisearch) -> Result<()> {
    let client = Client::new(&settings.host, settings.api_key.as_deref())
        .map_err(|err| anyhow!("create meilisearch client: {err}"))?;
    ensure_index(&client, settings).await?;
    let index = client.index(&settings.index_uid);
    apply_settings(&client, &index, settings).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{OutputBundle, ParsedDocument, ReleaseMeta, SourceInfo, ToolInfo};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

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

        let mapped = map_document(&doc, MeilisearchIdStrategy::Sha256);
        assert_eq!(mapped.id, "abc123");
    }

    #[test]
    fn normalize_canonical_removes_fragment_and_trailing_slash() {
        let value = normalize_canonical_url("https://example.com/game/#section").unwrap();
        assert_eq!(value, "https://example.com/game");
    }

    #[test]
    fn slugify_title_basic() {
        let value = slugify_title("A Plague Tale: Innocence").unwrap();
        assert_eq!(value, "a-plague-tale-innocence");
    }

    #[test]
    fn slugify_title_symbols_only_returns_none() {
        let value = slugify_title("!!!");
        assert!(value.is_none());
    }

    #[test]
    fn id_strategy_uses_canonical_when_available() {
        let doc = ParsedDocument {
            source: SourceInfo {
                path: "tmp/sample.html".to_string(),
                bytes: 0,
                sha256: "abc123".to_string(),
            },
            site: "generic".to_string(),
            poster: None,
            page: Some(crate::model::PageMeta {
                title: None,
                canonical_url: Some("https://example.com/game/".to_string()),
                meta: Default::default(),
            }),
            post: None,
            release: None,
            spoiler_sections: vec![],
            link_domain_counts: Default::default(),
            download_section_headings: vec![],
            torrent_file: None,
            torrent_file_names: vec![],
            torrent_file_links: vec![],
            magnet_links: vec![],
        };
        let mapped = map_document(&doc, MeilisearchIdStrategy::CanonicalUrl);
        assert_eq!(mapped.id, "https://example.com/game");
    }

    #[test]
    fn id_strategy_falls_back_to_sha256() {
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
            release: None,
            spoiler_sections: vec![],
            link_domain_counts: Default::default(),
            download_section_headings: vec![],
            torrent_file: None,
            torrent_file_names: vec![],
            torrent_file_links: vec![],
            magnet_links: vec![],
        };
        let mapped = map_document(&doc, MeilisearchIdStrategy::TitleSlug);
        assert_eq!(mapped.id, "abc123");
    }

    #[test]
    fn collect_input_reads_json_bundle() {
        let path = write_temp_file("bundle.json", &sample_bundle_json());
        let args = base_args().with_from_json(path.clone());
        let cfg = Config::default();
        let input = collect_input(&args, &cfg).expect("collect input");
        assert_eq!(input.documents.len(), 1);
        assert_eq!(input.stats.parsed_ok, 1);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn collect_input_reads_ndjson_with_errors() {
        let path = write_temp_file(
            "bundle.ndjson",
            &format!(
                "{}\n{{invalid}}\n{{\"type\":\"summary\",\"data\":{{\"input_count\":2,\"parsed_ok\":1,\"parsed_err\":1}}}}\n",
                serde_json::to_string(&sample_doc()).unwrap()
            ),
        );
        let args = base_args().with_from_ndjson(path.clone());
        let cfg = Config::default();
        let input = collect_input(&args, &cfg).expect("collect input");
        assert_eq!(input.documents.len(), 1);
        assert_eq!(input.stats.parsed_err, 1);
        std::fs::remove_file(path).ok();
    }

    fn base_args() -> MeilisearchArgs {
        MeilisearchArgs {
            inputs: vec![],
            recursive: false,
            follow_symlinks: false,
            mode: None,
            host: None,
            index: None,
            api_key: None,
            primary_key: None,
            id_strategy: None,
            batch_size: None,
            timeout_secs: None,
            apply_settings: false,
            dry_run: false,
            from_json: None,
            from_ndjson: None,
            stats_only: false,
            settings_only: false,
            sample: None,
            fail_fast: false,
            max_in_flight: None,
            settings_file: None,
        }
    }

    trait ArgsExt {
        fn with_from_json(self, path: PathBuf) -> Self;
        fn with_from_ndjson(self, path: PathBuf) -> Self;
    }

    impl ArgsExt for MeilisearchArgs {
        fn with_from_json(mut self, path: PathBuf) -> Self {
            self.from_json = Some(path);
            self
        }

        fn with_from_ndjson(mut self, path: PathBuf) -> Self {
            self.from_ndjson = Some(path);
            self
        }
    }

    fn sample_doc() -> ParsedDocument {
        ParsedDocument {
            source: SourceInfo {
                path: "tmp/sample.html".to_string(),
                bytes: 0,
                sha256: "abc123".to_string(),
            },
            site: "generic".to_string(),
            poster: None,
            page: None,
            post: None,
            release: None,
            spoiler_sections: vec![],
            link_domain_counts: Default::default(),
            download_section_headings: vec![],
            torrent_file: None,
            torrent_file_names: vec![],
            torrent_file_links: vec![],
            magnet_links: vec![],
        }
    }

    fn sample_bundle_json() -> String {
        let bundle = OutputBundle {
            tool: ToolInfo {
                name: "game-scraper".to_string(),
                version: "0.1.0".to_string(),
            },
            stats: Stats {
                input_count: 1,
                parsed_ok: 1,
                parsed_err: 0,
            },
            documents: vec![sample_doc()],
            errors: vec![],
        };
        serde_json::to_string(&bundle).unwrap()
    }

    fn write_temp_file(name: &str, contents: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let mut path = std::env::temp_dir();
        path.push(format!("game_scraper_{now}_{nonce}_{name}"));
        std::fs::write(&path, contents).expect("write temp file");
        path
    }
}
