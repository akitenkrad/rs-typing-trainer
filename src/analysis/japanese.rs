//! Diagnostics for Japanese romaji input.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::analysis::metrics::AnalysisSettings;
use crate::analysis::model::{ModelView, PlayerModel, build_player_model, session_language};
use crate::data::JapaneseText;
use crate::engine::romaji::RomajiJudge;
use crate::engine::session::KeystrokeKind;
use crate::layout::KeyboardLayout;
use crate::layout::geometry::KeyboardGeometry;
use crate::record::SessionRecord;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KanaTiming {
    pub kana: String,
    /// Number of accepted occurrences, including ones without a duration.
    pub n: u64,
    pub median_ms: Option<f64>,
    pub error_rate: f64,
    pub errors: u64,
    pub attempts: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpellingDecision {
    Suggested,
    Reference,
    NotFaster,
}

impl SpellingDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Suggested => "suggested",
            Self::Reference => "reference",
            Self::NotFaster => "not-faster",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpellingSuggestion {
    pub kana: String,
    pub current: String,
    pub candidate: String,
    pub current_expected_ms: f64,
    pub candidate_expected_ms: f64,
    pub savings_ms: f64,
    pub savings_ratio: f64,
    pub contexts: u64,
    pub minimum_effective_n: f64,
    pub decision: SpellingDecision,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LanguageTransitionRatio {
    pub from: String,
    pub to: String,
    pub english_iki_ms: f64,
    pub japanese_iki_ms: f64,
    pub ratio: f64,
    pub english_effective_n: f64,
    pub japanese_effective_n: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LanguageComparison {
    pub median_ratio: Option<f64>,
    /// Sorted by distance from a ratio of one, largest first.
    pub transitions: Vec<LanguageTransitionRatio>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct JapaneseDiagnosis {
    pub kana: Vec<KanaTiming>,
    pub spelling: Vec<SpellingSuggestion>,
    pub language_comparison: LanguageComparison,
}

#[allow(clippy::too_many_arguments)]
pub fn analyze_japanese(
    records: &[SessionRecord],
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
    settings: AnalysisSettings,
    as_of: DateTime<Utc>,
    corpus: &[JapaneseText],
    min_transition_n: u64,
    spelling_suggestions: bool,
    error_cost_ms: f64,
) -> JapaneseDiagnosis {
    let model = build_player_model(records, layout, geometry, settings, ModelView::Recent, as_of);
    let kana = kana_timings(records, settings.pause_ms);
    let observed_spellings = observed_spellings(records);
    JapaneseDiagnosis {
        kana,
        spelling: if spelling_suggestions {
            spelling_analysis(corpus, &model, &observed_spellings, min_transition_n, error_cost_ms)
        } else {
            Vec::new()
        },
        language_comparison: language_comparison(records, layout, geometry, settings, as_of, min_transition_n),
    }
}

#[derive(Default)]
struct KanaAccumulator {
    durations: Vec<f64>,
    occurrences: u64,
    errors: u64,
    attempts: u64,
}

fn kana_timings(records: &[SessionRecord], pause_ms: u64) -> Vec<KanaTiming> {
    let mut values = BTreeMap::<String, KanaAccumulator>::new();
    for record in records.iter().filter(|record| record.header.metrics_version > 0) {
        for item in &record.log.items {
            let Some(reading) = item.reading.as_deref() else { continue };
            let correct = record
                .log
                .keystrokes
                .iter()
                .filter(|key| key.item_idx == item.idx && key.kind == KeystrokeKind::Char && key.correct)
                .collect::<Vec<_>>();
            let spelling = correct.iter().filter_map(|key| key.typed.as_deref()).collect::<String>();
            let Some(segments) = RomajiJudge::new(reading).spelling_segments(&spelling) else {
                continue;
            };
            let mut start = 0_usize;
            let mut previous_end_time = None;
            for (kana, segment) in segments {
                let length = segment.chars().count();
                let end = start + length;
                if end > correct.len() || length == 0 {
                    break;
                }
                let end_time = correct[end - 1].t_us;
                let entry = values.entry(kana).or_default();
                entry.occurrences += 1;
                if let Some(previous_end_time) = previous_end_time {
                    let duration_ms = end_time.saturating_sub(previous_end_time) as f64 / 1_000.0;
                    if duration_ms <= pause_ms as f64 {
                        entry.durations.push(duration_ms);
                    }
                }
                for key in record
                    .log
                    .keystrokes
                    .iter()
                    .filter(|key| key.item_idx == item.idx && key.kind == KeystrokeKind::Char)
                    .filter(|key| (start..end).contains(&(key.pos as usize)))
                {
                    entry.attempts += 1;
                    entry.errors += u64::from(!key.correct);
                }
                previous_end_time = Some(end_time);
                start = end;
            }
        }
    }
    values
        .into_iter()
        .map(|(kana, mut value)| KanaTiming {
            kana,
            n: value.occurrences,
            median_ms: median(&mut value.durations),
            error_rate: if value.attempts > 0 {
                value.errors as f64 / value.attempts as f64
            } else {
                0.0
            },
            errors: value.errors,
            attempts: value.attempts,
        })
        .collect()
}

fn observed_spellings(records: &[SessionRecord]) -> BTreeMap<String, BTreeSet<String>> {
    let mut observed = BTreeMap::<String, BTreeSet<String>>::new();
    for record in records.iter().filter(|record| record.header.metrics_version > 0) {
        for item in &record.log.items {
            let Some(reading) = item.reading.as_deref() else {
                continue;
            };
            let spelling = record
                .log
                .keystrokes
                .iter()
                .filter(|key| key.item_idx == item.idx && key.kind == KeystrokeKind::Char && key.correct)
                .filter_map(|key| key.typed.as_deref())
                .collect::<String>();
            let Some(segments) = RomajiJudge::new(reading).spelling_segments(&spelling) else {
                continue;
            };
            for (kana, spelling) in segments {
                observed.entry(kana).or_default().insert(spelling);
            }
        }
    }
    observed
}

#[derive(Default)]
struct SuggestionAccumulator {
    current_cost: f64,
    candidate_cost: f64,
    contexts: u64,
    minimum_effective_n: f64,
    sufficient: bool,
}

fn spelling_analysis(
    corpus: &[JapaneseText],
    model: &PlayerModel,
    observed_spellings: &BTreeMap<String, BTreeSet<String>>,
    min_transition_n: u64,
    error_cost_ms: f64,
) -> Vec<SpellingSuggestion> {
    let mut values = BTreeMap::<(String, String, String), SuggestionAccumulator>::new();
    for word in corpus {
        let judge = RomajiJudge::new(&word.reading);
        let preferred = judge.preferred_spelling(&model.spelling_preferences);
        let Some(segments) = judge.spelling_segments(&preferred) else {
            continue;
        };
        let variants = judge.spelling_variants();
        for index in 0..segments.len() {
            let (kana, current) = &segments[index];
            let Some(observed) = observed_spellings.get(kana) else {
                continue;
            };
            if is_standalone_small_kana(kana) {
                continue;
            }
            let Some((_, candidates)) = variants.get(index) else { continue };
            let candidates = candidates
                .iter()
                .filter(|candidate| {
                    is_common_spelling(kana, candidate, candidates)
                        || (observed.contains(*candidate) && is_separate_small_kana_spelling(kana, candidate))
                })
                .collect::<Vec<_>>();
            if candidates.len() < 2 {
                continue;
            }
            let previous = index.checked_sub(1).and_then(|previous| segments[previous].1.chars().last());
            let next = segments.get(index + 1).and_then(|next| next.1.chars().next());
            let current_cost = spelling_cost(&local_sequence(previous, current, next), model, error_cost_ms, min_transition_n);
            for candidate in candidates.into_iter().filter(|candidate| *candidate != current) {
                let candidate_cost = spelling_cost(&local_sequence(previous, candidate, next), model, error_cost_ms, min_transition_n);
                let entry = values
                    .entry((kana.clone(), current.clone(), candidate.clone()))
                    .or_insert_with(|| SuggestionAccumulator {
                        minimum_effective_n: f64::INFINITY,
                        sufficient: true,
                        ..SuggestionAccumulator::default()
                    });
                entry.current_cost += current_cost.cost_ms;
                entry.candidate_cost += candidate_cost.cost_ms;
                entry.contexts += 1;
                entry.minimum_effective_n = entry
                    .minimum_effective_n
                    .min(current_cost.minimum_effective_n)
                    .min(candidate_cost.minimum_effective_n);
                entry.sufficient &= current_cost.sufficient && candidate_cost.sufficient;
            }
        }
    }
    let mut rows = values
        .into_iter()
        .filter_map(|((kana, current, candidate), value)| {
            if value.contexts == 0 {
                return None;
            }
            let current_expected_ms = value.current_cost / value.contexts as f64;
            let candidate_expected_ms = value.candidate_cost / value.contexts as f64;
            let savings_ms = current_expected_ms - candidate_expected_ms;
            let savings_ratio = if current_expected_ms > 0.0 {
                savings_ms / current_expected_ms
            } else {
                0.0
            };
            let materially_faster = savings_ms >= 15.0 && savings_ratio >= 0.08;
            Some(SpellingSuggestion {
                kana,
                current,
                candidate,
                current_expected_ms,
                candidate_expected_ms,
                savings_ms,
                savings_ratio,
                contexts: value.contexts,
                minimum_effective_n: value.minimum_effective_n,
                decision: if materially_faster && value.sufficient {
                    SpellingDecision::Suggested
                } else if materially_faster {
                    SpellingDecision::Reference
                } else {
                    SpellingDecision::NotFaster
                },
            })
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        left.kana
            .cmp(&right.kana)
            .then_with(|| right.savings_ms.total_cmp(&left.savings_ms))
            .then_with(|| left.candidate.cmp(&right.candidate))
    });
    rows
}

fn is_standalone_small_kana(kana: &str) -> bool {
    matches!(kana, "ぁ" | "ぃ" | "ぅ" | "ぇ" | "ぉ" | "ゃ" | "ゅ" | "ょ" | "ゎ" | "ゕ" | "ゖ")
}

fn is_separate_small_kana_spelling(kana: &str, spelling: &str) -> bool {
    kana.chars().count() > 1
        && kana
            .chars()
            .last()
            .is_some_and(|last| matches!(last, 'ぁ' | 'ぃ' | 'ぅ' | 'ぇ' | 'ぉ' | 'ゃ' | 'ゅ' | 'ょ' | 'ゎ'))
        && spelling.chars().any(|character| matches!(character, 'x' | 'l'))
}

fn is_common_spelling(kana: &str, candidate: &str, accepted: &[String]) -> bool {
    let listed = match kana {
        "し" => &["shi", "si", "ci"][..],
        "ち" => &["chi", "ti"],
        "つ" => &["tsu", "tu"],
        "ふ" => &["fu", "hu"],
        "じ" => &["ji", "zi"],
        "しゃ" => &["sha", "sya"],
        "しゅ" => &["shu", "syu"],
        "しょ" => &["sho", "syo"],
        "ちゃ" => &["cha", "tya", "cya"],
        "ちゅ" => &["chu", "tyu", "cyu"],
        "ちょ" => &["cho", "tyo", "cyo"],
        "じゃ" => &["ja", "zya", "jya"],
        "じゅ" => &["ju", "zyu", "jyu"],
        "じょ" => &["jo", "zyo", "jyo"],
        "か" => &["ka", "ca"],
        "く" => &["ku", "cu"],
        "こ" => &["ko", "co"],
        "ん" => &["n", "nn", "n'"],
        "っ" => return candidate == "ltu" || candidate == "xtu" || candidate.chars().count() == 1,
        _ => return accepted.first().is_some_and(|canonical| canonical == candidate),
    };
    listed.contains(&candidate)
}

struct Cost {
    cost_ms: f64,
    minimum_effective_n: f64,
    sufficient: bool,
}

fn spelling_cost(sequence: &str, model: &PlayerModel, error_cost_ms: f64, min_transition_n: u64) -> Cost {
    let characters = sequence.chars().collect::<Vec<_>>();
    let mut cost_ms = 0.0;
    let mut minimum_effective_n = f64::INFINITY;
    let mut sufficient = true;
    for pair in characters.windows(2) {
        if let Some(transition) = model.transition(&pair[0].to_string(), &pair[1].to_string()) {
            cost_ms += transition.estimated_iki_ms + transition.estimated_error_rate * error_cost_ms;
            minimum_effective_n = minimum_effective_n.min(transition.effective_n);
            sufficient &= transition.effective_n >= min_transition_n as f64;
        } else {
            cost_ms += model.personal_iki_median_ms;
            minimum_effective_n = 0.0;
            sufficient = false;
        }
    }
    Cost {
        cost_ms,
        minimum_effective_n: if minimum_effective_n.is_infinite() { 0.0 } else { minimum_effective_n },
        sufficient,
    }
}

fn local_sequence(previous: Option<char>, spelling: &str, next: Option<char>) -> String {
    let mut sequence = String::new();
    if let Some(previous) = previous {
        sequence.push(previous);
    }
    sequence.push_str(spelling);
    if let Some(next) = next {
        sequence.push(next);
    }
    sequence
}

fn language_comparison(
    records: &[SessionRecord],
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
    settings: AnalysisSettings,
    as_of: DateTime<Utc>,
    min_transition_n: u64,
) -> LanguageComparison {
    let english_records = records
        .iter()
        .filter(|record| session_language(record) == "en")
        .cloned()
        .collect::<Vec<_>>();
    let japanese_records = records
        .iter()
        .filter(|record| session_language(record) == "ja")
        .cloned()
        .collect::<Vec<_>>();
    let english = build_player_model(&english_records, layout, geometry, settings, ModelView::Recent, as_of);
    let japanese = build_player_model(&japanese_records, layout, geometry, settings, ModelView::Recent, as_of);
    let mut transitions = english
        .transitions
        .iter()
        .filter(|row| row.effective_n >= min_transition_n as f64)
        .filter_map(|left| {
            let right = japanese.transition(&left.from, &left.to)?;
            if right.effective_n < min_transition_n as f64 || left.estimated_iki_ms <= 0.0 {
                return None;
            }
            Some(LanguageTransitionRatio {
                from: left.from.clone(),
                to: left.to.clone(),
                english_iki_ms: left.estimated_iki_ms,
                japanese_iki_ms: right.estimated_iki_ms,
                ratio: right.estimated_iki_ms / left.estimated_iki_ms,
                english_effective_n: left.effective_n,
                japanese_effective_n: right.effective_n,
            })
        })
        .collect::<Vec<_>>();
    let mut ratios = transitions.iter().map(|row| row.ratio).collect::<Vec<_>>();
    transitions.sort_by(|left, right| {
        right
            .ratio
            .ln()
            .abs()
            .total_cmp(&left.ratio.ln().abs())
            .then_with(|| (&left.from, &left.to).cmp(&(&right.from, &right.to)))
    });
    LanguageComparison {
        median_ratio: median(&mut ratios),
        transitions,
    }
}

fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        Some((values[middle - 1] + values[middle]) / 2.0)
    } else {
        Some(values[middle])
    }
}
