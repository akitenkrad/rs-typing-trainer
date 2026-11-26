//! Rule-based, localized analysis summary shared by the TUI and CLI.

use std::collections::BTreeMap;

use chrono::{DateTime, Local, NaiveDate};

use crate::analysis::bench_v3::{BenchV3Group, bench_v3_record_order, bench_v3_run_key, compute_bench_v3_group_metrics};
use crate::analysis::metrics::{AnalysisSettings, DetailedMetrics, analyze_records};
use crate::bench::{BenchPart, is_preferred_bench_series, parse_bench_id, parse_v3_bench_id, preferred_bench_series};
use crate::config::{BenchSeries, Config};
use crate::i18n::Lang;
use crate::layout::geometry::geometry;
use crate::layout::{KeyboardLayout, builtin_layout};
use crate::record::SessionRecord;

const SPARSE_N: u64 = 10;
const ACCURACY_RECOMMENDATION: f64 = 0.95;
const TREND_FLAT_WPM_PER_WEEK: f64 = 0.5;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SummaryReport {
    pub lines: Vec<String>,
}

impl SummaryReport {
    pub fn to_text(&self) -> String {
        let mut output = self.lines.join("\n");
        output.push('\n');
        output
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillAxis {
    SustainedSpeed,
    Accuracy,
    Rhythm,
    Transitions,
    Reach,
    Reaction,
    Correction,
    Japanese,
}

impl SkillAxis {
    pub const ALL: [Self; 8] = [
        Self::SustainedSpeed,
        Self::Accuracy,
        Self::Rhythm,
        Self::Transitions,
        Self::Reach,
        Self::Reaction,
        Self::Correction,
        Self::Japanese,
    ];

    pub fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::SustainedSpeed, Lang::En) => "Sustained speed",
            (Self::SustainedSpeed, Lang::Ja) => "持続速度",
            (Self::Accuracy, Lang::En) => "Accuracy",
            (Self::Accuracy, Lang::Ja) => "正確さ",
            (Self::Rhythm, Lang::En) => "Rhythm",
            (Self::Rhythm, Lang::Ja) => "リズム",
            (Self::Transitions, Lang::En) => "Transitions",
            (Self::Transitions, Lang::Ja) => "遷移",
            (Self::Reach, Lang::En) => "Reach",
            (Self::Reach, Lang::Ja) => "到達",
            (Self::Reaction, Lang::En) => "Reaction",
            (Self::Reaction, Lang::Ja) => "反応",
            (Self::Correction, Lang::En) => "Correction",
            (Self::Correction, Lang::Ja) => "修正",
            (Self::Japanese, Lang::En) => "Japanese",
            (Self::Japanese, Lang::Ja) => "日本語",
        }
    }

    pub fn part(self) -> &'static str {
        match self {
            Self::SustainedSpeed | Self::Accuracy | Self::Rhythm | Self::Reaction | Self::Correction => "A",
            Self::Transitions | Self::Reach => "B",
            Self::Japanese => "C",
        }
    }

    pub fn higher_is_better(self) -> bool {
        matches!(self, Self::SustainedSpeed | Self::Accuracy | Self::Japanese)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkillAxisValue {
    pub axis: SkillAxis,
    pub raw: Option<f64>,
    pub percentile: Option<f64>,
}

pub fn generate(records: &[SessionRecord], config: &Config, as_of: NaiveDate, lang: Lang) -> SummaryReport {
    if records.is_empty() {
        return SummaryReport {
            lines: vec![match lang {
                Lang::En => "No records yet. Complete a session to see your summary.".to_owned(),
                Lang::Ja => "まだ記録がありません．セッションを完了すると総評を表示します．".to_owned(),
            }],
        };
    }
    let layout = builtin_layout(&config.keyboard.layout).unwrap_or_else(|_| builtin_layout("qwerty").expect("bundled qwerty"));
    let keyboard_geometry = geometry(&config.keyboard.geometry).unwrap_or_else(|| geometry("ansi").expect("bundled ansi"));
    let details = analyze_records(records, &layout, &keyboard_geometry, analysis_settings(config));
    generate_with_metrics(records, config, &layout, &details, as_of, lang)
}

pub fn generate_with_metrics(
    records: &[SessionRecord],
    config: &Config,
    layout: &KeyboardLayout,
    details: &DetailedMetrics,
    as_of: NaiveDate,
    lang: Lang,
) -> SummaryReport {
    if records.is_empty() {
        return generate(records, config, as_of, lang);
    }
    let axes = skill_axis_values(records, layout, config);
    let recent = records
        .iter()
        .filter(|record| record_date(record).is_some_and(|date| date <= as_of && date >= as_of - chrono::Duration::days(6)))
        .collect::<Vec<_>>();
    let mut lines = Vec::new();
    current_ability(&mut lines, &recent, records, lang);
    lines.push(String::new());
    trend(&mut lines, records, config.analysis.pause_ms, as_of, lang);
    lines.push(String::new());
    strengths_and_weaknesses(&mut lines, &axes, lang);
    lines.push(String::new());
    weak_keys_and_transitions(&mut lines, details, config.analysis.min_transition_n, lang);
    lines.push(String::new());
    recommendations(&mut lines, &recent, records, details, &axes, config, lang);
    lines.push(String::new());
    data_status(&mut lines, records, details, &axes, as_of, lang);
    SummaryReport { lines }
}

pub fn skill_axis_values(records: &[SessionRecord], layout: &KeyboardLayout, config: &Config) -> Vec<SkillAxisValue> {
    if records
        .iter()
        .any(|record| record.header.bench_id.as_deref().and_then(parse_v3_bench_id).is_some())
    {
        return v3_skill_axis_values(records, layout, config);
    }
    SkillAxis::ALL
        .into_iter()
        .map(|axis| {
            let part = bench_part_value(axis.part()).expect("skill axes use benchmark parts");
            let series = preferred_bench_series(records.iter().filter_map(|record| record.header.bench_id.as_deref()), part);
            let matching = records
                .iter()
                .filter(|record| {
                    bench_identity(record).is_some_and(|(candidate_series, candidate_part)| {
                        candidate_part == part && is_preferred_bench_series(candidate_series, part, series)
                    })
                })
                .collect::<Vec<_>>();
            let raw = matching.last().and_then(|record| axis_value(axis, record, layout, config));
            let history = matching
                .iter()
                .filter_map(|record| axis_value(axis, record, layout, config))
                .collect::<Vec<_>>();
            let percentile =
                (raw.is_some() && history.len() >= 3).then(|| percentile_position(raw.unwrap_or_default(), &history, axis.higher_is_better()));
            SkillAxisValue { axis, raw, percentile }
        })
        .collect()
}

fn v3_skill_axis_values(records: &[SessionRecord], layout: &KeyboardLayout, config: &Config) -> Vec<SkillAxisValue> {
    SkillAxis::ALL
        .into_iter()
        .map(|axis| {
            let group = match axis {
                SkillAxis::Japanese => BenchV3Group::Japanese,
                SkillAxis::Transitions => BenchV3Group::Transition,
                _ => BenchV3Group::English,
            };
            let history = v3_group_record_runs(records, group)
                .into_iter()
                .filter_map(|run| v3_axis_value(axis, &run, layout, config))
                .collect::<Vec<_>>();
            let raw = history.last().copied();
            let percentile = (history.len() >= 3).then(|| percentile_position(raw.unwrap_or_default(), &history, axis.higher_is_better()));
            SkillAxisValue { axis, raw, percentile }
        })
        .collect()
}

/// Complete v3 runs for a group, keyed by machine-local date and parallel
/// form, but ordered by the actual finish time. Form 08 followed by form 01
/// on one day therefore stays in that order instead of being sorted by form.
fn v3_group_record_runs(records: &[SessionRecord], group: BenchV3Group) -> Vec<Vec<SessionRecord>> {
    let mut grouped = BTreeMap::<(NaiveDate, u8), Vec<SessionRecord>>::new();
    for record in records {
        let Some((part, _)) = record.header.bench_id.as_deref().and_then(parse_v3_bench_id) else {
            continue;
        };
        if !group.parts().contains(&part) {
            continue;
        }
        let Some((date, form)) = bench_v3_run_key(record) else {
            continue;
        };
        grouped.entry((date, form)).or_default().push(record.clone());
    }
    let mut runs = grouped
        .into_values()
        .filter_map(|records| {
            let present = records
                .iter()
                .filter_map(|record| record.header.bench_id.as_deref().and_then(parse_v3_bench_id).map(|(part, _)| part))
                .collect::<Vec<_>>();
            if !group.parts().iter().all(|part| present.contains(part)) {
                return None;
            }
            let order = records.iter().filter_map(bench_v3_record_order).max()?;
            Some((order, records))
        })
        .collect::<Vec<_>>();
    runs.sort_by_key(|(order, _)| *order);
    runs.into_iter().map(|(_, records)| records).collect()
}

fn v3_axis_value(axis: SkillAxis, records: &[SessionRecord], layout: &KeyboardLayout, config: &Config) -> Option<f64> {
    let group = match axis {
        SkillAxis::Japanese => BenchV3Group::Japanese,
        SkillAxis::Transitions => BenchV3Group::Transition,
        _ => BenchV3Group::English,
    };
    let summary = compute_bench_v3_group_metrics(records, group, config.analysis.pause_ms)?;
    match axis {
        SkillAxis::SustainedSpeed | SkillAxis::Japanese => summary.metrics.effective_wpm,
        SkillAxis::Accuracy => Some(summary.metrics.accuracy * 100.0),
        _ => axis_value_for_records(axis, records, layout, config),
    }
}

fn axis_value_for_records(axis: SkillAxis, records: &[SessionRecord], layout: &KeyboardLayout, config: &Config) -> Option<f64> {
    let keyboard_geometry = records
        .first()
        .and_then(|record| geometry(&record.header.geometry))
        .or_else(|| geometry("ansi"))?;
    let details = analyze_records(records, layout, &keyboard_geometry, analysis_settings(config));
    match axis {
        SkillAxis::Rhythm if records.iter().all(|record| record.header.metrics_version == 0) => None,
        SkillAxis::Rhythm => Some(details.iki_cv),
        SkillAxis::Reach => reach_ratio(&details),
        SkillAxis::Transitions => {
            let mut values = details
                .bigrams
                .iter()
                .filter(|row| row.n >= config.analysis.min_transition_n)
                .map(|row| row.estimated_iki_ms)
                .collect::<Vec<_>>();
            values.sort_by(|left, right| right.total_cmp(left));
            let top = values.iter().take(20).copied().collect::<Vec<_>>();
            (!top.is_empty()).then(|| top.iter().sum::<f64>() / top.len() as f64 - details.iki_median_ms)
        }
        SkillAxis::Reaction => (details.reaction_median_ms > 0.0).then_some(details.reaction_median_ms),
        SkillAxis::Correction => (details.correction_median_ms > 0.0).then_some(details.correction_median_ms),
        SkillAxis::SustainedSpeed | SkillAxis::Accuracy | SkillAxis::Japanese => None,
    }
}

pub fn percentile_position(value: f64, history: &[f64], higher_is_better: bool) -> f64 {
    if history.is_empty() {
        return 0.0;
    }
    let favorable = history
        .iter()
        .filter(|sample| if higher_is_better { **sample <= value } else { **sample >= value })
        .count();
    100.0 * favorable as f64 / history.len() as f64
}

fn current_ability(lines: &mut Vec<String>, recent: &[&SessionRecord], all: &[SessionRecord], lang: Lang) {
    lines.push(heading("Current ability", "今の実力", lang));
    let mut modes = BTreeMap::<&str, Vec<&SessionRecord>>::new();
    for record in recent.iter().copied().filter(|record| !is_benchmark(record)) {
        modes.entry(&record.header.mode).or_default().push(record);
    }
    let mut modes = modes.into_iter().collect::<Vec<_>>();
    modes.sort_by(|left, right| right.1.len().cmp(&left.1.len()).then_with(|| left.0.cmp(right.0)));
    if modes.is_empty() {
        lines.push(match lang {
            Lang::En => "- No practice records in the last 7 days.".to_owned(),
            Lang::Ja => "- 直近 7 日に練習記録がありません．".to_owned(),
        });
    } else {
        for (mode, values) in modes.into_iter().take(2) {
            let wpm = median(&values.iter().map(|record| record.header.net_wpm).collect::<Vec<_>>());
            let accuracy = median(&values.iter().map(|record| record.header.accuracy).collect::<Vec<_>>()) * 100.0;
            lines.push(match lang {
                Lang::En => format!(
                    "- {} (last 7 days): median net WPM {wpm:.1}, accuracy {accuracy:.1}% (n={}).",
                    mode_label(mode, lang),
                    values.len()
                ),
                Lang::Ja => format!(
                    "- {}（直近 7 日）: net WPM 中央値 {wpm:.1}，正確さ中央値 {accuracy:.1}%（n={}）．",
                    mode_label(mode, lang),
                    values.len()
                ),
            });
        }
    }
    if let Some(latest) = all.iter().rev().find(|record| bench_identity(record).is_some()) {
        let part = bench_part(latest).unwrap_or("?");
        let identity = bench_identity(latest);
        let latest_index = all.iter().rposition(|record| std::ptr::eq(record, latest)).unwrap_or(all.len());
        let previous = all[..latest_index].iter().rev().find(|record| bench_identity(record) == identity);
        let delta = previous.map(|record| latest.header.net_wpm - record.header.net_wpm);
        let delta = delta.map_or_else(
            || match lang {
                Lang::En => "first result".to_owned(),
                Lang::Ja => "初回".to_owned(),
            },
            |value| match lang {
                Lang::En => format!("{value:+.1} vs previous"),
                Lang::Ja => format!("前回比 {value:+.1}"),
            },
        );
        lines.push(match lang {
            Lang::En => format!(
                "- Latest bench: {} {:.1} WPM ({delta}).",
                bench_part_label(part, lang),
                latest.header.net_wpm
            ),
            Lang::Ja => format!(
                "- 最新のベンチ: {} {:.1} WPM（{delta}）．",
                bench_part_label(part, lang),
                latest.header.net_wpm
            ),
        });
    }
}

fn trend(lines: &mut Vec<String>, records: &[SessionRecord], pause_ms: u64, as_of: NaiveDate, lang: Lang) {
    lines.push(heading("Trend", "推移", lang));
    let v3 = [BenchV3Group::English, BenchV3Group::Japanese, BenchV3Group::Transition]
        .into_iter()
        .find_map(|group| {
            let runs = crate::analysis::bench_v3::compute_bench_v3_group_runs(records, group, pause_ms);
            (!runs.is_empty()).then_some((group, runs))
        });
    let trend_bench_part = if records.iter().any(|record| bench_part(record) == Some("A")) {
        Some("A")
    } else {
        records.iter().rev().find_map(|record| bench_part(record))
    };
    let uses_bench = v3.is_some() || trend_bench_part.is_some();
    let trend_part = trend_bench_part.and_then(bench_part_value);
    let trend_series =
        trend_part.and_then(|part| preferred_bench_series(records.iter().filter_map(|record| record.header.bench_id.as_deref()), part));
    let samples = if let Some((_, runs)) = &v3 {
        let raw = runs.iter().filter_map(|run| run.metrics.effective_wpm).collect::<Vec<_>>();
        let rolling = crate::analysis::bench_v3::rolling_median_3(&raw);
        runs.iter()
            .filter(|run| run.metrics.effective_wpm.is_some())
            .zip(rolling)
            .map(|(run, value)| (run.date, value))
            .filter(|(date, _)| *date <= as_of && *date >= as_of - chrono::Duration::days(27))
            .collect::<Vec<_>>()
    } else {
        records
            .iter()
            .filter(|record| {
                if let Some(part) = trend_part {
                    bench_identity(record)
                        .is_some_and(|(series, candidate)| candidate == part && is_preferred_bench_series(series, part, trend_series))
                } else {
                    !is_benchmark(record)
                }
            })
            .filter_map(|record| Some((record_date(record)?, record.header.net_wpm)))
            .filter(|(date, _)| *date <= as_of && *date >= as_of - chrono::Duration::days(27))
            .collect::<Vec<_>>()
    };
    let mut weeks: [Vec<f64>; 4] = std::array::from_fn(|_| Vec::new());
    for (date, value) in samples {
        let age = (as_of - date).num_days() as usize;
        weeks[3 - age / 7].push(value);
    }
    let points = weeks
        .iter()
        .enumerate()
        .filter(|(_, values)| !values.is_empty())
        .map(|(week, values)| (week as f64, median(values)))
        .collect::<Vec<_>>();
    if points.len() < 2 {
        lines.push(match lang {
            Lang::En => "- At least 2 weeks of records are needed for a four-week trend.".to_owned(),
            Lang::Ja => "- 4 週間の傾向には 2 週分以上の記録が必要です．".to_owned(),
        });
    } else {
        let slope = regression_slope(&points);
        let direction = match lang {
            Lang::En if slope > TREND_FLAT_WPM_PER_WEEK => "upward",
            Lang::En if slope < -TREND_FLAT_WPM_PER_WEEK => "downward",
            Lang::En => "flat",
            Lang::Ja if slope > TREND_FLAT_WPM_PER_WEEK => "上向き",
            Lang::Ja if slope < -TREND_FLAT_WPM_PER_WEEK => "下向き",
            Lang::Ja => "横ばい",
        };
        let source = if let Some((group, _)) = &v3 {
            match (group, lang) {
                (BenchV3Group::English, Lang::En) => "bench-v3 English",
                (BenchV3Group::Japanese, Lang::En) => "bench-v3 Japanese",
                (BenchV3Group::Transition, Lang::En) => "bench-v3 transitions",
                (BenchV3Group::English, Lang::Ja) => "bench-v3 英語",
                (BenchV3Group::Japanese, Lang::Ja) => "bench-v3 日本語",
                (BenchV3Group::Transition, Lang::Ja) => "bench-v3 移り変わり",
            }
        } else if uses_bench {
            bench_part_label(trend_bench_part.unwrap_or("?"), lang)
        } else {
            match lang {
                Lang::En => "practice records",
                Lang::Ja => "練習記録",
            }
        };
        lines.push(match lang {
            Lang::En => format!("- Four-week weekly-median slope ({source}): {direction}, {slope:+.1} WPM/week."),
            Lang::Ja => format!("- 直近 4 週の週中央値の傾き（{source}）: {direction}，{slope:+.1} WPM／週．"),
        });
    }
    if !uses_bench {
        lines.push(match lang {
            Lang::En => "- Practice WPM is not an improvement measure because practice difficulty changes.".to_owned(),
            Lang::Ja => "- 練習の難しさは変わるため，練習 WPM は上達の指標ではありません．".to_owned(),
        });
    }
}

fn strengths_and_weaknesses(lines: &mut Vec<String>, axes: &[SkillAxisValue], lang: Lang) {
    lines.push(heading("Strengths and weaknesses", "強み・弱み", lang));
    let available = axes.iter().filter(|value| value.percentile.is_some()).collect::<Vec<_>>();
    if available.is_empty() {
        lines.push(match lang {
            Lang::En => "- No axis has the 3 benchmark results needed for a percentile yet.".to_owned(),
            Lang::Ja => "- パーセンタイルに必要なベンチ 3 回分がある軸はまだありません．".to_owned(),
        });
        return;
    }
    let strongest = available
        .iter()
        .max_by(|left, right| left.percentile.unwrap_or_default().total_cmp(&right.percentile.unwrap_or_default()))
        .unwrap();
    let weakest = available
        .iter()
        .min_by(|left, right| left.percentile.unwrap_or_default().total_cmp(&right.percentile.unwrap_or_default()))
        .unwrap();
    lines.push(match lang {
        Lang::En => format!(
            "- Strongest: {} {:.1} percentile. Weakest: {} {:.1} percentile.",
            strongest.axis.label(lang),
            strongest.percentile.unwrap_or_default(),
            weakest.axis.label(lang),
            weakest.percentile.unwrap_or_default()
        ),
        Lang::Ja => format!(
            "- 最も高い軸: {} {:.1} パーセンタイル．最も低い軸: {} {:.1} パーセンタイル．",
            strongest.axis.label(lang),
            strongest.percentile.unwrap_or_default(),
            weakest.axis.label(lang),
            weakest.percentile.unwrap_or_default()
        ),
    });
}

fn weak_keys_and_transitions(lines: &mut Vec<String>, details: &DetailedMetrics, min_transition_n: u64, lang: Lang) {
    lines.push(heading("Weak keys and transitions", "弱いキーと遷移", lang));
    let mut transitions = details.bigrams.iter().filter(|row| row.n >= min_transition_n).collect::<Vec<_>>();
    transitions.sort_by(|left, right| right.estimated_iki_ms.total_cmp(&left.estimated_iki_ms));
    if transitions.is_empty() {
        lines.push(no_data_line(lang));
    } else {
        let values = transitions
            .into_iter()
            .take(3)
            .map(|row| {
                format!(
                    "{}→{} {:.1} ms (n={}{} )",
                    row.from,
                    row.to,
                    row.estimated_iki_ms,
                    row.n,
                    sparse_note(row.n, lang)
                )
                .replace(" )", ")")
            })
            .collect::<Vec<_>>()
            .join("; ");
        lines.push(match lang {
            Lang::En => format!("- High-cost transitions (shrunk estimate): {values}."),
            Lang::Ja => format!("- コストの高い遷移（縮小推定）: {values}．"),
        });
    }
    let mut keys = details.keys.iter().filter(|row| row.total > 0).collect::<Vec<_>>();
    keys.sort_by(|left, right| {
        (1.0 - right.accuracy)
            .total_cmp(&(1.0 - left.accuracy))
            .then_with(|| left.key.cmp(&right.key))
    });
    if keys.is_empty() {
        lines.push(no_data_line(lang));
    } else {
        let values = keys
            .into_iter()
            .take(3)
            .map(|row| {
                format!(
                    "{} {:.1}% (n={}{} )",
                    row.key,
                    (1.0 - row.accuracy) * 100.0,
                    row.total,
                    sparse_note(row.total, lang)
                )
                .replace(" )", ")")
            })
            .collect::<Vec<_>>()
            .join("; ");
        lines.push(match lang {
            Lang::En => format!("- Highest key error rates: {values}."),
            Lang::Ja => format!("- 誤打率の高いキー: {values}．"),
        });
    }
}

fn recommendations(
    lines: &mut Vec<String>,
    recent: &[&SessionRecord],
    records: &[SessionRecord],
    details: &DetailedMetrics,
    axes: &[SkillAxisValue],
    config: &Config,
    lang: Lang,
) {
    lines.push(heading("Recommended practice", "おすすめの練習", lang));
    let mut items = Vec::new();
    let recent_accuracy = (!recent.is_empty()).then(|| median(&recent.iter().map(|record| record.header.accuracy).collect::<Vec<_>>()));
    if recent_accuracy.is_some_and(|accuracy| accuracy < ACCURACY_RECOMMENDATION) || weak_axis(axes, SkillAxis::Accuracy) {
        items.push(match lang {
            Lang::En => "Accuracy: set a 95% accuracy gate and turn the pacer off. Setting: set [gate] min_accuracy = 0.95; press P on mode selection until the pacer is off.".to_owned(),
            Lang::Ja => "正確さ: 正確さの関門を 95% に設定し，ペーサーを外して正打を優先します．設定: `[gate] min_accuracy = 0.95` とし，モード選択で `P` を押してペーサーを無効にします．".to_owned(),
        });
    }
    let slowest = details
        .bigrams
        .iter()
        .filter(|row| row.n >= config.analysis.min_transition_n)
        .max_by(|left, right| left.estimated_iki_ms.total_cmp(&right.estimated_iki_ms));
    if let Some(row) = slowest
        && (weak_axis(axes, SkillAxis::Transitions) || (details.iki_median_ms > 0.0 && row.estimated_iki_ms >= details.iki_median_ms * 1.25))
        && items.len() < 3
    {
        items.push(match lang {
            Lang::En => format!(
                "Transitions: practice {}→{} in adaptive mode. Setting: choose adaptive on mode selection; use ngram with keys '{}{}' for a focused drill.",
                row.from, row.to, row.from, row.to
            ),
            Lang::Ja => format!(
                "遷移: {}→{} を `adaptive` で練習します．設定: モード選択で `adaptive` を選び，集中的に反復する場合は `ngram` のキーを `{}{}` にします．",
                row.from, row.to, row.from, row.to
            ),
        });
    }
    for axis in axes.iter().filter(|value| value.percentile.is_some_and(|position| position <= 25.0)) {
        if items.len() >= 3 {
            break;
        }
        let item = match axis.axis {
            SkillAxis::Reach => details.keys.iter().min_by(|left, right| left.accuracy.total_cmp(&right.accuracy)).map(|key| match lang {
                Lang::En => format!("Reach: use unlock or ngram for '{}'. Setting: choose unlock, or select '{}' in the ngram key picker.", key.key, key.key),
                Lang::Ja => format!("到達: `{}` を含む `unlock` または `ngram` を使います．設定: モード選択で `unlock`，または `ngram` のキー選択で `{}` を選びます．", key.key, key.key),
            }),
            SkillAxis::Reaction => Some(match lang {
                Lang::En => "Reaction: increase look-ahead. Setting: press ] on mode selection to raise preview items by one.".to_owned(),
                Lang::Ja => "反応: 先読みを 1 項目増やします．設定: モード選択で `]` を押し，先読み項目数を増やします．".to_owned(),
            }),
            SkillAxis::Rhythm => Some(match lang {
                Lang::En => "Rhythm: use rhythm mode. Setting: choose rhythm on mode selection and start with automatic BPM.".to_owned(),
                Lang::Ja => "リズム: `rhythm` を使います．設定: モード選択で `rhythm` を選び，BPM は自動設定から始めます．".to_owned(),
            }),
            SkillAxis::Japanese => Some(match lang {
                Lang::En => "Japanese: practice romaji. Setting: choose romaji on mode selection.".to_owned(),
                Lang::Ja => "日本語: `romaji` を練習します．設定: モード選択で `romaji` を選びます．".to_owned(),
            }),
            _ => None,
        };
        if let Some(item) = item
            && !items.contains(&item)
        {
            items.push(item);
        }
    }
    if items.len() < 3
        && records
            .iter()
            .rev()
            .find(|record| record.header.mode == "endurance")
            .is_some_and(|record| crate::practice::session_variants::compare_endurance_log(&record.log).warn)
    {
        items.push(match lang {
            Lang::En => "Endurance: use endurance mode and take a break afterward. Setting: choose endurance on mode selection; keep the initial 4-minute duration.".to_owned(),
            Lang::Ja => "持久: `endurance` の後に休憩を取ります．設定: モード選択で `endurance` を選び，最初は 4 分で実施します．".to_owned(),
        });
    }
    if items.is_empty() {
        let series = match config.bench.series {
            BenchSeries::V2 => "bench-v2",
            BenchSeries::V3 => "bench-v3",
        };
        items.push(match lang {
            Lang::En => format!("Keep practice specific and use {series} to measure change. Setting: choose one practice mode, then run B from the title screen after several sessions."),
            Lang::Ja => format!("弱点を 1 つに絞って練習し，変化は `{series}` で測ります．設定: 練習モードを 1 つ選び，数回の練習後にタイトル画面の `B` からベンチを実施します．"),
        });
    }
    for item in items.into_iter().take(3) {
        lines.push(format!("- {item}"));
    }
}

fn data_status(lines: &mut Vec<String>, records: &[SessionRecord], details: &DetailedMetrics, axes: &[SkillAxisValue], as_of: NaiveDate, lang: Lang) {
    lines.push(heading("Data status", "データの状態", lang));
    let logged = records.iter().filter(|record| record.header.metrics_version > 0).count();
    lines.push(match lang {
        Lang::En => format!("- Sessions with keystroke logs: {logged}."),
        Lang::Ja => format!("- 打鍵ログのあるセッション数: {logged}．"),
    });
    if let Some(last_bench) = records.iter().rev().find(|record| is_benchmark(record)).and_then(record_date) {
        let days = (as_of - last_bench).num_days().max(0);
        let reminder = if days >= 7 {
            match lang {
                Lang::En => " It is time to take a benchmark.",
                Lang::Ja => "そろそろベンチを受けましょう．",
            }
        } else {
            ""
        };
        lines.push(match lang {
            Lang::En => format!("- {days} day(s) since the last bench.{reminder}"),
            Lang::Ja => format!("- 最後のベンチから {days} 日．{reminder}"),
        });
    } else {
        lines.push(match lang {
            Lang::En => "- No benchmark results yet.".to_owned(),
            Lang::Ja => "- ベンチの記録はまだありません．".to_owned(),
        });
    }
    let missing_axes = axes
        .iter()
        .filter(|value| value.percentile.is_none())
        .map(|value| value.axis.label(lang))
        .collect::<Vec<_>>();
    let mut missing = Vec::new();
    if !missing_axes.is_empty() {
        missing.push(match lang {
            Lang::En => format!("axis percentiles ({})", missing_axes.join(", ")),
            Lang::Ja => format!("軸のパーセンタイル（{}）", missing_axes.join("・")),
        });
    }
    if details.bigrams.is_empty() {
        missing.push(
            match lang {
                Lang::En => "transitions",
                Lang::Ja => "遷移",
            }
            .to_owned(),
        );
    }
    lines.push(match lang {
        Lang::En if missing.is_empty() => "- Missing data: none.".to_owned(),
        Lang::En => format!("- Missing data: {}.", missing.join("; ")),
        Lang::Ja if missing.is_empty() => "- データが足りない項目: ありません．".to_owned(),
        Lang::Ja => format!("- データが足りない項目: {}．", missing.join("，")),
    });
}

/// The raw value of one skill axis for a single session, whether or not it is
/// a benchmark session. `skill_axis_values` reads benchmark parts only; the
/// practice menu judges `reach.*` on its check steps with this (design §5,
/// MYTASK-3622).
pub fn session_axis_value(axis: SkillAxis, record: &SessionRecord, layout: &KeyboardLayout, config: &Config) -> Option<f64> {
    axis_value(axis, record, layout, config)
}

/// The reach axis of one session: the mean IKI of the remote rows (number,
/// top, bottom) over the home row's (`far-home-iki-ratio`). The skill axes,
/// the `reach.*` judgment and the live goal of the unlock row (MYTASK-3641)
/// all read it from here.
pub fn session_reach_ratio(record: &SessionRecord, layout: &KeyboardLayout, settings: AnalysisSettings) -> Option<f64> {
    let keyboard_geometry = geometry(&record.header.geometry).or_else(|| geometry("ansi"))?;
    let details = analyze_records(std::slice::from_ref(record), layout, &keyboard_geometry, settings);
    reach_ratio(&details)
}

fn reach_ratio(details: &DetailedMetrics) -> Option<f64> {
    let home = details
        .rows
        .iter()
        .find(|row| row.name == "home" && row.n > 0)
        .map(|row| row.iki_median_ms)?;
    let remote = details
        .rows
        .iter()
        .filter(|row| matches!(row.name.as_str(), "number" | "top" | "bottom") && row.n > 0)
        .map(|row| row.iki_median_ms)
        .collect::<Vec<_>>();
    (!remote.is_empty() && home > 0.0).then(|| (remote.iter().sum::<f64>() / remote.len() as f64) / home)
}

fn axis_value(axis: SkillAxis, record: &SessionRecord, layout: &KeyboardLayout, config: &Config) -> Option<f64> {
    match axis {
        SkillAxis::SustainedSpeed | SkillAxis::Japanese => Some(record.header.net_wpm),
        SkillAxis::Accuracy => Some(record.header.accuracy * 100.0),
        SkillAxis::Rhythm if record.header.metrics_version == 0 => None,
        SkillAxis::Rhythm => Some(record.header.iki_cv),
        SkillAxis::Reach => session_reach_ratio(record, layout, analysis_settings(config)),
        SkillAxis::Transitions | SkillAxis::Reaction | SkillAxis::Correction => {
            let keyboard_geometry = geometry(&record.header.geometry).or_else(|| geometry("ansi"))?;
            let details = analyze_records(std::slice::from_ref(record), layout, &keyboard_geometry, analysis_settings(config));
            match axis {
                SkillAxis::Transitions => {
                    let mut values = details
                        .bigrams
                        .iter()
                        .filter(|row| row.n >= config.analysis.min_transition_n)
                        .map(|row| row.estimated_iki_ms)
                        .collect::<Vec<_>>();
                    values.sort_by(|left, right| right.total_cmp(left));
                    let top = values.iter().take(20).copied().collect::<Vec<_>>();
                    (!top.is_empty()).then(|| top.iter().sum::<f64>() / top.len() as f64 - details.iki_median_ms)
                }
                SkillAxis::Reaction => (details.reaction_median_ms > 0.0).then_some(details.reaction_median_ms),
                SkillAxis::Correction => (details.correction_median_ms > 0.0).then_some(details.correction_median_ms),
                _ => None,
            }
        }
    }
}

fn weak_axis(axes: &[SkillAxisValue], target: SkillAxis) -> bool {
    axes.iter()
        .any(|value| value.axis == target && value.percentile.is_some_and(|position| position <= 25.0))
}

fn sparse_note(n: u64, lang: Lang) -> &'static str {
    if n >= SPARSE_N {
        ""
    } else {
        match lang {
            Lang::En => ", preliminary",
            Lang::Ja => "，参考値",
        }
    }
}

fn regression_slope(points: &[(f64, f64)]) -> f64 {
    let count = points.len() as f64;
    let mean_x = points.iter().map(|point| point.0).sum::<f64>() / count;
    let mean_y = points.iter().map(|point| point.1).sum::<f64>() / count;
    let numerator = points.iter().map(|point| (point.0 - mean_x) * (point.1 - mean_y)).sum::<f64>();
    let denominator = points.iter().map(|point| (point.0 - mean_x).powi(2)).sum::<f64>();
    if denominator == 0.0 { 0.0 } else { numerator / denominator }
}

fn analysis_settings(config: &Config) -> AnalysisSettings {
    AnalysisSettings {
        pause_ms: config.analysis.pause_ms,
        shrinkage_k: config.analysis.shrinkage_k,
        half_life_days: config.analysis.half_life_days,
    }
}

fn record_date(record: &SessionRecord) -> Option<NaiveDate> {
    DateTime::parse_from_rfc3339(&record.header.finished_at)
        .ok()
        .map(|value| value.with_timezone(&Local).date_naive())
}

fn is_benchmark(record: &SessionRecord) -> bool {
    bench_identity(record).is_some() || record.header.bench_id.as_deref().and_then(parse_v3_bench_id).is_some()
}

fn bench_part(record: &SessionRecord) -> Option<&str> {
    let (_, part, _) = parse_bench_id(record.header.bench_id.as_deref()?)?;
    Some(part.as_str())
}

fn bench_identity(record: &SessionRecord) -> Option<(&str, BenchPart)> {
    let (series, part, _) = parse_bench_id(record.header.bench_id.as_deref()?)?;
    Some((series, part))
}

fn bench_part_value(part: &str) -> Option<BenchPart> {
    match part {
        "A" => Some(BenchPart::A),
        "B" => Some(BenchPart::B),
        "C" => Some(BenchPart::C),
        "D" => Some(BenchPart::D),
        _ => None,
    }
}

fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}

fn heading(en: &str, ja: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("{en}:"),
        Lang::Ja => format!("{ja}:"),
    }
}

fn mode_label(mode: &str, lang: Lang) -> &str {
    if lang == Lang::En {
        return mode;
    }
    match mode {
        "random" => "ランダム",
        "words" => "単語",
        "romaji" => "ローマ字",
        "sentence" => "短文",
        "phrase" => "句",
        "ngram" => "n-gram",
        "adaptive" => "適応",
        "unlock" => "解放",
        "rhythm" => "リズム",
        "burst" => "バースト",
        "endurance" => "持久",
        other => other,
    }
}

fn bench_part_label(part: &str, lang: Lang) -> &'static str {
    match (part, lang) {
        ("A", Lang::En) => "A English words and phrases",
        ("A", Lang::Ja) => "A 英単語と句",
        ("B", Lang::En) => "B coverage",
        ("B", Lang::Ja) => "B 網羅",
        ("C", Lang::En) => "C Japanese words and phrases",
        ("C", Lang::Ja) => "C 日本語の単語と句",
        ("D", Lang::En) => "D numbers/symbols",
        ("D", Lang::Ja) => "D 数字・記号",
        (_, Lang::En) => "unknown part",
        (_, Lang::Ja) => "不明なパート",
    }
}

fn no_data_line(lang: Lang) -> String {
    match lang {
        Lang::En => "- No data.".to_owned(),
        Lang::Ja => "- データがありません．".to_owned(),
    }
}
