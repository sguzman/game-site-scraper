# game-scraper

`game-scraper` parses saved game-release HTML pages into structured JSON metadata.

## Intent

Convert messy saved pages into cleaner structured release records that can be searched, indexed, or fed into downstream catalogs.

## Ambition

The presence of Meilisearch integration and output-shaping modules suggests an ambition to be a repeatable game-metadata ingestion step rather than a one-off parser script.

## Current Status

The CLI, config, output, and search-integration modules are already present. The repository looks compact but purposeful.

## Core Capabilities Or Focus Areas

- Parse saved HTML pages into structured data.
- Emit JSON output.
- Integrate with Meilisearch-oriented workflows.
- Use config-driven behavior for scraping/parsing runs.
- Separate filesystem, model, and output concerns.

## Project Layout

- `docs/`: project documentation, reference material, and roadmap notes.
- `src/`: Rust source for the main crate or application entrypoint.
- `Cargo.toml`: crate or workspace manifest and the first place to check for package structure.

## Setup And Requirements

- Rust toolchain.
- Saved game-related HTML pages in the expected format.
- Optional Meilisearch environment if using that integration path.

## Build / Run / Test Commands

```bash
cargo build
cargo test
cargo run -- --help
```

## Notes, Limitations, Or Known Gaps

- This project parses saved pages, not live browser sessions.
- Output quality depends on the stability of the source page format.

## Next Steps Or Roadmap Hints

- Add fixtures as source pages change.
- Clarify the supported source-page families if the scraper broadens.
