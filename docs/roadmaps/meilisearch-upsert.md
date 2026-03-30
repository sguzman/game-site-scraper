# Roadmap: Meilisearch Upsert Subcommand

Goal: add a new CLI subcommand that takes the parsed FitGirl game JSON documents and upserts them into a Meilisearch index.

This is a roadmap only (no code changes).

## Scope / Requirements

- [ ] Add Meilisearch connection settings to `scrape.toml` (host/service info)
- [ ] Add Meilisearch auth settings to `scrape.toml` (API key / master key)
- [ ] Add Meilisearch index name (UID) to `scrape.toml`
- [ ] Add two indexing modes:
  - [ ] **Upsert**: add/update documents in-place (do not delete the index)
  - [ ] **Clean insert**: delete the index and recreate it, then insert all documents
- [ ] Ensure each indexed document has a clean Meilisearch primary key field (`id`)
- [ ] Add a “cover image” field that Meilisearch front-ends can display as the result thumbnail
  - [ ] Research-backed field name: use `poster` as the canonical URL-to-image attribute (Meilisearch docs/examples use `poster` as a poster image URL in sample datasets)
- [ ] Extensive `tracing` logging for:
  - [ ] config resolution
  - [ ] index lifecycle operations (create/delete/settings)
  - [ ] batching, task UIDs, and task completion status
  - [ ] per-document/aggregate counts

## Deliverables

- [ ] New CLI subcommand (suggested): `game-scraper meilisearch upsert ...` or `game-scraper index meili ...`
- [ ] Config schema additions under a new `[meilisearch]` table in `scrape.toml`
- [ ] Document “indexing shape” transformation layer (from `ParsedDocument` → `MeiliDocument`)
- [ ] Parser enhancement to reliably extract a cover image URL from the saved HTML
- [ ] Updated README with usage examples and local Docker instructions (using the provided compose file)

## Decisions (Proposed)

### Config: `[meilisearch]`

- [ ] `host`: base URL (e.g. `http://127.0.0.1:7700`)
- [ ] `api_key`: optional string (empty/omitted means “no auth”)
- [ ] `index_uid`: string (e.g. `fitgirl-games`)
- [ ] `primary_key`: default `"id"` (explicitly set when creating index)
- [ ] `batch_size`: default (e.g. 500–2000 docs per batch)
- [ ] `timeout_secs`: request timeout (optional)
- [ ] `mode`: `upsert|clean_insert` (or CLI flag overrides config)

### CLI UX

- [ ] Subcommand accepts the same inputs as `parse` (files/dirs, `--recursive`, `--follow-symlinks`)
- [ ] Accepts `--config <PATH>` like existing commands
- [ ] Accepts `--mode upsert|clean-insert` to override config
- [ ] Optional: accept `--index <UID>` and `--host <URL>` for quick overrides

## Work Breakdown

### Phase 0 — Research & constraints

- [ ] Confirm the Meilisearch Rust SDK to use and its API surface for:
  - [ ] creating/deleting an index
  - [ ] setting primary key at creation
  - [ ] adding/upserting documents in batches
  - [ ] waiting for task completion / handling failures
- [ ] Confirm “image field” conventions:
  - [ ] Use `poster` as the document attribute containing the cover image URL (aligned with Meilisearch examples)

### Phase 1 — Config plumbing

- [ ] Extend `Config` and default config template with a `[meilisearch]` table
- [ ] Ensure config prints correctly via `print-config`
- [ ] Add clear logging on:
  - [ ] whether Meilisearch config is present/enabled
  - [ ] which host/index UID are being targeted (never log secrets)

### Phase 2 — Document shape for Meilisearch

Meilisearch primary key inference can fail when multiple `*id` fields exist; avoid surprises by explicitly setting the primary key and also producing a clean `id` field.

- [ ] Define a stable `id` per game document:
  - [ ] Option A: `id = source.sha256` (stable and already computed)
  - [ ] Option B: `id = canonical_url` (if present) normalized
  - [ ] Option C: `id = slug(title)` (riskier unless guaranteed unique)
- [ ] Remove/rename conflicting `*id` attributes where helpful (e.g. `post.post_id` → `post.wordpress_post_id`) to reduce ambiguity and keep the schema clean
- [ ] Ensure the final Meili document uses a flat, UI-friendly schema:
  - [ ] `id` (primary key)
  - [ ] `title` (best user-facing title)
  - [ ] `poster` (cover image URL)
  - [ ] `site`, `source_path`, `canonical_url`
  - [ ] tags/genres/companies/languages/sizes

### Phase 3 — Cover image extraction (parser)

Add parsing mechanics to extract the FitGirl game cover image URL from the saved HTML.

- [ ] Identify where the cover image lives in the HTML across samples:
  - [ ] `meta[property="og:image"]`
  - [ ] `meta[name="twitter:image"]`
  - [ ] featured image elements (common WordPress patterns)
  - [ ] first large content image heuristics (last resort)
- [ ] Add config toggle for cover extraction if you want parity with other `scrape.*` flags
- [ ] Normalize to an absolute URL when possible
- [ ] Populate document field `poster` with the chosen URL

### Phase 4 — Index lifecycle & settings

- [ ] **Upsert mode**
  - [ ] Create index if missing (with explicit primary key)
  - [ ] Apply/verify settings once (optional but recommended)
  - [ ] Add documents in batches (documentAdditionOrUpdate)
- [ ] **Clean insert mode**
  - [ ] Delete index if present
  - [ ] Create index (explicit primary key)
  - [ ] Apply settings (searchable/displayed/filterable/sortable)
  - [ ] Insert all docs in batches
- [ ] Settings (initial proposal)
  - [ ] `displayedAttributes` includes `poster` so clients can render thumbnails
  - [ ] `searchableAttributes` focuses on title + key metadata fields
  - [ ] `filterableAttributes` for things like genre/company/language if desired

### Phase 5 — Observability & safety

- [ ] Add structured logs for:
  - [ ] counts (input docs, indexed docs, failures)
  - [ ] mode, host, index UID
  - [ ] batch number/size
  - [ ] Meilisearch task UID + status + duration
- [ ] Never log API keys/master keys
- [ ] Add failure handling:
  - [ ] surface Meilisearch task errors clearly
  - [ ] exit non-zero on failed indexing

### Phase 6 — Verification checklist

- [ ] Local run using `tmp/docker-compose.yaml` Meilisearch service
- [ ] Upsert run twice; verify second run updates existing docs (no duplicates)
- [ ] Clean insert run; verify index was recreated and doc count matches input
- [ ] Verify search results include `poster` and that the chosen UI/tooling can render it
- [ ] Verify the app builds (`cargo build`) after implementation (later PR step)

## Acceptance Criteria

- [ ] Config supports host + auth + index UID
- [ ] CLI supports upsert + clean insert modes
- [ ] Indexed docs have a stable `id` primary key and no unexpected primary key inference failures
- [ ] Indexed docs include a cover image URL in `poster`
- [ ] Logging is detailed, structured, and goes to stderr (so stdout can be reserved for JSON output when applicable)

## Notes / References

- Meilisearch does not mandate a special “cover image” attribute; the *client/UI* decides what to render. This roadmap standardizes on `poster` because Meilisearch’s own sample datasets and demos commonly use `poster` as a URL-to-image field.
- Primary key inference can error when multiple fields ending in `id` exist; prefer explicitly setting the primary key at index creation and also keeping document schemas tidy.
