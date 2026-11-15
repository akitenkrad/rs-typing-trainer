//! Cost-based weak-transition selection and deterministic prompt generation.

use std::collections::HashMap;

use rand::distributions::{Distribution, WeightedIndex};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};

use crate::analysis::metrics::DetailedMetrics;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AdaptiveOptions {
    pub error_weight_ms: Option<f64>,
    pub drill_ratio: f64,
    pub min_transition_types: usize,
    pub max_share: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransitionCost {
    pub from: String,
    pub to: String,
    pub frequency: u64,
    pub estimated_iki_ms: f64,
    pub estimated_error_rate: f64,
    pub cost: f64,
    pub weight: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AdaptivePlan {
    pub target_iki_ms: f64,
    pub error_weight_ms: f64,
    pub drill_ratio: f64,
    pub max_share: f64,
    pub transitions: Vec<TransitionCost>,
    pub fallback_to_words: bool,
}

impl AdaptivePlan {
    pub fn warning(&self, lang: &str) -> &'static str {
        if lang == "ja" {
            "記録が足りないので通常の words で出題します"
        } else {
            "Not enough records; using normal words mode"
        }
    }

    pub fn focus(&self) -> Vec<(String, String)> {
        self.transitions.iter().take(5).map(|row| (row.from.clone(), row.to.clone())).collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdaptiveItemKind {
    Word,
    Drill,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdaptiveItem {
    pub text: String,
    pub reading: Option<String>,
    pub target: String,
    pub transition: (String, String),
    pub kind: AdaptiveItemKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdaptiveSource {
    pub text: String,
    pub reading: Option<String>,
    pub target: String,
}

pub fn build_plan(metrics: &DetailedMetrics, corpus: &[String], options: AdaptiveOptions, previous_focus: &[(String, String)]) -> AdaptivePlan {
    let mut estimates = metrics.bigrams.iter().map(|row| row.estimated_iki_ms).collect::<Vec<_>>();
    estimates.sort_by(f64::total_cmp);
    let target_iki_ms = estimates.get(estimates.len() / 2).copied().unwrap_or(0.0);
    let error_weight_ms = options.error_weight_ms.unwrap_or(target_iki_ms * 2.0);
    let mut frequencies = HashMap::<(String, String), u64>::new();
    for word in corpus {
        let characters = word.chars().collect::<Vec<_>>();
        for pair in characters.windows(2) {
            *frequencies.entry((pair[0].to_string(), pair[1].to_string())).or_default() += 1;
        }
    }
    let mut transitions = metrics
        .bigrams
        .iter()
        .filter_map(|row| {
            let key = (row.from.clone(), row.to.clone());
            let frequency = frequencies.get(&key).copied().unwrap_or(0);
            let cost =
                frequency as f64 * (row.estimated_iki_ms - target_iki_ms).max(0.0) + error_weight_ms * frequency as f64 * row.estimated_error_rate;
            (frequency > 0).then(|| TransitionCost {
                from: row.from.clone(),
                to: row.to.clone(),
                frequency,
                estimated_iki_ms: row.estimated_iki_ms,
                estimated_error_rate: row.estimated_error_rate,
                cost,
                weight: if previous_focus.contains(&key) { cost * 0.5 } else { cost },
            })
        })
        .collect::<Vec<_>>();
    transitions.sort_by(|left, right| right.weight.total_cmp(&left.weight));
    AdaptivePlan {
        target_iki_ms,
        error_weight_ms,
        drill_ratio: options.drill_ratio.clamp(0.0, 1.0),
        max_share: options.max_share.clamp(f64::EPSILON, 1.0),
        fallback_to_words: metrics.bigrams.len() < options.min_transition_types || !transitions.iter().any(|row| row.cost > 0.0),
        transitions,
    }
}

pub fn generate_items(plan: &AdaptivePlan, words: &[&str], count: usize, seed: u64) -> Vec<AdaptiveItem> {
    let sources = words
        .iter()
        .map(|word| AdaptiveSource {
            text: (*word).to_owned(),
            reading: None,
            target: (*word).to_owned(),
        })
        .collect::<Vec<_>>();
    generate_from_sources(plan, &sources, count, seed)
}

pub fn generate_from_sources(plan: &AdaptivePlan, sources: &[AdaptiveSource], count: usize, seed: u64) -> Vec<AdaptiveItem> {
    if count == 0 || sources.is_empty() {
        return Vec::new();
    }
    let mut rng = StdRng::seed_from_u64(seed);
    if plan.fallback_to_words || plan.transitions.is_empty() {
        return (0..count)
            .map(|index| {
                let source = &sources[index % sources.len()];
                AdaptiveItem {
                    text: source.text.clone(),
                    reading: source.reading.clone(),
                    target: source.target.clone(),
                    transition: (String::new(), String::new()),
                    kind: AdaptiveItemKind::Word,
                }
            })
            .collect();
    }
    let focus = plan.transitions.iter().take(5).collect::<Vec<_>>();
    let transitions = plan.transitions.iter().collect::<Vec<_>>();
    let focus_pairs = focus.iter().map(|row| format!("{}{}", row.from, row.to)).collect::<Vec<_>>();
    let max_count = ((count as f64 * plan.max_share).floor() as usize).max(1);
    let mut transition_counts = vec![0_usize; focus.len()];
    let drill_count = (count as f64 * plan.drill_ratio).round() as usize;
    let mut kinds = vec![AdaptiveItemKind::Drill; drill_count.min(count)];
    kinds.resize(count, AdaptiveItemKind::Word);
    kinds.shuffle(&mut rng);
    kinds
        .into_iter()
        .enumerate()
        .map(|(item_index, kind)| {
            let can_add = |target: &str| {
                focus_pairs
                    .iter()
                    .enumerate()
                    .all(|(index, pair)| !target.contains(pair) || transition_counts[index] < max_count)
            };
            let candidates_for = |index: usize| {
                let pair = format!("{}{}", transitions[index].from, transitions[index].to);
                match kind {
                    AdaptiveItemKind::Word => sources.iter().any(|source| source.target.contains(&pair) && can_add(&source.target)),
                    AdaptiveItemKind::Drill => can_add(&pair.repeat(3)),
                }
            };
            let available = (0..transitions.len())
                .filter(|index| (*index >= focus.len() || transition_counts[*index] < max_count) && candidates_for(*index))
                .collect::<Vec<_>>();
            let selected = if item_index < focus.len() && available.contains(&item_index) {
                item_index
            } else if available.is_empty() {
                (0..transitions.len())
                    .find(|index| candidates_for(*index))
                    .unwrap_or(item_index % transitions.len())
            } else {
                let weights = available
                    .iter()
                    .map(|index| transitions[*index].weight.max(f64::EPSILON))
                    .collect::<Vec<_>>();
                let distribution = WeightedIndex::new(&weights).expect("positive adaptive weights");
                available[distribution.sample(&mut rng)]
            };
            let transition = transitions[selected];
            let pair = format!("{}{}", transition.from, transition.to);
            let (text, reading, target, actual_kind) = if kind == AdaptiveItemKind::Word {
                let matches = sources
                    .iter()
                    .filter(|source| source.target.contains(&pair) && can_add(&source.target))
                    .collect::<Vec<_>>();
                if matches.is_empty() {
                    let drill = pair.repeat(3);
                    (drill.clone(), None, drill, AdaptiveItemKind::Drill)
                } else {
                    let source = matches[rng.gen_range(0..matches.len())];
                    (source.text.clone(), source.reading.clone(), source.target.clone(), AdaptiveItemKind::Word)
                }
            } else {
                let drill = pair.repeat(3);
                (drill.clone(), None, drill, AdaptiveItemKind::Drill)
            };
            for (index, focus_pair) in focus_pairs.iter().enumerate() {
                transition_counts[index] += usize::from(target.contains(focus_pair));
            }
            AdaptiveItem {
                text,
                reading,
                target,
                transition: (transition.from.clone(), transition.to.clone()),
                kind: actual_kind,
            }
        })
        .collect()
}

pub fn transition_share<'a>(items: impl IntoIterator<Item = &'a str>, transition: (&str, &str)) -> f64 {
    let pair = format!("{}{}", transition.0, transition.1);
    let mut total = 0_u64;
    let mut matching = 0_u64;
    for item in items {
        total += 1;
        matching += u64::from(item.contains(&pair));
    }
    if total == 0 { 0.0 } else { matching as f64 / total as f64 }
}
