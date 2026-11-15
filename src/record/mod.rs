//! Session persistence and portable record files.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::analysis::{METRICS_VERSION, analyze_with_pause};
use crate::config::Config;
use crate::engine::session::SessionLog;

pub mod db;
pub mod storage;
pub mod ttrec;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionHeader {
    pub id: Option<i64>,
    pub started_at: String,
    pub finished_at: String,
    pub duration_ms: i64,
    pub mode: String,
    pub mode_params: String,
    pub bench_id: Option<String>,
    pub layout: String,
    pub input_layout: String,
    pub geometry: String,
    pub error_mode: String,
    pub release_events: bool,
    pub app_version: String,
    pub metrics_version: i64,
    pub net_wpm: f64,
    pub raw_wpm: Option<f64>,
    pub accuracy: f64,
    pub iki_median_ms: f64,
    pub iki_cv: f64,
    pub cps: f64,
    pub consistency: f64,
    pub max_combo: u32,
    pub completed_words: u32,
    pub timeout_words: u32,
    pub mistypes_total: u32,
    pub legacy_payload: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub header: SessionHeader,
    pub log: SessionLog,
}

impl SessionRecord {
    pub fn from_log(config: &Config, log: SessionLog, started_at: DateTime<Utc>) -> Self {
        let metrics = analyze_with_pause(&log, std::time::Duration::from_millis(config.analysis.pause_ms));
        let finished_at = started_at + chrono::Duration::microseconds(log.duration_us.min(i64::MAX as u64) as i64);
        let mode_params = serde_json::json!({
            "kind": config.mode.kind,
            "keys": config.mode.keys,
            "key_filter": config.mode.key_filter,
            "base_keys": config.mode.base_keys,
            "word_length": config.mode.random.word_length,
            "allow_repeat": config.mode.random.allow_repeat,
            "allow_consecutive_same_word": config.mode.random.allow_consecutive_same_word,
            "n": config.mode.ngram.n,
            "source": config.mode.ngram.source,
            "lang": config.mode.sentence.lang,
            "pacer_enabled": config.pacer.enabled,
            "pacer_target_wpm": config.pacer.target_wpm,
            "score_eligible": crate::practice::gate::is_eligible(config.gate.min_accuracy, metrics.accuracy),
        })
        .to_string();
        Self {
            header: SessionHeader {
                id: None,
                started_at: started_at.to_rfc3339(),
                finished_at: finished_at.to_rfc3339(),
                duration_ms: (log.duration_us / 1_000).min(i64::MAX as u64) as i64,
                mode: config.mode.kind.as_str().to_owned(),
                mode_params,
                bench_id: None,
                layout: config.keyboard.layout.clone(),
                input_layout: config.keyboard.input_layout.clone(),
                geometry: config.keyboard.geometry.clone(),
                error_mode: match if config.session.blind {
                    log.error_mode
                } else {
                    config.session.error_mode
                } {
                    crate::config::ErrorMode::Stop => "stop",
                    crate::config::ErrorMode::Advance => "advance",
                }
                .to_owned(),
                release_events: false,
                app_version: env!("CARGO_PKG_VERSION").to_owned(),
                metrics_version: METRICS_VERSION,
                net_wpm: metrics.net_wpm,
                raw_wpm: Some(metrics.raw_wpm),
                accuracy: metrics.accuracy,
                iki_median_ms: metrics.iki_median.as_secs_f64() * 1_000.0,
                iki_cv: metrics.iki_cv,
                cps: metrics.cps,
                consistency: metrics.consistency,
                max_combo: metrics.max_combo,
                completed_words: metrics.live.completed_words,
                timeout_words: metrics.live.timeout_words,
                mistypes_total: metrics.live.mistypes_total,
                legacy_payload: None,
            },
            log,
        }
    }
}
