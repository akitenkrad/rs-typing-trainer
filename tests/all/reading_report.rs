use chrono::{TimeZone, Utc};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::config::{Config, DataConfig, ModeKind};
use rs_typing_trainer::data::{DataCatalog, DataKind, TrainingDataSource};
use rs_typing_trainer::practice::reading_report::{
    ReadingCorrectionValidationError, ReadingReportKind, append_reading_report, apply_reading_reports_to_catalog, load_reading_corrections,
    load_reading_exclusions, load_reading_reports, remove_reading_report, remove_reading_report_and_correction, save_reading_correction,
    validate_reading_correction,
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
fn correction_is_saved_loaded_and_sanitized() {
    let directory = tempfile::tempdir().unwrap();
    let at = Utc.with_ymd_and_hms(2026, 11, 14, 9, 30, 0).unwrap();

    save_reading_correction(
        directory.path(),
        at,
        ReadingReportKind::Sentence,
        "表\t記\n続",
        "まち\rがい",
        "ただ\nしい",
    )
    .unwrap();
    save_reading_correction(
        directory.path(),
        at + chrono::Duration::minutes(1),
        ReadingReportKind::Sentence,
        "表\t記\n続",
        "まち\rがい",
        "なお\nした",
    )
    .unwrap();

    let source = std::fs::read_to_string(directory.path().join("reading_corrections.tsv")).unwrap();
    assert_eq!(source, "2026-11-14T09:31:00+00:00\tsentence\t表 記 続\tまち がい\tなお した\n");
    let corrections = load_reading_corrections(directory.path()).unwrap();
    assert_eq!(corrections.len(), 1);
    assert_eq!(corrections[0].at, at + chrono::Duration::minutes(1));
    assert_eq!(corrections[0].kind, ReadingReportKind::Sentence);
    assert_eq!(corrections[0].text, "表 記 続");
    assert_eq!(corrections[0].wrong_reading, "まち がい");
    assert_eq!(corrections[0].correct_reading, "なお した");
}

#[test]
fn resaving_a_correction_replaces_its_key_and_preserves_every_other_byte() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("reading_corrections.tsv");
    let mut source = concat!(
        "2026-11-14T08:00:00+00:00\tromaji\t十分\tじゅうふん\tじゅっぷん\r\n",
        "malformed line stays byte-for-byte\r\n",
        "2026-11-14T08:30:00+00:00\tsentence\t十分です．\tじゅうふんです．\tじゅっぷんです．\n",
        "2026-11-14T09:00:00+00:00\tromaji\t十分\tじゅうふん\tじゅうぶん\n",
    )
    .as_bytes()
    .to_vec();
    source.extend_from_slice(b"invalid utf-8: \xff\r\n");
    std::fs::write(&path, source).unwrap();
    let at = Utc.with_ymd_and_hms(2026, 11, 14, 10, 0, 0).unwrap();

    save_reading_correction(directory.path(), at, ReadingReportKind::Romaji, "十分", "じゅうふん", "じゅうぶん").unwrap();

    let mut expected = concat!(
        "malformed line stays byte-for-byte\r\n",
        "2026-11-14T08:30:00+00:00\tsentence\t十分です．\tじゅうふんです．\tじゅっぷんです．\n",
    )
    .as_bytes()
    .to_vec();
    expected.extend_from_slice(b"invalid utf-8: \xff\r\n");
    expected.extend_from_slice("2026-11-14T10:00:00+00:00\tromaji\t十分\tじゅうふん\tじゅうぶん\n".as_bytes());
    assert_eq!(std::fs::read(&path).unwrap(), expected);
    assert_eq!(
        std::fs::read(&path)
            .unwrap()
            .windows("\tromaji\t十分\tじゅうふん\t".len())
            .filter(|window| *window == "\tromaji\t十分\tじゅうふん\t".as_bytes())
            .count(),
        1
    );
    assert!(
        std::fs::read_dir(directory.path()).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".reading_corrections.tsv.tmp-"))
    );
}

#[test]
fn saving_after_an_unterminated_malformed_line_keeps_it_separate_from_the_new_record() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("reading_corrections.tsv");
    std::fs::write(&path, b"unterminated malformed line").unwrap();
    let at = Utc.with_ymd_and_hms(2026, 11, 14, 10, 0, 0).unwrap();

    save_reading_correction(directory.path(), at, ReadingReportKind::Romaji, "十分", "じゅうふん", "じゅうぶん").unwrap();

    assert_eq!(
        std::fs::read(&path).unwrap(),
        concat!(
            "unterminated malformed line\n",
            "2026-11-14T10:00:00+00:00\tromaji\t十分\tじゅうふん\tじゅうぶん\n"
        )
        .as_bytes()
    );
}

#[test]
fn removing_a_report_also_removes_its_correction_and_preserves_unrelated_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let at = Utc.with_ymd_and_hms(2026, 11, 14, 9, 30, 0).unwrap();
    append_reading_report(directory.path(), at, ReadingReportKind::Romaji, "十分", "じゅうふん").unwrap();
    append_reading_report(directory.path(), at, ReadingReportKind::Sentence, "十分です．", "じゅうふんです．").unwrap();
    save_reading_correction(directory.path(), at, ReadingReportKind::Romaji, "十分", "じゅうふん", "じゅうぶん").unwrap();
    save_reading_correction(
        directory.path(),
        at,
        ReadingReportKind::Sentence,
        "十分です．",
        "じゅうふんです．",
        "じゅうぶんです．",
    )
    .unwrap();

    let removed = remove_reading_report_and_correction(directory.path(), ReadingReportKind::Romaji, "十分", "じゅうふん").unwrap();

    assert_eq!(removed, (1, 1));
    assert!(
        !std::fs::read_to_string(directory.path().join("reading_reports.tsv"))
            .unwrap()
            .contains("\t十分\tじゅうふん")
    );
    let corrections = std::fs::read_to_string(directory.path().join("reading_corrections.tsv")).unwrap();
    assert!(!corrections.contains("\t十分\tじゅうふん\t"));
    assert!(corrections.contains("\t十分です．\tじゅうふんです．\tじゅうぶんです．"));
}

#[test]
fn reports_are_joined_with_the_latest_saved_correction() {
    let directory = tempfile::tempdir().unwrap();
    let at = Utc.with_ymd_and_hms(2026, 11, 14, 9, 30, 0).unwrap();
    append_reading_report(directory.path(), at, ReadingReportKind::Romaji, "十分", "じゅうふん").unwrap();
    save_reading_correction(directory.path(), at, ReadingReportKind::Romaji, "十分", "じゅうふん", "じゅうぶん").unwrap();

    let reports = load_reading_reports(directory.path()).unwrap();

    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].correct_reading.as_deref(), Some("じゅうぶん"));
}

#[test]
fn correction_validation_rejects_empty_unchanged_and_non_hiragana_input() {
    assert_eq!(
        validate_reading_correction("じゅうふん", "じゅうふん", ""),
        Err(ReadingCorrectionValidationError::Empty)
    );
    assert_eq!(
        validate_reading_correction("じゅうふん", "じゅうぶん", "じゅうぶん"),
        Err(ReadingCorrectionValidationError::Unchanged)
    );
    assert_eq!(
        validate_reading_correction("じゅうふん", "じゅうふん", "juubun"),
        Err(ReadingCorrectionValidationError::NonHiragana)
    );
    assert_eq!(
        validate_reading_correction("じゅうふん", "じゅうふん", "ジュウブン"),
        Err(ReadingCorrectionValidationError::NonHiragana)
    );
}

#[test]
fn correction_validation_allows_only_original_symbols_and_rejects_untypable_kana() {
    assert_eq!(
        validate_reading_correction("きょうは（はれ）？", "きょうは（はれ）？", "きょうははれ？"),
        Ok(())
    );
    assert_eq!(
        validate_reading_correction("きょうははれ", "きょうははれ", "きょうは，はれ"),
        Err(ReadingCorrectionValidationError::NonHiragana)
    );
    assert_eq!(
        validate_reading_correction("い", "い", "ゐ"),
        Err(ReadingCorrectionValidationError::Untypable("ゐ".to_owned()))
    );
}

#[test]
fn corrections_are_applied_before_exclusions_without_chaining_and_are_skipped_for_bench() {
    let directory = tempfile::tempdir().unwrap();
    let words = directory.path().join("words.tsv");
    let phrases = directory.path().join("phrases.tsv");
    let sentences = directory.path().join("sentences.tsv");
    std::fs::write(
        &words,
        concat!(
            "十分\tじゅうふん\n",
            "除外\tじょがい\n",
            "連鎖\tれんさ\n",
            "置換後除外\tちかんごじょがい\n",
        ),
    )
    .unwrap();
    std::fs::write(&phrases, "よく使う\tよくつかう\n").unwrap();
    std::fs::write(&sentences, "十分です．\tじゅうふんです．\n").unwrap();
    let config = DataConfig {
        words_ja: Some(words),
        phrases_ja: Some(phrases),
        sentences_ja: Some(sentences),
        ..DataConfig::default()
    };
    let at = Utc.with_ymd_and_hms(2026, 11, 14, 9, 30, 0).unwrap();
    for (kind, text, wrong, correct) in [
        (ReadingReportKind::Romaji, "十分", "じゅうふん", "じゅうぶん"),
        (ReadingReportKind::Romaji, "よく使う", "よくつかう", "よくつかいます"),
        (ReadingReportKind::Sentence, "十分です．", "じゅうふんです．", "じゅうぶんです．"),
        (ReadingReportKind::Romaji, "連鎖", "れんさ", "れんざ"),
        (ReadingReportKind::Romaji, "連鎖", "れんざ", "れんしゃ"),
        (ReadingReportKind::Romaji, "置換後除外", "ちかんごじょがい", "ちかんごじょうがい"),
    ] {
        save_reading_correction(directory.path(), at, kind, text, wrong, correct).unwrap();
    }
    for (kind, text, reading) in [
        (ReadingReportKind::Romaji, "十分", "じゅうふん"),
        (ReadingReportKind::Romaji, "除外", "じょがい"),
        (ReadingReportKind::Sentence, "十分です．", "じゅうふんです．"),
        (ReadingReportKind::Romaji, "置換後除外", "ちかんごじょがい"),
        (ReadingReportKind::Romaji, "置換後除外", "ちかんごじょうがい"),
    ] {
        append_reading_report(directory.path(), at, kind, text, reading).unwrap();
    }
    let original = DataCatalog::load(&config, directory.path()).unwrap();
    let mut practice = original.clone();
    let mut bench = original;

    apply_reading_reports_to_catalog(&mut practice, directory.path(), false).unwrap();
    apply_reading_reports_to_catalog(&mut bench, directory.path(), true).unwrap();

    assert!(practice.words_ja().iter().any(|row| row.text == "十分" && row.reading == "じゅうぶん"));
    assert!(!practice.words_ja().iter().any(|row| row.text == "除外"));
    assert!(practice.words_ja().iter().any(|row| row.text == "連鎖" && row.reading == "れんざ"));
    assert!(!practice.words_ja().iter().any(|row| row.text == "置換後除外"));
    assert!(
        practice
            .phrases_ja()
            .iter()
            .any(|row| row.text == "よく使う" && row.reading == "よくつかいます")
    );
    assert!(
        practice
            .sentences_ja()
            .iter()
            .any(|row| row.text == "十分です．" && row.reading == "じゅうぶんです．")
    );
    assert!(
        practice
            .all_fixed_items()
            .iter()
            .any(|item| item.text == "十分" && item.reading.as_deref() == Some("じゅうぶん") && item.key.ends_with("\tじゅうぶん"))
    );
    assert!(bench.words_ja().iter().any(|row| row.text == "十分" && row.reading == "じゅうふん"));
    assert!(bench.words_ja().iter().any(|row| row.text == "除外" && row.reading == "じょがい"));
}

#[test]
fn sentence_source_keys_and_passage_readings_follow_corrections() {
    let directory = tempfile::tempdir().unwrap();
    let mut catalog = DataCatalog::load(&DataConfig::default(), directory.path()).unwrap();
    let sentence = catalog
        .sentences_ja()
        .iter()
        .find(|row| {
            catalog
                .sentence_source(DataKind::JapaneseSentence, &row.text, Some(&row.reading))
                .is_some()
        })
        .unwrap()
        .clone();
    let source = catalog
        .sentence_source(DataKind::JapaneseSentence, &sentence.text, Some(&sentence.reading))
        .unwrap()
        .clone();
    let passage = catalog.passages_ja().first().unwrap().clone();
    let sentence_correct = format!("{}ー", sentence.reading);
    let passage_correct = format!("{}ー", passage.reading);
    let at = Utc.with_ymd_and_hms(2026, 11, 14, 9, 30, 0).unwrap();
    save_reading_correction(
        directory.path(),
        at,
        ReadingReportKind::Sentence,
        &sentence.text,
        &sentence.reading,
        &sentence_correct,
    )
    .unwrap();
    save_reading_correction(
        directory.path(),
        at,
        ReadingReportKind::Sentence,
        &passage.text,
        &passage.reading,
        &passage_correct,
    )
    .unwrap();

    apply_reading_reports_to_catalog(&mut catalog, directory.path(), false).unwrap();

    assert_eq!(
        catalog.sentence_source(DataKind::JapaneseSentence, &sentence.text, Some(&sentence_correct)),
        Some(&source)
    );
    assert!(
        catalog
            .passages_ja()
            .iter()
            .any(|row| row.text == passage.text && row.reading == passage_correct)
    );
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
