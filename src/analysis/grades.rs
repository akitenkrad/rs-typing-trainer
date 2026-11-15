//! Absolute S-to-G skill grades built from benchmark and practice records.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::analysis::errors::diagnose_errors;
use crate::analysis::metrics::AnalysisSettings;
use crate::analysis::model::{ModelView, PlayerModel, build_player_model, session_language};
use crate::analysis::progress::SessionForm;
use crate::analysis::style::diagnose_style;
use crate::data::JapaneseText;
use crate::engine::romaji::RomajiJudge;
use crate::layout::KeyboardLayout;
use crate::layout::geometry::KeyboardGeometry;
use crate::record::SessionRecord;

pub const GRADE_VERSION: &str = "grades-v1";
const GRADE_DATA: &str = include_str!("../../data/grades/grades-v1.toml");
const COMPONENTS: [Component; 7] = [
    Component::Speed,
    Component::Accuracy,
    Component::Rhythm,
    Component::Transition,
    Component::Preview,
    Component::Recovery,
    Component::Overlap,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Component {
    Speed,
    Accuracy,
    Rhythm,
    Transition,
    Preview,
    Recovery,
    Overlap,
}

impl Component {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Speed => "speed",
            Self::Accuracy => "accuracy",
            Self::Rhythm => "rhythm",
            Self::Transition => "transition",
            Self::Preview => "preview",
            Self::Recovery => "recovery",
            Self::Overlap => "overlap",
        }
    }

    pub fn all() -> &'static [Self] {
        &COMPONENTS
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Higher,
    Lower,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Level {
    G,
    F,
    E,
    D,
    C,
    B,
    A,
    S,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::G => "G",
            Self::F => "F",
            Self::E => "E",
            Self::D => "D",
            Self::C => "C",
            Self::B => "B",
            Self::A => "A",
            Self::S => "S",
        }
    }

    fn from_score(score: f64) -> Self {
        match score.floor().clamp(0.0, 7.0) as u8 {
            0 => Self::G,
            1 => Self::F,
            2 => Self::E,
            3 => Self::D,
            4 => Self::C,
            5 => Self::B,
            6 => Self::A,
            _ => Self::S,
        }
    }

    fn next(self) -> Option<Self> {
        match self {
            Self::G => Some(Self::F),
            Self::F => Some(Self::E),
            Self::E => Some(Self::D),
            Self::D => Some(Self::C),
            Self::C => Some(Self::B),
            Self::B => Some(Self::A),
            Self::A => Some(Self::S),
            Self::S => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GradeSource {
    Bench,
    Provisional,
    ProvisionalUncorrected,
}

impl GradeSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bench => "bench",
            Self::Provisional => "provisional",
            Self::ProvisionalUncorrected => "provisional-uncorrected",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GradeStatus {
    Official,
    Interim,
    Provisional,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GradeChangeKind {
    Promotion,
    Demotion,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ComponentValue {
    pub component: Component,
    pub value: f64,
}

impl ComponentValue {
    pub fn new(component: Component, value: f64) -> Self {
        Self { component, value }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ComponentGrade {
    pub component: Component,
    pub value: f64,
    pub unit: String,
    pub grade: Level,
    pub score: f64,
    pub next_grade: Option<Level>,
    pub next_threshold: Option<f64>,
    pub gap: Option<f64>,
    pub weight: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OverallGrade {
    pub grade: Level,
    pub score: f64,
    pub easiest_component: Option<Component>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LanguageGrade {
    pub language: String,
    pub source: GradeSource,
    pub status: GradeStatus,
    pub n_sessions: usize,
    pub components: Vec<ComponentGrade>,
    pub overall: Option<OverallGrade>,
    pub missing: Vec<Component>,
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradeHistoryPoint {
    pub date: NaiveDate,
    pub language: String,
    pub component: String,
    pub grade: Level,
    pub score: f64,
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradeChange {
    pub language: String,
    pub component: String,
    pub from: Level,
    pub to: Level,
    pub kind: GradeChangeKind,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GradesAnalysis {
    pub languages: Vec<LanguageGrade>,
    pub history: Vec<GradeHistoryPoint>,
    pub changes: Vec<GradeChange>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradeOptions {
    pub model: AnalysisSettings,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GradeThresholds {
    pub version: String,
    languages: BTreeMap<String, BTreeMap<String, ComponentThreshold>>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ComponentThreshold {
    pub metric: String,
    pub direction: Direction,
    pub unit: String,
    pub weight: f64,
    pub score_zero: f64,
    pub boundaries: Vec<f64>,
    pub score_eight: f64,
}

impl GradeThresholds {
    pub fn language(&self, language: &str) -> Option<&BTreeMap<String, ComponentThreshold>> {
        self.languages.get(language)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != GRADE_VERSION {
            return Err(format!("grade version must be {GRADE_VERSION}"));
        }
        for language in ["en", "ja"] {
            let table = self.language(language).ok_or_else(|| format!("missing language {language}"))?;
            for component in COMPONENTS {
                let row = table
                    .get(component.as_str())
                    .ok_or_else(|| format!("missing {language}.{}", component.as_str()))?;
                if row.boundaries.len() != 7 || row.weight <= 0.0 || !row.weight.is_finite() {
                    return Err(format!("invalid {language}.{} metadata", component.as_str()));
                }
                let values = std::iter::once(row.score_zero)
                    .chain(row.boundaries.iter().copied())
                    .chain(std::iter::once(row.score_eight))
                    .collect::<Vec<_>>();
                let monotone = values.windows(2).all(|pair| match row.direction {
                    Direction::Higher => pair[0] < pair[1],
                    Direction::Lower => pair[0] > pair[1],
                });
                if !monotone || values.iter().any(|value| !value.is_finite()) {
                    return Err(format!("non-monotone {language}.{} thresholds", component.as_str()));
                }
            }
            if table.len() != COMPONENTS.len() {
                return Err(format!("unexpected components for {language}"));
            }
        }
        Ok(())
    }

    fn component(&self, language: &str, component: Component) -> Result<&ComponentThreshold, String> {
        self.language(language)
            .and_then(|table| table.get(component.as_str()))
            .ok_or_else(|| format!("unknown grade component {language}.{}", component.as_str()))
    }
}

pub fn bundled_thresholds() -> Result<&'static GradeThresholds, String> {
    static THRESHOLDS: OnceLock<Result<GradeThresholds, String>> = OnceLock::new();
    THRESHOLDS
        .get_or_init(|| {
            let parsed = toml::from_str::<GradeThresholds>(GRADE_DATA).map_err(|error| error.to_string())?;
            parsed.validate()?;
            Ok(parsed)
        })
        .as_ref()
        .map_err(Clone::clone)
}

pub fn grade_component(language: &str, component: Component, value: f64) -> Result<ComponentGrade, String> {
    if !value.is_finite() {
        return Err("grade value must be finite".to_owned());
    }
    let row = bundled_thresholds()?.component(language, component)?;
    let points = std::iter::once(row.score_zero)
        .chain(row.boundaries.iter().copied())
        .chain(std::iter::once(row.score_eight))
        .collect::<Vec<_>>();
    let score = interpolate(value, &points, row.direction);
    let grade = Level::from_score(score);
    let next_grade = grade.next();
    let next_threshold = next_grade.map(|_| row.boundaries[grade_index(grade)]);
    Ok(ComponentGrade {
        component,
        value,
        unit: row.unit.clone(),
        grade,
        score,
        next_grade,
        next_threshold,
        gap: next_threshold.map(|threshold| threshold - value),
        weight: row.weight,
    })
}

fn interpolate(value: f64, points: &[f64], direction: Direction) -> f64 {
    let before_start = match direction {
        Direction::Higher => value <= points[0],
        Direction::Lower => value >= points[0],
    };
    if before_start {
        return 0.0;
    }
    let beyond_end = match direction {
        Direction::Higher => value >= points[8],
        Direction::Lower => value <= points[8],
    };
    if beyond_end {
        return 8.0;
    }
    for (index, pair) in points.windows(2).enumerate() {
        let inside = match direction {
            Direction::Higher => value <= pair[1],
            Direction::Lower => value >= pair[1],
        };
        if inside {
            return index as f64 + (value - pair[0]) / (pair[1] - pair[0]);
        }
    }
    8.0
}

fn grade_index(level: Level) -> usize {
    match level {
        Level::G => 0,
        Level::F => 1,
        Level::E => 2,
        Level::D => 3,
        Level::C => 4,
        Level::B => 5,
        Level::A => 6,
        Level::S => 7,
    }
}

pub fn evaluate_language(
    language: &str,
    values: &[ComponentValue],
    source: GradeSource,
    status: GradeStatus,
    n_sessions: usize,
) -> Result<LanguageGrade, String> {
    let mut components = values
        .iter()
        .map(|value| grade_component(language, value.component, value.value))
        .collect::<Result<Vec<_>, _>>()?;
    components.sort_by_key(|row| row.component);
    let present = components.iter().map(|row| row.component).collect::<BTreeSet<_>>();
    let missing = COMPONENTS
        .into_iter()
        .filter(|component| !present.contains(component))
        .collect::<Vec<_>>();
    let has_required = present.contains(&Component::Speed) && present.contains(&Component::Accuracy);
    let overall = has_required.then(|| overall_grade(&components));
    Ok(LanguageGrade {
        language: language.to_owned(),
        source,
        status,
        n_sessions,
        components,
        overall,
        missing,
        version: GRADE_VERSION.to_owned(),
    })
}

fn overall_grade(components: &[ComponentGrade]) -> OverallGrade {
    let score = harmonic_score(components.iter().map(|row| (row.score, row.weight)));
    let easiest_component = components
        .iter()
        .filter(|row| row.grade != Level::S)
        .map(|candidate| {
            let improved = harmonic_score(components.iter().map(|row| {
                let value = if row.component == candidate.component {
                    (row.score + 1.0).min(8.0)
                } else {
                    row.score
                };
                (value, row.weight)
            }));
            (candidate.component, improved - score)
        })
        .max_by(|left, right| left.1.total_cmp(&right.1).then_with(|| right.0.cmp(&left.0)))
        .map(|row| row.0);
    OverallGrade {
        grade: Level::from_score(score),
        score,
        easiest_component,
    }
}

fn harmonic_score(values: impl Iterator<Item = (f64, f64)>) -> f64 {
    let values = values.collect::<Vec<_>>();
    let weight = values.iter().map(|row| row.1).sum::<f64>();
    weight / values.iter().map(|row| row.1 / (row.0 + 1.0)).sum::<f64>() - 1.0
}

#[allow(clippy::too_many_arguments)]
pub fn analyze_grades(
    records: &[SessionRecord],
    forms: &[SessionForm],
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
    options: GradeOptions,
    as_of: DateTime<Utc>,
    english_corpus: &[String],
    japanese_corpus: &[JapaneseText],
) -> GradesAnalysis {
    let mut languages = Vec::new();
    for language in ["en", "ja"] {
        if let Some(grade) = current_language_grade(
            language,
            records,
            forms,
            layout,
            geometry,
            options,
            as_of,
            english_corpus,
            japanese_corpus,
        ) {
            languages.push(grade);
        }
    }
    let history = build_history(records, layout, geometry, options, english_corpus, japanese_corpus);
    let changes = detect_grade_changes(&history);
    GradesAnalysis { languages, history, changes }
}

#[allow(clippy::too_many_arguments)]
fn current_language_grade(
    language: &str,
    records: &[SessionRecord],
    forms: &[SessionForm],
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
    options: GradeOptions,
    as_of: DateTime<Utc>,
    english_corpus: &[String],
    japanese_corpus: &[JapaneseText],
) -> Option<LanguageGrade> {
    let benches = benchmark_records(records, language, Some(as_of));
    if !benches.is_empty() {
        return bench_grade(language, &benches, layout, geometry, options, as_of, english_corpus, japanese_corpus);
    }
    provisional_grade(
        language,
        records,
        forms,
        layout,
        geometry,
        options,
        as_of,
        english_corpus,
        japanese_corpus,
    )
}

fn benchmark_records<'a>(records: &'a [SessionRecord], language: &str, at: Option<DateTime<Utc>>) -> Vec<&'a SessionRecord> {
    let part = if language == "ja" { "/C/" } else { "/A/" };
    let mut rows = records
        .iter()
        .filter(|record| record.header.metrics_version > 0)
        .filter(|record| record.header.bench_id.as_deref().is_some_and(|id| id.contains(part)))
        .filter(|record| at.is_none_or(|at| parse_time(&record.header.started_at).is_some_and(|started| started <= at)))
        .collect::<Vec<_>>();
    rows.sort_by_key(|record| parse_time(&record.header.started_at));
    rows
}

#[allow(clippy::too_many_arguments)]
fn bench_grade(
    language: &str,
    benches: &[&SessionRecord],
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
    options: GradeOptions,
    as_of: DateTime<Utc>,
    english_corpus: &[String],
    japanese_corpus: &[JapaneseText],
) -> Option<LanguageGrade> {
    let selected = benches.iter().rev().take(3).copied().cloned().collect::<Vec<_>>();
    let values = component_values(&selected, layout, geometry, options, as_of, english_corpus, japanese_corpus, language);
    (!values.is_empty())
        .then(|| {
            evaluate_language(
                language,
                &values,
                GradeSource::Bench,
                if selected.len() >= 3 {
                    GradeStatus::Official
                } else {
                    GradeStatus::Interim
                },
                selected.len(),
            )
            .ok()
        })
        .flatten()
}

#[allow(clippy::too_many_arguments)]
fn provisional_grade(
    language: &str,
    records: &[SessionRecord],
    forms: &[SessionForm],
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
    options: GradeOptions,
    as_of: DateTime<Utc>,
    english_corpus: &[String],
    japanese_corpus: &[JapaneseText],
) -> Option<LanguageGrade> {
    let mut language_forms = forms
        .iter()
        .filter(|form| form.language == language)
        .filter(|form| form.coverage >= crate::analysis::progress::MIN_FORM_COVERAGE)
        .filter(|form| form.started_at <= as_of)
        .collect::<Vec<_>>();
    language_forms.sort_by_key(|form| form.started_at);
    let recent_forms = language_forms.iter().rev().take(3).copied().collect::<Vec<_>>();
    let mut practice = records
        .iter()
        .filter(|record| record.header.metrics_version > 0 && record.header.bench_id.is_none())
        .filter(|record| session_language(record) == language)
        .filter(|record| parse_time(&record.header.started_at).is_some_and(|started| started <= as_of))
        .collect::<Vec<_>>();
    practice.sort_by_key(|record| parse_time(&record.header.started_at));
    let recent_accuracy = practice.iter().rev().take(3).copied().collect::<Vec<_>>();
    let week_start = as_of - chrono::Duration::days(7);
    let week = practice
        .iter()
        .filter(|record| parse_time(&record.header.started_at).is_some_and(|started| started >= week_start && started <= as_of))
        .copied()
        .cloned()
        .collect::<Vec<_>>();
    let mut values = component_values(&week, layout, geometry, options, as_of, english_corpus, japanese_corpus, language);
    values.retain(|row| !matches!(row.component, Component::Speed | Component::Accuracy));
    let corrected_speed = median(recent_forms.iter().map(|form| form.corrected_wpm));
    let uncorrected_speed = median(recent_accuracy.iter().map(|record| record.header.net_wpm));
    if let Some(speed) = corrected_speed.or(uncorrected_speed) {
        values.push(ComponentValue::new(Component::Speed, speed));
    }
    if let Some(accuracy) = median(recent_accuracy.iter().map(|record| record.header.accuracy * 100.0)) {
        values.push(ComponentValue::new(Component::Accuracy, accuracy));
    }
    if values.is_empty() {
        return None;
    }
    let (source, n) = if corrected_speed.is_none() {
        (GradeSource::ProvisionalUncorrected, recent_accuracy.len())
    } else {
        (GradeSource::Provisional, recent_forms.len())
    };
    evaluate_language(language, &values, source, GradeStatus::Provisional, n).ok()
}

#[allow(clippy::too_many_arguments)]
fn component_values(
    records: &[SessionRecord],
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
    options: GradeOptions,
    as_of: DateTime<Utc>,
    english_corpus: &[String],
    japanese_corpus: &[JapaneseText],
    language: &str,
) -> Vec<ComponentValue> {
    if records.is_empty() {
        return Vec::new();
    }
    let model = build_player_model(records, layout, geometry, options.model, ModelView::Recent, as_of);
    let mut values = Vec::new();
    if let Some(value) = median(records.iter().map(|record| record.header.net_wpm)) {
        values.push(ComponentValue::new(Component::Speed, value));
    }
    if let Some(value) = median(records.iter().map(|record| record.header.accuracy * 100.0)) {
        values.push(ComponentValue::new(Component::Accuracy, value));
    }
    let styles = records
        .iter()
        .map(|record| diagnose_style(std::slice::from_ref(record), &model))
        .collect::<Vec<_>>();
    if let Some(value) = median(styles.iter().filter_map(|style| style.rhythm.within_word_cv)) {
        values.push(ComponentValue::new(Component::Rhythm, value));
    }
    if let Some(value) = median(styles.iter().filter_map(|style| style.word_start.as_ref().map(|row| row.ratio))) {
        values.push(ComponentValue::new(Component::Preview, value));
    }
    if let Some(value) = median(styles.iter().filter_map(|style| style.rollover.as_ref().map(|row| row.rate * 100.0))) {
        values.push(ComponentValue::new(Component::Overlap, value));
    }
    if let Some(value) = transition_loss_percent(&model, language, english_corpus, japanese_corpus) {
        values.push(ComponentValue::new(Component::Transition, value));
    }
    if let Some(value) = diagnose_errors(records, layout, geometry, options.model, as_of)
        .post_error_slowdown
        .map(|row| row.excess_ms)
    {
        values.push(ComponentValue::new(Component::Recovery, value));
    }
    values
}

fn transition_loss_percent(model: &PlayerModel, language: &str, english_corpus: &[String], japanese_corpus: &[JapaneseText]) -> Option<f64> {
    let baseline = model.personal_iki_median_ms;
    if baseline <= 0.0 || !baseline.is_finite() {
        return None;
    }
    let spellings = if language == "ja" {
        japanese_corpus
            .iter()
            .map(|row| RomajiJudge::new(&row.reading).preferred_spelling(&model.spelling_preferences))
            .collect::<Vec<_>>()
    } else {
        english_corpus.to_vec()
    };
    let mut frequencies = BTreeMap::<(String, String), u64>::new();
    for spelling in spellings {
        let characters = spelling.chars().collect::<Vec<_>>();
        for pair in characters.windows(2) {
            *frequencies.entry((pair[0].to_string(), pair[1].to_string())).or_default() += 1;
        }
    }
    let mut loss = 0.0;
    let mut count = 0_u64;
    for ((from, to), frequency) in frequencies {
        let estimate = model.estimate_or_parent(&from, &to)?;
        loss += frequency as f64 * (estimate.estimated_iki_ms - baseline).max(0.0);
        count += frequency;
    }
    (count > 0).then_some(loss / (baseline * count as f64) * 100.0)
}

#[allow(clippy::too_many_arguments)]
fn build_history(
    records: &[SessionRecord],
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
    options: GradeOptions,
    english_corpus: &[String],
    japanese_corpus: &[JapaneseText],
) -> Vec<GradeHistoryPoint> {
    let mut history = Vec::new();
    for language in ["en", "ja"] {
        let benches = benchmark_records(records, language, None);
        let dates = benches
            .iter()
            .filter_map(|record| parse_time(&record.header.started_at).map(|at| at.date_naive()))
            .collect::<BTreeSet<_>>();
        for date in dates {
            let at = date.and_hms_opt(23, 59, 59).expect("valid end of day").and_utc();
            let available = benchmark_records(records, language, Some(at));
            let Some(grade) = bench_grade(language, &available, layout, geometry, options, at, english_corpus, japanese_corpus) else {
                continue;
            };
            for component in &grade.components {
                history.push(GradeHistoryPoint {
                    date,
                    language: language.to_owned(),
                    component: component.component.as_str().to_owned(),
                    grade: component.grade,
                    score: component.score,
                    version: GRADE_VERSION.to_owned(),
                });
            }
            if let Some(overall) = grade.overall {
                history.push(GradeHistoryPoint {
                    date,
                    language: language.to_owned(),
                    component: "overall".to_owned(),
                    grade: overall.grade,
                    score: overall.score,
                    version: GRADE_VERSION.to_owned(),
                });
            }
        }
    }
    history.sort_by(|left, right| {
        left.date
            .cmp(&right.date)
            .then_with(|| left.language.cmp(&right.language))
            .then_with(|| left.component.cmp(&right.component))
    });
    history
}

pub fn detect_grade_changes(history: &[GradeHistoryPoint]) -> Vec<GradeChange> {
    let keys = history
        .iter()
        .map(|row| (row.language.clone(), row.component.clone()))
        .collect::<BTreeSet<_>>();
    let mut changes = Vec::new();
    for (language, component) in keys {
        let mut rows = history
            .iter()
            .filter(|row| row.language == language && row.component == component)
            .collect::<Vec<_>>();
        rows.sort_by_key(|row| row.date);
        if rows.len() < 2 {
            continue;
        }
        let previous = rows[rows.len() - 2];
        let current = rows[rows.len() - 1];
        if current.grade == previous.grade {
            continue;
        }
        changes.push(GradeChange {
            language: language.clone(),
            component: component.clone(),
            from: previous.grade,
            to: current.grade,
            kind: if current.grade > previous.grade {
                GradeChangeKind::Promotion
            } else {
                GradeChangeKind::Demotion
            },
        });
    }
    changes
}

fn parse_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value).ok().map(|time| time.with_timezone(&Utc))
}

fn median(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut values = values.filter(|value| value.is_finite()).collect::<Vec<_>>();
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        Some((values[middle - 1] + values[middle]) / 2.0)
    } else {
        Some(values[middle])
    }
}
