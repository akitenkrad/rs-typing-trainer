**English** | [日本語](training.ja.md)

# Training tools

rs-typing-trainer separates practice from measurement. Practice deliberately changes the prompt distribution or difficulty, so its WPM is not evidence of improvement. Use the fixed parallel forms in the current `bench-v2` to check progress. Retained `bench-v1` results stay visible but are not compared with v2.

## Adaptive weak-transition practice

`adaptive` estimates each observed two-key transition with the same recent shrinkage model used by the analysis screen. For every candidate it draws one log-IKI value from the normal posterior and one error rate from the Beta posterior (Thompson sampling), then assigns the sampled cost

```text
frequency × (max(0, sampled IKI − target IKI)
  + error weight × sampled error rate)
```

Frequency comes from the bundled word corpus after the daily deck has been selected. `[adaptive] lang` selects English words or Japanese romaji readings independently of the interface language. The target IKI is the median estimated IKI across the user's observed transitions. The default error weight is twice that median IKI, treating one error as two average keystrokes of time. These are initial values, not universal training thresholds.

Prompts are drawn in proportion to cost. Three through five transitions form “Today's focus”, each with a localized reason. Focus transitions are interleaved rather than placed consecutively. The initial mix is 70% real words and 30% repeated-transition drills. `max_share = 0.4` limits one transition to 40% of the generated set. A transition that has improved throughout prescriptions in the latest 14 days receives 1.2 times its score; one unchanged after at least three prescriptions receives 0.6 times. A transition used on the previous day still receives half weight. When fewer than 50 transition types have keystroke records, the mode explains that more records are needed and behaves as ordinary `words` practice.

A mastered focus transition enters review with a one-day interval. Retaining the target multiplies the interval by 2.5 (1, 2.5, 6.25 days, and so on); a regression resets it to one day. At most two due reviews occupy the focus set. Applying spaced repetition to motor transitions is an unverified hypothesis, so prescriptions measure transfer to real words before treating it as useful.

Each focus prescription stores up to three untreated controls matched on transition class, corpus-frequency quartile and starting estimate within ±20%. Active controls cannot become focus targets. Evaluation uses real-word keystrokes only, never repeated drills. After at least three prescription sessions and effective count 10 for target and controls, `effect = target log-change - mean control log-change`. Its standard error combines baseline and current posterior variances. An effect no greater than `log(0.95)` with `z <= -1.64` is effective; an absolute effect below `log(1.03)` is ineffective. Other complete cases remain pending, and an unresolved prescription closes as insufficient data after 14 days.

With `practice.auto_difficulty = true`, the median accuracy of the latest three adaptive sessions changes the saved difficulty. Above 97%, pacer target rises by 3% and drill share rises by five percentage points, capped at 45%. Below 92%, they fall by 5% and five points, with a 20% drill floor. The 92–97% band is a hypothesis inspired by the “85% rule” for binary classification (Wilson et al., 2019), not established evidence for motor-skill learning.

The result screen compares this session's IKI for each focus transition with its estimate before the session. A faster estimate naturally lowers future cost. The screen also repeats that adaptive-practice WPM is not an improvement measure.

Goal proposals return one to three choices: the ceiling of the latest ten benchmark (or corrected-WPM) results' p75, the next `grades-v1` threshold when it is within 10%, and a target for a weak component such as accuracy. The player selects a goal; the selected value is stored separately from measured results.

```toml
[adaptive]
lang = "en" # en | ja; independent of the interface language
# Initial mix: 30% drills and 70% real words.
drill_ratio = 0.3
max_share = 0.4
# Optional milliseconds. Omit to use twice the median estimated IKI.
# error_weight = 400.0

[practice]
auto_difficulty = true
```

## Pacer

Press `P` on the mode-selection screen to enable the pacer for `words`, `sentence`, `phrase` or `adaptive`. A `▾` cursor moves above the prompt at the target rate. The initial target is the most recent current-series benchmark A net WPM multiplied by `ratio`. If benchmark A is unavailable, the median net WPM of the latest five sessions is used. The target never falls below `min_wpm`; when neither benchmark nor practice records are available, `min_wpm` is used directly.

The initial ratio is 1.07. After a paced session, accuracy below 92% lowers the next target by 3%. Accuracy of at least 95% together with reaching the target raises it by 2%. Otherwise it stays unchanged. The result screen shows the next target.

```toml
[pacer]
enabled = false
ratio = 1.07
min_wpm = 20.0
```

## Ghost

A `◆` cursor replays comparable historical keystroke timing. Sentence practice uses the fastest previous run containing the same sentence. A benchmark uses the previous run of exactly the same series, part and form, such as `bench-v2/A/03`; benchmark prompts and scoring are unchanged. Other modes do not show a ghost because their prompts are not identical enough for a meaningful comparison.

## Gradual key unlocks

`unlock` starts with eight home-row keys for the active practice layout. It draws real words that can be typed using only unlocked keys and uses the same pseudo-word fallback as `words` with `only` when too few real words qualify.

A session qualifies at 97% accuracy or better when its median IKI is no slower than the target converted from `target_wpm`. Three qualifying sessions in a row unlock one key; a failed session resets the streak. State is stored independently for each layout. The keyboard shows unlocked keys, the next key and locked keys with distinct styles, and the result screen announces a newly unlocked key.

The initial target is 35 WPM. `frequency` adds letters in English frequency order; `rows` completes the home row and then proceeds by row.

```toml
[unlock]
order = "frequency" # or "rows"
target_wpm = 35.0
```

## Accuracy gate

The optional accuracy gate applies to every mode. A session below the threshold is still saved, but it is excluded from personal bests and ranked history views. A value of zero disables the gate.

```toml
[gate]
min_accuracy = 0.0
```

## Fading the keyboard and blind practice

`keyboard.show = "auto"` uses the P5 shrinkage estimates for each key. A key starts fading when estimated accuracy is at least 97% and estimated IKI is no slower than the user's median; keys with more evidence eventually lose their label. The next key remains highlighted. `full`, `dim` and `hidden` retain their original behavior. Benchmarks always use the fixed full display.

`session.blind = true` leaves the prompt visible but suppresses typed-progress and mistype feedback until the item ends. Blind practice forces `advance`, because `stop` would reveal a mistake by refusing to move. Benchmarks ignore blind mode.

```toml
[keyboard]
show = "auto" # initial default remains "full"

[session]
blind = false
```

## Look-ahead practice

Use `[` and `]` on the mode screen to choose zero through three preview items. `mode.sentence.hide_typed = true` removes the completed prefix so attention stays ahead of the current position. When the reaction axis falls below the user's own lower quartile, the title suggests increasing look-ahead. The range and recommendation threshold are initial values.

```toml
[session]
preview_items = 1 # 0..3

[mode.sentence]
hide_typed = false
```

## Rhythm practice

`rhythm` flashes one beat per intended keystroke and scores median absolute deviation from the nearest beat together with accuracy. When `bpm` is omitted, the initial rate is 90% of the rate converted from the user's median IKI. `bell = true` also emits the terminal bell, which may be silent in some terminals. The title recommends this mode only when the rhythm axis is below the user's own lower quartile; imposing a metronome is not generally useful for an already skilled typist.

```toml
[rhythm]
# bpm = 270.0 # optional; derived from the user's median IKI by default
bell = false
```

## Burst and endurance

`burst` runs three short sprints and keeps the highest net WPM. The initial length is 12 words, configurable from 10 through 15. `endurance` uses sentences for an initial four minutes, configurable from three through five. Its result compares first- and second-half median IKI and error rate. It recommends a break when the second half is at least 15% slower or its error rate is at least 1.5 times the first half.

```toml
[burst]
words = 12

[endurance]
minutes = 4
```

## Recovery after mistakes

Analysis and `stats --csv sessions` report the median number of keystrokes needed to return to the session's median IKI after a mistake, plus the error rate in the following three keystrokes. `session.retry_on_error = true` immediately repeats a mistyped item once. These measures complement the existing correction axis rather than replacing the original keystroke log.

```toml
[session]
retry_on_error = false
```

## Practice menu

The practice menu turns the analysis into one day of practice: which prescription to run, with which settings, for how long, what to attend to, and when to move on. The catalog of prescriptions is data bundled with the binary (`data/menu/menu-v1.toml`, version `menu-v1`) and is validated at startup; the code only holds the assembly rules. Every threshold below is an initial value recorded with that catalog version, not an established training standard.

### Opening the menu

The title screen shows `Today's menu (15 min) — M to open` (with `(introductory: more data needed)` when the introductory menu applies). Press `M` to open the menu screen. Each row shows the completion mark, step number, practice language, step name, prescription, level and minutes. The selected row adds the reason it was chosen, the focus, the graduation condition, and the evidence strength with its note. When no benchmark exists in a practice language, or the latest one is at least seven days old, the menu says that a benchmark (about 4 minutes) is recommended first; taking it is optional. See [Playing a menu](gameplay.md#practice-menu) for running the steps.

The menu for a day is fixed once its first step starts (for the same layout, practice languages and length): reopening it later that day shows the same plan with the finished steps marked, so playing a step does not reshuffle the remaining ones. `rs-typing-trainer stats --menu` prints the same plan without the TUI (see [CLI](analysis.md#practice-menu-and-skill-records)).

### Steps and length

| # | Step | Content | 10 min | 15 min (default) | 20 min |
|---|---|---|---:|---:|---:|
| 1 | Warm-up | `words`, pacer off, gate off | 2 | 2 | 2 |
| 2 | Primary A | the first-ranked prescription at its current level | 4 | 5 | 7 |
| 3 | Primary B | the second-ranked prescription at its current level | — | 4 | 4 |
| 4 | Check | `sentence` without aids: one preview item, pacer off; the keyboard follows your own setting | 3 | 3 | 4 |
| 5 | Finish | three `burst` sprints | 1 | 1 | 1 |

`[menu] minutes` selects 10, 15 or 20 (see [Configuration](configuration.md#practice-menu)). The 20-minute plan therefore totals 18 minutes. A prescription step runs for the minutes of its slot above, not for the `minutes` written on the catalog level. The settings of a step override the configuration for that session only and are never written back to the configuration file.

The check step is the daily small measurement. It removes every aid so that its keystrokes can be compared across days; the skill records and most graduation conditions are taken from it, because the prescription steps run with aids.

<!-- FIGURE: the five steps of a 15-minute menu on a time axis (warm-up 2, primary A 5, primary B 4, check 3, finish 1), marking which steps feed graduation and the skill records -->

### How weaknesses are chosen

1. **Enough data.** A language needs an overall grade (speed and accuracy present) and at least 2,000 logged character keystrokes. Otherwise it gets the introductory menu (below).
2. **Accuracy first.** When the accuracy grade is D or lower (below 96.0% under `grades-v1`), primary A is always an accuracy prescription: `accuracy.anticipation` when anticipation is the most common error kind, otherwise `accuracy.gate`. Speed practice is not stacked on careless keystrokes.
3. **Overall gain.** The remaining components follow the existing ranking by how much a one-point increase would raise the overall score (components already at S are skipped; ties keep the fixed component order speed, accuracy, rhythm, transitions, preview, recovery, overlap).
4. **Weaknesses outside the grades.** Reach (the reach axis is below your own 25th percentile), Japanese (a `suggested` romaji spelling exists) and endurance (the latest `endurance` session recommended a break) are put at the front of the list when the top two candidates are both in maintenance. Reach and Japanese are also put at the front when their percentile is below the weakest graded component (its 0–8 score scaled to 0–100).
5. **Variety.** A component that was a primary prescription on both of the previous two days, or whose prescription is in maintenance, moves down one place. The accuracy-first rule is never moved down.
6. **Prescription.** Each component uses its default prescription unless that one was judged “not effective” or “no change”, in which case its alternate is used. A paused prescription is skipped, and so is its component.

Reaction time is not used for ranking. A preview step only adds it to the reason, for example `Preview C (1.58); reaction 412 ms (own median 380 ms)`.

`blind.fade` is in the catalog, but no current assembly rule selects the visual-assistance component, so it is not offered.

### Introductory menu

When no practice language has enough data, the menu is: warm-up (2 min), `transition.adaptive` at its current level as primary A (the remaining time: 3, 8 or 13 minutes), check (3 min) and finish (2 min), with the reason `Introductory menu: fewer than 2,000 logged keystrokes or no overall grade`. With two practice languages and neither ready, the introductory menu alternates the language daily.

### Two practice languages

`[menu] languages` chooses the practice languages; without it the menu follows the interface language. With English and Japanese, one menu mixes both languages without changing its length. Each step is typed in one language, and the step row shows it (`EN`/`JA`, or `英`/`日` in the Japanese interface).

- Candidates, the accuracy-first rule, the overall-gain ranking, the two-day demotion, levels, judgments and skill records are all kept per language.
- Primary A: the language whose accuracy is D or lower (the lower one when both are). Otherwise the language whose first candidate has the larger overall gain; on a tie, the language not used for primary A yesterday, then English.
- Primary B: the first candidate of the **other** language, so that both languages get practice. The 10-minute menu has no primary B, so primary A alternates the language from day to day instead.
- The warm-up uses primary A's language, the finish uses primary B's language (primary A's when there is no B), and the check step rotates the languages daily.
- When one language lacks data, its introductory transition step takes the primary-B slot so that records accumulate.

A Japanese step in `words`, `ngram`, `unlock`, `rhythm` or `burst` receives Japanese word prompts typed in romaji; the recorded mode stays the same and the session is classified as Japanese.

### Levels and graduation

A prescription has one or more levels ordered from most aids to fewest. Following the guidance hypothesis (Salmoni et al., 1984) — relying on aids can leave unaided performance behind — each graduation removes one aid, and the final level checks whether the same result holds without it. Level, the consecutive count and judgments are stored per layout, practice language and prescription.

A graduation condition is judged from one of two sources:

- **The step itself** (`accuracy.gate`, `speed.pacer`, `endurance.split`): judged when the step finishes.
- **The same run's check step** (all others except transitions): judged once per run, when both the prescription step and a check step in the same language have been played, in either order. “At or below your median” compares the check value with the median of your earlier check steps in that language. With no earlier check, or when today's check is in the other language, the condition is not judged in that run.
- **Transitions** (`transition.*`): graduate when the existing R3 verdict of the latest adaptive prescription is “effective”. A verdict counts only once.

A session that meets the condition advances the count; one that does not resets it; one that cannot be determined (missing data) leaves it unchanged. Missing data is never treated as zero. Reaching the required count moves to the next level. Graduating the last level puts the prescription into **maintenance**: it is checked once more after 14 days at a check step, and if that check's component score is below the median of the earlier checks, practice resumes from its last level rather than from level 1.

### Reference judgment, swaps and pauses

| Prescription | Judgment | Swap |
|---|---|---|
| Transitions (`transition.*`) | The existing R3 verdict with matched controls | “Not effective” switches to the alternate. Insufficient data does not switch. |
| Others | **Reference judgment** on the component's 0–8 score from check steps: the median of the last three checks before the prescription started against the median of the last three checks since then. +0.3 or more is “improved”. When at least five checks exist since the start and the last five all stay within ±0.1 of the baseline median, the result is “no change”. | “No change” switches to the alternate. When the alternate (or a prescription that is its own alternate) is also “no change”, it rests for 14 days, during which its component is skipped. |

The reference judgment has **no control**. Anything else that changed during the same period — other practice, familiarity with the prompts, the day's condition — is mixed into it, so the screens always write it as `improved (reference only; no control)`. It is a prompt to keep or change a prescription, not evidence that the prescription works. Practice WPM is still not evidence of improvement; judge progress with benchmarks. The thresholds (+0.3, ±0.1, five checks, 14 days) are initial values.

Reach, Japanese and endurance have no 0–8 score, so they never receive a reference judgment and are not swapped or paused by it.

### Prescription catalog (`menu-v1`)

Evidence strength uses five levels. None is “strong”: no typing-training intervention in this catalog has been confirmed by an intervention study.

| Value | Meaning |
|---|---|
| `phenomenon` | Established as a phenomenon of skilled typists; whether training it helps is unverified |
| `medium` | A general skill-learning principle exists, and the app can check it within the player |
| `correlational` | Only a correlation with faster typists |
| `weak` | Practical know-how or an existing product's convention |
| `hypothesis` | Tried for the first time in this app |

| ID | Name | Aim (component) | Levels (most aids first) | Graduation condition | Evidence |
|---|---|---|---|---|---|
| `accuracy.gate` | Accuracy gate | Accuracy: give up speed and make every keystroke certain | ① `words`, `stop`, gate 97%, no pacer → ② gate 96% → ③ `sentence`, gate 96% | Pass the accuracy gate three sessions in a row (step) | `medium` |
| `accuracy.anticipation` | Control anticipation errors | Accuracy when anticipation is the most common error: confirm the next character before pressing | ① `words`, pacer ratio 0.90 → ② 0.95 | Anticipation share of first errors at or below your median, three times in a row (check) | `weak` |
| `transition.adaptive` | Adaptive transitions | Transitions: prepare the next finger before typing | ① `adaptive`, drill share 30% → ② 20% → ③ `words` with `include` | R3 judges the prescription effective | `medium` |
| `transition.ngram` | N-gram transition drill | Transitions (alternate): isolate a transition, then return to words | ① `ngram`, `pairs` | R3 judges the prescription effective | `weak` |
| `preview.wide` | Widen preview | Preview: look at the next word, not the character being typed | ① `sentence`, preview 3, typed text hidden → ② preview 2 → ③ preview 1, typed text shown | Word-onset ratio at or below your median, three times in a row (check) | `phenomenon` |
| `preview.phrase` | Read through phrases | Preview (alternate): start reading the next phrase at the boundary | ① `phrase`, long course, preview 2 → ② mixed course, preview 1 | Same as `preview.wide` | `weak` |
| `rhythm.metronome` | Even rhythm | Rhythm: keep intervals even rather than faster | ① `rhythm` → ② `rhythm` → ③ `words` without the beat | Within-word IKI CV at or below your median, three times in a row (check) | `hypothesis` |
| `recovery.retry` | Recover after errors | Recovery: correct and return to the prior pace without stopping | ① `sentence`, `advance`, retry on error → ② without retry | Post-error excess time at or below your median, three times in a row (check) | `hypothesis` |
| `overlap.burst` | Relaxed rollover | Overlap: the next press may come before the prior release; stay relaxed | ① `burst` → ② `phrase`, short course, pacer ratio 1.05 | Rollover rate at or above your median, three times in a row (check) | `correlational` |
| `speed.pacer` | Follow a faster pacer | Speed: follow a slightly faster pace, but stop chasing when accuracy breaks | ① `phrase`, pacer ratio 1.05 → ② 1.10 | Reach the pacer target at 95% accuracy or better, three sessions in a row (step) | `medium` |
| `reach.unlock` | Unlock remote rows | Reach: return to the home row after reaching | ① `words`, `only` with home-row base keys → ② `include` | Remote-row/home-row IKI ratio at or below your median, three times in a row (check) | `weak` |
| `japanese.spelling` | Use the suggested romaji spelling | Japanese: use the suggested spelling consistently | ① `romaji`, `include` → ② Japanese `sentence` | Suggested spelling used at least 80% of the time, three times in a row (check) | `weak` |
| `endurance.split` | Hold form through the second half | Endurance: keep the first-half technique in the second half | ① `endurance` 3 min → ② 4 min → ③ 5 min | Second-half slowdown below 15%, three sessions in a row (step) | `hypothesis` |
| `blind.fade` | Fade keyboard assistance | Visual assistance: do not look at the keyboard display | ① `keyboard.show = "auto"` → ② `hidden` → ③ `session.blind` | H5 marks keyboard assistance as removable (check) | `correlational` |

Notes on how the current implementation runs these levels:

- `words`, `ngram` and `romaji` levels use the key selection already in `mode.keys`; the menu does not insert the target transition or remote-row keys.
- Both `rhythm` levels play at the rhythm mode's own tempo (90% of the rate derived from your median IKI); the catalog's pacer ratio does not change that tempo.
- `endurance.split` levels differ in `endurance.minutes`, but a menu step lasts the minutes of its slot, so the length does not grow with the level.
- The spelling-usage rate is not measured yet, so the `japanese.spelling` condition always remains undetermined.
- `blind.fade` is not currently selected (see above).

The evidence notes behind these values: preview follows Salthouse (1986), who found slower typing when preview was restricted; accuracy and progressive overload follow general deliberate-practice principles (Ericsson et al., 1993); rollover and not looking at the keyboard are correlations with faster typists (Dhakal et al., 2018; Feit et al., 2016). Faster typists also make fewer errors (Dhakal et al., 2018), but that correlation is not direct evidence that raising accuracy makes you faster.

```toml
[menu]
minutes = 15            # 10 | 15 | 20
# languages = ["en", "ja"]  # omit to follow the interface language
```

## Why these tools exist

The design follows deliberate-practice principles: target a specific weakness, keep the challenge slightly above current performance, and provide immediate feedback. Fixed benchmarks remain separate because changing practice difficulty makes practice scores incomparable. Sparse transition measurements use shrinkage, and repeated sentences are compared only when their content is identical.

References:

- Ericsson, K. A., Krampe, R. T., & Tesch-Römer, C. (1993). “The role of deliberate practice in the acquisition of expert performance.” *Psychological Review*, 100(3), 363–406.
- Salthouse, T. A. (1986). “Perceptual, cognitive, and motoric aspects of transcription typing.” *Psychological Bulletin*, 99(3), 303–319.
- Feit, A. M., Weir, D., & Oulasvirta, A. (2016). “How We Type: Movement Strategies and Performance in Everyday Typing.” *Proceedings of CHI 2016*.
- Dhakal, V., Feit, A. M., Kristensson, P. O., & Oulasvirta, A. (2018). “Observations on Typing from 136 Million Keystrokes.” *Proceedings of CHI 2018*.
- Salmoni, A. W., Schmidt, R. A., & Walter, C. B. (1984). “Knowledge of results and motor learning: A review and critical reappraisal.” *Psychological Bulletin*, 95(3), 355–386.
