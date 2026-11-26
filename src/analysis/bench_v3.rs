//! Metrics for one bench-v3 part or a set of parts in the same run.

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};

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

/// One complete bench-v3 group result, keyed like the existing history by
/// local date and parallel-form number.
#[derive(Clone, Debug, PartialEq)]
pub struct BenchV3GroupRun {
    pub date: NaiveDate,
    pub form: u8,
    pub metrics: BenchV3Metrics,
}

/// Measurement error estimated from split-half differences across runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BenchV3Reliability {
    pub sample_count: usize,
    pub sem_wpm: f64,
    pub mdc95_wpm: f64,
    pub reference_only: bool,
}

/// Classification of a difference using the available MDC95 threshold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchV3Change {
    /// There is no MDC95 yet, so the caller should show the raw difference.
    DifferenceOnly,
    WithinError,
    Increase,
    Decrease,
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

/// Build complete language/group runs in chronological order.
pub fn compute_bench_v3_group_runs(records: &[SessionRecord], group: BenchV3Group, pause_ms: u64) -> Vec<BenchV3GroupRun> {
    let mut grouped = BTreeMap::<(NaiveDate, u8), Vec<SessionRecord>>::new();
    for record in records {
        let Some((part, form)) = record.header.bench_id.as_deref().and_then(parse_v3_bench_id) else {
            continue;
        };
        if !group.parts().contains(&part) {
            continue;
        }
        let Some(date) = DateTime::parse_from_rfc3339(&record.header.finished_at)
            .ok()
            .map(|time| time.date_naive())
        else {
            continue;
        };
        grouped.entry((date, form)).or_default().push(record.clone());
    }
    grouped
        .into_iter()
        .filter_map(|((date, form), records)| {
            let summary = compute_bench_v3_group_metrics(&records, group, pause_ms)?;
            (!summary.partial).then_some(BenchV3GroupRun {
                date,
                form,
                metrics: summary.metrics,
            })
        })
        .collect()
}

/// Return the median at each run using that run and up to two preceding runs.
pub fn rolling_median_3(values: &[f64]) -> Vec<f64> {
    (0..values.len())
        .map(|index| median(&values[index.saturating_sub(2)..=index]).expect("rolling window is non-empty"))
        .collect()
}

/// Estimate SEM and MDC95 from the latest ten split-half differences.
///
/// Each input is `half_a - half_b` for one run, ordered from oldest to
/// newest. At least two differences are required for the sample standard
/// deviation. Estimates based on fewer than five runs are reference-only.
pub fn compute_bench_v3_reliability(differences: &[f64]) -> Option<BenchV3Reliability> {
    let recent = &differences[differences.len().saturating_sub(10)..];
    if recent.len() < 2 {
        return None;
    }

    let mean = recent.iter().sum::<f64>() / recent.len() as f64;
    let squared_deviation_sum = recent.iter().map(|value| (value - mean).powi(2)).sum::<f64>();
    let sample_sd = (squared_deviation_sum / (recent.len() - 1) as f64).sqrt();
    let sem_wpm = sample_sd / 2.0;

    Some(BenchV3Reliability {
        sample_count: recent.len(),
        sem_wpm,
        mdc95_wpm: 1.96 * 2.0_f64.sqrt() * sem_wpm,
        reference_only: recent.len() < 5,
    })
}

/// Estimate SEM and MDC95 for one bench-v3 part across its latest runs.
pub fn compute_bench_v3_part_reliability(records: &[SessionRecord], part: BenchV3Part, pause_ms: u64) -> Option<BenchV3Reliability> {
    let mut rows = records
        .iter()
        .filter(|record| {
            record
                .header
                .bench_id
                .as_deref()
                .and_then(parse_v3_bench_id)
                .is_some_and(|(candidate, _)| candidate == part)
        })
        .filter_map(|record| {
            let metrics = compute_bench_v3_metrics(std::slice::from_ref(record), pause_ms);
            Some((record_order(record)?, metrics.half_a? - metrics.half_b?))
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|(order, _)| *order);
    compute_bench_v3_reliability(&rows.into_iter().map(|(_, difference)| difference).collect::<Vec<_>>())
}

/// Estimate SEM and MDC95 for a complete bench-v3 language/group summary.
pub fn compute_bench_v3_group_reliability(records: &[SessionRecord], group: BenchV3Group, pause_ms: u64) -> Option<BenchV3Reliability> {
    let differences = compute_bench_v3_group_runs(records, group, pause_ms)
        .into_iter()
        .filter_map(|run| run.metrics.half_a.zip(run.metrics.half_b).map(|(a, b)| a - b))
        .collect::<Vec<_>>();
    compute_bench_v3_reliability(&differences)
}

/// Classify a signed difference against MDC95.
pub fn classify_bench_v3_change(delta_wpm: f64, mdc95_wpm: Option<f64>) -> BenchV3Change {
    let Some(mdc95_wpm) = mdc95_wpm else {
        return BenchV3Change::DifferenceOnly;
    };
    if delta_wpm.abs() < mdc95_wpm || delta_wpm == 0.0 {
        BenchV3Change::WithinError
    } else if delta_wpm > 0.0 {
        BenchV3Change::Increase
    } else {
        BenchV3Change::Decrease
    }
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

#[cfg(test)]
mod tests {
    use super::{BenchV3Change, classify_bench_v3_change, compute_bench_v3_reliability, rolling_median_3};

    #[test]
    fn rolling_median_uses_up_to_the_current_and_two_preceding_runs() {
        assert!(rolling_median_3(&[]).is_empty());
        assert_eq!(rolling_median_3(&[10.0]), vec![10.0]);
        assert_eq!(rolling_median_3(&[10.0, 20.0]), vec![10.0, 15.0]);
        assert_eq!(rolling_median_3(&[10.0, 20.0, 30.0]), vec![10.0, 15.0, 20.0]);
        assert_eq!(rolling_median_3(&[10.0, 20.0, 30.0, 40.0]), vec![10.0, 15.0, 20.0, 30.0]);
    }

    #[test]
    fn reliability_uses_sample_sd_for_sem_and_derives_mdc95() {
        let estimate = compute_bench_v3_reliability(&[-2.0, 0.0, 2.0]).unwrap();

        assert_eq!(estimate.sample_count, 3);
        assert!((estimate.sem_wpm - 1.0).abs() < 1e-12);
        assert!((estimate.mdc95_wpm - 1.96 * 2.0_f64.sqrt()).abs() < 1e-12);
        assert!(estimate.reference_only);
    }

    #[test]
    fn reliability_uses_only_the_latest_ten_differences() {
        let estimate = compute_bench_v3_reliability(&[1_000.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]).unwrap();

        assert_eq!(estimate.sample_count, 10);
        assert!((estimate.sem_wpm - (82.5_f64 / 9.0).sqrt() / 2.0).abs() < 1e-12);
        assert!(!estimate.reference_only);
    }

    #[test]
    fn reliability_requires_two_differences_and_is_reference_below_five() {
        assert_eq!(compute_bench_v3_reliability(&[]), None);
        assert_eq!(compute_bench_v3_reliability(&[1.0]), None);
        let zero_error = compute_bench_v3_reliability(&[1.0, 1.0]).unwrap();
        assert_eq!(zero_error.sem_wpm, 0.0);
        assert_eq!(zero_error.mdc95_wpm, 0.0);
        assert!(compute_bench_v3_reliability(&[1.0, 2.0, 3.0, 4.0]).unwrap().reference_only);
        assert!(!compute_bench_v3_reliability(&[1.0, 2.0, 3.0, 4.0, 5.0]).unwrap().reference_only);
    }

    #[test]
    fn change_is_within_error_only_below_mdc95() {
        assert_eq!(classify_bench_v3_change(2.49, Some(2.5)), BenchV3Change::WithinError);
        assert_eq!(classify_bench_v3_change(-2.49, Some(2.5)), BenchV3Change::WithinError);
        assert_eq!(classify_bench_v3_change(2.5, Some(2.5)), BenchV3Change::Increase);
        assert_eq!(classify_bench_v3_change(-2.5, Some(2.5)), BenchV3Change::Decrease);
        assert_eq!(classify_bench_v3_change(0.0, Some(0.0)), BenchV3Change::WithinError);
    }

    #[test]
    fn change_without_mdc95_keeps_the_difference_unclassified() {
        assert_eq!(classify_bench_v3_change(2.5, None), BenchV3Change::DifferenceOnly);
    }
}
