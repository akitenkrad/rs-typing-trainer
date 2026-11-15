**English** | [日本語](modes.ja.md)

# Training modes and romaji input

## Modes

| Mode | Prompts | Key selection |
|---|---|---|
| `random` | Pseudo-words made from `mode.keys` | The selected characters are the alphabet |
| `words` | English words | `include` or `only` |
| `romaji` | Japanese words shown with their kana reading | `include` or `only`, evaluated against accepted romaji spellings |
| `sentence` | English or Japanese sentences | No key picker; set `mode.sentence.lang` to `en` or `ja` |
| `ngram` | Two- or three-character transitions | Cartesian `pairs` or n-grams found in the bundled corpus |
| `adaptive` | Cost-weighted weak transitions in real words and drills | Automatic from shrinkage estimates |
| `unlock` | Words and fallback pseudo-words using unlocked keys | Automatic, stored per layout |
| `rhythm` | Words paced one keystroke per metronome beat | Automatic |
| `burst` | A 10–15-word sprint, repeated three times | Automatic |
| `endurance` | A 3–5-minute sentence session | No key picker; uses the sentence language |

The title opens the mode selector. `words`, `romaji`, and `ngram` then open the keyboard key picker. Move with the arrow keys, press `Space` to toggle a key, `Tab` to switch between `include` and `only`, and `Enter` to start. The selection is saved in `[mode]` without rewriting unrelated TOML keys.

`include` keeps prompts containing at least one selected key and weights the draw by the number of selected-key occurrences. `only` keeps prompts that can be typed using the selected keys plus `base_keys`; use `none`, `home`, or a literal character string. For romaji, any accepted spelling is sufficient, so `ち` can qualify through `chi` even when `t` is unavailable. An empty `keys` value disables key filtering for `words`, `romaji`, and `sentence`; `random` and `ngram` instead return to the key picker and ask for a selection.

The previous ten prompts are excluded from each draw while enough candidates remain. With fewer than 20 matches, the UI warns. Only `words` with `only` adds pseudo-words, made strictly from the selected keys. `romaji`, `sentence`, and `words` with `include` repeat real candidates by relaxing the previous-ten rule.

Japanese sentences display the surface form, kana reading, and live romaji guide as three independently wrapped rows. Completed kana use a distinct style. Sentence mode never applies the per-item timeout; setting `item_timeout_seconds = 0` also hides the timer in every other mode.

`adaptive`, `unlock`, `rhythm`, `burst` and `endurance` start directly. Press `P` on the mode-selection screen to toggle the pacer for `words`, `sentence` or `adaptive`; press `L` on `adaptive` to switch its practice language independently of the interface; use `[` and `]` for zero through three preview items. See [Training tools](training.md) for goals, initial values and configuration.

## Daily rotation

Practice modes use a deterministic daily deck. The complete bundled catalog remains available separately for fixed benchmarks. `include` and `only` filter the complete candidate population before it is divided into high-frequency core and broad-tail pools. A user-specific random salt stored in the session database keeps two installations from receiving the same deck.

The default mix is 65% core. Previously mistyped items return after 1, 3, 7, and 14 days, up to 10% of a deck. Sentences not shown before are selected first, then less-exposed sentences. Weekly themes favor digits and symbols on Wednesday, one Japanese author on Friday, and a 50% broad tail on Sunday. The theme appears on the title and mode-selection screens.

```toml
[practice]
daily_rotation = true
core_ratio = 0.65
weekly_theme = true
```

Set `daily_rotation = false` to use the P3 random selection unchanged. While playing `romaji` or Japanese `sentence`, press `F2` to report an incorrect reading, skip it immediately, and exclude it on later starts.

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

Set any of `data.words_en`, `data.words_ja`, `data.sentences_en`, or `data.sentences_ja` to replace that bundled sample category. Blank lines and lines beginning with `#` are ignored.

English words and sentences use one UTF-8 TSV column:

```text
quiet
river
```

Japanese words and sentences use `text<TAB>reading`:

```text
公園	こうえん
短い手紙を書く。	みじかいてがみをかく。
```
