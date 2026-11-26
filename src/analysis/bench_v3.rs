//! Metrics for one bench-v3 part or a set of parts in the same run.

use chrono::{DateTime, Utc};

use crate::analysis::errors::post_error_detail;
use crate::analysis::metrics::AnalysisSettings;
use crate::bench::{BenchV3Part, parse_v3_bench_id};
use crate::engine::session::KeystrokeKind;
use crate::layout::KeyboardLayout;
use crate::layout::geometry::KeyboardGeometry;
use crate::record::SessionRecord;

/// Metrics derived from bench-v3 item and keystroke logs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BenchV3Metrics {
    pub tempo_wpm: Option<f64>,
    pub tempo_iki_median_ms: Option<f64>,
    pub tempo_interval_count: usize,
    pub accuracy: f64,
    pub effective_wpm: Option<f64>,
    pub reaction_ms: Option<f64>,
    pub post_error_excess_ms: Option<f64>,
    pub half_a: Option<f64>,
    pub half_b: Option<f64>,
    pub error_margin_wpm: Option<f64>,
    pub correct_keystrokes: u64,
    pub total_keystrokes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchV3Group {
    English,
    Japanese,
    Transition,
}

impl BenchV3Group {
    pub const fn parts(self) -> &'static [BenchV3Part] {
        match self {
            Self::English => &[BenchV3Part::EW, BenchV3Part::ES],
            Self::Japanese => &[BenchV3Part::JW, BenchV3Part::JS],
            Self::Transition => &[BenchV3Part::PW],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BenchV3GroupMetrics {
    pub group: BenchV3Group,
    pub present_parts: Vec<BenchV3Part>,
    pub partial: bool,
    pub metrics: BenchV3Metrics,
}

#[derive(Clone, Copy)]
enum ItemHalf {
    All,
    A,
    B,
}

#[derive(Default)]
struct Samples {
    intervals_ms: Vec<f64>,
    reactions_ms: Vec<f64>,
    correct_keystrokes: u64,
    total_keystrokes: u64,
}

/// Compute bench-v3 metrics without the model-based post-error measurement.
///
/// Passing more than one record pools their raw eligible intervals and
/// keystroke counts before calculating the median and accuracy.
pub fn compute_bench_v3_metrics(records: &[SessionRecord], pause_ms: u64) -> BenchV3Metrics {
    let samples = collect_samples(records, pause_ms, ItemHalf::All);
    let mut metrics = metrics_from_samples(&samples);
    metrics.half_a = effective_wpm(&collect_samples(records, pause_ms, ItemHalf::A));
    metrics.half_b = effective_wpm(&collect_samples(records, pause_ms, ItemHalf::B));
    metrics.error_margin_wpm = metrics.half_a.zip(metrics.half_b).map(|(a, b)| (a - b).abs() / 2.0);
    metrics
}

/// Compute a language/group summary from records belonging to one bench-v3
/// run. Records for other groups and non-v3 records are ignored.
pub fn compute_bench_v3_group_metrics(records: &[SessionRecord], group: BenchV3Group, pause_ms: u64) -> Option<BenchV3GroupMetrics> {
    let expected = group.parts();
    let selected = records
        .iter()
        .filter(|record| {
            record
                .header
                .bench_id
                .as_deref()
                .and_then(parse_v3_bench_id)
                .is_some_and(|(part, _)| expected.contains(&part))
        })
        .cloned()
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return None;
    }
    let present_parts = expected
        .iter()
        .copied()
        .filter(|part| {
            selected.iter().any(|record| {
                record
                    .header
                    .bench_id
                    .as_deref()
                    .and_then(parse_v3_bench_id)
                    .is_some_and(|(present, _)| present == *part)
            })
        })
        .collect::<Vec<_>>();
    Some(BenchV3GroupMetrics {
        group,
        partial: present_parts.len() < expected.len(),
        present_parts,
        metrics: compute_bench_v3_metrics(&selected, pause_ms),
    })
}

/// Difference in accuracy-adjusted speed from the preceding record of the
/// same bench-v3 part. Parallel-form numbers do not split the series.
pub fn previous_effective_wpm_delta(records: &[SessionRecord], current: &SessionRecord, pause_ms: u64) -> Option<f64> {
    let (current_part, _) = current.header.bench_id.as_deref().and_then(parse_v3_bench_id)?;
    let current_order = record_order(current)?;
    let previous = records
        .iter()
        .filter(|record| {
            record
                .header
                .bench_id
                .as_deref()
                .and_then(parse_v3_bench_id)
                .is_some_and(|(part, _)| part == current_part)
        })
        .filter_map(|record| Some((record_order(record)?, record)))
        .filter(|(order, _)| *order < current_order)
        .max_by_key(|(order, _)| *order)
        .map(|(_, record)| record)?;
    let current = compute_bench_v3_metrics(std::slice::from_ref(current), pause_ms).effective_wpm?;
    let previous = compute_bench_v3_metrics(std::slice::from_ref(previous), pause_ms).effective_wpm?;
    Some(current - previous)
}

/// Compute all bench-v3 metrics and fill the model-based
/// `post-error-excess-ms` value through the existing error analysis.
pub fn analyze_bench_v3_metrics(
    records: &[SessionRecord],
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
    settings: AnalysisSettings,
    as_of: DateTime<Utc>,
) -> BenchV3Metrics {
    let mut metrics = compute_bench_v3_metrics(records, settings.pause_ms);
    metrics.post_error_excess_ms = post_error_detail(records, layout, geometry, settings, as_of)
        .slowdown
        .map(|slowdown| slowdown.excess_ms);
    metrics
}

fn collect_samples(records: &[SessionRecord], pause_ms: u64, half: ItemHalf) -> Samples {
    let pause_us = pause_ms.saturating_mul(1_000);
    let mut samples = Samples::default();

    for record in records {
        for (item_position, item) in record.log.items.iter().enumerate() {
            let selected = match half {
                ItemHalf::All => true,
                ItemHalf::A => item_position.is_multiple_of(2),
                ItemHalf::B => !item_position.is_multiple_of(2),
            };
            if !selected {
                continue;
            }
            let keys = record
                .log
                .keystrokes
                .iter()
                .filter(|key| key.kind == KeystrokeKind::Char && key.item_idx == item.idx)
                .collect::<Vec<_>>();
            for key in &keys {
                samples.total_keystrokes += 1;
                samples.correct_keystrokes += u64::from(key.correct);
            }
            if let Some(first) = keys.first() {
                samples.reactions_ms.push(first.t_us.saturating_sub(item.shown_us) as f64 / 1_000.0);
            }
            let mut recovery_keys = 0_u8;
            for (index, key) in keys.iter().enumerate() {
                if !key.correct {
                    recovery_keys = 3;
                    continue;
                }
                let after_error = recovery_keys > 0;
                recovery_keys = recovery_keys.saturating_sub(1);
                if index == 0 || after_error || !keys[index - 1].correct {
                    continue;
                }
                let delta_us = key.t_us.saturating_sub(keys[index - 1].t_us);
                if delta_us <= pause_us {
                    samples.intervals_ms.push(delta_us as f64 / 1_000.0);
                }
            }
        }
    }
    samples
}

fn metrics_from_samples(samples: &Samples) -> BenchV3Metrics {
    let tempo_iki_median_ms = median(&samples.intervals_ms);
    let tempo_wpm = tempo_iki_median_ms.filter(|median| *median > 0.0).map(|median| 60_000.0 / (median * 5.0));
    let accuracy = if samples.total_keystrokes == 0 {
        1.0
    } else {
        samples.correct_keystrokes as f64 / samples.total_keystrokes as f64
    };

    BenchV3Metrics {
        tempo_wpm,
        tempo_iki_median_ms,
        tempo_interval_count: samples.intervals_ms.len(),
        accuracy,
        effective_wpm: tempo_wpm.map(|tempo| tempo * accuracy),
        reaction_ms: median(&samples.reactions_ms),
        correct_keystrokes: samples.correct_keystrokes,
        total_keystrokes: samples.total_keystrokes,
        ..BenchV3Metrics::default()
    }
}

fn effective_wpm(samples: &Samples) -> Option<f64> {
    metrics_from_samples(samples).effective_wpm
}

fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        Some((values[middle - 1] + values[middle]) / 2.0)
    } else {
        Some(values[middle])
    }
}

fn record_order(record: &SessionRecord) -> Option<(DateTime<Utc>, i64)> {
    let finished = DateTime::parse_from_rfc3339(&record.header.finished_at).ok()?.with_timezone(&Utc);
    Some((finished, record.header.id.unwrap_or(i64::MIN)))
}
