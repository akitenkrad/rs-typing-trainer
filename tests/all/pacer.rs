use std::time::Duration;

use chrono::{TimeZone, Utc};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::config::Config;
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::practice::pacer::{cursor_position, next_target_wpm, target_wpm};
use rs_typing_trainer::record::SessionRecord;
use rs_typing_trainer::ui::bigtext::{BigTextSize, font};

use crate::common;
use common::test_app;

fn cell_position(terminal: &Terminal<TestBackend>, symbol: &str) -> (u16, u16) {
    let buffer = terminal.backend().buffer();
    let index = buffer.content().iter().position(|cell| cell.symbol() == symbol).unwrap();
    (index as u16 % buffer.area.width, index as u16 / buffer.area.width)
}

fn record(wpm: f64, bench_id: Option<&str>, second: u32) -> SessionRecord {
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
    if let Some(id) = bench_id {
        record.header.mode = "bench".into();
        record.header.bench_id = Some(id.into());
    }
    record
}

#[test]
fn bench_a_is_preferred_for_target_speed() {
    let records = vec![
        record(40.0, None, 1),
        record(50.0, Some("bench-v1/A/03"), 2),
        record(80.0, Some("bench-v1/B/03"), 3),
    ];
    assert!((target_wpm(&records, 1.07, 20.0) - 53.5).abs() < 1e-9);
}

#[test]
fn legacy_bench_a_is_identified_by_bench_id_instead_of_mode() {
    let mut bench = record(50.0, Some("bench-v1/A/03"), 2);
    bench.header.mode = "words".into();
    assert!((target_wpm(&[record(40.0, None, 1), bench], 1.07, 20.0) - 53.5).abs() < 1e-9);
}

#[test]
fn recent_five_session_median_is_the_fallback() {
    let records = (1..=6).map(|value| record(value as f64 * 10.0, None, value)).collect::<Vec<_>>();
    assert!((target_wpm(&records, 1.07, 20.0) - 42.8).abs() < 1e-9);
}

#[test]
fn minimum_wpm_is_used_without_history_and_clamps_low_history() {
    assert_eq!(target_wpm(&[], 1.07, 20.0), 20.0);
    assert_eq!(target_wpm(&[record(0.0, None, 1)], 1.07, 20.0), 20.0);
    assert_eq!(target_wpm(&[record(5.0, Some("bench-v1/A/03"), 1)], 1.07, 20.0), 20.0);
}

#[test]
fn target_goes_down_for_low_accuracy_and_up_after_success() {
    assert!((next_target_wpm(50.0, 60.0, 0.91, 20.0) - 48.5).abs() < 1e-9);
    assert!((next_target_wpm(50.0, 50.0, 0.95, 20.0) - 51.0).abs() < 1e-9);
    assert_eq!(next_target_wpm(50.0, 49.9, 0.99, 20.0), 50.0);
    assert_eq!(next_target_wpm(20.0, 0.0, 0.0, 20.0), 20.0);
}

#[test]
fn cursor_position_uses_injected_elapsed_time() {
    assert_eq!(cursor_position(60.0, Duration::from_secs(1), 20), 5);
    assert_eq!(cursor_position(60.0, Duration::from_secs(10), 20), 20);
}

#[test]
fn the_speed_row_runs_the_pacer_and_the_normal_row_does_not() {
    // MYTASK-3638: the pacer is the «speed» row of the individual-training
    // table rather than a `[P]` toggle.
    let mut config = Config::default();
    config.mode.kind = rs_typing_trainer::config::ModeKind::Words;
    let mut app = test_app(config);
    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    app.handle_key(enter);
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(enter);
    app.handle_key(enter);
    assert!(app.config.pacer.enabled);
    assert!((app.config.pacer.ratio - 1.05).abs() < 1e-9);
    app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
    assert!(app.config.pacer.enabled, "P no longer toggles anything");
}

#[test]
fn a_newer_benchmark_resets_an_older_pacer_adjustment() {
    let mut paced = record(60.0, None, 1);
    paced.header.accuracy = 0.95;
    paced.header.mode_params = serde_json::json!({ "pacer_target_wpm": 50.0 }).to_string();
    let bench = record(40.0, Some("bench-v1/A/04"), 2);
    assert!((target_wpm(&[paced, bench], 1.07, 20.0) - 42.8).abs() < 1e-9);
}

#[test]
fn pacer_marker_is_directly_above_the_unicode_target_column_and_header_has_target() {
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("words.tsv");
    std::fs::write(&path, "a界b\n").unwrap();
    let mut config = Config::default();
    config.mode.kind = rs_typing_trainer::config::ModeKind::Words;
    config.mode.keys = "b".into();
    config.data.words_en = Some(path);
    config.pacer.enabled = true;
    config.ui.animation_enabled = false;
    let mut app = test_app(config);
    let start = std::time::Instant::now();
    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    app.handle_key_at(enter, start);
    app.handle_key_at(enter, start);
    app.handle_key_at(enter, start);
    crate::common::accept_briefing(&mut app, start);
    app.on_tick_at(Duration::from_secs(3), start).unwrap();
    app.on_tick_at(Duration::ZERO, start + Duration::from_millis(200)).unwrap();
    app.on_tick_at(Duration::ZERO, start + Duration::from_millis(400)).unwrap();
    app.config.pacer.target_wpm = Some(60.0);

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| rs_typing_trainer::ui::render(frame, &app)).unwrap();
    let marker = cell_position(&terminal, "▾");
    let font = font(BigTextSize::Dot12).unwrap();
    let expected_column = (80 - font.text_width("a界b")) / 2 + font.text_width("a");
    assert_eq!(marker.0, expected_column, "marker={marker:?}");
    let stats_row = terminal
        .backend()
        .buffer()
        .content()
        .chunks(80)
        .position(|row| row.iter().map(|cell| cell.symbol()).collect::<String>().contains("Combo"))
        .unwrap() as u16;
    assert_eq!(marker.1, stats_row + 1, "marker must occupy the row above the bitmap: {marker:?}");
    assert!((marker.1 + 1..=marker.1 + 6).any(|y| {
        (expected_column..expected_column + font.text_width("界")).any(|x| {
            matches!(
                terminal.backend().buffer()[(x, y)].symbol(),
                "▘" | "▝" | "▖" | "▗" | "▀" | "▄" | "▌" | "▐" | "▚" | "▞" | "▛" | "▜" | "▙" | "▟" | "█"
            )
        })
    }));
    let output = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect::<String>();
    assert!(output.contains("Pace: 60 WPM"), "{output}");
}
