use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use rs_typing_trainer::app::{ResultOrigin, Screen};
use rs_typing_trainer::config::Config;
use rs_typing_trainer::ui;

mod common;
use common::{TestApp, test_app};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn render(app: &rs_typing_trainer::app::App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<Vec<_>>()
        .join("")
}

fn phrase_color(app: &rs_typing_trainer::app::App, phrase: &str) -> Option<ratatui::style::Color> {
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let suffix = (x..buffer.area.width).map(|column| buffer[(column, y)].symbol()).collect::<String>();
            if suffix.starts_with(phrase) {
                return buffer[(x, y)].style().fg;
            }
        }
    }
    None
}

fn start_practice(config: Config, started: Instant) -> TestApp {
    let mut app = test_app(config);
    app.handle_key_at(key(KeyCode::Enter), started);
    app.handle_key_at(key(KeyCode::Enter), started);
    assert!(matches!(app.screen, Screen::Countdown { .. }));
    app.on_tick_at(Duration::from_secs(3), started).unwrap();
    assert_eq!(app.screen, Screen::Playing);
    app
}

fn finish_practice(mut config: Config, started: Instant) -> TestApp {
    config.session.duration_seconds = 1;
    let mut app = start_practice(config, started);
    app.on_tick_at(Duration::from_secs(1), started + Duration::from_secs(1)).unwrap();
    assert_eq!(app.screen, Screen::Result);
    app
}

#[test]
fn just_played_result_ignores_all_actions_until_delay_expires() {
    let started = Instant::now();
    for code in [KeyCode::Char('h'), KeyCode::Char('q'), KeyCode::Esc, KeyCode::Char(' ')] {
        let mut app = finish_practice(Config::default(), started);
        app.handle_key_at(key(code), started + Duration::from_secs(1));
        assert_eq!(app.screen, Screen::Result, "{code:?}");
        assert!(!app.should_quit, "{code:?}");
    }

    let delayed_actions = [
        (KeyCode::Char('h'), Screen::History, false),
        (KeyCode::Char('q'), Screen::Result, true),
        (KeyCode::Esc, Screen::Result, true),
        (KeyCode::Char(' '), Screen::Countdown { remaining: 3.0 }, false),
    ];
    for (code, expected_screen, should_quit) in delayed_actions {
        let mut app = finish_practice(Config::default(), started);
        app.handle_key_at(key(code), started + Duration::from_millis(2_500));
        assert_eq!(app.screen, expected_screen, "{code:?}");
        assert_eq!(app.should_quit, should_quit, "{code:?}");
    }
}

#[test]
fn result_delay_ignores_repeat_events_and_mutes_the_hint_until_ready() {
    let started = Instant::now();
    let mut app = finish_practice(Config::default(), started);
    let repeat = KeyEvent::new_with_kind(KeyCode::Char('h'), KeyModifiers::NONE, KeyEventKind::Repeat);
    app.handle_key_at(repeat, started + Duration::from_millis(1_100));
    assert_eq!(app.screen, Screen::Result);
    assert_eq!(
        phrase_color(&app, "[SPACE/ENTER]"),
        Some(rs_typing_trainer::ui::theme::Theme::default().muted)
    );

    app.on_tick_at(Duration::ZERO, started + Duration::from_millis(2_500)).unwrap();
    assert_eq!(
        phrase_color(&app, "[SPACE/ENTER]"),
        Some(rs_typing_trainer::ui::theme::Theme::default().accent)
    );
}

#[test]
fn zero_result_delay_takes_effect_immediately_and_history_detail_has_no_delay() {
    let started = Instant::now();
    let config = Config::from_toml("[session]\nresult_input_delay_ms = 0\n").unwrap();
    let immediate_actions = [
        (KeyCode::Char('h'), Screen::History, false),
        (KeyCode::Char('q'), Screen::Result, true),
        (KeyCode::Esc, Screen::Result, true),
        (KeyCode::Char(' '), Screen::Countdown { remaining: 3.0 }, false),
    ];
    for (code, expected_screen, should_quit) in immediate_actions {
        let mut app = finish_practice(config.clone(), started);
        app.handle_key_at(key(code), started + Duration::from_secs(1));
        assert_eq!(app.screen, expected_screen, "{code:?}");
        assert_eq!(app.should_quit, should_quit, "{code:?}");
    }

    let mut app = test_app(Config::default());
    app.screen = Screen::HistoryDetail;
    app.result_origin = ResultOrigin::History;
    app.handle_key_at(key(KeyCode::Esc), started);
    assert_eq!(app.screen, Screen::History);
}

#[test]
fn countdown_escape_requires_two_presses_and_other_keys_disarm_it() {
    for bench in [false, true] {
        let started = Instant::now();
        let mut app = test_app(Config::default());
        if bench {
            app.handle_key_at(key(KeyCode::Char('b')), started);
        } else {
            app.handle_key_at(key(KeyCode::Enter), started);
        }
        app.handle_key_at(key(KeyCode::Enter), started);
        assert!(matches!(app.screen, Screen::Countdown { .. }));

        app.handle_key_at(key(KeyCode::Esc), started + Duration::from_millis(100));
        assert!(matches!(app.screen, Screen::Countdown { .. }), "bench={bench}");
        assert!(render(&app).contains("Press Esc again to quit"), "bench={bench}");
        app.handle_key_at(key(KeyCode::Esc), started + Duration::from_millis(200));
        assert_eq!(app.screen, Screen::Title, "bench={bench}");

        let mut app = test_app(Config::default());
        if bench {
            app.handle_key_at(key(KeyCode::Char('b')), started);
        } else {
            app.handle_key_at(key(KeyCode::Enter), started);
        }
        app.handle_key_at(key(KeyCode::Enter), started);
        app.handle_key_at(key(KeyCode::Esc), started + Duration::from_millis(100));
        app.handle_key_at(key(KeyCode::Char('x')), started + Duration::from_millis(200));
        app.handle_key_at(key(KeyCode::Esc), started + Duration::from_millis(300));
        assert!(matches!(app.screen, Screen::Countdown { .. }), "bench={bench}");
    }
}

#[test]
fn countdown_escape_confirmation_expires_after_two_seconds() {
    let started = Instant::now();
    let mut app = test_app(Config::default());
    app.handle_key_at(key(KeyCode::Enter), started);
    app.handle_key_at(key(KeyCode::Enter), started);
    app.handle_key_at(key(KeyCode::Esc), started + Duration::from_millis(100));
    app.handle_key_at(key(KeyCode::Esc), started + Duration::from_millis(2_101));
    assert!(matches!(app.screen, Screen::Countdown { .. }));
    assert!(render(&app).contains("Press Esc again to quit"));
}

#[test]
fn playing_escape_requires_two_presses_and_other_keys_disarm_it() {
    for bench in [false, true] {
        let started = Instant::now();
        let mut config = Config::default();
        config.session.duration_seconds = 60;
        let mut app = if bench {
            let mut app = test_app(config);
            app.handle_key_at(key(KeyCode::Char('b')), started);
            app.handle_key_at(key(KeyCode::Enter), started);
            app.on_tick_at(Duration::from_secs(3), started).unwrap();
            app
        } else {
            start_practice(config, started)
        };

        app.handle_key_at(key(KeyCode::Esc), started + Duration::from_millis(100));
        assert_eq!(app.screen, Screen::Playing, "bench={bench}");
        assert!(render(&app).contains("Press Esc again to quit"), "bench={bench}");
        app.handle_key_at(key(KeyCode::Esc), started + Duration::from_millis(200));
        assert_eq!(app.screen, Screen::Title, "bench={bench}");

        let mut app = if bench {
            let mut app = test_app(Config::default());
            app.handle_key_at(key(KeyCode::Char('b')), started);
            app.handle_key_at(key(KeyCode::Enter), started);
            app.on_tick_at(Duration::from_secs(3), started).unwrap();
            app
        } else {
            start_practice(Config::default(), started)
        };
        app.handle_key_at(key(KeyCode::Esc), started + Duration::from_millis(100));
        app.handle_key_at(key(KeyCode::Char('x')), started + Duration::from_millis(200));
        app.handle_key_at(key(KeyCode::Esc), started + Duration::from_millis(300));
        assert_eq!(app.screen, Screen::Playing, "bench={bench}");
    }
}

#[test]
fn playing_escape_confirmation_expires_after_two_seconds() {
    let started = Instant::now();
    let mut app = start_practice(Config::default(), started);
    app.handle_key_at(key(KeyCode::Esc), started + Duration::from_millis(100));
    app.handle_key_at(key(KeyCode::Esc), started + Duration::from_millis(2_101));
    assert_eq!(app.screen, Screen::Playing);
    assert!(render(&app).contains("Press Esc again to quit"));
}
