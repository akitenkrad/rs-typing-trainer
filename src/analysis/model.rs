//! Hierarchical, time-weighted estimates of a player's key transitions.
//!
//! Finger and hand groups use the standard touch-typing assignments from the
//! selected keyboard geometry. They are estimates of technique, not claims
//! about which finger the player actually used.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::analysis::metrics::AnalysisSettings;
use crate::engine::romaji::RomajiJudge;
use crate::engine::session::{KeystrokeKind, KeystrokeLog};
use crate::layout::KeyboardLayout;
use crate::layout::geometry::{Finger, KeyboardGeometry};
use crate::record::SessionRecord;

pub const MODEL_VERSION: i64 = 2;
const NORMAL_90: f64 = 1.644_853_626_951_472_2;
const DEFAULT_LOG_VARIANCE: f64 = 0.04;
const SHRINKAGE_K_MIN: f64 = 3.0;
const SHRINKAGE_K_MAX: f64 = 100.0;
const ERROR_PRIOR_MIN: f64 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelView {
    Recent,
    AllTime,
}

impl ModelView {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recent => "recent",
            Self::AllTime => "all-time",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransitionStatistic {
    pub from: String,
    pub to: String,
    pub from_hand: String,
    pub to_hand: String,
    pub from_finger: String,
    pub to_finger: String,
    pub weight: f64,
    pub log_iki_sum: f64,
    pub log_iki_sq_sum: f64,
    pub error_weight: f64,
    pub attempt_weight: f64,
    pub observations: u64,
    pub errors: u64,
    pub last_added_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SufficientStatistics {
    pub model_version: i64,
    pub layout: String,
    pub language: String,
    pub view: ModelView,
    pub half_life_days: f64,
    pub as_of: DateTime<Utc>,
    pub last_session_id: Option<i64>,
    pub rows: Vec<TransitionStatistic>,
}

impl SufficientStatistics {
    pub fn approx_eq(&self, other: &Self, tolerance: f64) -> bool {
        self.model_version == other.model_version
            && self.layout == other.layout
            && self.language == other.language
            && self.view == other.view
            && (self.half_life_days - other.half_life_days).abs() <= tolerance
            && self.as_of == other.as_of
            && self.last_session_id == other.last_session_id
            && self.rows.len() == other.rows.len()
            && self.rows.iter().zip(&other.rows).all(|(left, right)| {
                left.from == right.from
                    && left.to == right.to
                    && left.from_hand == right.from_hand
                    && left.to_hand == right.to_hand
                    && left.from_finger == right.from_finger
                    && left.to_finger == right.to_finger
                    && (left.weight - right.weight).abs() <= tolerance
                    && (left.log_iki_sum - right.log_iki_sum).abs() <= tolerance
                    && (left.log_iki_sq_sum - right.log_iki_sq_sum).abs() <= tolerance
                    && (left.error_weight - right.error_weight).abs() <= tolerance
                    && (left.attempt_weight - right.attempt_weight).abs() <= tolerance
                    && left.observations == right.observations
                    && left.errors == right.errors
                    && left.last_added_at == right.last_added_at
            })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransitionEstimate {
    pub from: String,
    pub to: String,
    pub from_hand: String,
    pub to_hand: String,
    pub from_finger: String,
    pub to_finger: String,
    pub n: u64,
    pub effective_n: f64,
    pub attempts: u64,
    pub effective_attempts: f64,
    pub sample_iki_ms: f64,
    pub parent_iki_ms: f64,
    pub estimated_iki_ms: f64,
    pub estimated_iki_lo_ms: f64,
    pub estimated_iki_hi_ms: f64,
    pub estimated_error_rate: f64,
    pub relative_iki: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EstimateSource {
    Transition,
    FingerPair,
    HandPair,
    Overall,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResolvedTransitionEstimate {
    pub estimated_iki_ms: f64,
    pub estimated_error_rate: f64,
    pub effective_n: f64,
    pub source: EstimateSource,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct KeyHierarchy {
    hand: String,
    finger: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlayerModel {
    pub view: ModelView,
    pub as_of: DateTime<Utc>,
    pub personal_iki_median_ms: f64,
    pub reaction_median_ms: f64,
    pub word_start_ratio: f64,
    pub transitions: Vec<TransitionEstimate>,
    pub spelling_preferences: BTreeMap<String, String>,
    #[serde(default)]
    overall_estimate: Option<NodeEstimate>,
    #[serde(default)]
    hand_pair_estimates: BTreeMap<String, NodeEstimate>,
    #[serde(default)]
    finger_pair_estimates: BTreeMap<String, NodeEstimate>,
    #[serde(default)]
    key_hierarchies: BTreeMap<String, KeyHierarchy>,
}

impl PlayerModel {
    pub fn transition(&self, from: &str, to: &str) -> Option<&TransitionEstimate> {
        self.transitions.iter().find(|row| row.from == from && row.to == to)
    }

    /// Resolves a transition through the standard touch-typing hierarchy.
    ///
    /// An observed transition is returned directly.  Otherwise the estimate
    /// falls back through its finger pair, hand pair, and the player's
    /// overall estimate.  Keys without a standard finger assignment skip the
    /// unavailable hierarchy levels.
    pub fn estimate_or_parent(&self, from: &str, to: &str) -> Option<ResolvedTransitionEstimate> {
        if let Some(row) = self.transition(from, to) {
            return Some(ResolvedTransitionEstimate {
                estimated_iki_ms: row.estimated_iki_ms,
                estimated_error_rate: row.estimated_error_rate,
                effective_n: row.effective_n,
                source: EstimateSource::Transition,
            });
        }
        let from_hierarchy = self.key_hierarchies.get(from);
        let to_hierarchy = self.key_hierarchies.get(to);
        if let Some((from_hierarchy, to_hierarchy)) = from_hierarchy.zip(to_hierarchy) {
            let hand = pair_key(&from_hierarchy.hand, &to_hierarchy.hand);
            let finger = pair_key(&from_hierarchy.finger, &to_hierarchy.finger);
            if let Some(node) = hand
                .as_ref()
                .zip(finger.as_ref())
                .and_then(|(hand, finger)| self.finger_pair_estimates.get(&finger_pair_key(hand, finger)))
            {
                return Some(node.resolved(EstimateSource::FingerPair));
            }
            if let Some(node) = hand.as_ref().and_then(|hand| self.hand_pair_estimates.get(hand)) {
                return Some(node.resolved(EstimateSource::HandPair));
            }
        }
        self.overall_estimate.map(|node| node.resolved(EstimateSource::Overall)).or_else(|| {
            (self.personal_iki_median_ms > 0.0).then_some(ResolvedTransitionEstimate {
                estimated_iki_ms: self.personal_iki_median_ms,
                estimated_error_rate: 0.0,
                effective_n: 0.0,
                source: EstimateSource::Overall,
            })
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpectedPrompt {
    pub text: String,
    pub reading: Option<String>,
}

impl ExpectedPrompt {
    pub fn english(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            reading: None,
        }
    }

    pub fn japanese(reading: impl Into<String>) -> Self {
        let reading = reading.into();
        Self {
            text: reading.clone(),
            reading: Some(reading),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExpectedPerformance {
    pub keystrokes: String,
    pub transition_ms: f64,
    pub word_start_delay_ms: f64,
    pub reaction_ms: f64,
    pub total_ms: f64,
    pub expected_wpm: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransitionKind {
    SameHand,
    AlternatingHands,
    SameFinger,
    RepeatedKey,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransitionKindEffect {
    pub kind: TransitionKind,
    pub transitions: usize,
    pub frequency: f64,
    pub difference_ms: f64,
    pub weighted_per_keystroke_ms: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeyMapMetric {
    pub key: String,
    pub relative_iki: f64,
    pub effective_n: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransitionMapMetric {
    pub from: String,
    pub to: String,
    pub relative_iki: f64,
    pub error_rate: f64,
    pub effective_n: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TransitionMap {
    pub keys: Vec<KeyMapMetric>,
    pub transitions: Vec<TransitionMapMetric>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Prescription {
    pub id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub layout: String,
    pub kind: String,
    pub target: serde_json::Value,
    pub control: serde_json::Value,
    pub hypothesis: serde_json::Value,
    pub baseline: serde_json::Value,
    pub status: String,
    pub evaluated_at: Option<DateTime<Utc>>,
    pub result: Option<serde_json::Value>,
}

#[derive(Clone, Copy, Debug)]
struct Physical {
    finger: Finger,
}

#[derive(Clone, Debug)]
struct Observation {
    from: String,
    to: String,
    from_hand: String,
    to_hand: String,
    from_finger: String,
    to_finger: String,
    log_iki: Option<f64>,
    error: bool,
    at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Default)]
struct Moments {
    weight: f64,
    sum: f64,
    sum_sq: f64,
    error: f64,
    attempts: f64,
}

impl Moments {
    fn add(&mut self, row: &TransitionStatistic) {
        self.weight += row.weight;
        self.sum += row.log_iki_sum;
        self.sum_sq += row.log_iki_sq_sum;
        self.error += row.error_weight;
        self.attempts += row.attempt_weight;
    }

    fn mean(self) -> f64 {
        if self.weight > 0.0 { self.sum / self.weight } else { 0.0 }
    }

    fn variance(self) -> f64 {
        if self.weight > 0.0 {
            (self.sum_sq / self.weight - self.mean().powi(2)).max(0.0)
        } else {
            0.0
        }
    }

    fn error_rate(self) -> f64 {
        if self.attempts > 0.0 { self.error / self.attempts } else { 0.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct NodeEstimate {
    log_mean: f64,
    variance: f64,
    error_rate: f64,
    effective_n: f64,
}

impl NodeEstimate {
    fn resolved(self, source: EstimateSource) -> ResolvedTransitionEstimate {
        ResolvedTransitionEstimate {
            estimated_iki_ms: self.log_mean.exp(),
            estimated_error_rate: self.error_rate,
            effective_n: self.effective_n,
            source,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Shrinkage {
    k: f64,
    within_variance: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ParentKey {
    Root,
    Hand(String),
    Finger(String, String),
}

#[derive(Clone, Debug)]
struct HierarchicalEstimates {
    transitions: Vec<TransitionEstimate>,
    overall: Option<NodeEstimate>,
    hands: BTreeMap<String, NodeEstimate>,
    fingers: BTreeMap<(String, String), NodeEstimate>,
}

pub fn build_sufficient_statistics(
    records: &[SessionRecord],
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
    settings: AnalysisSettings,
    view: ModelView,
    as_of: DateTime<Utc>,
) -> SufficientStatistics {
    let mut statistics = SufficientStatistics {
        model_version: MODEL_VERSION,
        layout: layout.name().to_owned(),
        language: languages(records),
        view,
        half_life_days: settings.half_life_days,
        as_of,
        last_session_id: records.iter().filter_map(|record| record.header.id).max(),
        rows: Vec::new(),
    };
    add_records(&mut statistics, records, layout, geometry, settings, as_of);
    statistics
}

pub fn update_sufficient_statistics(
    statistics: &mut SufficientStatistics,
    records: &[SessionRecord],
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
    settings: AnalysisSettings,
    as_of: DateTime<Utc>,
) {
    if statistics.model_version != MODEL_VERSION
        || statistics.layout != layout.name()
        || statistics.view == ModelView::Recent && (statistics.half_life_days - settings.half_life_days).abs() > f64::EPSILON
    {
        *statistics = build_sufficient_statistics(records, layout, geometry, settings, statistics.view, as_of);
        return;
    }
    let records = records
        .iter()
        .filter(|record| {
            statistics
                .last_session_id
                .zip(record.header.id)
                .is_none_or(|(last, current)| current > last)
        })
        .cloned()
        .collect::<Vec<_>>();
    if records.is_empty() {
        return;
    }
    append_sufficient_statistics(statistics, &records, layout, geometry, settings, as_of);
}

/// Appends records to an in-memory sufficient-statistics stream without
/// applying the database-cache session-id filter.
///
/// Chronological backtests use this entry point because time order, rather
/// than database insertion order, defines which observations are available.
pub fn append_sufficient_statistics(
    statistics: &mut SufficientStatistics,
    records: &[SessionRecord],
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
    settings: AnalysisSettings,
    as_of: DateTime<Utc>,
) {
    if statistics.model_version != MODEL_VERSION
        || statistics.layout != layout.name()
        || statistics.view == ModelView::Recent && (statistics.half_life_days - settings.half_life_days).abs() > f64::EPSILON
    {
        *statistics = build_sufficient_statistics(records, layout, geometry, settings, statistics.view, as_of);
        return;
    }
    let target_as_of = statistics.as_of.max(as_of);
    if statistics.view == ModelView::Recent && target_as_of > statistics.as_of {
        let factor = decay_weight(statistics.as_of, target_as_of, settings.half_life_days);
        for row in &mut statistics.rows {
            row.weight *= factor;
            row.log_iki_sum *= factor;
            row.log_iki_sq_sum *= factor;
            row.error_weight *= factor;
            row.attempt_weight *= factor;
        }
    }
    statistics.as_of = target_as_of;
    let new_languages = languages(records);
    if statistics.language != new_languages {
        statistics.language = "all".to_owned();
    }
    statistics.last_session_id = statistics.last_session_id.max(records.iter().filter_map(|record| record.header.id).max());
    add_records(statistics, records, layout, geometry, settings, target_as_of);
}

pub fn build_player_model(
    records: &[SessionRecord],
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
    settings: AnalysisSettings,
    view: ModelView,
    as_of: DateTime<Utc>,
) -> PlayerModel {
    let statistics = build_sufficient_statistics(records, layout, geometry, settings, view, as_of);
    let auxiliary = auxiliary_samples(records, settings, view, as_of);
    let baseline = weighted_median(&auxiliary.intervals).unwrap_or_else(|| {
        let root = aggregate(statistics.rows.iter());
        if root.weight > 0.0 { root.mean().exp() } else { 0.0 }
    });
    let reaction = weighted_median(&auxiliary.reactions).unwrap_or(0.0);
    let word_start_ratio = match (weighted_median(&auxiliary.word_starts), weighted_median(&auxiliary.word_internal)) {
        (Some(start), Some(internal)) if internal > 0.0 => start / internal,
        _ => 1.0,
    };
    let spelling_preferences = auxiliary
        .spellings
        .into_iter()
        .filter_map(|(reading, variants)| {
            variants
                .into_iter()
                .max_by(|left, right| left.1.total_cmp(&right.1).then_with(|| right.0.cmp(&left.0)))
                .map(|(spelling, _)| (reading, spelling))
        })
        .collect();
    let hierarchy = hierarchical_estimates(&statistics, settings.shrinkage_k, baseline);
    PlayerModel {
        view,
        as_of,
        personal_iki_median_ms: baseline,
        reaction_median_ms: reaction,
        word_start_ratio,
        transitions: hierarchy.transitions,
        spelling_preferences,
        overall_estimate: hierarchy.overall,
        hand_pair_estimates: hierarchy.hands,
        finger_pair_estimates: hierarchy
            .fingers
            .into_iter()
            .map(|((hand, finger), estimate)| (finger_pair_key(&hand, &finger), estimate))
            .collect(),
        key_hierarchies: layout_key_hierarchies(layout, geometry),
    }
}

/// Builds a transition-only player model from an incrementally maintained
/// sufficient-statistics cache.
///
/// This is used for chronological backtesting, where rebuilding the model
/// from every earlier keystroke for every session would be quadratic.  The
/// transition estimates are identical to a rebuild at `as_of` (apart from
/// floating-point accumulation order).  Auxiliary medians that are not part
/// of the cache use neutral values: no reaction delay, a word-start ratio of
/// one, and the romaji automaton's default spellings.
pub fn build_player_model_from_statistics(statistics: &SufficientStatistics, settings: AnalysisSettings, as_of: DateTime<Utc>) -> PlayerModel {
    build_player_model_from_statistics_inner(statistics, settings, as_of, observed_key_hierarchies(statistics))
}

/// Builds an incremental player model with the selected layout's complete
/// standard touch-typing hierarchy available for unseen transitions.
pub fn build_player_model_from_statistics_with_layout(
    statistics: &SufficientStatistics,
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
    settings: AnalysisSettings,
    as_of: DateTime<Utc>,
) -> PlayerModel {
    build_player_model_from_statistics_inner(statistics, settings, as_of, layout_key_hierarchies(layout, geometry))
}

fn build_player_model_from_statistics_inner(
    statistics: &SufficientStatistics,
    settings: AnalysisSettings,
    as_of: DateTime<Utc>,
    key_hierarchies: BTreeMap<String, KeyHierarchy>,
) -> PlayerModel {
    let mut statistics = statistics.clone();
    if statistics.view == ModelView::Recent && as_of > statistics.as_of {
        let factor = decay_weight(statistics.as_of, as_of, settings.half_life_days);
        for row in &mut statistics.rows {
            row.weight *= factor;
            row.log_iki_sum *= factor;
            row.log_iki_sq_sum *= factor;
            row.error_weight *= factor;
            row.attempt_weight *= factor;
        }
        statistics.as_of = as_of;
    }
    let root = aggregate(statistics.rows.iter());
    let baseline = if root.weight > 0.0 { root.mean().exp() } else { 0.0 };
    let hierarchy = hierarchical_estimates(&statistics, settings.shrinkage_k, baseline);
    PlayerModel {
        view: statistics.view,
        as_of,
        personal_iki_median_ms: baseline,
        reaction_median_ms: 0.0,
        word_start_ratio: 1.0,
        transitions: hierarchy.transitions,
        spelling_preferences: BTreeMap::new(),
        overall_estimate: hierarchy.overall,
        hand_pair_estimates: hierarchy.hands,
        finger_pair_estimates: hierarchy
            .fingers
            .into_iter()
            .map(|((hand, finger), estimate)| (finger_pair_key(&hand, &finger), estimate))
            .collect(),
        key_hierarchies,
    }
}

pub fn expected_performance(model: &PlayerModel, prompt: &ExpectedPrompt) -> ExpectedPerformance {
    let keystrokes = prompt.reading.as_ref().map_or_else(
        || prompt.text.clone(),
        |reading| RomajiJudge::new(reading).preferred_spelling(&model.spelling_preferences),
    );
    let characters = keystrokes.chars().collect::<Vec<_>>();
    let transition_ms = characters
        .windows(2)
        .map(|pair| {
            model
                .estimate_or_parent(&pair[0].to_string(), &pair[1].to_string())
                .map_or(0.0, |row| row.estimated_iki_ms)
        })
        .sum::<f64>();
    let word_starts = keystrokes.split_whitespace().count().saturating_sub(1);
    let word_start_delay_ms = word_starts as f64 * model.personal_iki_median_ms * (model.word_start_ratio - 1.0).max(0.0);
    let reaction_ms = model.reaction_median_ms;
    let total_ms = transition_ms + word_start_delay_ms + reaction_ms;
    let expected_wpm = if total_ms > 0.0 {
        (characters.len() as f64 / 5.0) / (total_ms / 60_000.0)
    } else {
        0.0
    };
    ExpectedPerformance {
        keystrokes,
        transition_ms,
        word_start_delay_ms,
        reaction_ms,
        total_ms,
        expected_wpm,
    }
}

pub fn transition_kind_effects(model: &PlayerModel) -> Vec<TransitionKindEffect> {
    let mut groups = BTreeMap::<TransitionKind, Vec<&TransitionEstimate>>::new();
    for row in &model.transitions {
        groups.entry(transition_kind(row)).or_default().push(row);
    }
    [
        TransitionKind::SameHand,
        TransitionKind::AlternatingHands,
        TransitionKind::SameFinger,
        TransitionKind::RepeatedKey,
    ]
    .into_iter()
    .map(|kind| {
        let rows = groups.remove(&kind).unwrap_or_default();
        let frequency = rows.iter().map(|row| row.effective_n).sum::<f64>();
        let unweighted = if rows.is_empty() {
            0.0
        } else {
            rows.iter().map(|row| row.estimated_iki_ms - model.personal_iki_median_ms).sum::<f64>() / rows.len() as f64
        };
        let weighted = if frequency > 0.0 {
            rows.iter()
                .map(|row| row.effective_n * (row.estimated_iki_ms - model.personal_iki_median_ms))
                .sum::<f64>()
                / frequency
        } else {
            0.0
        };
        TransitionKindEffect {
            kind,
            transitions: rows.len(),
            frequency,
            difference_ms: unweighted,
            weighted_per_keystroke_ms: weighted,
        }
    })
    .collect()
}

pub fn transition_map(model: &PlayerModel) -> TransitionMap {
    let mut incoming = BTreeMap::<String, (f64, f64)>::new();
    for row in &model.transitions {
        let entry = incoming.entry(row.to.clone()).or_default();
        entry.0 += row.relative_iki * row.effective_n;
        entry.1 += row.effective_n;
    }
    TransitionMap {
        keys: incoming
            .into_iter()
            .map(|(key, (weighted, effective_n))| KeyMapMetric {
                key,
                relative_iki: if effective_n > 0.0 { weighted / effective_n } else { 0.0 },
                effective_n,
            })
            .collect(),
        transitions: model
            .transitions
            .iter()
            .map(|row| TransitionMapMetric {
                from: row.from.clone(),
                to: row.to.clone(),
                relative_iki: row.relative_iki,
                error_rate: row.estimated_error_rate,
                effective_n: row.effective_n,
            })
            .collect(),
    }
}

fn add_records(
    statistics: &mut SufficientStatistics,
    records: &[SessionRecord],
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
    settings: AnalysisSettings,
    as_of: DateTime<Utc>,
) {
    let mut rows = statistics
        .rows
        .drain(..)
        .map(|row| ((row.from.clone(), row.to.clone()), row))
        .collect::<BTreeMap<_, _>>();
    for observation in observations(records, layout, geometry, settings.pause_ms) {
        let weight = match statistics.view {
            ModelView::Recent => decay_weight(observation.at, as_of, settings.half_life_days),
            ModelView::AllTime => 1.0,
        };
        let entry = rows
            .entry((observation.from.clone(), observation.to.clone()))
            .or_insert_with(|| TransitionStatistic {
                from: observation.from.clone(),
                to: observation.to.clone(),
                from_hand: observation.from_hand.clone(),
                to_hand: observation.to_hand.clone(),
                from_finger: observation.from_finger.clone(),
                to_finger: observation.to_finger.clone(),
                weight: 0.0,
                log_iki_sum: 0.0,
                log_iki_sq_sum: 0.0,
                error_weight: 0.0,
                attempt_weight: 0.0,
                observations: 0,
                errors: 0,
                last_added_at: observation.at,
            });
        entry.attempt_weight += weight;
        entry.error_weight += weight * f64::from(observation.error);
        entry.errors += u64::from(observation.error);
        if let Some(log_iki) = observation.log_iki {
            entry.weight += weight;
            entry.log_iki_sum += weight * log_iki;
            entry.log_iki_sq_sum += weight * log_iki * log_iki;
            entry.observations += 1;
        }
        entry.last_added_at = entry.last_added_at.max(observation.at);
    }
    statistics.rows = rows.into_values().collect();
}

fn observations(records: &[SessionRecord], layout: &KeyboardLayout, geometry: &KeyboardGeometry, pause_ms: u64) -> Vec<Observation> {
    let physical = geometry
        .rows
        .iter()
        .flatten()
        .map(|key| (key.code, Physical { finger: key.finger }))
        .collect::<HashMap<_, _>>();
    let pause_us = pause_ms.saturating_mul(1_000);
    let mut output = Vec::new();
    for record in records
        .iter()
        .filter(|record| record.header.metrics_version > 0 && record.header.layout == layout.name())
    {
        let Some(at) = parse_time(&record.header.finished_at) else { continue };
        let char_keys = record
            .log
            .keystrokes
            .iter()
            .filter(|key| key.kind == KeystrokeKind::Char)
            .collect::<Vec<_>>();
        for item in &record.log.items {
            let item_keys = char_keys.iter().copied().filter(|key| key.item_idx == item.idx).collect::<Vec<_>>();
            for pair in item_keys.windows(2) {
                if !pair[0].correct {
                    continue;
                }
                let delta_us = pair[1].t_us.saturating_sub(pair[0].t_us);
                if delta_us > pause_us {
                    continue;
                }
                let Some((from, from_physical)) = expected_physical(pair[0], layout, &physical) else {
                    continue;
                };
                let Some((to, to_physical)) = expected_physical(pair[1], layout, &physical) else {
                    continue;
                };
                let (from_hand, from_finger) = hierarchy_names(from_physical);
                let (to_hand, to_finger) = hierarchy_names(to_physical);
                output.push(Observation {
                    from,
                    to,
                    from_hand: from_hand.to_owned(),
                    to_hand: to_hand.to_owned(),
                    from_finger: from_finger.to_owned(),
                    to_finger: to_finger.to_owned(),
                    log_iki: (pair[1].correct && delta_us > 0).then(|| (delta_us as f64 / 1_000.0).ln()),
                    error: !pair[1].correct,
                    at,
                });
            }
        }
    }
    output
}

fn hierarchical_estimates(statistics: &SufficientStatistics, fallback_k: f64, baseline: f64) -> HierarchicalEstimates {
    if statistics.rows.is_empty() {
        return HierarchicalEstimates {
            transitions: Vec::new(),
            overall: None,
            hands: BTreeMap::new(),
            fingers: BTreeMap::new(),
        };
    }
    let root_moments = aggregate(statistics.rows.iter());
    let root = NodeEstimate {
        log_mean: root_moments.mean(),
        variance: positive_variance(root_moments.variance(), DEFAULT_LOG_VARIANCE),
        error_rate: root_moments.error_rate(),
        effective_n: root_moments.weight,
    };
    let mut hand_groups = BTreeMap::<String, Moments>::new();
    for row in &statistics.rows {
        if let Some(key) = hand_key(row) {
            hand_groups.entry(key).or_default().add(row);
        }
    }
    let hand_shrinkage = estimate_shrinkage(hand_groups.values().copied(), fallback_k, root.variance);
    let hand_estimates = hand_groups
        .iter()
        .map(|(key, moments)| (key.clone(), shrink_node(*moments, root, hand_shrinkage)))
        .collect::<BTreeMap<_, _>>();

    let mut finger_groups = BTreeMap::<(String, String), Moments>::new();
    for row in &statistics.rows {
        if let (Some(hand), Some(finger)) = (hand_key(row), finger_key(row)) {
            finger_groups.entry((hand, finger)).or_default().add(row);
        }
    }
    let mut finger_estimates = BTreeMap::<(String, String), NodeEstimate>::new();
    for (hand, parent) in &hand_estimates {
        let children = finger_groups
            .iter()
            .filter(|((candidate, _), _)| candidate == hand)
            .map(|(_, moments)| *moments)
            .collect::<Vec<_>>();
        let shrinkage = estimate_shrinkage(children.iter().copied(), fallback_k, parent.variance);
        for ((candidate_hand, finger), moments) in finger_groups.iter().filter(|((candidate, _), _)| candidate == hand) {
            finger_estimates.insert((candidate_hand.clone(), finger.clone()), shrink_node(*moments, *parent, shrinkage));
        }
    }

    let mut transitions_by_parent = BTreeMap::<ParentKey, Vec<&TransitionStatistic>>::new();
    for row in &statistics.rows {
        transitions_by_parent.entry(parent_key(row)).or_default().push(row);
    }
    let mut estimates = Vec::new();
    for (parent_key, rows) in transitions_by_parent {
        let parent = match &parent_key {
            ParentKey::Root => root,
            ParentKey::Hand(hand) => hand_estimates.get(hand).copied().unwrap_or(root),
            ParentKey::Finger(hand, finger) => finger_estimates
                .get(&(hand.clone(), finger.clone()))
                .copied()
                .or_else(|| hand_estimates.get(hand).copied())
                .unwrap_or(root),
        };
        let shrinkage = estimate_shrinkage(rows.iter().map(|row| moments(row)), fallback_k, parent.variance);
        for row in rows {
            let row_moments = moments(row);
            let estimate = shrink_node(row_moments, parent, shrinkage);
            let standard_error = (shrinkage.within_variance / (row.weight + shrinkage.k).max(f64::EPSILON)).sqrt();
            let sample_iki_ms = if row.weight > 0.0 {
                row_moments.mean().exp()
            } else {
                parent.log_mean.exp()
            };
            let estimated_iki_ms = estimate.log_mean.exp();
            estimates.push(TransitionEstimate {
                from: row.from.clone(),
                to: row.to.clone(),
                from_hand: row.from_hand.clone(),
                to_hand: row.to_hand.clone(),
                from_finger: row.from_finger.clone(),
                to_finger: row.to_finger.clone(),
                n: row.observations,
                effective_n: row.weight,
                attempts: row.observations + row.errors,
                effective_attempts: row.attempt_weight,
                sample_iki_ms,
                parent_iki_ms: parent.log_mean.exp(),
                estimated_iki_ms,
                estimated_iki_lo_ms: (estimate.log_mean - NORMAL_90 * standard_error).exp(),
                estimated_iki_hi_ms: (estimate.log_mean + NORMAL_90 * standard_error).exp(),
                estimated_error_rate: estimate.error_rate,
                relative_iki: if baseline > 0.0 { estimated_iki_ms / baseline } else { 0.0 },
            });
        }
    }
    estimates.sort_by(|left, right| (&left.from, &left.to).cmp(&(&right.from, &right.to)));
    HierarchicalEstimates {
        transitions: estimates,
        overall: Some(root),
        hands: hand_estimates,
        fingers: finger_estimates,
    }
}

fn shrink_node(sample: Moments, parent: NodeEstimate, shrinkage: Shrinkage) -> NodeEstimate {
    let k = shrinkage.k;
    let log_mean = if sample.weight + k > 0.0 {
        (sample.sum + k * parent.log_mean) / (sample.weight + k)
    } else {
        parent.log_mean
    };
    let error_k = k.max(ERROR_PRIOR_MIN);
    let error_rate = if sample.attempts + error_k > 0.0 {
        (sample.error + error_k * parent.error_rate) / (sample.attempts + error_k)
    } else {
        parent.error_rate
    };
    NodeEstimate {
        log_mean,
        variance: shrinkage.within_variance,
        error_rate,
        effective_n: sample.weight,
    }
}

fn estimate_shrinkage(samples: impl Iterator<Item = Moments>, fallback_k: f64, fallback_variance: f64) -> Shrinkage {
    let samples = samples.filter(|sample| sample.weight > 0.0).collect::<Vec<_>>();
    let total_weight = samples.iter().map(|sample| sample.weight).sum::<f64>();
    let residual_df = total_weight - samples.len() as f64;
    let residual_sum_squares = samples
        .iter()
        .map(|sample| (sample.sum_sq - sample.sum.powi(2) / sample.weight).max(0.0))
        .sum::<f64>();
    let within_variance = if residual_df > 0.0 {
        positive_variance(residual_sum_squares / residual_df, fallback_variance)
    } else {
        positive_variance(fallback_variance, DEFAULT_LOG_VARIANCE)
    };
    if samples.len() < 2 {
        return Shrinkage {
            k: SHRINKAGE_K_MAX,
            within_variance,
        };
    }

    let weights = samples.iter().map(|sample| sample.weight / within_variance).collect::<Vec<_>>();
    let weight_sum = weights.iter().sum::<f64>();
    let fixed_mean = samples.iter().zip(&weights).map(|(sample, weight)| sample.mean() * weight).sum::<f64>() / weight_sum;
    let q = samples
        .iter()
        .zip(&weights)
        .map(|(sample, weight)| weight * (sample.mean() - fixed_mean).powi(2))
        .sum::<f64>();
    let c = weight_sum - weights.iter().map(|weight| weight.powi(2)).sum::<f64>() / weight_sum;
    let tau_squared = if c.is_finite() && c > 0.0 {
        ((q - (samples.len() - 1) as f64) / c).max(0.0)
    } else {
        f64::NAN
    };
    let k = if tau_squared.is_finite() {
        if tau_squared > 0.0 {
            within_variance / tau_squared
        } else {
            SHRINKAGE_K_MAX
        }
    } else {
        fallback_k
    }
    .clamp(SHRINKAGE_K_MIN, SHRINKAGE_K_MAX);
    Shrinkage { k, within_variance }
}

fn transition_kind(row: &TransitionEstimate) -> TransitionKind {
    if row.from == row.to {
        TransitionKind::RepeatedKey
    } else if !row.from_finger.is_empty() && row.from_finger == row.to_finger {
        TransitionKind::SameFinger
    } else if !row.from_hand.is_empty() && !row.to_hand.is_empty() && row.from_hand != row.to_hand {
        TransitionKind::AlternatingHands
    } else {
        TransitionKind::SameHand
    }
}

fn aggregate<'a>(rows: impl Iterator<Item = &'a TransitionStatistic>) -> Moments {
    let mut result = Moments::default();
    for row in rows {
        result.add(row);
    }
    result
}

fn moments(row: &TransitionStatistic) -> Moments {
    Moments {
        weight: row.weight,
        sum: row.log_iki_sum,
        sum_sq: row.log_iki_sq_sum,
        error: row.error_weight,
        attempts: row.attempt_weight,
    }
}

fn hand_key(row: &TransitionStatistic) -> Option<String> {
    (!row.from_hand.is_empty() && !row.to_hand.is_empty()).then(|| format!("{}>{}", row.from_hand, row.to_hand))
}

fn finger_key(row: &TransitionStatistic) -> Option<String> {
    (!row.from_finger.is_empty() && !row.to_finger.is_empty()).then(|| format!("{}>{}", row.from_finger, row.to_finger))
}

fn parent_key(row: &TransitionStatistic) -> ParentKey {
    match (hand_key(row), finger_key(row)) {
        (Some(hand), Some(finger)) => ParentKey::Finger(hand, finger),
        (Some(hand), None) => ParentKey::Hand(hand),
        _ => ParentKey::Root,
    }
}

fn pair_key(from: &str, to: &str) -> Option<String> {
    (!from.is_empty() && !to.is_empty()).then(|| format!("{from}>{to}"))
}

fn finger_pair_key(hand: &str, finger: &str) -> String {
    format!("{hand}|{finger}")
}

fn layout_key_hierarchies(layout: &KeyboardLayout, geometry: &KeyboardGeometry) -> BTreeMap<String, KeyHierarchy> {
    let physical = geometry
        .rows
        .iter()
        .flatten()
        .map(|key| (key.code, Physical { finger: key.finger }))
        .collect::<HashMap<_, _>>();
    let mut result = BTreeMap::new();
    for (code, pair) in layout.keys() {
        let (hand, finger) = hierarchy_names(physical.get(code.as_str()).copied());
        for character in [pair.normal, pair.shift] {
            result.insert(
                character.to_string(),
                KeyHierarchy {
                    hand: hand.to_owned(),
                    finger: finger.to_owned(),
                },
            );
        }
    }
    result
}

fn observed_key_hierarchies(statistics: &SufficientStatistics) -> BTreeMap<String, KeyHierarchy> {
    let mut result = BTreeMap::new();
    for row in &statistics.rows {
        result.entry(row.from.clone()).or_insert_with(|| KeyHierarchy {
            hand: row.from_hand.clone(),
            finger: row.from_finger.clone(),
        });
        result.entry(row.to.clone()).or_insert_with(|| KeyHierarchy {
            hand: row.to_hand.clone(),
            finger: row.to_finger.clone(),
        });
    }
    result
}

fn expected_physical(key: &KeystrokeLog, layout: &KeyboardLayout, physical: &HashMap<&str, Physical>) -> Option<(String, Option<Physical>)> {
    let character = key.expected.as_deref()?.chars().next()?;
    let mapped = layout
        .stroke_for(character)
        .and_then(|stroke| physical.get(stroke.code.as_str()).copied());
    Some((character.to_string(), mapped))
}

fn hierarchy_names(physical: Option<Physical>) -> (&'static str, &'static str) {
    match physical.map(|key| key.finger) {
        Some(Finger::Thumb) | None => ("", ""),
        Some(finger) => (hand_name(finger), finger_name(finger)),
    }
}

fn decay_weight(at: DateTime<Utc>, as_of: DateTime<Utc>, half_life_days: f64) -> f64 {
    if half_life_days <= 0.0 || at >= as_of {
        return 1.0;
    }
    let days = (as_of - at).num_milliseconds() as f64 / 86_400_000.0;
    0.5_f64.powf(days / half_life_days)
}

fn parse_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value).ok().map(|time| time.with_timezone(&Utc))
}

pub fn session_language(record: &SessionRecord) -> &'static str {
    // A benchmark types its part's language whatever the practice language
    // was; benchmarks before MYTASK-3665 recorded the latter in `lang`.
    if let Some((_, part, _)) = record.header.bench_id.as_deref().and_then(crate::bench::parse_bench_id) {
        return if part == crate::bench::BenchPart::C { "ja" } else { "en" };
    }
    if record.header.mode == "romaji"
        || serde_json::from_str::<serde_json::Value>(&record.header.mode_params)
            .ok()
            .and_then(|value| value.get("lang").and_then(serde_json::Value::as_str).map(str::to_owned))
            .as_deref()
            == Some("ja")
        || record.log.items.iter().any(|item| item.reading.is_some())
    {
        "ja"
    } else {
        "en"
    }
}

fn languages(records: &[SessionRecord]) -> String {
    let values = records.iter().map(session_language).collect::<BTreeSet<_>>();
    if values.len() == 1 {
        values.into_iter().next().unwrap_or("all").to_owned()
    } else {
        "all".to_owned()
    }
}

#[derive(Default)]
struct AuxiliarySamples {
    intervals: Vec<(f64, f64)>,
    reactions: Vec<(f64, f64)>,
    word_starts: Vec<(f64, f64)>,
    word_internal: Vec<(f64, f64)>,
    spellings: BTreeMap<String, BTreeMap<String, f64>>,
}

fn auxiliary_samples(records: &[SessionRecord], settings: AnalysisSettings, view: ModelView, as_of: DateTime<Utc>) -> AuxiliarySamples {
    let mut samples = AuxiliarySamples::default();
    let pause_us = settings.pause_ms.saturating_mul(1_000);
    for record in records.iter().filter(|record| record.header.metrics_version > 0) {
        let Some(at) = parse_time(&record.header.finished_at) else { continue };
        let weight = if view == ModelView::Recent {
            decay_weight(at, as_of, settings.half_life_days)
        } else {
            1.0
        };
        let char_keys = record
            .log
            .keystrokes
            .iter()
            .filter(|key| key.kind == KeystrokeKind::Char)
            .collect::<Vec<_>>();
        for item in &record.log.items {
            let item_keys = char_keys.iter().copied().filter(|key| key.item_idx == item.idx).collect::<Vec<_>>();
            if let Some(first) = item_keys.first() {
                samples
                    .reactions
                    .push((first.t_us.saturating_sub(item.shown_us) as f64 / 1_000.0, weight));
            }
            for pair in item_keys.windows(2) {
                let delta_us = pair[1].t_us.saturating_sub(pair[0].t_us);
                if pair[0].correct && pair[1].correct && delta_us > 0 && delta_us <= pause_us {
                    let value = delta_us as f64 / 1_000.0;
                    samples.intervals.push((value, weight));
                    let previous = pair[0].expected.as_deref().and_then(|value| value.chars().next());
                    let current = pair[1].expected.as_deref().and_then(|value| value.chars().next());
                    if previous.is_some_and(char::is_whitespace) && current.is_some_and(|character| !character.is_whitespace()) {
                        samples.word_starts.push((value, weight));
                    } else if previous.is_some_and(|character| !character.is_whitespace())
                        && current.is_some_and(|character| !character.is_whitespace())
                    {
                        samples.word_internal.push((value, weight));
                    }
                }
            }
            if let Some(reading) = item.reading.as_ref()
                && !item_keys.is_empty()
                && item_keys.iter().all(|key| key.correct)
            {
                let spelling = item_keys.iter().filter_map(|key| key.typed.as_deref()).collect::<String>();
                if !spelling.is_empty()
                    && let Some(segments) = RomajiJudge::new(reading).spelling_segments(&spelling)
                {
                    for (mora, variant) in segments {
                        *samples.spellings.entry(mora).or_default().entry(variant).or_default() += weight;
                    }
                }
            }
        }
    }
    samples
}

fn weighted_median(values: &[(f64, f64)]) -> Option<f64> {
    let mut values = values
        .iter()
        .copied()
        .filter(|(value, weight)| value.is_finite() && *weight > 0.0)
        .collect::<Vec<_>>();
    if values.is_empty() {
        return None;
    }
    values.sort_by(|left, right| left.0.total_cmp(&right.0));
    let target = values.iter().map(|(_, weight)| weight).sum::<f64>() / 2.0;
    let mut cumulative = 0.0;
    for (value, weight) in values {
        cumulative += weight;
        if cumulative >= target {
            return Some(value);
        }
    }
    None
}

fn positive_variance(value: f64, fallback: f64) -> f64 {
    if value.is_finite() && value > f64::EPSILON {
        value
    } else {
        fallback.max(f64::EPSILON)
    }
}

fn finger_name(finger: Finger) -> &'static str {
    match finger {
        Finger::LeftPinky => "left-pinky",
        Finger::LeftRing => "left-ring",
        Finger::LeftMiddle => "left-middle",
        Finger::LeftIndex => "left-index",
        Finger::RightIndex => "right-index",
        Finger::RightMiddle => "right-middle",
        Finger::RightRing => "right-ring",
        Finger::RightPinky => "right-pinky",
        Finger::Thumb => "thumb",
    }
}

fn hand_name(finger: Finger) -> &'static str {
    match finger {
        Finger::LeftPinky | Finger::LeftRing | Finger::LeftMiddle | Finger::LeftIndex => "left",
        Finger::RightIndex | Finger::RightMiddle | Finger::RightRing | Finger::RightPinky => "right",
        Finger::Thumb => "thumb",
    }
}
