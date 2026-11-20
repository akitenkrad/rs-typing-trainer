use chrono::{TimeZone, Utc};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::config::{Config, ModeKind};
use rs_typing_trainer::practice::reading_report::{ReadingReportKind, append_reading_report, load_reading_exclusions};

use crate::common;
use common::test_app_at;

#[test]
fn report_is_appended_and_loaded_as_a_next_start_exclusion() {
    let directory = tempfile::tempdir().unwrap();
    let at = Utc.with_ymd_and_hms(2026, 11, 14, 9, 30, 0).unwrap();
    append_reading_report(directory.path(), at, ReadingReportKind::Romaji, "今日", "きょう").unwrap();
    append_reading_report(directory.path(), at, ReadingReportKind::Sentence, "今日は晴れ。", "きょうははれ。").unwrap();

    let text = std::fs::read_to_string(directory.path().join("reading_reports.tsv")).unwrap();
    assert_eq!(text.lines().count(), 2);
    assert!(text.contains("2026-11-14T09:30:00+00:00\tromaji\t今日\tきょう"));
    let exclusions = load_reading_exclusions(directory.path()).unwrap();
    assert!(exclusions.contains(&(ReadingReportKind::Romaji, "今日".into(), "きょう".into())));
    assert!(exclusions.contains(&(ReadingReportKind::Sentence, "今日は晴れ。".into(), "きょうははれ。".into())));
}

#[test]
fn f2_reports_and_immediately_leaves_the_bad_item() {
    let directory = tempfile::tempdir().unwrap();
    let data = directory.path().join("sentences.tsv");
    std::fs::write(&data, "短い文です。\tみじかいぶんです。\n").unwrap();
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
    let mut config = Config::default();
    config.mode.kind = ModeKind::Sentence;
    config.mode.sentence.lang = "ja".into();
    config.data.sentences_ja = Some(data);
    config.storage.db_path = Some(directory.path().join("sessions.db"));
    config.ui.animation_enabled = false;
    let mut app = test_app_at(config, date);
    let now = std::time::Instant::now();
    app.handle_key_at(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), now);
    app.handle_key_at(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), now);
    app.on_tick_at_date(std::time::Duration::from_secs(3), now, date).unwrap();
    app.on_tick_at_date(std::time::Duration::from_millis(1), now, date).unwrap();
    let reported = app.game.as_ref().unwrap().current_prompt.text.clone();

    app.handle_key_at(
        KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE),
        now + std::time::Duration::from_millis(2),
    );

    assert_ne!(app.game.as_ref().unwrap().current_prompt.text, reported);
    let report = std::fs::read_to_string(directory.path().join("reading_reports.tsv")).unwrap();
    assert!(report.contains(&reported));
}

#[test]
fn ctrl_r_reports_like_f2_because_macos_sends_f2_only_with_fn() {
    let directory = tempfile::tempdir().unwrap();
    let data = directory.path().join("sentences.tsv");
    std::fs::write(&data, "短い文です。\tみじかいぶんです。\n").unwrap();
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
    let mut config = Config::default();
    config.mode.kind = ModeKind::Sentence;
    config.mode.sentence.lang = "ja".into();
    config.data.sentences_ja = Some(data);
    config.storage.db_path = Some(directory.path().join("sessions.db"));
    config.ui.animation_enabled = false;
    let mut app = test_app_at(config, date);
    let now = std::time::Instant::now();
    app.handle_key_at(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), now);
    app.handle_key_at(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), now);
    app.on_tick_at_date(std::time::Duration::from_secs(3), now, date).unwrap();
    app.on_tick_at_date(std::time::Duration::from_millis(1), now, date).unwrap();
    let reported = app.game.as_ref().unwrap().current_prompt.text.clone();

    app.handle_key_at(
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
        now + std::time::Duration::from_millis(2),
    );

    // MYTASK-3635: the report must not be typed as the character `r`.
    assert_eq!(app.game.as_ref().unwrap().session.log().keystrokes.len(), 0);
    assert_ne!(app.game.as_ref().unwrap().current_prompt.text, reported);
    let report = std::fs::read_to_string(directory.path().join("reading_reports.tsv")).unwrap();
    assert!(report.contains(&reported));
}
