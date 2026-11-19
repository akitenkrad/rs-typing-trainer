use std::time::Duration;

use chrono::{TimeZone, Utc};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;
use rs_typing_trainer::analysis::errors::ConfusionMetric;
use rs_typing_trainer::analysis::metrics::KeyMetric;
use rs_typing_trainer::analysis::model::KeyMapMetric;
use rs_typing_trainer::app::Screen;
use rs_typing_trainer::config::{Config, ModeKind};
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::record::{SessionRecord, db};
use rs_typing_trainer::ui;
use tempfile::TempDir;

use crate::common;
use common::{test_app, test_app_at};

/// The analysis screen opens on the last 30 days (MYTASK-3630), so tests
/// whose records carry fixed November 2026 dates pin «today» next to them
/// instead of following the wall clock.
fn as_of() -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(2026, 11, 19).unwrap()
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn save_record(path: &std::path::Path, mode: ModeKind, bench_id: Option<&str>, wpm: f64, day: u32) {
    let mut config = Config::default();
    config.mode.kind = mode;
    let layout = rs_typing_trainer::layout::builtin_layout("qwerty").unwrap();
    let mut session = Session::new(Duration::from_secs(1), Duration::ZERO, Item::exact("as"));
    session.activate(Duration::ZERO);
    session.input_char_with_code(Duration::from_millis(100), "a", layout.stroke_for('a').map(|stroke| stroke.code));
    session.input_char_with_code(Duration::from_millis(250), "s", layout.stroke_for('s').map(|stroke| stroke.code));
    session.tick(Duration::from_secs(1));
    let mut record = SessionRecord::from_log(&config, session.into_log(), Utc.with_ymd_and_hms(2026, 11, day, 0, 0, 0).unwrap());
    record.header.net_wpm = wpm;
    if let Some(id) = bench_id {
        record.header.mode = "bench".into();
        record.header.bench_id = Some(id.into());
    }
    db::save(path, &record).unwrap();
}

#[test]
fn analysis_mode_filter_includes_phrase_sessions() {
    let directory = TempDir::new().unwrap();
    let mut config = Config::default();
    config.storage.db_path = Some(directory.path().join("sessions.db"));
    let mut app = test_app_at(config, as_of());
    save_record(&app.db_path(), ModeKind::Phrase, None, 42.0, 1);
    save_record(&app.db_path(), ModeKind::Words, None, 55.0, 2);

    app.handle_key(key(KeyCode::Char('a')));
    for _ in 0..5 {
        app.handle_key(key(KeyCode::Char('m')));
    }

    assert_eq!(app.analysis_mode_filter.as_deref(), Some("phrase"));
    assert_eq!(app.analysis_records.len(), 1);
    assert_eq!(app.analysis_records[0].header.mode, "phrase");
}

fn render(app: &rs_typing_trainer::app::App) -> String {
    render_size(app, 120, 40)
}

fn render_size(app: &rs_typing_trainer::app::App, width: u16, height: u16) -> String {
    render_buffer(app, width, height)
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<Vec<_>>()
        .join("")
}

fn render_buffer(app: &rs_typing_trainer::app::App, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    terminal.backend().buffer().clone()
}

fn compact(text: &str) -> String {
    text.chars().filter(|character| !character.is_whitespace()).collect()
}

fn keyboard_analysis_app(lang: Lang) -> common::TestApp {
    let mut config = Config::default();
    config.general.lang = lang;
    let mut app = test_app(config);
    app.handle_key(key(KeyCode::Char('a')));
    app.analysis_tab = rs_typing_trainer::app::ANALYSIS_KEYBOARD_TAB;
    app.analysis_details.keys = vec![
        KeyMetric {
            key: "a".into(),
            code: "KeyA".into(),
            correct: 3,
            total: 4,
            accuracy: 0.75,
            iki_n: 3,
            iki_median_ms: 240.0,
            iki_p90_ms: 260.0,
        },
        KeyMetric {
            key: "s".into(),
            code: "KeyS".into(),
            correct: 9,
            total: 10,
            accuracy: 0.9,
            iki_n: 9,
            iki_median_ms: 120.0,
            iki_p90_ms: 150.0,
        },
    ];
    let report = app.analysis_coach.as_mut().expect("analysis coach report");
    report.map.keys = vec![KeyMapMetric {
        key: "a".into(),
        relative_iki: 1.5,
        effective_n: 8.0,
    }];
    report.diagnosis.errors.confusion = vec![ConfusionMetric {
        expected: "a".into(),
        typed: "s".into(),
        n: 7,
    }];
    app
}

fn row_text(row: &[ratatui::buffer::Cell]) -> String {
    row.iter().map(|cell| cell.symbol()).collect()
}

fn heatmap_band(row: &[ratatui::buffer::Cell]) -> Vec<&ratatui::buffer::Cell> {
    row.iter()
        .filter(|cell| {
            matches!(
                cell.bg,
                Color::Rgb(red, green, 20) if (40..=220).contains(&red) && green <= 20
            )
        })
        .collect()
}

#[test]
fn keyboard_heatmap_legends_show_real_values_in_english_and_japanese() {
    for (lang, expected_key, no_data, times, fast, slow, personal_median) in [
        (Lang::En, "expected key", "no data", "7 times", "fast", "slow", "personal median"),
        (Lang::Ja, "期待したキー", "データなし", "7 回", "速い", "遅い", "本人の中央値"),
    ] {
        let mut app = keyboard_analysis_app(lang);
        for (view, expected_values) in [
            (0, vec!["0%", "25.0%"]),
            (1, vec!["0 ms", "240 ms"]),
            (2, vec!["0.0", "1.0", "2.0+"]),
            (3, vec!["0", times]),
        ] {
            app.analysis_keyboard_view = view;
            let buffer = render_buffer(&app, 120, 40);
            let rows = buffer.content().chunks(120).collect::<Vec<_>>();
            let legend = rows
                .iter()
                .find(|row| heatmap_band(row).len() >= 12 && row_text(row).contains(expected_values[0]))
                .unwrap_or_else(|| panic!("view {view} has no legend"));
            let text = row_text(legend);
            let normalized = compact(&text);
            for value in expected_values {
                assert!(normalized.contains(&compact(value)), "view {view}, lang {lang:?}: {text}");
            }
            assert!(normalized.contains(&compact(no_data)), "view {view}, lang {lang:?}: {text}");
            if view == 2 {
                for label in [fast, slow, personal_median] {
                    assert!(normalized.contains(&compact(label)), "view {view}, lang {lang:?}: {text}");
                }
                let note = match lang {
                    Lang::En => "(1.0 = personal median)",
                    Lang::Ja => "(1.0 = 本人の中央値)",
                };
                let note = compact(note);
                let slow = compact(slow);
                assert!(normalized.contains(&note), "view {view}, lang {lang:?}: {text}");
                assert!(
                    normalized.find(&slow).unwrap() < normalized.find(&note).unwrap(),
                    "view {view}, lang {lang:?}: {text}"
                );
            }
            if view == 3 {
                assert!(normalized.contains(&compact(expected_key)), "view {view}, lang {lang:?}: {text}");
                assert!(legend.iter().any(|cell| cell.bg == Color::Cyan), "view {view}, lang {lang:?}: {text}");
            }
        }
    }
}

#[test]
fn every_keyboard_heatmap_legend_uses_the_shared_colour_scale_endpoints() {
    let mut app = keyboard_analysis_app(Lang::En);
    for view in 0..4 {
        app.analysis_keyboard_view = view;
        let buffer = render_buffer(&app, 120, 40);
        let legend = buffer
            .content()
            .chunks(120)
            .find(|row| heatmap_band(row).len() >= 12)
            .unwrap_or_else(|| panic!("view {view} has no colour band"));
        let band = heatmap_band(legend);
        assert_eq!(band.len(), 16, "view {view}: {}", row_text(legend));
        assert_eq!(band.first().unwrap().bg, Color::Rgb(40, 20, 20), "view {view}");
        assert_eq!(band.last().unwrap().bg, Color::Rgb(220, 0, 20), "view {view}");
    }
}

#[test]
fn narrow_keyboard_legend_drops_no_data_then_median_note_before_words_and_colour_band() {
    let mut app = keyboard_analysis_app(Lang::En);
    app.analysis_keyboard_view = 2;

    let medium = render_buffer(&app, 55, 40);
    let medium_row = medium
        .content()
        .chunks(55)
        .find(|row| heatmap_band(row).len() >= 12)
        .expect("medium-width legend");
    let medium_text = row_text(medium_row);
    assert!(!medium_text.contains("no data"), "{medium_text}");
    assert!(medium_text.contains("fast"), "{medium_text}");
    assert!(!medium_text.contains("personal median"), "{medium_text}");
    assert_eq!(heatmap_band(medium_row).len(), 16, "{medium_text}");

    let narrow = render_buffer(&app, 47, 40);
    let narrow_row = narrow
        .content()
        .chunks(47)
        .find(|row| heatmap_band(row).len() >= 12)
        .expect("narrow legend");
    let narrow_text = row_text(narrow_row);
    assert!(!narrow_text.contains("no data"), "{narrow_text}");
    assert!(narrow_text.contains("fast"), "{narrow_text}");
    assert!(!narrow_text.contains("personal median"), "{narrow_text}");
    assert_eq!(heatmap_band(narrow_row).len(), 16, "{narrow_text}");

    app.analysis_keyboard_view = 1;
    app.analysis_details.keys[0].iki_median_ms = 123_456_789_012_345_678_901_234_567_890.0;
    let minimum = render_buffer(&app, 47, 40);
    let minimum_row = minimum
        .content()
        .chunks(47)
        .find(|row| heatmap_band(row).len() >= 12)
        .expect("minimum-length colour band");
    let minimum_text = row_text(minimum_row);
    assert!(!minimum_text.contains("no data"), "{minimum_text}");
    assert!(!minimum_text.contains("ms"), "{minimum_text}");
    assert_eq!(heatmap_band(minimum_row).len(), 12, "{minimum_text}");
}

#[test]
fn keyboard_legend_is_omitted_when_only_the_keyboard_height_fits() {
    let mut app = keyboard_analysis_app(Lang::Ja);
    app.analysis_keyboard_view = 0;
    let buffer = render_buffer(&app, 120, 16);
    assert!(
        buffer.content().chunks(120).all(|row| heatmap_band(row).len() < 12),
        "legend should not replace keyboard rows"
    );
}

#[test]
fn every_analysis_tab_renders_and_bench_shows_previous_delta() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    save_record(&path, ModeKind::Words, None, 30.0, 1);
    save_record(&path, ModeKind::Words, Some("bench-v1/A/01"), 40.0, 2);
    save_record(&path, ModeKind::Words, Some("bench-v1/A/02"), 50.0, 3);
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    config.storage.db_path = Some(path);
    let mut app = test_app_at(config, as_of());
    app.handle_key(key(KeyCode::Char('a')));
    assert_eq!(app.screen, Screen::Analysis);

    // MYTASK-3631 puts «今日» first; the screen still opens on «コーチ».
    // M6 adds the «スキル» tab (§8.4) after «ベンチ»: eight tabs in all.
    assert_eq!(app.analysis_tab, rs_typing_trainer::app::ANALYSIS_COACH_TAB);
    for expected in ["コーチ", "推移", "キーボード", "遷移", "指・手", "ベンチ", "スキル", "今日"] {
        let output = render(&app);
        let compact = output.chars().filter(|character| !character.is_whitespace()).collect::<String>();
        assert!(compact.contains(expected), "tab {expected}: {output}");
        app.handle_key(key(KeyCode::Tab));
    }
    // Back at the coach tab after a full cycle; five more presses reach «ベンチ».
    assert_eq!(app.analysis_tab, rs_typing_trainer::app::ANALYSIS_COACH_TAB);
    for _ in 0..5 {
        app.handle_key(key(KeyCode::Tab));
    }
    let bench = render(&app);
    let compact = bench.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    assert!(compact.contains("前回比"), "{bench}");
    assert!(compact.contains("直近3回"), "{bench}");
}

#[test]
fn analysis_bench_tab_combines_v1_and_v2_for_part_b() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    save_record(&path, ModeKind::Ngram, Some("bench-v1/B/01"), 40.0, 1);
    save_record(&path, ModeKind::Ngram, Some("bench-v2/B/01"), 52.0, 2);
    let mut config = Config::default();
    config.storage.db_path = Some(path);
    let mut app = test_app_at(config, as_of());
    app.handle_key(key(KeyCode::Char('a')));
    app.analysis_tab = rs_typing_trainer::app::ANALYSIS_BENCH_TAB;

    let output = render(&app);
    let compact = output.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    assert!(compact.contains("+12.0WPM"), "{output}");
}

#[test]
fn selected_transition_creates_an_ngram_practice_configuration() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    for day in 1..=5 {
        save_record(&path, ModeKind::Words, None, 30.0, day);
    }
    let mut config = Config::default();
    config.storage.db_path = Some(path);
    let mut app = test_app_at(config, as_of());
    app.handle_key(key(KeyCode::Char('a')));
    app.handle_key(key(KeyCode::Tab));
    app.handle_key(key(KeyCode::Tab));
    app.handle_key(key(KeyCode::Tab));
    app.handle_key(key(KeyCode::Enter));

    assert_eq!(app.config.mode.kind, ModeKind::Ngram);
    assert_eq!(app.config.mode.ngram.n, 2);
    assert_eq!(app.config.mode.ngram.source, "pairs");
    assert_eq!(app.config.mode.keys, "as");
    assert_eq!(app.screen, Screen::KeyPicker);
}

#[test]
fn sparse_transition_is_shown_as_insufficient_and_cannot_start_a_drill() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    save_record(&path, ModeKind::Words, None, 30.0, 1);
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    config.storage.db_path = Some(path);
    let mut app = test_app_at(config, as_of());
    app.handle_key(key(KeyCode::Char('a')));
    for _ in 0..3 {
        app.handle_key(key(KeyCode::Tab));
    }

    assert!(compact(&render(&app)).contains("データ不足（n<5）"));
    app.handle_key(key(KeyCode::Enter));
    assert_ne!(app.config.mode.kind, ModeKind::Ngram);
}

#[test]
fn result_screen_shows_benchmark_comparisons_from_prior_runs() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    save_record(&path, ModeKind::Words, Some("bench-v1/A/01"), 40.0, 1);
    save_record(&path, ModeKind::Words, Some("bench-v1/A/02"), 50.0, 2);
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    config.storage.db_path = Some(path.clone());
    let mut app = test_app(config.clone());
    app.bench_comparison = Some(db::bench_comparison(&path, "bench-v1/A/03", 60.0, 1.0).unwrap());
    app.last_result = Some(rs_typing_trainer::metrics::stats::GameResult::from_metrics(
        &config,
        Default::default(),
        Duration::from_secs(60),
    ));
    app.screen = Screen::Result;
    let output = render(&app);
    let compact = output.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    assert!(compact.contains("前回比"), "{output}");
    assert!(compact.contains("直近3回の中央値との差"), "{output}");
}

#[test]
fn japanese_analysis_tabs_do_not_contain_english_ui_phrases() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    save_record(&path, ModeKind::Words, None, 30.0, 1);
    save_record(&path, ModeKind::Words, Some("bench-v1/A/01"), 40.0, 2);
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    config.storage.db_path = Some(path);
    let mut app = test_app(config);
    app.handle_key(key(KeyCode::Char('a')));

    let forbidden = [
        "net WPM / daily median",
        "accuracy % / daily median",
        "IKI ms / daily median",
        "day",
        "slow top 20",
        "[Enter] practice",
        "error-prone top 20",
        "error",
        "bench-v1 / 8 skill axes",
        "raw (fewer than 3 results)",
        "[Tab/←→] tab",
        "[P] period",
        "[M] mode: all",
        "[ESC] back",
        "Error-rate heatmap",
        "same-finger",
        "alternating hands",
    ];
    for tab in 0..7 {
        let output = render(&app);
        for phrase in forbidden {
            assert!(!output.contains(phrase), "tab {tab} contains {phrase:?}: {output}");
        }
        app.handle_key(key(KeyCode::Tab));
    }
}

#[test]
fn japanese_benchmark_selection_and_result_are_localized() {
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    let mut app = test_app(config.clone());
    app.handle_key(key(KeyCode::Char('b')));
    let selection = render(&app);
    for phrase in ["BENCHMARK", "English words", "coverage", "Japanese", "Numbers/symbols", "Toggle", "Back"] {
        assert!(!selection.contains(phrase), "{phrase:?}: {selection}");
    }

    app.handle_key(key(KeyCode::Enter));
    app.last_result = Some(rs_typing_trainer::metrics::stats::GameResult::from_metrics(
        &config,
        Default::default(),
        Duration::from_secs(60),
    ));
    app.screen = Screen::Result;
    let result = render(&app);
    for phrase in ["Previous", "recent-3 median", "Restart", "History", "Quit"] {
        assert!(!result.contains(phrase), "{phrase:?}: {result}");
    }
}

#[test]
fn trend_needs_two_valid_days_and_shows_current_value() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    save_record(&path, ModeKind::Words, None, 30.0, 1);
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    config.storage.db_path = Some(path);
    let mut app = test_app_at(config, as_of());
    app.handle_key(key(KeyCode::Char('a')));
    app.handle_key(key(KeyCode::Tab));

    let output = render(&app);
    assert!(compact(&output).contains("データが2日分たまると推移を表示します"), "{output}");
    assert!(output.contains("30.0"), "{output}");
}

#[test]
fn trend_labels_first_last_dates_and_value_range() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    save_record(&path, ModeKind::Words, None, 30.0, 1);
    save_record(&path, ModeKind::Words, None, 50.0, 3);
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    config.storage.db_path = Some(path);
    let mut app = test_app_at(config, as_of());
    app.handle_key(key(KeyCode::Char('a')));
    app.handle_key(key(KeyCode::Tab));

    let output = render(&app);
    assert!(output.contains("2026-11-01"), "{output}");
    assert!(output.contains("2026-11-03"), "{output}");
    assert!(output.contains("30.0"), "{output}");
    assert!(output.contains("50.0"), "{output}");
}

#[test]
fn legacy_sessions_are_excluded_from_iki_trend() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    save_record(&path, ModeKind::Words, None, 30.0, 1);
    save_record(&path, ModeKind::Words, None, 50.0, 3);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE sessions SET metrics_version = 0, iki_median_ms = 9876.0, iki_cv = 9.876 WHERE id = (SELECT min(id) FROM sessions)",
            [],
        )
        .unwrap();
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    config.storage.db_path = Some(path);
    let mut app = test_app(config);
    app.handle_key(key(KeyCode::Char('a')));
    app.handle_key(key(KeyCode::Tab));

    let output = render(&app);
    assert!(!output.contains("9876"), "{output}");
    assert!(compact(&output).matches("データが2日分たまると推移を表示します").count() >= 1, "{output}");
}

#[test]
fn unavailable_skill_axes_show_no_data_in_aligned_columns() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    save_record(&path, ModeKind::Words, Some("bench-v1/A/01"), 40.0, 2);
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    config.storage.db_path = Some(path);
    let mut app = test_app(config);
    app.handle_key(key(KeyCode::Char('a')));
    for _ in 0..5 {
        app.handle_key(key(KeyCode::Tab));
    }

    let output = compact(&render(&app));
    for axis in ["遷移", "到達", "日本語"] {
        assert!(output.contains(&format!("{axis}—（データなし）")), "{axis}: {output}");
    }
    assert!(!output.contains("遷移0.00"), "{output}");
    assert!(!output.contains("到達0.00"), "{output}");
    assert!(!output.contains("日本語0.00"), "{output}");
}

#[test]
fn coach_is_the_first_tab_and_scrolls_in_an_80_by_24_terminal() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    save_record(&path, ModeKind::Words, None, 30.0, 14);
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    config.storage.db_path = Some(path);
    let mut app = test_app(config);
    app.handle_key(key(KeyCode::Char('a')));

    let first = render_size(&app, 80, 24);
    assert!(compact(&first).contains("コーチ"), "{first}");
    assert!(compact(&first).contains("今の位置"), "{first}");
    assert!(compact(&first).contains("[P]期間"), "{first}");

    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.analysis_summary_scroll, 1);
    let scrolled = render_size(&app, 80, 24);
    assert_ne!(first, scrolled);
}
