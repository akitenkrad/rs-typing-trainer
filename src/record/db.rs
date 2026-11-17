//! SQLite storage format v2 and transactional v1 migration.

use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Local};
use rand::RngCore;
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::analysis::metrics::AnalysisSettings;
use crate::analysis::model::{
    MODEL_VERSION, ModelView, Prescription, SufficientStatistics, TransitionStatistic, build_sufficient_statistics, session_language,
    update_sufficient_statistics,
};
use crate::config::Config;
use crate::engine::session::{ItemKind, ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};
use crate::error::{AppError, AppResult};
use crate::metrics::stats::GameResult;
use crate::record::{SessionHeader, SessionRecord};

pub const SCHEMA_VERSION: i64 = 7;

const V6_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS transition_model (
  layout TEXT NOT NULL,
  language TEXT NOT NULL,
  view TEXT NOT NULL,
  from_key TEXT NOT NULL,
  to_key TEXT NOT NULL,
  from_hand TEXT NOT NULL,
  to_hand TEXT NOT NULL,
  from_finger TEXT NOT NULL,
  to_finger TEXT NOT NULL,
  weight REAL NOT NULL,
  log_iki_sum REAL NOT NULL,
  log_iki_sq_sum REAL NOT NULL,
  error_weight REAL NOT NULL,
  attempt_weight REAL NOT NULL,
  observations INTEGER NOT NULL,
  errors INTEGER NOT NULL,
  last_added_at TEXT NOT NULL,
  as_of TEXT NOT NULL,
  last_session_id INTEGER,
  half_life_days REAL NOT NULL,
  model_version INTEGER NOT NULL,
  PRIMARY KEY (layout, language, view, from_key, to_key)
);
CREATE TABLE IF NOT EXISTS prescriptions (
  id INTEGER PRIMARY KEY,
  created_at TEXT NOT NULL,
  layout TEXT NOT NULL,
  kind TEXT NOT NULL,
  target_json TEXT NOT NULL,
  control_json TEXT NOT NULL,
  hypothesis_json TEXT NOT NULL,
  baseline_json TEXT NOT NULL,
  status TEXT NOT NULL,
  evaluated_at TEXT,
  result_json TEXT
);
CREATE INDEX IF NOT EXISTS idx_prescriptions_created_at ON prescriptions(created_at DESC);
"#;

const V7_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS review_schedule (
  layout TEXT NOT NULL,
  from_key TEXT NOT NULL,
  to_key TEXT NOT NULL,
  next_due TEXT NOT NULL,
  interval_days REAL NOT NULL,
  last_reviewed_at TEXT NOT NULL,
  PRIMARY KEY (layout, from_key, to_key)
);
CREATE INDEX IF NOT EXISTS idx_review_schedule_due ON review_schedule(layout, next_due);
"#;

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
  kind TEXT,
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
    pub started_at: DateTime<Local>,
    pub finished_at: DateTime<Local>,
    pub duration: Duration,
    pub mode: String,
    pub mode_params: String,
    pub bench_id: Option<String>,
    pub layout: String,
    pub input_layout: String,
    pub error_mode: String,
    pub wpm: f64,
    pub accuracy: f64,
    pub max_combo: u32,
    pub completed_words: u32,
    pub metrics_version: i64,
    pub score_eligible: bool,
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
            "INSERT INTO items (session_id, idx, text, reading, kind, shown_us, done_us, outcome) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                i64::from(item.idx),
                item.text,
                item.reading,
                item.kind.map(item_kind),
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
        "SELECT id, started_at, finished_at, duration_ms, mode, mode_params, bench_id, layout, input_layout, error_mode, net_wpm, accuracy, max_combo, completed_words, metrics_version, score_eligible FROM sessions ORDER BY datetime(finished_at) DESC, id DESC",
    )?;
    let rows = stmt.query_map([], summary_from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn ranked_sessions(path: &Path) -> AppResult<Vec<SessionSummary>> {
    let conn = open(path)?;
    let mut stmt = conn.prepare(
        "SELECT id, started_at, finished_at, duration_ms, mode, mode_params, bench_id, layout, input_layout, error_mode, net_wpm, accuracy, max_combo, completed_words, metrics_version, score_eligible \
         FROM sessions WHERE score_eligible = 1 ORDER BY net_wpm DESC, datetime(finished_at) DESC, id DESC",
    )?;
    let rows = stmt.query_map([], summary_from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn personal_best(path: &Path, mode: &str) -> AppResult<Option<SessionSummary>> {
    let conn = open(path)?;
    conn.query_row(
        "SELECT id, started_at, finished_at, duration_ms, mode, mode_params, bench_id, layout, input_layout, error_mode, net_wpm, accuracy, max_combo, completed_words, metrics_version, score_eligible \
         FROM sessions WHERE score_eligible = 1 AND mode = ?1 ORDER BY net_wpm DESC, datetime(finished_at) DESC, id DESC LIMIT 1",
        params![mode],
        summary_from_row,
    )
    .optional()
    .map_err(Into::into)
}

fn summary_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionSummary> {
    let started_at: String = row.get(1)?;
    let finished_at: String = row.get(2)?;
    Ok(SessionSummary {
        id: row.get(0)?,
        started_at: parse_local(&started_at),
        finished_at: parse_local(&finished_at),
        duration: Duration::from_millis(row.get::<_, i64>(3)?.max(0) as u64),
        mode: row.get(4)?,
        mode_params: row.get(5)?,
        bench_id: row.get(6)?,
        layout: row.get(7)?,
        input_layout: row.get(8)?,
        error_mode: row.get(9)?,
        wpm: row.get(10)?,
        accuracy: row.get(11)?,
        max_combo: row.get::<_, i64>(12)?.max(0) as u32,
        completed_words: row.get::<_, i64>(13)?.max(0) as u32,
        metrics_version: row.get(14)?,
        score_eligible: row.get::<_, i64>(15)? != 0,
    })
}

pub fn latest_bench_parallel(path: &Path, series: &str) -> AppResult<Option<u8>> {
    let conn = open(path)?;
    let pattern = format!("{series}/%/%");
    let mut statement = conn.prepare("SELECT bench_id FROM sessions WHERE bench_id LIKE ?1 ORDER BY datetime(finished_at) DESC, id DESC")?;
    let ids = statement
        .query_map(params![pattern], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids
        .into_iter()
        .find_map(|id| crate::bench::parse_bench_id(&id).and_then(|(candidate_series, _, index)| (candidate_series == series).then_some(index))))
}

pub fn incomplete_benchmark(path: &Path, series: &str, today: chrono::NaiveDate) -> AppResult<Option<crate::bench::BenchResume>> {
    let records = load_records_filtered(path, None, Some("bench"))?;
    let today_records = records
        .iter()
        .enumerate()
        .filter_map(|(position, record)| {
            let finished = DateTime::parse_from_rfc3339(&record.header.finished_at).ok()?;
            if finished.with_timezone(&Local).date_naive() != today {
                return None;
            }
            let (part, parallel_index) = parse_bench_id(record.header.bench_id.as_deref()?, series)?;
            Some((position, record, part, parallel_index))
        })
        .collect::<Vec<_>>();
    let parallel_index = today_records.last().map(|row| row.3);
    let Some(parallel_index) = parallel_index else {
        return Ok(None);
    };
    let completed = bench_results_for_records(&records, &today_records, series, parallel_index);
    let part = [crate::bench::BenchPart::A, crate::bench::BenchPart::B, crate::bench::BenchPart::C]
        .into_iter()
        .find(|part| !completed.iter().any(|result| result.part == *part));
    Ok(part.map(|part| crate::bench::BenchResume {
        part,
        parallel_index,
        completed,
    }))
}

pub fn bench_results_for_day_form(
    path: &Path,
    series: &str,
    today: chrono::NaiveDate,
    parallel_index: u8,
) -> AppResult<Vec<crate::bench::BenchResultSummary>> {
    let records = load_records_filtered(path, None, Some("bench"))?;
    let today_records = records
        .iter()
        .enumerate()
        .filter_map(|(position, record)| {
            let finished = DateTime::parse_from_rfc3339(&record.header.finished_at).ok()?;
            if finished.with_timezone(&Local).date_naive() != today {
                return None;
            }
            let (part, index) = parse_bench_id(record.header.bench_id.as_deref()?, series)?;
            Some((position, record, part, index))
        })
        .collect::<Vec<_>>();
    Ok(bench_results_for_records(&records, &today_records, series, parallel_index))
}

fn bench_results_for_records(
    records: &[SessionRecord],
    today_records: &[(usize, &SessionRecord, crate::bench::BenchPart, u8)],
    series: &str,
    parallel_index: u8,
) -> Vec<crate::bench::BenchResultSummary> {
    let mut selected = std::collections::BTreeMap::new();
    for (position, record, part, index) in today_records {
        if *index == parallel_index {
            selected.insert(*part, (*position, *record));
        }
    }
    selected
        .into_iter()
        .map(|(part, (position, record))| {
            let prior = records[..position]
                .iter()
                .filter(|candidate| {
                    candidate.header.bench_id.as_deref().and_then(crate::bench::parse_bench_id).is_some_and(
                        |(candidate_series, candidate_part, _)| {
                            candidate_part == part
                                && (matches!(part, crate::bench::BenchPart::B | crate::bench::BenchPart::D) || candidate_series == series)
                        },
                    )
                })
                .map(|candidate| (candidate.header.net_wpm, candidate.header.accuracy))
                .collect::<Vec<_>>();
            let previous = prior.last().copied();
            let best_wpm = prior.iter().map(|row| row.0).max_by(f64::total_cmp);
            let best_accuracy = prior.iter().map(|row| row.1).max_by(f64::total_cmp);
            let recent = prior.iter().rev().take(3).copied().collect::<Vec<_>>();
            let recent_wpm = (!recent.is_empty()).then(|| median_f64(recent.iter().map(|row| row.0).collect()));
            let recent_accuracy = (!recent.is_empty()).then(|| median_f64(recent.iter().map(|row| row.1).collect()));
            crate::bench::BenchResultSummary {
                id: record.header.bench_id.clone().unwrap_or_default(),
                part,
                wpm: record.header.net_wpm,
                accuracy: record.header.accuracy,
                comparison: crate::bench::BenchComparison {
                    previous_wpm_delta: previous.map(|row| record.header.net_wpm - row.0),
                    previous_accuracy_delta: previous.map(|row| record.header.accuracy - row.1),
                    best_wpm_delta: best_wpm.map(|value| record.header.net_wpm - value),
                    best_accuracy_delta: best_accuracy.map(|value| record.header.accuracy - value),
                    recent_three_wpm_delta: recent_wpm.map(|value| record.header.net_wpm - value),
                    recent_three_accuracy_delta: recent_accuracy.map(|value| record.header.accuracy - value),
                },
            }
        })
        .collect()
}

fn parse_bench_id(id: &str, series: &str) -> Option<(crate::bench::BenchPart, u8)> {
    let mut fields = id.split('/');
    if fields.next()? != series {
        return None;
    }
    let part = match fields.next()? {
        "A" => crate::bench::BenchPart::A,
        "B" => crate::bench::BenchPart::B,
        "C" => crate::bench::BenchPart::C,
        "D" => crate::bench::BenchPart::D,
        _ => return None,
    };
    let parallel_index = fields.next()?.parse().ok()?;
    (fields.next().is_none() && (1..=crate::bench::PARALLEL_VERSION_COUNT).contains(&parallel_index)).then_some((part, parallel_index))
}

pub fn bench_comparison(path: &Path, bench_id: &str, current_wpm: f64, current_accuracy: f64) -> AppResult<crate::bench::BenchComparison> {
    let Some((series, part, _)) = crate::bench::parse_bench_id(bench_id) else {
        return Ok(crate::bench::BenchComparison::default());
    };
    let conn = open(path)?;
    let patterns = if matches!(part, crate::bench::BenchPart::B | crate::bench::BenchPart::D) {
        (
            format!("{}/{}/%", crate::bench::BENCH_SERIES_V1, part.as_str()),
            format!("{}/{}/%", crate::bench::BENCH_SERIES_V2, part.as_str()),
        )
    } else {
        let pattern = format!("{series}/{}/%", part.as_str());
        (pattern.clone(), pattern)
    };
    let rows = {
        let mut statement = conn.prepare(
            "SELECT bench_id, net_wpm, accuracy FROM sessions
             WHERE bench_id LIKE ?1 OR bench_id LIKE ?2
             ORDER BY datetime(finished_at), id",
        )?;
        statement
            .query_map(params![patterns.0, patterns.1], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?, row.get::<_, f64>(2)?))
            })?
            .filter_map(|row| match row {
                Ok((id, wpm, accuracy))
                    if crate::bench::parse_bench_id(&id).is_some_and(|(candidate_series, candidate_part, _)| {
                        candidate_part == part
                            && (matches!(part, crate::bench::BenchPart::B | crate::bench::BenchPart::D) || candidate_series == series)
                    }) =>
                {
                    Some(Ok((wpm, accuracy)))
                }
                Ok(_) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    let previous = rows.last().copied();
    let best_wpm = rows.iter().map(|row| row.0).max_by(f64::total_cmp);
    let best_accuracy = rows.iter().map(|row| row.1).max_by(f64::total_cmp);
    let recent = rows.iter().rev().take(3).copied().collect::<Vec<_>>();
    let recent_wpm = (!recent.is_empty()).then(|| median_f64(recent.iter().map(|row| row.0).collect()));
    let recent_accuracy = (!recent.is_empty()).then(|| median_f64(recent.iter().map(|row| row.1).collect()));
    Ok(crate::bench::BenchComparison {
        previous_wpm_delta: previous.map(|row| current_wpm - row.0),
        previous_accuracy_delta: previous.map(|row| current_accuracy - row.1),
        best_wpm_delta: best_wpm.map(|value| current_wpm - value),
        best_accuracy_delta: best_accuracy.map(|value| current_accuracy - value),
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
        let is_bench = record.header.bench_id.as_deref().and_then(crate::bench::parse_bench_id).is_some();
        let mode_matches = mode.is_none_or(|expected| {
            if expected == "bench" {
                is_bench
            } else {
                !is_bench && record.header.mode == expected
            }
        });
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

fn save_meta_json<T: serde::Serialize>(path: &Path, key: &str, value: &T) -> AppResult<()> {
    let conn = open(path)?;
    let json = serde_json::to_vec(value)?;
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, json],
    )?;
    Ok(())
}

fn load_meta_json<T: serde::de::DeserializeOwned>(path: &Path, key: &str) -> AppResult<Option<T>> {
    let conn = open(path)?;
    let value = conn
        .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| row.get::<_, Vec<u8>>(0))
        .optional()?;
    value.map(|bytes| serde_json::from_slice(&bytes).map_err(Into::into)).transpose()
}

pub fn save_adaptive_difficulty(path: &Path, state: &crate::analysis::prescription::DifficultyState) -> AppResult<()> {
    save_meta_json(path, "adaptive_difficulty", state)
}

pub fn load_adaptive_difficulty(path: &Path) -> AppResult<Option<crate::analysis::prescription::DifficultyState>> {
    load_meta_json(path, "adaptive_difficulty")
}

pub fn save_selected_goal(path: &Path, goal: &crate::analysis::prescription::GoalSuggestion) -> AppResult<()> {
    save_meta_json(path, "selected_goal", goal)
}

pub fn load_selected_goal(path: &Path) -> AppResult<Option<crate::analysis::prescription::GoalSuggestion>> {
    load_meta_json(path, "selected_goal")
}

/// Replace one cached model scope transactionally.
///
/// Replacing instead of adding makes retries idempotent. The cache can be
/// discarded at any time because every value is derived from keystroke logs.
pub fn save_transition_model(path: &Path, statistics: &SufficientStatistics) -> AppResult<()> {
    let mut conn = open(path)?;
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM transition_model WHERE layout = ?1 AND language = ?2 AND view = ?3",
        params![statistics.layout, statistics.language, statistics.view.as_str()],
    )?;
    for row in &statistics.rows {
        tx.execute(
            "INSERT INTO transition_model (layout, language, view, from_key, to_key, from_hand, to_hand, from_finger, to_finger, weight, log_iki_sum, log_iki_sq_sum, error_weight, attempt_weight, observations, errors, last_added_at, as_of, last_session_id, half_life_days, model_version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
            params![
                statistics.layout,
                statistics.language,
                statistics.view.as_str(),
                row.from,
                row.to,
                row.from_hand,
                row.to_hand,
                row.from_finger,
                row.to_finger,
                row.weight,
                row.log_iki_sum,
                row.log_iki_sq_sum,
                row.error_weight,
                row.attempt_weight,
                as_i64(row.observations),
                as_i64(row.errors),
                row.last_added_at.to_rfc3339(),
                statistics.as_of.to_rfc3339(),
                statistics.last_session_id,
                statistics.half_life_days,
                statistics.model_version,
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Load a valid cache scope. A model-version mismatch returns `None`, which
/// tells the caller to rebuild it from the authoritative keystroke log.
pub fn load_transition_model(path: &Path, layout: &str, language: &str, view: ModelView) -> AppResult<Option<SufficientStatistics>> {
    let conn = open(path)?;
    let mut statement = conn.prepare(
        "SELECT from_key, to_key, from_hand, to_hand, from_finger, to_finger, weight, log_iki_sum, log_iki_sq_sum, error_weight, attempt_weight, observations, errors, last_added_at, as_of, last_session_id, half_life_days, model_version FROM transition_model WHERE layout = ?1 AND language = ?2 AND view = ?3 ORDER BY from_key, to_key",
    )?;
    let stored = statement
        .query_map(params![layout, language, view.as_str()], |row| {
            Ok((
                TransitionStatistic {
                    from: row.get(0)?,
                    to: row.get(1)?,
                    from_hand: row.get(2)?,
                    to_hand: row.get(3)?,
                    from_finger: row.get(4)?,
                    to_finger: row.get(5)?,
                    weight: row.get(6)?,
                    log_iki_sum: row.get(7)?,
                    log_iki_sq_sum: row.get(8)?,
                    error_weight: row.get(9)?,
                    attempt_weight: row.get(10)?,
                    observations: row.get::<_, i64>(11)?.max(0) as u64,
                    errors: row.get::<_, i64>(12)?.max(0) as u64,
                    last_added_at: parse_utc_sql(row.get::<_, String>(13)?)?,
                },
                row.get::<_, String>(14)?,
                row.get::<_, Option<i64>>(15)?,
                row.get::<_, f64>(16)?,
                row.get::<_, i64>(17)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let Some((_, as_of, last_session_id, half_life_days, model_version)) = stored.first() else {
        return Ok(None);
    };
    if *model_version != MODEL_VERSION {
        return Ok(None);
    }
    Ok(Some(SufficientStatistics {
        model_version: *model_version,
        layout: layout.to_owned(),
        language: language.to_owned(),
        view,
        half_life_days: *half_life_days,
        as_of: parse_utc_record(as_of)?,
        last_session_id: *last_session_id,
        rows: stored.into_iter().map(|(row, _, _, _, _)| row).collect(),
    }))
}

pub fn update_transition_model_cache(
    path: &Path,
    record: &SessionRecord,
    layout: &crate::layout::KeyboardLayout,
    geometry: &crate::layout::geometry::KeyboardGeometry,
    settings: AnalysisSettings,
) -> AppResult<()> {
    let language = session_language(record);
    let as_of = parse_utc_record(&record.header.finished_at)?;
    for view in [ModelView::Recent, ModelView::AllTime] {
        let statistics = load_transition_model(path, layout.name(), language, view)?;
        let statistics = if let Some(mut statistics) = statistics {
            update_sufficient_statistics(&mut statistics, std::slice::from_ref(record), layout, geometry, settings, as_of);
            statistics
        } else {
            let records = load_records_filtered(path, None, None)?
                .into_iter()
                .filter(|candidate| candidate.header.layout == layout.name() && session_language(candidate) == language)
                .collect::<Vec<_>>();
            let rebuild_as_of = records
                .iter()
                .filter_map(|candidate| parse_utc_record(&candidate.header.finished_at).ok())
                .max()
                .unwrap_or(as_of);
            build_sufficient_statistics(&records, layout, geometry, settings, view, rebuild_as_of)
        };
        save_transition_model(path, &statistics)?;
    }
    Ok(())
}

pub fn save_prescription(path: &Path, prescription: &Prescription) -> AppResult<i64> {
    let conn = open(path)?;
    let target = serde_json::to_string(&prescription.target)?;
    let control = serde_json::to_string(&prescription.control)?;
    let hypothesis = serde_json::to_string(&prescription.hypothesis)?;
    let baseline = serde_json::to_string(&prescription.baseline)?;
    let evaluated_at = prescription.evaluated_at.map(|value| value.to_rfc3339());
    let result = prescription.result.as_ref().map(serde_json::to_string).transpose()?;
    if let Some(id) = prescription.id {
        let changed = conn.execute(
            "UPDATE prescriptions SET created_at = ?1, layout = ?2, kind = ?3, target_json = ?4, control_json = ?5, hypothesis_json = ?6, baseline_json = ?7, status = ?8, evaluated_at = ?9, result_json = ?10 WHERE id = ?11",
            params![
                prescription.created_at.to_rfc3339(),
                prescription.layout,
                prescription.kind,
                target,
                control,
                hypothesis,
                baseline,
                prescription.status,
                evaluated_at,
                result,
                id,
            ],
        )?;
        if changed == 0 {
            return Err(AppError::Record(format!("prescription #{id} does not exist")));
        }
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO prescriptions (created_at, layout, kind, target_json, control_json, hypothesis_json, baseline_json, status, evaluated_at, result_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            prescription.created_at.to_rfc3339(),
            prescription.layout,
            prescription.kind,
            target,
            control,
            hypothesis,
            baseline,
            prescription.status,
            evaluated_at,
            result,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn load_prescriptions(path: &Path) -> AppResult<Vec<Prescription>> {
    let conn = open(path)?;
    let mut statement = conn.prepare(
        "SELECT id, created_at, layout, kind, target_json, control_json, hypothesis_json, baseline_json, status, evaluated_at, result_json FROM prescriptions ORDER BY created_at, id",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, Option<String>>(10)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(
            |(id, created_at, layout, kind, target, control, hypothesis, baseline, status, evaluated_at, result)| {
                Ok(Prescription {
                    id: Some(id),
                    created_at: parse_utc_record(&created_at)?,
                    layout,
                    kind,
                    target: serde_json::from_str(&target)?,
                    control: serde_json::from_str(&control)?,
                    hypothesis: serde_json::from_str(&hypothesis)?,
                    baseline: serde_json::from_str(&baseline)?,
                    status,
                    evaluated_at: evaluated_at.map(|value| parse_utc_record(&value)).transpose()?,
                    result: result.map(|value| serde_json::from_str(&value)).transpose()?,
                })
            },
        )
        .collect()
}

pub fn save_review_schedule(path: &Path, schedule: &crate::practice::review::ReviewSchedule) -> AppResult<()> {
    let conn = open(path)?;
    conn.execute(
        "INSERT INTO review_schedule (layout, from_key, to_key, next_due, interval_days, last_reviewed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(layout, from_key, to_key) DO UPDATE SET next_due=excluded.next_due, interval_days=excluded.interval_days, last_reviewed_at=excluded.last_reviewed_at",
        params![
            schedule.layout,
            schedule.from,
            schedule.to,
            schedule.next_due.to_rfc3339(),
            schedule.interval_days,
            schedule.last_reviewed_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

pub fn load_review_schedules(path: &Path, layout: &str) -> AppResult<Vec<crate::practice::review::ReviewSchedule>> {
    let conn = open(path)?;
    let mut statement = conn.prepare(
        "SELECT layout, from_key, to_key, next_due, interval_days, last_reviewed_at FROM review_schedule WHERE layout = ?1 ORDER BY next_due, from_key, to_key",
    )?;
    let rows = statement
        .query_map([layout], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, f64>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(layout, from, to, next_due, interval_days, last_reviewed_at)| {
            Ok(crate::practice::review::ReviewSchedule {
                layout,
                from,
                to,
                next_due: parse_utc_record(&next_due)?,
                interval_days,
                last_reviewed_at: parse_utc_record(&last_reviewed_at)?,
            })
        })
        .collect()
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
    ensure_item_kind(&tx)?;
    let version: i64 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version < SCHEMA_VERSION {
        recompute_legacy_metrics(&tx)?;
        if version < 6 {
            migrate_v6(&tx)?;
        }
        if version < 7 {
            migrate_v7(&tx)?;
        }
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    }
    tx.commit()?;
    Ok(())
}

fn migrate_v6(tx: &Transaction<'_>) -> AppResult<()> {
    tx.execute_batch(V6_SCHEMA)?;
    Ok(())
}

fn migrate_v7(tx: &Transaction<'_>) -> AppResult<()> {
    tx.execute_batch(V7_SCHEMA)?;
    ensure_item_kind(tx)?;
    Ok(())
}

fn ensure_item_kind(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    let mut statement = tx.prepare("PRAGMA table_info(items)")?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?.collect::<Result<Vec<_>, _>>()?;
    if !columns.iter().any(|name| name == "kind") {
        tx.execute("ALTER TABLE items ADD COLUMN kind TEXT", [])?;
    }
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
        let mut stmt = conn.prepare("SELECT idx, text, reading, kind, shown_us, done_us, outcome FROM items WHERE session_id = ?1 ORDER BY idx")?;
        stmt.query_map(params![id], |row| {
            let kind: Option<String> = row.get(3)?;
            let outcome: String = row.get(6)?;
            Ok(ItemLog {
                idx: row.get::<_, i64>(0)?.max(0) as u32,
                text: row.get(1)?,
                reading: row.get(2)?,
                kind: kind.as_deref().and_then(parse_item_kind),
                shown_us: row.get::<_, i64>(4)?.max(0) as u64,
                done_us: row.get::<_, Option<i64>>(5)?.map(|value| value.max(0) as u64),
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

fn item_kind(value: ItemKind) -> &'static str {
    match value {
        ItemKind::Drill => "drill",
        ItemKind::Word => "word",
    }
}

fn parse_item_kind(value: &str) -> Option<ItemKind> {
    match value {
        "drill" => Some(ItemKind::Drill),
        "word" => Some(ItemKind::Word),
        _ => None,
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

fn parse_utc_sql(value: String) -> rusqlite::Result<DateTime<chrono::Utc>> {
    DateTime::parse_from_rfc3339(&value)
        .map(|time| time.with_timezone(&chrono::Utc))
        .map_err(|error| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error)))
}

fn parse_utc_record(value: &str) -> AppResult<DateTime<chrono::Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&chrono::Utc))
        .map_err(|error| AppError::Record(format!("invalid RFC 3339 timestamp {value:?}: {error}")))
}
