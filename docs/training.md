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

## Why these tools exist

The design follows deliberate-practice principles: target a specific weakness, keep the challenge slightly above current performance, and provide immediate feedback. Fixed benchmarks remain separate because changing practice difficulty makes practice scores incomparable. Sparse transition measurements use shrinkage, and repeated sentences are compared only when their content is identical.

References:

- Ericsson, K. A., Krampe, R. T., & Tesch-Römer, C. (1993). “The role of deliberate practice in the acquisition of expert performance.” *Psychological Review*, 100(3), 363–406.
- Salthouse, T. A. (1986). “Perceptual, cognitive, and motoric aspects of transcription typing.” *Psychological Bulletin*, 99(3), 303–319.
- Feit, A. M., Weir, D., & Oulasvirta, A. (2016). “How We Type: Movement Strategies and Performance in Everyday Typing.” *Proceedings of CHI 2016*.
- Dhakal, V., Feit, A. M., Kristensson, P. O., & Oulasvirta, A. (2018). “Observations on Typing from 136 Million Keystrokes.” *Proceedings of CHI 2018*.
