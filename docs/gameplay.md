**English** | [日本語](gameplay.ja.md)

# Playing rs-typing-trainer

## Starting a session

Run `rs-typing-trainer` or `cargo run --release` to open the title screen. The default interface language is English; use `--lang ja` for Japanese. An explicit `--config PATH` takes the place of the normal configuration search.

Running the program without a subcommand is the same as `rs-typing-trainer play`. `replay FILE` plays a portable record, `stats` prints saved-session analysis, `graphics` reports terminal-image support, and `export SESSION_ID [-o FILE]` writes a v2 session as `.ttrec`, using standard output when `-o` is omitted. Replay reads only its record file; see [Replay](replay.md).

## Terminal graphics check

`rs-typing-trainer graphics` prints the selected protocol, cell pixel size, reason, `TERM`, `TERM_PROGRAM`, and the presence of tmux, screen, and SSH. It follows `--lang`, reads `display.graphics`, and accepts the global `--graphics auto|kitty|sixel|iterm2|off` override.

`rs-typing-trainer graphics --demo [--seconds N]` opens the alternate screen and shows a roughly 40×12-cell gradient, one-pixel grid, and corner marks with the selected protocol. It closes after three seconds by default or on a key press. If images are unavailable, it shows the reason as text. This diagnostic does not change the normal game rendering yet; see [Terminal graphics](graphics.md).

## Controls

| Screen | Keys | Action |
|---|---|---|
| Title | `↑` / `↓` (`k` / `j`) | Move the menu selection |
| Title | `Enter` | Run the selected menu item |
| Title | `Space` | Open mode selection directly |
| Title | `H` | Open history |
| Title | `B` | Open `bench-v2` selection |
| Title | `A` | Open analysis |
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
