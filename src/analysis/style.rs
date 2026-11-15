//! Typing-style metrics derived from word boundaries and key releases.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::analysis::model::PlayerModel;
use crate::engine::session::{KeystrokeKind, KeystrokeLog};
use crate::record::{SessionModeParams, SessionRecord};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WordStartMetric {
    pub start_median_ms: f64,
    pub internal_median_ms: f64,
    pub ratio: f64,
    pub n: u64,
    pub internal_n: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WordTiming {
    pub word: String,
    pub n: u64,
    pub observed_ms: f64,
    pub predicted_ms: f64,
    pub ratio: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RolloverMetric {
    pub rate: f64,
    pub comparable_pairs: u64,
    pub overlap_pairs: u64,
    pub overlap_median_ms: f64,
    pub overlap_p90_ms: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RhythmBreakdown {
    pub within_word_cv: Option<f64>,
    pub between_word_cv: Option<f64>,
    pub within_word_n: u64,
    pub between_word_n: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TypingStyleDiagnosis {
    pub word_start: Option<WordStartMetric>,
    /// Sorted from the strongest chunking (smallest ratio) to the weakest.
    pub words: Vec<WordTiming>,
    pub rollover: Option<RolloverMetric>,
    pub rhythm: RhythmBreakdown,
}

#[derive(Clone, Copy)]
struct TimedChar<'a> {
    key: &'a KeystrokeLog,
    character: char,
}

pub fn diagnose_style(records: &[SessionRecord], model: &PlayerModel) -> TypingStyleDiagnosis {
    let mut starts = Vec::new();
    let mut between_words = Vec::new();
    let mut internal = Vec::new();
    let mut word_samples = BTreeMap::<String, Vec<(f64, f64)>>::new();
    for record in records.iter().filter(|record| record.header.metrics_version > 0) {
        let mut previous_item_last = None::<TimedChar<'_>>;
        for item in &record.log.items {
            let keys = correct_chars(record, item.idx);
            let nonspace = keys.iter().copied().filter(|key| !key.character.is_whitespace()).collect::<Vec<_>>();
            if let (Some(previous), Some(first)) = (previous_item_last, nonspace.first().copied()) {
                let interval = first.key.t_us.saturating_sub(previous.key.t_us) as f64 / 1_000.0;
                between_words.push(interval);
                if item_boundary_was_visible(record) {
                    starts.push(interval);
                }
            }

            let mut last_nonspace = None::<TimedChar<'_>>;
            let mut crossed_space = false;
            for key in &keys {
                if key.character.is_whitespace() {
                    crossed_space = last_nonspace.is_some();
                    continue;
                }
                if let Some(previous) = last_nonspace {
                    let interval = key.key.t_us.saturating_sub(previous.key.t_us) as f64 / 1_000.0;
                    if crossed_space {
                        starts.push(interval);
                        between_words.push(interval);
                    } else {
                        internal.push(interval);
                    }
                }
                last_nonspace = Some(*key);
                crossed_space = false;
            }
            previous_item_last = nonspace.last().copied().or(previous_item_last);

            if !item.text.chars().any(char::is_whitespace)
                && keys.len() >= 2
                && record
                    .log
                    .keystrokes
                    .iter()
                    .filter(|key| key.item_idx == item.idx && key.kind == KeystrokeKind::Char)
                    .all(|key| key.correct)
            {
                let observed = keys.last().unwrap().key.t_us.saturating_sub(keys.first().unwrap().key.t_us) as f64 / 1_000.0;
                let predicted = keys
                    .windows(2)
                    .map(|pair| {
                        model
                            .transition(&pair[0].character.to_string(), &pair[1].character.to_string())
                            .map_or(model.personal_iki_median_ms, |row| row.estimated_iki_ms)
                    })
                    .sum::<f64>();
                if predicted > 0.0 {
                    word_samples.entry(item.text.clone()).or_default().push((observed, predicted));
                }
            }
        }
    }
    let word_start = match (median(&mut starts), median(&mut internal)) {
        (Some(start), Some(inside)) if inside > 0.0 => Some(WordStartMetric {
            start_median_ms: start,
            internal_median_ms: inside,
            ratio: start / inside,
            n: starts.len() as u64,
            internal_n: internal.len() as u64,
        }),
        _ => None,
    };
    let mut words = word_samples
        .into_iter()
        .filter(|(_, values)| values.len() >= 3)
        .filter_map(|(word, values)| {
            let mut observed = values.iter().map(|value| value.0).collect::<Vec<_>>();
            let mut predicted = values.iter().map(|value| value.1).collect::<Vec<_>>();
            let observed_ms = median(&mut observed)?;
            let predicted_ms = median(&mut predicted)?;
            (predicted_ms > 0.0).then_some(WordTiming {
                word,
                n: values.len() as u64,
                observed_ms,
                predicted_ms,
                ratio: observed_ms / predicted_ms,
            })
        })
        .collect::<Vec<_>>();
    words.sort_by(|left, right| left.ratio.total_cmp(&right.ratio).then_with(|| left.word.cmp(&right.word)));
    TypingStyleDiagnosis {
        word_start,
        words,
        rollover: rollover(records),
        rhythm: RhythmBreakdown {
            within_word_cv: coefficient_of_variation(&internal),
            between_word_cv: coefficient_of_variation(&between_words),
            within_word_n: internal.len() as u64,
            between_word_n: between_words.len() as u64,
        },
    }
}

fn item_boundary_was_visible(record: &SessionRecord) -> bool {
    serde_json::from_str::<SessionModeParams>(&record.header.mode_params)
        .ok()
        .and_then(|params| params.preview_items)
        .is_some_and(|count| count > 0)
}

fn correct_chars(record: &SessionRecord, item_idx: u32) -> Vec<TimedChar<'_>> {
    record
        .log
        .keystrokes
        .iter()
        .filter(|key| key.item_idx == item_idx && key.kind == KeystrokeKind::Char && key.correct)
        .filter_map(|key| {
            let mut characters = key.expected.as_deref()?.chars();
            let character = characters.next()?;
            characters.next().is_none().then_some(TimedChar { key, character })
        })
        .collect()
}

fn rollover(records: &[SessionRecord]) -> Option<RolloverMetric> {
    let mut comparable = 0_u64;
    let mut overlaps = Vec::new();
    for record in records
        .iter()
        .filter(|record| record.header.metrics_version > 0 && record.header.release_events)
    {
        for item in &record.log.items {
            let keys = record
                .log
                .keystrokes
                .iter()
                .filter(|key| key.item_idx == item.idx && key.kind == KeystrokeKind::Char)
                .collect::<Vec<_>>();
            for pair in keys.windows(2) {
                let Some(released) = pair[0].release_us else { continue };
                comparable += 1;
                if pair[1].t_us < released {
                    overlaps.push(released.saturating_sub(pair[1].t_us) as f64 / 1_000.0);
                }
            }
        }
    }
    if comparable == 0 {
        return None;
    }
    let overlap_pairs = overlaps.len() as u64;
    let overlap_median_ms = median(&mut overlaps).unwrap_or(0.0);
    let overlap_p90_ms = percentile(&mut overlaps, 0.9).unwrap_or(0.0);
    Some(RolloverMetric {
        rate: overlap_pairs as f64 / comparable as f64,
        comparable_pairs: comparable,
        overlap_pairs,
        overlap_median_ms,
        overlap_p90_ms,
    })
}

fn coefficient_of_variation(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    if mean <= 0.0 {
        return None;
    }
    let variance = values.iter().map(|value| (value - mean).powi(2)).sum::<f64>() / values.len() as f64;
    Some(variance.sqrt() / mean)
}

fn median(values: &mut [f64]) -> Option<f64> {
    percentile(values, 0.5)
}

fn percentile(values: &mut [f64], fraction: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let position = (values.len() - 1) as f64 * fraction;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    if lower == upper {
        Some(values[lower])
    } else {
        let weight = position - lower as f64;
        Some(values[lower] * (1.0 - weight) + values[upper] * weight)
    }
}
