use std::time::Duration;

use chrono::{TimeZone, Utc};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier, Style};
use rs_typing_trainer::app::{ResultOrigin, Screen};
use rs_typing_trainer::config::{Config, ModeKind};
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::metrics::stats::GameResult;
use rs_typing_trainer::record::ttrec;
use rs_typing_trainer::record::{SessionRecord, db};
use rs_typing_trainer::ui;

mod common;
use common::{TestApp, test_app_at};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn record(started_at: chrono::DateTime<Utc>, mode: &str, bench_id: Option<&str>, lang: Option<&str>, wpm: f64, accuracy: f64) -> SessionRecord {
    let mut config = Config::default();
    config.mode.kind = match mode {
        "words" => ModeKind::Words,
        "romaji" => ModeKind::Romaji,
        "sentence" => ModeKind::Sentence,
        "ngram" => ModeKind::Ngram,
        _ => ModeKind::Random,
    };
    let mut session = Session::new(Duration::from_secs(1), Duration::ZERO, Item::exact("a"));
    session.activate(Duration::ZERO);
    session.input_char(Duration::from_millis(100), "a");
    session.tick(Duration::from_secs(1));
    let mut record = SessionRecord::from_log(&config, session.into_log(), started_at);
    record.header.mode = mode.to_owned();
    record.header.bench_id = bench_id.map(str::to_owned);
    record.header.mode_params = serde_json::json!({
        "lang": lang,
        "score_eligible": true,
        "key_filter": "home",
        "pacer_enabled": true,
        "blind": false,
        "keyboard_show": "always"
    })
    .to_string();
    record.header.net_wpm = wpm;
    record.header.accuracy = accuracy;
    record.header.layout = "dvorak".to_owned();
    record.header.input_layout = "qwerty".to_owned();
    record.header.max_combo = 7;
    record.header.completed_words = 3;
    record
}

fn save(app: &TestApp, record: &SessionRecord) -> i64 {
    db::save(&app.db_path(), record).unwrap()
}

fn open_history(app: &mut TestApp) {
    app.handle_key(key(KeyCode::Char('h')));
    assert_eq!(app.screen, Screen::History);
}

fn render_buffer(app: &TestApp, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    terminal.backend().buffer().clone()
}

fn buffer_text(buffer: &Buffer) -> String {
    (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn render(app: &TestApp, width: u16, height: u16) -> String {
    buffer_text(&render_buffer(app, width, height))
}

fn style_for(buffer: &Buffer, row_contains: &str, value: &str) -> Style {
    for y in 0..buffer.area.height {
        let row = (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>();
        if row.contains(row_contains) {
            let x = row.find(value).unwrap_or_else(|| panic!("missing {value:?} in row {row:?}"));
            return buffer[(u16::try_from(x).unwrap(), y)].style();
        }
    }
    panic!("missing row containing {row_contains:?}");
}

#[test]
fn history_columns_distinguish_practice_bench_part_and_v1_import() {
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
    let mut app = test_app_at(Config::default(), date);
    save(
        &app,
        &record(
            Utc.with_ymd_and_hms(2026, 11, 16, 8, 0, 0).unwrap(),
            "words",
            None,
            Some("en"),
            55.0,
            0.98,
        ),
    );
    save(
        &app,
        &record(
            Utc.with_ymd_and_hms(2026, 11, 16, 9, 0, 0).unwrap(),
            "romaji",
            Some("bench-v1/C/03"),
            Some("ja"),
            42.0,
            0.96,
        ),
    );
    let mut imported = record(Utc.with_ymd_and_hms(2026, 11, 16, 10, 0, 0).unwrap(), "random", None, None, 35.0, 0.90);
    imported.header.metrics_version = 0;
    imported.header.legacy_payload =
        Some(serde_json::to_string(&GameResult::from_metrics(&Config::default(), Default::default(), Duration::from_secs(1))).unwrap());
    save(&app, &imported);

    open_history(&mut app);
    let output = render(&app, 140, 30);
    for expected in ["Practice", "Bench", "v1 import", "Words", "C · form 03", "en", "ja", "dvorak"] {
        assert!(output.contains(expected), "missing {expected:?}:\n{output}");
    }
}

#[test]
fn history_buffers_use_only_half_width_spaces() {
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    let mut list_app = test_app_at(config.clone(), date);
    let base = Utc.with_ymd_and_hms(2026, 11, 16, 8, 0, 0).unwrap();
    save(&list_app, &record(base, "words", Some("bench-v1/A/03"), Some("en"), 64.2, 0.98));
    save(
        &list_app,
        &record(
            base + chrono::Duration::minutes(1),
            "romaji",
            Some("bench-v1/C/03"),
            Some("ja"),
            42.1,
            0.96,
        ),
    );
    open_history(&mut list_app);
    let list = render(&list_app, 140, 30);
    assert!(!list.contains('\u{3000}'), "history list contains a full-width space:\n{list}");

    let mut detail_app = test_app_at(config, date);
    save(&detail_app, &record(base, "words", None, Some("en"), 55.0, 0.95));
    open_history(&mut detail_app);
    detail_app.handle_key(key(KeyCode::Enter));
    let detail = render(&detail_app, 140, 42);
    assert!(!detail.contains('\u{3000}'), "history detail contains a full-width space:\n{detail}");
}

#[test]
fn history_content_and_detail_show_only_display_mode_names() {
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
    let base = Utc.with_ymd_and_hms(2026, 11, 16, 8, 0, 0).unwrap();
    let mut english = test_app_at(Config::default(), date);
    save(&english, &record(base, "words", None, Some("en"), 50.0, 0.95));
    save(
        &english,
        &record(base + chrono::Duration::hours(1), "sentence", None, Some("en"), 55.0, 0.96),
    );
    open_history(&mut english);
    let list = render(&english, 140, 30);
    for expected in ["Words", "Sentences"] {
        assert!(list.contains(expected), "missing {expected:?}:\n{list}");
    }
    for internal in ["words —", "sentence —"] {
        assert!(!list.contains(internal), "internal mode name {internal:?} leaked:\n{list}");
    }
    english.handle_key(key(KeyCode::Enter));
    let detail = render(&english, 140, 42);
    assert!(detail.contains("Mode: Sentences"), "{detail}");
    assert!(!detail.contains("sentence —"), "{detail}");

    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    let mut japanese = test_app_at(config, date);
    for (hour, mode, lang) in [(8, "words", "en"), (9, "romaji", "ja"), (10, "sentence", "ja")] {
        save(
            &japanese,
            &record(
                Utc.with_ymd_and_hms(2026, 11, 16, hour, 0, 0).unwrap(),
                mode,
                None,
                Some(lang),
                50.0,
                0.95,
            ),
        );
    }
    open_history(&mut japanese);
    let list = render(&japanese, 140, 30);
    let compact_list = list.replace(' ', "");
    for expected in ["英単語", "日本語の単語", "短文"] {
        assert!(compact_list.contains(expected), "missing {expected:?}:\n{list}");
    }
    for internal in ["words —", "romaji —", "sentence —"] {
        assert!(!list.contains(internal), "internal mode name {internal:?} leaked:\n{list}");
    }
    japanese.handle_key(key(KeyCode::Enter));
    let detail = render(&japanese, 140, 42);
    assert!(detail.replace(' ', "").contains("モード:短文"), "{detail}");
}

#[test]
fn history_mode_filter_cycles_modes_matching_other_filters() {
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
    let mut app = test_app_at(Config::default(), date);
    save(
        &app,
        &record(
            Utc.with_ymd_and_hms(2026, 11, 16, 8, 0, 0).unwrap(),
            "romaji",
            None,
            Some("ja"),
            45.0,
            0.95,
        ),
    );
    save(
        &app,
        &record(
            Utc.with_ymd_and_hms(2026, 11, 16, 9, 0, 0).unwrap(),
            "random",
            Some("bench-v1/D/01"),
            Some("en"),
            65.0,
            0.95,
        ),
    );
    save(
        &app,
        &record(
            Utc.with_ymd_and_hms(2026, 10, 1, 8, 0, 0).unwrap(),
            "random",
            None,
            Some("ja"),
            35.0,
            0.95,
        ),
    );
    open_history(&mut app);
    app.handle_key(key(KeyCode::Tab));
    app.handle_key(key(KeyCode::Char('l')));
    app.handle_key(key(KeyCode::Char('l')));
    app.handle_key(key(KeyCode::Char('p')));
    app.handle_key(key(KeyCode::Char('p')));

    app.handle_key(key(KeyCode::Char('m')));
    assert_eq!(app.history_mode_filter.as_deref(), Some("romaji"));
    assert_eq!(app.history.len(), 1);
    let filtered = render(&app, 120, 30);
    assert!(filtered.contains("Mode: Japanese Words"), "{filtered}");

    app.handle_key(key(KeyCode::Char('m')));
    assert_eq!(app.history_mode_filter, None);
    assert_eq!(app.history.len(), 1);
}

#[test]
fn empty_history_and_no_filter_matches_have_distinct_messages() {
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
    for (lang, empty_message, filtered_message) in [
        (
            Lang::En,
            "(no past sessions yet — finish a game to record one)",
            "No sessions match the filters (change them with Tab, M, L, P)",
        ),
        (
            Lang::Ja,
            "(履歴はまだありません — ゲームを完了すると記録されます)",
            "条件に合う記録はありません（Tab・M・L・P で絞り込みを変更）",
        ),
    ] {
        let mut config = Config::default();
        config.general.lang = lang;
        let mut empty = test_app_at(config.clone(), date);
        open_history(&mut empty);
        let output = render(&empty, 120, 24);
        let contains = |message: &str| {
            if lang == Lang::Ja {
                output.replace(' ', "").contains(&message.replace(' ', ""))
            } else {
                output.contains(message)
            }
        };
        assert!(contains(empty_message), "{output}");
        assert!(!contains(filtered_message), "{output}");

        let mut filtered = test_app_at(config, date);
        save(
            &filtered,
            &record(
                Utc.with_ymd_and_hms(2026, 11, 16, 8, 0, 0).unwrap(),
                "words",
                None,
                Some("en"),
                50.0,
                0.95,
            ),
        );
        open_history(&mut filtered);
        filtered.handle_key(key(KeyCode::Char('l')));
        filtered.handle_key(key(KeyCode::Char('l')));
        let output = render(&filtered, 120, 24);
        let contains = |message: &str| {
            if lang == Lang::Ja {
                output.replace(' ', "").contains(&message.replace(' ', ""))
            } else {
                output.contains(message)
            }
        };
        assert!(contains(filtered_message), "{output}");
        assert!(!contains(empty_message), "{output}");
    }
}

#[test]
fn history_sort_emphasizes_only_active_metric_column() {
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
    let mut app = test_app_at(Config::default(), date);
    save(
        &app,
        &record(
            Utc.with_ymd_and_hms(2026, 11, 16, 8, 0, 0).unwrap(),
            "words",
            None,
            Some("en"),
            50.0,
            0.90,
        ),
    );
    save(
        &app,
        &record(
            Utc.with_ymd_and_hms(2026, 11, 16, 9, 0, 0).unwrap(),
            "words",
            None,
            Some("en"),
            70.0,
            0.98,
        ),
    );
    open_history(&mut app);
    app.handle_key(key(KeyCode::Char('s')));
    app.handle_key(key(KeyCode::Char('s')));
    let wpm = render_buffer(&app, 120, 30);
    let wpm_style = style_for(&wpm, "50.0", "50.0");
    let accuracy_style = style_for(&wpm, "50.0", "90.0%");
    assert_eq!(wpm_style.fg, Some(Color::LightCyan));
    assert!(wpm_style.add_modifier.contains(Modifier::BOLD));
    assert_eq!(accuracy_style.fg, Some(Color::White));
    assert!(!accuracy_style.add_modifier.contains(Modifier::BOLD));

    app.handle_key(key(KeyCode::Char('s')));
    let accuracy = render_buffer(&app, 120, 30);
    let wpm_style = style_for(&accuracy, "50.0", "50.0");
    let accuracy_style = style_for(&accuracy, "50.0", "90.0%");
    assert_eq!(wpm_style.fg, Some(Color::White));
    assert!(!wpm_style.add_modifier.contains(Modifier::BOLD));
    assert_eq!(accuracy_style.fg, Some(Color::LightGreen));
    assert!(accuracy_style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn bench_language_column_fits_en_slash_ja() {
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
    let mut app = test_app_at(Config::default(), date);
    let base = Utc.with_ymd_and_hms(2026, 11, 16, 8, 0, 0).unwrap();
    save(&app, &record(base, "words", Some("bench-v1/A/03"), Some("en"), 64.2, 0.98));
    save(
        &app,
        &record(
            base + chrono::Duration::minutes(1),
            "romaji",
            Some("bench-v1/C/03"),
            Some("ja"),
            42.1,
            0.96,
        ),
    );
    open_history(&mut app);
    let output = render(&app, 140, 30);
    assert!(output.contains("en/ja"), "{output}");
    assert!(!output.contains("en/…"), "{output}");
}

#[test]
fn bench_parts_group_at_thirty_minutes_but_not_thirty_one_or_across_series() {
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
    let mut app = test_app_at(Config::default(), date);
    let base = Utc.with_ymd_and_hms(2026, 11, 16, 0, 0, 0).unwrap();
    for (seconds, id, mode, wpm) in [
        (0, "bench-v1/A/03", "words", 62.1),
        (1_801, "bench-v1/B/03", "ngram", 48.0),
        (3_662, "bench-v1/C/03", "romaji", 35.2),
        (3_663, "bench-v2/A/03", "words", 70.0),
    ] {
        save(&app, &record(base + chrono::Duration::seconds(seconds), mode, Some(id), None, wpm, 0.95));
    }

    open_history(&mut app);
    assert_eq!(app.history.len(), 3, "A/B should be one run; C and bench-v2 should be separate");
    let collapsed = render(&app, 140, 30);
    assert!(collapsed.contains("A 62.1 / B 48.0"), "{collapsed}");

    app.handle_key(key(KeyCode::End));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.screen, Screen::History);
    assert!(app.history.len() > 3);
}

#[test]
fn bench_ranks_are_independent_for_each_series_and_selected_part() {
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
    let mut app = test_app_at(Config::default(), date);
    for (hour, id, mode, wpm) in [
        (6, "bench-v1/A/01", "words", 40.0),
        (7, "bench-v1/A/02", "words", 60.0),
        (8, "bench-v1/B/01", "ngram", 90.0),
        (9, "bench-v2/A/01", "words", 100.0),
    ] {
        save(
            &app,
            &record(Utc.with_ymd_and_hms(2026, 11, 16, hour, 0, 0).unwrap(), mode, Some(id), None, wpm, 0.95),
        );
    }
    open_history(&mut app);
    app.handle_key(key(KeyCode::Tab));
    app.handle_key(key(KeyCode::Tab));
    app.handle_key(key(KeyCode::Char('m')));
    app.handle_key(key(KeyCode::Char('s')));
    app.handle_key(key(KeyCode::Char('s')));
    let output = render(&app, 120, 30);
    assert_eq!(output.matches("#1").count(), 2, "each series should have its own #1:\n{output}");
    assert_eq!(output.matches("#2").count(), 1, "{output}");
    assert!(!output.contains("90.0"), "part B must be outside the selected part ranking:\n{output}");
}

#[test]
fn history_filters_combine_type_mode_language_and_period() {
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
    let mut app = test_app_at(Config::default(), date);
    for row in [
        record(
            Utc.with_ymd_and_hms(2026, 11, 16, 8, 0, 0).unwrap(),
            "words",
            None,
            Some("en"),
            50.0,
            0.95,
        ),
        record(
            Utc.with_ymd_and_hms(2026, 11, 15, 8, 0, 0).unwrap(),
            "romaji",
            None,
            Some("ja"),
            45.0,
            0.95,
        ),
        record(Utc.with_ymd_and_hms(2026, 10, 1, 8, 0, 0).unwrap(), "words", None, Some("en"), 40.0, 0.95),
        record(
            Utc.with_ymd_and_hms(2026, 11, 16, 9, 0, 0).unwrap(),
            "words",
            Some("bench-v1/A/01"),
            Some("en"),
            60.0,
            0.95,
        ),
    ] {
        save(&app, &row);
    }
    open_history(&mut app);
    assert_eq!(app.history.len(), 4);

    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.history.len(), 3);
    app.handle_key(key(KeyCode::Char('m')));
    assert_eq!(app.history.len(), 2);
    app.handle_key(key(KeyCode::Char('l')));
    assert_eq!(app.history.len(), 2);
    app.handle_key(key(KeyCode::Char('p')));
    assert_eq!(app.history.len(), 1);
    let output = render(&app, 120, 30);
    for expected in ["Practice", "Mode: Words", "en", "30 days"] {
        assert!(output.contains(expected), "missing {expected:?}:\n{output}");
    }
}

#[test]
fn ranking_requires_a_mode_and_uses_only_that_mode() {
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
    let mut app = test_app_at(Config::default(), date);
    for row in [
        record(
            Utc.with_ymd_and_hms(2026, 11, 16, 8, 0, 0).unwrap(),
            "words",
            None,
            Some("en"),
            50.0,
            0.95,
        ),
        record(
            Utc.with_ymd_and_hms(2026, 11, 16, 9, 0, 0).unwrap(),
            "words",
            None,
            Some("en"),
            70.0,
            0.95,
        ),
        record(
            Utc.with_ymd_and_hms(2026, 11, 16, 10, 0, 0).unwrap(),
            "romaji",
            None,
            Some("ja"),
            90.0,
            0.95,
        ),
    ] {
        save(&app, &row);
    }
    open_history(&mut app);
    app.handle_key(key(KeyCode::Char('s')));
    app.handle_key(key(KeyCode::Char('s')));
    let unfiltered = render(&app, 120, 30);
    assert!(!unfiltered.contains("#1"), "{unfiltered}");
    assert!(unfiltered.contains("Rank appears after selecting a mode"), "{unfiltered}");

    app.handle_key(key(KeyCode::Tab));
    app.handle_key(key(KeyCode::Char('m')));
    let ranked = render(&app, 120, 30);
    assert!(ranked.contains("#1") && ranked.contains("#2"), "{ranked}");
    assert!(!ranked.contains("90.0"), "romaji row must be filtered out:\n{ranked}");
}

#[test]
fn history_returns_to_its_entry_screen_and_detail_return_keeps_state() {
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
    let mut from_title = test_app_at(Config::default(), date);
    open_history(&mut from_title);
    from_title.handle_key(key(KeyCode::Esc));
    assert_eq!(from_title.screen, Screen::Title);

    let mut from_result = test_app_at(Config::default(), date);
    save(
        &from_result,
        &record(
            Utc.with_ymd_and_hms(2026, 11, 16, 8, 0, 0).unwrap(),
            "words",
            None,
            Some("en"),
            50.0,
            0.95,
        ),
    );
    from_result.last_result = Some(GameResult::from_metrics(&Config::default(), Default::default(), Duration::from_secs(1)));
    from_result.result_origin = ResultOrigin::JustPlayed;
    from_result.screen = Screen::Result;
    open_history(&mut from_result);
    from_result.handle_key(key(KeyCode::Tab));
    from_result.handle_key(key(KeyCode::Enter));
    assert_eq!(from_result.screen, Screen::HistoryDetail);
    from_result.handle_key(key(KeyCode::Esc));
    assert_eq!(from_result.screen, Screen::History);
    assert_eq!(from_result.history.len(), 1);
    from_result.handle_key(key(KeyCode::Esc));
    assert_eq!(from_result.screen, Screen::Result);
    assert_eq!(from_result.result_origin, ResultOrigin::JustPlayed);
}

#[test]
fn history_at_eighty_columns_omits_layout_language_and_time() {
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
    let mut app = test_app_at(Config::default(), date);
    save(
        &app,
        &record(
            Utc.with_ymd_and_hms(2026, 11, 16, 8, 0, 0).unwrap(),
            "words",
            None,
            Some("en"),
            50.0,
            0.95,
        ),
    );
    open_history(&mut app);
    let output = render(&app, 80, 24);
    let columns = output.lines().find(|line| line.contains("Finished")).unwrap();
    for required in ["Finished", "Type", "Content", "WPM", "Acc"] {
        assert!(columns.contains(required), "missing {required:?}:\n{output}");
    }
    for omitted in ["Layout", "Language", "Time"] {
        assert!(!columns.contains(omitted), "unexpected {omitted:?}:\n{output}");
    }
}

#[test]
fn history_detail_shows_conditions_best_previous_delta_and_v2_actions() {
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
    let mut app = test_app_at(Config::default(), date);
    save(
        &app,
        &record(
            Utc.with_ymd_and_hms(2026, 11, 16, 8, 0, 0).unwrap(),
            "words",
            None,
            Some("en"),
            50.0,
            0.95,
        ),
    );
    save(
        &app,
        &record(
            Utc.with_ymd_and_hms(2026, 11, 16, 9, 0, 0).unwrap(),
            "words",
            None,
            Some("en"),
            70.0,
            0.98,
        ),
    );
    open_history(&mut app);
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.screen, Screen::HistoryDetail);

    let output = render(&app, 140, 42);
    for expected in [
        "Mode: Words",
        "Language: en",
        "Practice layout: dvorak",
        "Input layout: qwerty",
        "Error mode: stop",
        "Pacer: ON",
        "Blind: OFF",
        "Keyboard: always",
        "Key filter: home",
        "Words: 3",
        "Max combo: 7",
        "Personal best: 70.0 WPM",
        "Previous: +20.0 WPM",
        "[R] Replay",
        "[E] Export",
    ] {
        assert!(output.contains(expected), "missing {expected:?}:\n{output}");
    }
}

#[test]
fn v1_history_detail_does_not_offer_replay_or_export() {
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
    let mut app = test_app_at(Config::default(), date);
    let mut imported = record(Utc.with_ymd_and_hms(2026, 11, 16, 9, 0, 0).unwrap(), "random", None, None, 35.0, 0.90);
    imported.header.metrics_version = 0;
    imported.header.legacy_payload =
        Some(serde_json::to_string(&GameResult::from_metrics(&Config::default(), Default::default(), Duration::from_secs(1))).unwrap());
    save(&app, &imported);
    open_history(&mut app);
    app.handle_key(key(KeyCode::Enter));
    let output = render(&app, 120, 36);
    assert!(!output.contains("[R] Replay"), "{output}");
    assert!(!output.contains("[E] Export"), "{output}");
}

#[test]
fn history_detail_replays_in_app_and_escape_returns_to_detail() {
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
    let mut app = test_app_at(Config::default(), date);
    save(
        &app,
        &record(
            Utc.with_ymd_and_hms(2026, 11, 16, 9, 0, 0).unwrap(),
            "words",
            None,
            Some("en"),
            70.0,
            0.98,
        ),
    );
    open_history(&mut app);
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Char('r')));
    assert_eq!(app.screen, Screen::Replay);
    assert!(render(&app, 100, 28).contains("Replay"));
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.screen, Screen::HistoryDetail);
}

#[test]
fn history_detail_exports_a_readable_ttrec_and_reports_the_path() {
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
    let mut app = test_app_at(Config::default(), date);
    let expected = record(
        Utc.with_ymd_and_hms(2026, 11, 16, 9, 0, 0).unwrap(),
        "words",
        None,
        Some("en"),
        70.0,
        0.98,
    );
    let id = save(&app, &expected);
    let export_dir = tempfile::TempDir::new().unwrap();
    app.history_export_dir = Some(export_dir.path().to_path_buf());
    open_history(&mut app);
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Char('e')));

    let path = export_dir.path().join(format!("session-{id}.ttrec"));
    let actual = ttrec::read(std::io::BufReader::new(std::fs::File::open(&path).unwrap())).unwrap();
    assert_eq!(actual.header.id, Some(id));
    let output = render(&app, 140, 42);
    assert!(output.contains(path.to_str().unwrap()), "{output}");
}
