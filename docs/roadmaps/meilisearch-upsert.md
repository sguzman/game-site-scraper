# Roadmap: Meilisearch Upsert Subcommand

Goal: add a new CLI subcommand that takes the parsed FitGirl game JSON documents and upserts them into a Meilisearch index.

This is a roadmap only (no code changes).

## Scope / Requirements

- [x] Add Meilisearch connection settings to `scrape.toml` (host/service info)
- [x] Add Meilisearch auth settings to `scrape.toml` (API key / master key)
- [x] Add Meilisearch index name (UID) to `scrape.toml`
- [x] Add two indexing modes:
  - [x] **Upsert**: add/update documents in-place (do not delete the index)
  - [x] **Clean insert**: delete the index and recreate it, then insert all documents
- [x] Ensure each indexed document has a clean Meilisearch primary key field (`id`)
- [x] Add a “cover image” field that Meilisearch front-ends can display as the result thumbnail
  - [x] Research-backed field name: use `poster` as the canonical URL-to-image attribute (Meilisearch docs/examples use `poster` as a poster image URL in sample datasets)
- [x] Extensive `tracing` logging for:
  - [x] config resolution
  - [x] index lifecycle operations (create/delete/settings)
  - [x] batching, task UIDs, and task completion status
  - [x] per-document/aggregate counts

## Deliverables

- [x] New CLI subcommand (suggested): `game-scraper meilisearch upsert ...` or `game-scraper index meili ...`
- [x] Config schema additions under a new `[meilisearch]` table in `scrape.toml`
- [x] Document “indexing shape” transformation layer (from `ParsedDocument` → `MeiliDocument`)
- [x] Parser enhancement to reliably extract a cover image URL from the saved HTML
- [x] Updated README with usage examples and local Docker instructions (using the provided compose file)

## Decisions (Proposed)

### Config: `[meilisearch]`

- [x] `host`: base URL (e.g. `http://127.0.0.1:7700`)
- [x] `api_key`: optional string (empty/omitted means “no auth”)
- [x] `index_uid`: string (e.g. `fitgirl-games`)
- [x] `primary_key`: default `"id"` (explicitly set when creating index)
- [x] `batch_size`: default (e.g. 500–2000 docs per batch)
- [x] `timeout_secs`: request timeout (optional)
- [x] `mode`: `upsert|clean_insert` (or CLI flag overrides config)

### CLI UX

- [x] Subcommand accepts the same inputs as `parse` (files/dirs, `--recursive`, `--follow-symlinks`)
- [x] Accepts `--config <PATH>` like existing commands
- [x] Accepts `--mode upsert|clean-insert` to override config
- [x] Optional: accept `--index <UID>` and `--host <URL>` for quick overrides

## Work Breakdown

### Phase 0 — Research & constraints

- [x] Confirm the Meilisearch Rust SDK to use and its API surface for:
  - [x] creating/deleting an index
  - [x] setting primary key at creation
  - [x] adding/upserting documents in batches
  - [x] waiting for task completion / handling failures
- [x] Confirm “image field” conventions:
  - [x] Use `poster` as the document attribute containing the cover image URL (aligned with Meilisearch examples)

### Phase 1 — Config plumbing

- [x] Extend `Config` and default config template with a `[meilisearch]` table
- [x] Ensure config prints correctly via `print-config`
- [x] Add clear logging on:
  - [x] whether Meilisearch config is present/enabled
  - [x] which host/index UID are being targeted (never log secrets)

### Phase 2 — Document shape for Meilisearch

Meilisearch primary key inference can fail when multiple `*id` fields exist; avoid surprises by explicitly setting the primary key and also producing a clean `id` field.

- [x] Define a stable `id` per game document:
  - [x] Option A: `id = source.sha256` (stable and already computed)
  - [ ] Option B: `id = canonical_url` (if present) normalized
  - [ ] Option C: `id = slug(title)` (riskier unless guaranteed unique)
- [x] Remove/rename conflicting `*id` attributes where helpful (e.g. `post.post_id` → `post.wordpress_post_id`) to reduce ambiguity and keep the schema clean
- [x] Ensure the final Meili document uses a flat, UI-friendly schema:
  - [x] `id` (primary key)
  - [x] `title` (best user-facing title)
  - [x] `poster` (cover image URL)
  - [x] `site`, `source_path`, `canonical_url`
  - [x] tags/genres/companies/languages/sizes

### Phase 3 — Cover image extraction (parser)

Add parsing mechanics to extract the FitGirl game cover image URL from the saved HTML.

- [x] Identify where the cover image lives in the HTML across samples:
  - [x] `meta[property="og:image"]`
  - [x] `meta[name="twitter:image"]`
  - [x] featured image elements (common WordPress patterns)
  - [x] first large content image heuristics (last resort)
- [x] Add config toggle for cover extraction if you want parity with other `scrape.*` flags
- [x] Normalize to an absolute URL when possible
- [x] Populate document field `poster` with the chosen URL

### Phase 4 — Index lifecycle & settings

- [x] **Upsert mode**
  - [x] Create index if missing (with explicit primary key)
  - [x] Apply/verify settings once (optional but recommended)
  - [x] Add documents in batches (documentAdditionOrUpdate)
- [x] **Clean insert mode**
  - [x] Delete index if present
  - [x] Create index (explicit primary key)
  - [x] Apply settings (searchable/displayed/filterable/sortable)
  - [x] Insert all docs in batches
- [x] Settings (initial proposal)
  - [x] `displayedAttributes` includes `poster` so clients can render thumbnails
  - [x] `searchableAttributes` focuses on title + key metadata fields
  - [x] `filterableAttributes` for things like genre/company/language if desired

### Phase 5 — Observability & safety

- [x] Add structured logs for:
  - [x] counts (input docs, indexed docs, failures)
  - [x] mode, host, index UID
  - [x] batch number/size
  - [x] Meilisearch task UID + status + duration
- [x] Never log API keys/master keys
- [x] Add failure handling:
  - [x] surface Meilisearch task errors clearly
  - [x] exit non-zero on failed indexing

### Phase 6 — Verification checklist

- [ ] Local run using `tmp/docker-compose.yaml` Meilisearch service
- [ ] Upsert run twice; verify second run updates existing docs (no duplicates)
- [ ] Clean insert run; verify index was recreated and doc count matches input
- [ ] Verify search results include `poster` and that the chosen UI/tooling can render it
- [x] Verify the app builds (`cargo build`) after implementation (later PR step)

## Acceptance Criteria

- [x] Config supports host + auth + index UID
- [x] CLI supports upsert + clean insert modes
- [x] Indexed docs have a stable `id` primary key and no unexpected primary key inference failures
- [x] Indexed docs include a cover image URL in `poster`
- [x] Logging is detailed, structured, and goes to stderr (so stdout can be reserved for JSON output when applicable)

## Notes / References

- Meilisearch does not mandate a special “cover image” attribute; the *client/UI* decides what to render. This roadmap standardizes on `poster` because Meilisearch’s own sample datasets and demos commonly use `poster` as a URL-to-image field.
- Primary key inference can error when multiple fields ending in `id` exist; prefer explicitly setting the primary key at index creation and also keeping document schemas tidy.

## Batch 1 (30 Items)

- [x] Decide which Meilisearch Rust SDK/version to use
- [x] Add `[meilisearch]` to default `scrape.toml`
- [x] Extend config structs with Meilisearch settings
- [x] Add config validation for required settings per mode
- [x] Decide final CLI subcommand name and structure
- [x] Add `--mode` override flag for `upsert|clean-insert`
- [x] Add `--host` override flag
- [x] Add `--index` override flag
- [x] Add `--api-key` override flag (with redaction in logs)
- [x] Add `--batch-size` override flag
- [x] Implement a Meilisearch client wrapper module
- [x] Implement index create with explicit primary key
- [x] Implement index delete
- [x] Implement index existence check
- [x] Implement upsert mode flow
- [x] Implement clean-insert mode flow
- [x] Implement batched document submission
- [x] Implement task polling with timeout
- [x] Add retry/backoff for transient HTTP errors
- [x] Add document-mapper with stable `id`
- [x] Add cover image extraction to parser
- [x] Add `poster` field to Meili document
- [x] Ensure `poster` is the first image-like URL for mini-dashboard
- [x] Normalize cover image URL to absolute
- [x] Add unit tests for `id` mapping
- [x] Add unit tests for cover image extraction
- [x] Add integration test plan for local Meilisearch
- [x] Update README with Meilisearch usage
- [x] Add troubleshooting section (auth/index/payload)
- [x] Add log events for task UIDs and durations

## Batch 2 (30 Items)

- [x] Scope: add Meilisearch host to config
- [x] Scope: add Meilisearch auth settings
- [x] Scope: add index UID to config
- [x] Scope: implement upsert mode
- [x] Scope: implement clean-insert mode
- [x] Scope: ensure stable `id` primary key
- [x] Scope: add `poster` cover image field
- [x] Deliverable: add Meilisearch CLI subcommand
- [x] Deliverable: add `[meilisearch]` config schema
- [x] Deliverable: add `ParsedDocument` → `MeiliDocument` mapping
- [x] Deliverable: add cover image extraction parser
- [x] Deliverable: update README with Meilisearch usage + Docker
- [x] Decision: include `host`, `api_key`, `index_uid`, `primary_key`
- [x] Decision: include `batch_size`, `timeout_secs`, `mode`
- [x] CLI UX: accept same inputs as `parse`
- [x] CLI UX: accept `--config`
- [x] CLI UX: accept `--mode`
- [x] CLI UX: accept `--index`/`--host` overrides
- [x] Phase 0: confirm SDK API coverage (create/delete/index/tasks)
- [x] Phase 0: confirm `poster` field convention
- [x] Phase 1: ensure config prints via `print-config`
- [x] Phase 1: log host/index target without secrets
- [x] Phase 2: set `id = source.sha256`
- [x] Phase 2: ensure flat Meili document schema
- [x] Phase 3: use og/twitter/featured image selectors
- [x] Phase 3: add cover image toggle
- [x] Phase 3: normalize cover URLs to absolute
- [x] Phase 4: apply index settings on create
- [x] Phase 4: set displayed/searchable/filterable/sortable attributes
