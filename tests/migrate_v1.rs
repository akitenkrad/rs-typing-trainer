use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::config::Config;
use rs_typing_trainer::metrics::stats::{GameResult, LiveMetrics};
use rs_typing_trainer::record::db;
use rs_typing_trainer::ui;
use tempfile::TempDir;

mod common;
use common::test_app;

const V1_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    finished_at TEXT NOT NULL,
    duration_ms INTEGER NOT NULL,
    wpm REAL NOT NULL,
    cps REAL NOT NULL,
    accuracy REAL NOT NULL,
    consistency REAL NOT NULL,
    max_combo INTEGER NOT NULL,
    completed_words INTEGER NOT NULL,
    timeout_words INTEGER NOT NULL,
    mistypes_total INTEGER NOT NULL,
    payload TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_sessions_finished_at
    ON sessions(finished_at DESC);
"#;

const P1_V2_SCHEMA: &str = r#"
CREATE TABLE sessions (
  id INTEGER PRIMARY KEY,
  started_at TEXT NOT NULL,
  finished_at TEXT NOT NULL,
  duration_ms INTEGER NOT NULL,
  mode TEXT NOT NULL,
  mode_params TEXT NOT NULL,
  bench_id TEXT,
  layout TEXT NOT NULL,
  input_layout TEXT NOT NULL,
  geometry TEXT NOT NULL,
  error_mode TEXT NOT NULL,
  release_events INTEGER NOT NULL,
  app_version TEXT NOT NULL,
  metrics_version INTEGER NOT NULL,
  net_wpm REAL NOT NULL,
  raw_wpm REAL NOT NULL,
  accuracy REAL NOT NULL,
  iki_median_ms REAL NOT NULL,
  iki_cv REAL NOT NULL,
  cps REAL NOT NULL,
  consistency REAL NOT NULL,
  max_combo INTEGER NOT NULL,
  completed_words INTEGER NOT NULL,
  timeout_words INTEGER NOT NULL,
  mistypes_total INTEGER NOT NULL,
  legacy_payload TEXT
);
PRAGMA user_version = 2;
"#;

fn legacy_result() -> GameResult {
    let metrics = LiveMetrics {
        completed_words: 25,
        correct_chars: 77,
        total_chars_typed: 80,
        ..LiveMetrics::default()
    };
    GameResult::from_metrics(&Config::default(), metrics, Duration::new(45, 12_800_000))
}

fn assert_new_metrics(path: &std::path::Path) -> (f64, f64) {
    let conn = rusqlite::Connection::open(path).unwrap();
    let (net_wpm, cps, raw_wpm): (f64, f64, Option<f64>) = conn
        .query_row("SELECT net_wpm, cps, raw_wpm FROM sessions WHERE metrics_version = 0", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap();
    assert!((net_wpm - 20.53).abs() <= 0.01, "net_wpm={net_wpm}");
    assert!((cps - 77.0 / 45.0128).abs() < 1e-9, "cps={cps}");
    assert_eq!(raw_wpm, None);
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0)).unwrap(),
        db::SCHEMA_VERSION
    );
    (net_wpm, cps)
}

#[test]
fn v1_rows_are_migrated_once_and_keep_legacy_results() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("sessions.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(V1_SCHEMA).unwrap();

    let mut originals = Vec::new();
    for n in 1..=3 {
        let metrics = LiveMetrics {
            completed_words: n,
            correct_chars: n * 3,
            total_chars_typed: n * 3 + 1,
            ..LiveMetrics::default()
        };
        let result = GameResult::from_metrics(&Config::default(), metrics, Duration::from_secs(10 + n as u64));
        let payload = serde_json::to_string(&result).unwrap();
        conn.execute(
            "INSERT INTO sessions (finished_at, duration_ms, wpm, cps, accuracy, consistency, max_combo, completed_words, timeout_words, mistypes_total, payload) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                result.finished_at.to_rfc3339(),
                result.total_duration.as_millis() as i64,
                result.wpm,
                result.cps,
                result.accuracy,
                result.consistency,
                result.max_combo,
                result.metrics.completed_words,
                result.metrics.timeout_words,
                result.metrics.mistypes_total,
                payload,
            ],
        )
        .unwrap();
        originals.push(result);
    }
    drop(conn);

    assert_eq!(db::list(&path).unwrap().len(), 3);
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM sessions_v1", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM sessions WHERE metrics_version = 0", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        3
    );
    drop(conn);

    assert_eq!(db::list(&path).unwrap().len(), 3, "2 回目の open で重複しない");
    for (index, original) in originals.iter().enumerate() {
        let loaded = db::load_result(&path, index as i64 + 1, &Config::default()).unwrap().unwrap();
        assert_eq!(loaded.metrics.completed_words, original.metrics.completed_words);
        assert_eq!(loaded.total_duration, original.total_duration);
    }
}

#[test]
fn v1_import_recomputes_wpm_and_cps_from_correct_characters() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("sessions.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(V1_SCHEMA).unwrap();
    let result = legacy_result();
    let payload = serde_json::to_string(&result).unwrap();
    conn.execute(
        "INSERT INTO sessions (finished_at, duration_ms, wpm, cps, accuracy, consistency, max_combo, completed_words, timeout_words, mistypes_total, payload) VALUES (?1, ?2, 33.3, 9.9, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            result.finished_at.to_rfc3339(),
            result.total_duration.as_millis() as i64,
            result.accuracy,
            result.consistency,
            result.max_combo,
            result.metrics.completed_words,
            result.metrics.timeout_words,
            result.metrics.mistypes_total,
            payload,
        ],
    )
    .unwrap();
    drop(conn);

    db::list(&path).unwrap();
    assert_new_metrics(&path);
    let loaded = db::load_result(&path, 1, &Config::default()).unwrap().unwrap();
    assert!((loaded.wpm - 20.53).abs() <= 0.01);
    assert!((loaded.cps - 77.0 / 45.0128).abs() < 1e-9);
}

#[test]
fn opening_p1_v2_database_recomputes_once_and_is_idempotent() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("sessions.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(P1_V2_SCHEMA).unwrap();
    let result = legacy_result();
    let payload = serde_json::to_string(&result).unwrap();
    conn.execute(
        "INSERT INTO sessions VALUES (1, ?1, ?2, ?3, 'random', '{}', NULL, 'unknown', 'unknown', 'unknown', 'stop', 0, '0.1.0', 0, 33.3, 33.3, ?4, 0.0, 0.0, 9.9, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            result.finished_at.to_rfc3339(),
            result.finished_at.to_rfc3339(),
            result.total_duration.as_millis() as i64,
            result.accuracy,
            result.consistency,
            result.max_combo,
            result.metrics.completed_words,
            result.metrics.timeout_words,
            result.metrics.mistypes_total,
            payload,
        ],
    )
    .unwrap();
    drop(conn);

    db::list(&path).unwrap();
    let first = assert_new_metrics(&path);
    db::list(&path).unwrap();
    let second = assert_new_metrics(&path);
    assert_eq!(first, second);
}

#[test]
fn legacy_history_detail_shows_new_wpm_and_old_definition_note() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("sessions.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(V1_SCHEMA).unwrap();
    let result = legacy_result();
    let payload = serde_json::to_string(&result).unwrap();
    conn.execute(
        "INSERT INTO sessions (finished_at, duration_ms, wpm, cps, accuracy, consistency, max_combo, completed_words, timeout_words, mistypes_total, payload) VALUES (?1, ?2, 33.3, 9.9, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            result.finished_at.to_rfc3339(),
            result.total_duration.as_millis() as i64,
            result.accuracy,
            result.consistency,
            result.max_combo,
            result.metrics.completed_words,
            result.metrics.timeout_words,
            result.metrics.mistypes_total,
            payload,
        ],
    )
    .unwrap();
    drop(conn);

    let mut config = Config::default();
    config.storage.db_path = Some(path);
    let mut app = test_app(config);
    app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!((app.last_result.as_ref().unwrap().wpm - 20.53).abs() <= 0.01);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<Vec<_>>()
        .join("");
    assert!(rendered.contains("v1 record (old WPM definition: 33.3)"), "{rendered}");
}
