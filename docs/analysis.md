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

Transition IKI is modelled on the logarithmic scale. Estimates follow a four-level hierarchy: the player's overall mean, hand pair (left-left, left-right, right-left or right-right), finger pair, then the exact transition. At each level the parent estimate is the prior mean:

```text
estimate = (n * sample mean + k * parent estimate) / (n + k)
```

For each parent's children, the within-child variance is pooled as the sum of squared deviations from each child mean divided by total weight minus the number of children. The between-child variance `tau²` uses a DerSimonian–Laird moment estimate, including its correction for unequal child sizes. The prior strength is `k = within variance / tau²`, clamped to 3 through 100; a non-positive `tau²` uses 100. `[analysis].shrinkage_k` (default 10) remains the fallback when the moment calculation is undefined. Error rates are shrunk in the same overall → hand pair → finger pair → transition order. Their Beta prior strength has a lower bound of 10, so a single error-free observation cannot collapse a non-zero parent rate to zero.

The 90% interval uses posterior variance `within variance / (n + k)` on the log scale, then transforms its bounds back to milliseconds. A key without a known geometry assignment skips the finger level, and also the hand level when its hand is unknown. Displays and CSV retain the raw `n` alongside the estimate and interval. High-cost rankings in the summary, transition tab and adaptive mode require `n >= [analysis].min_transition_n` (default 5); smaller samples remain visible as insufficient data instead of becoming practice targets.

Hand and finger groups use the standard touch-typing assignments in the selected keyboard geometry. They estimate likely technique; the application does not observe the finger that was actually used.

### Recent and all-time models

The recent view gives each session a weight of `0.5^(age in days / half-life)`. `[analysis].half_life_days` defaults to 14. The all-time view uses weight 1 for every observation. `transition_model` stores weighted sufficient statistics for both views. Adding a session decays the existing recent sums to the new timestamp and adds only the new log; a model-version mismatch discards this derived cache and rebuilds it from keystrokes.

Each transition also reports `relative_iki`, its estimated IKI divided by the player's weighted IKI median. This removes the player's overall speed level. The public player-model API resolves an unobserved transition through its standard finger pair, hand pair, then overall estimate, and reports which level supplied the value. A key without a known standard assignment skips the unavailable levels. Expected-prompt performance sums these resolved estimates, adds the player's median word-start delay and reaction time, and returns expected WPM. English uses the prompt keystrokes directly. Japanese uses the player's most frequently observed accepted spelling for each mora, falling back to the normal romaji guide when no preference has been observed.

### Recorded assistance settings

Each new session records `pacer_enabled`, `preview_items`, `blind`, `ghost` (enabled and opponent), `keyboard_show`, `keyboard_style`, `prompt_style` and `accuracy_gate` in `mode_params`. These fields are optional when reading, so records written before schema v6 do not acquire guessed values.

### Error diagnosis

In `stop` mode, the first wrong press at one position is classified in this order: the next expected key (`anticipation`), the previous expected key (`repeated-previous`), a physically adjacent key (`adjacent-key`), another key assigned to the same standard touch-typing finger (`same-finger`), the mirrored finger on the opposite hand and same row (`opposite-hand`), or another substitution (`other-substitution`). A second or later wrong press at that position is `consecutive`. In the `errors` CSV, ordinary category proportions use the number of first errors as their denominator and therefore sum to one. The `consecutive` proportion is separate: consecutive errors per first error. Physical adjacency uses the logged input key code and geometry, while the expected character, hand and finger use the practice layout. In `advance` mode, optimal-string-alignment Damerau–Levenshtein alignment produces substitutions, insertions, omissions and adjacent transpositions; substitutions use the same physical categories. The confusion matrix counts expected/typed pairs, with an empty side for insertions and omissions.

Post-error slowdown is the median of `actual IKI - recent transition estimate` for the next one through three correct presses, minus the same median for correct presses at least ten positions from an error. Pre-error rhythm disruption is the median population SD of the four IKIs ending at an error divided by the corresponding median at positions at least ten presses from an error. Correction cost reports KSPC, summed time from the first wrong press until the position advances correctly as a share of session time, and the residual edit rate for `advance`. The aggressive-region ratio is the error rate after an IKI below the player's p25 divided by the error rate after other IKIs. A result whose required denominator or sample is absent is returned as no data, not zero.

### Typing style

Word-start delay is the median IKI from the last key of one word to the first key of the next, divided by the median within-word IKI. A space inside one item always supplies a boundary because the following word was already visible. An item boundary supplies a sample only when the recorded `preview_items` is greater than zero, meaning that the next item was visible before the switch; `preview_items = 1` displays the next one item. A boundary from an old record without this setting is excluded rather than guessed. If no eligible boundary remains, word-start delay is unmeasured. The first key's reaction time is excluded. For a word typed at least three times without errors, chunking is `median observed within-word time / median sum of recent transition estimates`; values below one indicate that the word is typed faster as a familiar unit than its transitions predict.

Rollover is measured only in sessions with release events. Its rate is `pairs where next press < previous release / comparable pairs`, with median and p90 overlap duration. Sessions without usable release timestamps produce no rollover measurement. Rhythm decomposition reports separate population CVs for within-word and between-word IKIs.

### Japanese romaji diagnosis

Accepted spelling reconstruction splits a Japanese item into mora units, including contracted sounds, っ and ん. Mora duration runs from the preceding mora's last press to the current mora's last press. An item's first mora has no preceding endpoint and contributes no duration sample. An interval above `[analysis].pause_ms` is also excluded. The occurrence count and press-level error rate still include those moras; if none of a mora's occurrences has a duration sample, `median_ms` is empty in CSV rather than zero.

Spelling analysis emits rows only for moras the player has typed. It compares the common alternatives for し, ち, つ, ふ, じ, the しゃ/ちゃ/じゃ families, k/c spellings of か・く・こ, context-valid ん, and repeated-consonant/`ltu`/`xtu` spellings of っ. Standalone small kana such as ぁ are excluded. A composed spelling that types a small kana separately, such as `kilya`, is included only after the player has actually used that spelling. Every retained variant must also be accepted by the romaji automaton in that reading context. For every occurrence in the bundled Japanese-word corpus, the calculation includes the preceding spelling's last character and the following spelling's first character, then computes `sum(estimated IKI + estimated error rate * one-error cost)`. The error cost comes from post-error slowdown and correction time when measurable, otherwise 300 ms. A candidate is `suggested` only when it saves at least 15 ms and 8% per occurrence and every involved transition has effective count at least `[analysis].min_transition_n`; the same improvement with insufficient evidence is `reference`, and a smaller improvement is `not-faster`. `[analysis].spelling_suggestions = false` suppresses these rows. English/Japanese comparison includes transitions meeting that evidence threshold in both languages and reports the median and largest deviations of `Japanese estimated IKI / English estimated IKI`.

### Progress and state

Difficulty-adjusted form is backtested chronologically. Immediately before a session, the recent transition model contains only earlier sessions. The session's expected WPM is M4 applied to all of its prompts; `form = observed net WPM / expected WPM`. Corrected WPM is `form * expected WPM of benchmark A parallel 01` for English, or benchmark C parallel 01 for Japanese. Unobserved and sparse transitions use the hierarchy described above. A form estimate requires at least 300 earlier character-key logs and coverage of at least 0.5. Coverage is the occurrence-weighted fraction of prompt transitions whose exact transition has effective count at least `[analysis].min_transition_n`; hierarchical fallback does not make an occurrence covered. The benchmark reference has no coverage cutoff and is always completed from hierarchical estimates. Imported `metrics_version = 0` sessions are excluded. The implementation advances one sufficient-statistics cache in time order, estimates a session, then adds that session, so later records cannot leak into earlier estimates and the keystroke model is not rebuilt for every session.

Daily speed is the median benchmark A/C net WPM, falling back to corrected WPM on days without that benchmark. Accuracy is tracked separately. Four-week slopes use the 28 days ending on the analysis date and are reported only when that language has records in at least two calendar weeks in the window. They are reported per week; older records do not move the window. With at least six benchmark results, Pearson correlation of consecutive results is returned as a retest-reliability indication. For the last 28 days, a least-squares line describes the personal speed/accuracy operating point. The latest point is `too-cautious` at accuracy at least 98.5% and corrected WPM below the personal median, `too-aggressive` below 94% accuracy, and `balanced` otherwise.

Learning curves use cumulative practice minutes. Both `y = a - b exp(-c t)` and `y = a - b (t+1)^(-c)` are fitted by Levenberg–Marquardt nonlinear least squares, and AICc selects the model. At least six points are required. The 28-day forecast assumes the recent mean daily practice time continues; its band is the forecast plus or minus 1.96 residual SD. Bayesian online normal change-point detection uses a constant hazard of 1/30. Posterior probability above 0.5 creates a candidate; adjacent candidates are consolidated. A rise of at least 3 WPM is `sudden-gain`, a following segment of at least 10 days with slope below 0.2 WPM/day in magnitude is `plateau-start`, and the remaining decrease is `drop`. Transition regression compares the fastest eligible 14-day estimate with the recent model and reports ratios of at least 1.15.

Warm-up compares form after a break of at least three hours with other sessions, and compares median IKI in the first 30 seconds with later IKI. Sessions less than ten minutes apart form one practice block. Deterioration buckets are 0–5, 5–10, 10–20 and 20+ minutes; the first bucket whose error rate is at least 1.3 times the first is marked. Daily practice reports minutes, within-day form gain and next-day retention (`next first / previous last`). A segmented line reports a plateau only with at least 14 days. Time-of-day medians use morning 05–11, afternoon 11–17, evening 17–23 and night 23–05 and are explicitly exploratory. Pacer, ghost, keyboard display, preview and blind assistance compare form with and without the aid; unrecorded settings are excluded. An aid is removable when both sides have at least five sessions and median form without it is at least 97% of median form with it.

### Prescription validation

Adaptive prescriptions use within-player matched controls. Controls share the transition kind and corpus-frequency quartile, and their baseline estimate is within 20% of the target. Up to three are stored with the target and excluded from focus selection while the prescription is active. Baseline and current estimates retain posterior log variance. Only real-word prompts count as evaluation evidence. With at least three prescription sessions and effective count 10 for every target/control estimate, the difference in differences is `effect = log(target current / target baseline) - mean(log(control current / control baseline))`. `effect <= log(0.95)` and `z <= -1.64` is effective; `|effect| < log(1.03)` is ineffective. Otherwise it remains pending, and closes as insufficient data after 14 days. The result includes target/control millisecond changes.

The `prescriptions` table is authoritative user history rather than a derived cache. Schema v7 adds `review_schedule(layout, from_key, to_key, next_due, interval_days, last_reviewed_at)`. Review state is also authoritative; migration never clears prescriptions. Selected goals and adaptive-difficulty state are stored in `meta`.

## Absolute S-to-G grades

`grades-v1` assigns an absolute S/A/B/C/D/E/F/G grade to seven components. The bundled and startup-validated source is `data/grades/grades-v1.toml`. The values are provisional standards rather than population norms.

| Component | Metric | S | A | B | C | D | E | F | score 0 | score 8 |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Speed (English) | net WPM | 110 | 90 | 75 | 60 | 48 | 36 | 25 | 10 | 130 |
| Speed (Japanese) | net WPM by romaji character count | 95 | 80 | 66 | 53 | 42 | 32 | 22 | 8 | 115 |
| Accuracy | percent correct | 99.0 | 98.0 | 97.0 | 96.0 | 94.5 | 93.0 | 90.0 | 80.0 | 99.8 |
| Rhythm | within-word IKI CV, lower is better | 0.30 | 0.36 | 0.42 | 0.48 | 0.55 | 0.63 | 0.72 | 0.90 | 0.22 |
| Transitions | corpus-weighted loss %, lower is better | 5 | 8 | 12 | 16 | 21 | 27 | 35 | 50 | 2 |
| Preview | word-start IKI / within-word IKI, lower is better | 1.10 | 1.25 | 1.40 | 1.60 | 1.85 | 2.15 | 2.50 | 3.0 | 1.0 |
| Recovery | post-error excess ms, lower is better | 20 | 35 | 50 | 70 | 95 | 125 | 160 | 220 | 5 |
| Overlap | rollover % | 50 | 40 | 32 | 25 | 18 | 12 | 6 | 0 | 65 |

Each component has a continuous score from 0 through 8. Values are linearly interpolated between the score-0 endpoint, the F through S boundaries, and the score-8 endpoint. Scores below or above the endpoints are clamped. G occupies 0–1, F 1–2, through S 7–8; an exact boundary enters the new grade. The next-grade gap is the next boundary minus the current value, so lower-is-better gaps are negative.

Transition loss is `sum(frequency * max(0, estimated IKI - personal median IKI)) / sum(frequency * personal median IKI)`. English frequencies come from bundled English words. Japanese words are expanded with the player's most-used accepted romaji spellings before counting bigrams. Rhythm, preview and overlap are session medians. Transition loss and recovery combine the selected sessions because they need more keystrokes. Missing release timestamps leave overlap unmeasured rather than zero.

The overall score is the weighted harmonic mean after adding one to every component score, then subtracting one:

```text
overall = sum(weight) / sum(weight / (component score + 1)) - 1
```

Speed and accuracy have weight 2; every other component has weight 1. Overall is absent when speed or accuracy is absent. Other missing components are listed and omitted from the formula. The suggested component is the one whose one-point score increase raises overall most.

Official English grades use the latest three benchmark A sessions; Japanese uses benchmark C. Speed, accuracy, rhythm, preview and overlap are medians of per-session values, while transitions and recovery pool their keystrokes. One or two benchmark sessions are marked interim. With no benchmark in that language, speed preferentially uses the latest three eligible corrected-WPM practice values. Until corrected WPM is available, it falls back to the latest three net-WPM values in that language and is marked `provisional-uncorrected` (difficulty uncorrected). Accuracy uses the latest three practice sessions, and the other components use practice keystrokes from the latest seven days. Both provisional sources never create a promotion or demotion. Benchmark-day history is recomputed from records available on that day and carries the threshold version.

`stats --grades` prints the localized English and Japanese grades. It cannot be combined with `--summary` or `--csv`.

## Eight skill axes

The benchmark tab reports sustained speed, accuracy, rhythm, transitions, reach, reaction, correction and Japanese. Speed and Japanese use net WPM; rhythm uses IKI CV; transitions compare slow bigrams with overall IKI; reach compares remote rows with the home row; reaction and correction use their median times. Each latest value is placed in the user's own historical distribution for that benchmark part. With fewer than three results, only the raw value is shown. An axis without usable samples is shown as no data rather than zero.

## Analysis screen

Press `A` on the title screen. `Tab` or `←/→` changes tabs. `P` cycles all time, 30 days and 7 days; `M` cycles the mode filter. Every tab, including Coach, uses the records left by these filters. The expensive player model, diagnosis, progress, habits and grade calculations run once when the screen opens and again only when either filter changes; redraws use the cached report.

- Coach: a scrollable four-part report. With terminal images, each language starts with a G-to-S ladder: the darker, outlined step is the current grade and the triangle locates the continuous 0–8 score. The radar chart below uses the same scale on every component axis; the outer rings are higher grades, the axis suffix is that component's grade, and a labelled axis without a vertex has no data. The radar is omitted in short areas, and the existing prose continues below all graphics. “Where you are now” combines the S-to-G grade, component values and next-grade gaps, promotion/demotion, four-week speed/accuracy slopes, difficulty-adjusted form and the selected goal. “Last prescription” reports the target and matched-control changes in milliseconds with the R3 verdict. “Best opportunities” starts with the component that most raises overall and adds up to two concrete diagnoses, evidence counts, assumptions and a practice mode. Error advice is specific to anticipation, repeated-previous, adjacent-key, same-finger and opposite-hand errors; a total below 20 first errors is marked as a reference. Transition evidence is an effective count and is therefore shown as a rounded `n≈`, while observed counts use `n=`. Only `suggested` spelling rows appear. “Habits” includes only supported H1–H3/H5 findings; H4 is marked exploratory. Data coverage and benchmark reminders remain at the end. `T` opens goal candidates; `↑`/`↓` and `Enter` save one.
- Trend: daily medians for net WPM, accuracy and median IKI, separated by mode or benchmark part. Charts show endpoint dates and value bounds after two valid days have accumulated; before that they show the current value. With terminal images, horizontal distance represents calendar time, including days without records. The translucent fan on the speed chart narrows to the latest language-level daily speed and widens to the fitted 28-day forecast interval; it is omitted when no learning curve is available.
- Keyboard: error rate, median IKI, relative speed (personal median = 1.0), and confusion. `Enter` cycles the four views. In confusion view, `←`/`→` selects the expected key and the keyboard highlights the keys pressed instead. The legend below the keyboard maps the dark-to-bright red scale to each view's values; relative speed runs from fast at 0.0 to slow at 2.0+, with `(1.0 = personal median)` kept as a separate note. The uncoloured sample means no data.
- Transitions: the 20 slowest and 20 most error-prone shrunk estimates. With terminal images, the selected slow transition is drawn as an arrow between key centres; a repeated key is a loop and a character absent from the active layout has no arrow. The lower two columns use bar length for estimated IKI and estimated error rate. Their dark-to-bright colour uses error rate within that column, and the selected slow bar has an outline. Counts and values remain terminal text, with insufficient rows below the bars. Select a slow transition and press `Enter` to prepare two-key `ngram` practice.
- Fingers/Hands: finger accuracy/IKI and same-finger versus alternating-hand timing. With terminal images, the finger chart orders left pinky through left index, one shared thumb, then right index through right pinky; the thumb splits into two bars only when separate left/right thumb data is available. IKI bars rise and error-rate bars fall; their colours match the keyboard finger guide, and samples below 30 are translucent. The adjacent four bars compare left hand, right hand, different keys on one finger, and alternating hands on one millisecond scale. Missing fingers are marked `—`, and all original numeric rows continue below the figures.
- Bench: latest part results, comparisons and the eight axes. With terminal images, the radar radius is the personal-history percentile; the 50 and 100 labels sit between axes. An axis marked “under 3” has no percentile vertex. The part A–D sparklines keep attempt order from left to right, emphasize the latest point, and repeat its WPM and change from the previous result. The original WPM and raw/percentile rows remain below the graphics.

The four-week slope uses benchmark A when it is available, then another available benchmark part; without a benchmark it uses practice records and explicitly warns that practice WPM is not evidence of improvement. Slopes above `+0.5 WPM/week` are upward, below `-0.5 WPM/week` are downward, and values in between are flat. Recommendations follow fixed rules: low accuracy selects a 95% accuracy gate with the pacer off; a slow transition selects `adaptive` and names the pair; weak reach selects `unlock` or `ngram`; weak reaction increases look-ahead; weak rhythm selects `rhythm`; second-half endurance deterioration selects `endurance` plus a break; and weak Japanese selects `romaji`.

## CLI and CSV columns

`rs-typing-trainer stats` prints a summary table. CSV export accepts `--since YYYY-MM-DD` and `--mode MODE`:

```sh
rs-typing-trainer stats --csv sessions
rs-typing-trainer stats --csv keys --since 2026-10-01 --mode words
rs-typing-trainer stats --csv bigrams
rs-typing-trainer stats --csv fingers
rs-typing-trainer stats --csv errors
rs-typing-trainer stats --csv spelling
rs-typing-trainer stats --grades
rs-typing-trainer stats --coach
rs-typing-trainer stats --csv grades
rs-typing-trainer stats --csv grade-history
rs-typing-trainer stats --summary --since 2026-10-01 --mode words
```

`stats --coach` prints the same four-part personalized report as the Coach tab and follows `--lang`, `--since`, and `--mode`. `stats --summary` remains as the compatible rule-based legacy summary. Neither text mode can be combined with `--csv`.

| CSV | Columns |
|---|---|
| `sessions` | `id, started_at, finished_at, duration_ms, mode, bench_id, layout, net_wpm, raw_wpm, accuracy, iki_median_ms, iki_cv, metrics_version` |
| `keys` | `key, code, correct, total, accuracy, iki_n, iki_median_ms, iki_p90_ms` |
| `bigrams` | `from, to, class, n, median_ms, error_n, attempts, error_rate, estimated_iki_ms, estimated_error_rate, estimated_iki_lo_ms, estimated_iki_hi_ms, relative_iki` |
| `fingers` | `finger, correct, total, accuracy, n, iki_median_ms` |
| `errors` | `kind, n, proportion` |
| `confusion` | `expected, typed, n` |
| `kana` | `kana, n, median_ms, error_rate` |
| `spelling` | `kana, current, candidate, current_expected_ms, candidate_expected_ms, savings_ms, decision` |
| `words` | `word, n, observed_ms, predicted_ms, ratio` |
| `form` | `session_id, started_at, mode, net_wpm, expected_wpm, form, corrected_wpm, coverage` |
| `curve` | `target, model, a, b, c, aicc, n, forecast_days, forecast, band_lo, band_hi` |
| `changepoints` | `target, date, probability, kind` |
| `regressions` | `from, to, best_ms, recent_ms, ratio` |
| `habits` | `kind, bucket, n, value` |
| `prescriptions` | `id, created_at, status, targets, controls, sessions, target_delta_ms, control_delta_ms, effect, z, verdict` |
| `reviews` | `from, to, next_due, interval_days` |
| `grades` | `language, component, value, unit, grade, score, next_grade, next_threshold, gap, source, n_sessions, version` |
| `grade_history` | `date, language, component, grade, score, version` |

## Measurement cautions

Repeated text measures memory as well as typing, which is why benchmark forms rotate. Do not trust rare bigrams without their count and shrunk estimate. Judge progress with benchmarks rather than adaptive practice scores. Hold and rollover values require terminal release events and should not be mixed with sessions that lack them. Japanese parts measure direct romaji input, not IME conversion and candidate selection.
