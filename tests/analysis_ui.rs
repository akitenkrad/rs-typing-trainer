use std::time::Duration;

use chrono::{TimeZone, Utc};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::app::Screen;
use rs_typing_trainer::config::{Config, ModeKind};
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::record::{SessionRecord, db};
use rs_typing_trainer::ui;
use tempfile::TempDir;

mod common;
use common::test_app;

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

fn render(app: &rs_typing_trainer::app::App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
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

fn compact(text: &str) -> String {
    text.chars().filter(|character| !character.is_whitespace()).collect()
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
    let mut app = test_app(config);
    app.handle_key(key(KeyCode::Char('a')));
    assert_eq!(app.screen, Screen::Analysis);

    for expected in ["推移", "キーボード", "遷移", "指・手", "ベンチ"] {
        let output = render(&app);
        let compact = output.chars().filter(|character| !character.is_whitespace()).collect::<String>();
        assert!(compact.contains(expected), "tab {expected}: {output}");
        app.handle_key(key(KeyCode::Tab));
    }
    for _ in 0..4 {
        app.handle_key(key(KeyCode::Tab));
    }
    let bench = render(&app);
    let compact = bench.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    assert!(compact.contains("前回比"), "{bench}");
    assert!(compact.contains("直近3回"), "{bench}");
}

#[test]
fn selected_transition_creates_an_ngram_practice_configuration() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    save_record(&path, ModeKind::Words, None, 30.0, 1);
    let mut config = Config::default();
    config.storage.db_path = Some(path);
    let mut app = test_app(config);
    app.handle_key(key(KeyCode::Char('a')));
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
    for tab in 0..5 {
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
    let mut app = test_app(config);
    app.handle_key(key(KeyCode::Char('a')));

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
    let mut app = test_app(config);
    app.handle_key(key(KeyCode::Char('a')));

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
    for _ in 0..4 {
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
