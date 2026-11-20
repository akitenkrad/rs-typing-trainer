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

Choose the Speed row of [individual training](modes.md#individual-training) to play any column with the pacer (the other rows play without it). A `▾` cursor moves above the prompt at the target rate. The initial target is the most recent current-series benchmark A net WPM multiplied by `ratio`. If benchmark A is unavailable, the median net WPM of the latest five sessions is used. The target never falls below `min_wpm`; when neither benchmark nor practice records are available, `min_wpm` is used directly.

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

Use `[` and `]` on the individual-training screen to choose zero through three preview items. `mode.sentence.hide_typed = true` removes the completed prefix so attention stays ahead of the current position. The range is an initial value. Whether to suggest more look-ahead is decided by the [practice menu](#practice-menu) from the grades, not by the title screen.

```toml
[session]
preview_items = 1 # 0..3

[mode.sentence]
hide_typed = false
```

## Rhythm practice

`rhythm` flashes one beat per intended keystroke and scores median absolute deviation from the nearest beat together with accuracy. When `bpm` is omitted, the initial rate is 90% of the rate converted from the user's median IKI. A practice-menu step of `rhythm.metronome` uses its level's ratio instead (see [Practice menu](#practice-menu)). `bell = true` also emits the terminal bell, which may be silent in some terminals. Imposing a metronome is not generally useful for an already skilled typist, so whether to suggest it is decided by the [practice menu](#practice-menu) from the rhythm grade, not by the title screen.

```toml
[rhythm]
# bpm = 270.0 # optional; derived from the user's median IKI by default
bell = false
```

## Burst and endurance

`burst` runs three all-out sprints over a short stretch of 10–15 words and keeps the net WPM of the fastest one. Today's training places it last, as the finish step, to check that you can type fast in the day's technique while staying relaxed. The initial length is 12 words, configurable from 10 through 15. `endurance` uses sentences for an initial four minutes, configurable from three through five. Its result compares first- and second-half median IKI and error rate. It recommends a break when the second half is at least 15% slower or its error rate is at least 1.5 times the first half.

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

Today's training is the first item on the title screen; choose it (or press `M`) to open the menu screen. The menu screen and `stats --menu` show the menu's actual total, followed by `[menu] minutes` when the two differ (see [Steps and length](#steps-and-length)), and say when the introductory menu applies. Each row shows the completion mark, step number, practice language, step name, prescription, level and minutes. The selected row adds the cell of the individual-training table it plays (see [Steps are cells of the table](#steps-are-cells-of-the-table)), the reason it was chosen, the focus, the graduation condition, and the evidence strength with its note. When no benchmark exists in a practice language, or the latest one is at least seven days old, the menu says that a benchmark (about 4 minutes) is recommended first; taking it is optional. See [Playing a menu](gameplay.md#practice-menu) for running the steps.

The menu for a day is fixed once its first step starts (for the same layout, practice languages and length): reopening it later that day shows the same plan with the finished steps marked, so playing a step does not reshuffle the remaining ones. `rs-typing-trainer stats --menu` prints the same plan without the TUI (see [CLI](analysis.md#practice-menu-and-skill-records)).

### Steps and length

| # | Step | Content | 10 min | 15 min (default) | 20 min |
|---|---|---|---:|---:|---:|
| 1 | Warm-up | `words`, pacer off, gate off | 1 | 1 | 1 |
| 2 | Primary A | the first-ranked prescription at its current level, plus the warm-up's spare minute | level (4) + 1 | level (5) + 1 | level (5) + 1 |
| 3 | Primary B | the second-ranked prescription at its current level | — | level (4) | level (4) |
| 4 | Check | `sentence` without aids: one preview item, pacer off; the keyboard follows your own setting | 3 | 3 | 3 |
| 5 | Finish | three `burst` sprints | 1 | 1 | 1 |

`[menu] minutes` selects 10, 15 or 20 (see [Configuration](configuration.md#practice-menu)). “level (n)” means that a prescription step runs for the `minutes` of its catalog level; n is only the slot default for a level that gives no `minutes`. The menu sets aside 2 minutes for the warm-up; the warm-up takes 1, and the spare minute goes to primary A (“+ 1”) unless that would take primary A over its step limit (below), in which case the menu is a minute shorter. The difference is not redistributed to other steps, so the menu screen and `stats --menu` show the actual total, followed by the configured length when they differ, for example `14 min (set: 15)`.

No step runs longer than **5 minutes**, and a step that types long text — `sentence` or `endurance` — no longer than **3 minutes**, because long text tires the arms sooner. The limits apply to every way a step gets its time: a catalog level, the slot default (the 20-minute menu's 7-minute slot becomes 5, or 3 for long text) and the check step (3 minutes in every length; the 20-minute menu's fourth minute is not redistributed). The catalog is checked against the limits when it is loaded, so a level or fixed step over its limit is a catalog error rather than something cut silently. Every level of `menu-v1` gives `minutes`: 4 for most levels, 5 for `transition.adaptive` ① and ②, and 3 for the `sentence` levels and every `endurance.split` level. With two 4-minute prescriptions (primary A becomes 5 with the spare minute) the 10-, 15- and 20-minute plans therefore total 10, 14 and 14 minutes; a `sentence` or 5-minute primary A cannot take the spare minute, so such a plan is a minute shorter. A level that sets a mode's own duration (`endurance.minutes`) must agree with its `minutes`, and the step runs the mode for exactly the step time. The settings of a step override the configuration for that session only and are never written back to the configuration file.

The check step is the daily small measurement. It removes every aid so that its keystrokes can be compared across days; the skill records and most graduation conditions are taken from it, because the prescription steps run with aids.

```text
 0  1           6          10     13 14 min
 ├──┼───────────┼──────────┼──────┼──┤
 │W │ Primary A │ Primary B│Check │F │
 │  │ (level+1) │ (level)  │      │  │
 └──┴───────────┴──────────┴──────┴──┘
          ▲ step-source graduation (e.g. accuracy.gate, endurance.split)
                                  ▲ check-source graduation, reference
                                    verdicts and the skill records
```

A 15-minute menu with two 4-minute prescriptions (W = warm-up, F = finish; primary A takes the warm-up's spare minute, and the actual total is 14 minutes; see the previous paragraph). Prescription steps run with assistance, so most graduation conditions, every reference verdict and the skill records come from the unassisted check step; only `advance.source = "step"` conditions use the prescription step itself. Transitions and recovery in the skill records also pool the same-language warm-up and finish.

### How weaknesses are chosen

1. **Enough data.** A language needs an overall grade (speed and accuracy present) and at least 2,000 logged character keystrokes. Otherwise it gets the introductory menu (below).
2. **Accuracy first.** When the accuracy grade is D or lower (below 96.0% under `grades-v1`), primary A is always an accuracy prescription: `accuracy.anticipation` when anticipation is the most common error kind, otherwise `accuracy.gate`. Speed practice is not stacked on careless keystrokes.
3. **Overall gain.** The remaining components follow the existing ranking by how much a one-point increase would raise the overall score (components already at S are skipped; ties keep the fixed component order speed, accuracy, rhythm, transitions, preview, recovery, overlap).
4. **Weaknesses outside the grades.** Reach (the reach axis is below your own 25th percentile), Japanese (a `suggested` romaji spelling exists) and endurance (the latest `endurance` session recommended a break) are put at the front of the list when fewer than two candidates of rules 2–3 remain after removing maintenance (rule 5). Reach and Japanese are also put at the front when their percentile is below the weakest graded component (its 0–8 score scaled to 0–100).
   Visual assistance (`blind.fade`) goes to the very front when the keyboard display is shown in your settings (`keyboard.show` is not `hidden` and `session.blind` is off) and the H5 assistance comparison marks it removable for that language (the same mark that graduates `blind.fade`). It only takes an aid away, so it is tried as soon as the mark appears.
5. **Maintenance and variety.** A prescription in maintenance is not a candidate. When that leaves fewer than two prescriptions, `transition.adaptive` fills the slot, and its reason says so. A component that was a primary prescription on both of the previous two days moves down one place. The accuracy-first rule is never moved down.
6. **Prescription.** Each component uses its default prescription unless that one was judged “not effective” or “no change”, in which case its alternate is used. A paused prescription is skipped, and so is its component.

Reaction time is not used for ranking. A preview step only adds it to the reason, for example `Preview C (1.58); reaction 412 ms (own median 380 ms)`.


### Introductory menu

When no practice language has enough data, the menu is: warm-up (1 min), transition steps for the remaining time, check (3 min) and finish (2 min), with the reason `Introductory menu: fewer than 2,000 logged keystrokes or no overall grade`. The menu keeps its length: the remaining time is split into the fewest steps of at most 5 minutes, as evenly as possible with the longer steps first. The transition steps alternate between `transition.adaptive` and `transition.ngram`, starting with `transition.adaptive`, each at its current level. All of them are primary A.

| Length | Steps (minutes) |
|---|---|
| 10 min | warm-up 1 · `transition.adaptive` 4 · check 3 · finish 2 |
| 15 min | warm-up 1 · `transition.adaptive` 5 · `transition.ngram` 4 · check 3 · finish 2 |
| 20 min | warm-up 1 · `transition.adaptive` 5 · `transition.ngram` 5 · `transition.adaptive` 4 · check 3 · finish 2 |

`ngram` drills come from the English word pool, and a Japanese `ngram` step would only be a plain romaji word list, so a Japanese introductory menu uses `transition.adaptive` for every transition step. Before the player model has a target transition, an English `ngram` step builds its drills from `mode.keys` and its reason says `no target transition yet` (see [Targets in the prompts](#targets-in-the-prompts)). With two practice languages and neither ready, the introductory menu alternates the language daily and is split the same way in the language of the day.

### Warm-up on its own

`W` on the individual-training screen (shown above the table as `[W] 1-minute warm-up`) plays the menu's warm-up step by itself: the same mode, length and settings as `fixed.warmup` in the catalog (`words` for 1 minute, pacer off, accuracy gate off). It uses the table's practice language (`L`); a Japanese warm-up gets Japanese words typed in romaji and shows the surface / kana / romaji rows, as the menu's Japanese warm-up does. The session is recorded as `words` and is not part of the day's menu run. Its settings apply to that session only: your configuration file is not changed, and the next ordinary session uses your own settings again. `Enter` on its result screen plays the warm-up once more.

### Steps are cells of the table

Every step of today's training is a cell of the [individual-training table](modes.md#individual-training) — a skill (row) and what you type (column) — played with the settings of its level. The catalog names the cell of every fixed step and every level (`cell = { skill = "accuracy", unit = "word" }`), and a cell that the table cannot play is a catalog error. The menu screen (`Drill: Accuracy × Words`), the focus screen before the countdown, and the result screen (its title, and the step row when there is room) show the cell's name; the progress band keeps its short names.

| Step or prescription | Levels | Cell |
|---|---|---|
| Warm-up / Check / Finish | — | Normal × Words / Normal × Sentence / Burst × Words |
| `accuracy.gate` | ① ② / ③ | Accuracy × Words / Accuracy × Sentence |
| `accuracy.anticipation` | ① ② | Anticipation × Words |
| `transition.adaptive` | ① ② ③ | Weak transitions × Words |
| `transition.ngram` | ① | Weak transitions × Combos |
| `preview.wide` | ① ② ③ | Look-ahead × Sentence |
| `preview.phrase` | ① ② | Look-ahead × Phrases |
| `rhythm.metronome` | ① ② ③ | Rhythm × Words |
| `recovery.retry` | ① ② | Recovery × Sentence |
| `overlap.burst` | ① / ② | Rollover × Words / Rollover × Phrases |
| `speed.pacer` | ① ② | Speed × Phrases |
| `reach.unlock` | ① ② | Unlock keys × Words |
| `japanese.spelling` | ① / ② | Spelling × Words / Spelling × Sentence |
| `endurance.split` | ① ② ③ | Endurance × Sentence |
| `blind.fade` | ① ② ③ | No-look × Words |

A step runs and records the mode and settings of its level, exactly as before, so its history, judgments and graduation are unchanged. When that mode is the cell's own mode (a Japanese `words` level counts as the `romaji` of a Japanese word cell), the step makes its prompts the way the cell does on the table: the cell becomes the session's prompt unit, Japanese words come from the same daily deck, and `mode_params` gains `drill` and `unit` as a table session's do. The step is typed in its own language (`[menu] languages`), never in the table's practice language.

A level may override its cell's mode. It then keeps its mode and the prompts of that mode, and records no `drill`:

| Level | Cell | Level's mode |
|---|---|---|
| `transition.adaptive` ③ | Weak transitions × Words (`adaptive`) | `words` with the target transitions |
| `transition.ngram` ① | Weak transitions × Combos (`adaptive`) | `ngram` with `pairs` |
| `rhythm.metronome` ③ | Rhythm × Words (`rhythm`) | `words` without the beat |
| `overlap.burst` ① | Rollover × Words (`words`) | `burst` |
| `reach.unlock` ① ② | Unlock keys × Words (`unlock`) | `words` with the remote-row keys |

### Two practice languages

`[menu] languages` chooses the practice languages; without it the menu follows the interface language. With English and Japanese, one menu mixes both languages without changing its length. Each step is typed in one language, and the end of the step row shows it (`EN`/`JA`, or `英語`/`日本語` in the Japanese interface).

- Candidates, the accuracy-first rule, the overall-gain ranking, the two-day demotion, levels, judgments and skill records are all kept per language.
- Primary A: the language whose accuracy is D or lower (the lower one when both are). Otherwise the language whose first candidate has the larger overall gain; on a tie, the language not used for primary A yesterday, then English.
- Primary B: the first candidate of the **other** language, so that both languages get practice. The 10-minute menu has no primary B, so primary A alternates the language from day to day instead.
- The warm-up uses primary A's language, the finish uses primary B's language (primary A's when there is no B), and the check step rotates the languages daily.
- When one language lacks data, its introductory transition step takes the primary-B slot so that records accumulate.

A Japanese step in `words`, `ngram`, `unlock`, `rhythm` or `burst` receives Japanese word prompts typed in romaji; the recorded mode stays the same and the session is classified as Japanese. When the step has a target, the prompts are Japanese words that contain it (see [Targets in the prompts](#targets-in-the-prompts)).

### Targets in the prompts

A prescription step puts its target into the prompts, using the existing key selection (`mode.keys`, `only`/`include`) and the narrowing of the word pool that the daily deck already uses:

- `reach.unlock`: the remote-row letters of the layout (top and bottom letter rows; `qwertyuiopzxcvbnm` on QWERTY) become `mode.keys`, so level ① (`only` with the home-row base keys) and level ② (`include`) prefer words with them.
- `transition.adaptive` level ③ and `transition.ngram`: the target transitions are the letter-to-letter transitions that `transition.adaptive` selects from the language's player model (up to five, by the adaptive planner with a fixed seed so that the day's plan is reproducible; transitions held out as R3 controls are never targets). Words are narrowed to the ones that contain a target transition, and the keys of the targets become `mode.keys`, from which `ngram` builds its drills. The reason lists them, for example `; target transitions th he`. Before the planner has enough data there is no target: a `words` step then ignores the free-play key selection and its reason says `no target transition yet`.
- `japanese.spelling`: level ① prefers Japanese words whose reading contains the kana of the suggested spelling, and the focus and the reason show the suggestion as the coach does, for example `し: si → shi`.

When fewer than 20 prompts contain the target (`MIN_TARGET_WORDS`), the narrowing is relaxed — the word pool is not narrowed and `only` becomes `include` — and the reason says `only N words contain the target, so the narrowing is relaxed`. This mostly concerns Japanese steps, whose word list is smaller. The targets are stored in the step's `settings` under `target.*` (`target.transitions`, `target.spelling`, `target.relaxed`); they are not configuration keys and are never applied as such.

### Levels and graduation

A prescription has one or more levels ordered from most aids to fewest. Following the guidance hypothesis (Salmoni et al., 1984) — relying on aids can leave unaided performance behind — each graduation removes one aid, and the final level checks whether the same result holds without it. Level, the consecutive count and judgments are stored per layout, practice language and prescription.

A graduation condition is judged from one of two sources:

- **The step itself** (`accuracy.gate`, `speed.pacer`, `endurance.split`): judged when the step finishes.
- **The same run's check step** (all others except transitions): judged once per run, when both the prescription step and a check step in the same language have been played, in either order. “At or below your median” compares the check value with the median of your earlier check steps in that language. With no earlier check, or when today's check is in the other language, the condition is not judged in that run.
- **Transitions** (`transition.*`): graduate when the prescription check (R3, against control transitions) of the latest adaptive prescription says “effective”. A verdict counts only once.

Every step whose cell is on a row other than «normal» shows its row's metric live as a meter with the target line (the gate, your own median, the pacer target with the 95% accuracy line, the 15% slowdown line, the 80% spelling line, or the estimate of the target transitions), and its result screen leads with that metric; the finish step shows its three sprints. The same goal appears when you play a cell from the individual-training table. See [The goal on screen](gameplay.md#the-goal-on-screen).

A session that meets the condition advances the count; one that does not resets it; one that cannot be determined (missing data) leaves it unchanged. Missing data is never treated as zero. Reaching the required count moves to the next level. Graduating the last level puts the prescription into **maintenance**: it is checked once more after 14 days at a check step, and if that check's component score is below the median of the earlier checks, practice resumes from its last level rather than from level 1.

### Reference judgment, swaps and pauses

| Prescription | Judgment | Swap |
|---|---|---|
| Transitions (`transition.*`) | The prescription check (R3, with matched controls) | “Not effective” switches to the alternate. Insufficient data does not switch. |
| Others | **Reference judgment** on the component's 0–8 score from check steps: the median of the last three checks before the prescription started against the median of the last three checks since then. +0.3 or more is “improved”. When at least five checks exist since the start and the last five all stay within ±0.1 of the baseline median, the result is “no change”. | “No change” switches to the alternate. When the alternate (or a prescription that is its own alternate) is also “no change”, it rests for 14 days, during which its component is skipped. |

The reference judgment has **no control**. Anything else that changed during the same period — other practice, familiarity with the prompts, the day's condition — is mixed into it, so the screens always write it as `improved (reference only; no control)`. It is a prompt to keep or change a prescription, not evidence that the prescription works. Practice WPM is still not evidence of improvement; judge progress with benchmarks. The thresholds (+0.3, ±0.1, five checks, 14 days) are initial values.

Prescriptions outside the seven components have no 0–8 score. Reach (`reach.*`) and endurance (`endurance.*`) receive the same kind of reference judgment on the raw value of the catalog's `advance.metric` instead: the median of three values before the prescription started against the median of the latest three since then. “Improved” is a change of at least 5% of the baseline median in the metric's better direction (the reach IKI ratio and the endurance second-half slowdown are both better when smaller); “no change” is at least five values since the start, the last five all within ±2% of the baseline median. A baseline median of exactly 0 has no relative change and stays pending. The values come from the source in `advance.source`: reach compares check steps, using the remote-row/home-row IKI ratio of each check session; endurance compares its own prescription steps with the endurance sessions of the same language before it started. Those endurance steps run with the prescription's aids, so the graduation condition and the summary add that the judgment is taken on the assisted step — an even weaker reference. Swaps and pauses follow the same rules as the seven components. `japanese.spelling` (its spelling-usage rate depends on the suggestion of the day, so there is no baseline to compare) and `blind.fade` (it graduates on the H5 mark alone) are not judged: their graduation condition adds `(no reference judgment)`, and the summary says `no reference judgment`. The thresholds (5%, ±2%) are initial values.

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
| `transition.adaptive` | Adaptive transitions | Transitions: prepare the next finger before typing | ① `adaptive`, drill share 30% → ② 20% → ③ `words` with `include` | The prescription check (R3, against control transitions) finds it effective | `medium` |
| `transition.ngram` | N-gram transition drill | Transitions (alternate): isolate a transition, then return to words | ① `ngram`, `pairs` | The prescription check (R3, against control transitions) finds it effective | `weak` |
| `preview.wide` | Widen preview | Preview: look at the next word, not the character being typed | ① `sentence`, preview 3, typed text hidden → ② preview 2 → ③ preview 1, typed text shown | Word-onset ratio at or below your median, three times in a row (check) | `phenomenon` |
| `preview.phrase` | Read through phrases | Preview (alternate): start reading the next phrase at the boundary | ① `phrase`, long course, preview 2 → ② mixed course, preview 1 | Same as `preview.wide` | `weak` |
| `rhythm.metronome` | Even rhythm | Rhythm: keep intervals even rather than faster | ① `rhythm` → ② `rhythm` → ③ `words` without the beat | Within-word IKI CV at or below your median, three times in a row (check) | `hypothesis` |
| `recovery.retry` | Recover after errors | Recovery: correct and return to the prior pace without stopping | ① `sentence`, `advance`, retry on error → ② without retry | Post-error excess time at or below your median, three times in a row (check) | `hypothesis` |
| `overlap.burst` | Relaxed rollover | Overlap: the next press may come before the prior release; stay relaxed | ① `burst` → ② `phrase`, short course, pacer ratio 1.05 | Rollover rate at or above your median, three times in a row (check) | `correlational` |
| `speed.pacer` | Follow a faster pacer | Speed: follow a slightly faster pace, but stop chasing when accuracy breaks | ① `phrase`, pacer ratio 1.05 → ② 1.10 | Reach the pacer target at 95% accuracy or better, three sessions in a row (step) | `medium` |
| `reach.unlock` | Unlock remote rows | Reach: return to the home row after reaching | ① `words`, `only` with home-row base keys → ② `include` | Remote-row/home-row IKI ratio at or below your median, three times in a row (check) | `weak` |
| `japanese.spelling` | Use the suggested romaji spelling | Japanese: use the suggested spelling consistently | ① `romaji`, `include` → ② Japanese `sentence` | Suggested spelling used at least 80% of the time, three times in a row (check) | `weak` |
| `endurance.split` | Hold form through the second half | Endurance: keep the first-half technique in the second half | ①②③ `endurance` 3 min | Second-half slowdown below 15%, three sessions in a row (step) | `hypothesis` |
| `blind.fade` | Fade keyboard assistance | Visual assistance: do not look at the keyboard display | ① `keyboard.show = "auto"` → ② `hidden` → ③ `session.blind` | H5 marks keyboard assistance as removable (check) | `correlational` |

Notes on how the current implementation runs these levels:

- `words`, `ngram` and `romaji` levels of `reach.unlock`, `transition.*` and `japanese.spelling` use the targets described in [Targets in the prompts](#targets-in-the-prompts).
- The `rhythm` levels play at the level's `pacer.ratio` times the rate derived from your median IKI: 90% at level ① and 95% at level ②. During the menu this replaces a fixed `rhythm.bpm`; outside the menu the rhythm mode keeps its own default (90%, or `rhythm.bpm`).
- Every `endurance.split` level lasts 3 minutes (the long-text limit); the levels do not lengthen the session, so moving up only means meeting the condition again. `endurance.minutes` follows the step time.
- The spelling-usage rate of `japanese.spelling` is the share of the suggested kana's occurrences in the check step that were typed with the suggested spelling (each completed item is split into kana and spellings as the spelling analysis does). A step without a suggestion, or a check step that never typed the kana, leaves the condition undetermined.
- `blind.fade` is offered by rule 4 of [How weaknesses are chosen](#how-weaknesses-are-chosen).

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
