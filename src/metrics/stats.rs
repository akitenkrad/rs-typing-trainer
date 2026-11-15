//! Result-screen types. New results are populated by `analysis` from the
//! canonical logs; the legacy constructor remains for v1 JSON compatibility.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::engine::session::SessionLog;

/// Detailed values consumed by the existing playing and result screens.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LiveMetrics {
    pub completed_words: u32,
    pub timeout_words: u32,
    pub total_chars_typed: u32,
    pub correct_chars: u32,
    pub mistypes_total: u32,
    pub mistypes_by_char: HashMap<String, u32>,
    pub mistypes_by_word: HashMap<String, u32>,
    pub reaction_times: Vec<Duration>,
    pub keystroke_intervals: Vec<Duration>,
    pub word_completion_times: Vec<Duration>,
    pub max_combo: u32,
    #[serde(default)]
    pub uncorrected_errors: u32,
}

impl LiveMetrics {
    pub fn record_correct_char(&mut self) {
        self.correct_chars += 1;
        self.total_chars_typed += 1;
    }

    pub fn record_mistype(&mut self, expected: &str, current_word: &str) {
        self.mistypes_total += 1;
        self.total_chars_typed += 1;
        *self.mistypes_by_char.entry(expected.to_string()).or_insert(0) += 1;
        *self.mistypes_by_word.entry(current_word.to_string()).or_insert(0) += 1;
    }
}

/// Final, frozen result of one session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameResult {
    pub config: Config,
    pub finished_at: chrono::DateTime<chrono::Local>,
    pub total_duration: Duration,
    pub metrics: LiveMetrics,

    pub wpm: f64,
    pub cps: f64,
    /// Accuracy as a fraction in [0, 1].
    pub accuracy: f64,
    /// Consistency as a fraction in [0, 1] (Monkeytype-style).
    pub consistency: f64,
    pub mean_reaction: Duration,
    pub p50_reaction: Duration,
    pub p90_reaction: Duration,
    pub p99_reaction: Duration,
    pub max_combo: u32,
    #[serde(default)]
    pub analysis: crate::analysis::Analysis,
}

impl GameResult {
    pub fn from_metrics(config: &Config, metrics: LiveMetrics, total_duration: Duration) -> Self {
        let secs = total_duration.as_secs_f64().max(1e-9);
        let wpm = metrics.completed_words as f64 / (secs / 60.0);
        let cps = metrics.correct_chars as f64 / secs;
        let accuracy = if metrics.total_chars_typed == 0 {
            1.0
        } else {
            metrics.correct_chars as f64 / metrics.total_chars_typed as f64
        };
        let consistency = compute_consistency(&metrics.keystroke_intervals);
        let mean_reaction = mean_duration(&metrics.reaction_times);
        let p50_reaction = percentile(&metrics.reaction_times, 50.0);
        let p90_reaction = percentile(&metrics.reaction_times, 90.0);
        let p99_reaction = percentile(&metrics.reaction_times, 99.0);

        Self {
            config: config.clone(),
            finished_at: chrono::Local::now(),
            total_duration,
            max_combo: metrics.max_combo,
            metrics,
            wpm,
            cps,
            accuracy,
            consistency,
            mean_reaction,
            p50_reaction,
            p90_reaction,
            p99_reaction,
            analysis: crate::analysis::Analysis::default(),
        }
    }

    /// Build the exact value shown by the result screen from the canonical
    /// item and keystroke logs.
    pub fn from_log(config: &Config, log: &SessionLog) -> Self {
        let analysis = crate::analysis::analyze_with_pause(log, Duration::from_millis(config.analysis.pause_ms));
        Self {
            config: config.clone(),
            finished_at: chrono::Local::now(),
            total_duration: Duration::from_micros(log.duration_us),
            metrics: analysis.live.clone(),
            wpm: analysis.net_wpm,
            cps: analysis.cps,
            accuracy: analysis.accuracy,
            consistency: analysis.consistency,
            mean_reaction: analysis.mean_reaction,
            p50_reaction: analysis.p50_reaction,
            p90_reaction: analysis.p90_reaction,
            p99_reaction: analysis.p99_reaction,
            max_combo: analysis.max_combo,
            analysis,
        }
    }
}

fn mean_duration(samples: &[Duration]) -> Duration {
    if samples.is_empty() {
        return Duration::ZERO;
    }
    let total_ns: u128 = samples.iter().map(|d| d.as_nanos()).sum();
    let mean = total_ns / samples.len() as u128;
    Duration::from_nanos(mean as u64)
}

/// Linear-interpolation percentile (`pct` in [0, 100]).
pub fn percentile(samples: &[Duration], pct: f64) -> Duration {
    if samples.is_empty() {
        return Duration::ZERO;
    }
    let mut xs: Vec<u128> = samples.iter().map(|d| d.as_nanos()).collect();
    xs.sort_unstable();
    let p = pct.clamp(0.0, 100.0) / 100.0;
    let idx = p * (xs.len() as f64 - 1.0);
    let lo = idx.floor() as usize;
    let hi = idx.ceil() as usize;
    let frac = idx - lo as f64;
    let value = if lo == hi {
        xs[lo] as f64
    } else {
        xs[lo] as f64 + (xs[hi] - xs[lo]) as f64 * frac
    };
    Duration::from_nanos(value as u64)
}

/// Consistency = 1 - σ/μ for the keystroke interval distribution, clamped to [0,1].
pub fn compute_consistency(intervals: &[Duration]) -> f64 {
    if intervals.len() < 2 {
        return 0.0;
    }
    let xs: Vec<f64> = intervals.iter().map(|d| d.as_secs_f64()).collect();
    let mean = xs.iter().sum::<f64>() / xs.len() as f64;
    if mean <= 0.0 {
        return 0.0;
    }
    let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / xs.len() as f64;
    let std = var.sqrt();
    (1.0 - std / mean).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn percentile_basic() {
        let xs: Vec<Duration> = (1..=10).map(ms).collect();
        assert_eq!(percentile(&xs, 0.0), ms(1));
        assert_eq!(percentile(&xs, 100.0), ms(10));
    }

    #[test]
    fn consistency_constant_intervals_is_one() {
        let xs = vec![ms(100); 10];
        assert!((compute_consistency(&xs) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn consistency_empty_is_zero() {
        assert_eq!(compute_consistency(&[]), 0.0);
    }
}
