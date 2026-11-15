//! Per-key fading derived from shrinkage estimates.

use std::collections::HashMap;

use crate::analysis::metrics::{DetailedMetrics, shrink_mean, shrink_rate};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyFade {
    Dim,
    Hidden,
}

pub fn key_fade_states(metrics: &DetailedMetrics, shrinkage_k: f64) -> HashMap<String, KeyFade> {
    let total = metrics.keys.iter().map(|row| row.total).sum::<u64>();
    let errors = metrics.keys.iter().map(|row| row.total.saturating_sub(row.correct)).sum::<u64>();
    let prior_error = if total == 0 { 0.0 } else { errors as f64 / total as f64 };
    metrics
        .keys
        .iter()
        .filter_map(|row| {
            let accuracy = 1.0 - shrink_rate(row.total, row.total.saturating_sub(row.correct), prior_error, shrinkage_k);
            let iki = shrink_mean(row.iki_n, row.iki_median_ms, metrics.iki_median_ms, shrinkage_k);
            (row.total > 0 && row.iki_n > 0 && accuracy >= 0.97 && iki <= metrics.iki_median_ms).then(|| {
                let fade = if row.iki_n >= 30 { KeyFade::Hidden } else { KeyFade::Dim };
                (row.code.clone(), fade)
            })
        })
        .collect()
}
