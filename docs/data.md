**English** | [日本語](data.ja.md)

# Prompt data

rs-typing-trainer bundles six deterministic gzip-compressed TSV datasets. They are embedded with `include_bytes!`, decompressed once at startup, and cached. Their exact input hashes, generation date, filters, output hashes, lindera version, and dictionary version are recorded in [`data/manifest.toml`](../data/manifest.toml). Copyright and attribution details are in [`data/NOTICE`](../data/NOTICE).

The application also bundles the public-domain Shinonome 0.9.11 font at 12 and 16 dots. Both files contain 6,879 JIS X 0208 fullwidth glyphs, 95 printable ASCII glyphs, and 63 JIS X 0201 halfwidth-kana glyphs, keyed by Unicode code point. `data/NOTICE` includes the complete UTF-8-converted `LICENSE` and `AUTHORS` files and the upstream URL.

## Bundled files

| File | Rows | Columns |
|---|---:|---|
| `data/en_words.tsv.gz` | 20,000 | `word`, `rank`, `tier` |
| `data/ja_words.tsv.gz` | 12,000 | `surface`, `reading`, `rank`, `tier` |
| `data/en_phrases.tsv.gz` | 9,069 | `text`, `tatoeba_id`, `tier` |
| `data/ja_phrases.tsv.gz` | 1,989 | `surface`, `reading`, `rank`, `tier` |
| `data/en_sentences.tsv.gz` | 19,705 | `text`, `tatoeba_id`, `tier` |
| `data/ja_sentences.tsv.gz` | 7,394 | `text`, `reading`, `source_id`, `author`, `tier` |
| `data/font/shinonome12.bin.gz` | 7,037 glyphs | fixed binary records |
| `data/font/shinonome16.bin.gz` | 7,037 glyphs | fixed binary records |

Each uncompressed font starts with a 20-byte header: `SHNFONT\0`, format version, dot height, halfwidth pixel width, fullwidth pixel width, a little-endian glyph count, and four reserved bytes. Code-point-sorted fixed records then contain a little-endian `u32` Unicode value, one width byte, and `height` little-endian `u16` rows. Pixel x is bit `15-x`. Gzip timestamps are zero. The compressed 12-dot and 16-dot files are 113,046 and 150,545 bytes respectively.

Readings are hiragana with the documented punctuation; phrase readings may also contain `！` and `？`. A long-vowel mark appears only when it is also present in the written prompt. The source ID for a Japanese sentence is the six-digit work ID in `tools/gen-data/sources/aozora_selection.tsv`. `tier=core` contains 2,000 English words, 1,886 English phrases, 2,068 English sentences, 561 hand-reviewed Japanese words, all 1,989 original Japanese phrases, and 537 hand-reviewed Japanese sentences; all other rows use `tier=ext`.

## Sources and licenses

- English word frequencies come from 30 public-domain books plus Tatoeba's CC0 English sentences. SCOWL levels through 60 restrict the result to English and American spellings. The complete SCOWL copyright and permission text is retained in `data/NOTICE`.
- Japanese words and sentences come from 560 public-domain Japanese works. `data/NOTICE` retains each title, author, and input contributor.
- Japanese readings are produced with lindera 6.2.0 and the embedded IPADIC `mecab-ipadic-2.7.0-20250920` dictionary. The complete NAIST permission and warranty disclaimer, including the ICOT terms, is retained in `data/NOTICE`.
- English phrases and sentences come from Tatoeba's CC0 English export. The source is credited as `tatoeba.org` even though CC0 does not require attribution.
- Japanese phrases were written for this project and use the same license as the repository.
- Large prompt glyphs come from Shinonome 0.9.11 at `http://openlab.ring.gr.jp/efont/shinonome/`. Its archive declares all included font data, documents, and scripts to be provided as public domain; the complete terms and author list are in `data/NOTICE`.

Public-domain status can vary by jurisdiction. Check the original works if distributing the data where a different copyright term applies.

## Processing

English book headers and distribution notices are removed at the `START OF` and `END OF` markers. ASCII words are counted case-insensitively, intersected with the selected SCOWL lists, checked against `tools/gen-data/blocklist_en.txt`, and ranked by frequency.

Japanese archives are decoded from Shift_JIS. The title, author, legend, annotations, ruby markup, and colophon are removed from the body, while ruby pairs are retained for reading checks. IPADIC nouns, independent verbs and adjectives, and adverbs are counted by base form. Each base form is segmented again and its `reading` fields are joined to obtain the word reading.

Sentence candidates have 15–45 characters, balanced Japanese quotation marks, and only the documented Japanese characters and punctuation. Their readings join IPADIC `reading` fields, not `pronunciation` fields. The written particles `は`, `へ`, and `を` are preserved, ruby readings take precedence at annotated positions, and candidates containing an unknown `*` reading are excluded. The review and override files record the manual correction stage.

English CC0 phrases have 10–29 printable ASCII characters, while sentences have 30–90. For rows made entirely of quoted dialogue parts, the surrounding double quotes are removed and the trimmed parts are joined with one space before filtering. Every matching unique, non-blocklisted row is included, retaining the first Tatoeba ID for duplicate text.

The Japanese phrase source is `tools/gen-data/phrases/ja_phrases.tsv`. Its surface and reading must be unique, readings must contain 6–20 hiragana, long-vowel marks, or `！`/`？`, and surfaces reject ASCII alphanumerics, whitespace, and sentence punctuation. Rows are ranked by reading length, preserving source order for ties, and all use `tier=core`. IPADIC comparisons are written to `tools/gen-data/target/regen-out/ja_phrases_mismatch.tsv` without changing the authored readings. Japanese works contribute at most 15 sentences each. Stable maps, tie-breakers, and FNV-1a ordering make regeneration deterministic.

## Fixed data and daily practice decks

The data API has two entry points. The fixed complete catalog is reserved for reproducible benchmarks. Practice modes derive a daily deck from the complete catalog after applying `include` or `only` to the candidate population. The seed combines the local `YYYY-MM-DD` date with a random 32-byte user salt stored once in the database `meta` table.

Daily decks normally contain 65% high-frequency core items and 35% broad-tail items. Due mistakes return 1, 3, 7, and 14 days after the last mistake, up to 10% of the deck. Sentence selection prefers `item_exposure.count = 0`, then the least-exposed rows. Wednesday favors sentences with digits or symbols, Friday favors one Japanese author, and Sunday uses a 50% broad tail. The current theme appears on the title and mode-selection screens.

Press `F2` while practicing Japanese words or sentences to report a wrong reading. The item is skipped and excluded on later starts. Reports are appended as `timestamp<TAB>kind<TAB>text<TAB>reading` to `reading_reports.tsv` beside the configured database.

## Regeneration

The generator is a separate Cargo project. Recreate the input tree if necessary, then generate from the repository root:

```sh
tools/gen-data/sources/fetch.sh /path/to/p4in
cargo run --release --manifest-path tools/gen-data/Cargo.toml -- \
  --input /path/to/p4in --extra-aozora /path/to/extra/aozora --out data/
cargo run --release --manifest-path tools/gen-data/Cargo.toml -- \
  --font-source /path/to/p4in/shinonome/shinonome-0.9.11 --out data/font \
  --font-notice data/NOTICE --font-manifest data/manifest.toml
```

Run the font command second: the data command rewrites `data/NOTICE` and `data/manifest.toml`, and the font command adds the font notice, the input hashes (the archive and every file read from it), and the font outputs back. `fetch.sh` checks the archive against SHA-256 `95663c95c92ba5765f63ccbdf033eb93b707be01812a989c548db943479c838f`, and the font command repeats that check when the archive sits next to the extracted directory. Running both commands twice leaves `data/` unchanged.

See [`tools/gen-data/README.md`](../tools/gen-data/README.md) for the font file format, the dictionary cache, review files, override formats, and deterministic date handling.

## User files

Word and sentence configuration paths replace only the corresponding bundled category. Phrase paths add prompts to the bundled phrase category, retaining the bundled row when a prompt is duplicated. User English word, phrase, and sentence files contain one prompt per non-empty line. User Japanese files contain two tab-separated fields:

```text
text<TAB>reading
```

Lines beginning with `#` and blank lines are ignored. Japanese readings must be typeable by the romaji judge.
