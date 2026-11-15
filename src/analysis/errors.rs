//! Error classification and diagnostics derived from recorded keystrokes.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::analysis::metrics::AnalysisSettings;
use crate::analysis::model::{ModelView, PlayerModel, build_player_model};
use crate::config::ErrorMode;
use crate::engine::session::{KeystrokeKind, KeystrokeLog};
use crate::layout::KeyboardLayout;
use crate::layout::geometry::{Finger, KeyRow, KeyboardGeometry};
use crate::record::SessionRecord;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorKind {
    Anticipation,
    RepeatedPrevious,
    AdjacentKey,
    SameFinger,
    OppositeHand,
    OtherSubstitution,
    Consecutive,
    Insertion,
    Omission,
    Transposition,
}

impl ErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anticipation => "anticipation",
            Self::RepeatedPrevious => "repeated-previous",
            Self::AdjacentKey => "adjacent-key",
            Self::SameFinger => "same-finger",
            Self::OppositeHand => "opposite-hand",
            Self::OtherSubstitution => "other-substitution",
            Self::Consecutive => "consecutive",
            Self::Insertion => "insertion",
            Self::Omission => "omission",
            Self::Transposition => "transposition",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ErrorKindMetric {
    pub kind: ErrorKind,
    pub n: u64,
    pub proportion: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfusionMetric {
    pub expected: String,
    pub typed: String,
    pub n: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PostErrorSlowdown {
    pub post_error_median_ms: f64,
    pub baseline_median_ms: f64,
    pub excess_ms: f64,
    pub post_n: u64,
    pub baseline_n: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PreErrorRhythm {
    pub pre_error_sd_median_ms: f64,
    pub baseline_sd_median_ms: f64,
    pub ratio: f64,
    pub error_windows: u64,
    pub baseline_windows: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CorrectionCost {
    pub kspc: f64,
    pub correction_time_ms: f64,
    pub correction_time_ratio: f64,
    pub residual_error_rate: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AttackErrorRate {
    pub p25_ms: f64,
    pub fast_error_rate: f64,
    pub other_error_rate: f64,
    pub ratio: f64,
    pub fast_attempts: u64,
    pub other_attempts: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ErrorDiagnosis {
    pub kinds: Vec<ErrorKindMetric>,
    pub confusion: Vec<ConfusionMetric>,
    pub post_error_slowdown: Option<PostErrorSlowdown>,
    pub pre_error_rhythm: Option<PreErrorRhythm>,
    pub correction: CorrectionCost,
    pub attack_errors: Option<AttackErrorRate>,
}

impl ErrorDiagnosis {
    pub fn kind_count(&self, kind: ErrorKind) -> u64 {
        self.kinds.iter().find(|row| row.kind == kind).map_or(0, |row| row.n)
    }

    pub fn substitution_count(&self) -> u64 {
        [
            ErrorKind::AdjacentKey,
            ErrorKind::SameFinger,
            ErrorKind::OppositeHand,
            ErrorKind::OtherSubstitution,
        ]
        .into_iter()
        .map(|kind| self.kind_count(kind))
        .sum()
    }
}

#[derive(Clone, Copy)]
struct Physical {
    finger: Finger,
    row: KeyRow,
    row_index: usize,
    key_index: usize,
    center: f64,
    width: f64,
}

#[derive(Clone)]
struct FinalChar {
    character: char,
    code: Option<String>,
}

#[derive(Clone, Copy)]
enum Edit {
    Match,
    Substitute,
    Insert,
    Omit,
    Transpose,
}

pub fn diagnose_errors(
    records: &[SessionRecord],
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
    settings: AnalysisSettings,
    as_of: DateTime<Utc>,
) -> ErrorDiagnosis {
    let physical = physical_map(geometry);
    let mut counts = BTreeMap::<ErrorKind, u64>::new();
    let mut confusion = BTreeMap::<(String, String), u64>::new();
    for record in records
        .iter()
        .filter(|record| record.header.metrics_version > 0 && record.header.layout == layout.name())
    {
        if record.log.error_mode == ErrorMode::Advance || record.header.error_mode == "advance" {
            classify_advance(record, layout, &physical, &mut counts, &mut confusion);
        } else {
            classify_stop(record, layout, &physical, &mut counts, &mut confusion);
        }
    }
    let first_error_total = counts
        .iter()
        .filter(|(kind, _)| **kind != ErrorKind::Consecutive)
        .map(|(_, count)| count)
        .sum::<u64>();
    let model = build_player_model(records, layout, geometry, settings, ModelView::Recent, as_of);
    ErrorDiagnosis {
        kinds: counts
            .into_iter()
            .map(|(kind, n)| ErrorKindMetric {
                kind,
                n,
                proportion: if first_error_total > 0 {
                    n as f64 / first_error_total as f64
                } else {
                    0.0
                },
            })
            .collect(),
        confusion: confusion
            .into_iter()
            .map(|((expected, typed), n)| ConfusionMetric { expected, typed, n })
            .collect(),
        post_error_slowdown: post_error_slowdown(records, &model),
        pre_error_rhythm: pre_error_rhythm(records),
        correction: correction_cost(records),
        attack_errors: attack_error_rate(records),
    }
}

#[derive(Clone, Copy)]
struct Stroke<'a> {
    key: &'a KeystrokeLog,
    expected: Option<char>,
}

fn item_strokes(record: &SessionRecord, item_idx: u32) -> Vec<Stroke<'_>> {
    record
        .log
        .keystrokes
        .iter()
        .filter(|key| key.item_idx == item_idx && key.kind == KeystrokeKind::Char)
        .map(|key| Stroke {
            key,
            expected: one_char(key.expected.as_deref()),
        })
        .collect()
}

fn post_error_slowdown(records: &[SessionRecord], model: &PlayerModel) -> Option<PostErrorSlowdown> {
    let mut post = Vec::new();
    let mut baseline = Vec::new();
    for record in records.iter().filter(|record| record.header.metrics_version > 0) {
        for item in &record.log.items {
            let strokes = item_strokes(record, item.idx);
            let errors = strokes
                .iter()
                .enumerate()
                .filter_map(|(index, stroke)| (!stroke.key.correct).then_some(index))
                .collect::<Vec<_>>();
            for &error_index in &errors {
                let mut previous_expected = strokes[..error_index]
                    .iter()
                    .rev()
                    .find(|stroke| stroke.key.correct)
                    .and_then(|stroke| stroke.expected);
                let mut collected = 0;
                for index in error_index + 1..strokes.len() {
                    if !strokes[index].key.correct {
                        continue;
                    }
                    if let (Some(from), Some(to)) = (previous_expected, strokes[index].expected) {
                        let actual = strokes[index].key.t_us.saturating_sub(strokes[index - 1].key.t_us) as f64 / 1_000.0;
                        post.push(actual - transition_baseline(model, from, to));
                        collected += 1;
                    }
                    previous_expected = strokes[index].expected.or(previous_expected);
                    if collected == 3 {
                        break;
                    }
                }
            }
            for index in 1..strokes.len() {
                if !strokes[index].key.correct || errors.iter().any(|error| error.abs_diff(index) < 10) {
                    continue;
                }
                let previous = strokes[..index].iter().rev().find(|stroke| stroke.key.correct);
                if let (Some(previous), Some(to)) = (previous, strokes[index].expected)
                    && let Some(from) = previous.expected
                {
                    let actual = strokes[index].key.t_us.saturating_sub(strokes[index - 1].key.t_us) as f64 / 1_000.0;
                    baseline.push(actual - transition_baseline(model, from, to));
                }
            }
        }
    }
    let post_median = median(&mut post)?;
    let baseline_median = median(&mut baseline)?;
    Some(PostErrorSlowdown {
        post_error_median_ms: post_median,
        baseline_median_ms: baseline_median,
        excess_ms: post_median - baseline_median,
        post_n: post.len() as u64,
        baseline_n: baseline.len() as u64,
    })
}

fn transition_baseline(model: &PlayerModel, from: char, to: char) -> f64 {
    model
        .transition(&from.to_string(), &to.to_string())
        .map_or(model.personal_iki_median_ms, |row| row.estimated_iki_ms)
}

fn pre_error_rhythm(records: &[SessionRecord]) -> Option<PreErrorRhythm> {
    let mut error_windows = Vec::new();
    let mut baseline_windows = Vec::new();
    for record in records.iter().filter(|record| record.header.metrics_version > 0) {
        for item in &record.log.items {
            let strokes = item_strokes(record, item.idx);
            let errors = strokes
                .iter()
                .enumerate()
                .filter_map(|(index, stroke)| (!stroke.key.correct).then_some(index))
                .collect::<Vec<_>>();
            for &index in &errors {
                if index >= 4 {
                    error_windows.push(interval_sd(&strokes[index - 4..=index]));
                }
            }
            for index in 4..strokes.len() {
                if strokes[index].key.correct && !errors.iter().any(|error| error.abs_diff(index) < 10) {
                    baseline_windows.push(interval_sd(&strokes[index - 4..=index]));
                }
            }
        }
    }
    let error_median = median(&mut error_windows)?;
    let baseline_median = median(&mut baseline_windows)?;
    (baseline_median > 0.0).then_some(PreErrorRhythm {
        pre_error_sd_median_ms: error_median,
        baseline_sd_median_ms: baseline_median,
        ratio: error_median / baseline_median,
        error_windows: error_windows.len() as u64,
        baseline_windows: baseline_windows.len() as u64,
    })
}

fn interval_sd(strokes: &[Stroke<'_>]) -> f64 {
    let intervals = strokes
        .windows(2)
        .map(|pair| pair[1].key.t_us.saturating_sub(pair[0].key.t_us) as f64 / 1_000.0)
        .collect::<Vec<_>>();
    let mean = intervals.iter().sum::<f64>() / intervals.len() as f64;
    (intervals.iter().map(|value| (value - mean).powi(2)).sum::<f64>() / intervals.len() as f64).sqrt()
}

fn correction_cost(records: &[SessionRecord]) -> CorrectionCost {
    let total_keystrokes = records.iter().map(|record| record.log.keystrokes.len() as u64).sum::<u64>();
    let target_units = records
        .iter()
        .map(|record| {
            record
                .log
                .items
                .iter()
                .filter_map(|item| {
                    record
                        .log
                        .keystrokes
                        .iter()
                        .filter(|key| key.item_idx == item.idx && key.kind == KeystrokeKind::Char)
                        .map(|key| key.pos as u64 + 1)
                        .max()
                })
                .sum::<u64>()
        })
        .sum::<u64>();
    let mut correction_us = 0_u64;
    let mut advance_edits = 0_u64;
    let mut advance_expected = 0_u64;
    for record in records {
        let mut pending = HashMap::<(u32, u32), u64>::new();
        for key in &record.log.keystrokes {
            if key.kind != KeystrokeKind::Char {
                continue;
            }
            let position = (key.item_idx, key.pos);
            if key.correct {
                if let Some(start) = pending.remove(&position) {
                    correction_us += key.t_us.saturating_sub(start);
                }
            } else {
                pending.entry(position).or_insert(key.t_us);
            }
        }
        if record.log.error_mode == ErrorMode::Advance || record.header.error_mode == "advance" {
            for item in &record.log.items {
                if item.reading.is_none() {
                    let expected = item.text.chars().collect::<Vec<_>>();
                    let typed = final_input(record, item.idx);
                    advance_expected += expected.len() as u64;
                    advance_edits += align(&expected, &typed)
                        .iter()
                        .filter(|(edit, _, _)| !matches!(edit, Edit::Match))
                        .count() as u64;
                }
            }
        }
    }
    let total_duration = records.iter().map(|record| record.log.duration_us).sum::<u64>();
    CorrectionCost {
        kspc: if target_units > 0 {
            total_keystrokes as f64 / target_units as f64
        } else {
            0.0
        },
        correction_time_ms: correction_us as f64 / 1_000.0,
        correction_time_ratio: if total_duration > 0 {
            correction_us as f64 / total_duration as f64
        } else {
            0.0
        },
        residual_error_rate: (advance_expected > 0).then(|| advance_edits as f64 / advance_expected as f64),
    }
}

fn attack_error_rate(records: &[SessionRecord]) -> Option<AttackErrorRate> {
    let mut reference = Vec::new();
    let mut attempts = Vec::new();
    for record in records.iter().filter(|record| record.header.metrics_version > 0) {
        for item in &record.log.items {
            let strokes = item_strokes(record, item.idx);
            for pair in strokes.windows(2) {
                let interval = pair[1].key.t_us.saturating_sub(pair[0].key.t_us) as f64 / 1_000.0;
                if pair[0].key.correct && pair[1].key.correct {
                    reference.push(interval);
                }
                attempts.push((interval, !pair[1].key.correct));
            }
        }
    }
    let p25 = percentile(&mut reference, 0.25)?;
    let (mut fast_attempts, mut fast_errors, mut other_attempts, mut other_errors) = (0_u64, 0_u64, 0_u64, 0_u64);
    for (interval, error) in attempts {
        if interval < p25 {
            fast_attempts += 1;
            fast_errors += u64::from(error);
        } else {
            other_attempts += 1;
            other_errors += u64::from(error);
        }
    }
    if fast_attempts == 0 || other_attempts == 0 {
        return None;
    }
    let fast_error_rate = fast_errors as f64 / fast_attempts as f64;
    let other_error_rate = other_errors as f64 / other_attempts as f64;
    Some(AttackErrorRate {
        p25_ms: p25,
        fast_error_rate,
        other_error_rate,
        ratio: if other_error_rate > 0.0 {
            fast_error_rate / other_error_rate
        } else if fast_error_rate > 0.0 {
            f64::INFINITY
        } else {
            0.0
        },
        fast_attempts,
        other_attempts,
    })
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

fn classify_stop(
    record: &SessionRecord,
    layout: &KeyboardLayout,
    physical: &HashMap<&str, Physical>,
    counts: &mut BTreeMap<ErrorKind, u64>,
    confusion: &mut BTreeMap<(String, String), u64>,
) {
    let mut errors_at_position = HashMap::<(u32, u32), u64>::new();
    for item in &record.log.items {
        let target = expected_characters(record, item.idx);
        for key in record
            .log
            .keystrokes
            .iter()
            .filter(|key| key.item_idx == item.idx && key.kind == KeystrokeKind::Char && !key.correct)
        {
            let Some(typed) = one_char(key.typed.as_deref()) else { continue };
            let Some(expected) = one_char(key.expected.as_deref()).or_else(|| target.get(key.pos as usize).copied()) else {
                continue;
            };
            *confusion.entry((expected.to_string(), typed.to_string())).or_default() += 1;
            let repeated = errors_at_position.entry((item.idx, key.pos)).or_default();
            let kind = if *repeated > 0 {
                ErrorKind::Consecutive
            } else if target.get(key.pos as usize + 1) == Some(&typed) {
                ErrorKind::Anticipation
            } else if key.pos > 0 && target.get(key.pos as usize - 1) == Some(&typed) {
                ErrorKind::RepeatedPrevious
            } else {
                classify_substitution(expected, typed, key.code.as_deref(), layout, physical)
            };
            *repeated += 1;
            *counts.entry(kind).or_default() += 1;
        }
    }
}

fn classify_advance(
    record: &SessionRecord,
    layout: &KeyboardLayout,
    physical: &HashMap<&str, Physical>,
    counts: &mut BTreeMap<ErrorKind, u64>,
    confusion: &mut BTreeMap<(String, String), u64>,
) {
    for item in &record.log.items {
        if item.reading.is_some() {
            continue;
        }
        let expected = item.text.chars().collect::<Vec<_>>();
        let typed = final_input(record, item.idx);
        for (edit, expected_index, typed_index) in align(&expected, &typed) {
            match edit {
                Edit::Match => {}
                Edit::Substitute => {
                    let expected_char = expected[expected_index];
                    let typed_char = typed[typed_index].character;
                    let kind = classify_substitution(expected_char, typed_char, typed[typed_index].code.as_deref(), layout, physical);
                    *counts.entry(kind).or_default() += 1;
                    *confusion.entry((expected_char.to_string(), typed_char.to_string())).or_default() += 1;
                }
                Edit::Insert => {
                    *counts.entry(ErrorKind::Insertion).or_default() += 1;
                    *confusion.entry((String::new(), typed[typed_index].character.to_string())).or_default() += 1;
                }
                Edit::Omit => {
                    *counts.entry(ErrorKind::Omission).or_default() += 1;
                    *confusion.entry((expected[expected_index].to_string(), String::new())).or_default() += 1;
                }
                Edit::Transpose => {
                    *counts.entry(ErrorKind::Transposition).or_default() += 1;
                    for offset in 0..2 {
                        *confusion
                            .entry((
                                expected[expected_index + offset].to_string(),
                                typed[typed_index + 1 - offset].character.to_string(),
                            ))
                            .or_default() += 1;
                    }
                }
            }
        }
    }
}

fn expected_characters(record: &SessionRecord, item_idx: u32) -> Vec<char> {
    if let Some(item) = record.log.items.iter().find(|item| item.idx == item_idx)
        && item.reading.is_none()
    {
        return item.text.chars().collect();
    }
    let mut values = BTreeMap::<u32, char>::new();
    for key in record.log.keystrokes.iter().filter(|key| key.item_idx == item_idx) {
        if let Some(character) = one_char(key.expected.as_deref()) {
            values.entry(key.pos).or_insert(character);
        }
    }
    values.into_values().collect()
}

fn final_input(record: &SessionRecord, item_idx: u32) -> Vec<FinalChar> {
    let mut slots = Vec::new();
    for key in record.log.keystrokes.iter().filter(|key| key.item_idx == item_idx) {
        match key.kind {
            KeystrokeKind::Char => {
                let Some(character) = one_char(key.typed.as_deref()) else { continue };
                let position = key.pos as usize;
                if position < slots.len() {
                    slots.truncate(position);
                }
                slots.push(FinalChar {
                    character,
                    code: key.code.clone(),
                });
            }
            KeystrokeKind::Backspace => {
                slots.pop();
            }
        }
    }
    slots
}

fn align(expected: &[char], typed: &[FinalChar]) -> Vec<(Edit, usize, usize)> {
    let rows = expected.len() + 1;
    let columns = typed.len() + 1;
    let mut costs = vec![vec![0_usize; columns]; rows];
    let mut edits = vec![vec![Edit::Match; columns]; rows];
    for index in 1..rows {
        costs[index][0] = index;
        edits[index][0] = Edit::Omit;
    }
    for index in 1..columns {
        costs[0][index] = index;
        edits[0][index] = Edit::Insert;
    }
    for i in 1..rows {
        for j in 1..columns {
            if expected[i - 1] == typed[j - 1].character {
                costs[i][j] = costs[i - 1][j - 1];
                edits[i][j] = Edit::Match;
                continue;
            }
            let mut best = (costs[i - 1][j - 1] + 1, Edit::Substitute);
            if i >= 2
                && j >= 2
                && expected[i - 1] == typed[j - 2].character
                && expected[i - 2] == typed[j - 1].character
                && costs[i - 2][j - 2] + 1 < best.0
            {
                best = (costs[i - 2][j - 2] + 1, Edit::Transpose);
            }
            if costs[i][j - 1] + 1 < best.0 {
                best = (costs[i][j - 1] + 1, Edit::Insert);
            }
            if costs[i - 1][j] + 1 < best.0 {
                best = (costs[i - 1][j] + 1, Edit::Omit);
            }
            costs[i][j] = best.0;
            edits[i][j] = best.1;
        }
    }
    let mut output = Vec::new();
    let (mut i, mut j) = (expected.len(), typed.len());
    while i > 0 || j > 0 {
        let edit = edits[i][j];
        match edit {
            Edit::Match | Edit::Substitute => {
                output.push((edit, i - 1, j - 1));
                i -= 1;
                j -= 1;
            }
            Edit::Insert => {
                output.push((edit, i, j - 1));
                j -= 1;
            }
            Edit::Omit => {
                output.push((edit, i - 1, j));
                i -= 1;
            }
            Edit::Transpose => {
                output.push((edit, i - 2, j - 2));
                i -= 2;
                j -= 2;
            }
        }
    }
    output.reverse();
    output
}

fn classify_substitution(
    expected: char,
    typed: char,
    typed_code: Option<&str>,
    layout: &KeyboardLayout,
    physical: &HashMap<&str, Physical>,
) -> ErrorKind {
    let expected_code = layout.stroke_for(expected).map(|stroke| stroke.code);
    let typed_layout_code = layout.stroke_for(typed).map(|stroke| stroke.code);
    let actual_typed_code = typed_code.or(typed_layout_code.as_deref());
    let expected_physical = expected_code.as_deref().and_then(|code| physical.get(code));
    let typed_physical = actual_typed_code.and_then(|code| physical.get(code));
    if expected_physical.zip(typed_physical).is_some_and(|(left, right)| adjacent(*left, *right)) {
        return ErrorKind::AdjacentKey;
    }
    let standard_typed = typed_layout_code.as_deref().and_then(|code| physical.get(code));
    if expected_physical
        .zip(standard_typed)
        .is_some_and(|(left, right)| left.finger == right.finger)
    {
        return ErrorKind::SameFinger;
    }
    if expected_physical
        .zip(standard_typed)
        .is_some_and(|(left, right)| left.row == right.row && mirrored_finger(left.finger, right.finger))
    {
        return ErrorKind::OppositeHand;
    }
    ErrorKind::OtherSubstitution
}

fn physical_map(geometry: &KeyboardGeometry) -> HashMap<&str, Physical> {
    let mut output = HashMap::new();
    for (row_index, row) in geometry.rows.iter().enumerate() {
        let mut x = 0.0;
        for (key_index, key) in row.iter().enumerate() {
            let width = f64::from(key.width);
            output.insert(
                key.code,
                Physical {
                    finger: key.finger,
                    row: key.row,
                    row_index,
                    key_index,
                    center: x + width / 2.0,
                    width,
                },
            );
            x += width;
        }
    }
    output
}

fn adjacent(left: Physical, right: Physical) -> bool {
    if left.row_index == right.row_index {
        return left.key_index.abs_diff(right.key_index) == 1;
    }
    left.row_index.abs_diff(right.row_index) == 1 && (left.center - right.center).abs() <= (left.width + right.width) / 2.0 + 1.0
}

fn mirrored_finger(left: Finger, right: Finger) -> bool {
    matches!(
        (left, right),
        (Finger::LeftPinky, Finger::RightPinky)
            | (Finger::RightPinky, Finger::LeftPinky)
            | (Finger::LeftRing, Finger::RightRing)
            | (Finger::RightRing, Finger::LeftRing)
            | (Finger::LeftMiddle, Finger::RightMiddle)
            | (Finger::RightMiddle, Finger::LeftMiddle)
            | (Finger::LeftIndex, Finger::RightIndex)
            | (Finger::RightIndex, Finger::LeftIndex)
    )
}

fn one_char(value: Option<&str>) -> Option<char> {
    let mut characters = value?.chars();
    let character = characters.next()?;
    characters.next().is_none().then_some(character)
}
