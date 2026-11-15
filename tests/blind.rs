use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;
use rs_typing_trainer::config::{Config, ErrorMode};
use rs_typing_trainer::ui;

mod common;
use common::test_app;

fn key(character: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)
}

#[test]
fn blind_hides_typed_feedback_shows_item_result_and_forces_advance() {
    let mut config = Config::default();
    config.mode.keys = "ab".into();
    config.mode.random.word_length = 2;
    config.game.word_length = 2;
    config.session.blind = true;
    config.session.error_mode = ErrorMode::Stop;
    config.ui.animation_enabled = false;
    let mut app = test_app(config);
    let now = Instant::now();
    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    app.handle_key_at(enter, now);
    app.handle_key_at(enter, now);
    app.on_tick_at(Duration::from_secs(3), now).unwrap();
    app.on_tick_at(Duration::ZERO, now).unwrap();
    assert_eq!(app.game.as_ref().unwrap().session.log().error_mode, ErrorMode::Advance);

    let first = app.game.as_ref().unwrap().session.expected().unwrap().chars().next().unwrap();
    app.handle_key_at(key(first), now + Duration::from_millis(10));
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    let typed = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .find(|cell| cell.symbol() == first.to_string())
        .unwrap();
    assert_ne!(typed.style().fg, Some(Color::Green));
    let during = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect::<String>();
    assert!(!during.contains("Got '"), "{during}");
    assert!(during.contains("Blind: advance forced"), "{during}");

    let second = app.game.as_ref().unwrap().session.expected().unwrap().chars().next().unwrap();
    app.handle_key_at(key(second), now + Duration::from_millis(20));
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    let completed = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect::<String>();
    assert!(completed.contains("Correct"), "{completed}");
}
