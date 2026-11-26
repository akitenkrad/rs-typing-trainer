use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::config::{Config, ModeKind};
use rs_typing_trainer::ui;

use crate::common;
use common::test_app;

fn start(config: Config) -> common::TestApp {
    let mut app = test_app(config);
    let now = Instant::now();
    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    app.handle_key_at(enter, now);
    app.handle_key_at(enter, now);
    common::accept_briefing(&mut app, now);
    app.on_tick_at(Duration::from_secs(3), now).unwrap();
    app.on_tick_at(Duration::ZERO, now).unwrap();
    app
}

fn render(app: &rs_typing_trainer::app::App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(100, 26)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect::<String>()
}

#[test]
fn preview_zero_through_three_controls_visible_items_and_mode_screen_keys() {
    for count in 0..=3 {
        let mut config = Config::default();
        config.session.preview_items = count;
        config.ui.animation_enabled = false;
        let app = start(config);
        let output = render(&app);
        assert_eq!(output.contains("Next:"), count > 0, "count={count}: {output}");
    }
    let mut app = test_app(Config::default());
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE));
    assert_eq!(app.config.session.preview_items, 2);
    app.handle_key(KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE));
    assert_eq!(app.config.session.preview_items, 1);
}

#[test]
fn sentence_hide_typed_removes_the_completed_prefix() {
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("sentences.tsv");
    std::fs::write(&path, "abcde\n").unwrap();
    let mut config = Config::default();
    config.mode.kind = ModeKind::Sentence;
    config.mode.sentence.hide_typed = true;
    config.session.preview_items = 0;
    config.data.sentences_en = Some(path);
    config.ui.animation_enabled = false;
    let mut app = start(config);
    app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    let output = render(&app);
    assert!(output.contains("bcde"), "{output}");
    assert!(!output.contains("abcde"), "{output}");
}
