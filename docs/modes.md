**English** | [日本語](modes.ja.md)

# Training modes and romaji input

## Individual training

Individual training is a table. Each row is the **skill** a session trains, each column **what you type**. Move with the arrow keys or `h` `j` `k` `l` and press `Enter` on a cell. Under the table, two lines say what the cell trains and what its prompts are.

| Skill \ Prompts | Letters | Combos | Words | Phrases | Sentence | Passage |
|---|---|---|---|---|---|---|
| Normal | ○ | ○ | ○ | ○ | ○ | ○ |
| Accuracy, Speed, Recovery, No-look | ○ | ○ | ○ | ○ | ○ | ○ |
| Anticipation, Look-ahead, Rollover | — | — | ○ | ○ | ○ | ○ |
| Spelling (Japanese only) | — | — | ○ | ○ | ○ | ○ |
| Weak transitions | — | ○ | ○ | ○ | ○ | — |
| Rhythm | — | ○ | ○ | ○ | ○ | — |
| Burst | — | — | ○ | ○ | ○ | — |
| Endurance | — | — | ○ | ○ | ○ | ○ |
| Unlock keys | ○ | — | ○ | — | — | — |

A `—` cell is shown dimmed. The cursor stops on it and the line under the table says why it cannot be played; `Enter` does nothing there. Spelling practices romaji, so its cells are `—` while the practice language is English.

Every step of [today's training](training.md#steps-are-cells-of-the-table) is one of these cells with the settings of its level.

### Skills (rows)

| Skill | What it does | Settings it applies |
|---|---|---|
| Normal | The plain prompts with your own settings | Pacer off |
| Accuracy | A typo stops the cursor; a run under 97% accuracy does not rank | The first level of the menu's `accuracy.gate` (`session.error_mode = "stop"`, `gate.min_accuracy = 0.97`, pacer off) |
| Speed | A pacer 5% faster than your recent speed runs above the prompt, on every column | The first level of `speed.pacer` (`pacer.enabled = true`, `pacer.ratio = 1.05`) |
| Recovery | A typo moves on and the missed prompt comes back once | The first level of `recovery.retry` (`session.error_mode = "advance"`, `session.retry_on_error = true`) |
| No-look | Keys you know well fade from the on-screen keyboard | The first level of `blind.fade` (`keyboard.show = "auto"`) |
| Anticipation | A pacer 10% slower than your recent speed; confirm each letter before pressing it | The first level of `accuracy.anticipation` (`pacer.enabled = true`, `pacer.ratio = 0.90`) |
| Look-ahead | Three prompts ahead are shown and typed text is hidden in sentences; read the next one, not the letter under the cursor | The first level of `preview.wide` (`session.preview_items = 3`, `mode.sentence.hide_typed = true`) |
| Rollover | Press the next key before releasing the last one, staying relaxed | The first level of `overlap.burst` (pacer off) |
| Spelling | Keep to the romaji spelling the analysis suggests (Japanese only) | The first level of `japanese.spelling` (`mode.key_filter = "include"`) |
| Weak transitions | More of your slow or error-prone key transitions (`adaptive`) | Pacer off |
| Rhythm | One keystroke per metronome beat; the result shows the median drift from the beat (`rhythm`) | Pacer off |
| Burst | Three all-out sprints; the fastest is recorded (`burst`) | Pacer off |
| Endurance | `endurance.minutes` (3–5) without a break; the first and second halves are compared (`endurance`) | Pacer off |
| Unlock keys | Start with a few keys and add one when you are fast on the newest (`unlock`) | Pacer off |

A cell's settings apply to that session only, the same way a step of today's training does: they are put on an in-memory copy of the configuration, your configuration file is not changed, and the next session starts from your own settings again. `Enter` on the result screen plays the same cell once more.

### Prompts (columns)

| Column | Prompts |
|---|---|
| Letters | Pseudo-words made from the selected keys (`mode.keys`); for Unlock keys, from the unlocked keys |
| Combos | Two- or three-character transitions of the selected keys (`mode.ngram`); for Weak transitions, the drills of your weak transitions |
| Words | English words, or Japanese words typed in romaji |
| Phrases | English or Japanese phrases of the course chosen with `C` |
| Sentence | One English or Japanese sentence |
| Passage | English: three to five bundled sentences in a row as one prompt. Japanese: consecutive sentences of one Aozora Bunko work, in the order of the text |

How the new combinations work:

- **Weak transitions × Phrases / Sentence**: phrases or sentences that contain more of the focus transitions of the adaptive plan are drawn more often (weight 1 + 4 × occurrences); when at least 20 of them contain one, the rest are left out. Without enough records the prompts are drawn evenly. **Weak transitions × Combos** keeps only the plan's drills.
- **Rhythm × Combos / Phrases / Sentence**: the metronome and its measurement are unchanged; only the prompts differ.
- **Burst × Phrases / Sentence**: a sprint is `burst.words` ÷ 3 phrases (4 by default) or one sentence, about the same distance as the word sprint.
- **Endurance × Words / Phrases / Passage**: the same halves comparison over words, phrases or passages.

Phrases, sentences and passages have no per-item timer in any row. Bundled sentences and passages show their source under the prompt (Tatoeba numbers, or the Aozora Bunko work).

### Practice language

The table has one practice language, shown above it; `L` switches English ⇄ Japanese for every cell. Words become `words` ⇄ `romaji`, and `mode.sentence.lang`, `mode.phrase.lang` and `adaptive.lang` follow it. Letters, Combos (except Weak transitions) and Unlock keys always use English letters and are recorded as English. The language and the cell are saved in `[drill]` and `[mode]` and come back on the next start.

### Other keys

`W` plays the 1-minute warm-up of today's training on its own in the practice language (see [Warm-up on its own](training.md#warm-up-on-its-own)). `C` cycles the phrase course while the cursor is in the Phrases column: short, normal, long and mixed. English length ranges are 10–15, 16–22 and 23–29 display characters; Japanese ranges are 6–10, 11–15 and 16–20 reading characters; mixed repeats five short, five normal and five long prompts in that order. `[` and `]` choose zero through three preview items, and `Z` switches the prompt text between large and normal.

### Records

The four rows added for today's training (Anticipation, Look-ahead, Rollover, Spelling) run the plain mode of their column, as Accuracy does. Their Words cells pass through the key picker like the other setting rows.

A cell is recorded under the same `mode` as before the table existed, so history and analysis keep their series: Normal × Words is `words` (or `romaji` in Japanese), Normal × Sentence is `sentence`, Rhythm × Words is `rhythm`, Endurance × Sentence is `endurance`, and so on. Accuracy, Speed, Recovery, No-look, Anticipation, Look-ahead, Rollover and Spelling record the mode of their column (`random`, `ngram`, `words`/`romaji`, `phrase`, `sentence`); Normal × Passage is `sentence`. Every cell also writes `drill` (`{"skill", "unit"}`) and `unit` into `mode_params`, which tells a rhythm session on sentences from one on words. The database schema is unchanged.

## Key selection

Letters, Combos, and the Words cells of the first five rows open the keyboard key picker before starting (Letters only when no key is selected yet). Move with the arrow keys, press `Space` to toggle a key, `Tab` to switch between `include` and `only`, and `Enter` to start. The selection is saved in `[mode]` without rewriting unrelated TOML keys.

`include` keeps prompts containing at least one selected key and weights the draw by the number of selected-key occurrences. `only` keeps prompts that can be typed using the selected keys plus `base_keys`; use `none`, `home`, or a literal character string. For romaji, any accepted spelling is sufficient, so `ち` can qualify through `chi` even when `t` is unavailable. An empty `keys` value disables key filtering for words; Letters and Combos instead return to the key picker and ask for a selection.

The previous ten prompts are excluded from each draw while enough candidates remain. With fewer than 20 matches, the UI warns. Only `words` with `only` adds pseudo-words, made strictly from the selected keys. `romaji`, `sentence`, and `words` with `include` repeat real candidates by relaxing the previous-ten rule.

Japanese sentences, passages and phrases display the surface form, kana reading, and live romaji guide as three independently wrapped rows. Completed kana use a distinct style. Setting `item_timeout_seconds = 0` hides the timer in every timed mode. See [Training tools](training.md) for goals, initial values and configuration.

## Daily rotation

Practice modes use a deterministic daily deck. The complete bundled catalog remains available separately for fixed benchmarks. `include` and `only` filter the complete candidate population before it is divided into high-frequency core and broad-tail pools. A user-specific random salt stored in the session database keeps two installations from receiving the same deck.

The default mix is 65% core. Previously mistyped items return after 1, 3, 7, and 14 days, up to 10% of a deck. Phrases and sentences not shown before are selected first, then less-exposed items. Weekly themes favor digits and symbols on Wednesday, one Japanese author on Friday, and a 50% broad tail on Sunday. The theme appears on the title and individual-training screens.

```toml
[practice]
daily_rotation = true
core_ratio = 0.65
weekly_theme = true
```

Set `daily_rotation = false` to use the P3 random selection unchanged. While playing Japanese words or sentences, press `Ctrl+R` (or `F2`) to report an incorrect reading, skip it immediately, and exclude it on later starts.

## Romaji spellings

The judge retains every spelling that still matches the typed prefix. The displayed guide starts with the representative spelling and follows the user's choice, such as changing `shi` to `si` after `s`, `i`. Katakana in a reading is normalized to hiragana. ASCII letters, digits, and symbols pass through unchanged.

| Kana or class | Accepted spellings |
|---|---|
| し / ち / つ / ふ / じ | `shi si ci` / `chi ti` / `tsu tu` / `fu hu` / `ji zi` |
| か / く / こ / せ | `ka ca` / `ku cu qu` / `ko co` / `se ce` |
| ぢ / づ / ゔ / を | `di` / `du` / `vu` / `wo` |
| しゃ | `sha sya sixya silya shixya shilya` (and the corresponding `shu`/`sho` forms) |
| ちゃ | `cha tya cya chixya chilya tixya tilya` (and `chu`/`cho` forms) |
| じゃ | `ja jya zya jixya jilya zixya zilya` (and `ju`/`jo` forms) |
| Other contracted sounds | `kya`, `nya`, `hya`, `mya`, `rya`, `gya`, `bya`, `pya`, plus explicit small-kana forms |
| Small kana | `la/xa`, `li/xi`, `lu/xu`, `le/xe`, `lo/xo`, `lya/xya`, `lyu/xyu`, `lyo/xyo`, `lwa/xwa` |
| てぃ / でぃ | `thi texi teli` / `dhi dexi deli` |
| ふぁ / ふぃ / ふぇ / ふぉ | `fa fi fe fo`, plus explicit `fu` + small-kana forms |
| しぇ / じぇ / ちぇ | `she sye` / `je jye zye` / `che tye cye`, plus explicit small-kana forms |
| っ | The first consonant of the next spelling, or `ltu xtu ltsu xtsu` |
| ん | `n nn n' xn`; `n'` and `xn` are controlled by `[mode.romaji]` |
| ー / 、 / 。 / ？ / ！ | `-` / `,` / `.` / `?` / `!` |

Before a vowel, a y-row or n-row mora, or another `ん`, a single `n` does not commit `ん`; use `nn`, `n'`, or `xn`. A terminal `ん` likewise needs an explicit spelling. Before another consonant, both `n` and `nn` work. A following punctuation mark is not a consonant mora, so `こうえん、` accepts `kouenn,` and `kouen',` but rejects `kouen,`. For a sokuon, the first consonant of any accepted next spelling is doubled: both `cchi` and `tchi` paths are accepted before `ち`; explicit small-tsu spellings always remain available.

```toml
[mode.romaji]
accept_apostrophe = true
accept_xn = true
```

## Error handling

`session.error_mode = "stop"` records a mistype without moving the cursor. `"advance"` moves past the position even on a mistype; `Backspace` moves back and is recorded as `kind = "backspace"`. Metrics replay the final editable buffer. A corrected error therefore leaves no unresolved error, while an uncorrected position reduces net WPM and accuracy. Raw WPM still counts every character key that was pressed.

Blind practice forces `advance`. With `retry_on_error = true`, a mistyped item is repeated immediately, at most once. Benchmarks ignore both options and retain their fixed input behavior.

## User data files

`data.words_en`, `data.words_ja`, `data.sentences_en` and `data.sentences_ja` replace the corresponding bundled category. `data.phrases_en` and `data.phrases_ja` add user phrases to the bundled phrases; duplicate rows keep the bundled copy. Blank lines and lines beginning with `#` are ignored.

English words, phrases and sentences use one UTF-8 TSV column:

```text
quiet
river
```

Japanese words, phrases and sentences use `text<TAB>reading`:

```text
公園	こうえん
短い手紙を書く。	みじかいてがみをかく。
```
