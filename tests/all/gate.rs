use std::time::Duration;

use chrono::{TimeZone, Utc};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::config::Config;
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::practice::gate::is_eligible;
use rs_typing_trainer::record::{SessionRecord, db};
use rs_typing_trainer::ui;
use tempfile::TempDir;

use crate::common;
use common::test_app;

fn record(wpm: f64, accuracy: f64, eligible: bool, second: u32) -> SessionRecord {
    let mut session = Session::new(Duration::from_secs(1), Duration::ZERO, Item::exact("a"));
    session.activate(Duration::ZERO);
    session.input_char(Duration::from_millis(100), "a");
    session.tick(Duration::from_secs(1));
    let mut record = SessionRecord::from_log(
        &Config::default(),
        session.into_log(),
        Utc.with_ymd_and_hms(2026, 11, 14, 0, 0, second).unwrap(),
    );
    record.header.net_wpm = wpm;
    record.header.accuracy = accuracy;
    record.header.mode = "words".into();
    record.header.mode_params = serde_json::json!({ "score_eligible": eligible }).to_string();
    record
}

#[test]
fn accuracy_gate_has_an_inclusive_threshold_and_zero_disables_it() {
    assert!(is_eligible(0.0, 0.1));
    assert!(is_eligible(0.95, 0.95));
    assert!(!is_eligible(0.95, 0.949));
}

#[test]
fn ineligible_session_is_saved_but_excluded_from_best_and_ranking() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("sessions.db");
    db::save(&path, &record(90.0, 0.80, false, 1)).unwrap();
    let eligible_id = db::save(&path, &record(50.0, 0.99, true, 2)).unwrap();

    assert_eq!(db::list(&path).unwrap().len(), 2);
    let ranked = db::ranked_sessions(&path).unwrap();
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0].id, eligible_id);
    assert_eq!(db::personal_best(&path, "words").unwrap().unwrap().id, eligible_id);
}

#[test]
fn result_screen_explains_gate_failure() {
    let mut config = Config::default();
    config.gate.min_accuracy = 0.97;
    config.mode.keys = "a".into();
    config.mode.random.word_length = 1;
    config.game.word_length = 1;
    config.session.duration_seconds = 1;
    config.ui.animation_enabled = false;
    let mut app = test_app(config);
    let now = std::time::Instant::now();
    let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
    app.handle_key_at(key(KeyCode::Enter), now);
    app.handle_key_at(key(KeyCode::Enter), now);
    crate::common::accept_briefing(&mut app, now);
    app.on_tick_at(Duration::from_secs(3), now).unwrap();
    app.on_tick_at(Duration::from_millis(1), now).unwrap();
    app.handle_key_at(key(KeyCode::Char('b')), now + Duration::from_millis(10));
    app.on_tick_at(Duration::from_secs(2), now + Duration::from_secs(2)).unwrap();
    assert!(app.gate_failed);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    let output = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect::<String>();
    assert!(output.contains("Gate not met"), "{output}");
}
