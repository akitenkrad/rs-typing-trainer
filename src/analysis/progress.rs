//! Chronological progress estimates and learning-history diagnostics.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::analysis::metrics::AnalysisSettings;
use crate::analysis::model::{
    ExpectedPrompt, MODEL_VERSION, ModelView, PlayerModel, SufficientStatistics, TransitionEstimate, TransitionStatistic,
    append_sufficient_statistics, build_player_model, build_player_model_from_statistics, build_player_model_from_statistics_with_layout,
    build_sufficient_statistics, expected_performance, session_language,
};
use crate::bench::{BenchPart, is_preferred_bench_series, parse_bench_id, preferred_bench_series};
use crate::engine::romaji::RomajiJudge;
use crate::engine::session::KeystrokeKind;
use crate::layout::KeyboardLayout;
use crate::layout::geometry::KeyboardGeometry;
use crate::record::SessionRecord;

pub const OPERATING_CAUTIOUS_ACCURACY: f64 = 0.985;
pub const OPERATING_AGGRESSIVE_ACCURACY: f64 = 0.94;
pub const CHANGE_HAZARD: f64 = 1.0 / 30.0;
pub const REGRESSION_RATIO: f64 = 1.15;
pub const MIN_FORM_PRIOR_KEYSTROKES: usize = 300;
pub const MIN_FORM_COVERAGE: f64 = 0.5;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReferencePrompts {
    pub english: Vec<ExpectedPrompt>,
    pub japanese: Vec<ExpectedPrompt>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProgressOptions {
    pub model: AnalysisSettings,
    pub min_transition_n: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionForm {
    pub session_id: Option<i64>,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub mode: String,
    pub language: String,
    pub net_wpm: f64,
    pub expected_wpm: f64,
    pub form: f64,
    pub corrected_wpm: f64,
    #[serde(default)]
    pub coverage: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DailyPoint {
    pub date: NaiveDate,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DailyTrend {
    pub target: String,
    #[serde(default)]
    pub source: DailyTrendSource,
    pub speed: Vec<DailyPoint>,
    pub accuracy: Vec<DailyPoint>,
    pub speed_slope_per_week: Option<f64>,
    pub accuracy_slope_per_week: Option<f64>,
    pub retest_correlation: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DailyTrendSource {
    Bench,
    #[default]
    CorrectedWpm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OperatingPointKind {
    TooCautious,
    TooAggressive,
    Balanced,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OperatingPoint {
    pub slope_accuracy_per_wpm: Option<f64>,
    pub latest_wpm: f64,
    pub latest_accuracy: f64,
    pub median_wpm: f64,
    pub kind: OperatingPointKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CurveKind {
    Exponential,
    Power,
}

impl CurveKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exponential => "exponential",
            Self::Power => "power",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LearningCurve {
    pub target: String,
    pub model: CurveKind,
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub aicc: f64,
    pub n: usize,
    pub forecast_days: u32,
    pub forecast: f64,
    pub band_lo: f64,
    pub band_hi: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChangePointKind {
    PlateauStart,
    SuddenGain,
    Drop,
}

impl ChangePointKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PlateauStart => "plateau-start",
            Self::SuddenGain => "sudden-gain",
            Self::Drop => "drop",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChangePoint {
    pub target: String,
    pub date: NaiveDate,
    pub probability: f64,
    pub kind: ChangePointKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransitionRegression {
    pub from: String,
    pub to: String,
    pub best_ms: f64,
    pub recent_ms: f64,
    pub ratio: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub forms: Vec<SessionForm>,
    pub trends: Vec<DailyTrend>,
    pub operating_point: Option<OperatingPoint>,
    pub curves: Vec<LearningCurve>,
    pub changepoints: Vec<ChangePoint>,
    pub regressions: Vec<TransitionRegression>,
}

#[allow(clippy::too_many_arguments)]
pub fn analyze_progress(
    records: &[SessionRecord],
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
    options: ProgressOptions,
    as_of: DateTime<Utc>,
    references: &ReferencePrompts,
) -> Progress {
    let mut ordered = records
        .iter()
        .filter(|record| record.header.metrics_version > 0)
        .filter_map(|record| parse_time(&record.header.started_at).map(|started| (started, record)))
        .collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.header.id.cmp(&right.1.header.id)));
    let initial = ordered.first().map_or(as_of, |row| row.0);
    let mut statistics = build_sufficient_statistics(&[], layout, geometry, options.model, ModelView::Recent, initial);
    let mut auxiliary = RecentAuxiliary::new(initial);
    let mut forms = Vec::new();
    let mut prior_keystrokes = 0_usize;
    for (started_at, record) in ordered {
        let finished_at = parse_time(&record.header.finished_at).unwrap_or(started_at);
        auxiliary.advance(started_at, options.model.half_life_days);
        let mut model = build_player_model_from_statistics_with_layout(&statistics, layout, geometry, options.model, started_at);
        auxiliary.apply(&mut model);
        let language = session_language(record);
        let prompts = record_prompts(record);
        let reference = if language == "ja" { &references.japanese } else { &references.english };
        if let (Some(expected), Some(reference)) = (
            aggregate_expected(&model, &prompts, options.min_transition_n),
            aggregate_expected(&model, reference, options.min_transition_n),
        ) && prior_keystrokes >= MIN_FORM_PRIOR_KEYSTROKES
            && expected.coverage >= MIN_FORM_COVERAGE
            && expected.wpm > 0.0
            && reference.wpm > 0.0
        {
            let form = record.header.net_wpm / expected.wpm;
            forms.push(SessionForm {
                session_id: record.header.id,
                started_at,
                finished_at,
                mode: record.header.mode.clone(),
                language: language.to_owned(),
                net_wpm: record.header.net_wpm,
                expected_wpm: expected.wpm,
                form,
                corrected_wpm: form * reference.wpm,
                coverage: expected.coverage,
            });
        }
        append_sufficient_statistics(
            &mut statistics,
            std::slice::from_ref(record),
            layout,
            geometry,
            options.model,
            finished_at,
        );
        auxiliary.add_record(record, finished_at, options.model);
        prior_keystrokes += record.log.keystrokes.iter().filter(|key| key.kind == KeystrokeKind::Char).count();
    }

    let trends = ["en", "ja"]
        .into_iter()
        .filter_map(|target| daily_trend(target, records, &forms, as_of.date_naive()))
        .collect::<Vec<_>>();
    let operating_samples = recent_operating_samples(records, &forms, as_of.date_naive());
    let operating_point = classify_operating_point(&operating_samples);
    let mut curves = Vec::new();
    let mut changepoints = Vec::new();
    for trend in &trends {
        let points = cumulative_practice_points(&trend.speed, records, &trend.target);
        let daily_minutes = recent_daily_minutes(records, &trend.target, as_of.date_naive());
        if let Some(curve) = fit_learning_curve(&trend.target, &points, daily_minutes) {
            curves.push(curve);
        }
        changepoints.extend(detect_changepoints(
            &trend.target,
            &trend.speed.iter().map(|point| (point.date, point.value)).collect::<Vec<_>>(),
        ));
    }
    let regressions = transition_regressions_from_records(records, layout, geometry, options, as_of);
    Progress {
        forms,
        trends,
        operating_point,
        curves,
        changepoints,
        regressions,
    }
}

#[derive(Clone, Debug)]
struct RecentAuxiliary {
    at: DateTime<Utc>,
    scale: f64,
    intervals: BTreeMap<u64, f64>,
    reactions: BTreeMap<u64, f64>,
    word_starts: BTreeMap<u64, f64>,
    word_internal: BTreeMap<u64, f64>,
    spellings: BTreeMap<String, BTreeMap<String, f64>>,
}

impl RecentAuxiliary {
    fn new(at: DateTime<Utc>) -> Self {
        Self {
            at,
            scale: 1.0,
            intervals: BTreeMap::new(),
            reactions: BTreeMap::new(),
            word_starts: BTreeMap::new(),
            word_internal: BTreeMap::new(),
            spellings: BTreeMap::new(),
        }
    }

    fn advance(&mut self, at: DateTime<Utc>, half_life_days: f64) {
        if at <= self.at || half_life_days <= 0.0 {
            self.at = self.at.max(at);
            return;
        }
        let days = (at - self.at).num_milliseconds() as f64 / 86_400_000.0;
        self.scale *= 0.5_f64.powf(days / half_life_days);
        self.at = at;
        if self.scale < 1e-100 {
            Self::rescale_map(&mut self.intervals, self.scale);
            Self::rescale_map(&mut self.reactions, self.scale);
            Self::rescale_map(&mut self.word_starts, self.scale);
            Self::rescale_map(&mut self.word_internal, self.scale);
            for variants in self.spellings.values_mut() {
                for weight in variants.values_mut() {
                    *weight *= self.scale;
                }
            }
            self.scale = 1.0;
        }
    }

    fn rescale_map(values: &mut BTreeMap<u64, f64>, factor: f64) {
        for weight in values.values_mut() {
            *weight *= factor;
        }
    }

    fn add_record(&mut self, record: &SessionRecord, at: DateTime<Utc>, settings: AnalysisSettings) {
        self.advance(at, settings.half_life_days);
        let stored_weight = 1.0 / self.scale;
        let pause_us = settings.pause_ms.saturating_mul(1_000);
        let char_keys = record
            .log
            .keystrokes
            .iter()
            .filter(|key| key.kind == KeystrokeKind::Char)
            .collect::<Vec<_>>();
        for item in &record.log.items {
            let keys = char_keys.iter().copied().filter(|key| key.item_idx == item.idx).collect::<Vec<_>>();
            if let Some(first) = keys.first() {
                Self::add_sample(&mut self.reactions, first.t_us.saturating_sub(item.shown_us), stored_weight);
            }
            for pair in keys.windows(2) {
                let delta_us = pair[1].t_us.saturating_sub(pair[0].t_us);
                if !pair[0].correct || !pair[1].correct || delta_us == 0 || delta_us > pause_us {
                    continue;
                }
                Self::add_sample(&mut self.intervals, delta_us, stored_weight);
                let previous = pair[0].expected.as_deref().and_then(|value| value.chars().next());
                let current = pair[1].expected.as_deref().and_then(|value| value.chars().next());
                if previous.is_some_and(char::is_whitespace) && current.is_some_and(|value| !value.is_whitespace()) {
                    Self::add_sample(&mut self.word_starts, delta_us, stored_weight);
                } else if previous.is_some_and(|value| !value.is_whitespace()) && current.is_some_and(|value| !value.is_whitespace()) {
                    Self::add_sample(&mut self.word_internal, delta_us, stored_weight);
                }
            }
            if let Some(reading) = item.reading.as_ref()
                && !keys.is_empty()
                && keys.iter().all(|key| key.correct)
            {
                let spelling = keys.iter().filter_map(|key| key.typed.as_deref()).collect::<String>();
                if let Some(segments) = RomajiJudge::new(reading).spelling_segments(&spelling) {
                    for (mora, variant) in segments {
                        *self.spellings.entry(mora).or_default().entry(variant).or_default() += stored_weight;
                    }
                }
            }
        }
    }

    fn add_sample(values: &mut BTreeMap<u64, f64>, microseconds: u64, weight: f64) {
        *values.entry(microseconds).or_default() += weight;
    }

    fn apply(&self, model: &mut PlayerModel) {
        if let Some(interval) = weighted_map_median(&self.intervals) {
            model.personal_iki_median_ms = interval / 1_000.0;
            for transition in &mut model.transitions {
                transition.relative_iki = transition.estimated_iki_ms / model.personal_iki_median_ms;
            }
        }
        model.reaction_median_ms = weighted_map_median(&self.reactions).unwrap_or_default() / 1_000.0;
        model.word_start_ratio = weighted_map_median(&self.word_starts)
            .zip(weighted_map_median(&self.word_internal))
            .and_then(|(start, internal)| (internal > 0.0).then_some(start / internal))
            .unwrap_or(1.0);
        model.spelling_preferences = self
            .spellings
            .iter()
            .filter_map(|(mora, variants)| {
                variants
                    .iter()
                    .max_by(|left, right| left.1.total_cmp(right.1).then_with(|| right.0.cmp(left.0)))
                    .map(|(variant, _)| (mora.clone(), variant.clone()))
            })
            .collect();
    }
}

fn weighted_map_median(values: &BTreeMap<u64, f64>) -> Option<f64> {
    let target = values.values().sum::<f64>() / 2.0;
    if target <= 0.0 {
        return None;
    }
    let mut cumulative = 0.0;
    for (value, weight) in values {
        cumulative += weight;
        if cumulative >= target {
            return Some(*value as f64);
        }
    }
    None
}

fn record_prompts(record: &SessionRecord) -> Vec<ExpectedPrompt> {
    record
        .log
        .items
        .iter()
        .map(|item| {
            item.reading
                .as_ref()
                .map_or_else(|| ExpectedPrompt::english(&item.text), ExpectedPrompt::japanese)
        })
        .collect()
}

#[derive(Clone, Copy, Debug)]
struct AggregateExpected {
    wpm: f64,
    coverage: f64,
}

fn aggregate_expected(model: &PlayerModel, prompts: &[ExpectedPrompt], min_n: u64) -> Option<AggregateExpected> {
    if prompts.is_empty() {
        return None;
    }
    let mut characters = 0_usize;
    let mut total_ms = 0.0;
    let mut covered_transitions = 0_usize;
    let mut transitions = 0_usize;
    for prompt in prompts {
        let result = expected_performance(model, prompt);
        let chars = result.keystrokes.chars().collect::<Vec<_>>();
        for pair in chars.windows(2) {
            transitions += 1;
            covered_transitions += usize::from(
                model
                    .transition(&pair[0].to_string(), &pair[1].to_string())
                    .is_some_and(|row| row.effective_n >= min_n as f64),
            );
        }
        characters += chars.len();
        total_ms += result.total_ms;
    }
    (characters > 0 && total_ms > 0.0).then(|| AggregateExpected {
        wpm: characters as f64 / 5.0 / (total_ms / 60_000.0),
        coverage: if transitions == 0 {
            1.0
        } else {
            covered_transitions as f64 / transitions as f64
        },
    })
}

fn daily_trend(target: &str, records: &[SessionRecord], forms: &[SessionForm], as_of: NaiveDate) -> Option<DailyTrend> {
    let bench_part = if target == "ja" { BenchPart::C } else { BenchPart::A };
    let bench_series = preferred_bench_series(records.iter().filter_map(|record| record.header.bench_id.as_deref()), bench_part);
    let mut by_day = BTreeMap::<NaiveDate, Vec<&SessionRecord>>::new();
    for record in records.iter().filter(|record| session_language(record) == target) {
        if let Some(date) = parse_time(&record.header.started_at).map(|time| time.date_naive()) {
            by_day.entry(date).or_default().push(record);
        }
    }
    let mut speed = Vec::new();
    let mut accuracy = Vec::new();
    let mut benches = Vec::new();
    for (date, day_records) in by_day {
        let bench = day_records
            .iter()
            .filter(|record| {
                record
                    .header
                    .bench_id
                    .as_deref()
                    .and_then(parse_bench_id)
                    .is_some_and(|(series, part, _)| part == bench_part && is_preferred_bench_series(series, bench_part, bench_series))
            })
            .copied()
            .collect::<Vec<_>>();
        let values = if bench_series.is_some() {
            benches.extend(bench.iter().map(|record| record.header.net_wpm));
            bench.iter().map(|record| record.header.net_wpm).collect::<Vec<_>>()
        } else {
            forms
                .iter()
                .filter(|form| form.language == target && form.started_at.date_naive() == date)
                .map(|form| form.corrected_wpm)
                .collect::<Vec<_>>()
        };
        if !values.is_empty() {
            speed.push(DailyPoint {
                date,
                value: median(&values).unwrap_or_default(),
            });
        }
        let accuracies = if bench_series.is_some() {
            bench.iter().map(|record| record.header.accuracy).collect::<Vec<_>>()
        } else {
            day_records.iter().map(|record| record.header.accuracy).collect::<Vec<_>>()
        };
        if !accuracies.is_empty() {
            accuracy.push(DailyPoint {
                date,
                value: median(&accuracies).unwrap_or_default(),
            });
        }
    }
    if speed.is_empty() && accuracy.is_empty() {
        return None;
    }
    let speed_slope_per_week = four_week_slope(&speed, as_of);
    let accuracy_slope_per_week = four_week_slope(&accuracy, as_of);
    let retest_correlation = (benches.len() >= 6).then(|| serial_correlation(&benches)).flatten();
    Some(DailyTrend {
        target: target.to_owned(),
        source: if bench_series.is_some() {
            DailyTrendSource::Bench
        } else {
            DailyTrendSource::CorrectedWpm
        },
        speed,
        accuracy,
        speed_slope_per_week,
        accuracy_slope_per_week,
        retest_correlation,
    })
}

fn four_week_slope(points: &[DailyPoint], as_of: NaiveDate) -> Option<f64> {
    let start = as_of - Duration::days(27);
    let selected = points
        .iter()
        .filter(|point| (start..=as_of).contains(&point.date))
        .map(|point| ((point.date - start).num_days() as f64, point.value))
        .collect::<Vec<_>>();
    let weeks = points
        .iter()
        .filter(|point| (start..=as_of).contains(&point.date))
        .map(|point| {
            let week = point.date.iso_week();
            (week.year(), week.week())
        })
        .collect::<std::collections::BTreeSet<_>>();
    if weeks.len() < 2 {
        return None;
    }
    linear_slope(&selected).map(|slope| slope * 7.0)
}

fn recent_operating_samples(records: &[SessionRecord], forms: &[SessionForm], as_of: NaiveDate) -> Vec<(NaiveDate, f64, f64)> {
    let accuracy = records
        .iter()
        .filter_map(|record| {
            Some((
                record.header.id?,
                (parse_time(&record.header.started_at)?.date_naive(), record.header.accuracy),
            ))
        })
        .collect::<HashMap<_, _>>();
    forms
        .iter()
        .filter_map(|form| {
            let id = form.session_id?;
            let (date, accuracy) = accuracy.get(&id).copied()?;
            ((as_of - date).num_days() <= 27).then_some((date, form.corrected_wpm, accuracy))
        })
        .collect()
}

pub fn classify_operating_point(points: &[(NaiveDate, f64, f64)]) -> Option<OperatingPoint> {
    let latest = points.iter().max_by_key(|point| point.0)?;
    let wpms = points.iter().map(|point| point.1).collect::<Vec<_>>();
    let median_wpm = median(&wpms)?;
    let regression = points.iter().map(|point| (point.1, point.2)).collect::<Vec<_>>();
    let kind = if latest.2 < OPERATING_AGGRESSIVE_ACCURACY {
        OperatingPointKind::TooAggressive
    } else if latest.2 >= OPERATING_CAUTIOUS_ACCURACY && latest.1 < median_wpm {
        OperatingPointKind::TooCautious
    } else {
        OperatingPointKind::Balanced
    };
    Some(OperatingPoint {
        slope_accuracy_per_wpm: linear_slope(&regression),
        latest_wpm: latest.1,
        latest_accuracy: latest.2,
        median_wpm,
        kind,
    })
}

#[derive(Clone, Copy)]
struct Fit {
    kind: CurveKind,
    a: f64,
    b: f64,
    c: f64,
    sse: f64,
}

pub fn fit_learning_curve(target: &str, points: &[(f64, f64)], daily_minutes: f64) -> Option<LearningCurve> {
    if points.len() < 6 || points.iter().any(|point| !point.0.is_finite() || !point.1.is_finite()) {
        return None;
    }
    let exponential = best_curve(points, CurveKind::Exponential)?;
    let power = best_curve(points, CurveKind::Power)?;
    let selected = if aicc(exponential.sse, points.len(), 3) <= aicc(power.sse, points.len(), 3) {
        exponential
    } else {
        power
    };
    let score = aicc(selected.sse, points.len(), 3);
    let last_t = points.iter().map(|point| point.0).fold(0.0, f64::max);
    let forecast_t = last_t + daily_minutes.max(0.0) * 28.0;
    let forecast = curve_value(selected, forecast_t);
    let residual_sd = (selected.sse / (points.len() - 3) as f64).sqrt();
    Some(LearningCurve {
        target: target.to_owned(),
        model: selected.kind,
        a: selected.a,
        b: selected.b,
        c: selected.c,
        aicc: score,
        n: points.len(),
        forecast_days: 28,
        forecast,
        band_lo: forecast - 1.96 * residual_sd,
        band_hi: forecast + 1.96 * residual_sd,
    })
}

fn best_curve(points: &[(f64, f64)], kind: CurveKind) -> Option<Fit> {
    let mut best: Option<Fit> = None;
    for index in 0..800 {
        let log_c = -6.9 + index as f64 / 799.0 * 7.6;
        let c = log_c.exp();
        let transformed = points
            .iter()
            .map(|(time, value)| {
                let x = match kind {
                    CurveKind::Exponential => (-c * time).exp(),
                    CurveKind::Power => (time + 1.0).powf(-c),
                };
                (x, *value)
            })
            .collect::<Vec<_>>();
        let (intercept, slope) = linear_fit(&transformed)?;
        let fit = Fit {
            kind,
            a: intercept,
            b: -slope,
            c,
            sse: transformed.iter().map(|(x, y)| (y - (intercept + slope * x)).powi(2)).sum(),
        };
        if fit.b >= 0.0 && best.is_none_or(|current| fit.sse < current.sse) {
            best = Some(fit);
        }
    }
    best.map(|fit| refine_curve(points, fit))
}

fn refine_curve(points: &[(f64, f64)], mut fit: Fit) -> Fit {
    let mut lambda = 1e-3;
    for _ in 0..50 {
        let mut normal = [[0.0; 3]; 3];
        let mut right = [0.0; 3];
        for (time, observed) in points {
            let x = match fit.kind {
                CurveKind::Exponential => (-fit.c * time).exp(),
                CurveKind::Power => (time + 1.0).powf(-fit.c),
            };
            let predicted = fit.a - fit.b * x;
            let derivative_c = match fit.kind {
                CurveKind::Exponential => fit.b * time * x,
                CurveKind::Power => fit.b * (time + 1.0).ln() * x,
            };
            let jacobian = [1.0, -x, derivative_c];
            let residual = observed - predicted;
            for row in 0..3 {
                right[row] += jacobian[row] * residual;
                for column in 0..3 {
                    normal[row][column] += jacobian[row] * jacobian[column];
                }
            }
        }
        for (index, row) in normal.iter_mut().enumerate() {
            row[index] *= 1.0 + lambda;
        }
        let Some(delta) = solve_linear_3(normal, right) else { break };
        let candidate = Fit {
            kind: fit.kind,
            a: fit.a + delta[0],
            b: fit.b + delta[1],
            c: fit.c + delta[2],
            sse: 0.0,
        };
        if candidate.b <= 0.0 || candidate.c <= 0.0 {
            lambda *= 10.0;
            continue;
        }
        let candidate_sse = points
            .iter()
            .map(|(time, observed)| (observed - curve_value(candidate, *time)).powi(2))
            .sum::<f64>();
        if candidate_sse < fit.sse {
            fit = Fit {
                sse: candidate_sse,
                ..candidate
            };
            lambda = (lambda / 3.0).max(1e-12);
            if delta.iter().all(|value| value.abs() < 1e-10) {
                break;
            }
        } else {
            lambda *= 10.0;
        }
    }
    fit
}

fn solve_linear_3(mut matrix: [[f64; 3]; 3], mut right: [f64; 3]) -> Option<[f64; 3]> {
    for pivot in 0..3 {
        let best = (pivot..3).max_by(|left, right_index| matrix[*left][pivot].abs().total_cmp(&matrix[*right_index][pivot].abs()))?;
        matrix.swap(pivot, best);
        right.swap(pivot, best);
        let divisor = matrix[pivot][pivot];
        if divisor.abs() < 1e-15 {
            return None;
        }
        for value in &mut matrix[pivot][pivot..] {
            *value /= divisor;
        }
        right[pivot] /= divisor;
        let pivot_row = matrix[pivot];
        let pivot_right = right[pivot];
        for (row_index, row) in matrix.iter_mut().enumerate() {
            if row_index == pivot {
                continue;
            }
            let factor = row[pivot];
            for (value, pivot_value) in row[pivot..].iter_mut().zip(&pivot_row[pivot..]) {
                *value -= factor * pivot_value;
            }
            right[row_index] -= factor * pivot_right;
        }
    }
    Some(right)
}

fn curve_value(fit: Fit, time: f64) -> f64 {
    match fit.kind {
        CurveKind::Exponential => fit.a - fit.b * (-fit.c * time).exp(),
        CurveKind::Power => fit.a - fit.b * (time + 1.0).powf(-fit.c),
    }
}

fn aicc(sse: f64, n: usize, parameters: usize) -> f64 {
    let safe_sse = sse.max(f64::EPSILON);
    n as f64 * (safe_sse / n as f64).ln() + 2.0 * parameters as f64 + 2.0 * parameters as f64 * (parameters + 1) as f64 / (n - parameters - 1) as f64
}

/// Online normal change detector with the Adams--MacKay constant hazard.
/// The variance is estimated from adjacent differences; the unknown run mean
/// has a weak normal prior and is updated for every possible run length.
pub fn detect_changepoints(target: &str, points: &[(NaiveDate, f64)]) -> Vec<ChangePoint> {
    if points.len() < 12 {
        return Vec::new();
    }
    let differences = points.windows(2).map(|pair| (pair[1].1 - pair[0].1).powi(2) / 2.0).collect::<Vec<_>>();
    let variance = median(&differences).unwrap_or(1.0).max(0.25);
    let prior_mean = points[0].1;
    let prior_strength = 0.01;
    let mut probabilities = vec![1.0];
    let mut states = vec![(0_usize, prior_mean)];
    let mut candidates = Vec::new();
    for (index, (_, value)) in points.iter().enumerate() {
        let prior_predictive = normal_density(*value, prior_mean, variance * (1.0 + 1.0 / prior_strength));
        let mut next_probabilities = vec![0.0; probabilities.len() + 1];
        next_probabilities[0] = probabilities.iter().sum::<f64>() * CHANGE_HAZARD * prior_predictive;
        for (run, ((n, mean), probability)) in states.iter().zip(&probabilities).enumerate() {
            let strength = prior_strength + *n as f64;
            let predictive = normal_density(*value, *mean, variance * (1.0 + 1.0 / strength));
            next_probabilities[run + 1] = probability * (1.0 - CHANGE_HAZARD) * predictive;
        }
        let total = next_probabilities.iter().sum::<f64>();
        if total <= 0.0 || !total.is_finite() {
            probabilities = vec![1.0];
            states = vec![(1, *value)];
            continue;
        }
        for probability in &mut next_probabilities {
            *probability /= total;
        }
        let change_probability = next_probabilities[0];
        let mut next_states = Vec::with_capacity(states.len() + 1);
        next_states.push((1, (prior_strength * prior_mean + value) / (prior_strength + 1.0)));
        next_states.extend(states.iter().map(|(n, mean)| {
            let strength = prior_strength + *n as f64;
            (n + 1, (strength * mean + value) / (strength + 1.0))
        }));
        probabilities = next_probabilities;
        states = next_states;
        if index >= 5 && index + 5 < points.len() && change_probability > 0.5 {
            candidates.push((index, change_probability));
        }
    }
    let mut output = Vec::new();
    let mut cursor = 0;
    while cursor < candidates.len() {
        let mut best = candidates[cursor];
        cursor += 1;
        let mut last_split = best.0;
        while cursor < candidates.len() && candidates[cursor].0 <= last_split + 1 {
            last_split = candidates[cursor].0;
            if candidates[cursor].1 > best.1 {
                best = candidates[cursor];
            }
            cursor += 1;
        }
        let (split, probability) = best;
        let before_start = split.saturating_sub(10);
        let after_end = (split + 10).min(points.len());
        let before = points[before_start..split].iter().map(|point| point.1).sum::<f64>() / (split - before_start) as f64;
        let after = points[split..after_end].iter().map(|point| point.1).sum::<f64>() / (after_end - split) as f64;
        let after_points = &points[split..];
        let after_slope = linear_slope(
            &after_points
                .iter()
                .enumerate()
                .map(|(index, point)| (index as f64, point.1))
                .collect::<Vec<_>>(),
        )
        .unwrap_or_default();
        let kind = if after - before >= 3.0 {
            ChangePointKind::SuddenGain
        } else if after_points.len() >= 10 && after_slope.abs() < 0.2 {
            ChangePointKind::PlateauStart
        } else {
            ChangePointKind::Drop
        };
        output.push(ChangePoint {
            target: target.to_owned(),
            date: points[split].0,
            probability,
            kind,
        });
    }
    output
}

fn normal_density(value: f64, mean: f64, variance: f64) -> f64 {
    let variance = variance.max(f64::EPSILON);
    (-(value - mean).powi(2) / (2.0 * variance)).exp() / (2.0 * std::f64::consts::PI * variance).sqrt()
}

pub fn find_transition_regressions(best: &[TransitionEstimate], recent: &[TransitionEstimate], min_transition_n: u64) -> Vec<TransitionRegression> {
    let best = best
        .iter()
        .filter(|row| row.effective_n >= min_transition_n as f64)
        .map(|row| ((row.from.as_str(), row.to.as_str()), row.estimated_iki_ms))
        .collect::<HashMap<_, _>>();
    let mut rows = recent
        .iter()
        .filter(|row| row.effective_n >= min_transition_n as f64)
        .filter_map(|row| {
            let best_ms = *best.get(&(row.from.as_str(), row.to.as_str()))?;
            let ratio = row.estimated_iki_ms / best_ms;
            (ratio >= REGRESSION_RATIO).then(|| TransitionRegression {
                from: row.from.clone(),
                to: row.to.clone(),
                best_ms,
                recent_ms: row.estimated_iki_ms,
                ratio,
            })
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| right.ratio.total_cmp(&left.ratio));
    rows
}

fn transition_regressions_from_records(
    records: &[SessionRecord],
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
    options: ProgressOptions,
    as_of: DateTime<Utc>,
) -> Vec<TransitionRegression> {
    let recent = build_player_model(records, layout, geometry, options.model, ModelView::Recent, as_of);
    let mut best = HashMap::<(String, String), TransitionEstimate>::new();
    let mut records_by_day = BTreeMap::<NaiveDate, Vec<&SessionRecord>>::new();
    for record in records {
        if let Some(date) = parse_time(&record.header.finished_at).map(|time| time.date_naive()) {
            records_by_day.entry(date).or_default().push(record);
        }
    }
    let daily = records_by_day
        .into_iter()
        .map(|(date, records)| {
            let at = date.and_hms_opt(23, 59, 59).expect("valid day").and_utc();
            let records = records.into_iter().cloned().collect::<Vec<_>>();
            (
                date,
                build_sufficient_statistics(&records, layout, geometry, options.model, ModelView::AllTime, at),
            )
        })
        .collect::<Vec<_>>();
    for (end, _) in &daily {
        let start = *end - Duration::days(13);
        let window = daily
            .iter()
            .filter(|(date, _)| (start..=*end).contains(date))
            .map(|(_, statistics)| statistics)
            .collect::<Vec<_>>();
        let at = end.and_hms_opt(23, 59, 59).expect("valid day").and_utc();
        let statistics = combine_statistics(&window, layout.name(), at);
        let model = build_player_model_from_statistics(&statistics, options.model, at);
        for row in model.transitions {
            if row.effective_n >= options.min_transition_n as f64 {
                let key = (row.from.clone(), row.to.clone());
                if best.get(&key).is_none_or(|current| row.estimated_iki_ms < current.estimated_iki_ms) {
                    best.insert(key, row);
                }
            }
        }
    }
    find_transition_regressions(&best.into_values().collect::<Vec<_>>(), &recent.transitions, options.min_transition_n)
}

fn combine_statistics(parts: &[&SufficientStatistics], layout: &str, as_of: DateTime<Utc>) -> SufficientStatistics {
    let mut combined = BTreeMap::<(String, String), TransitionStatistic>::new();
    for row in parts.iter().flat_map(|statistics| &statistics.rows) {
        let entry = combined.entry((row.from.clone(), row.to.clone())).or_insert_with(|| TransitionStatistic {
            from: row.from.clone(),
            to: row.to.clone(),
            from_hand: row.from_hand.clone(),
            to_hand: row.to_hand.clone(),
            from_finger: row.from_finger.clone(),
            to_finger: row.to_finger.clone(),
            weight: 0.0,
            log_iki_sum: 0.0,
            log_iki_sq_sum: 0.0,
            error_weight: 0.0,
            attempt_weight: 0.0,
            observations: 0,
            errors: 0,
            last_added_at: row.last_added_at,
        });
        entry.weight += row.weight;
        entry.log_iki_sum += row.log_iki_sum;
        entry.log_iki_sq_sum += row.log_iki_sq_sum;
        entry.error_weight += row.error_weight;
        entry.attempt_weight += row.attempt_weight;
        entry.observations += row.observations;
        entry.errors += row.errors;
        entry.last_added_at = entry.last_added_at.max(row.last_added_at);
    }
    SufficientStatistics {
        model_version: MODEL_VERSION,
        layout: layout.to_owned(),
        language: "all".to_owned(),
        view: ModelView::AllTime,
        half_life_days: 0.0,
        as_of,
        last_session_id: parts.iter().filter_map(|statistics| statistics.last_session_id).max(),
        rows: combined.into_values().collect(),
    }
}

fn cumulative_practice_points(points: &[DailyPoint], records: &[SessionRecord], target: &str) -> Vec<(f64, f64)> {
    let mut cumulative = 0.0;
    let mut durations = BTreeMap::<NaiveDate, f64>::new();
    for record in records.iter().filter(|record| session_language(record) == target) {
        if let Some(date) = parse_time(&record.header.started_at).map(|time| time.date_naive()) {
            *durations.entry(date).or_default() += record.header.duration_ms.max(0) as f64 / 60_000.0;
        }
    }
    points
        .iter()
        .map(|point| {
            let result = (cumulative, point.value);
            cumulative += durations.get(&point.date).copied().unwrap_or_default();
            result
        })
        .collect()
}

fn recent_daily_minutes(records: &[SessionRecord], target: &str, as_of: NaiveDate) -> f64 {
    let start = as_of - Duration::days(27);
    records
        .iter()
        .filter(|record| session_language(record) == target)
        .filter(|record| {
            parse_time(&record.header.started_at)
                .map(|time| (start..=as_of).contains(&time.date_naive()))
                .unwrap_or(false)
        })
        .map(|record| record.header.duration_ms.max(0) as f64 / 60_000.0)
        .sum::<f64>()
        / 28.0
}

fn serial_correlation(values: &[f64]) -> Option<f64> {
    pearson(&values[..values.len() - 1], &values[1..])
}

fn pearson(left: &[f64], right: &[f64]) -> Option<f64> {
    if left.len() != right.len() || left.len() < 2 {
        return None;
    }
    let left_mean = left.iter().sum::<f64>() / left.len() as f64;
    let right_mean = right.iter().sum::<f64>() / right.len() as f64;
    let covariance = left
        .iter()
        .zip(right)
        .map(|(left, right)| (left - left_mean) * (right - right_mean))
        .sum::<f64>();
    let left_ss = left.iter().map(|value| (value - left_mean).powi(2)).sum::<f64>();
    let right_ss = right.iter().map(|value| (value - right_mean).powi(2)).sum::<f64>();
    (left_ss > 0.0 && right_ss > 0.0).then(|| covariance / (left_ss * right_ss).sqrt())
}

fn linear_fit(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    let slope = linear_slope(points)?;
    let x_mean = points.iter().map(|point| point.0).sum::<f64>() / points.len() as f64;
    let y_mean = points.iter().map(|point| point.1).sum::<f64>() / points.len() as f64;
    Some((y_mean - slope * x_mean, slope))
}

fn linear_slope(points: &[(f64, f64)]) -> Option<f64> {
    if points.len() < 2 {
        return None;
    }
    let x_mean = points.iter().map(|point| point.0).sum::<f64>() / points.len() as f64;
    let y_mean = points.iter().map(|point| point.1).sum::<f64>() / points.len() as f64;
    let denominator = points.iter().map(|point| (point.0 - x_mean).powi(2)).sum::<f64>();
    (denominator > 0.0).then(|| points.iter().map(|point| (point.0 - x_mean) * (point.1 - y_mean)).sum::<f64>() / denominator)
}

fn median(values: &[f64]) -> Option<f64> {
    let mut values = values.iter().copied().filter(|value| value.is_finite()).collect::<Vec<_>>();
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    Some(if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    })
}

fn parse_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value).ok().map(|time| time.with_timezone(&Utc))
}
