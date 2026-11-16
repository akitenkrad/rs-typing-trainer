**English** | [日本語](graphics.ja.md)

# Terminal graphics

The application detects terminal-image support at startup and retains the selected protocol and cell pixel size. Use `rs-typing-trainer graphics` for a text report and `rs-typing-trainer graphics --demo` for the temporary test image.

## Protocols

| Protocol | Characteristics |
|---|---|
| kitty graphics | Transmit a numbered image once, then place it by reference. Supports layering and efficient updates. |
| Sixel | Transmit the complete image for each update. Roughly 256 colours; erase by drawing over it. |
| iTerm2 (OSC 1337) | Transmit a base64-encoded PNG for each update. |

## Terminal support

| Terminal | Preferred protocol | Verification |
|---|---|---|
| Ghostty, kitty | kitty | Ghostty verified on 2026-11-15 |
| WezTerm | kitty (Sixel and iTerm2 also available) | Can exercise all three paths |
| iTerm2 | iTerm2 | Needs verification |
| foot, Windows Terminal 1.22+ | Sixel | Needs verification |
| VS Code terminal | iTerm2 or Sixel when enabled | Needs verification |
| macOS Terminal, Alacritty | none → block/text rendering | — |

This table records the current design assumptions. Entries marked “Needs verification” remain unverified.

VHS's xterm.js reports Sixel support, but VHS canvas recordings omit the image layer; use `--graphics off` in tapes.

## Detection rules

`auto` queries through `ratatui-image` once. Kitty has priority over iTerm2, then Sixel; halfblocks mean no image protocol. Non-terminal standard input/output is always `off`. `auto` is `off` inside tmux or screen because passthrough cannot be verified; an explicit mode may be used there. An explicit mode uses the queried cell pixel size, then falls back to terminal window pixels divided by columns/rows, and becomes `off` if neither is available.

On resize, the application updates cell pixels from the window dimensions without sending another capability query. Kitty images are deleted on exit before keyboard-protocol cleanup and before leaving the alternate screen.

## Image keycaps

When the resolved keyboard style is `keycap` and an image protocol is available, each key is drawn as a separate pixel-sized tile. Tiles use rounded faces, shadows, press motion, Shinonome bitmap labels, and an optional finger-colour strip. The existing keyboard state still determines all colours and labels, including next-key feedback, errors, heatmaps, unlock state, and fading.

Tiles are cached by their complete appearance and terminal geometry. Kitty transmits each cached tile once and subsequently places it by reference; Sixel and iTerm2 rely on terminal-buffer differences so unchanged cells are not emitted again. A resize or protocol change clears the cache. The `raised` and `plain` styles, unsupported terminals, tmux/screen in `auto`, and non-terminal output continue to use the existing text renderer.

## Large prompt characters

With `display.prompt_style = "large"`, an available image protocol also draws each large prompt character as one transparent tile. `display.big_font` selects the bundled BIZ UD Gothic outlines, an integer-scaled Shinonome bitmap, or face 0 of a user TTF/OTF/TTC file. The tile keeps the existing character columns, styles, visibility, cursor, and marker rows; only the glyph block becomes an image. Its height is the number of terminal rows needed for a square fullwidth box, so a Dot16 glyph at 9×20 pixels per cell uses four rows instead of the text renderer's eight.

Missing user-font glyphs fall back character by character to BIZ UD Gothic, then Shinonome, then the existing text fallback. Tiles share the keycap protocol cache policy: a protocol or cell-pixel change clears all entries, and reaching the fixed limit clears the cache. The visible window is prepared before placement, so an ordinary keystroke creates only tiles whose colour changed. Without an image protocol, large prompts retain the previous Shinonome block rendering exactly.

## Analysis graphics

When an image protocol is available, the Coach tab draws a G-to-S ladder for each language and places the continuous score above it. A shared radar renderer draws the component-grade view below the ladders. The Bench tab uses that renderer for the eight historical-percentile axes and adds one WPM sparkline per measured part. Radar labels, grade letters and scale labels use the resolved `display.big_font`; language names and the raw coaching and benchmark rows remain terminal text. Short Coach areas omit the radar, and opening the goal picker keeps the existing text-only view.

The Trend tab draws the net-WPM, accuracy and IKI plot areas as antialiased pixel images with calendar-proportional spacing, while titles, legends and axis labels remain terminal text. The net-WPM plot also shows a language-coloured 28-day forecast as a dashed line and a translucent fan from the latest daily speed to the predicted interval.

The Transitions tab composites the keycaps and the selected pair's arrow or same-key loop into one image. Its slow and error-prone columns use horizontal bars with a neutral body and an error-rate colour tag at the right edge, while pair labels, values, counts and insufficient-data rows stay as terminal text. The Fingers/Hands tab adds a symmetric cached finger chart with upward IKI and downward error bars, plus a four-row hand/fingering comparison; its axes, finger labels and original numeric rows remain terminal text. These figures shrink with the available area and are omitted before the numeric rows would be displaced.

Analysis images are cached by their complete visible input, font, terminal-cell geometry and protocol. The transition-keyboard key includes the selected pair, so its combined image is rebuilt only when that selection changes. Without an image protocol, the previous block-letter grade view, transition and finger rows, benchmark rows and braille `Chart` widgets remain unchanged; the forecast fan is available only in the image chart.
