//! SQLite storage format v2 and transactional v1 migration.

use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Local};
use rand::RngCore;
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::config::Config;
use crate::engine::session::{ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};
use crate::error::{AppError, AppResult};
use crate::metrics::stats::GameResult;
use crate::record::{SessionHeader, SessionRecord};

pub const SCHEMA_VERSION: i64 = 5;

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
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
  raw_wpm REAL,
  accuracy REAL NOT NULL,
  score_eligible INTEGER NOT NULL DEFAULT 1,
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
CREATE TABLE IF NOT EXISTS items (
  session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  idx INTEGER NOT NULL,
  text TEXT NOT NULL,
  reading TEXT,
  shown_us INTEGER NOT NULL,
  done_us INTEGER,
  outcome TEXT NOT NULL,
  PRIMARY KEY (session_id, idx)
);
CREATE TABLE IF NOT EXISTS keystrokes (
  session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  seq INTEGER NOT NULL,
  t_us INTEGER NOT NULL,
  release_us INTEGER,
  item_idx INTEGER NOT NULL,
  pos INTEGER NOT NULL,
  kind TEXT NOT NULL,
  typed TEXT,
  expected TEXT,
  correct INTEGER NOT NULL,
  code TEXT,
  PRIMARY KEY (session_id, seq)
);
CREATE INDEX IF NOT EXISTS idx_sessions_finished_at_v2 ON sessions(finished_at DESC);
CREATE TABLE IF NOT EXISTS meta (
  key TEXT PRIMARY KEY,
  value BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS item_exposure (
  kind TEXT NOT NULL,
  item_key TEXT NOT NULL,
  count INTEGER NOT NULL,
  last_shown TEXT NOT NULL,
  PRIMARY KEY (kind, item_key)
);
CREATE TABLE IF NOT EXISTS item_mistake (
  kind TEXT NOT NULL,
  item_key TEXT NOT NULL,
  last_mistake TEXT NOT NULL,
  PRIMARY KEY (kind, item_key)
);
CREATE TABLE IF NOT EXISTS unlock_state (
  layout TEXT PRIMARY KEY,
  unlocked TEXT NOT NULL,
  streak INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS adaptive_focus (
  day TEXT NOT NULL,
  layout TEXT NOT NULL,
  transitions TEXT NOT NULL,
  PRIMARY KEY (day, layout)
);
"#;

#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub id: i64,
    pub finished_at: DateTime<Local>,
    pub duration: Duration,
    pub wpm: f64,
    pub accuracy: f64,
    pub max_combo: u32,
    pub completed_words: u32,
    pub metrics_version: i64,
}

pub fn save(path: &Path, record: &SessionRecord) -> AppResult<i64> {
    let mut conn = open(path)?;
    let tx = conn.transaction()?;
    let h = &record.header;
    let score_eligible = serde_json::from_str::<serde_json::Value>(&h.mode_params)
        .ok()
        .and_then(|value| value.get("score_eligible").and_then(serde_json::Value::as_bool))
        .unwrap_or(true);
    tx.execute(
        "INSERT INTO sessions (started_at, finished_at, duration_ms, mode, mode_params, bench_id, layout, input_layout, geometry, error_mode, release_events, app_version, metrics_version, net_wpm, raw_wpm, accuracy, score_eligible, iki_median_ms, iki_cv, cps, consistency, max_combo, completed_words, timeout_words, mistypes_total, legacy_payload) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26)",
        params![
            h.started_at,
            h.finished_at,
            h.duration_ms,
            h.mode,
            h.mode_params,
            h.bench_id,
            h.layout,
            h.input_layout,
            h.geometry,
            h.error_mode,
            i64::from(h.release_events),
            h.app_version,
            h.metrics_version,
            h.net_wpm,
            h.raw_wpm,
            h.accuracy,
            i64::from(score_eligible),
            h.iki_median_ms,
            h.iki_cv,
            h.cps,
            h.consistency,
            i64::from(h.max_combo),
            i64::from(h.completed_words),
            i64::from(h.timeout_words),
            i64::from(h.mistypes_total),
            h.legacy_payload,
        ],
    )?;
    let id = tx.last_insert_rowid();
    for item in &record.log.items {
        tx.execute(
            "INSERT INTO items (session_id, idx, text, reading, shown_us, done_us, outcome) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                i64::from(item.idx),
                item.text,
                item.reading,
                as_i64(item.shown_us),
                item.done_us.map(as_i64),
                item_outcome(item.outcome),
            ],
        )?;
    }
    for key in &record.log.keystrokes {
        tx.execute(
            "INSERT INTO keystrokes (session_id, seq, t_us, release_us, item_idx, pos, kind, typed, expected, correct, code) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                id,
                i64::from(key.seq),
                as_i64(key.t_us),
                key.release_us.map(as_i64),
                i64::from(key.item_idx),
                i64::from(key.pos),
                key_kind(key.kind),
                key.typed,
                key.expected,
                i64::from(key.correct),
                key.code,
            ],
        )?;
    }
    tx.commit()?;
    Ok(id)
}

pub fn list(path: &Path) -> AppResult<Vec<SessionSummary>> {
    let conn = open(path)?;
    let mut stmt = conn.prepare(
        "SELECT id, finished_at, duration_ms, net_wpm, accuracy, max_combo, completed_words, metrics_version FROM sessions ORDER BY datetime(finished_at) DESC, id DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        let raw: String = row.get(1)?;
        Ok(SessionSummary {
            id: row.get(0)?,
            finished_at: parse_local(&raw),
            duration: Duration::from_millis(row.get::<_, i64>(2)?.max(0) as u64),
            wpm: row.get(3)?,
            accuracy: row.get(4)?,
            max_combo: row.get::<_, i64>(5)?.max(0) as u32,
            completed_words: row.get::<_, i64>(6)?.max(0) as u32,
            metrics_version: row.get(7)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn ranked_sessions(path: &Path) -> AppResult<Vec<SessionSummary>> {
    let conn = open(path)?;
    let mut stmt = conn.prepare(
        "SELECT id, finished_at, duration_ms, net_wpm, accuracy, max_combo, completed_words, metrics_version \
         FROM sessions WHERE score_eligible = 1 ORDER BY net_wpm DESC, datetime(finished_at) DESC, id DESC",
    )?;
    let rows = stmt.query_map([], summary_from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn personal_best(path: &Path, mode: &str) -> AppResult<Option<SessionSummary>> {
    let conn = open(path)?;
    conn.query_row(
        "SELECT id, finished_at, duration_ms, net_wpm, accuracy, max_combo, completed_words, metrics_version \
         FROM sessions WHERE score_eligible = 1 AND mode = ?1 ORDER BY net_wpm DESC, datetime(finished_at) DESC, id DESC LIMIT 1",
        params![mode],
        summary_from_row,
    )
    .optional()
    .map_err(Into::into)
}

fn summary_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionSummary> {
    let raw: String = row.get(1)?;
    Ok(SessionSummary {
        id: row.get(0)?,
        finished_at: parse_local(&raw),
        duration: Duration::from_millis(row.get::<_, i64>(2)?.max(0) as u64),
        wpm: row.get(3)?,
        accuracy: row.get(4)?,
        max_combo: row.get::<_, i64>(5)?.max(0) as u32,
        completed_words: row.get::<_, i64>(6)?.max(0) as u32,
        metrics_version: row.get(7)?,
    })
}

pub fn latest_bench_parallel(path: &Path, series: &str) -> AppResult<Option<u8>> {
    let conn = open(path)?;
    let pattern = format!("{series}/%/%");
    let value = conn
        .query_row(
            "SELECT bench_id FROM sessions WHERE mode = 'bench' AND bench_id LIKE ?1 ORDER BY datetime(finished_at) DESC, id DESC LIMIT 1",
            params![pattern],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(value.and_then(|id| id.rsplit('/').next()?.parse::<u8>().ok()))
}

pub fn bench_comparison(path: &Path, bench_id: &str, current_wpm: f64, current_accuracy: f64) -> AppResult<crate::bench::BenchComparison> {
    let Some((prefix, _)) = bench_id.rsplit_once('/') else {
        return Ok(crate::bench::BenchComparison::default());
    };
    let conn = open(path)?;
    let pattern = format!("{prefix}/%");
    let rows = {
        let mut statement =
            conn.prepare("SELECT net_wpm, accuracy FROM sessions WHERE mode = 'bench' AND bench_id LIKE ?1 ORDER BY datetime(finished_at), id")?;
        statement
            .query_map(params![pattern], |row| Ok((row.get::<_, f64>(0)?, row.get::<_, f64>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?
    };
    let previous = rows.last().copied();
    let recent = rows.iter().rev().take(3).copied().collect::<Vec<_>>();
    let recent_wpm = (!recent.is_empty()).then(|| median_f64(recent.iter().map(|row| row.0).collect()));
    let recent_accuracy = (!recent.is_empty()).then(|| median_f64(recent.iter().map(|row| row.1).collect()));
    Ok(crate::bench::BenchComparison {
        previous_wpm_delta: previous.map(|row| current_wpm - row.0),
        previous_accuracy_delta: previous.map(|row| current_accuracy - row.1),
        recent_three_wpm_delta: recent_wpm.map(|value| current_wpm - value),
        recent_three_accuracy_delta: recent_accuracy.map(|value| current_accuracy - value),
    })
}

fn median_f64(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

pub fn load_record(path: &Path, id: i64) -> AppResult<Option<SessionRecord>> {
    let conn = open(path)?;
    load_record_from(&conn, id)
}

pub fn load_records_filtered(path: &Path, since: Option<chrono::NaiveDate>, mode: Option<&str>) -> AppResult<Vec<SessionRecord>> {
    let conn = open(path)?;
    let ids = {
        let mut statement = conn.prepare("SELECT id FROM sessions ORDER BY datetime(finished_at), id")?;
        statement.query_map([], |row| row.get::<_, i64>(0))?.collect::<Result<Vec<_>, _>>()?
    };
    let mut records = Vec::new();
    for id in ids {
        let Some(record) = load_record_from(&conn, id)? else { continue };
        let date_matches = since.is_none_or(|minimum| {
            DateTime::parse_from_rfc3339(&record.header.finished_at)
                .map(|value| value.date_naive() >= minimum)
                .unwrap_or(false)
        });
        let mode_matches = mode.is_none_or(|expected| record.header.mode == expected);
        if date_matches && mode_matches {
            records.push(record);
        }
    }
    Ok(records)
}

pub fn load_result(path: &Path, id: i64, config: &Config) -> AppResult<Option<GameResult>> {
    let Some(record) = load_record(path, id)? else {
        return Ok(None);
    };
    if record.header.metrics_version == 0 {
        let payload = record
            .header
            .legacy_payload
            .ok_or_else(|| AppError::Record(format!("legacy session {id} has no payload")))?;
        let mut result: GameResult = serde_json::from_str(&payload)?;
        result.wpm = record.header.net_wpm;
        result.cps = record.header.cps;
        return Ok(Some(result));
    }
    let mut result = GameResult::from_log(config, &record.log);
    if let Ok(parsed) = DateTime::parse_from_rfc3339(&record.header.finished_at) {
        result.finished_at = parsed.with_timezone(&Local);
    }
    Ok(Some(result))
}

fn open(path: &Path) -> AppResult<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA foreign_keys = OFF;")?;
    initialize_or_migrate(&mut conn)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(conn)
}

pub fn practice_salt(path: &Path) -> AppResult<Vec<u8>> {
    let conn = open(path)?;
    if let Some(value) = conn
        .query_row("SELECT value FROM meta WHERE key = 'practice_salt'", [], |row| row.get::<_, Vec<u8>>(0))
        .optional()?
    {
        return Ok(value);
    }
    let mut value = vec![0_u8; 32];
    rand::thread_rng().fill_bytes(&mut value);
    conn.execute("INSERT OR IGNORE INTO meta (key, value) VALUES ('practice_salt', ?1)", params![value])?;
    conn.query_row("SELECT value FROM meta WHERE key = 'practice_salt'", [], |row| row.get(0))
        .map_err(Into::into)
}

pub fn load_item_exposures(path: &Path, kind: &str) -> AppResult<std::collections::HashMap<String, crate::practice::deck::Exposure>> {
    let conn = open(path)?;
    let mut statement = conn.prepare("SELECT item_key, count, last_shown FROM item_exposure WHERE kind = ?1")?;
    let rows = statement.query_map(params![kind], |row| {
        let date: String = row.get(2)?;
        Ok((
            row.get::<_, String>(0)?,
            crate::practice::deck::Exposure {
                count: row.get::<_, i64>(1)?.max(0) as u64,
                last_shown: chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").ok(),
            },
        ))
    })?;
    rows.collect::<Result<std::collections::HashMap<_, _>, _>>().map_err(Into::into)
}

pub fn record_item_exposures(path: &Path, kind: &str, item_keys: &[&str], shown: chrono::NaiveDate) -> AppResult<()> {
    let mut conn = open(path)?;
    let transaction = conn.transaction()?;
    for key in item_keys {
        transaction.execute(
            "INSERT INTO item_exposure (kind, item_key, count, last_shown) VALUES (?1, ?2, 1, ?3) \
             ON CONFLICT(kind, item_key) DO UPDATE SET count = count + 1, last_shown = excluded.last_shown",
            params![kind, key, shown.format("%Y-%m-%d").to_string()],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

pub fn record_item_mistake(path: &Path, kind: &str, item_key: &str, at: chrono::NaiveDate) -> AppResult<()> {
    let conn = open(path)?;
    conn.execute(
        "INSERT INTO item_mistake (kind, item_key, last_mistake) VALUES (?1, ?2, ?3) \
         ON CONFLICT(kind, item_key) DO UPDATE SET last_mistake = excluded.last_mistake",
        params![kind, item_key, at.format("%Y-%m-%d").to_string()],
    )?;
    Ok(())
}

pub fn load_item_mistakes(path: &Path, kind: &str) -> AppResult<Vec<crate::practice::deck::Mistake>> {
    let conn = open(path)?;
    let mut statement = conn.prepare("SELECT item_key, last_mistake FROM item_mistake WHERE kind = ?1")?;
    let rows = statement.query_map(params![kind], |row| {
        let date: String = row.get(1)?;
        Ok((row.get::<_, String>(0)?, date))
    })?;
    Ok(rows
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter_map(|(item_key, date)| {
            chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d")
                .ok()
                .map(|last_mistake| crate::practice::deck::Mistake { item_key, last_mistake })
        })
        .collect())
}

pub fn save_unlock_state(path: &Path, layout: &str, state: &crate::practice::unlock::UnlockState) -> AppResult<()> {
    let conn = open(path)?;
    conn.execute(
        "INSERT INTO unlock_state (layout, unlocked, streak) VALUES (?1, ?2, ?3) \
         ON CONFLICT(layout) DO UPDATE SET unlocked = excluded.unlocked, streak = excluded.streak",
        params![layout, state.unlocked, i64::from(state.streak)],
    )?;
    Ok(())
}

pub fn load_unlock_state(path: &Path, layout: &str) -> AppResult<Option<crate::practice::unlock::UnlockState>> {
    let conn = open(path)?;
    conn.query_row("SELECT unlocked, streak FROM unlock_state WHERE layout = ?1", params![layout], |row| {
        Ok(crate::practice::unlock::UnlockState {
            unlocked: row.get(0)?,
            streak: row.get::<_, i64>(1)?.clamp(0, i64::from(u8::MAX)) as u8,
        })
    })
    .optional()
    .map_err(Into::into)
}

pub fn save_adaptive_focus(path: &Path, day: chrono::NaiveDate, layout: &str, focus: &[(String, String)]) -> AppResult<()> {
    let conn = open(path)?;
    let transitions = serde_json::to_string(focus)?;
    conn.execute(
        "INSERT INTO adaptive_focus (day, layout, transitions) VALUES (?1, ?2, ?3) \
         ON CONFLICT(day, layout) DO UPDATE SET transitions = excluded.transitions",
        params![day.format("%Y-%m-%d").to_string(), layout, transitions],
    )?;
    Ok(())
}

pub fn load_adaptive_focus(path: &Path, day: chrono::NaiveDate, layout: &str) -> AppResult<Vec<(String, String)>> {
    let conn = open(path)?;
    let value = conn
        .query_row(
            "SELECT transitions FROM adaptive_focus WHERE day = ?1 AND layout = ?2",
            params![day.format("%Y-%m-%d").to_string(), layout],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    value.map_or_else(|| Ok(Vec::new()), |json| serde_json::from_str(&json).map_err(Into::into))
}

fn initialize_or_migrate(conn: &mut Connection) -> AppResult<()> {
    let tx = conn.transaction()?;
    if is_v1(&tx)? {
        tx.execute_batch("ALTER TABLE sessions RENAME TO sessions_v1;")?;
        tx.execute_batch(SCHEMA)?;
        migrate_rows(&tx)?;
    } else {
        if sessions_exist(&tx)? && raw_wpm_is_not_null(&tx)? {
            make_raw_wpm_nullable(&tx)?;
        }
        tx.execute_batch(SCHEMA)?;
    }
    ensure_score_eligible(&tx)?;
    let version: i64 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version < SCHEMA_VERSION {
        recompute_legacy_metrics(&tx)?;
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    }
    tx.commit()?;
    Ok(())
}

fn ensure_score_eligible(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    let mut statement = tx.prepare("PRAGMA table_info(sessions)")?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?.collect::<Result<Vec<_>, _>>()?;
    if !columns.iter().any(|name| name == "score_eligible") {
        tx.execute("ALTER TABLE sessions ADD COLUMN score_eligible INTEGER NOT NULL DEFAULT 1", [])?;
    }
    Ok(())
}

fn sessions_exist(tx: &Transaction<'_>) -> rusqlite::Result<bool> {
    tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='sessions')",
        [],
        |row| row.get(0),
    )
}

fn is_v1(tx: &Transaction<'_>) -> rusqlite::Result<bool> {
    let exists = sessions_exist(tx)?;
    if !exists {
        return Ok(false);
    }
    let mut stmt = tx.prepare("PRAGMA table_info(sessions)")?;
    let columns = stmt.query_map([], |row| row.get::<_, String>(1))?.collect::<Result<Vec<_>, _>>()?;
    Ok(columns.iter().any(|name| name == "payload") && !columns.iter().any(|name| name == "mode"))
}

fn raw_wpm_is_not_null(tx: &Transaction<'_>) -> rusqlite::Result<bool> {
    let mut stmt = tx.prepare("PRAGMA table_info(sessions)")?;
    let columns = stmt
        .query_map([], |row| Ok((row.get::<_, String>(1)?, row.get::<_, i64>(3)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(columns.into_iter().any(|(name, not_null)| name == "raw_wpm" && not_null != 0))
}

fn make_raw_wpm_nullable(tx: &Transaction<'_>) -> AppResult<()> {
    tx.execute_batch(
        r#"
CREATE TABLE sessions_v3_new (
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
  raw_wpm REAL,
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
INSERT INTO sessions_v3_new
SELECT id, started_at, finished_at, duration_ms, mode, mode_params, bench_id,
       layout, input_layout, geometry, error_mode, release_events, app_version,
       metrics_version, net_wpm,
       CASE WHEN metrics_version = 0 THEN NULL ELSE raw_wpm END,
       accuracy, iki_median_ms, iki_cv, cps, consistency, max_combo,
       completed_words, timeout_words, mistypes_total, legacy_payload
FROM sessions;
DROP TABLE sessions;
ALTER TABLE sessions_v3_new RENAME TO sessions;
"#,
    )?;
    Ok(())
}

#[derive(Debug)]
struct V1Row {
    id: i64,
    finished_at: String,
    duration_ms: i64,
    accuracy: f64,
    consistency: f64,
    max_combo: i64,
    completed_words: i64,
    timeout_words: i64,
    mistypes_total: i64,
    payload: String,
}

fn migrate_rows(tx: &Transaction<'_>) -> AppResult<()> {
    let rows = {
        let mut stmt = tx.prepare("SELECT id, finished_at, duration_ms, accuracy, consistency, max_combo, completed_words, timeout_words, mistypes_total, payload FROM sessions_v1 ORDER BY id")?;
        stmt.query_map([], |row| {
            Ok(V1Row {
                id: row.get(0)?,
                finished_at: row.get(1)?,
                duration_ms: row.get(2)?,
                accuracy: row.get(3)?,
                consistency: row.get(4)?,
                max_combo: row.get(5)?,
                completed_words: row.get(6)?,
                timeout_words: row.get(7)?,
                mistypes_total: row.get(8)?,
                payload: row.get(9)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?
    };
    for row in rows {
        let (net_wpm, cps) = metrics_from_legacy_payload(&row.payload)?;
        let started_at = DateTime::parse_from_rfc3339(&row.finished_at)
            .map(|time| time - chrono::Duration::milliseconds(row.duration_ms.max(0)))
            .map(|time| time.to_rfc3339())
            .unwrap_or_else(|_| row.finished_at.clone());
        tx.execute(
            "INSERT INTO sessions (id, started_at, finished_at, duration_ms, mode, mode_params, bench_id, layout, input_layout, geometry, error_mode, release_events, app_version, metrics_version, net_wpm, raw_wpm, accuracy, iki_median_ms, iki_cv, cps, consistency, max_combo, completed_words, timeout_words, mistypes_total, legacy_payload) VALUES (?1, ?2, ?3, ?4, 'random', '{}', NULL, 'unknown', 'unknown', 'unknown', 'stop', 0, '0.1.0', 0, ?5, NULL, ?6, 0.0, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                row.id,
                started_at,
                row.finished_at,
                row.duration_ms,
                net_wpm,
                row.accuracy,
                1.0 - row.consistency,
                cps,
                row.consistency,
                row.max_combo,
                row.completed_words,
                row.timeout_words,
                row.mistypes_total,
                row.payload,
            ],
        )?;
    }
    Ok(())
}

#[derive(serde::Deserialize)]
struct LegacyPayload {
    metrics: LegacyMetrics,
    total_duration: LegacyDuration,
    #[serde(default)]
    wpm: Option<f64>,
}

#[derive(serde::Deserialize)]
struct LegacyMetrics {
    correct_chars: u64,
}

#[derive(serde::Deserialize)]
struct LegacyDuration {
    secs: u64,
    nanos: u32,
}

fn parse_legacy_payload(payload: &str) -> AppResult<LegacyPayload> {
    serde_json::from_str(payload).map_err(Into::into)
}

fn metrics_from_legacy_payload(payload: &str) -> AppResult<(f64, f64)> {
    let payload = parse_legacy_payload(payload)?;
    let seconds = payload.total_duration.secs as f64 + f64::from(payload.total_duration.nanos) / 1_000_000_000.0;
    if seconds <= 0.0 {
        return Ok((0.0, 0.0));
    }
    let correct = payload.metrics.correct_chars as f64;
    Ok(((correct / 5.0) / (seconds / 60.0), correct / seconds))
}

fn recompute_legacy_metrics(tx: &Transaction<'_>) -> AppResult<()> {
    let rows = {
        let mut stmt = tx.prepare("SELECT id, legacy_payload FROM sessions WHERE metrics_version = 0")?;
        stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?
    };
    for (id, payload) in rows {
        let (net_wpm, cps) = metrics_from_legacy_payload(&payload)?;
        tx.execute(
            "UPDATE sessions SET net_wpm = ?1, cps = ?2, raw_wpm = NULL WHERE id = ?3 AND metrics_version = 0",
            params![net_wpm, cps, id],
        )?;
    }
    Ok(())
}

pub fn legacy_wpm(path: &Path, id: i64) -> AppResult<Option<f64>> {
    let record = load_record(path, id)?;
    let Some(payload) = record.and_then(|record| record.header.legacy_payload) else {
        return Ok(None);
    };
    Ok(parse_legacy_payload(&payload)?.wpm)
}

fn load_record_from(conn: &Connection, id: i64) -> AppResult<Option<SessionRecord>> {
    let header = conn
        .query_row(
            "SELECT id, started_at, finished_at, duration_ms, mode, mode_params, bench_id, layout, input_layout, geometry, error_mode, release_events, app_version, metrics_version, net_wpm, raw_wpm, accuracy, iki_median_ms, iki_cv, cps, consistency, max_combo, completed_words, timeout_words, mistypes_total, legacy_payload FROM sessions WHERE id = ?1",
            params![id],
            |row| {
                Ok(SessionHeader {
                    id: Some(row.get(0)?),
                    started_at: row.get(1)?,
                    finished_at: row.get(2)?,
                    duration_ms: row.get(3)?,
                    mode: row.get(4)?,
                    mode_params: row.get(5)?,
                    bench_id: row.get(6)?,
                    layout: row.get(7)?,
                    input_layout: row.get(8)?,
                    geometry: row.get(9)?,
                    error_mode: row.get(10)?,
                    release_events: row.get::<_, i64>(11)? != 0,
                    app_version: row.get(12)?,
                    metrics_version: row.get(13)?,
                    net_wpm: row.get(14)?,
                    raw_wpm: row.get(15)?,
                    accuracy: row.get(16)?,
                    iki_median_ms: row.get(17)?,
                    iki_cv: row.get(18)?,
                    cps: row.get(19)?,
                    consistency: row.get(20)?,
                    max_combo: row.get::<_, i64>(21)?.max(0) as u32,
                    completed_words: row.get::<_, i64>(22)?.max(0) as u32,
                    timeout_words: row.get::<_, i64>(23)?.max(0) as u32,
                    mistypes_total: row.get::<_, i64>(24)?.max(0) as u32,
                    legacy_payload: row.get(25)?,
                })
            },
        )
        .optional()?;
    let Some(header) = header else {
        return Ok(None);
    };
    let items = {
        let mut stmt = conn.prepare("SELECT idx, text, reading, shown_us, done_us, outcome FROM items WHERE session_id = ?1 ORDER BY idx")?;
        stmt.query_map(params![id], |row| {
            let outcome: String = row.get(5)?;
            Ok(ItemLog {
                idx: row.get::<_, i64>(0)?.max(0) as u32,
                text: row.get(1)?,
                reading: row.get(2)?,
                shown_us: row.get::<_, i64>(3)?.max(0) as u64,
                done_us: row.get::<_, Option<i64>>(4)?.map(|value| value.max(0) as u64),
                outcome: parse_outcome(&outcome),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?
    };
    let keystrokes = {
        let mut stmt = conn.prepare(
            "SELECT seq, t_us, release_us, item_idx, pos, kind, typed, expected, correct, code FROM keystrokes WHERE session_id = ?1 ORDER BY seq",
        )?;
        stmt.query_map(params![id], |row| {
            let kind: String = row.get(5)?;
            Ok(KeystrokeLog {
                seq: row.get::<_, i64>(0)?.max(0) as u32,
                t_us: row.get::<_, i64>(1)?.max(0) as u64,
                release_us: row.get::<_, Option<i64>>(2)?.map(|value| value.max(0) as u64),
                item_idx: row.get::<_, i64>(3)?.max(0) as u32,
                pos: row.get::<_, i64>(4)?.max(0) as u32,
                kind: if kind == "backspace" {
                    KeystrokeKind::Backspace
                } else {
                    KeystrokeKind::Char
                },
                typed: row.get(6)?,
                expected: row.get(7)?,
                correct: row.get::<_, i64>(8)? != 0,
                code: row.get(9)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?
    };
    Ok(Some(SessionRecord {
        log: SessionLog {
            duration_us: header.duration_ms.max(0) as u64 * 1_000,
            error_mode: if header.error_mode == "advance" {
                crate::config::ErrorMode::Advance
            } else {
                crate::config::ErrorMode::Stop
            },
            items,
            keystrokes,
        },
        header,
    }))
}

fn as_i64(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

fn item_outcome(value: ItemOutcome) -> &'static str {
    match value {
        ItemOutcome::Completed => "completed",
        ItemOutcome::Timeout => "timeout",
        ItemOutcome::Aborted => "aborted",
    }
}

fn parse_outcome(value: &str) -> ItemOutcome {
    match value {
        "completed" => ItemOutcome::Completed,
        "timeout" => ItemOutcome::Timeout,
        _ => ItemOutcome::Aborted,
    }
}

fn key_kind(value: KeystrokeKind) -> &'static str {
    match value {
        KeystrokeKind::Char => "char",
        KeystrokeKind::Backspace => "backspace",
    }
}

fn parse_local(value: &str) -> DateTime<Local> {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&Local))
        .unwrap_or_else(|_| Local::now())
}
