**English** | [日本語](data.ja.md)

# Prompt data

rs-typing-trainer bundles four deterministic gzip-compressed TSV datasets. They are embedded with `include_bytes!`, decompressed once at startup, and cached. Their exact input hashes, generation date, filters, output hashes, lindera version, and dictionary version are recorded in [`data/manifest.toml`](../data/manifest.toml). Copyright and attribution details are in [`data/NOTICE`](../data/NOTICE).

## Bundled files

| File | Rows | Columns |
|---|---:|---|
| `data/en_words.tsv.gz` | 20,000 | `word`, `rank`, `tier` |
| `data/ja_words.tsv.gz` | 12,000 | `surface`, `reading`, `rank`, `tier` |
| `data/en_sentences.tsv.gz` | 19,696 | `text`, `tatoeba_id`, `tier` |
| `data/ja_sentences.tsv.gz` | 7,394 | `text`, `reading`, `source_id`, `author`, `tier` |

Readings are hiragana with the punctuation `、。「」`. A long-vowel mark appears only when it is also present in the written prompt. The source ID for a Japanese sentence is the six-digit work ID in `tools/gen-data/sources/aozora_selection.tsv`. `tier=core` contains 2,000 English words, 2,070 English sentences, 561 hand-reviewed Japanese words, and 537 hand-reviewed Japanese sentences; all other rows use `tier=ext`.

## Sources and licenses

- English word frequencies come from 30 public-domain books plus Tatoeba's CC0 English sentences. SCOWL levels through 60 restrict the result to English and American spellings. The complete SCOWL copyright and permission text is retained in `data/NOTICE`.
- Japanese words and sentences come from 560 public-domain Japanese works. `data/NOTICE` retains each title, author, and input contributor.
- Japanese readings are produced with lindera 6.2.0 and the embedded IPADIC `mecab-ipadic-2.7.0-20250920` dictionary. The complete NAIST permission and warranty disclaimer, including the ICOT terms, is retained in `data/NOTICE`.
- English sentences come from Tatoeba's CC0 English export. The source is credited as `tatoeba.org` even though CC0 does not require attribution.

Public-domain status can vary by jurisdiction. Check the original works if distributing the data where a different copyright term applies.

## Processing

English book headers and distribution notices are removed at the `START OF` and `END OF` markers. ASCII words are counted case-insensitively, intersected with the selected SCOWL lists, checked against `tools/gen-data/blocklist_en.txt`, and ranked by frequency.

Japanese archives are decoded from Shift_JIS. The title, author, legend, annotations, ruby markup, and colophon are removed from the body, while ruby pairs are retained for reading checks. IPADIC nouns, independent verbs and adjectives, and adverbs are counted by base form. Each base form is segmented again and its `reading` fields are joined to obtain the word reading.

Sentence candidates have 15–45 characters, balanced Japanese quotation marks, and only the documented Japanese characters and punctuation. Their readings join IPADIC `reading` fields, not `pronunciation` fields. The written particles `は`, `へ`, and `を` are preserved, ruby readings take precedence at annotated positions, and candidates containing an unknown `*` reading are excluded. The review and override files record the manual correction stage.

English CC0 sentences have 30–90 printable ASCII characters. Every matching unique, non-blocklisted sentence is included. Japanese works contribute at most 15 sentences each. Stable maps, tie-breakers, and FNV-1a ordering make regeneration deterministic.

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
```

See [`tools/gen-data/README.md`](../tools/gen-data/README.md) for the dictionary cache, review files, override formats, and deterministic date handling.

## User files

Configuration paths replace only the corresponding bundled category. User English word and sentence files contain one prompt per non-empty line. User Japanese files contain two tab-separated fields:

```text
text<TAB>reading
```

Lines beginning with `#` and blank lines are ignored. Japanese readings must be typeable by the romaji judge.
