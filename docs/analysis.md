**English** | [日本語](analysis.ja.md)

# Benchmarks and analysis

## `bench-v2` and `bench-v1`

Press `B` on the title screen to run the current `bench-v2` benchmark. Parts A–C run in sequence; part D is optional. Existing `bench-v1` records and all eight of its prompt fingerprints remain available unchanged.

| Part | Material | Time |
|---|---|---:|
| A | English words and phrases, staged from short to long | 60 s |
| B | the same 3–6 letter Markov pseudowords as the corresponding `bench-v1` form | 60 s |
| C | Japanese words and phrases typed in romaji, staged by reading length | 60 s |
| D | the same digits and symbols as the corresponding `bench-v1` form | 45 s |

Part A contains 15 English words of 3–5 characters, 15 words of 6–8 characters, and 12 phrases in each of the 10–15, 16–22 and 23–29 character stages. The words are taken in frequency order from the same bundled source as `bench-v1` A. Phrase candidates must be `core`, and every ASCII-letter word in a phrase must occur among the top 3,000 bundled English words. Part C similarly contains 15 core Japanese words whose readings have 2–4 characters, 15 whose readings have 5–8 characters, and 12 phrases in each of the 6–10, 11–15 and 16–20 reading-length stages. Phrase selection uses the input-order-independent rule named `sha256-v1`: English candidates are ordered by the SHA-256 digest of their UTF-8 text; Japanese candidates by the SHA-256 digest of their UTF-8 reading, then by text on a digest tie. Each form has the same multiset within every stage and uses a fixed form-and-stage seed to change only the order inside that stage. The stages themselves are never interleaved.

Every series has eight deterministic parallel forms (`bench-v2/A/01` through `08`, and the retained `bench-v1` forms). A new run counts its next form inside `bench-v2`, independently of `bench-v1`, and wraps from 08 to 01. The original `bench-v1` A, C and D use the same item multiset in every form and shuffle it with fixed seeds. For its B, a first-order Markov model is built from the top 5,000 bundled English words. Each form independently generates and greedily selects 240 pseudowords while balancing character and transition-class shares and maximizing distinct bigrams. Exact matches to bundled English words, periodic words and words that repeat a bigram are excluded. The eight B forms use different words rather than shuffled copies. `bench-v2` B and D copy the prompts of the same-numbered `bench-v1` form and change only the series in `bench_id`.

Benchmark settings are fixed: stop-on-error, no item timeout and one preview item. User session settings do not alter them. Benchmark material is loaded from the complete bundled data before daily decks, exposure counts, user salts, weekday themes or reported-reading exclusions are applied. Japanese benchmark items must have `tier = core`.

Each part is stored as a separate session with `mode = bench` and its full form ID in `bench_id`. Because B and D have unchanged prompts, their `bench-v1` and `bench-v2` records form one comparison series for previous-result and personal-best deltas, recent-three comparisons, eight-axis values, trends and grade inputs. A and C remain version-specific: once a part has any v2 result, v2 alone is preferred. A/C trend charts may show both versions, but draw a labelled v2 break and never join a line across it. Each part result names the part and form, shows the comparisons, and names the next part. The last part leads to a run summary. Pressing `Esc` twice within two seconds ends the run while retaining completed-part records.

An interrupted run is reconstructed only from the database. Among current-series benchmark records saved on the current local date, the application takes the form number from the latest record, collects parts saved today with that series and form, and offers the first missing required part in A → B → C order. Part D is optional, so A through C count as complete. A resumed run keeps the series and form number fixed, and its summary combines the earlier completed parts with the newly completed parts. History likewise groups parts only when both series and form match. Ignoring the resume entry and starting a new run continues to use the normal next-form rule.

## `bench-v3` when `[bench] series = "v3"`

With `[bench] series = "v3"`, `B` opens the same benchmark selection screen and then runs Warm-up (`W`, 15 s, not scored or stored) → English words (`EW`, 60 s) → English sentences (`ES`, 60 s) → Japanese words and phrases (`JW`, 60 s) → Japanese sentences (`JS`, 60 s) → Pseudo-words (`PW`, 30 s) → the run summary. The warm-up is followed by a short notice and the `EW` countdown instead of a part-result screen. A resumed run repeats `W`, then continues from the first missing scored part.

The series has eight forms, used in order and wrapping after 08. Scored parts are stored as `bench-v3/<part>/<form>`. v3 fixes stop-on-error, no item timeout, one preview item, no pacer, ghost, blind mode, keyboard display, daily deck, reading correction or key filtering; only the user's keyboard layout remains. Until v3 analysis is added, these records appear in history but are excluded from the v2 benchmark, grade, eight-axis, daily-speed and growth analyses. The v3 summary therefore shows per-part net WPM, accuracy and previous-run deltas, followed by a note that v3 analysis is being prepared.

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

Word-start delay is the median IKI from the space after one word to the first key of the next, divided by the median within-word IKI. Both are single keystroke intervals, so even typing gives a ratio of about one. An interval that spans a mistype or Backspace is a correction, not an inter-key interval, and is not sampled. Only a word start after a space inside the same item supplies a sample. Item boundaries are excluded regardless of the recorded `preview_items` because their intervals include the item switch, when typing may be unavailable. If no eligible boundary remains, word-start delay is unmeasured. The first key's reaction time is excluded. For a word typed at least three times without errors, chunking is `median observed within-word time / median sum of recent transition estimates`; values below one indicate that the word is typed faster as a familiar unit than its transitions predict.

Rollover is measured only in sessions with release events. Its rate is `pairs where next press < previous release / comparable pairs`, with median and p90 overlap duration. Sessions without usable release timestamps produce no rollover measurement. Rhythm decomposition reports separate population CVs for within-word and between-word IKIs.

### Japanese romaji diagnosis

Accepted spelling reconstruction splits a Japanese item into mora units, including contracted sounds, っ and ん. Mora duration runs from the preceding mora's last press to the current mora's last press. An item's first mora has no preceding endpoint and contributes no duration sample. An interval above `[analysis].pause_ms` is also excluded. The occurrence count and press-level error rate still include those moras; if none of a mora's occurrences has a duration sample, `median_ms` is empty in CSV rather than zero.

Spelling analysis emits rows only for moras the player has typed. It compares the common alternatives for し, ち, つ, ふ, じ, the しゃ/ちゃ/じゃ families, k/c spellings of か・く・こ, context-valid ん, and repeated-consonant/`ltu`/`xtu` spellings of っ. Standalone small kana such as ぁ are excluded. A composed spelling that types a small kana separately, such as `kilya`, is included only after the player has actually used that spelling. Every retained variant must also be accepted by the romaji automaton in that reading context. For every occurrence in the bundled Japanese-word corpus, the calculation includes the preceding spelling's last character and the following spelling's first character, then computes `sum(estimated IKI + estimated error rate * one-error cost)`. The error cost comes from post-error slowdown and correction time when measurable, otherwise 300 ms. A candidate is `suggested` only when it saves at least 15 ms and 8% per occurrence and every involved transition has effective count at least `[analysis].min_transition_n`; the same improvement with insufficient evidence is `reference`, and a smaller improvement is `not-faster`. `[analysis].spelling_suggestions = false` suppresses these rows. English/Japanese comparison includes transitions meeting that evidence threshold in both languages and reports the median and largest deviations of `Japanese estimated IKI / English estimated IKI`.

### Progress and state

Difficulty-adjusted form is backtested chronologically. Immediately before a session, the recent transition model contains only earlier sessions. The session's expected WPM is M4 applied to all of its prompts; `form = observed net WPM / expected WPM`. Corrected WPM is `form * expected WPM of bench-v1 A form 01` for English, or `bench-v1` C form 01 for Japanese; introducing `bench-v2` does not change this reference. Unobserved and sparse transitions use the hierarchy described above. A form estimate requires at least 300 earlier character-key logs and coverage of at least 0.5. Coverage is the occurrence-weighted fraction of prompt transitions whose exact transition has effective count at least `[analysis].min_transition_n`; hierarchical fallback does not make an occurrence covered. The benchmark reference has no coverage cutoff and is always completed from hierarchical estimates. Imported `metrics_version = 0` sessions are excluded. The implementation advances one sufficient-statistics cache in time order, estimates a session, then adds that session, so later records cannot leak into earlier estimates and the keystroke model is not rebuilt for every session.

Daily speed uses the median net WPM of the preferred benchmark series for A/C: `bench-v2` when that part has at least one v2 result, otherwise `bench-v1`. Results from the other series are not mixed into that daily series, its four-week slope or its retest reliability. Corrected WPM is used only when no benchmark series exists for that language. Accuracy is tracked separately. Four-week slopes use the 28 days ending on the analysis date and are reported only when that language has records in at least two calendar weeks in the window. They are reported per week; older records do not move the window. With at least six benchmark results, Pearson correlation of consecutive results is returned as a retest-reliability indication. For the last 28 days, a least-squares line describes the personal speed/accuracy operating point. The latest point is `too-cautious` at accuracy at least 98.5% and corrected WPM below the personal median, `too-aggressive` below 94% accuracy, and `balanced` otherwise.

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

Official English grades use the latest three benchmark A sessions; Japanese uses benchmark C. If that part has any `bench-v2` result, only v2 is used; otherwise the retained `bench-v1` results are used. One or two results in the selected series are marked interim. Speed, accuracy, rhythm, preview and overlap are medians of per-session values, while transitions and recovery pool their keystrokes. With no benchmark in that language, speed preferentially uses the latest three eligible corrected-WPM practice values. Until corrected WPM is available, it falls back to the latest three net-WPM values in that language and is marked `provisional-uncorrected` (difficulty uncorrected). Accuracy uses the latest three practice sessions, and the other components use practice keystrokes from the latest seven days. Both provisional sources never create a promotion or demotion. Benchmark-day history is recomputed from records available on that day and carries the threshold version.

`stats --grades` prints the localized English and Japanese grades. It cannot be combined with `--summary` or `--csv`.

## Eight skill axes

The benchmark tab reports sustained speed, accuracy, rhythm, transitions, reach, reaction, correction and Japanese. Speed and Japanese use net WPM; rhythm uses IKI CV; transitions compare slow bigrams with overall IKI; reach compares remote rows with the home row; reaction and correction use their median times. Each latest value is placed in the user's own historical distribution for that benchmark part: A/C use the preferred version, while B/D combine v1 and v2. With fewer than three results, only the raw value is shown. An axis without usable samples is shown as no data rather than zero.

## Analysis screen

Press `A` on the title screen. `Tab` or `←/→` changes tabs. The tabs are Today, Coach, Trend, Keyboard, Transitions, Fingers/Hands, Bench, Skills and Growth; the screen opens on Coach, and `←` from Coach reaches Today. The screen opens on the last 30 days (ending today); `P` steps to 6 months, 1 year, all time and 7 days, then back to 30 days. `M` cycles the mode filter. Every tab, including Coach, uses the records left by these filters, except that Today reads the database directly, Skills reads menu check steps, and Growth compares all-time records independently of `P` and `M`. The expensive reports are cached when the screen opens; changing `P` or `M` does not rebuild or alter Growth.

- Today: today's menu steps, individual sessions and totals against the average of the past 7 days (see [Today tab](#today-tab)).
- Coach: a scrollable four-part report. With terminal images, each language starts with a G-to-S ladder: the darker, outlined step is the current grade and the triangle locates the continuous 0–8 score. The radar chart below uses the same scale on every component axis; the outer rings are higher grades, the axis suffix is that component's grade, and a labelled axis without a vertex has no data. The radar is omitted in short areas, and the existing prose continues below all graphics. “Where you are now” combines the S-to-G grade, component values and next-grade gaps, promotion/demotion, four-week speed/accuracy slopes, difficulty-adjusted form and the selected goal. “Last prescription” reports the target and matched-control changes in milliseconds with the R3 verdict. “Best opportunities” starts with the component that most raises overall and adds up to two concrete diagnoses, evidence counts, assumptions and a practice mode. Error advice is specific to anticipation, repeated-previous, adjacent-key, same-finger and opposite-hand errors; a total below 20 first errors is marked as a reference. Transition evidence is an effective count and is therefore shown as a rounded `n≈`, while observed counts use `n=`. Only `suggested` spelling rows appear. “Habits” includes only supported H1–H3/H5 findings; H4 is marked exploratory. Data coverage and benchmark reminders remain at the end. `T` opens goal candidates; `↑`/`↓` and `Enter` save one.
- Trend: daily medians for net WPM, accuracy and median IKI, separated by mode. Benchmark runs of every version (`bench-v1`, `bench-v2`) and part (A–D) form one series, `bench`, whose daily point is the median over all of that day's parts (the forecast is not affected). Charts show endpoint dates and value bounds after two valid days have accumulated; before that they show the current value. Horizontal distance represents calendar time, including days without records, in both the image and the text charts, and the three charts share one axis. A gap of more than 14 days between two recorded days is cut out of the axis and squeezed to the width of 3 days: the cut is marked `≈` on the axis (below the image with terminal images), no line is drawn across it, and the first date after it is labelled when there is room. Axis dates read `MM/DD`; the start date carries the year (`2026/10/21`), and the end date and the dates after a cut carry it only when the year changes. Tick dates are labelled between the endpoints. The tick interval is the smallest of 1 day, 2 days, 1 week, 2 weeks and 1 month (then 3 months, 6 months and 1 year) at which the labels (5 columns plus one blank column on each side) do not overlap. Weekly ticks fall on Mondays and monthly ones on the first of the month. Ticks are placed only inside the uncut stretches of the axis, never in a squeezed gap, and a tick whose label would touch the start, end or after-cut date is left out. Each tick label is centred on the column of its day's points. With terminal images, a faint vertical grid line, as faint as the horizontal ones, is drawn at each labelled tick, and the `day` caption under the image is shown only when there is no tick. Axis labels are always real dates, and the forecast fan follows the same axis. The translucent fan on the speed chart narrows to the latest language-level daily speed and widens to the fitted 28-day forecast interval; it is omitted when no learning curve is available.
- Keyboard: error rate, median IKI, relative speed (personal median = 1.0), and confusion. `Enter` cycles the four views. In confusion view, `←`/`→` selects the expected key and the keyboard highlights the keys pressed instead. The legend below the keyboard maps the dark-to-bright red scale to each view's values; relative speed runs from fast at 0.0 to slow at 2.0+, with `(1.0 = personal median)` kept as a separate note. The uncoloured sample means no data.
- Transitions: the 20 slowest and 20 most error-prone shrunk estimates. With terminal images, the selected slow transition is drawn as an arrow between key centres; a repeated key is a loop and a character absent from the active layout has no arrow. The lower two columns use bar length for estimated IKI and estimated error rate. Their dark-to-bright colour uses error rate within that column, and the selected slow bar has an outline. Counts and values remain terminal text, with insufficient rows below the bars. Select a slow transition and press `Enter` to prepare two-key `ngram` practice.
- Fingers/Hands: finger accuracy/IKI and same-finger versus alternating-hand timing. With terminal images, the finger chart orders left pinky through left index, one shared thumb, then right index through right pinky; the thumb splits into two bars only when separate left/right thumb data is available. IKI bars rise and error-rate bars fall; their colours match the keyboard finger guide, and samples below 30 are translucent. The adjacent four bars compare left hand, right hand, different keys on one finger, and alternating hands on one millisecond scale. Missing fingers are marked `—`, and all original numeric rows continue below the figures.
- Bench: latest part results, comparisons and the eight axes. With terminal images, the radar radius is the personal-history percentile; the 50 and 100 labels sit between axes. An axis marked “under 3” has no percentile vertex. The part A–D sparklines keep attempt order from left to right, emphasize the latest point, and repeat its WPM and change from the previous result. B/D place v1 and v2 in one sequence; A/C use only the preferred version here. The original WPM and raw/percentile rows remain below the graphics. `S` switches to the [benchmark Summary](#benchmark-summary) and back; the footer adds `[S] Summary`.
- Skills: the seven components measured by the practice menu's check steps, one small chart per component, stacked vertically (see [Skill trends](#skill-trends)).
- Growth: an all-time comparison with the beginning, one and three months ago, benchmark milestones and accumulated practice (see [Growth tab](#growth-tab)).

The four-week slope uses benchmark A when it is available, then another available benchmark part. A/C select v2 when present and otherwise v1; B/D combine both versions. Without a benchmark it uses practice records and explicitly warns that practice WPM is not evidence of improvement. Slopes above `+0.5 WPM/week` are upward, below `-0.5 WPM/week` are downward, and values in between are flat. Recommendations follow fixed rules: low accuracy selects a 95% accuracy gate with the pacer off; a slow transition selects `adaptive` and names the pair; weak reach selects `unlock` or `ngram`; weak reaction increases look-ahead; weak rhythm selects `rhythm`; second-half endurance deterioration selects `endurance` plus a break; and weak Japanese selects `romaji`.

The text version of the Trend tab at 80×24 with all time selected (sample data: three days at the end of April and four days in November):

```text
┌ ANALYSIS ────────────────────────────────────────────────────────────────────┐
│   Today  Coach  Trend  Keyboard  Transitions  Fingers/Hands  Bench  Skills   │
│                                                                              │
│net WPM / daily median                                                        │
│47.0     │                           ⢀⣀⣀⣀⣀⠤⠤⠤⠤⠤⠤⢄⣀⣀⣀⣀⣀⣀⣀⡀      ⣀⣀⣀⣀⠤⠤⠤⠤⠔⠒⠒⠒⠊⠉⠉│
│         │                    ⠐⠒⠒⠉⠉⠉⠉⠁                  ⠈⠉⠉⠉⠉⠉⠉               │
│30.0     │⣀⣀⠤⠤⠤⠤⠤⠤⠤⠤⠄                                                      day│
│         └───────────────≈────────────────────────────────────────────────────│
│2026/04/27                    11/01                    11/09             11/15│
│accuracy % / daily median                                                     │
│100.0    │                                                                    │
│         │⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠄         ⠠⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤│
│100.0    │                                                                 day│
│         └───────────────≈────────────────────────────────────────────────────│
│2026/04/27                    11/01                    11/09             11/15│
│IKI ms / daily median                                                         │
│100.0    │                                                                    │
│         │⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠄         ⠠⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤│
│100.0    │                                                                 day│
│         └───────────────≈────────────────────────────────────────────────────│
│2026/04/27                    11/01                    11/09             11/15│
│          [Tab/←→] tab   [P] period: all   [M] mode: all   [ESC] back         │
│                                                                              │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Today tab

The first tab sums up today (the local date). It reads the database directly, so the period (`P`) and mode (`M`) filters do not apply: with the 7-day period selected, the 7-day average still counts the day 7 days ago that the period leaves out. The footer says so instead of showing the period and mode. `↑`/`↓` (`k`/`j`) scroll the text version. `S` switches to the [Summary](#summary) and back.

- Today's training: for each menu run of today with at least one played step, newest first (for example after changing the practice languages), one row per planned step with the same figures as the upper section of the [menu summary](training.md#practice-menu): step, prescription and level, this time's value, the difference from the previous run of the same prescription at the same level, and the graduation condition (`●●○ 2/3`, `●●● → Lv 2`). Steps not played yet are marked `not yet`. When no run of today has a played step, the latest run is shown as `not played`; without any run the tab says that there is no menu for today yet.
- Individual practice: today's sessions that are not menu steps, with start time, mode, language, length, net WPM and accuracy. Benchmark parts are included and marked `[bench A]`.
- Today's totals: practice time, sessions and keystrokes, and per language the median net WPM (with `▁▂▃▄▅▆▇` bars of the day's sessions in time order) and the median accuracy. Each value is compared with the average of the past 7 days: the 7 calendar days before today (today − 7 to yesterday), averaged over the days with records — days without practice do not count as zero, and today is never included. Net WPM and accuracy take each day's median (as above) and average those per day; time, sessions and keystrokes are averaged per day with records; each language is averaged over the days it was practised. The difference reads `vs 7-day avg +5.0` and uses the result screen's colours and arrows (green `▲` higher, red `▼` lower, `±0` unchanged), and `—` when there is nothing to compare with. Without any record in those 7 days the heading says `no records in the past 7 days` and no difference is shown. The comparison with the previous run of the same prescription in Today's training is unchanged.
- Without any session today the tab shows `No practice recorded today yet`.

With terminal images and at least 120×37 cells (120×29 for a compact version with lower cards), the three sections are figures: the steps with a done/not-yet badge, a bar for the step's minutes (filled when played), the value and difference, the graduation condition as circles, and level-ups highlighted with an up-arrow; a timeline of today's sessions (horizontal = time of day, vertical = net WPM, colour = language, filled circle = menu step, ring = individual, diamond = benchmark, marker size and the label above it = accuracy), with the list of individual sessions as text beside it; and big-number cards for practice time, sessions, keystrokes, and net WPM and accuracy per language, each with its difference from the 7-day average. Without terminal images, without a known cell size, or in a smaller window, the tab falls back to the text version in the result screen's order.

The text version at 80×24 (sample data: two of five menu steps played, the accuracy gate moving up to level 2, a Japanese romaji session and benchmark part A; the past 7 days have English records on 11/12, 11/15 and 11/18):

```text
┌ ANALYSIS ────────────────────────────────────────────────────────────────────┐
│   Today  Coach  Trend  Keyboard  Transitions  Fingers/Hands  Bench  Skills   │
│                                                                              │
│  ── Today's training 2026-11-19 ───────────────────────────────────────────  │
│  Menu (English, 15 min) — 2 of 5 steps                                       │
│  Step           Prescription · level        This time Prev     Graduation    │
│  1 Warm-up   EN words                       62 WPM    ▲+3                    │
│  2 Primary A EN Accuracy gate ①             met       —        ●●● → Lv 2    │
│  3 Primary B EN Widen preview ①             not yet                          │
│  4 Check     EN sentence                    not yet                          │
│  5 Finish    EN burst                       not yet                          │
│  ── Individual practice (2 sessions) ──────────────────────────────────────  │
│  Time  Mode         Lang     Length net WPM  Accuracy                        │
│  20:30 romaji       Japanese 1:00      48.5   93.0%                          │
│  21:15 [bench A]    English  1:00      64.0   98.0%                          │
│  ── Today's totals — vs the 7-day average (11/12–11/18, 3 days) ───────────  │
│  Time 4.0 min ▲+2.7   Sessions 4 ▲+2.7   Keystrokes 1,170 ▲+870              │
│  English   net WPM 62.0 ▲+6.7   ▄▁▇       accuracy 97.0% ▲+1.0pt             │
│  Japanese  net WPM 48.5 —       ▄         accuracy 93.0% —                   │
│                                                                              │
│                                                                              │
│            [Tab/←→] tab   [↑/↓] scroll   [S] Summary   [Esc] back            │
│         Today only: the period [P] and mode [M] filters do not apply         │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Summary

`S` on the Today tab switches between the detailed view and **Summary**, a screen for screenshots that shows the main results of the day in large figures. The state lasts while the analysis screen is open and is not saved. The tab bar is hidden and the footer is a single small line (`Summary   [S] details   [Esc] back`).

- Heading band: `Today's practice 2026-11-19` and the menu (`15-min menu · 2 of 5 steps`), or `Individual practice` without a menu.
- Main value: today's net WPM (median) in very large figures with a small `WPM`, its difference from the 7-day average and the average itself (`vs 7-day avg ▲+6.7 (avg 55.3)`). With both languages practised, the two values stand side by side under `English` and `Japanese`.
- Supporting figures: accuracy (per language when there are two), practice time and keystrokes, each with its difference from the 7-day average in the result screen's colours and arrows.
- Bottom band: up to two events of the day — level-ups first (`Accuracy gate: up to level 2`), then graduations, then swaps. Without any, the band shows today's menu as a chain of its steps with the progress band's short names, `✓Warm-up → ✓Accuracy → Preview → Check → Finish`: done steps carry `✓` and are bright green, the others are dimmed. When the chain does not fit, the spaces around the arrows go first and last it becomes `Step 2/5` (done of total), as the progress band shrinks. A day with individual practice only shows `Practiced today (4 sessions)`; without a record, `No practice recorded today yet`.

With terminal images and at least 120×29 cells, the Summary is one image in the theme colours: an accent heading band, a frame with cut corners, the main value about a fifth of the screen high (it scales with the window), and a dark band for the events. In the image the arrows and checks of the step chain are drawn as shapes (the image font has neither). Otherwise it is text: the main value is drawn with the bitmap big text used for prompts (several rows even at 80×24), and with normal characters when even that does not fit.

The text version at 80×24 (sample data as above, without the Japanese session):

```text
┌ ANALYSIS ────────────────────────────────────────────────────────────────────┐
│ Today's practice 2026-11-19                       15-min menu · 2 of 5 steps │
│╔════════════════════════════════════════════════════════════════════════════╗│
│║                                                                            ║│
│║                                   English                                  ║│
│║                               ▄▖  ▗▖      ▗▖                               ║│
│║                              ▐ ▝ ▗▘▝▖     ▌▐                               ║│
│║                              ▌   ▝  ▌    ▐  ▌                              ║│
│║                              ▙▀▚   ▞     ▐  ▌                              ║│
│║                              ▌  ▌ ▞      ▐  ▌                              ║│
│║                              ▌  ▌▗▘      ▝▖▗▘                              ║│
│║                              ▝▄▞ ▐▄▄▖▐▌   ▚▞                               ║│
│║                                                                            ║│
│║                     WPM  vs 7-day avg ▲+6.7 (avg 55.3)                     ║│
│║                                                                            ║│
│║                                                                            ║│
│║   Accuracy 97.0% vs 7-day avg ▲+1.0pt   Time 3.0 min vs 7-day avg ▲+1.7    ║│
│║                     Keystrokes 920 vs 7-day avg ▲+620                      ║│
│║                                                                            ║│
│║                                                                            ║│
│║                       ▲ Accuracy gate: up to level 2                       ║│
│╚════════════════════════════════════════════════════════════════════════════╝│
│                                            Summary   [S] details   [Esc] back│
└──────────────────────────────────────────────────────────────────────────────┘
```

### Benchmark Summary

`S` on the Bench tab switches to a Summary of the latest benchmark run in the same layout as the [Today Summary](#summary): no tab bar, the same footer, a heading band, a decorated frame, large figures and a dark bottom band. Moving to another tab, or reopening the analysis screen, returns to the tab. The run is the latest benchmark record's day, series and form, grouped as history and the result screen group parts; it uses the records left by the period and mode filters, like the tab. It shows only values the Bench tab and the benchmark result screen already show — there is no combined score.

- Heading band: `Benchmark 2026-11-19` (the day the run was played) and `bench-v2 / form 03 / Part A, B, C, D` (series, form and the parts in the run). Benchmark parts have their own languages (A English, C Japanese), so no language is shown separately.
- Large figures: each part's net WPM side by side, under the part's name (`Part A` when the full names do not fit). Under each: its accuracy, the difference from the previous result and from the median of the last three results, computed as on the result screen — B/D compare across `bench-v1` and `bench-v2`, A/C only within the run's version. A first result reads `first result`. When an A/C part has earlier results only in the other version, the line reads `Different version: no comparison` instead.
- The eight skill axes: the percentile of each axis as on the tab; `*` marks a raw value (fewer than three results) and `—` no data.
- Bottom band: the result screen's personal-best verdicts — `New best! Part A, C` (net WPM, highlighted) and `New best accuracy! Part A, B`. Without any, `Previous benchmark: 2026-11-15`, or `First benchmark`. Without benchmark records the frame shows `No benchmark recorded yet` in the middle.

With terminal images and at least 120×29 cells it is one image: the four values share a row and are about an eighth of the screen high, and the eight axes stand on one row. Otherwise it is text with the bitmap big text (several rows at 80×24), or normal characters when even that does not fit.

The text version at 80×24 (sample data: three `bench-v2` runs on 11/12, 11/15 and 11/19; the last adds part D):

```text
┌ ANALYSIS ────────────────────────────────────────────────────────────────────┐
│ Benchmark 2026-11-19                    bench-v2 / form 03 / Part A, B, C, D │
│╔════════════════════════════════════════════════════════════════════════════╗│
│║      Part A             Part B             Part C             Part D       ║│
│║ ▗▄▄  ▄▖      ▗▖     ▄▖  ▄▖     ▗▄▄      ▖ ▄▄▄▖     ▗▖     ▄▖  ▗▖      ▗▖   ║│
│║ ▐   ▞ ▝▖     ▌▐    ▞ ▝▖▞ ▝▖    ▐       ▐▌   ▗▘     ▌▐    ▞ ▝▖ ▌▐      ▌▐   ║│
│║ ▞▄▖ ▌  ▌    ▐  ▌   ▘  ▌▌  ▌    ▞▄▖     ▌▌   ▞     ▐  ▌   ▘  ▌▐  ▌    ▐  ▌  ║│
│║ ▛ ▝▖▝▄▞     ▐  ▌    ▄▞ ▚ ▗▌    ▛ ▝▖   ▐ ▌   ▌     ▐  ▌    ▄▞ ▐  ▌    ▐  ▌  ║│
│║    ▌▞ ▝▖    ▐  ▌     ▝▖ ▀▘▌       ▌   ▙▄▙▖ ▗▘     ▐  ▌     ▝▖▐  ▌    ▐  ▌  ║│
│║ ▌  ▌▌  ▌    ▝▖▗▘   ▌  ▌▖  ▌    ▌  ▌     ▌  ▐      ▝▖▗▘   ▌  ▌▝▖▗▘    ▝▖▗▘  ║│
│║ ▝▄▞ ▝▄▞ ▐▌   ▚▞    ▝▄▞ ▝▄▞ ▐▌  ▝▄▞      ▌  ▐  ▐▌   ▚▞    ▝▄▞  ▚▞ ▐▌   ▚▞   ║│
│║                                            ▝                               ║│
│║  WPM  acc 97.0%     WPM  acc 97.0%     WPM  acc 92.0%     WPM  acc 90.0%   ║│
│║ vs previous ▲+6.0  vs previous ▼-1.5  vs previous ▲+3.0  prev first result ║│
│║  recent-3 ▲+6.0     recent-3 ▼-1.5     recent-3 ▲+2.0      first result    ║│
│║                          8 skill axes (percentile)                         ║│
│║   Sustained speed 100.0   Accuracy 100.0   Rhythm 100.0   Transitions —    ║│
│║          Reach —   Reaction 100.0   Correction —   Japanese 100.0          ║│
│║                                                                            ║│
│║                           ▲ New best! Part A, C                            ║│
│║                        New best accuracy! Part A, B                        ║│
│╚════════════════════════════════════════════════════════════════════════════╝│
│                                            Summary   [S] details   [Esc] back│
└──────────────────────────────────────────────────────────────────────────────┘
```

## Skill trends

Benchmarks are taken once or twice a week, which is too sparse to follow day-to-day change. The [practice menu](training.md#practice-menu) therefore treats its check step — `sentence` with every aid removed, the same kind of prompt at the same length every day — as a small daily measurement. These records complement benchmarks; they never change the official grade.

### What is recorded

When a menu's check step finishes, the raw values of the seven graded components are computed and stored for that run. The series language is the language of the check session; English and Japanese are separate series.

- Speed, accuracy, rhythm, preview and overlap come from the check step alone (`source = check`). Speed is the difficulty-corrected WPM (form × expected WPM of the reference benchmark) when it can be computed; otherwise it is the raw net WPM and is marked uncorrected (`corrected = false`).
- Transitions and recovery need more keystrokes, so they pool the check step with the same run's warm-up and finish in the same language (`source = check+steps`; prescription steps are excluded because they run with aids). A day with fewer observations than `[analysis] min_transition_n` stores nothing for them.
- A component that cannot be computed, including one without a single observation, is left out. It is never stored as zero.

Only raw values are stored. Grades and 0–8 scores are computed on display with the current thresholds (`grades-v1`), so a threshold revision never rewrites past records. Finishing another step of the same run after the check step recomputes that run's rows. `stats --rebuild-skills` recomputes every run from the keystroke logs after a change to the computation.

### Trend figures

A three-minute check varies a lot from day to day, so a single previous difference cannot separate chance from progress. Each series therefore shows these figures for its latest point:

| Figure | Definition | No data when |
|---|---|---|
| Previous difference | Latest value − the previous run's value in the same series. Raw units; for lower-is-better components a negative difference is an improvement. | The series has only one run. |
| Moving median | Median of the latest 7 runs, including the current one. The window counts runs, not days, so days without practice do not shrink it; the median resists outlier days. | Fewer than 5 runs. |
| Typical change | Median absolute previous difference over the latest 20 differences. | Fewer than 4 differences (5 runs). |
| Beyond typical | `abs(previous difference) > typical change`. Only then does the summary call a change beyond the typical change. | Either value is missing. |
| Trend | Least-squares slope against calendar days of the points within the latest 28 days (ending on the latest point), in units per week. It is `improving` or `declining` when the slope in the better or worse direction exceeds a quarter of the typical change per week, otherwise `flat`. | Points span fewer than two ISO calendar weeks within those 28 days, or the typical change is missing. |

Missing values are shown as `— (no data)` on screen, `null` in JSON and an empty cell in CSV. The thresholds (7 and 5 runs, 20 differences, 28 days, one quarter) are initial values. Because the check step rotates daily between two practice languages, each language's series then gains a point only every other day.

### Growth tab

Growth always uses all records and shows `Compares all time` in its footer; the period and mode filters do not apply. `↑`/`↓` scroll one row and `PgUp`/`PgDn` scroll ten rows, with both directions bounded by the content.

For each language, speed reuses the existing daily-speed series without mixing definitions: the preferred benchmark series (A for English, C for Japanese), or difficulty-adjusted WPM when that language has no such benchmark. The source is printed beside the section title. The first value is the median from the first recorded date through the following six calendar days; the current value is the median from the analysis date and the preceding six days. A comparison starts after seven distinct dates in that speed series, not after seven practice days in general. Until then, a benchmark series says how many benchmark dates exist and points to `[B]` in the title; an adjusted-WPM series calls them comparable-speed records. The headline uses the same source-specific explanation. Accuracy is the median of that language's session accuracies in the same calendar window. The 30- and 90-day comparisons use a ±3-day window around each target date. Missing windows say `no records`.

An upward triangle means a higher speed or accuracy and a downward triangle means a lower value. A difference that rounds to zero at the displayed one-decimal precision has no triangle and is written `±0.0`. The headline first chooses the language with the largest positive first-to-current speed change. If none is available, it chooses the positive first-to-latest benchmark change with the largest percentage gain; changes that display as zero do not qualify. If neither improved, it shows accumulated time and days, adding the source-specific seven-date explanation when appropriate. When comparable speed declined, it states the speed difference without blame and also names an accuracy improvement when one exists; an accuracy change that displays as zero is not described as an improvement.

Benchmark rows use the existing preferred-version rule, and show first, latest and all-time best. For recent personal-best events, benchmark results are first reduced to the highest value for each date, language and part; that daily high is listed only when it strictly exceeds every earlier daily high for the same language and part. Thus several improvements on one day produce one event, while a result below an earlier best produces none. Overall-grade promotions from stored skill snapshots and prescription graduations from recorded menu results are also listed newest first when present. Accumulation sums saved session duration and character keystrokes, distinct practice dates, duration in the analysis date's calendar month, and current/longest consecutive-date streaks; large integer counts use thousands separators. The current streak may end on the analysis date or the preceding day. The speed chart normally groups the reused daily series by ISO week and draws the weekly median. If that produces fewer than two points, it draws the daily values instead and changes the heading accordingly. With fewer than two daily points it shows how many dates have accumulated rather than drawing a chart. The chart labels its first and latest values; terminals without image support use a Braille line chart.

Text version at 80×24 (the middle is scrollable):

```text
┌ ANALYSIS ────────────────────────────────────────────────────────────────────┐
│ Today Coach Trend Keyboard Trans. Fingers Bench Skills Growth                │
│You are +9.4 WPM (+29%) faster than when you started. Practice 12 h 30 min…   │
│■ English                                                                    │
│  Since you started (daily speed: benchmark)                                 │
│    First 7 days    32.1 WPM   accuracy 94.2%   (2026-09-01–09-07)           │
│    Latest 7 days   41.5 WPM   accuracy 97.0%   (2026-11-20–11-26)           │
│    Change          ▲ +9.4 WPM (+29%)   ▲ +2.8pt                             │
│                                                                              │
│  One and three months ago                                                   │
│    1 month ago   37.0 WPM → 41.5 WPM   ▲ +4.5                               │
│    3 months ago  no records                                                 │
│                                                                              │
│  Benchmarks (first → latest, personal best)                                 │
│    A   30.2 → 41.0 WPM   ▲ +10.8   best 43.0 (11/20)                        │
│                                                                              │
│  Speed over time (all time, weekly medians)                                 │
│    first 2026-09-01 32.1 WPM   latest 2026-11-24 41.5 WPM                  │
│    ⡀⡄⡆⡇⣇⣧⣷⣿                                                                   │
│                                                                              │
│■ Accumulation                                                               │
│  Practice time 12 h 30 min (this month 4 h 10 min)                          │
│  Practice days 42   current streak 5   longest streak 9                     │
│  Keystrokes 183,402                                                         │
│            [↑/↓ PgUp/PgDn] Scroll   Compares all time   [Esc] Back          │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Skills tab

The text version of the Skills tab at 80×24 (sample data; with terminal images each chart is drawn as an image, with the current grade's band tinted and a faint vertical line at each date tick):

```text
┌ ANALYSIS ────────────────────────────────────────────────────────────────────┐
│   Today  Coach  Trend  Keyboard  Transitions  Fingers/Hands  Bench  Skills   │
│                                                                              │
│English — • each run  ━ median of last 7 runs  █ bench  bands G–S             │
│Prescriptions (ref.)    ▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀        │
│top half = A, bottom = B (no control): ■Widen preview ①  ■Accuracy gate ①     │
│Speed D 58.0 WPM        ⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤█⠤ C≥60.0 │
│+2.0 WPM to C                                       •    •    •    ⣀•         │
│median 55.0 WPM          •  ⢀⣀⣀█⣀⣀⣀⣀•⣀⣀⠤⠤•⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠒⠒⠒⠒⠒⠒⠒⠒⠒⠊⠉⠉    D≥48.0 │
│                        ⠤⠈⠉⠉⠁⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤ E≥36.0 │
│Accuracy C 96.6%        ⠒⠒⠒⠒⠒⠒⠒█⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒█⠒ A≥98.0 │
│+0.4% to B              ⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒•⠒⠒⠒⠒•⠒⠒⠒⠒⠒•⠒ B≥97.0 │
│median 96.0%            ⠉•⠤⠤⠤⠤⠤•⠤⠤⠤⠤•⠤⠤⠤⠤•⠤⠤⠤⠤⠤⠒⠒⠒⠒⠒•⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠉⠉⠁⠉ C≥96.0 │
│Rhythm  no records                                                            │
│Transitions  no records                                                       │
│Preview C 1.54          ⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒ B≤1.40 │
│-0.14 to B              ⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀•⣀⣀⣀⣀•⣀⣀⣀⣀•⣀⣀⣀⣀⣀•⣀ C≤1.60 │
│median 1.60              •⠤⠤⠤⠤⠤•⠤⠤⠤⠤•⠤⠤⠒⠒•⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠊⠉⠉⠉⠉⠉     D≤1.85 │
│Recovery  no records                                                          │
│Overlap  no records                                                           │
│day                     2026/11/02           11/06     11/08     11/10        │
│        [Tab/←→] tab   [P] period: 30 days   [M] mode: all   [ESC] back       │
│                             [L] language: English                            │
└──────────────────────────────────────────────────────────────────────────────┘
```

The Skills tab, immediately before Growth, draws the series of one language. Its header reads `English — ● each run  ━ median of last 7 runs  ◆ bench  bands G–S` (`•` and `█` in the text chart).

- Each component with records has a label column and a chart. The label column gives the name, the grade and the latest value with its unit (`Speed D 58.0 WPM`), then how far the next grade is (`+2.0 WPM to C`; `top grade` at S), then the median of the latest runs (`median 55.0 WPM`) when the row is tall enough. A component without any record takes a single row (`Rhythm  no records`), and the rows left over go to the charts.
- In each chart, every run is a point and the median of the latest 7 runs is a bold line. The line starts at the first run: until there are 7 runs it is the median of all runs so far (the stored moving median of `stats --skills` still needs 5 runs; from the fifth run on the two agree).
- The grade bands are drawn behind the series. The band of the latest run's grade is highlighted — tinted in its grade colour on the image, its edges drawn in that colour in the text chart — and the other bands stay faint. The vertical range always contains the whole current band, so the next grade's threshold is in view. The column right of each chart names the bands in view, each with the value that enters it (`C≥60.0`); for components where lower is better the chart is drawn upside down, so the better band is always above, and the names read `B≤1.40`.
- The horizontal axis is shared by every chart and the prescription band. It runs from the first to the last day with a record in the selected period — the period only limits which records are used, so a few recent days fill the width instead of sitting at the right end of a 30-day axis. A single day is drawn in the middle. As on the Trend tab, gaps longer than 14 days are cut out and marked with `≈`, and lines are not drawn across them. The bottom row (`day`) labels the first day (`YYYY/MM/DD`), the last day (`MM/DD`) and tick dates between them where they fit; the image charts draw a faint vertical line at each labelled tick.
- Benchmark results are overlaid only on speed (net WPM) and accuracy, the two components that the benchmark trend records per session (part A for English, part C for Japanese). They use a different mark — `◆` with terminal images, `█` in the text chart — so official and practice values stay distinguishable. The other five components have no benchmark points.
- The **prescription history** band above the charts shows, on the same axis, which prescription was played on each menu day: the upper half of a cell is prescription A and the lower half prescription B, and each menu day's colour continues until the next menu day. The legend reads `top half = A, bottom = B (no control):` followed by each prescription with the levels played. It is labelled `Prescriptions (ref.)`: comparing the line before and after a prescription started has no control, so it is a reference, not evidence of an effect.
- `L` switches the series language when both languages have records (by default the interface language, else the first language with records). `P` limits the displayed period to 30 days, 6 months, 1 year, all time or 7 days; medians are still computed over the whole series, so the first point of a period uses earlier runs. `M` (mode filter) does not change this tab, because its series come only from menu check steps and benchmark points are always taken from benchmark sessions.
- Without any skill record the tab shows `No skill records yet. Shown after finishing the check step of a practice menu.` Without terminal images every chart is drawn with text.

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

### Practice menu and skill records

```sh
rs-typing-trainer stats --menu
rs-typing-trainer stats --menu --json
rs-typing-trainer stats --skills
rs-typing-trainer stats --skills --json
rs-typing-trainer stats --skills --csv
rs-typing-trainer stats --rebuild-skills
```

`stats --menu` prints today's [practice menu](training.md#practice-menu) without the TUI: the actual total length (followed by `[menu] minutes` when the two differ, for example `14 min (set: 15)`), the benchmark recommendation, and for each step its number and role, language, prescription ID, title, level, mode, minutes, reason, focus, graduation condition and evidence strength. Once a step of today's menu has been started, the stored plan is printed. It only reads the database. `--json` emits the plan as stored in `menu_runs.plan_json` (`date`, `language`, `catalog_version`, `requested_minutes`, `introductory`, `bench_recommended` and `steps`, each with `role`, `language`, `prescription_id`, `title`, `level`, `mode`, `minutes`, `reason`, `focus`, `advance`, `advance_condition`, `evidence`, `evidence_note` and `settings`; a step's `settings` may also hold its practice target under `target.*`, see [Targets in the prompts](training.md#targets-in-the-prompts)). `requested_minutes` is the configured length; the actual total is the sum of the steps' `minutes`.

`stats --skills` prints, for every language with records, one row per component: the latest value (speed marked `(uncorrected)` when difficulty correction was unavailable), grade, 0–8 score, previous difference, 7-run moving median, typical change, whether the latest difference is beyond it, and the 28-day trend with its slope per week. With no record yet it says so. `--json` emits `grade_version` and `languages`, each with `language` and `components`; every component has `component`, `unit`, `runs`, `latest` (`run_id`, `date`, `raw_value`, `n`, `source`, `corrected`), `grade`, `score`, `previous_difference`, `moving_median`, `typical_change`, `beyond_typical`, `slope_per_week` and `trend` (`improving`, `flat` or `declining`), with `null` for missing values. `--csv` takes no table name with `--skills` and emits one row per stored point:

| CSV | Columns |
|---|---|
| `--skills --csv` | `date, lang, component, raw_value, grade, score, moving_median, previous_diff, corrected, source` |

`moving_median` and `previous_diff` are the values as they stood at that point; missing values are empty cells. `grade` and `score` use the current thresholds.

`stats --rebuild-skills` recomputes the skill records of every menu run from the keystroke logs and replaces the whole `skill_snapshots` table in one transaction, then prints the number of runs and rows. Use it after a change to the computation; the menu runs, steps and prescription states are not touched.

`--lang` only selects the display language: the menu always uses `[menu] languages`, and `--skills` prints every language with records. `--menu`, `--skills` and `--rebuild-skills` exclude one another and cannot be combined with `--summary`, `--grades`, `--coach` or a CSV table name. `--json` requires `--menu` or `--skills` and cannot be combined with `--csv`. These records are stored in database version 8; see [Practice-menu records](records.md#practice-menu-records-version-8).

## Measurement cautions

Repeated text measures memory as well as typing, which is why benchmark forms rotate. Do not trust rare bigrams without their count and shrunk estimate. Judge progress with benchmarks rather than adaptive practice scores. Hold and rollover values require terminal release events and should not be mixed with sessions that lack them. Japanese parts measure direct romaji input, not IME conversion and candidate selection.
