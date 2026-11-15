//! Translate raw key events into game actions. Slide-in/out drop logic lives
//! here so the rest of the game state remains free of UI-timing concerns.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputAction {
    /// A printable grapheme was typed.
    Char(String),
    /// Backspace is logged even though stop mode does not move the cursor.
    Backspace,
    /// User pressed Esc / Ctrl-C — abort the current game.
    Abort,
    /// Some control key we don't care about.
    Ignored,
}

/// Convert a `KeyEvent` into an `InputAction`.
pub fn classify(key: KeyEvent) -> InputAction {
    if key.kind != KeyEventKind::Press {
        return InputAction::Ignored;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C')) {
        return InputAction::Abort;
    }
    match key.code {
        KeyCode::Esc => InputAction::Abort,
        KeyCode::Char(c) => InputAction::Char(c.to_string()),
        KeyCode::Backspace => InputAction::Backspace,
        _ => InputAction::Ignored,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyEvent, KeyEventKind, KeyEventState};

    fn k(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }
    }

    #[test]
    fn char_keys_become_actions() {
        assert_eq!(classify(k(KeyCode::Char('a'))), InputAction::Char("a".into()));
    }

    #[test]
    fn esc_is_abort() {
        assert_eq!(classify(k(KeyCode::Esc)), InputAction::Abort);
    }

    #[test]
    fn other_keys_ignored() {
        assert_eq!(classify(k(KeyCode::F(5))), InputAction::Ignored);
    }

    #[test]
    fn backspace_becomes_an_action() {
        assert_eq!(classify(k(KeyCode::Backspace)), InputAction::Backspace);
    }
}
