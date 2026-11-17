//! Analysis derived only from the sessions in one completed benchmark run.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};

use crate::analysis::metrics::{AnalysisSettings, analyze_records};
use crate::analysis::model::Prescription;
use crate::bench::{BenchPart, is_preferred_bench_series, parse_bench_id, preferred_bench_series};
use crate::engine::session::KeystrokeKind;
use crate::layout::KeyboardLayout;
use crate::layout::geometry::KeyboardGeometry;
use crate::record::SessionRecord;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchMetric {
    Wpm,
    Accuracy,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BenchChange {
    pub part: BenchPart,
    pub metric: BenchMetric,
    /// WPM for speed and percentage points for accuracy.
    pub delta: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SlowTransition {
    pub from: String,
    pub to: String,
    pub milliseconds: f64,
    pub n: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeakFingerReason {
    ErrorRate,
    Iki,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WeakFinger {
    pub name: String,
    pub reason: WeakFingerReason,
    pub error_rate: f64,
    pub iki_median_ms: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PracticeSuggestion {
    Transition { from: String, to: String },
    Finger { name: String },
    Keys { keys: Vec<String> },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BenchmarkRunAnalysis {
    pub slow_transitions: Vec<SlowTransition>,
    pub transition_counts_insufficient: bool,
    pub weak_fingers: Vec<WeakFinger>,
    pub changes: Vec<BenchChange>,
    pub practice: Vec<PracticeSuggestion>,
    pub mistyped_keys: BTreeMap<String, u32>,
}

#[allow(clippy::too_many_arguments)]
pub fn analyze_benchmark_run(
    current: &[SessionRecord],
    history: &[SessionRecord],
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
    settings: AnalysisSettings,
    min_transition_n: u64,
    prescriptions: &[Prescription],
) -> BenchmarkRunAnalysis {
    let details = analyze_records(current, layout, geometry, settings);
    let mut slow_transitions = details
        .bigrams
        .iter()
        .filter(|row| row.n >= min_transition_n)
        .map(|row| SlowTransition {
            from: row.from.clone(),
            to: row.to.clone(),
            milliseconds: row.median_ms,
            n: row.n,
        })
        .collect::<Vec<_>>();
    slow_transitions.sort_by(|left, right| {
        right
            .milliseconds
            .total_cmp(&left.milliseconds)
            .then_with(|| left.from.cmp(&right.from))
            .then_with(|| left.to.cmp(&right.to))
    });
    slow_transitions.truncate(5);
    let transition_counts_insufficient = slow_transitions.is_empty() && !details.bigrams.is_empty();

    let mut weak_fingers = Vec::new();
    if let Some(row) = details.fingers.iter().filter(|row| row.total > 0).max_by(|left, right| {
        (1.0 - left.accuracy)
            .total_cmp(&(1.0 - right.accuracy))
            .then_with(|| right.name.cmp(&left.name))
    }) {
        weak_fingers.push(WeakFinger {
            name: row.name.clone(),
            reason: WeakFingerReason::ErrorRate,
            error_rate: 1.0 - row.accuracy,
            iki_median_ms: row.iki_median_ms,
        });
    }
    if let Some(row) = details
        .fingers
        .iter()
        .filter(|row| row.n > 0 && weak_fingers.iter().all(|finger| finger.name != row.name))
        .max_by(|left, right| {
            left.iki_median_ms
                .total_cmp(&right.iki_median_ms)
                .then_with(|| right.name.cmp(&left.name))
        })
    {
        weak_fingers.push(WeakFinger {
            name: row.name.clone(),
            reason: WeakFingerReason::Iki,
            error_rate: 1.0 - row.accuracy,
            iki_median_ms: row.iki_median_ms,
        });
    }

    let changes = benchmark_changes(current, history);
    let mistyped_keys = current
        .iter()
        .flat_map(|record| &record.log.keystrokes)
        .filter(|key| key.kind == KeystrokeKind::Char && !key.correct)
        .filter_map(|key| key.expected.clone())
        .fold(BTreeMap::<String, u32>::new(), |mut counts, key| {
            *counts.entry(key).or_default() += 1;
            counts
        });
    let practice = practice_suggestions(prescriptions, layout, &slow_transitions, &weak_fingers, &mistyped_keys);
    BenchmarkRunAnalysis {
        slow_transitions,
        transition_counts_insufficient,
        weak_fingers,
        changes,
        practice,
        mistyped_keys,
    }
}

fn benchmark_changes(current: &[SessionRecord], history: &[SessionRecord]) -> Vec<BenchChange> {
    let mut changes = Vec::new();
    for record in current {
        let Some((_, part, _)) = record.header.bench_id.as_deref().and_then(parse_bench_id) else {
            continue;
        };
        let preferred = preferred_bench_series(history.iter().filter_map(|row| row.header.bench_id.as_deref()), part);
        let current_at = parse_time(&record.header.started_at);
        let previous = history
            .iter()
            .filter(|candidate| !std::ptr::eq(*candidate, record))
            .filter(|candidate| {
                candidate
                    .header
                    .bench_id
                    .as_deref()
                    .and_then(parse_bench_id)
                    .is_some_and(|(series, candidate_part, _)| candidate_part == part && is_preferred_bench_series(series, part, preferred))
            })
            .filter(|candidate| match (parse_time(&candidate.header.started_at), current_at) {
                (Some(candidate_at), Some(current_at)) => candidate_at < current_at,
                _ => true,
            })
            .max_by_key(|candidate| parse_time(&candidate.header.started_at));
        if let Some(previous) = previous {
            changes.push(BenchChange {
                part,
                metric: BenchMetric::Wpm,
                delta: record.header.net_wpm - previous.header.net_wpm,
            });
            changes.push(BenchChange {
                part,
                metric: BenchMetric::Accuracy,
                delta: (record.header.accuracy - previous.header.accuracy) * 100.0,
            });
        }
    }
    changes.sort_by(|left, right| {
        left.part
            .cmp(&right.part)
            .then_with(|| metric_order(left.metric).cmp(&metric_order(right.metric)))
    });
    changes
}

fn practice_suggestions(
    prescriptions: &[Prescription],
    layout: &KeyboardLayout,
    transitions: &[SlowTransition],
    fingers: &[WeakFinger],
    mistypes: &BTreeMap<String, u32>,
) -> Vec<PracticeSuggestion> {
    let mut suggestions = Vec::new();
    let mut seen = BTreeSet::new();
    for prescription in prescriptions
        .iter()
        .filter(|row| row.layout == layout.name() && matches!(row.status.as_str(), "active" | "pending"))
    {
        let from = prescription.target.get("from").and_then(serde_json::Value::as_str);
        let to = prescription.target.get("to").and_then(serde_json::Value::as_str);
        if let (Some(from), Some(to)) = (from, to) {
            push_unique_transition(&mut suggestions, &mut seen, from, to);
        }
        if suggestions.len() >= 3 {
            return suggestions;
        }
    }
    for row in transitions {
        push_unique_transition(&mut suggestions, &mut seen, &row.from, &row.to);
        if suggestions.len() >= 3 {
            return suggestions;
        }
    }
    if let Some(finger) = fingers.first() {
        suggestions.push(PracticeSuggestion::Finger { name: finger.name.clone() });
    }
    if suggestions.len() < 3 && !mistypes.is_empty() {
        let mut keys = mistypes.iter().collect::<Vec<_>>();
        keys.sort_by(|(left_key, left), (right_key, right)| right.cmp(left).then_with(|| left_key.cmp(right_key)));
        suggestions.push(PracticeSuggestion::Keys {
            keys: keys.into_iter().take(3).map(|(key, _)| key.clone()).collect(),
        });
    }
    suggestions.truncate(3);
    suggestions
}

fn push_unique_transition(suggestions: &mut Vec<PracticeSuggestion>, seen: &mut BTreeSet<(String, String)>, from: &str, to: &str) {
    if seen.insert((from.to_owned(), to.to_owned())) {
        suggestions.push(PracticeSuggestion::Transition {
            from: from.to_owned(),
            to: to.to_owned(),
        });
    }
}

fn parse_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value).ok().map(|time| time.with_timezone(&Utc))
}

const fn metric_order(metric: BenchMetric) -> u8 {
    match metric {
        BenchMetric::Wpm => 0,
        BenchMetric::Accuracy => 1,
    }
}
