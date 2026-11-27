//! Pure metronome timing and recommendation helpers.

use std::time::Duration;

/// Tempo of the rhythm mode outside the menu: 90% of the own median speed.
pub const DEFAULT_RHYTHM_RATIO: f64 = 0.9;

/// Maximum tempo used for the rhythm mode's visual pendulum.
pub const DISPLAY_MAX_BPM: f64 = 120.0;

/// Smallest supported divisor that brings the displayed tempo to 120 BPM or
/// below.
pub fn display_divisor(bpm: f64) -> u32 {
    if bpm <= 0.0 || !bpm.is_finite() {
        return 1;
    }

    [1, 2, 4, 8, 16]
        .into_iter()
        .find(|divisor| bpm / f64::from(*divisor) <= DISPLAY_MAX_BPM)
        .unwrap_or(16)
}

/// Position of the visual pendulum, from the left end (`0.0`) to the right
/// end (`1.0`).
pub fn pendulum_position(bpm: f64, divisor: u32, elapsed: Duration) -> f64 {
    if bpm <= 0.0 || !bpm.is_finite() || divisor == 0 {
        return 0.0;
    }

    let display_beats = elapsed.as_secs_f64() * bpm / (60.0 * f64::from(divisor));
    if !display_beats.is_finite() {
        return 0.0;
    }

    let cycle_position = display_beats % 2.0;
    if cycle_position <= 1.0 { cycle_position } else { 2.0 - cycle_position }
}

pub fn default_bpm(median_iki_ms: f64) -> f64 {
    bpm_for_ratio(median_iki_ms, DEFAULT_RHYTHM_RATIO)
}

/// Tempo at `ratio` × the own median speed (one beat per median IKI); 180
/// BPM without history. The menu's `rhythm.metronome` levels pass their
/// `pacer.ratio` (design §4.2, MYTASK-3619).
pub fn bpm_for_ratio(median_iki_ms: f64, ratio: f64) -> f64 {
    if median_iki_ms <= 0.0 { 180.0 } else { 60_000.0 / median_iki_ms * ratio }
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
