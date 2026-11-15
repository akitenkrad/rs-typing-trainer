//! Cost-based weak-transition selection and deterministic prompt generation.

use std::collections::HashMap;

use rand::distributions::{Distribution, WeightedIndex};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};

use crate::analysis::metrics::DetailedMetrics;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FocusReason {
    Review,
    Improving,
    ErrorProne,
    #[default]
    Slow,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AdaptiveHistory {
    pub improving: Vec<(String, String)>,
    pub stalled: Vec<(String, String)>,
    pub excluded_controls: Vec<(String, String)>,
    pub due_reviews: Vec<(String, String, i64)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AdaptiveOptions {
    pub error_weight_ms: Option<f64>,
    pub drill_ratio: f64,
    pub min_transition_types: usize,
    pub min_transition_n: u64,
    pub max_share: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransitionCost {
    pub from: String,
    pub to: String,
    pub kind: String,
    pub frequency: u64,
    pub frequency_quartile: u8,
    pub observation_n: u64,
    pub estimated_iki_ms: f64,
    pub estimated_error_rate: f64,
    pub target_iki_ms: f64,
    pub cost: f64,
    pub weight: f64,
    pub sampled_iki_ms: f64,
    pub sampled_error_rate: f64,
    pub posterior_log_variance: f64,
    pub progress_multiplier: f64,
    pub reason_kind: FocusReason,
    pub review_days: Option<i64>,
}

impl TransitionCost {
    pub fn reason(&self, lang: &str) -> String {
        match (self.reason_kind, lang) {
            (FocusReason::Review, "ja") => format!("復習の時期（前回から {} 日）", self.review_days.unwrap_or(1)),
            (FocusReason::Review, _) => format!("Review due ({} days since last time)", self.review_days.unwrap_or(1)),
            (FocusReason::Improving, "ja") => "前回の処方から伸びている".to_owned(),
            (FocusReason::Improving, _) => "Improving since the previous prescription".to_owned(),
            (FocusReason::ErrorProne, "ja") => format!("誤りやすい（推定 {:.0}%）", self.estimated_error_rate * 100.0),
            (FocusReason::ErrorProne, _) => format!("Error-prone ({:.0}% estimated)", self.estimated_error_rate * 100.0),
            (FocusReason::Slow, "ja") => format!(
                "本人の目標より {:.0} ms 遅い（n={}）",
                (self.estimated_iki_ms - self.target_iki_ms).max(0.0),
                self.frequency
            ),
            (FocusReason::Slow, _) => format!("Slower than your target (n={})", self.frequency),
        }
    }
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
    build_plan_seeded(metrics, corpus, options, previous_focus, 0, &AdaptiveHistory::default())
}

pub fn build_plan_seeded(
    metrics: &DetailedMetrics,
    corpus: &[String],
    options: AdaptiveOptions,
    previous_focus: &[(String, String)],
    seed: u64,
    history: &AdaptiveHistory,
) -> AdaptivePlan {
    let mut rng = StdRng::seed_from_u64(seed);
    let eligible = metrics.bigrams.iter().filter(|row| row.n >= options.min_transition_n).collect::<Vec<_>>();
    let mut estimates = eligible.iter().map(|row| row.estimated_iki_ms).collect::<Vec<_>>();
    estimates.sort_by(f64::total_cmp);
    let target_iki_ms = estimates.get(estimates.len() / 2).copied().unwrap_or(0.0);
    let error_weight_ms = options.error_weight_ms.unwrap_or(target_iki_ms * 2.0);
    let mut due_reviews = history.due_reviews.clone();
    due_reviews.sort_by(|left, right| right.2.cmp(&left.2).then_with(|| (&left.0, &left.1).cmp(&(&right.0, &right.1))));
    due_reviews.truncate(2);
    let mut frequencies = HashMap::<(String, String), u64>::new();
    for word in corpus {
        let characters = word.chars().collect::<Vec<_>>();
        for pair in characters.windows(2) {
            *frequencies.entry((pair[0].to_string(), pair[1].to_string())).or_default() += 1;
        }
    }
    let mut corpus_frequencies = frequencies.values().copied().filter(|value| *value > 0).collect::<Vec<_>>();
    corpus_frequencies.sort_unstable();
    let mut transitions = eligible
        .iter()
        .filter_map(|row| {
            let key = (row.from.clone(), row.to.clone());
            if history.excluded_controls.contains(&key) {
                return None;
            }
            let frequency = frequencies.get(&key).copied().unwrap_or(0);
            let posterior_log_variance = posterior_log_variance(row.estimated_iki_lo_ms, row.estimated_iki_hi_ms);
            let sampled_iki_ms = (row.estimated_iki_ms.ln() + normal_sample(&mut rng) * posterior_log_variance.sqrt()).exp();
            let strength = row.attempts as f64 + 10.0;
            let mean = row.estimated_error_rate.clamp(1e-6, 1.0 - 1e-6);
            let sampled_error_rate = beta_sample(&mut rng, mean * strength, (1.0 - mean) * strength);
            let cost = frequency as f64 * ((sampled_iki_ms - target_iki_ms).max(0.0) + error_weight_ms * sampled_error_rate);
            let progress_multiplier = if history.improving.contains(&key) {
                1.2
            } else if history.stalled.contains(&key) {
                0.6
            } else {
                1.0
            };
            let review_days = due_reviews
                .iter()
                .find(|(from, to, _)| from == &row.from && to == &row.to)
                .map(|(_, _, days)| *days);
            let reason_kind = if review_days.is_some() {
                FocusReason::Review
            } else if progress_multiplier > 1.0 {
                FocusReason::Improving
            } else if row.estimated_error_rate >= 0.05 {
                FocusReason::ErrorProne
            } else {
                FocusReason::Slow
            };
            (frequency > 0).then(|| TransitionCost {
                from: row.from.clone(),
                to: row.to.clone(),
                kind: row.class.as_str().to_owned(),
                frequency,
                frequency_quartile: frequency_quartile(frequency, &corpus_frequencies),
                observation_n: row.n,
                estimated_iki_ms: row.estimated_iki_ms,
                estimated_error_rate: row.estimated_error_rate,
                target_iki_ms,
                cost,
                weight: (if previous_focus.contains(&key) { cost * 0.5 } else { cost }) * progress_multiplier,
                sampled_iki_ms,
                sampled_error_rate,
                posterior_log_variance,
                progress_multiplier,
                reason_kind,
                review_days,
            })
        })
        .collect::<Vec<_>>();
    transitions.sort_by(|left, right| {
        matches!(right.reason_kind, FocusReason::Review)
            .cmp(&matches!(left.reason_kind, FocusReason::Review))
            .then_with(|| right.weight.total_cmp(&left.weight))
    });
    AdaptivePlan {
        target_iki_ms,
        error_weight_ms,
        drill_ratio: options.drill_ratio.clamp(0.0, 1.0),
        max_share: options.max_share.clamp(f64::EPSILON, 1.0),
        fallback_to_words: eligible.len() < options.min_transition_types || !transitions.iter().any(|row| row.cost > 0.0),
        transitions,
    }
}

fn frequency_quartile(frequency: u64, sorted: &[u64]) -> u8 {
    if sorted.is_empty() {
        return 1;
    }
    let rank = sorted.partition_point(|value| *value <= frequency).max(1);
    ((rank * 4).div_ceil(sorted.len())).clamp(1, 4) as u8
}

fn posterior_log_variance(lo: f64, hi: f64) -> f64 {
    if lo > 0.0 && hi > lo {
        ((hi.ln() - lo.ln()) / (2.0 * 1.644_853_626_951_472_2)).powi(2)
    } else {
        0.0
    }
}

fn normal_sample(rng: &mut StdRng) -> f64 {
    let u1 = rng.r#gen::<f64>().clamp(f64::MIN_POSITIVE, 1.0);
    let u2 = rng.r#gen::<f64>();
    (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
}

fn gamma_sample(rng: &mut StdRng, shape: f64) -> f64 {
    if shape < 1.0 {
        return gamma_sample(rng, shape + 1.0) * rng.r#gen::<f64>().powf(1.0 / shape);
    }
    let d = shape - 1.0 / 3.0;
    let c = (1.0 / (9.0 * d)).sqrt();
    loop {
        let x = normal_sample(rng);
        let v = (1.0 + c * x).powi(3);
        if v <= 0.0 {
            continue;
        }
        let u = rng.r#gen::<f64>();
        if u < 1.0 - 0.0331 * x.powi(4) || u.ln() < 0.5 * x * x + d * (1.0 - v + v.ln()) {
            return d * v;
        }
    }
}

fn beta_sample(rng: &mut StdRng, alpha: f64, beta: f64) -> f64 {
    let left = gamma_sample(rng, alpha.max(1e-6));
    let right = gamma_sample(rng, beta.max(1e-6));
    left / (left + right)
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
    let mut output = kinds
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
        .collect::<Vec<_>>();
    let mut mixed = Vec::with_capacity(output.len());
    while !output.is_empty() {
        let previous = mixed.last().map(|item: &AdaptiveItem| &item.transition);
        let mut counts = HashMap::<(String, String), usize>::new();
        for item in &output {
            *counts.entry(item.transition.clone()).or_default() += 1;
        }
        let selected = output
            .iter()
            .enumerate()
            .filter(|(_, item)| previous != Some(&item.transition))
            .max_by_key(|(_, item)| counts.get(&item.transition).copied().unwrap_or_default())
            .map(|(index, _)| index)
            .unwrap_or(0);
        mixed.push(output.swap_remove(selected));
    }
    mixed
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
