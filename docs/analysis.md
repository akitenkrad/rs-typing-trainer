**English** | [日本語](analysis.ja.md)

# Benchmarks and analysis

## `bench-v1`

Press `B` on the title screen to run the benchmark. Parts A–C run in sequence; part D is optional.

| Part | Material | Time |
|---|---|---:|
| A | the 200 highest-ranked bundled English words | 60 s |
| B | 3–6 letter Markov pseudowords covering a–z and the four transition classes | 60 s |
| C | romaji for the 300 highest-ranked bundled core Japanese words | 60 s |
| D | digits and symbols | 45 s |

Every part has eight deterministic parallel forms (`bench-v1/A/01` through `08`). A, C and D use the same item multiset in every form and shuffle it with fixed seeds. For B, a first-order Markov model is built from the top 5,000 bundled English words. Each form independently generates and greedily selects 240 pseudowords while balancing character and transition-class shares and maximizing distinct bigrams. Exact matches to bundled English words, periodic words and words that repeat a bigram are excluded. The eight B forms use different words rather than shuffled copies. After a completed benchmark, the next run uses the next form and wraps from 08 to 01.

Benchmark settings are fixed: stop-on-error, no item timeout and one preview item. User session settings do not alter them. Benchmark material is loaded from the complete bundled data before daily decks, exposure counts, user salts, weekday themes or reported-reading exclusions are applied. Japanese benchmark items must have `tier = core`.

Each part is stored as a separate session with `mode = bench` and its full form ID in `bench_id`. Comparisons only use the same `bench-v1` part. Each part result names the part and form, shows the preceding-result and recent-three comparisons, and names the next part. The last part leads to a run summary. `Esc` ends the run while retaining completed-part records.

## Metrics

- Net WPM: correctly retained characters divided by five, per minute. In advance mode, unresolved incorrect characters do not count.
- Raw WPM: character presses excluding Backspace, divided by five, per minute.
- Accuracy: correct presses divided by all character presses in `stop`; correct positions divided by all positions remaining in the editable buffer in `advance`.
- IKI: time between consecutive character presses in one item. The first press belongs to reaction time, not IKI. Intervals above `[analysis].pause_ms` (default 2000 ms) are pauses and are excluded.
- Key metrics: accuracy and the median/p90 IKI into each expected key.
- Bigram metrics: median IKI and error rate for an expected-key transition. An IKI sample is admitted only when both consecutive presses are correct and belong to one item; a correct press following a stop-mode error therefore does not create a transition sample.
- Finger, hand and row metrics: accuracy and median IKI using the physical geometry and practice layout. Same-finger transitions compare different keys assigned to one finger; alternating-hand transitions are reported separately.
- Rhythm: population coefficient of variation of admitted IKI samples (`standard deviation / mean`).
- Reaction: item display to its first character press.
- Correction: incorrect press to the next correct press in stop mode, or to Backspace in advance mode.
- Hold and rollover: release minus press time and the fraction of adjacent presses where the next press precedes the prior release. These are calculated only for sessions with `release_events = 1`.
- Japanese spelling choice: counts alternative accepted sequences such as `shi`/`si`, `chi`/`ti` and `tsu`/`tu`.

Sessions imported from v1 have `metrics_version = 0` and no keystroke log. They remain in WPM/accuracy trends and the `sessions` CSV, but their `iki_median_ms` and `iki_cv` CSV fields are empty. They are excluded from the IKI trend, keystroke-derived metrics and benchmark analysis.

### Shrinkage for transitions

Sparse bigrams are pulled toward their transition-class mean:

```text
estimate = (n * sample_mean + k * class_mean) / (n + k)
```

`[analysis].shrinkage_k` defaults to 10. The four classes are same finger, same hand with adjacent fingers, same hand with non-adjacent fingers and alternating hands. Error rates use the class error rate as a Beta prior with strength `k`. Displays and CSV always include `n`.

## Eight skill axes

The benchmark tab reports sustained speed, accuracy, rhythm, transitions, reach, reaction, correction and Japanese. Speed and Japanese use net WPM; rhythm uses IKI CV; transitions compare slow bigrams with overall IKI; reach compares remote rows with the home row; reaction and correction use their median times. Each latest value is placed in the user's own historical distribution for that benchmark part. With fewer than three results, only the raw value is shown. An axis without usable samples is shown as no data rather than zero.

## Analysis screen

Press `A` on the title screen. `Tab` or `←/→` changes tabs. `P` cycles all time, 30 days and 7 days; `M` cycles the mode filter. Every tab, including Summary, uses the records left by these filters.

- Summary: a deterministic, rule-based assessment; use `↑`/`↓` to scroll when it does not fit. It reports current seven-day medians by the one or two most frequent modes, the latest benchmark and its previous-result delta, the slope of weekly medians over four weeks, the strongest and weakest available benchmark axes, three weak transitions and keys, one to three practice recommendations, and data coverage. Sparse key or transition results with fewer than 10 samples are marked preliminary. A benchmark at least seven days old triggers a reminder.
- Trend: daily medians for net WPM, accuracy and median IKI, separated by mode or benchmark part. Charts show endpoint dates and value bounds after two valid days have accumulated; before that they show the current value.
- Keyboard: error-rate and median-IKI heatmaps; press `Enter` to switch.
- Transitions: the 20 slowest and 20 most error-prone shrunk estimates. Select a slow transition and press `Enter` to prepare two-key `ngram` practice.
- Fingers/Hands: finger accuracy/IKI and same-finger versus alternating-hand timing.
- Bench: latest part results, comparisons and the eight axes.

The four-week slope uses benchmark A when it is available, then another available benchmark part; without a benchmark it uses practice records and explicitly warns that practice WPM is not evidence of improvement. Slopes above `+0.5 WPM/week` are upward, below `-0.5 WPM/week` are downward, and values in between are flat. Recommendations follow fixed rules: low accuracy selects a 95% accuracy gate with the pacer off; a slow transition selects `adaptive` and names the pair; weak reach selects `unlock` or `ngram`; weak reaction increases look-ahead; weak rhythm selects `rhythm`; second-half endurance deterioration selects `endurance` plus a break; and weak Japanese selects `romaji`.

## CLI and CSV columns

`rs-typing-trainer stats` prints a summary table. CSV export accepts `--since YYYY-MM-DD` and `--mode MODE`:

```sh
rs-typing-trainer stats --csv sessions
rs-typing-trainer stats --csv keys --since 2026-10-01 --mode words
rs-typing-trainer stats --csv bigrams
rs-typing-trainer stats --csv fingers
rs-typing-trainer stats --summary --since 2026-10-01 --mode words
```

`stats --summary` prints the same rule-based content as the Summary tab and follows `--lang`. It cannot be combined with `--csv`.

| CSV | Columns |
|---|---|
| `sessions` | `id, started_at, finished_at, duration_ms, mode, bench_id, layout, net_wpm, raw_wpm, accuracy, iki_median_ms, iki_cv, metrics_version` |
| `keys` | `key, code, correct, total, accuracy, iki_n, iki_median_ms, iki_p90_ms` |
| `bigrams` | `from, to, class, n, median_ms, error_n, attempts, error_rate, estimated_iki_ms, estimated_error_rate` |
| `fingers` | `finger, correct, total, accuracy, n, iki_median_ms` |

## Measurement cautions

Repeated text measures memory as well as typing, which is why benchmark forms rotate. Do not trust rare bigrams without their count and shrunk estimate. Judge progress with benchmarks rather than adaptive practice scores. Hold and rollover values require terminal release events and should not be mixed with sessions that lack them. Japanese parts measure direct romaji input, not IME conversion and candidate selection.
