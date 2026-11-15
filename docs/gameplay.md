**English** | [日本語](gameplay.ja.md)

# Playing rs-typing-trainer

## Starting a session

Run `rs-typing-trainer` or `cargo run --release` to open the title screen. The default interface language is English; use `--lang ja` for Japanese. An explicit `--config PATH` takes the place of the normal configuration search.

Running the program without a subcommand is the same as `rs-typing-trainer play`. `replay FILE` plays a portable record, `stats` prints saved-session analysis, and `export SESSION_ID [-o FILE]` writes a v2 session as `.ttrec`, using standard output when `-o` is omitted. Replay reads only its record file; see [Replay](replay.md).

## Controls

| Screen | Keys | Action |
|---|---|---|
| Title | `Space` / `Enter` | Open mode selection |
| Title | `H` | Open history |
| Title | `B` | Run `bench-v1` |
| Title | `A` | Open analysis |
| Title | `S` | Open settings |
| Title | `Q` / `Esc` | Quit |
| Mode selection | `↑` / `↓` | Select any practice mode |
| Mode selection | `P` | Toggle the pacer on supported modes |
| Mode selection | `L` | Switch adaptive-practice language |
| Mode selection | `[` / `]` | Decrease/increase preview items |
| Mode selection | `Enter` | Continue to key selection or the countdown |
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

## Session history

Every completed session is stored in a local SQLite database. By default the file is `~/.local/share/rs-typing-trainer/sessions.db`; set `storage.db_path` in the configuration to use another location. The old `metrics.db_path` name remains accepted, with `storage.db_path` taking precedence when both are present.

When the new default file does not exist but `~/.cache/rs-typing-trainer/sessions.db` does, the program copies the old database to the new location and leaves the old file untouched. The title screen reports that copy once. No automatic copy is attempted when a database path is configured or when both default files already exist.

Press `H` on the title screen to open history. Press `S` to cycle through newest date, oldest date, WPM, accuracy, maximum combo and completed-word sort orders. Score-based orders show a rank. `Enter` opens the original result, including the mistype heatmap and reaction-time distribution.

## Configuration

If `--config` is omitted, rs-typing-trainer reads the first existing file from `./config.toml` and `~/.config/rs-typing-trainer/config.toml`. If neither exists, it uses defaults. `--lang` takes precedence over `[general] lang`. The original `[game]`, `[characters]`, and `[metrics]` names remain accepted; an explicit v2 section takes precedence.

`item_timeout_seconds = 0` disables the per-item timer and its bar. Sentence mode always disables that timer. An empty `mode.keys` value means no filtering in `words`, `romaji`, and `sentence`; `random` and `ngram` ask for keys on the key-selection screen.

The settings screen edits layout, input layout, geometry, keyboard visibility (including `auto`), language and error mode. It preserves existing comments and unrelated TOML values. See the [configuration reference](configuration.md) for every key and default, [Training tools](training.md) for training behavior, and [Keyboard layouts and display](keyboard.md) for remapping, custom layouts and key-release support.
