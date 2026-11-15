**English** | [日本語](gameplay.ja.md)

# Playing rs-typing-trainer

## Starting a session

Run `rs-typing-trainer` or `cargo run --release` to open the title screen. The default interface language is English; use `--lang ja` for Japanese. An explicit `--config PATH` takes the place of the normal configuration search.

Running the program without a subcommand is the same as `rs-typing-trainer play`. `replay FILE` plays a portable record, `stats` prints saved-session analysis, and `export SESSION_ID [-o FILE]` writes a v2 session as `.ttrec`, using standard output when `-o` is omitted. Replay reads only its record file; see [Replay](replay.md).

## Controls

| Screen | Keys | Action |
|---|---|---|
| Title | `↑` / `↓` (`k` / `j`) | Move the menu selection |
| Title | `Enter` | Run the selected menu item |
| Title | `Space` | Open mode selection directly |
| Title | `H` | Open history |
| Title | `B` | Open `bench-v1` selection |
| Title | `A` | Open analysis |
| Title | `S` | Open settings |
| Title | `Q` / `Esc` | Quit |
| Mode selection | `↑` / `↓` (`k` / `j`) | Select any practice mode |
| Mode selection | `P` | Toggle the pacer on supported modes |
| Mode selection | `L` | Switch adaptive-practice language |
| Mode selection | `[` / `]` | Decrease/increase preview items |
| Mode selection | `Z` | Switch the prompt text between large and normal (saved when the mode starts) |
| Mode selection | `Enter` | Continue to key selection or the countdown |
| Benchmark selection | `↑` / `↓` (`k` / `j`) | Select Start or optional Part D |
| Benchmark selection | `Enter` | Run the selected action |
| Benchmark selection | `Space` | Start the benchmark directly |
| Benchmark selection | `D` | Toggle optional Part D directly |
| Benchmark selection | `Esc` / `Q` | Return to the title screen |
| Key selection | Arrow keys | Move on the keyboard |
| Key selection | `Space` | Toggle the current key |
| Key selection | `Tab` | Switch `include` / `only` |
| Key selection | `Enter` | Save the selection and start the countdown |
| Play | Printable keys | Type the word |
| Play | `Backspace` | Move back in `advance` error mode |
| Play | `Esc` | Abort to the title screen |
| Result | `Space` / `Enter` | Start another session |
| Result | `H` | Open history |
| Result | `Q` / `Esc` | Quit |
| History | `↑` / `↓` (`k` / `j`) | Move the selection |
| History | `PgUp` / `PgDn` / `Home` / `End` | Jump through the list |
| History | `S` | Cycle the sort order |
| History | `Enter` | View a session in detail |
| History | `Esc` / `Q` | Return to the title screen |
| History detail | `Space` / `Enter` / `Esc` | Return to history |
| History detail | `Q` | Quit |
| Settings | `↑` / `↓` (`k` / `j`) | Select a setting |
| Settings | `←` / `→` (`h` / `l`) | Change the selected value |
| Settings | `Enter` | Save and return to the title |
| Settings | `Esc` / `Q` | Cancel and return to the title |

While a word is sliding in or out, for about 200 ms on each side, keystrokes are dropped. This prevents the key that completed one word from being credited to the next word.

## Playing-screen layout

The playing screen shows, from top to bottom: the header (time, WPM, item count and pace), the keyboard, the item timer bar, the combo/mistype/accuracy row, the prompt, and the preview and controls on the bottom row. The prompt starts immediately below the combo row instead of being vertically centred in the area below the keyboard, and remains centred horizontally.

`display.prompt_style` chooses how the prompt is drawn: `large` (default) or `normal`. The settings screen calls it "Prompt text" with the values "Large text" and "Normal text", and `Z` on the mode selection screen toggles it.

**Large text** draws the item being typed with the bundled Shinonome bitmap font. Four font dots become one terminal quadrant-block cell, so a 12-dot fullwidth character occupies 6×6 cells and a 16-dot character occupies 8×8 cells. One blank row separates each large block from the normal-size rows around it, and labels sit in a column to the left of the block they name, on its top row:

- English item modes: the large item and its `▔` cursor, then a blank row and a helper row (locking, mistype details, mistypes in this word, or the result) when there is room.
- Romaji: the large written form, a blank row, the kana (bold, accent colour, the kana being typed reversed), a blank row, then the large romanization and its cursor. The labels (Surface / Kana / Romaji) line up in the left column and the blocks start at the same column.
- Sentence and endurance: a large window around the current position and its cursor, a blank row, then the full sentence at normal size with the current position reversed. Japanese sentences show the romaji layout above, then a blank row and the surface / kana / romaji rows of the whole sentence.

When the area is short, blank rows are given up first; glyphs are never cut. Pacer and ghost markers are placed above the corresponding large character. Blind mode leaves completed large glyphs blank. Large prompts keep one blank column inside the frame on each side. They need a terminal font that contains the quadrant block elements (U+2596–U+259F and U+2580–U+2590); if the terminal substitutes a fallback font whose glyphs are wider than one cell, the large rows can look wider than the frame, and `display.prompt_style = "normal"` avoids them. A character outside the bundled font (outside JIS X 0208 and ASCII/halfwidth kana) is drawn at normal size in the middle of its large cell.

Large text picks its size from the terminal: 16-dot with the selected keyboard style, 12-dot with that style, 12-dot with a simpler keyboard, 12-dot with no keyboard, and finally normal text with whatever keyboard still fits. The prompt and controls are never removed. `keyboard.show = "hidden"` omits and collapses the keyboard area at every height.

**Normal text** draws the prompt at terminal size inside a rounded card (`╭╮╰╯`) with two columns and one row of padding, centred below the combo row. Text still to type is bright and bold, the current character is reversed (on a red background while a mistype flashes), and typed text uses a calm colour. Item modes (words, random, n-gram, adaptive, unlock, rhythm, burst) put one blank cell between characters, with the `▲` caret directly below the current character. Romaji uses two columns — muted labels on the left, text on the right — for the written form (bold), kana and romaji, with a blank row between them; the current kana and romaji character are reversed. Sentences wrap inside the card; the row being typed is bright, the other rows are slightly dimmed, and the caret row follows the current row. Japanese sentences use the three-row layout with each row wrapped. Pacer and ghost markers sit on the row directly above the characters. Helper messages appear under the card. Normal text keeps the card in preference to the keyboard: the keyboard is simplified or hidden first, and on a very small terminal the card gives up its blank rows, padding and border before any text.

## Session history

Every completed session is stored in a local SQLite database. By default the file is `~/.local/share/rs-typing-trainer/sessions.db`; set `storage.db_path` in the configuration to use another location. The old `metrics.db_path` name remains accepted, with `storage.db_path` taking precedence when both are present.

When the new default file does not exist but `~/.cache/rs-typing-trainer/sessions.db` does, the program copies the old database to the new location and leaves the old file untouched. The title screen reports that copy once. No automatic copy is attempted when a database path is configured or when both default files already exist.

Press `H` on the title screen to open history. Press `S` to cycle through newest date, oldest date, WPM, accuracy, maximum combo and completed-word sort orders. Score-based orders show a rank. `Enter` opens the original result, including the mistype heatmap and reaction-time distribution.

## Configuration

If `--config` is omitted, rs-typing-trainer reads the first existing file from `./config.toml` and `~/.config/rs-typing-trainer/config.toml`. If neither exists, it uses defaults. `--lang` takes precedence over `[general] lang`. The original `[game]`, `[characters]`, and `[metrics]` names remain accepted; an explicit v2 section takes precedence.

`item_timeout_seconds = 0` disables the per-item timer and its bar. Sentence mode always disables that timer. An empty `mode.keys` value means no filtering in `words`, `romaji`, and `sentence`; `random` and `ngram` ask for keys on the key-selection screen.

The settings screen edits layout, input layout, geometry, keyboard visibility (including `auto`), keyboard appearance, prompt text (large or normal), language and error mode. It preserves existing comments and unrelated TOML values. See the [configuration reference](configuration.md) for every key and default, [Training tools](training.md) for training behavior, and [Keyboard layouts and display](keyboard.md) for remapping, custom layouts and key-release support.

## If the terminal remains in a bad state

The application restores keyboard-protocol flags, drains pending input briefly, leaves the alternate screen, shows the cursor and then disables raw mode on normal exit, errors and panic unwinding. If the terminal still behaves incorrectly after a crash outside those recoverable paths, run `reset` and press `Enter` to restore the terminal state.
