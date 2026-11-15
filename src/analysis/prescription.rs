//! Adaptive-practice prescriptions, evaluation, difficulty, and goal proposals.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::analysis::model::Prescription;
use crate::engine::session::ItemKind;
use crate::record::SessionRecord;

pub const MIN_PRESCRIPTION_SESSIONS: u32 = 3;
pub const MIN_EVALUATION_N: f64 = 10.0;
pub const PRESCRIPTION_TIMEOUT_DAYS: i64 = 14;
pub const EFFECT_RATIO: f64 = 0.95;
pub const EFFECT_Z: f64 = -1.64;
pub const NO_EFFECT_RATIO: f64 = 1.03;

/// Keeps post-prescription keystrokes that represent transfer outside drills.
///
/// Persisted item kinds are authoritative. For records written before item
/// kinds existed, n-gram sessions are drills, and adaptive items consisting
/// of one two-key transition repeated three times are treated as drills. The
/// latter string check is deliberately limited to old adaptive records.
pub fn real_word_records_since(records: &[SessionRecord], since: DateTime<Utc>) -> Vec<SessionRecord> {
    records
        .iter()
        .filter(|record| DateTime::parse_from_rfc3339(&record.header.started_at).is_ok_and(|started| started.with_timezone(&Utc) >= since))
        .filter_map(|record| {
            let mut record = record.clone();
            let drills = record
                .log
                .items
                .iter()
                .filter(|item| match item.kind {
                    Some(ItemKind::Drill) => true,
                    Some(ItemKind::Word) => false,
                    None if record.header.mode == "ngram" => true,
                    None if record.header.mode == "adaptive" => is_repeated_transition_drill(&item.text),
                    None => false,
                })
                .map(|item| item.idx)
                .collect::<std::collections::HashSet<_>>();
            record.log.items.retain(|item| !drills.contains(&item.idx));
            record.log.keystrokes.retain(|key| !drills.contains(&key.item_idx));
            (!record.log.keystrokes.is_empty()).then_some(record)
        })
        .collect()
}

fn is_repeated_transition_drill(text: &str) -> bool {
    let characters = text.chars().collect::<Vec<_>>();
    characters.len() == 6 && characters[0..2] == characters[2..4] && characters[0..2] == characters[4..6]
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Posterior {
    pub estimated_ms: f64,
    pub log_variance: f64,
    pub effective_n: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrescriptionCase {
    pub started_at: DateTime<Utc>,
    pub sessions: u32,
    pub target_baseline: Posterior,
    pub control_baselines: Vec<Posterior>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceKind {
    RealWords,
    Drills,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub kind: EvidenceKind,
    pub target: Posterior,
    pub controls: Vec<Posterior>,
}

impl Evidence {
    pub fn real_words(target: Posterior, controls: Vec<Posterior>) -> Self {
        Self {
            kind: EvidenceKind::RealWords,
            target,
            controls,
        }
    }

    pub fn drills(target: Posterior, controls: Vec<Posterior>) -> Self {
        Self {
            kind: EvidenceKind::Drills,
            target,
            controls,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrescriptionVerdict {
    Effective,
    Ineffective,
    Pending,
    InsufficientData,
}

impl PrescriptionVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Effective => "effective",
            Self::Ineffective => "ineffective",
            Self::Pending => "pending",
            Self::InsufficientData => "insufficient-data",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrescriptionEvaluation {
    pub target_delta_ms: f64,
    pub control_delta_ms: f64,
    pub effect: f64,
    pub z: f64,
    pub verdict: PrescriptionVerdict,
    pub closed: bool,
}

pub fn evaluate_prescription(case: &PrescriptionCase, evidence: &Evidence, at: DateTime<Utc>) -> PrescriptionEvaluation {
    let timed_out = at - case.started_at >= Duration::days(PRESCRIPTION_TIMEOUT_DAYS);
    let comparable = evidence.kind == EvidenceKind::RealWords
        && case.sessions >= MIN_PRESCRIPTION_SESSIONS
        && evidence.target.effective_n >= MIN_EVALUATION_N
        && !case.control_baselines.is_empty()
        && case.control_baselines.len() == evidence.controls.len()
        && evidence.controls.iter().all(|row| row.effective_n >= MIN_EVALUATION_N);
    if !comparable {
        return PrescriptionEvaluation {
            target_delta_ms: evidence.target.estimated_ms - case.target_baseline.estimated_ms,
            control_delta_ms: mean_delta(&case.control_baselines, &evidence.controls),
            effect: 0.0,
            z: 0.0,
            verdict: PrescriptionVerdict::InsufficientData,
            closed: timed_out,
        };
    }
    let target_change = (evidence.target.estimated_ms / case.target_baseline.estimated_ms).ln();
    let control_changes = case
        .control_baselines
        .iter()
        .zip(&evidence.controls)
        .map(|(baseline, current)| (current.estimated_ms / baseline.estimated_ms).ln())
        .collect::<Vec<_>>();
    let control_change = control_changes.iter().sum::<f64>() / control_changes.len() as f64;
    let effect = target_change - control_change;
    let control_variance = case
        .control_baselines
        .iter()
        .zip(&evidence.controls)
        .map(|(baseline, current)| baseline.log_variance + current.log_variance)
        .sum::<f64>()
        / (control_changes.len() as f64).powi(2);
    let standard_error = (case.target_baseline.log_variance + evidence.target.log_variance + control_variance).sqrt();
    let z = if standard_error > 0.0 { effect / standard_error } else { 0.0 };
    let verdict = if effect <= EFFECT_RATIO.ln() && z <= EFFECT_Z {
        PrescriptionVerdict::Effective
    } else if effect.abs() < NO_EFFECT_RATIO.ln() {
        PrescriptionVerdict::Ineffective
    } else if timed_out {
        PrescriptionVerdict::InsufficientData
    } else {
        PrescriptionVerdict::Pending
    };
    PrescriptionEvaluation {
        target_delta_ms: evidence.target.estimated_ms - case.target_baseline.estimated_ms,
        control_delta_ms: mean_delta(&case.control_baselines, &evidence.controls),
        effect,
        z,
        verdict,
        closed: matches!(verdict, PrescriptionVerdict::Effective | PrescriptionVerdict::Ineffective) || timed_out,
    }
}

/// Evaluates a persisted prescription and returns an updated row ready to be
/// saved. Malformed legacy JSON is reported as `None` instead of inventing a
/// baseline.
pub fn evaluate_stored_prescription(prescription: &Prescription, evidence: &Evidence, at: DateTime<Utc>) -> Option<Prescription> {
    let baseline = serde_json::from_value::<PrescriptionBaseline>(prescription.baseline.clone()).ok()?;
    let sessions = prescription.hypothesis.get("sessions")?.as_u64()?.min(u64::from(u32::MAX)) as u32;
    let evaluation = evaluate_prescription(
        &PrescriptionCase {
            started_at: prescription.created_at,
            sessions,
            target_baseline: baseline.target,
            control_baselines: baseline.controls,
        },
        evidence,
        at,
    );
    let mut updated = prescription.clone();
    updated.evaluated_at = Some(at);
    updated.result = serde_json::to_value(&evaluation).ok();
    if evaluation.closed {
        updated.status = evaluation.verdict.as_str().to_owned();
    }
    Some(updated)
}

fn mean_delta(baselines: &[Posterior], currents: &[Posterior]) -> f64 {
    if baselines.is_empty() || baselines.len() != currents.len() {
        return 0.0;
    }
    baselines
        .iter()
        .zip(currents)
        .map(|(baseline, current)| current.estimated_ms - baseline.estimated_ms)
        .sum::<f64>()
        / baselines.len() as f64
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ControlCandidate {
    pub from: String,
    pub to: String,
    pub kind: String,
    pub frequency_quartile: u8,
    pub estimated_ms: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrescriptionTarget {
    pub from: String,
    pub to: String,
    pub kind: String,
    pub frequency_quartile: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrescriptionBaseline {
    pub target: Posterior,
    pub controls: Vec<Posterior>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrescriptionTrends {
    pub improving: Vec<(String, String)>,
    pub stalled: Vec<(String, String)>,
    pub controls: Vec<(String, String)>,
}

/// Summarizes active prescriptions from the latest 14 days for adaptive
/// scoring. A monotone decrease is improving; three or more prescriptions
/// with less than 3% total change are stalled.
pub fn prescription_trends(rows: &[Prescription], at: DateTime<Utc>) -> PrescriptionTrends {
    let mut result = PrescriptionTrends::default();
    for row in rows
        .iter()
        .filter(|row| row.status == "active" && at - row.created_at <= Duration::days(PRESCRIPTION_TIMEOUT_DAYS))
    {
        let Some(from) = row.target.get("from").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(to) = row.target.get("to").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let estimates = row
            .hypothesis
            .get("estimates_ms")
            .and_then(serde_json::Value::as_array)
            .map(|values| values.iter().filter_map(serde_json::Value::as_f64).collect::<Vec<_>>())
            .unwrap_or_default();
        if estimates.len() >= 2 && estimates.windows(2).all(|pair| pair[1] < pair[0]) {
            result.improving.push((from.to_owned(), to.to_owned()));
        }
        if estimates.len() >= 3 && (estimates[estimates.len() - 1] / estimates[0] - 1.0).abs() < 0.03 {
            result.stalled.push((from.to_owned(), to.to_owned()));
        }
        if let Some(controls) = row.control.as_array() {
            for control in controls {
                if let (Some(from), Some(to)) = (
                    control.get("from").and_then(serde_json::Value::as_str),
                    control.get("to").and_then(serde_json::Value::as_str),
                ) {
                    result.controls.push((from.to_owned(), to.to_owned()));
                }
            }
        }
    }
    result
}

/// Records one completed adaptive session, extending an active prescription
/// for the same transition instead of opening a duplicate.
pub fn record_prescription_session(
    existing: &[Prescription],
    at: DateTime<Utc>,
    layout: &str,
    target: &PrescriptionTarget,
    controls: &[ControlCandidate],
    baseline: &PrescriptionBaseline,
) -> Prescription {
    if let Some(current) = existing.iter().find(|row| {
        row.status == "active"
            && row.layout == layout
            && row.target.get("from").and_then(serde_json::Value::as_str) == Some(target.from.as_str())
            && row.target.get("to").and_then(serde_json::Value::as_str) == Some(target.to.as_str())
    }) {
        let mut continued = current.clone();
        let sessions = continued
            .hypothesis
            .get("sessions")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default()
            + 1;
        continued.hypothesis["sessions"] = serde_json::Value::from(sessions);
        let estimates = continued.hypothesis.get_mut("estimates_ms").and_then(serde_json::Value::as_array_mut);
        if let Some(estimates) = estimates {
            estimates.push(serde_json::Value::from(baseline.target.estimated_ms));
        } else {
            continued.hypothesis["estimates_ms"] = serde_json::json!([
                serde_json::from_value::<PrescriptionBaseline>(continued.baseline.clone())
                    .map_or(baseline.target.estimated_ms, |row| row.target.estimated_ms),
                baseline.target.estimated_ms
            ]);
        }
        return continued;
    }
    Prescription {
        id: None,
        created_at: at,
        layout: layout.to_owned(),
        kind: "adaptive-transition".to_owned(),
        target: serde_json::to_value(target).expect("prescription target is serializable"),
        control: serde_json::to_value(controls).expect("prescription controls are serializable"),
        hypothesis: serde_json::json!({"sessions": 1, "estimates_ms": [baseline.target.estimated_ms]}),
        baseline: serde_json::to_value(baseline).expect("prescription baseline is serializable"),
        status: "active".to_owned(),
        evaluated_at: None,
        result: None,
    }
}

impl ControlCandidate {
    pub fn new(from: &str, to: &str, kind: &str, frequency_quartile: u8, estimated_ms: f64) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            kind: kind.into(),
            frequency_quartile,
            estimated_ms,
        }
    }
}

pub fn select_controls(
    kind: &str,
    frequency_quartile: u8,
    estimated_ms: f64,
    candidates: &[ControlCandidate],
    excluded: &[(String, String)],
) -> Vec<ControlCandidate> {
    let mut rows = candidates
        .iter()
        .filter(|row| row.kind == kind && row.frequency_quartile == frequency_quartile)
        .filter(|row| (row.estimated_ms / estimated_ms - 1.0).abs() <= 0.20)
        .filter(|row| !excluded.contains(&(row.from.clone(), row.to.clone())))
        .cloned()
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        (left.estimated_ms - estimated_ms)
            .abs()
            .total_cmp(&(right.estimated_ms - estimated_ms).abs())
            .then_with(|| (&left.from, &left.to).cmp(&(&right.from, &right.to)))
    });
    rows.truncate(3);
    rows
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DifficultyState {
    pub pacer_multiplier: f64,
    pub drill_ratio: f64,
}

pub fn adjust_difficulty(state: DifficultyState, recent_accuracy: &[f64], enabled: bool) -> DifficultyState {
    if !enabled || recent_accuracy.len() < 3 {
        return state;
    }
    let mut values = recent_accuracy.iter().rev().take(3).copied().collect::<Vec<_>>();
    values.sort_by(f64::total_cmp);
    match values[1] {
        value if value > 0.97 => DifficultyState {
            pacer_multiplier: state.pacer_multiplier * 1.03,
            drill_ratio: (state.drill_ratio + 0.05).min(0.45),
        },
        value if value < 0.92 => DifficultyState {
            pacer_multiplier: state.pacer_multiplier * 0.95,
            drill_ratio: (state.drill_ratio - 0.05).max(0.20),
        },
        _ => state,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GoalSuggestionInput {
    pub recent_wpm: Vec<f64>,
    pub next_grade_wpm: Option<f64>,
    pub weak_accuracy: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GoalSuggestion {
    pub kind: String,
    pub value: f64,
}

pub fn suggest_goals(input: &GoalSuggestionInput) -> Vec<GoalSuggestion> {
    let mut values = input.recent_wpm.iter().rev().take(10).copied().collect::<Vec<_>>();
    values.sort_by(f64::total_cmp);
    if values.is_empty() {
        return Vec::new();
    }
    let position = 0.75 * (values.len() - 1) as f64;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    let fraction = position - lower as f64;
    let p75 = (values[lower] * (1.0 - fraction) + values[upper] * fraction).ceil();
    let mut goals = vec![GoalSuggestion {
        kind: "personal-p75".into(),
        value: p75,
    }];
    if let Some(next) = input.next_grade_wpm.filter(|next| *next <= p75 * 1.10) {
        goals.push(GoalSuggestion {
            kind: "next-grade".into(),
            value: next,
        });
    }
    if input.weak_accuracy.is_some_and(|accuracy| accuracy < 0.97) {
        goals.push(GoalSuggestion {
            kind: "accuracy".into(),
            value: 0.97,
        });
    }
    goals.truncate(3);
    goals
}
