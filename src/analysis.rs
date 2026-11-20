//! Metrics derived exclusively from item and keystroke logs.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::analysis::errors::{ErrorDiagnosis, diagnose_errors};
use crate::analysis::japanese::{JapaneseDiagnosis, analyze_japanese};
use crate::analysis::metrics::AnalysisSettings;
use crate::analysis::model::{ModelView, PlayerModel, build_player_model};
use crate::analysis::style::{TypingStyleDiagnosis, diagnose_style};
use crate::data::JapaneseText;
use crate::engine::session::{ItemOutcome, KeystrokeKind, SessionLog};
use crate::layout::KeyboardLayout;
use crate::layout::geometry::KeyboardGeometry;
use crate::metrics::stats::{LiveMetrics, percentile};
use crate::record::SessionRecord;

pub mod bench_summary;
pub mod coach;
pub mod errors;
pub mod grades;
pub mod habits;
pub mod japanese;
pub mod metrics;
pub mod model;
pub mod prescription;
pub mod progress;
pub mod stats;
pub mod style;
pub mod summary;

pub const METRICS_VERSION: i64 = 2;
pub const IKI_PAUSE_THRESHOLD: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DiagnosisOptions {
    pub model: AnalysisSettings,
    pub min_transition_n: u64,
    pub spelling_suggestions: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Diagnosis {
    pub player_model: PlayerModel,
    pub errors: ErrorDiagnosis,
    pub style: TypingStyleDiagnosis,
    pub japanese: JapaneseDiagnosis,
}

#[allow(clippy::too_many_arguments)]
pub fn diagnose(
    records: &[SessionRecord],
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
    options: DiagnosisOptions,
    as_of: chrono::DateTime<chrono::Utc>,
    japanese_corpus: &[JapaneseText],
) -> Diagnosis {
    let player_model = build_player_model(records, layout, geometry, options.model, ModelView::Recent, as_of);
    let errors = diagnose_errors(records, layout, geometry, options.model, as_of);
    let style = diagnose_style(records, &player_model);
    let error_cost_ms = spelling_error_cost_ms(records, &errors);
    let japanese = analyze_japanese(
        records,
        layout,
        geometry,
        options.model,
        as_of,
        japanese_corpus,
        options.min_transition_n,
        options.spelling_suggestions,
        error_cost_ms,
    );
    Diagnosis {
        player_model,
        errors,
        style,
        japanese,
    }
}

/// The cost of one error (ms) that the spelling analysis adds to a
/// spelling's typing time: the post-error excess plus the correction time per
/// error, or 300 ms when neither was observed.
pub fn spelling_error_cost_ms(records: &[SessionRecord], errors: &errors::ErrorDiagnosis) -> f64 {
    let error_count = records
        .iter()
        .flat_map(|record| &record.log.keystrokes)
        .filter(|key| key.kind == KeystrokeKind::Char && !key.correct)
        .count() as u64;
    let mut error_cost_ms = 0.0;
    let mut has_error_cost = false;
    if let Some(slowdown) = &errors.post_error_slowdown {
        error_cost_ms += slowdown.excess_ms.max(0.0);
        has_error_cost = true;
    }
    if error_count > 0 && errors.correction.correction_time_ms > 0.0 {
        error_cost_ms += errors.correction.correction_time_ms / error_count as f64;
        has_error_cost = true;
    }
    if !has_error_cost {
        error_cost_ms = 300.0;
    }
    error_cost_ms
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Analysis {
    pub net_wpm: f64,
    pub raw_wpm: f64,
    pub cps: f64,
    pub accuracy: f64,
    pub iki_median: Duration,
    pub iki_cv: f64,
    pub consistency: f64,
    pub mean_reaction: Duration,
    pub p50_reaction: Duration,
    pub p90_reaction: Duration,
    pub p99_reaction: Duration,
    pub max_combo: u32,
    pub current_combo: u32,
    pub uncorrected_errors: u32,
    pub live: LiveMetrics,
}

pub fn analyze(log: &SessionLog) -> Analysis {
    analyze_with_pause(log, IKI_PAUSE_THRESHOLD)
}

pub fn analyze_with_pause(log: &SessionLog, pause_threshold: Duration) -> Analysis {
    let duration = Duration::from_micros(log.duration_us);
    let secs = duration.as_secs_f64().max(1e-9);
    let char_keys: Vec<_> = log.keystrokes.iter().filter(|key| key.kind == KeystrokeKind::Char).collect();
    let raw_correct_chars = char_keys.iter().filter(|key| key.correct).count() as u32;
    let raw_total_chars = char_keys.len() as u32;
    let (correct_chars, accuracy_total, uncorrected_errors) = if log.error_mode == crate::config::ErrorMode::Advance {
        let mut final_chars = 0u32;
        let mut errors = 0u32;
        for item in &log.items {
            let mut slots = Vec::<bool>::new();
            for key in log.keystrokes.iter().filter(|key| key.item_idx == item.idx) {
                match key.kind {
                    KeystrokeKind::Char => {
                        let position = key.pos as usize;
                        if position < slots.len() {
                            slots.truncate(position);
                        }
                        slots.push(key.correct);
                    }
                    KeystrokeKind::Backspace => {
                        slots.pop();
                    }
                }
            }
            final_chars += slots.len() as u32;
            errors += slots.iter().filter(|correct| !**correct).count() as u32;
        }
        (final_chars.saturating_sub(errors), final_chars, errors)
    } else {
        (raw_correct_chars, raw_total_chars, raw_total_chars.saturating_sub(raw_correct_chars))
    };

    let mut reaction_times = Vec::new();
    let mut intervals = Vec::new();
    for item in &log.items {
        let keys: Vec<_> = log.keystrokes.iter().filter(|key| key.item_idx == item.idx).collect();
        if let Some(first) = keys.first() {
            reaction_times.push(Duration::from_micros(first.t_us.saturating_sub(item.shown_us)));
        }
        for pair in keys.windows(2) {
            let interval = Duration::from_micros(pair[1].t_us.saturating_sub(pair[0].t_us));
            if interval <= pause_threshold {
                intervals.push(interval);
            }
        }
    }

    let completed_words = log.items.iter().filter(|item| item.outcome == ItemOutcome::Completed).count() as u32;
    let timeout_words = log.items.iter().filter(|item| item.outcome == ItemOutcome::Timeout).count() as u32;
    let mistypes_total = char_keys.iter().filter(|key| !key.correct).count() as u32;
    let mut mistypes_by_char = HashMap::new();
    let mut mistypes_by_word = HashMap::new();
    for key in char_keys.iter().filter(|key| !key.correct) {
        if let Some(expected) = &key.expected {
            *mistypes_by_char.entry(expected.clone()).or_insert(0) += 1;
        }
        if let Some(item) = log.items.iter().find(|item| item.idx == key.item_idx) {
            *mistypes_by_word.entry(item.text.clone()).or_insert(0) += 1;
        }
    }

    let mut word_completion_times = Vec::new();
    let mut combo = 0u32;
    let mut max_combo = 0u32;
    for item in &log.items {
        match item.outcome {
            ItemOutcome::Completed => {
                if log
                    .keystrokes
                    .iter()
                    .any(|key| key.item_idx == item.idx && key.kind == KeystrokeKind::Char && !key.correct)
                {
                    combo = 0;
                }
                combo += 1;
                max_combo = max_combo.max(combo);
                if let Some(done) = item.done_us {
                    word_completion_times.push(Duration::from_micros(done.saturating_sub(item.shown_us)));
                }
            }
            ItemOutcome::Timeout => combo = 0,
            ItemOutcome::Aborted => {}
        }
    }

    let iki_cv = coefficient_of_variation(&intervals);
    let live = LiveMetrics {
        completed_words,
        timeout_words,
        total_chars_typed: accuracy_total,
        correct_chars,
        mistypes_total,
        mistypes_by_char,
        mistypes_by_word,
        reaction_times: reaction_times.clone(),
        keystroke_intervals: intervals.clone(),
        word_completion_times,
        max_combo,
        uncorrected_errors,
    };
    Analysis {
        net_wpm: (correct_chars as f64 / 5.0) / (secs / 60.0),
        raw_wpm: (raw_total_chars as f64 / 5.0) / (secs / 60.0),
        cps: correct_chars as f64 / secs,
        accuracy: if accuracy_total == 0 {
            1.0
        } else {
            correct_chars as f64 / accuracy_total as f64
        },
        iki_median: percentile(&intervals, 50.0),
        iki_cv,
        consistency: if intervals.len() < 2 { 0.0 } else { (1.0 - iki_cv).clamp(0.0, 1.0) },
        mean_reaction: mean_duration(&reaction_times),
        p50_reaction: percentile(&reaction_times, 50.0),
        p90_reaction: percentile(&reaction_times, 90.0),
        p99_reaction: percentile(&reaction_times, 99.0),
        max_combo,
        current_combo: combo,
        uncorrected_errors,
        live,
    }
}

fn mean_duration(samples: &[Duration]) -> Duration {
    if samples.is_empty() {
        return Duration::ZERO;
    }
    let total: u128 = samples.iter().map(Duration::as_nanos).sum();
    Duration::from_nanos((total / samples.len() as u128).min(u64::MAX as u128) as u64)
}

fn coefficient_of_variation(samples: &[Duration]) -> f64 {
    if samples.len() < 2 {
        return 0.0;
    }
    let values: Vec<f64> = samples.iter().map(Duration::as_secs_f64).collect();
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    if mean <= 0.0 {
        return 0.0;
    }
    let variance = values.iter().map(|value| (value - mean).powi(2)).sum::<f64>() / values.len() as f64;
    variance.sqrt() / mean
}
