use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::app::Screen;
use rs_typing_trainer::config::Config;
use rs_typing_trainer::metrics::stats::{GameResult, LiveMetrics};
use rs_typing_trainer::ui;

use crate::common::test_app;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn result_app() -> crate::common::TestApp {
    let mut config = Config::default();
    config.session.result_input_delay_ms = 0;
    let result = GameResult::from_metrics(&config, LiveMetrics::default(), Duration::from_secs(1));
    let mut app = test_app(config);
    app.screen = Screen::Result;
    app.last_result = Some(result);
    app
}

fn render(app: &rs_typing_trainer::app::App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn d_opens_review_and_d_or_escape_returns_to_result() {
    let now = Instant::now();
    let mut app = result_app();
    app.handle_key_at(key(KeyCode::Char('d')), now);
    assert_eq!(app.screen, Screen::Review);
    app.handle_key_at(key(KeyCode::Char('D')), now);
    assert_eq!(app.screen, Screen::Result);

    app.handle_key_at(key(KeyCode::Char('d')), now);
    app.handle_key_at(key(KeyCode::Esc), now);
    assert_eq!(app.screen, Screen::Result);
}

#[test]
fn review_scroll_stops_at_the_last_body_line_for_down_and_page_down() {
    let now = Instant::now();
    let mut app = result_app();
    app.handle_key_at(key(KeyCode::Char('d')), now);
    let before = render(&app, 80, 24);
    assert!(before.contains("Review data is unavailable."), "{before}");

    for _ in 0..30 {
        app.handle_key_at(key(KeyCode::Down), now);
    }
    let after_down = render(&app, 80, 24);
    assert!(after_down.contains("Review data is unavailable."), "{after_down}");
    let bottom = app.review_scroll;

    app.handle_key_at(key(KeyCode::PageDown), now);
    let after_page_down = render(&app, 80, 24);
    assert_eq!(app.review_scroll, bottom);
    assert!(after_page_down.contains("Review data is unavailable."), "{after_page_down}");
}

#[test]
fn enter_on_review_uses_the_result_restart_action() {
    let now = Instant::now();
    let mut app = result_app();
    app.handle_key_at(key(KeyCode::Char('d')), now);
    app.handle_key_at(key(KeyCode::Enter), now);
    assert!(matches!(app.screen, Screen::Countdown { .. }));
}

#[test]
fn benchmark_result_does_not_open_review() {
    let now = Instant::now();
    let mut app = result_app();
    app.active_bench_id = Some("bench-v2/A/01".to_owned());
    app.handle_key_at(key(KeyCode::Char('d')), now);
    assert_eq!(app.screen, Screen::Result);
}
