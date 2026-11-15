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
