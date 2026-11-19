**English** | [日本語](gameplay.ja.md)

# Playing rs-typing-trainer

## Starting a session

Run `rs-typing-trainer` or `cargo run --release` to open the title screen. The default interface language is English; use `--lang ja` for Japanese. An explicit `--config PATH` takes the place of the normal configuration search.

When terminal images are available and the window has enough room, the title screen centres a two-row keycap logo drawn with the selected theme and large font. If images are disabled or unavailable, or the window is too narrow or short, it keeps the text logo so that every title action remains visible.

Running the program without a subcommand is the same as `rs-typing-trainer play`. `replay FILE` plays a portable record, `stats` prints saved-session analysis, `graphics` reports terminal-image support, and `export SESSION_ID [-o FILE]` writes a v2 session as `.ttrec`, using standard output when `-o` is omitted. Replay reads only its record file; see [Replay](replay.md).

## Terminal graphics check

`rs-typing-trainer graphics` prints the selected protocol, cell pixel size, reason, `TERM`, `TERM_PROGRAM`, and the presence of tmux, screen, and SSH. It follows `--lang`, reads `display.graphics`, and accepts the global `--graphics auto|kitty|sixel|iterm2|off` override.

`rs-typing-trainer graphics --demo [--seconds N]` opens the alternate screen and shows a roughly 40×12-cell gradient, one-pixel grid, and corner marks with the selected protocol. It closes after three seconds by default or on a key press. If images are unavailable, it shows the reason as text. This diagnostic does not change the normal game rendering yet; see [Terminal graphics](graphics.md).

## Controls

| Screen | Keys | Action |
|---|---|---|
| Title | `↑` / `↓` (`k` / `j`) | Move the menu selection |
| Title | `Enter` | Run the selected item: Today's training (the practice menu, selected first), Individual training (mode selection), Benchmark, Analysis, History, Settings or Quit |
| Title | `Space` | Open mode selection directly (Individual training) |
| Title | `H` | Open history |
| Title | `B` | Open `bench-v2` selection |
| Title | `A` | Open analysis |
| Title | `M` | Open today's practice menu |
| Title | `S` | Open settings |
| Title | `Q` / `Esc` | Quit |
| Mode selection | `↑` / `↓` (`k` / `j`) | Select any practice mode |
| Mode selection | `P` | Toggle the pacer on supported modes |
| Mode selection | `L` | Switch the language for phrase or adaptive practice |
| Mode selection | `C` | Cycle the phrase course: short / normal / long / mixed |
| Mode selection | `[` / `]` | Decrease/increase preview items |
| Mode selection | `Z` | Switch the prompt text between large and normal (saved when the mode starts) |
| Mode selection | `Enter` | Continue to key selection or the countdown |
| Benchmark selection | `↑` / `↓` (`k` / `j`) | Select Resume (when shown), Start, or optional Part D |
| Benchmark selection | `Enter` | Run the selected action |
| Benchmark selection | `Space` | Start the benchmark directly |
| Benchmark selection | `D` | Toggle optional Part D directly |
| Benchmark selection | `Esc` / `Q` | Return to the title screen |
| Key selection | Arrow keys | Move on the keyboard |
| Key selection | `Space` | Toggle the current key |
| Key selection | `Tab` | Switch `include` / `only` |
| Key selection | `Enter` | Save the selection and start the countdown |
| Countdown | `Esc` twice within 2 seconds | Abort to the title screen; another key or the timeout cancels the first press |
| Play | Printable keys | Type the word |
| Play | `Backspace` | Move back in `advance` error mode |
| Play | `Esc` twice within 2 seconds | Abort to the title screen; the timer continues after the first press, and another key or the timeout cancels it |
| Just-played result | Any key during `session.result_input_delay_ms` | Ignore the input; the hint shows the approximate remaining delay |
| Result | `Space` / `Enter` | Start another session |
| Result | `H` | Open history |
| Result | `Q` / `Esc` | Quit |
| Benchmark part result | `Space` / `Enter` | Continue to the next part or summary after the input delay |
| Benchmark part result | `Esc` twice within 2 seconds | End the benchmark after the input delay, retaining completed parts |
| Benchmark part result | `H` / `Q` | No action |
| Benchmark summary | Any key during `session.result_input_delay_ms` | Ignore the input; the hint shows the approximate remaining delay |
| Benchmark summary | `Enter` / `Space` / `Esc` / `Q` | Return to the title screen after the input delay |
| Coach | `↑` / `↓` (`k` / `j`) | Scroll the report |
| Coach | `T` | Open or close goal candidates |
| Goal candidates | `↑` / `↓`, `Enter` | Select and save a goal |
| Analysis keyboard | `Enter` | Cycle error rate, median IKI, relative speed and confusion |
| Analysis confusion | `←` / `→` | Select the expected key |
| History | `↑` / `↓` (`k` / `j`) | Move the selection |
| History | `PgUp` / `PgDn` / `Home` / `End` | Jump through the list |
| History | `Tab` | Cycle All / Practice / Bench |
| History | `M` / `L` / `P` | Cycle recorded modes, language, or period |
| History | `S` | Cycle newest, oldest, WPM and accuracy sort orders |
| History | `Enter` | Expand/collapse a benchmark run, or open a session/part in detail |
| History | `Esc` / `Q` | Return to the screen that opened history |
| History detail | `Space` / `Enter` / `Esc` | Return to history |
| History detail | `R` | Replay a v2 session inside the application; `Esc` returns to the detail |
| History detail | `E` | Export a v2 session as `session-ID.ttrec` and show the path |
| History detail | `Q` | Quit |
| History replay | `Space` / `→` | Pause/resume, or advance one keystroke |
| History replay | `Esc` / `Q` | Return to the history detail |
| Settings | `↑` / `↓` (`k` / `j`) | Select a setting |
| Settings | `←` / `→` (`h` / `l`) | Change the selected value |
| Settings | `Enter` | Save and return to the title |
| Settings | `Esc` / `Q` | Cancel and return to the title |
| Practice menu | `↑` / `↓` (`k` / `j`) | Select a step |
| Practice menu | `Enter` / `Space` | Start the selected step |
| Practice menu | `Esc` / `Q` | Return to the title screen |
| Menu focus | Any key / `Esc` | Start the countdown now (it starts after about two seconds otherwise) / abandon the step |
| Menu step countdown or play | `Esc` twice within 2 seconds | Abandon the step without saving it and leave the run; another key or the timeout cancels the first press |
| Menu step result | `Space` / `Enter` | Continue with the next step (or the next `burst` sprint) |
| Menu step result | `Esc` | Leave the run: the summary when a step has finished, otherwise the menu |
| Menu summary | `Enter` / `Space` / `Esc` | Return to the menu |
| Analysis skills | `L` | Switch the language of the skill series |

While a word is sliding in or out, for about 200 ms on each side, keystrokes are dropped. This prevents the key that completed one word from being credited to the next word.

If today’s latest saved benchmark form has a missing required part, the benchmark-selection screen offers to resume from the first missing part using the same form number. Parts A through C define completion; optional Part D does not keep a benchmark in the resumable state. Starting a new benchmark remains available and follows the normal form-rotation rule.

## Playing-screen layout

The playing screen shows, from top to bottom: the header (time, WPM, item count and pace), the keyboard, the item timer bar, the combo/mistype/accuracy row, the prompt, and the preview and controls on the bottom row. The prompt starts immediately below the combo row instead of being vertically centred in the area below the keyboard, and remains centred horizontally.

`display.prompt_style` chooses how the prompt is drawn: `large` (default) or `normal`. The settings screen calls it "Prompt text" with the values "Large text" and "Normal text", and `Z` on the mode selection screen toggles it.

**Large text** draws the item being typed with the bundled Shinonome bitmap font. Four font dots become one terminal quadrant-block cell, so a 12-dot fullwidth character occupies 6×6 cells and a 16-dot character occupies 8×8 cells. One blank row separates each large block from the normal-size rows around it, and labels sit in a column to the left of the block they name, on its top row:

- English item modes, including phrases: the large item and its `▔` cursor, then a blank row and a helper row (locking, mistype details, mistypes in this item, or the result) when there is room.
- Romaji, including Japanese phrases: the large written form, a blank row, the kana (bold, accent colour, the kana being typed reversed), a blank row, then the large romanization and its cursor. The labels (Surface / Kana / Romaji) line up in the left column and the blocks start at the same column.
- Sentence and endurance: a large window around the current position and its cursor, a blank row, then the full sentence at normal size with the current position reversed. Japanese sentences show the romaji layout above, then a blank row and the surface / kana / romaji rows of the whole sentence.

When the area is short, blank rows are given up first; glyphs are never cut. Pacer and ghost markers are placed above the corresponding large character. Blind mode leaves completed large glyphs blank. Large prompts keep one blank column inside the frame on each side. They need a terminal font that contains the quadrant block elements (U+2596–U+259F and U+2580–U+2590); if the terminal substitutes a fallback font whose glyphs are wider than one cell, the large rows can look wider than the frame, and `display.prompt_style = "normal"` avoids them. A character outside the bundled font (outside JIS X 0208 and ASCII/halfwidth kana) is drawn at normal size in the middle of its large cell.

Large text picks its size from the terminal: 16-dot with the selected keyboard style, 12-dot with that style, 12-dot with a simpler keyboard, 12-dot with no keyboard, and finally normal text with whatever keyboard still fits. The prompt and controls are never removed. `keyboard.show = "hidden"` omits and collapses the keyboard area at every height.

**Normal text** draws the prompt at terminal size inside a rounded card (`╭╮╰╯`) with two columns and one row of padding, centred below the combo row. Text still to type is bright and bold, the current character is reversed (on a red background while a mistype flashes), and typed text uses a calm colour. Item modes (words, English phrases, random, n-gram, adaptive, unlock, rhythm, burst) put one blank cell between characters, with the `▲` caret directly below the current character. Romaji, including Japanese phrases, uses two columns — muted labels on the left, text on the right — for the written form (bold), kana and romaji, with a blank row between them; the current kana and romaji character are reversed. Sentences wrap inside the card; the row being typed is bright, the other rows are slightly dimmed, and the caret row follows the current row. Japanese sentences use the three-row layout with each row wrapped. Pacer and ghost markers sit on the row directly above the characters. Helper messages appear under the card. Normal text keeps the card in preference to the keyboard: the keyboard is simplified or hidden first, and on a very small terminal the card gives up its blank rows, padding and border before any text.

## Result screen

When terminal images are available and the window is large enough, the result screen uses four cached pixel visuals drawn with the selected theme. WPM, CPS and accuracy appear in font-rendered cards. Each card shows available changes from the preceding matching session and the personal best; upward green and downward mistype-colour markers mean higher and lower values. The WPM card also plots up to the latest ten matching sessions, including the displayed session. A history detail never uses sessions later than the selected entry for its preceding value or recent line.

The middle row plots speed by completed item and the reaction-time distribution. Item speed is the number of correct character keystrokes for that item, divided by five and then divided by the item's completion time in minutes. The chart adds the whole-session WPM as a horizontal line and marks items containing mistypes. Fewer than two usable completed items produces a no-data message. Reaction times use 0.1-second buckets, with every value at or above 1.5 seconds in the last bucket, plus p50 and p90 markers.

The lower row maps expected mistyped characters to the configured keyboard layout. Colour strength represents count, the three most frequent mapped keys include their counts, and unmatched strings are reported as `other N`. A session without mistypes keeps the perfect-run message. The session observation remains beside this keyboard while the reaction distribution stays visible above it.

If terminal images are disabled or unavailable, the result uses text for the same information. A narrow or short terminal also falls back to text for visuals that do not fit. Text mistype bars use separated `▆` rows, and the reaction distribution remains visible even when a session observation exists. Benchmark-part results keep their dedicated comparison screen; adaptive details and history conditions remain part of their respective result views.

After the final benchmark part, the summary fills the available height with three sections. The top shows the run grade and component grades beside one card per completed part. Each card contains WPM, accuracy, the change from the preceding run, the change from the previous personal best, and the short form ID. The middle shows up to 20 attempts per part with separate WPM and accuracy lines; the current run is enlarged. For A/C, a labelled v2 divider breaks the line between versions. The lower section combines expected mistyped keys across every part in this run and lists up to five sufficiently observed slow transitions, one or two weak fingers, improvements and regressions from the preceding matching part, and one to three concrete practice suggestions. Existing active transition prescriptions take priority.

The summary degrades visuals in this order as space becomes scarce: trends, mistype keyboard, grade badge, then part cards. Its text form still includes the grade line, every part's WPM/accuracy/preceding/personal-best values, and the analysis prose. An 80×24 terminal therefore remains usable without overlapping the return hint.

## Practice menu

Choose Today's training (the first title item, or `M`) to open today's practice menu. How the steps are chosen, their levels and graduation conditions are described in [Training tools](training.md#practice-menu); this section covers playing them.

### Playing the steps

Select a step with `↑`/`↓` and press `Enter`. The steps are normally played from the top, but any step can be started. Before each step, a `FOCUS` screen shows what to attend to for about two seconds; any key starts the countdown at once, and `Esc` abandons the step. The step then runs with its own mode and settings for the minutes shown on the menu. These settings apply only to that session; your configuration file is not changed.

The finish step consists of three `burst` sprints: `Enter` after each sprint starts the next one, and the step counts as finished after the third. On the result screen of every step, `Enter` continues with the next step, and `Esc` leaves the run. During the countdown or play of a menu step, `Esc` asks first, as in ordinary sessions: the first press shows `Press Esc again to stop (this step is not recorded)` during play (`Press Esc again to quit` during the countdown), and a second `Esc` within 2 seconds abandons the step without saving it and leaves the run. Another key or the timeout cancels the first press. A finished step is marked `Done` on the menu. Playing a `Done` step again saves the session but does not count toward the menu a second time.

When you leave the run after at least one finished step — with `Esc`, or by finishing the last step — the menu summary screen opens first. `Enter`, `Space` or `Esc` on it returns to the menu.

### Where you are in the menu

The countdown before each step and the result screen after it show the whole menu on one line: every step by a short name (the practice component of a prescription step, such as `Transition`, or the role of a fixed step) and its minutes, `✓` on finished steps and `▶` on the step about to start — on the result screen, the step that `Enter` starts next. The line ends with the position and the minutes left, which count the steps not finished yet, including the marked one:

```text
✓Warm-up 2 › ▶Transition 4 › Transition 4 › Check 3 › Finish 2  Step 2/5 · 13 min left
```

When the window is too narrow, the minutes of finished steps are dropped first, then every step's minutes, and last the line shrinks to `Step 2/5 ▶Transition · 13 min left`. Width is measured as terminal columns, so Japanese names count double. A session started from Individual training shows no such line. The countdown of the first step of a 15-minute introductory menu at 80×24:

```text
┌ Get ready! ──────────────────────────────────────────────────────────────────┐
│                                                                              │
│                                   ┌───────┐                                  │
│                                   │       │                                  │
│                                   │   3   │                                  │
│                                   │       │                                  │
│                                   └───────┘                                  │
│                                                                              │
│                              Get ready to type!                              │
│                                                                              │
│  ▶Warm-up › Transition › Transition › Check › Finish  Step 1/5 · 15 min left │
│                                                                              │
└──────────────────────────────────────────────────────────────────────────────┘
```

(Blank rows above and below are trimmed. At 80 columns the English line drops the minutes; the Japanese one keeps them: `▶ウォームアップ 2 › 遷移 4 › 遷移 4 › 確認 3 › 仕上げ 2  段 1/5 · 残り 15 分`.)

### The menu line on the result screen

The result screen of a menu step adds two lines above the key hint — the menu line, then the progress line under it — and the hint becomes `[Enter] Next step    [Esc] Menu`. The bottom of the result screen after the warm-up of the same menu at 80×24:

```text
│  💾 Saved to DB → row #1                                                     │
│                                                                              │
│  ✓Warm-up › ▶Transition › Transition › Check › Finish  Step 2/5 · 13 min left│
│  [Enter] Next step    [Esc] Menu                                             │
└──────────────────────────────────────────────────────────────────────────────┘
```

The warm-up has no graduation condition, so its menu line is empty. For a prescription step it reads:

```text
Menu 2/5 Widen preview ② — Graduation condition: Keep word-onset delay at or below your median for three sessions. — This session: judged after the check step
```

The step position and title are dropped first when the window is narrow. “This session” is one of:

- `met 2/3`, `not met 0/3` or `cannot determine 1/3`: the outcome of this session and the consecutive count toward the level's required number;
- `graduated → level 3` or `graduated → maintenance`;
- `judged after the check step`, when the condition uses the same run's check step and it has not been played yet;
- `today's check is in Japanese, judged next time` (or English), when today's check step is in the other language.

`; improved (reference only; no control)` is appended when the reference judgment taken at this step is “improved”. The check step has no condition of its own; its line lists the prescriptions it judged, for example `Judged with this check: Widen preview met 2/3; Even rhythm not met 0/3`.

### Menu summary screen

The summary is titled `TODAY'S PRACTICE 2026-11-18 (15 min / 5 of 5 steps)`. When you leave before the last step, it is titled `SUMMARY SO FAR` and unplayed steps show `not played`. It has three sections.

The text version of the summary screen at 80×24 (sample data):

```text
┌ TODAY'S PRACTICE 2026-11-11 (15 min / 5 of 5 steps) ─────────────────────────┐
│  Step           Prescription · level        This time Prev     Graduation    │
│  1 Warm-up   EN words                       62 WPM    ▲+3                    │
│  2 Primary A EN Accuracy gate ①             met       —        ●●● → Lv 2    │
│  3 Primary B EN Widen preview ②             1.48      ▲-0.06   ●●○ 2/3       │
│  4 Check     EN sentence                    58 WPM    ▲+1                    │
│  5 Finish    EN burst                       74 WPM    —                      │
│  ── Skills now — English (check step; vs 7-run moving median) ─────────────  │
│  Component   Gr This time    Prev diff Median (7) 28 days  Trend             │
│  Speed       D  59.0 WPM     ▲+0.5     57.5       ▁▂▁▃▄▅▆▇ improving         │
│  Accuracy    B  97.0 %       ±0.0      97.0       ▄▄▄▄▄▄▄▄ flat              │
│  Rhythm      — (no data)                                                     │
│  Transitions — (no data)                                                     │
│  Preview     B  1.40         ▲-0.16    1.56       ▇▆▆▆▆▅▆▁ improving         │
│  Recovery    — (no data)                                                     │
│  Overlap     — (no data)                                                     │
│  Overall (from practice): C 4.51 (prev C 4.34)                               │
│  Official grade (bench): — (no data)                                         │
│  ── Today's changes ───────────────────────────────────────────────────────  │
│  ・EN Accuracy gate moves up to level 2                                      │
│  ・EN Widen preview: improved (reference only; no control)                   │
│  ・Preview: improved beyond the typical change (-0.16; typical ±0.01)        │
│  ・No benchmark yet. A benchmark (about 4 min) is recommended                │
└ [Enter/Esc] Back to the menu ────────────────────────────────────────────────┘
```

**Upper section — this run.** One row per planned step: `Step`, the practice language, `Prescription · level` (the level as ①②③), `This time`, `Prev` and `Graduation`.

- `This time` is the prescription's graduation metric (for example `1.48` for word-onset delay, `412 ms`, `18%`), or net WPM for the warm-up, check and finish. Pass/fail metrics (accuracy gate, pacer target, R3, keyboard assistance) show `met` or `not met`.
- `Prev` is the difference from the latest earlier run of the same prescription at the same level in the same language. Green `▲` is better, red `▼` is worse and `±` is unchanged; for lower-is-better metrics, a decrease is shown as `▲`. Pass/fail metrics have no difference.
- `Graduation` shows the consecutive count as dots, for example `●●○ 2/3`. A step that graduated shows `●●● → Lv 3` or `●●● → maint.` in bold green.

**Middle section — skills now.** `Skills now — English (check step; vs 7-run moving median)`. It appears only after the run's check step, and before that reads `Shown after the check step`. One row per component: `Gr` (grade under the current thresholds), `This time`, `Prev diff`, `Median (7)`, a 28-day line drawn with `▁▂▃▄▅▆▇`, and `Trend` (`improving`, `flat` or `declining`). A component without a value in this run shows `— (no data)`; it is never shown as zero. The definitions are in [Skill trends](analysis.md#skill-trends).

The last two rows keep two sources apart:

- `Overall (from practice): C 3.40 (prev C 3.30)` — computed from this run's check-step values with the usual overall formula. It is a practice value from one three-minute check.
- `Official grade (bench English, 11/12): C` — the grade from benchmarks, as in the Coach tab, marked `(interim)` when fewer than three results exist. Only benchmarks change the official grade; a good practice overall never promotes it.

**Lower section — today's changes.** Up to four lines, in this order: level changes, swaps and pauses, judgments, changes beyond the typical change, and the benchmark reminder. Examples: `EN Widen preview moves up to level 2`, `EN Widen preview: no change (reference only; no control); switching to Read through phrases next time`, `EN Accuracy gate: improved (reference only; no control)`, `Preview: improved beyond the typical change (-0.06; typical ±0.04)`, `5 days since the benchmark. Re-measure in 2 days`. Transition judgments by R3 are written `effective (R3, controlled)` or `not effective (R3)`.

With terminal images, a window of at least 120×29 cells draws the 28-day lines as images, and at least 37 rows adds the practice-overall grade badge. Otherwise everything is drawn as text, so an 80×24 terminal keeps every value.

## Session history

Every completed session is stored in a local SQLite database. By default the file is `~/.local/share/rs-typing-trainer/sessions.db`; set `storage.db_path` in the configuration to use another location. The old `metrics.db_path` name remains accepted, with `storage.db_path` taking precedence when both are present.

When the new default file does not exist but `~/.cache/rs-typing-trainer/sessions.db` does, the program copies the old database to the new location and leaves the old file untouched. The title screen reports that copy once. No automatic copy is attempted when a database path is configured or when both default files already exist.

Press `H` on the title or result screen to open history. The list shows date, kind, content, language, layout, WPM, accuracy and duration; layout, language and duration are dropped in that order as the terminal narrows. Legacy v1 imports, practice and benchmark records have distinct kinds. Benchmark parts from the same series and form are one run when each next part starts within 30 minutes of the preceding part's finish. `Enter` expands that run into its saved parts, including incomplete runs.

`Tab` filters All / Practice / Bench. `M` cycles only modes with records after the type, language and period filters are applied, `L` cycles all / `en` / `ja`, and `P` cycles all time / 30 days / 7 days. `S` cycles newest date, oldest date, WPM and accuracy. A WPM rank appears only after one practice mode or benchmark part is selected; only score-eligible sessions count, and benchmark ranks are separate for each benchmark series and part.

The detail keeps the original result charts and adds the recorded mode, language, layouts, error mode, assists, key filter, benchmark ID, word count and maximum combo when available. It also shows the personal-best WPM and the change from the previous session under the same condition. `R` replays a v2 session in the application and `E` writes it as `.ttrec`; v1 imports have no keystroke log and offer neither action. Returning from a detail preserves the selection, expanded benchmark runs and filters. Leaving the list returns to the title or just-played result that opened it.

## Coaching and grades

Press `A` to open the analysis screen. Its first tab is Coach: current English and Japanese grades, the latest adaptive prescription result, one to three concrete opportunities, and stable habit findings. Large terminals draw the overall grade as a coloured badge; compact terminals keep the grade letter and value in normal text. The report always names its source (official, interim, provisional, or provisional with difficulty uncorrected), keeps evidence counts, and states when evidence is insufficient. `T` opens goal candidates derived from recent records; `Enter` persists the selected goal and its remaining gap is shown in the first section.

The mode-selection heading carries a compact English/Japanese grade. Completing benchmark A or C shows that language's official or interim grade and any promotion or demotion on the result screen; ordinary practice does not show a grade change. After any completed session, the result screen may show the clearest session-specific observation. `[display].feedback = "summary"` limits this to one line, while `"detailed"` allows up to three; nothing is displayed when the session lacks usable evidence.

## Configuration

If `--config` is omitted, rs-typing-trainer reads the first existing file from `./config.toml` and `~/.config/rs-typing-trainer/config.toml`. If neither exists, it uses defaults. `--lang` takes precedence over `[general] lang`. The original `[game]`, `[characters]`, and `[metrics]` names remain accepted; an explicit v2 section takes precedence.

`item_timeout_seconds = 0` disables the per-item timer and its bar. Sentence mode always disables that timer; phrase mode follows the word-mode duration and timeout settings. An empty `mode.keys` value means no filtering in `words`, `romaji`, and `sentence`; phrase mode has no key picker. `random` and `ngram` ask for keys on the key-selection screen.

The settings screen edits layout, input layout, geometry, keyboard visibility (including `auto`), keyboard appearance, prompt text (large or normal), terminal graphics, language, error mode and result feedback. It preserves existing comments and unrelated TOML values. See the [configuration reference](configuration.md) for every key and default, [Training tools](training.md) for training behavior, and [Keyboard layouts and display](keyboard.md) for remapping, custom layouts and key-release support.

## If the terminal remains in a bad state

The application restores keyboard-protocol flags, drains pending input briefly, leaves the alternate screen, shows the cursor and then disables raw mode on normal exit, errors and panic unwinding. If the terminal still behaves incorrectly after a crash outside those recoverable paths, run `reset` and press `Enter` to restore the terminal state.
