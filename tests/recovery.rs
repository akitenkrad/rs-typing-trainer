use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::config::Config;
use rs_typing_trainer::config::ErrorMode;
use rs_typing_trainer::engine::session::{KeystrokeKind, KeystrokeLog, SessionLog};
use rs_typing_trainer::practice::recovery::recovery_metrics;
use std::time::{Duration, Instant};

mod common;
use common::test_app;

fn key(seq: u32, ms: u64, correct: bool) -> KeystrokeLog {
    key_for_item(seq, ms, correct, 0)
}

fn key_for_item(seq: u32, ms: u64, correct: bool, item_idx: u32) -> KeystrokeLog {
    KeystrokeLog {
        seq,
        t_us: ms * 1_000,
        release_us: None,
        item_idx,
        pos: seq,
        kind: KeystrokeKind::Char,
        typed: Some("a".into()),
        expected: Some("a".into()),
        correct,
        code: Some("KeyA".into()),
    }
}

#[test]
fn recovery_counts_keys_to_session_median_speed_and_next_three_error_rate() {
    let log = SessionLog {
        duration_us: 2_000_000,
        error_mode: ErrorMode::Advance,
        items: vec![],
        keystrokes: vec![
            key(0, 0, true),
            key(1, 100, true),
            key(2, 200, false),
            key(3, 600, false),
            key(4, 900, true),
            key(5, 1_000, true),
            key(6, 1_100, true),
        ],
    };
    let metrics = recovery_metrics(&log);
    assert_eq!(metrics.median_keys_to_recover, Some(2.5));
    assert!((metrics.next_three_error_rate - 1.0 / 6.0).abs() < 1e-9);
}

#[test]
fn empty_log_has_no_recovery_sample() {
    assert_eq!(recovery_metrics(&SessionLog::default()).median_keys_to_recover, None);
}

#[test]
fn recovery_speed_excludes_item_boundaries() {
    let log = SessionLog {
        duration_us: 2_000_000,
        error_mode: ErrorMode::Advance,
        items: vec![],
        keystrokes: vec![
            key_for_item(0, 0, true, 0),
            key_for_item(1, 100, false, 0),
            key_for_item(2, 600, true, 1),
            key_for_item(3, 1_000, true, 1),
        ],
    };

    assert_eq!(recovery_metrics(&log).median_keys_to_recover, None);
}

#[test]
fn recovery_with_only_item_boundaries_has_no_timing_sample() {
    let log = SessionLog {
        duration_us: 1_000_000,
        error_mode: ErrorMode::Advance,
        items: vec![],
        keystrokes: vec![key_for_item(0, 100, false, 0), key_for_item(1, 600, true, 1)],
    };

    assert_eq!(recovery_metrics(&log).median_keys_to_recover, None);
}

#[test]
fn retry_on_error_repeats_the_item_once_immediately() {
    let mut config = Config::default();
    config.mode.keys = "a".into();
    config.mode.random.word_length = 1;
    config.game.word_length = 1;
    config.session.error_mode = ErrorMode::Advance;
    config.session.retry_on_error = true;
    config.ui.animation_enabled = false;
    let mut app = test_app(config);
    let now = Instant::now();
    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    app.handle_key_at(enter, now);
    app.handle_key_at(enter, now);
    app.on_tick_at(Duration::from_secs(3), now).unwrap();
    app.on_tick_at(Duration::ZERO, now).unwrap();
    let original = app.game.as_ref().unwrap().current_prompt.text.clone();
    app.handle_key_at(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE), now + Duration::from_millis(10));
    app.on_tick_at(Duration::from_secs(1), now + Duration::from_secs(1)).unwrap();
    app.on_tick_at(Duration::ZERO, now + Duration::from_secs(1)).unwrap();
    let game = app.game.as_ref().unwrap();
    assert_eq!(game.current_prompt.text, original);
    assert!(game.current_is_retry);

    app.handle_key_at(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE), now + Duration::from_millis(1_100));
    app.on_tick_at(Duration::from_secs(1), now + Duration::from_secs(2)).unwrap();
    app.on_tick_at(Duration::ZERO, now + Duration::from_secs(2)).unwrap();
    assert!(!app.game.as_ref().unwrap().current_is_retry);
}
