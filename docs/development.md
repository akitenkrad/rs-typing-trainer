**English** | [日本語](development.ja.md)

# Development

## Building

```sh
cargo build --release
cargo run --release
cargo run --release -- --lang ja
```

rs-typing-trainer uses Rust edition 2024, [ratatui](https://ratatui.rs) 0.30 for drawing and [clap](https://docs.rs/clap) 4 for command-line parsing. Terminal access uses the `ratatui::crossterm` re-export so ratatui and the application cannot select incompatible crossterm versions.

## Source layout

| File | Role |
|---|---|
| `src/main.rs` | Command dispatch, terminal setup and the event loop |
| `src/lib.rs` | Exposes modules to the binary and integration tests |
| `src/cli.rs` | clap definitions for play, replay, stats and export |
| `src/i18n.rs` | English and Japanese text shown to the user |
| `src/config.rs` | v2 TOML loading, v1 mapping, validation and comment-preserving writes |
| `src/data/` | Bundled TSV inclusion and user-data loading |
| `data/` | Generated English and Japanese prompt datasets, manifest and notices |
| `tools/gen-data/` | Independent deterministic data generator and review files |
| `src/layout/` | Physical geometry, bundled TOML layouts, reverse lookup and remapping |
| `src/app.rs` | Screen state and the bridge between the UI and engine |
| `src/engine/` | Items, exact/romaji judges, mode generators and deterministic session progression |
| `src/game/` | Input classification, timers and word generation |
| `src/analysis.rs` | Metrics calculated from item and keystroke logs |
| `src/analysis/` | Detailed key, bigram, finger and CSV analysis |
| `src/bench.rs` | Deterministic `bench-v1` parallel-form generation |
| `src/metrics/` | Result-screen compatibility types |
| `src/record/` | SQLite v2, v1 migration, storage paths and `.ttrec` files |
| `src/replay.rs` | Timestamp scheduling, stepping and log-derived replay state |
| `src/ui/` | ratatui screens, keyboard drawing and animation helpers |
| `examples/gen_sample_records.rs` | Fixed-seed synthetic record generator |

## Tests

```sh
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo fmt --check
cargo run --locked --example gen_sample_records
cargo test --locked --manifest-path tools/gen-data/Cargo.toml
cargo clippy --all-targets --locked --manifest-path tools/gen-data/Cargo.toml -- -D warnings
```

Integration tests are compiled into two test binaries instead of one per file. `tests/all/main.rs` declares every file under `tests/all/` as a module, and `tests/perf/main.rs` holds the image-creation timing tests, which run one at a time and assert their budgets only in release builds. Filter by binary and module path:

```sh
cargo test --locked --test all                     # every integration test except timing
cargo test --locked --test all menu_ui::           # one former test file
cargo test --locked --test all menu_ui::menu_step  # tests in that module whose names start with the text
cargo test --locked --test all localized           # tests in any module whose names contain the text
cargo test --locked --test perf -- --nocapture     # timing tests with their reports
```

A new integration test file goes in `tests/all/` and needs a `mod` line in `tests/all/main.rs`. Shared helpers are `crate::common`. All tests in a binary share one process and run in parallel, so a test must not change environment variables or the current directory, and must use its own temporary directory.

Unit tests live beside the implementation. Integration tests in `tests/all/` also cover the romaji examples and properties, every training mode, benchmark parallel forms and rotation, hand-calculated analysis metrics and shrinkage, analysis tabs, CSV filters, key filtering and recent-item exclusion, advance-mode editing metrics, v1/v2 configuration precedence, bundled and user TSV data, mode/key/romaji/sentence rendering, timestamp-injected `.ttrec` replay, sample records, configuration-document defaults, SQLite migration, layout vectors, remapping, Press/Release/Repeat handling, comment-preserving writes, CLI behavior and translations.

Layout definitions are compiled with `include_str!`; each `src/layout/defs/*.toml` file records its primary source URL in the first comment. Integration tests construct `App` through `tests/all/common/mod.rs`, which always supplies a temporary home/database path. Home-directory discovery occurs only in `src/main.rs`; library path resolution requires an explicit home path. Tests do not read the user's configuration or session database.

The application package embeds `data/*.tsv.gz` with `include_bytes!` and decompresses each dataset once. The data generator is not a workspace member and keeps lindera, IPADIC, and archive dependencies out of the application. See [Prompt data](data.md) for regeneration and review. The smaller sample-record generator is a normal example target and uses only the application dependencies.
