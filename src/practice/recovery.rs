//! Recovery measurements following a mistyped character.

use crate::engine::session::{KeystrokeKind, SessionLog};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RecoveryMetrics {
    pub median_keys_to_recover: Option<f64>,
    pub next_three_error_rate: f64,
}

pub fn recovery_metrics(log: &SessionLog) -> RecoveryMetrics {
    let keys = log.keystrokes.iter().filter(|key| key.kind == KeystrokeKind::Char).collect::<Vec<_>>();
    if keys.len() < 2 {
        return RecoveryMetrics::default();
    }
    let mut intervals = keys
        .windows(2)
        .filter(|pair| pair[0].item_idx == pair[1].item_idx)
        .map(|pair| pair[1].t_us.saturating_sub(pair[0].t_us) as f64)
        .collect::<Vec<_>>();
    let median_us = (!intervals.is_empty()).then(|| median(&mut intervals));
    let mut recoveries = Vec::new();
    let mut following_errors = 0_u64;
    let mut following_total = 0_u64;
    for (index, _) in keys.iter().enumerate().filter(|(_, key)| !key.correct) {
        for next in keys.iter().skip(index + 1).take(3) {
            following_total += 1;
            following_errors += u64::from(!next.correct);
        }
        let recovered = median_us.and_then(|median_us| {
            (index + 1..keys.len()).find_map(|next| {
                if keys[next - 1].item_idx != keys[next].item_idx {
                    return None;
                }
                let interval = keys[next].t_us.saturating_sub(keys[next - 1].t_us) as f64;
                (keys[next].correct && interval <= median_us).then_some((next - index) as f64)
            })
        });
        if let Some(count) = recovered {
            recoveries.push(count);
        }
    }
    RecoveryMetrics {
        median_keys_to_recover: (!recoveries.is_empty()).then(|| median(&mut recoveries)),
        next_three_error_rate: if following_total == 0 {
            0.0
        } else {
            following_errors as f64 / following_total as f64
        },
    }
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}
