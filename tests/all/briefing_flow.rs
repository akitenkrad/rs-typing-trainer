use std::time::{Duration, Instant};

use chrono::NaiveDate;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::app::Screen;
use rs_typing_trainer::config::{BenchSeries, Config, DrillCell, DrillSkill, PromptUnit};

use crate::common::{test_app, test_app_at};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn benchmark_uses_v3_by_default_and_keeps_explicit_v2_available() {
    let default = test_app(Config::default());
    assert!(default.benchmark_series_v3());

    let v2_config = Config::from_toml("[bench]\nseries = \"v2\"\n").unwrap();
    assert_eq!(v2_config.bench.series, BenchSeries::V2);
    let v2 = test_app(v2_config);
    assert!(!v2.benchmark_series_v3());
}

#[test]
fn menu_briefing_waits_for_enter_and_escape_returns_to_the_menu() {
    let date = NaiveDate::from_ymd_opt(2026, 11, 18).unwrap();
    let now = Instant::now();
    let mut app = test_app_at(Config::default(), date);
    app.handle_key_at(key(KeyCode::Char('m')), now);
    app.handle_key_at(key(KeyCode::Enter), now);
    assert_eq!(app.screen, Screen::Briefing);

    app.on_tick_at_date(Duration::from_secs(30), now + Duration::from_secs(30), date).unwrap();
    assert_eq!(app.screen, Screen::Briefing);
    app.handle_key_at(key(KeyCode::Char('x')), now + Duration::from_secs(31));
    assert_eq!(app.screen, Screen::Briefing);
    app.handle_key_at(key(KeyCode::Enter), now + Duration::from_secs(32));
    assert!(matches!(app.screen, Screen::Countdown { .. }));

    let mut back = test_app_at(Config::default(), date);
    back.handle_key_at(key(KeyCode::Char('m')), now);
    back.handle_key_at(key(KeyCode::Enter), now);
    back.handle_key_at(key(KeyCode::Esc), now);
    assert_eq!(back.screen, Screen::Menu);
    assert_eq!(back.menu_active_step, None);
}

#[test]
fn cell_briefing_waits_for_enter_and_escape_returns_to_the_table() {
    let now = Instant::now();
    let mut app = test_app(Config::default());
    app.handle_key_at(key(KeyCode::Enter), now);
    assert_eq!(app.screen, Screen::ModeSelect);
    app.handle_key_at(key(KeyCode::Enter), now);
    assert_eq!(app.screen, Screen::Briefing);
    assert!(app.cell_active().is_some());

    app.on_tick_at(Duration::from_secs(30), now + Duration::from_secs(30)).unwrap();
    assert_eq!(app.screen, Screen::Briefing);
    app.handle_key_at(key(KeyCode::Esc), now + Duration::from_secs(31));
    assert_eq!(app.screen, Screen::ModeSelect);
    assert!(app.cell_active().is_none());
}

#[test]
fn cell_briefing_prepares_the_live_goal_line_before_countdown() {
    let now = Instant::now();
    let mut app = test_app(Config::default());
    app.handle_key_at(key(KeyCode::Enter), now);
    app.handle_key_at(key(KeyCode::Down), now);
    app.handle_key_at(key(KeyCode::Enter), now);

    assert_eq!(app.screen, Screen::Briefing);
    let goal = &app.menu_live_goal.as_ref().expect("accuracy cell has a goal").goal;
    assert!(goal.target.is_some(), "the accuracy gate is available before countdown");
}

#[test]
fn key_picker_leads_to_the_briefing_before_countdown() {
    let now = Instant::now();
    let mut app = test_app(Config::default());
    app.handle_key_at(key(KeyCode::Enter), now);
    app.drill_cell = DrillCell::new(DrillSkill::Normal, PromptUnit::Word);
    app.handle_key_at(key(KeyCode::Enter), now);
    assert_eq!(app.screen, Screen::KeyPicker);

    app.handle_key_at(key(KeyCode::Enter), now + Duration::from_millis(1));
    assert_eq!(app.screen, Screen::Briefing);
    app.on_tick_at(Duration::from_secs(30), now + Duration::from_secs(30)).unwrap();
    assert_eq!(app.screen, Screen::Briefing);
    app.handle_key_at(key(KeyCode::Enter), now + Duration::from_secs(31));
    assert!(matches!(app.screen, Screen::Countdown { .. }));
}

#[test]
fn cell_retry_and_benchmark_skip_the_briefing() {
    let now = Instant::now();
    let mut config = Config::default();
    config.session.duration_seconds = 1;
    config.session.result_input_delay_ms = 0;
    let mut app = test_app(config);
    app.handle_key_at(key(KeyCode::Enter), now);
    app.handle_key_at(key(KeyCode::Enter), now);
    assert_eq!(app.screen, Screen::Briefing);
    app.handle_key_at(key(KeyCode::Enter), now);
    app.on_tick_at(Duration::from_secs(3), now + Duration::from_secs(3)).unwrap();
    app.on_tick_at(Duration::from_secs(1), now + Duration::from_secs(4)).unwrap();
    assert_eq!(app.screen, Screen::Result);
    app.handle_key_at(key(KeyCode::Enter), now + Duration::from_secs(5));
    assert!(matches!(app.screen, Screen::Countdown { .. }));

    let mut bench = test_app(Config::default());
    bench.handle_key_at(key(KeyCode::Char('b')), now);
    bench.handle_key_at(key(KeyCode::Enter), now);
    assert!(matches!(bench.screen, Screen::Countdown { .. }));
}
