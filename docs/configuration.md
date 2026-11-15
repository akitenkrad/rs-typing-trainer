**English** | [日本語](configuration.ja.md)

# Configuration reference

Without `--config`, rs-typing-trainer reads the first existing file from `./config.toml` and `~/.config/rs-typing-trainer/config.toml`. If neither exists, the defaults below are used. Paths beginning with `~/` are expanded relative to the user's home directory. Command-line `--lang` overrides `general.lang`.

Every supported key is listed here. “Unset” means that the application derives a value or uses bundled/default storage instead of reading a path. The `[game]`, `[characters]`, and `[metrics]` sections are v1 compatibility keys; use their v2 replacements for new files.

| Key | Type | Default | Description |
|---|---|---|---|
| `general.lang` | `en` / `ja` | `"en"` | Interface language. |
| `keyboard.layout` | string | `"qwerty"` | Practice layout; bundled or user-defined name. |
| `keyboard.input_layout` | string | `"same"` | OS input layout used for physical-key remapping. |
| `keyboard.geometry` | `ansi` / `jis` | `"ansi"` | Physical keyboard geometry. |
| `keyboard.show` | `full` / `dim` / `hidden` / `auto` | `"full"` | On-screen keyboard visibility and fading. |
| `keyboard.heatmap` | `off` / `errors` / `latency` | `"off"` | Default analysis heatmap. |
| `keyboard.finger_guide` | boolean | `true` | Show the coloured finger guide. |
| `keyboard.digit6_finger` | `left-index` / `right-index` | `"left-index"` | Finger assigned to the `6` key. |
| `session.duration_seconds` | integer | `60` | Normal session duration. |
| `session.item_timeout_seconds` | integer | `5` | Per-item limit; zero disables it. |
| `session.error_mode` | `stop` / `advance` | `"stop"` | Whether a mistype blocks progress or advances the cursor. |
| `session.preview_items` | integer | `1` | Look-ahead items, from 0 through 3. |
| `session.blind` | boolean | `false` | Hide typed progress until an item ends. |
| `session.retry_on_error` | boolean | `false` | Repeat a mistyped item once. |
| `mode.kind` | mode name | `"random"` | Initial practice mode. |
| `mode.keys` | string | `"bmwvz"` | Selected characters for filtering or generation. |
| `mode.key_filter` | `include` / `only` | `"include"` | Prompt-filter interpretation. |
| `mode.base_keys` | string | `"home"` | Extra keys for `only`: `none`, `home`, or literal characters. |
| `mode.random.word_length` | integer | `3` | Pseudo-word length. |
| `mode.random.allow_repeat` | boolean | `true` | Permit a character to repeat within a pseudo-word. |
| `mode.random.allow_consecutive_same_word` | boolean | `false` | Permit the same pseudo-word twice in succession. |
| `mode.ngram.n` | integer | `2` | N-gram length. |
| `mode.ngram.source` | `pairs` / `corpus` | `"pairs"` | Cartesian pairs or corpus n-grams. |
| `mode.romaji.accept_apostrophe` | boolean | `true` | Accept `n'` for ん. |
| `mode.romaji.accept_xn` | boolean | `true` | Accept `xn` for ん. |
| `mode.sentence.lang` | `en` / `ja` | `"en"` | Sentence language. |
| `mode.sentence.hide_typed` | boolean | `false` | Hide the completed sentence prefix. |
| `data.words_en` | path | `—` | Replacement English-word TSV; unset uses bundled data. |
| `data.words_ja` | path | `—` | Replacement Japanese-word TSV. |
| `data.sentences_en` | path | `—` | Replacement English-sentence TSV. |
| `data.sentences_ja` | path | `—` | Replacement Japanese-sentence TSV. |
| `practice.daily_rotation` | boolean | `true` | Use deterministic daily decks. |
| `practice.core_ratio` | number | `0.65` | High-frequency share of a daily deck. |
| `practice.weekly_theme` | boolean | `true` | Enable weekday themes. |
| `analysis.pause_ms` | integer | `2000` | IKI gap treated as a pause. |
| `analysis.shrinkage_k` | number | `10.0` | Transition/key shrinkage strength. |
| `adaptive.error_weight` | number | `—` | Mistype cost in milliseconds; unset derives it from IKI. |
| `adaptive.drill_ratio` | number | `0.3` | Share of generated drills. |
| `adaptive.lang` | `en` / `ja` | `"en"` | Adaptive-practice language. |
| `adaptive.max_share` | number | `0.4` | Maximum generated share for one focus transition. |
| `pacer.enabled` | boolean | `false` | Enable the pacer initially. |
| `pacer.ratio` | number | `1.07` | Multiplier applied to a benchmark or recent median. |
| `pacer.min_wpm` | number | `20.0` | Lower bound and no-history target. |
| `unlock.order` | `frequency` / `rows` | `"frequency"` | Order in which keys unlock. |
| `unlock.target_wpm` | number | `35.0` | Speed required by the unlock gate. |
| `gate.min_accuracy` | number | `0.0` | Accuracy gate; zero disables it. |
| `rhythm.bpm` | number | `—` | Metronome tempo; unset derives it from IKI. |
| `rhythm.bell` | boolean | `false` | Emit a terminal bell on beats. |
| `burst.words` | integer | `12` | Words per sprint, from 10 through 15. |
| `endurance.minutes` | integer | `4` | Endurance duration, from 3 through 5 minutes. |
| `ui.animation_enabled` | boolean | `true` | Enable item slide animations. |
| `ui.animation_speed` | number | `1.0` | Animation speed multiplier. |
| `ui.theme` | `default` / `mono` / `neon` | `"default"` | Colour theme. |
| `ui.show_next_preview` | boolean | `true` | Show the next-item preview. |
| `storage.db_path` | path | `—` | Session database override. |
| `game.duration_seconds` | integer | `60` | v1 alias for `session.duration_seconds`. |
| `game.word_timeout_seconds` | integer | `5` | v1 alias for `session.item_timeout_seconds`. |
| `game.word_length` | integer | `3` | v1 alias for `mode.random.word_length`. |
| `characters.charset` | string | `"abcdefghijklmnopqrstuvwxyz"` | v1 alias used to populate `mode.keys`. |
| `characters.allow_repeat` | boolean | `true` | v1 alias for `mode.random.allow_repeat`. |
| `characters.allow_consecutive_same_word` | boolean | `false` | v1 alias for the random-mode setting. |
| `metrics.db_path` | path | `—` | v1 database-path alias; `storage.db_path` wins. |

The settings screen edits layout, input layout, geometry, keyboard visibility, language, and error mode while preserving comments and unrelated TOML keys. See [Training modes](modes.md), [Training tools](training.md), and [Keyboard layouts](keyboard.md) for behavior behind these values.
