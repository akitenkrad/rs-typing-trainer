use std::time::Instant;

use chrono::NaiveDate;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::app::Screen;
use rs_typing_trainer::config::Config;
use rs_typing_trainer::ui;

use crate::common::test_app_at;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn render(app: &rs_typing_trainer::app::App, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect()
}

#[test]
fn briefing_renders_at_small_and_large_sizes_with_the_hint_on_the_last_inner_row() {
    let mut app = test_app_at(Config::default(), NaiveDate::from_ymd_opt(2026, 11, 18).unwrap());
    app.handle_key_at(key(KeyCode::Char('m')), Instant::now());
    app.handle_key_at(key(KeyCode::Enter), Instant::now());
    assert_eq!(app.screen, Screen::Briefing);

    for (width, height) in [(80, 24), (120, 40)] {
        let rows = render(&app, width, height);
        let text = rows.join("\n");
        assert!(text.contains("TRAINING BRIEFING"), "{width}x{height}:\n{text}");
        assert!(text.contains("Purpose"), "{width}x{height}:\n{text}");
        assert!(text.contains("[Enter] Start"), "{width}x{height}:\n{text}");
        assert!(
            !text.contains("│Purpose"),
            "briefing body must use the standard left inset: {width}x{height}:\n{text}"
        );
        assert!(rows[usize::from(height - 2)].contains("[Enter] Start"), "{width}x{height}:\n{text}");
    }
}
