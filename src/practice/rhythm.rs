//! Pure metronome timing and recommendation helpers.

use std::time::Duration;

pub fn default_bpm(median_iki_ms: f64) -> f64 {
    if median_iki_ms <= 0.0 { 180.0 } else { 60_000.0 / median_iki_ms * 0.9 }
}

pub fn beat_index(bpm: f64, elapsed: Duration) -> u64 {
    if bpm <= 0.0 {
        return 0;
    }
    (elapsed.as_secs_f64() * bpm / 60.0).floor() as u64
}

pub fn median_beat_deviation_ms(bpm: f64, times: &[Duration]) -> f64 {
    if bpm <= 0.0 || times.is_empty() {
        return 0.0;
    }
    let period = 60_000.0 / bpm;
    let mut values = times
        .iter()
        .map(|time| {
            let ms = time.as_secs_f64() * 1_000.0;
            (ms - (ms / period).round() * period).abs()
        })
        .collect::<Vec<_>>();
    median(&mut values)
}

pub fn recommend_rhythm(rhythm_axis: f64, own_axis_distribution: &[f64]) -> bool {
    rhythm_axis < percentile(own_axis_distribution, 0.25)
}

fn percentile(values: &[f64], q: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let index = ((sorted.len() - 1) as f64 * q.clamp(0.0, 1.0)).floor() as usize;
    sorted[index]
}

fn median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}
