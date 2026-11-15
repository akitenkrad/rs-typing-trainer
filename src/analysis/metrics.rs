use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::analysis::model::{ModelView, build_player_model};
use crate::engine::session::{KeystrokeKind, KeystrokeLog};
use crate::layout::KeyboardLayout;
use crate::layout::geometry::{Finger, KeyRow, KeyboardGeometry};
use crate::record::SessionRecord;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnalysisSettings {
    pub pause_ms: u64,
    pub shrinkage_k: f64,
    pub half_life_days: f64,
}

impl Default for AnalysisSettings {
    fn default() -> Self {
        Self {
            pause_ms: 2_000,
            shrinkage_k: 10.0,
            half_life_days: 14.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransitionClass {
    SameFinger,
    SameHandAdjacent,
    SameHandNonAdjacent,
    AlternatingHands,
}

impl TransitionClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SameFinger => "same-finger",
            Self::SameHandAdjacent => "same-hand-adjacent",
            Self::SameHandNonAdjacent => "same-hand-non-adjacent",
            Self::AlternatingHands => "alternating-hands",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeyMetric {
    pub key: String,
    pub code: String,
    pub correct: u64,
    pub total: u64,
    pub accuracy: f64,
    pub iki_n: u64,
    pub iki_median_ms: f64,
    pub iki_p90_ms: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BigramMetric {
    pub from: String,
    pub to: String,
    pub class: TransitionClass,
    pub n: u64,
    pub median_ms: f64,
    pub error_n: u64,
    pub attempts: u64,
    pub error_rate: f64,
    pub estimated_iki_ms: f64,
    pub estimated_error_rate: f64,
    pub estimated_iki_lo_ms: f64,
    pub estimated_iki_hi_ms: f64,
    pub relative_iki: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupMetric {
    pub name: String,
    pub correct: u64,
    pub total: u64,
    pub accuracy: f64,
    pub n: u64,
    pub iki_median_ms: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpellingChoiceMetric {
    pub kana: String,
    pub variant: String,
    pub count: u64,
    pub ratio: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DetailedMetrics {
    pub keys: Vec<KeyMetric>,
    pub bigrams: Vec<BigramMetric>,
    pub fingers: Vec<GroupMetric>,
    pub hands: Vec<GroupMetric>,
    pub rows: Vec<GroupMetric>,
    pub iki_median_ms: f64,
    pub iki_cv: f64,
    pub reaction_median_ms: f64,
    pub correction_median_ms: f64,
    pub recovery_keys_median: Option<f64>,
    pub post_error_three_error_rate: Option<f64>,
    pub same_finger_median_ms: f64,
    pub alternating_hands_median_ms: f64,
    pub hold_median_ms: Option<f64>,
    pub rollover_rate: Option<f64>,
    pub spelling_choices: Vec<SpellingChoiceMetric>,
}

#[derive(Default)]
struct Accumulator {
    correct: u64,
    total: u64,
    intervals: Vec<f64>,
}

#[derive(Default)]
struct BigramAccumulator {
    intervals: Vec<f64>,
    error_n: u64,
    attempts: u64,
}

struct TransitionAccumulators<'a> {
    bigrams: &'a mut BTreeMap<(String, String, TransitionClass), BigramAccumulator>,
    same_finger: &'a mut Vec<f64>,
    alternating_hands: &'a mut Vec<f64>,
}

#[derive(Clone, Copy)]
struct KeyPhysical {
    finger: Finger,
    row: KeyRow,
}

pub fn shrink_mean(n: u64, sample_mean: f64, class_mean: f64, k: f64) -> f64 {
    let n = n as f64;
    let k = k.max(0.0);
    if n + k == 0.0 {
        return class_mean;
    }
    (n * sample_mean + k * class_mean) / (n + k)
}

pub fn shrink_rate(attempts: u64, errors: u64, class_rate: f64, k: f64) -> f64 {
    let attempts = attempts as f64;
    let k = k.max(0.0);
    if attempts + k == 0.0 {
        return class_rate;
    }
    (errors as f64 + k * class_rate) / (attempts + k)
}

pub fn analyze_records(
    records: &[SessionRecord],
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
    settings: AnalysisSettings,
) -> DetailedMetrics {
    analyze_records_view(records, layout, geometry, settings, ModelView::Recent)
}

pub fn analyze_records_view(
    records: &[SessionRecord],
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
    settings: AnalysisSettings,
    view: ModelView,
) -> DetailedMetrics {
    let physical = geometry
        .rows
        .iter()
        .flatten()
        .map(|key| {
            (
                key.code,
                KeyPhysical {
                    finger: key.finger,
                    row: key.row,
                },
            )
        })
        .collect::<HashMap<_, _>>();
    let pause_us = settings.pause_ms.saturating_mul(1_000);
    let mut keys = BTreeMap::<String, Accumulator>::new();
    let mut bigrams = BTreeMap::<(String, String, TransitionClass), BigramAccumulator>::new();
    let mut fingers = BTreeMap::<String, Accumulator>::new();
    let mut hands = BTreeMap::<String, Accumulator>::new();
    let mut rows = BTreeMap::<String, Accumulator>::new();
    let mut all_intervals = Vec::new();
    let mut reactions = Vec::new();
    let mut corrections = Vec::new();
    let mut same_finger = Vec::new();
    let mut alternating_hands = Vec::new();
    let mut holds = Vec::new();
    let mut overlap_count = 0u64;
    let mut overlap_pairs = 0u64;
    let mut spelling_choices = BTreeMap::<String, BTreeMap<String, u64>>::new();

    for record in records.iter().filter(|record| record.header.metrics_version > 0) {
        let char_keys = record
            .log
            .keystrokes
            .iter()
            .filter(|key| key.kind == KeystrokeKind::Char)
            .collect::<Vec<_>>();
        for item in &record.log.items {
            let item_keys = char_keys.iter().copied().filter(|key| key.item_idx == item.idx).collect::<Vec<_>>();
            if let Some(first) = item_keys.first() {
                reactions.push(first.t_us.saturating_sub(item.shown_us) as f64 / 1_000.0);
            }
            collect_spelling_choices(item.reading.as_deref(), &item_keys, &mut spelling_choices);
            for pair in item_keys.windows(2) {
                let delta_us = pair[1].t_us.saturating_sub(pair[0].t_us);
                if delta_us <= pause_us {
                    let delta_ms = delta_us as f64 / 1_000.0;
                    all_intervals.push(delta_ms);
                    if let Some((_, current)) = expected_physical(pair[1], layout, &physical) {
                        add_interval_to_groups(current, delta_ms, &mut fingers, &mut hands, &mut rows);
                    }
                }
                collect_bigram(
                    pair[0],
                    pair[1],
                    delta_us,
                    pause_us,
                    layout,
                    &physical,
                    &mut TransitionAccumulators {
                        bigrams: &mut bigrams,
                        same_finger: &mut same_finger,
                        alternating_hands: &mut alternating_hands,
                    },
                );
            }
        }

        collect_corrections(record, &mut corrections);
        for key in &char_keys {
            let Some(expected) = key.expected.as_deref() else { continue };
            let Some(character) = expected.chars().next() else { continue };
            let Some(stroke) = layout.stroke_for(character) else { continue };
            let entry = keys.entry(character.to_string()).or_default();
            entry.total += 1;
            entry.correct += u64::from(key.correct);
            if let Some(key_physical) = physical.get(stroke.code.as_str()).copied() {
                add_accuracy_to_groups(key_physical, key.correct, &mut fingers, &mut hands, &mut rows);
            }
            if record.header.release_events
                && let Some(release_us) = key.release_us
            {
                holds.push(release_us.saturating_sub(key.t_us) as f64 / 1_000.0);
            }
        }
        if record.header.release_events {
            for pair in char_keys.windows(2) {
                if let Some(release_us) = pair[0].release_us {
                    overlap_pairs += 1;
                    overlap_count += u64::from(pair[1].t_us < release_us);
                }
            }
        }
        for item in &record.log.items {
            let item_keys = char_keys.iter().copied().filter(|key| key.item_idx == item.idx).collect::<Vec<_>>();
            for pair in item_keys.windows(2) {
                let delta_us = pair[1].t_us.saturating_sub(pair[0].t_us);
                if delta_us > pause_us {
                    continue;
                }
                if let Some(expected) = pair[1].expected.as_deref().and_then(|value| value.chars().next()) {
                    keys.entry(expected.to_string()).or_default().intervals.push(delta_us as f64 / 1_000.0);
                }
            }
        }
    }

    let as_of = records
        .iter()
        .filter_map(|record| chrono::DateTime::parse_from_rfc3339(&record.header.finished_at).ok())
        .map(|time| time.with_timezone(&chrono::Utc))
        .max()
        .unwrap_or(chrono::DateTime::UNIX_EPOCH);
    let player_model = build_player_model(records, layout, geometry, settings, view, as_of);
    let recovery_rows = records
        .iter()
        .filter(|record| record.header.metrics_version > 0)
        .filter_map(|record| {
            let has_error = record.log.keystrokes.iter().any(|key| key.kind == KeystrokeKind::Char && !key.correct);
            has_error.then(|| crate::practice::recovery::recovery_metrics(&record.log))
        })
        .collect::<Vec<_>>();
    let recovery_keys = recovery_rows.iter().filter_map(|row| row.median_keys_to_recover).collect::<Vec<_>>();
    let post_error_rate =
        (!recovery_rows.is_empty()).then(|| recovery_rows.iter().map(|row| row.next_three_error_rate).sum::<f64>() / recovery_rows.len() as f64);
    DetailedMetrics {
        keys: keys
            .into_iter()
            .map(|(key, acc)| {
                let code = key
                    .chars()
                    .next()
                    .and_then(|character| layout.stroke_for(character))
                    .map(|stroke| stroke.code)
                    .unwrap_or_default();
                KeyMetric {
                    key,
                    code,
                    correct: acc.correct,
                    total: acc.total,
                    accuracy: ratio(acc.correct, acc.total, 1.0),
                    iki_n: acc.intervals.len() as u64,
                    iki_median_ms: median(&acc.intervals),
                    iki_p90_ms: percentile(&acc.intervals, 0.9),
                }
            })
            .collect(),
        bigrams: bigrams
            .into_iter()
            .filter(|(_, acc)| !acc.intervals.is_empty())
            .map(|((from, to, class), acc)| {
                let sample_mean = mean(&acc.intervals);
                let estimate = player_model.transition(&from, &to);
                let estimated_iki_ms = estimate.map_or(sample_mean, |row| row.estimated_iki_ms);
                BigramMetric {
                    from,
                    to,
                    class,
                    n: acc.intervals.len() as u64,
                    median_ms: median(&acc.intervals),
                    error_n: acc.error_n,
                    attempts: acc.attempts,
                    error_rate: ratio(acc.error_n, acc.attempts, 0.0),
                    estimated_iki_ms,
                    estimated_error_rate: estimate.map_or_else(|| ratio(acc.error_n, acc.attempts, 0.0), |row| row.estimated_error_rate),
                    estimated_iki_lo_ms: estimate.map_or(estimated_iki_ms, |row| row.estimated_iki_lo_ms),
                    estimated_iki_hi_ms: estimate.map_or(estimated_iki_ms, |row| row.estimated_iki_hi_ms),
                    relative_iki: estimate.map_or_else(
                        || {
                            if player_model.personal_iki_median_ms > 0.0 {
                                estimated_iki_ms / player_model.personal_iki_median_ms
                            } else {
                                0.0
                            }
                        },
                        |row| row.relative_iki,
                    ),
                }
            })
            .collect(),
        fingers: group_rows(fingers),
        hands: group_rows(hands),
        rows: group_rows(rows),
        iki_median_ms: median(&all_intervals),
        iki_cv: coefficient_of_variation(&all_intervals),
        reaction_median_ms: median(&reactions),
        correction_median_ms: median(&corrections),
        recovery_keys_median: (!recovery_keys.is_empty()).then(|| median(&recovery_keys)),
        post_error_three_error_rate: post_error_rate,
        same_finger_median_ms: median(&same_finger),
        alternating_hands_median_ms: median(&alternating_hands),
        hold_median_ms: (!holds.is_empty()).then(|| median(&holds)),
        rollover_rate: (overlap_pairs > 0).then(|| overlap_count as f64 / overlap_pairs as f64),
        spelling_choices: spelling_choice_rows(spelling_choices),
    }
}

fn spelling_choice_rows(counts: BTreeMap<String, BTreeMap<String, u64>>) -> Vec<SpellingChoiceMetric> {
    counts
        .into_iter()
        .flat_map(|(kana, variants)| {
            let total = variants.values().sum::<u64>();
            variants.into_iter().map(move |(variant, count)| SpellingChoiceMetric {
                kana: kana.clone(),
                variant,
                count,
                ratio: ratio(count, total, 0.0),
            })
        })
        .collect()
}

fn collect_bigram(
    previous: &KeystrokeLog,
    current: &KeystrokeLog,
    delta_us: u64,
    pause_us: u64,
    layout: &KeyboardLayout,
    physical: &HashMap<&str, KeyPhysical>,
    accumulators: &mut TransitionAccumulators<'_>,
) {
    if !previous.correct || delta_us > pause_us {
        return;
    }
    let Some((from_char, from)) = expected_physical(previous, layout, physical) else {
        return;
    };
    let Some((to_char, to)) = expected_physical(current, layout, physical) else {
        return;
    };
    let class = transition_class(from.finger, to.finger);
    let entry = accumulators.bigrams.entry((from_char.clone(), to_char.clone(), class)).or_default();
    entry.attempts += 1;
    if current.correct {
        let delta_ms = delta_us as f64 / 1_000.0;
        entry.intervals.push(delta_ms);
        if class == TransitionClass::SameFinger && from_char != to_char {
            accumulators.same_finger.push(delta_ms);
        }
        if class == TransitionClass::AlternatingHands {
            accumulators.alternating_hands.push(delta_ms);
        }
    } else {
        entry.error_n += 1;
    }
}

fn expected_physical(key: &KeystrokeLog, layout: &KeyboardLayout, physical: &HashMap<&str, KeyPhysical>) -> Option<(String, KeyPhysical)> {
    let character = key.expected.as_deref()?.chars().next()?;
    let stroke = layout.stroke_for(character)?;
    Some((character.to_string(), *physical.get(stroke.code.as_str())?))
}

fn collect_corrections(record: &SessionRecord, corrections: &mut Vec<f64>) {
    let mut pending: Option<(u32, u64)> = None;
    for key in &record.log.keystrokes {
        match record.log.error_mode {
            crate::config::ErrorMode::Stop => {
                if key.kind == KeystrokeKind::Char && !key.correct {
                    pending = Some((key.item_idx, key.t_us));
                } else if key.kind == KeystrokeKind::Char
                    && key.correct
                    && let Some((item, at)) = pending.take()
                    && item == key.item_idx
                {
                    corrections.push(key.t_us.saturating_sub(at) as f64 / 1_000.0);
                }
            }
            crate::config::ErrorMode::Advance => {
                if key.kind == KeystrokeKind::Char && !key.correct {
                    pending = Some((key.item_idx, key.t_us));
                } else if key.kind == KeystrokeKind::Backspace
                    && let Some((item, at)) = pending.take()
                    && item == key.item_idx
                {
                    corrections.push(key.t_us.saturating_sub(at) as f64 / 1_000.0);
                }
            }
        }
    }
}

fn collect_spelling_choices(reading: Option<&str>, keys: &[&KeystrokeLog], choices: &mut BTreeMap<String, BTreeMap<String, u64>>) {
    let Some(reading) = reading else { return };
    let typed = keys
        .iter()
        .filter(|key| key.correct)
        .filter_map(|key| key.typed.as_deref())
        .collect::<String>();
    for (kana, variants) in [("し", ["shi", "si"]), ("ち", ["chi", "ti"]), ("つ", ["tsu", "tu"])] {
        if !reading.contains(kana) {
            continue;
        }
        for variant in variants {
            let count = typed.matches(variant).count() as u64;
            if count > 0 {
                *choices.entry(kana.to_owned()).or_default().entry(variant.to_owned()).or_default() += count;
            }
        }
    }
}

fn add_accuracy_to_groups(
    physical: KeyPhysical,
    correct: bool,
    fingers: &mut BTreeMap<String, Accumulator>,
    hands: &mut BTreeMap<String, Accumulator>,
    rows: &mut BTreeMap<String, Accumulator>,
) {
    for (map, name) in [
        (fingers, finger_name(physical.finger)),
        (hands, hand_name(physical.finger)),
        (rows, row_name(physical.row)),
    ] {
        let entry = map.entry(name.to_owned()).or_default();
        entry.total += 1;
        entry.correct += u64::from(correct);
    }
}

fn add_interval_to_groups(
    physical: KeyPhysical,
    interval: f64,
    fingers: &mut BTreeMap<String, Accumulator>,
    hands: &mut BTreeMap<String, Accumulator>,
    rows: &mut BTreeMap<String, Accumulator>,
) {
    for (map, name) in [
        (fingers, finger_name(physical.finger)),
        (hands, hand_name(physical.finger)),
        (rows, row_name(physical.row)),
    ] {
        map.entry(name.to_owned()).or_default().intervals.push(interval);
    }
}

fn group_rows(groups: BTreeMap<String, Accumulator>) -> Vec<GroupMetric> {
    groups
        .into_iter()
        .map(|(name, acc)| GroupMetric {
            name,
            correct: acc.correct,
            total: acc.total,
            accuracy: ratio(acc.correct, acc.total, 1.0),
            n: acc.intervals.len() as u64,
            iki_median_ms: median(&acc.intervals),
        })
        .collect()
}

fn transition_class(from: Finger, to: Finger) -> TransitionClass {
    if from == to {
        return TransitionClass::SameFinger;
    }
    if hand_name(from) != hand_name(to) {
        return TransitionClass::AlternatingHands;
    }
    if finger_index(from).abs_diff(finger_index(to)) == 1 {
        TransitionClass::SameHandAdjacent
    } else {
        TransitionClass::SameHandNonAdjacent
    }
}

fn finger_index(finger: Finger) -> u8 {
    match finger {
        Finger::LeftPinky | Finger::RightPinky => 0,
        Finger::LeftRing | Finger::RightRing => 1,
        Finger::LeftMiddle | Finger::RightMiddle => 2,
        Finger::LeftIndex | Finger::RightIndex => 3,
        Finger::Thumb => 4,
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

fn row_name(row: KeyRow) -> &'static str {
    match row {
        KeyRow::Number => "number",
        KeyRow::Top => "top",
        KeyRow::Home => "home",
        KeyRow::Bottom => "bottom",
        KeyRow::Control => "control",
    }
}

fn ratio(numerator: u64, denominator: u64, empty: f64) -> f64 {
    if denominator == 0 {
        empty
    } else {
        numerator as f64 / denominator as f64
    }
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn median(values: &[f64]) -> f64 {
    percentile(values, 0.5)
}

fn percentile(values: &[f64], quantile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let index = ((sorted.len() - 1) as f64 * quantile).round() as usize;
    sorted[index]
}

fn coefficient_of_variation(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let mean = mean(values);
    if mean <= 0.0 {
        return 0.0;
    }
    let variance = values.iter().map(|value| (value - mean).powi(2)).sum::<f64>() / values.len() as f64;
    variance.sqrt() / mean
}
