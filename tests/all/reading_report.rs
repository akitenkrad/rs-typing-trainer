use chrono::{TimeZone, Utc};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::config::{Config, ModeKind};
use rs_typing_trainer::practice::reading_report::{
    ReadingReportKind, append_reading_report, load_reading_exclusions, load_reading_reports, remove_reading_report,
};

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
fn reports_are_grouped_counted_and_sorted_by_latest_timestamp() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("reading_reports.tsv"),
        concat!(
            "2026-11-14T09:30:00+00:00\tromaji\t十分\tじゅうぶん\n",
            "not a report\n",
            "2026-11-14T10:00:00+00:00\tsentence\t今日は晴れ。\tきょうははれ。\n",
            "2026-11-14T11:00:00+00:00\tromaji\t十分\tじゅうぶん\n",
            "2026-11-14T12:00:00+00:00\tunknown\t未知\tみち\n",
        ),
    )
    .unwrap();

    let reports = load_reading_reports(directory.path()).unwrap();

    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].text, "十分");
    assert_eq!(reports[0].reading, "じゅうぶん");
    assert_eq!(reports[0].kind, ReadingReportKind::Romaji);
    assert_eq!(reports[0].count, 2);
    assert_eq!(reports[0].latest_at, Utc.with_ymd_and_hms(2026, 11, 14, 11, 0, 0).unwrap());
    assert_eq!(reports[1].kind, ReadingReportKind::Sentence);
}

#[test]
fn missing_report_file_loads_empty_and_removal_is_a_noop() {
    let directory = tempfile::tempdir().unwrap();

    assert!(load_reading_reports(directory.path()).unwrap().is_empty());
    assert_eq!(
        remove_reading_report(directory.path(), ReadingReportKind::Romaji, "十分", "じゅうぶん").unwrap(),
        0
    );
    assert!(!directory.path().join("reading_reports.tsv").exists());
}

#[test]
fn removal_drops_every_matching_report_and_preserves_all_other_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("reading_reports.tsv");
    let source = concat!(
        "2026-11-14T09:30:00+00:00\tromaji\t十分\tじゅうぶん\n",
        "malformed line stays byte-for-byte\r\n",
        "2026-11-14T10:00:00+00:00\tsentence\t十分です。\tじゅうぶんです。\r\n",
        "\n",
        "2026-11-14T11:00:00+00:00\tromaji\t十分\tじゅうぶん\n",
        "2026-11-14T12:00:00+00:00\tunknown\t十分\tじゅうぶん\n",
    );
    let expected = concat!(
        "malformed line stays byte-for-byte\r\n",
        "2026-11-14T10:00:00+00:00\tsentence\t十分です。\tじゅうぶんです。\r\n",
        "\n",
        "2026-11-14T12:00:00+00:00\tunknown\t十分\tじゅうぶん\n",
    );
    std::fs::write(&path, source.as_bytes()).unwrap();

    let removed = remove_reading_report(directory.path(), ReadingReportKind::Romaji, "十分", "じゅうぶん").unwrap();

    assert_eq!(removed, 2);
    assert_eq!(std::fs::read(path).unwrap(), expected.as_bytes());
}

#[test]
fn fields_requiring_tsv_sanitizing_round_trip_through_load_and_removal() {
    let directory = tempfile::tempdir().unwrap();
    let at = Utc.with_ymd_and_hms(2026, 11, 14, 9, 30, 0).unwrap();
    append_reading_report(directory.path(), at, ReadingReportKind::Romaji, "表\t記\n続", "よみ\rがな").unwrap();

    let reports = load_reading_reports(directory.path()).unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].text, "表 記 続");
    assert_eq!(reports[0].reading, "よみ がな");
    assert_eq!(
        remove_reading_report(directory.path(), reports[0].kind, &reports[0].text, &reports[0].reading).unwrap(),
        1
    );
    assert!(std::fs::read(directory.path().join("reading_reports.tsv")).unwrap().is_empty());
    assert!(load_reading_exclusions(directory.path()).unwrap().is_empty());
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
    common::accept_briefing(&mut app, now);
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
    common::accept_briefing(&mut app, now);
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
