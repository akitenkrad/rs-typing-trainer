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

## Generate

From the repository root:

```sh
cargo run --release --manifest-path tools/gen-data/Cargo.toml -- \
  --input /path/to/p4in --extra-aozora /path/to/extra/aozora --out data/
```

Pass `--date YYYY-MM-DD` to choose the date in `data/manifest.toml`. Without it, the generator uses the latest modification date among the input files. It never uses the execution time.

`--extra-aozora` is optional and points directly to a directory containing `selection.tsv` and `zips/`. Missing archives in that supplemental selection are reported and skipped. The generator writes gzip-compressed TSV files to the selected output directory and review candidates to `tools/gen-data/review/`. Repeated writes are content-aware and replace changed files atomically. Gzip timestamps are fixed at zero; sampling uses a fixed seed and an in-tree FNV-1a implementation; maps and tie-breaking use sorted order.

## Review and overrides

- `review/ja_words_top600.tsv`: surface, reading, rank, and ruby comparison result.
- `review/ja_sentences_candidates.tsv`: text, reading, and source work ID.
- `overrides/ja_readings.tsv`: `surface<TAB>reading`, or `surface<TAB>-` to exclude the word.
- `overrides/ja_sentences.tsv`: `text<TAB>reading`, or `text<TAB>-` to exclude the sentence.
- `reviewed/ja_words.tsv` and `reviewed/ja_sentences.tsv`: immutable hand-reviewed pairs emitted with `tier=core`.

Run the generator again after changing an override file.
