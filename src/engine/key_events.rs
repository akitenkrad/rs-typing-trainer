//! Deterministic translation of terminal key event kinds into session changes.

use std::time::Duration;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

use crate::engine::session::{InputEffect, Session};
use crate::layout::{KeyboardLayout, remap_char};

pub fn handle_key_event(
    session: &mut Session,
    now: Duration,
    event: KeyEvent,
    practice_layout: &KeyboardLayout,
    input_layout: Option<&KeyboardLayout>,
) -> InputEffect {
    match (event.kind, event.code) {
        (KeyEventKind::Press, KeyCode::Char(character)) => {
            let stroke = input_layout.unwrap_or(practice_layout).stroke_for(character);
            let remapped = remap_char(character, input_layout, practice_layout);
            session.input_char_with_code(now, &remapped.to_string(), stroke.map(|value| value.code))
        }
        (KeyEventKind::Press, KeyCode::Backspace) => session.input_backspace_with_code(now, Some("Backspace".to_owned())),
        (KeyEventKind::Release, KeyCode::Char(character)) => {
            if let Some(stroke) = input_layout.unwrap_or(practice_layout).stroke_for(character) {
                session.release_key(now, &stroke.code);
            }
            InputEffect::Dropped
        }
        (KeyEventKind::Release, KeyCode::Backspace) => {
            session.release_key(now, "Backspace");
            InputEffect::Dropped
        }
        _ => InputEffect::Dropped,
    }
}
