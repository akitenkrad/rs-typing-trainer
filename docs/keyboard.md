**English** | [日本語](keyboard.ja.md)

# Keyboard layouts and display

## Bundled layouts

rs-typing-trainer includes seven layouts:

- QWERTY (US), selected as `qwerty`
- Dvorak (US), selected as `dvorak`
- Colemak, selected as `colemak`
- Colemak-DH without Angle Mod, selected as `colemak-dh`
- The official Colemak-DH ANSI Angle(Z) variant, selected as `colemak-dh-ansi`
- Workman, selected as `workman`
- JIS QWERTY without kana legends, selected as `jis-qwerty`

Set `keyboard.geometry` independently to `ansi` or `jis`. Geometry controls the physical rows, key widths and finger guide; the layout controls the character printed by each physical key. Physical keys use W3C `KeyboardEvent.code` names such as `KeyA`, `Semicolon`, `Digit1`, `IntlYen` and `IntlRo`. `Digit6` belongs to the left index finger by default; set `digit6_finger = "right-index"` to use the other common convention.

The legacy `colemak-dh` name retains the existing no-Angle-Mod bottom row `zxcdvkh,./`. The official ANSI variant follows the Colemak-DH [ANSI Angle(Z) placement](https://colemakmods.github.io/mod-dh/keyboards.html), whose bottom row is `xcdvzkh,./`: the left bottom-row letters move one key left and `z` moves to the centre key.

The ANSI board uses physical quarter-unit widths: Backspace 2u, Tab 1.5u, Caps Lock 1.75u, Enter 2.25u, left Shift 2.25u, right Shift 2.75u, and Space 6.25u. Every row totals 15u. The JIS board approximates its distinct Enter shape and includes `IntlYen`, `IntlRo`, Muhenkan, Henkan, and Kana keys.

Set `style` under `[keyboard]` to choose the preferred appearance:

- `keycap` (default) draws shared box borders and physical key widths. It uses 76 columns by 11 rows. A pressed key changes to a heavy border for the 150 ms feedback frame.
- `raised` draws coloured key faces and a `▀` shadow. It uses 60 columns by 10 rows. A pressed face moves down by one row and its shadow disappears.
- `plain` keeps the compact labels-only display. It uses 45 columns by 5 rows, plus one row when the finger guide is enabled.

The play, key-selection, and analysis screens use the same widget. When the available width or height is too small, it falls back only toward simpler appearances: `keycap` → `raised` → `plain` → hidden. The prompt and controls keep their space on the play screen.

All appearances highlight the next key and Shift when needed. A correct press flashes green for about 150 ms. A mistype flashes both the pressed key and the expected key red. Heatmaps, unlock state, `auto` fading, and the finger guide also apply to every appearance. In `keycap`, the lower border carries the finger colour; in `raised`, the shadow carries it.

Set `show = "dim"` to mute the labels or `show = "hidden"` to omit the keyboard. `show = "auto"` fades and then hides labels for keys whose shrinkage estimates reach 97% accuracy and the user's median IKI; the next key always remains visible. The initial default is `full`, and benchmarks force the fixed full display. `finger_guide = false` hides the coloured finger guide.

## Input-layout remapping

Normally `input_layout = "same"`: the character delivered by the operating system is judged unchanged. Set an explicit input layout when the operating-system layout and the layout being practised differ.

For example, with `input_layout = "qwerty"` and `layout = "dvorak"`, typing `d` is resolved to physical key `KeyD`; Dvorak assigns `e` to that key, so the trainer judges `e`. Shift state is retained, so `D` becomes `E`. A character that cannot be resolved in the input layout passes through unchanged.

## User-defined layouts

Place `*.toml` files in `~/.config/rs-typing-trainer/layouts/`. When `--config PATH` is used, layouts are read from a `layouts/` directory beside that file. The format is the same as the bundled definitions:

```toml
name = "my-layout"

[keys.KeyA]
normal = "a"
shift = "A"

[keys.Semicolon]
normal = ";"
shift = ":"
```

Each normal and shifted value is one Unicode scalar value. A character may not be assigned to two different physical keys. A duplicate reports both the file name and character and prevents the layout set from loading.

## Key-release events

The application asks crossterm whether the terminal supports keyboard enhancement and enables `REPORT_EVENT_TYPES` only when supported. This is the kitty keyboard protocol feature implemented by terminals such as kitty, WezTerm, Ghostty and foot. Support is detected at runtime rather than inferred from the terminal name.

When available, a Release event fills `release_us` on the preceding press of the same inferred physical key, and `sessions.release_events` is true. Release and Repeat events never type a character or operate a screen. On a terminal without the protocol, the game works from Press events, leaves `release_us` empty and records `sessions.release_events` as false.

The enhancement flags are popped during normal shutdown and by the panic cleanup hook before the terminal is restored.
