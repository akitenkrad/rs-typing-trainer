# Bundled data generator

This directory is an independent Cargo project. It is intentionally outside the application package so the embedded morphological dictionary and archive readers do not become application dependencies.

## Toolchain and dictionary

The generator pins lindera 6.2.0 and enables its `embed-ipadic` feature. During the first build, `lindera-ipadic` downloads `mecab-ipadic-2.7.0-20250920.tar.gz` from `https://Lindera.dev/`, verifies it with the checksum recorded by the crate, builds the dictionary, and embeds it in the generator binary. `tools/gen-data/.cargo/config.toml` points `LINDERA_BUILD_DICTIONARY_CACHE_DIR` at the ignored `target/lindera-cache` directory. An environment value can override that location.

The generator uses IPADIC's `reading` field and never its `pronunciation` field. Sentence readings preserve the written particles `は`, `へ`, and `を`; ruby readings override morphological readings at annotated positions. A sentence containing a token whose reading is `*` is excluded. Japanese words are counted by IPADIC base form, and each base form is segmented again so its reading is obtained from the base form rather than copied from an inflected occurrence.

The authored Japanese phrase source is `phrases/ja_phrases.tsv`. The generator validates every row instead of discarding invalid rows, ranks rows by reading length with source order as the tie-breaker, and emits every row with `tier=core`. It also compares the authored reading with an IPADIC reading and writes mismatches to `target/regen-out/ja_phrases_mismatch.tsv` without changing the source.

## Inputs

Use the supplied input directory, or recreate its layout with:

```sh
tools/gen-data/sources/fetch.sh /path/to/p4in
```

The script waits two seconds after each download. Existing non-empty files are left in place. `sources/aozora_selection.tsv` records the selected Japanese works, and `sources/public_domain_books.tsv` records the number, title, and author of each selected public-domain English book.

The same script downloads `shinonome-0.9.11p1.tar.bz2` over the upstream HTTP URL, verifies SHA-256 `95663c95c92ba5765f63ccbdf033eb93b707be01812a989c548db943479c838f`, and extracts it under `shinonome/`. It also downloads BIZ UD Gothic v1.051 as `bizud/v1.051.zip`, verifies SHA-256 `73b7fc086ed39d00219437b68593e86aa535c8590d9975936f7f45e8b3f94bde`, and extracts `morisawa-biz-ud-gothic-1.051/` under `bizud/`.

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
  --bizud-source /path/to/p4in/bizud/morisawa-biz-ud-gothic-1.051 \
  --out data/font --font-notice data/NOTICE --font-manifest data/manifest.toml
```

The Shinonome part of the font command reads only the 12-dot and 16-dot JIS X 0208/JIS X 0201 BDF files. It emits Unicode-sorted fixed records in deterministic gzip streams and converts `LICENSE` and `AUTHORS` from EUC-JP to UTF-8. The BIZ UD part reads `BIZUDGothic-Regular.ttf` and retains the ASCII U+0020–U+007E and JIS X 0208 characters that the font maps. It records the requested, retained, and missing counts and copies the full `OFL.txt` into its NOTICE block. Both parts can be run together or separately; each reconciles its dedicated NOTICE and manifest blocks idempotently without removing the other.

Run the font command after the data command: the data command rewrites `data/NOTICE` and `data/manifest.toml` from scratch and drops the font blocks, which the font command then restores. The manifest records both archives, every source file read, and every generated font. When an archive sits next to its extracted directory, as `fetch.sh` leaves it, the font command verifies its SHA-256 and stops on a mismatch.

The font files start with the 8-byte magic `SHNFONT\0`, a format version byte (1), the dot height, the halfwidth and fullwidth dot widths, a little-endian `u32` glyph count, and four reserved bytes. Each glyph record is a little-endian `u32` Unicode scalar value, a width byte, and one little-endian `u16` per dot row with the leftmost dot in the most significant bit. Records are sorted by code point.

`bizud-gothic.outl.gz` is a deterministic gzip stream whose uncompressed data starts with the 8-byte magic `BIZOUTL\0`, a little-endian `u16` format version (1), `units_per_em` as `u16`, ascender and descender as `i16`, and the glyph count as `u32`. Glyphs are sorted by Unicode scalar value. Each glyph stores a `u32` scalar, a `u16` horizontal advance, a `u32` command-byte length, and the command bytes. Opcodes 0–4 are `moveTo`, `lineTo`, `quadTo`, `curveTo`, and `close`; every point coordinate is rounded to an integer font unit and stored as a zigzag unsigned varint delta from the preceding point. Control points participate in that same point sequence. Hinting instructions and all other TTF tables are omitted. Gzip mtime is zero.

`--extra-aozora` is optional and points directly to a directory containing `selection.tsv` and `zips/`. Missing archives in that supplemental selection are reported and skipped. The generator writes gzip-compressed TSV files to the selected output directory and review candidates to `tools/gen-data/review/`. Repeated writes are content-aware and replace changed files atomically. Gzip timestamps are fixed at zero; sampling uses a fixed seed and an in-tree FNV-1a implementation; maps and tie-breaking use sorted order.

English phrases use the same Tatoeba CC0 processing, blocklist, printable-ASCII check, duplicate handling, and tier calculation as English sentences, with a 10–29 character range instead of 30–90. For English phrases and sentences made entirely of one or more quoted dialogue parts, the generator removes the surrounding double quotes, trims each part, and joins the parts with one ASCII space before applying the length filter.

Japanese phrase surfaces reject ASCII alphanumerics, whitespace, and `、`/`。`; readings must contain 6–20 hiragana, long-vowel marks, or `！`/`？`. Surfaces and readings must each be unique. The IPADIC comparison uses the sentence-reading path: katakana readings are converted to hiragana, written particles `は`, `へ`, and `を` are preserved, and written `！`/`？` are retained.

## Review and overrides

- `review/ja_words_top600.tsv`: surface, reading, rank, and ruby comparison result.
- `review/ja_sentences_candidates.tsv`: text, reading, and source work ID.
- `overrides/ja_readings.tsv`: `surface<TAB>reading`, or `surface<TAB>-` to exclude the word.
- `overrides/ja_sentences.tsv`: `text<TAB>reading`, or `text<TAB>-` to exclude the sentence.
- `reviewed/ja_words.tsv` and `reviewed/ja_sentences.tsv`: immutable hand-reviewed pairs emitted with `tier=core`.

Run the generator again after changing an override file.
