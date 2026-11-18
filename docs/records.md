**English** | [日本語](records.ja.md)

# Record format and metrics

## SQLite v2

The database contains three tables. A completed session and all of its child rows are written in one transaction. `sessions` keeps summary values for fast history sorting; `items` and `keystrokes` are the canonical source for metrics with `metrics_version = 2`.

### `sessions`

| Column | Meaning |
|---|---|
| `id` | Integer primary key |
| `started_at`, `finished_at` | RFC 3339 timestamps |
| `duration_ms` | Session duration |
| `mode`, `mode_params`, `bench_id` | Mode name, JSON parameters and optional benchmark id |
| `layout`, `input_layout`, `geometry` | The configured practice layout, input layout and physical geometry |
| `error_mode` | `stop` or `advance` |
| `release_events` | Whether the terminal supplied key-release events |
| `app_version`, `metrics_version` | Writer and metric-definition versions |
| `net_wpm`, `raw_wpm`, `accuracy`, `iki_median_ms`, `iki_cv` | Main metric summaries |
| `cps`, `consistency`, `max_combo` | Result and history summaries |
| `completed_words`, `timeout_words`, `mistypes_total` | Outcome summaries |
| `legacy_payload` | Original v1 `GameResult` JSON; `NULL` for v2 records |

### `items`

The primary key is (`session_id`, `idx`). Each row stores `text`, optional `reading`, `shown_us`, optional `done_us`, and `outcome` (`completed`, `timeout`, or `aborted`). Microsecond times are relative to the instant when the countdown ended.

### `keystrokes`

The primary key is (`session_id`, `seq`). Each row stores `t_us`, optional `release_us`, `item_idx`, grapheme `pos`, `kind` (`char` or `backspace`), optional `typed`, optional `expected`, `correct`, and optional physical-key `code`. `code` is inferred through the configured input layout and is `NULL` only when the character cannot be mapped. `release_us` is filled when the terminal supports release events. Input discarded during an item transition is not recorded.

The current `PRAGMA user_version` is 8; version 8 adds the [practice-menu records](#practice-menu-records-version-8). When a v1 `sessions` table is detected, it is renamed to `sessions_v1` and retained. Its rows are copied once with `mode = random`, `metrics_version = 0`, and the old JSON in `legacy_payload`. Net WPM and CPS are recalculated from the payload's `correct_chars` and nanosecond-resolution `total_duration`; raw WPM is `NULL` because v1 did not retain enough information to derive it. Databases written by the earlier v2 schema are updated idempotently when opened. Legacy rows can be displayed but cannot be exported as keystroke records.

## Practice-menu records (version 8)

Database version 8 adds three tables for the [practice menu](training.md#practice-menu); opening an older database creates them without changing existing rows, and the existing `prescriptions` and `review_schedule` tables are unchanged. Removing a menu run removes its steps and skill records (`ON DELETE CASCADE`).

### `menu_runs`

One row per menu that was actually started (`stats --menu` alone writes nothing).

| Column | Meaning |
|---|---|
| `id` | Integer primary key |
| `date` | Menu day (local date) |
| `layout` | Practice layout |
| `lang` | The configured practice languages as a sorted, comma-separated key (`en`, `ja` or `en,ja`), not the languages that happen to appear in the steps |
| `catalog_version` | Catalog version, `menu-v1` |
| `minutes` | Requested length: 10, 15 or 20 |
| `plan_json` | The assembled plan exactly as shown, the same JSON as `stats --menu --json` |
| `completed_steps` | Number of recorded steps |

The day, layout, languages, catalog version and length identify the day's run, so reopening the menu continues the same run. While no step of it has been recorded, starting a step stores the current plan in place of the earlier one.

### `menu_steps`

The primary key is (`run_id`, `idx`), with `idx` starting at 1. Each finished step stores `role` (`warmup`, `primary-a`, `primary-b`, `check`, `finish`), `prescription_id` (`fixed.*` for the fixed steps), `level`, the `session_id` of the saved session, `metric` and `value` (the graduation metric of a prescription step; `NULL` when it cannot be measured, never zero), `advance_met` (`1`, `0` or `NULL` when undetermined) and `level_after`. A step is recorded once per run; playing it again does not add a row.

### `skill_snapshots`

The primary key is (`run_id`, `lang`, `component`, `source`). Each row stores the menu `date`, the series language `lang`, `component` (`speed`, `accuracy`, `rhythm`, `transition`, `preview`, `recovery`, `overlap`), `raw_value`, the observation count `n`, `source` (`check` or `check+steps`) and `corrected` (`1` when speed is the difficulty-corrected WPM). Only raw values are stored; grades and scores are computed on display. These rows are derived from keystroke logs and can be recreated with `stats --rebuild-skills`. See [Skill trends](analysis.md#skill-trends).

### Prescription state in `meta`

The current state of each prescription is stored in `meta` under `menu.state.<layout>.<lang>.<prescription_id>` as JSON: `level`, `consecutive` (sessions meeting the condition in a row), `judgment` (`none`, `improved`, `no-change`, `ineffective` or `maintenance`), `paused_until`, `maintenance_due`, `started_on`, `pair_judged_run` (the run whose prescription and check steps were last judged together) and `r3_counted` (the R3 verdict already counted). This state cannot be rebuilt from keystroke logs and is authoritative history, like `prescriptions`.

### Session parameters

A session played as a menu step adds `menu_run_id`, `menu_prescription_id` and `menu_step` (1-based) to its `mode_params`, so a session can be traced back to its menu.

## `.ttrec`

`.ttrec` is JSON Lines. The first line has `"type":"session"`, `format_version: 1`, exact `duration_us`, and the `sessions` fields. Readers also accept older v1 files without `duration_us` and reconstruct it from `duration_ms`. Later lines have `"type":"item"` or `"type":"key"` and the corresponding row fields, ordered by `shown_us` or `t_us`. Export to standard output or a file with:

```sh
rs-typing-trainer export SESSION_ID
rs-typing-trainer export SESSION_ID -o session.ttrec
```

Replay a file without opening the database:

```sh
rs-typing-trainer replay session.ttrec
```

See [Replay](replay.md) for playback controls, summaries, sample records and video recording.

## Metric definitions

- Net WPM: `(correctly confirmed graphemes / 5) / minutes`.
- Raw WPM: `(character keystrokes / 5) / minutes`; Backspace is excluded.
- CPS: correctly confirmed graphemes per second.
- Accuracy: in `stop`, correct character keystrokes divided by all character keystrokes; in `advance`, correct positions in the final editable buffer divided by all remaining positions. Backspace is excluded from both denominators.
- Inter-key interval (IKI): the press-time difference between adjacent keystrokes in the same item. The first keystroke belongs to reaction time instead. Intervals over two seconds are excluded as pauses.
- IKI median and CV: median IKI and population standard deviation divided by mean IKI.
- Consistency: `clamp(1 - IKI CV, 0, 1)` when at least two IKIs exist; otherwise `0`.
- Reaction time: time from `shown_us` to the first recorded keystroke of an item. Mean and p50/p90/p99 use items with a keystroke.
- Maximum combo: the largest run of completed items, reset by a timeout or mistype. A completed item after a mistype starts a new run at one.

Percentiles use linear interpolation between adjacent sorted samples.
