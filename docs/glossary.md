**English** | [日本語](glossary.ja.md)

# Glossary

## Speed and accuracy

### WPM

Words per minute: typed characters ÷ 5 per minute.

A «word» is counted as five characters, whatever the real words are, so speeds of different texts can be compared. For Japanese the characters are the romaji keystrokes.

### net WPM

WPM counting only the characters left correct.

Characters that were wrong and not corrected are not counted. When a typo stops you until you type the right key (the benchmark and the accuracy drill), every character ends correct, so net WPM reflects errors only through the time they cost.

### raw WPM

WPM counting every character keystroke, right or wrong.

### CPS

Characters per second.

### Accuracy

Aliases: 正打率

Share of character keystrokes that were correct.

When a typo stops you, it is correct keystrokes ÷ all character keystrokes. When you type on past a typo, it is the share of positions left correct after editing.

### pt

Aliases: ポイント

Percentage points: the difference between two percentages.

### Difficulty-adjusted WPM

Aliases: 難しさを補正した WPM

Practice speed adjusted for how hard the prompts were.

Each session's net WPM is divided by the speed your own model expects for its prompts (your form), then scaled to a fixed reference benchmark. Used for daily speed when a language has no benchmark.

### Form

Measured speed ÷ the speed your model expects for the same prompts.

Above 1.0 means faster than usual for those prompts.

### Daily speed

The day's benchmark speed, or the adjusted practice speed when there is no benchmark.

Used for the four-week slope, the Growth tab and the trend charts. Once a language has a benchmark, practice sessions no longer feed it.

## Keystroke metrics

### IKI

Aliases: 打鍵間隔

Inter-key interval: the time between two keystrokes in a row.

Measured within one prompt; the first keystroke of a prompt counts as reaction time instead, and pauses longer than the pause threshold (2 s by default) are left out. Smaller is faster.

### CV

Aliases: IKI CV, 変動係数

Coefficient of variation: standard deviation ÷ mean of the intervals.

How uneven your rhythm is. Smaller means more even intervals. The rhythm drill aims to lower the CV within words.

### ms

Milliseconds (1/1000 of a second).

### Transition

Aliases: 2-gram, bigram

Moving from one key to the next, such as «t → h».

Each pair of keys is measured for its interval and error rate. Rare pairs are estimated with help from similar pairs (shrinkage).

### n-gram

Aliases: ngram

A run of n characters, such as «th» (2-gram) or «the» (3-gram).

The n-gram drill repeats short runs cut out of words to practise a transition on its own.

### Word onset delay

Interval at the start of a word ÷ interval inside words.

Close to 1.0 means you do not stop at the start of words, i.e. you read ahead. The look-ahead drill aims to lower it.

### Rollover rate

Aliases: 重なり

Share of key pairs where the next key went down before the previous one came up.

Measured only in terminals that report key releases. Fast typists tend to overlap keys more.

### Reaction time

Time from a prompt appearing to its first keystroke.

### Excess after errors

Extra time the keystrokes after a typo took compared with your usual pace.

The recovery drill aims to keep it at or below your own median.

### Key-release timing

Whether the terminal reports when a key is let go.

Needed for rollover and key hold times. Shown on the About screen.

### n= / n≈

Number of samples behind a value: n= is counted, n≈ is an effective count.

## Analysis

### Grade (S–G)

Aliases: S〜G, grades-v1

Absolute grade from G (lowest) to S (highest), per component and overall.

Each grade also has a continuous score from 0 to 8. The thresholds are provisional and versioned (grades-v1).

### Component

One of the skills graded on its own: speed, accuracy, rhythm, transitions, look-ahead, recovery, rollover.

The overall grade is a weighted harmonic mean of the component scores, so a weak component pulls it down.

### Eight axes

Benchmark radar: sustained speed, accuracy, rhythm, transitions, reach, reaction, correction, Japanese.

Each axis places your latest value within your own past benchmarks (percentile). With fewer than three benchmarks only raw values are shown.

### Four-week slope

Change in daily speed per week over the last 28 days.

Above +0.5 WPM/week is shown as rising, below −0.5 as falling, otherwise flat.

### R3

Aliases: 処方の効果判定

The check of whether a transition prescription worked, against control transitions.

Controls are similar transitions kept out of the drill. After three sessions with enough samples, the target must improve clearly more than the controls to be judged effective.

### H1–H5

Aliases: 習慣

Practice-habit findings: H1 warm-up, H2 fatigue within a sitting, H3 daily volume and retention, H4 time of day, H5 training aids.

H4 is exploratory. H5 marks an aid (such as the on-screen keyboard) as removable when you type as well without it.

### Evidence strength

Aliases: phenomenon, medium, correlational, weak, hypothesis

How well a prescription is supported: medium, phenomenon, correlational, weak, hypothesis.

medium: a general skill-learning principle you can check in this app. phenomenon: seen in experts, but training it is unverified. correlational: only correlated with speed. weak: practical know-how. hypothesis: tried first in this app.

## Practice menu

### Prescription

A drill the menu picks for your weakest point, with levels and a graduation condition.

### Primary A / B

The two main drills of today's menu, chosen from your weakest components.

### Check step

Aliases: 補助なしの確認

The menu step typed without training aids; several graduation conditions are judged here.

### Graduation

Meeting a prescription's condition the required number of times; the level then goes up.

### Your own median

The middle value of your recent sessions of the same kind; many goals use it as their line.

### Cell

One box of the individual-training table: a skill × what you type (letters, words, sentences…).

## Drills

### Pacer

A marker that moves at a set speed for you to follow.

### Ghost

A replay of one of your past sessions shown as a pace to race.

Sentences use your best past session with the same sentence; a benchmark uses your previous result of the same part and version.

### Gate

The accuracy a session must reach to count as passed or toward your bests.

### Anticipation error

Pressing the key after next before the current one.

### Repeated previous

Pressing the key you just typed again instead of the next one.

### Burst

Aliases: 短距離

Short all-out sprints; the fastest one counts.

### Endurance

Typing for several minutes; the second half is compared with the first.

## Benchmark

### Benchmark

Aliases: ベンチマーク

A fixed test taken under the same conditions each time, used as the yardstick for progress.

### bench-v1 / bench-v2

Versions of the benchmark. Results of different series are not compared directly.

### Parts A–D

A: English words and phrases; B: pseudo-words; C: Japanese words and phrases; D: numbers and symbols (optional).

### Parallel form

Aliases: 版

One of eight versions of each benchmark part, used in turn so you do not memorise the prompts.

### Pseudo-word

A made-up word with English-like letter patterns, to measure finger transitions without word familiarity.

### Tempo

WPM converted from the median interval between eligible correct keystrokes.

It uses consecutive correct keystrokes within one item. The first keystroke of each item, the first three correct keystrokes after an error, and intervals above the pause threshold are excluded.

### Accuracy-adjusted speed

The benchmark's main score: tempo multiplied by accuracy.

It expresses how quickly the correctly typed share was entered. Tempo and accuracy are shown beside it so that changes can be interpreted.

### This run's error margin

Half the difference between accuracy-adjusted speeds for the odd- and even-numbered items.

Tempo and accuracy are calculated separately for each half and multiplied. Half of the absolute difference is shown as a split-half estimate of the run's error.

## Records

