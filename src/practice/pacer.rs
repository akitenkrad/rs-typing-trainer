//! Pacer target and cursor calculations with injected records and time.

use std::time::Duration;

use crate::record::SessionRecord;

pub fn target_wpm(records: &[SessionRecord], ratio: f64, min_wpm: f64) -> f64 {
    let latest_pacer = records.iter().enumerate().rev().find(|(_, record)| pacer_target(record).is_some());
    let latest_bench =
        records.iter().enumerate().rev().find(|(_, record)| {
            record.header.mode == "bench" && record.header.bench_id.as_deref().is_some_and(|id| id.split('/').nth(1) == Some("A"))
        });
    if let Some((pacer_index, record)) = latest_pacer
        && latest_bench.is_none_or(|(bench_index, _)| pacer_index > bench_index)
        && let Some(target) = pacer_target(record)
    {
        return next_target_wpm(target, record.header.net_wpm, record.header.accuracy, min_wpm);
    }
    if let Some((_, bench)) = latest_bench {
        return (bench.header.net_wpm * ratio).max(min_wpm);
    }
    let mut recent = records
        .iter()
        .rev()
        .filter(|record| record.header.metrics_version > 0)
        .take(5)
        .map(|record| record.header.net_wpm)
        .collect::<Vec<_>>();
    if recent.is_empty() {
        return min_wpm;
    }
    recent.sort_by(f64::total_cmp);
    (recent[recent.len() / 2] * ratio).max(min_wpm)
}

fn pacer_target(record: &SessionRecord) -> Option<f64> {
    serde_json::from_str::<serde_json::Value>(&record.header.mode_params)
        .ok()?
        .get("pacer_target_wpm")?
        .as_f64()
}

pub fn next_target_wpm(current_target: f64, achieved_wpm: f64, accuracy: f64, min_wpm: f64) -> f64 {
    let target = if accuracy < 0.92 {
        current_target * 0.97
    } else if accuracy >= 0.95 && achieved_wpm >= current_target {
        current_target * 1.02
    } else {
        current_target
    };
    target.max(min_wpm)
}

pub fn cursor_position(target_wpm: f64, elapsed: Duration, grapheme_count: usize) -> usize {
    let characters_per_second = target_wpm.max(0.0) * 5.0 / 60.0;
    ((elapsed.as_secs_f64() * characters_per_second).floor() as usize).min(grapheme_count)
}
