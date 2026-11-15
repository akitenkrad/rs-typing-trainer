# Bundled data generator

This directory is an independent Cargo project. It is intentionally outside the application package so the embedded morphological dictionary and archive readers do not become application dependencies.

## Toolchain and dictionary

The generator pins lindera 6.2.0 and enables its `embed-ipadic` feature. During the first build, `lindera-ipadic` downloads `mecab-ipadic-2.7.0-20250920.tar.gz` from `https://Lindera.dev/`, verifies it with the checksum recorded by the crate, builds the dictionary, and embeds it in the generator binary. `tools/gen-data/.cargo/config.toml` points `LINDERA_BUILD_DICTIONARY_CACHE_DIR` at the ignored `target/lindera-cache` directory. An environment value can override that location.

The generator uses IPADIC's `reading` field and never its `pronunciation` field. Sentence readings preserve the written particles `は`, `へ`, and `を`; ruby readings override morphological readings at annotated positions. A sentence containing a token whose reading is `*` is excluded. Japanese words are counted by IPADIC base form, and each base form is segmented again so its reading is obtained from the base form rather than copied from an inflected occurrence.

## Inputs

Use the supplied input directory, or recreate its layout with:

```sh
tools/gen-data/sources/fetch.sh /path/to/p4in
```

The script waits two seconds after each download. Existing non-empty files are left in place. `sources/aozora_selection.tsv` records the selected Japanese works, and `sources/public_domain_books.tsv` records the number, title, and author of each selected public-domain English book.

The same script downloads `shinonome-0.9.11p1.tar.bz2` over the upstream HTTP URL, verifies SHA-256 `95663c95c92ba5765f63ccbdf033eb93b707be01812a989c548db943479c838f`, and extracts it under `shinonome/`.

## Generate

From the repository root:

```sh
cargo run --release --manifest-path tools/gen-data/Cargo.toml -- \
  --input /path/to/p4in --extra-aozora /path/to/extra/aozora --out data/
```

Pass `--date YYYY-MM-DD` to choose the date in `data/manifest.toml`. Without it, the generator uses the latest modification date among the input files. It never uses the execution time.

Generate the cached bitmap fonts and reconcile their notices and manifest entries with:

```sh
cargo run --release --manifest-path tools/gen-data/Cargo.toml -- \
  --font-source /path/to/p4in/shinonome/shinonome-0.9.11 \
  --out data/font --font-notice data/NOTICE --font-manifest data/manifest.toml
```

The font command reads only the 12-dot and 16-dot JIS X 0208/JIS X 0201 BDF files. It emits Unicode-sorted fixed records in deterministic gzip streams, converts `LICENSE` and `AUTHORS` from EUC-JP to UTF-8, and updates its dedicated NOTICE and manifest blocks idempotently.

Run the font command after the data command: the data command rewrites `data/NOTICE` and `data/manifest.toml` from scratch and drops the font blocks, which the font command then restores. The manifest records the archive's SHA-256 and the SHA-256 of every file read from the extracted tree (the four BDF files, `LICENSE`, and `AUTHORS`). When `shinonome-0.9.11p1.tar.bz2` sits next to the extracted directory, as `fetch.sh` leaves it, the font command verifies its SHA-256 and stops on a mismatch.

The font files start with the 8-byte magic `SHNFONT\0`, a format version byte (1), the dot height, the halfwidth and fullwidth dot widths, a little-endian `u32` glyph count, and four reserved bytes. Each glyph record is a little-endian `u32` Unicode scalar value, a width byte, and one little-endian `u16` per dot row with the leftmost dot in the most significant bit. Records are sorted by code point.

`--extra-aozora` is optional and points directly to a directory containing `selection.tsv` and `zips/`. Missing archives in that supplemental selection are reported and skipped. The generator writes gzip-compressed TSV files to the selected output directory and review candidates to `tools/gen-data/review/`. Repeated writes are content-aware and replace changed files atomically. Gzip timestamps are fixed at zero; sampling uses a fixed seed and an in-tree FNV-1a implementation; maps and tie-breaking use sorted order.

## Review and overrides

- `review/ja_words_top600.tsv`: surface, reading, rank, and ruby comparison result.
- `review/ja_sentences_candidates.tsv`: text, reading, and source work ID.
- `overrides/ja_readings.tsv`: `surface<TAB>reading`, or `surface<TAB>-` to exclude the word.
- `overrides/ja_sentences.tsv`: `text<TAB>reading`, or `text<TAB>-` to exclude the sentence.
- `reviewed/ja_words.tsv` and `reviewed/ja_sentences.tsv`: immutable hand-reviewed pairs emitted with `tier=core`.

Run the generator again after changing an override file.
