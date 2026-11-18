//! Bundled practice-menu catalog and pure daily-menu assembly.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::analysis::errors::ErrorKind;
use crate::analysis::grades::{Component, Direction, LanguageGrade, Level, ranked_components};
use crate::config::{Config, MenuConfig, ModeKind};
use crate::i18n::Lang;

pub const MENU_VERSION: &str = "menu-v1";
const MENU_DATA: &str = include_str!("../../data/menu/menu-v1.toml");

/// Design §4.1: a menu needs this many logged character keystrokes.
pub const MIN_MENU_KEYSTROKES: u64 = 2_000;
/// Design §4.1: below 94.5%, accuracy is always primary A.
pub const ACCURACY_PRIORITY_PERCENT: f64 = 94.5;
/// Design §4.2: offer a benchmark after this many days.
pub const BENCH_REMINDER_DAYS: i64 = 7;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalizedText {
    pub en: String,
    pub ja: String,
}

impl LocalizedText {
    pub fn text(&self, lang: Lang) -> &str {
        match lang {
            Lang::En => &self.en,
            Lang::Ja => &self.ja,
        }
    }

    fn validate(&self, field: &str) -> Result<(), String> {
        if self.en.trim().is_empty() || self.ja.trim().is_empty() {
            Err(format!("{field} must contain non-empty en and ja text"))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceStrength {
    Phenomenon,
    Medium,
    Correlational,
    Weak,
    Hypothesis,
}

impl EvidenceStrength {
    pub fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Phenomenon, Lang::En) => "phenomenon",
            (Self::Phenomenon, Lang::Ja) => "現象",
            (Self::Medium, Lang::En) => "medium",
            (Self::Medium, Lang::Ja) => "中",
            (Self::Correlational, Lang::En) => "correlational",
            (Self::Correlational, Lang::Ja) => "相関",
            (Self::Weak, Lang::En) => "weak",
            (Self::Weak, Lang::Ja) => "弱",
            (Self::Hypothesis, Lang::En) => "hypothesis",
            (Self::Hypothesis, Lang::Ja) => "仮説",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MenuComponent {
    Speed,
    Accuracy,
    Rhythm,
    Transition,
    Preview,
    Recovery,
    Overlap,
    Reach,
    Japanese,
    Endurance,
    Blind,
}

impl MenuComponent {
    fn from_grade(component: Component) -> Self {
        match component {
            Component::Speed => Self::Speed,
            Component::Accuracy => Self::Accuracy,
            Component::Rhythm => Self::Rhythm,
            Component::Transition => Self::Transition,
            Component::Preview => Self::Preview,
            Component::Recovery => Self::Recovery,
            Component::Overlap => Self::Overlap,
        }
    }

    fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Speed, Lang::En) => "Speed",
            (Self::Speed, Lang::Ja) => "速さ",
            (Self::Accuracy, Lang::En) => "Accuracy",
            (Self::Accuracy, Lang::Ja) => "正確さ",
            (Self::Rhythm, Lang::En) => "Rhythm",
            (Self::Rhythm, Lang::Ja) => "リズム",
            (Self::Transition, Lang::En) => "Transitions",
            (Self::Transition, Lang::Ja) => "遷移",
            (Self::Preview, Lang::En) => "Preview",
            (Self::Preview, Lang::Ja) => "先読み",
            (Self::Recovery, Lang::En) => "Recovery",
            (Self::Recovery, Lang::Ja) => "立ち直り",
            (Self::Overlap, Lang::En) => "Rollover",
            (Self::Overlap, Lang::Ja) => "重なり",
            (Self::Reach, Lang::En) => "Reach",
            (Self::Reach, Lang::Ja) => "到達",
            (Self::Japanese, Lang::En) => "Japanese",
            (Self::Japanese, Lang::Ja) => "日本語",
            (Self::Endurance, Lang::En) => "Endurance",
            (Self::Endurance, Lang::Ja) => "持久",
            (Self::Blind, Lang::En) => "Visual assistance",
            (Self::Blind, Lang::Ja) => "視線",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricSource {
    TypingStyleWordStartRatio,
    TypingStyleWithinWordCv,
    ErrorDiagnosisPostErrorExcessMs,
    TypingStyleRolloverRate,
    SessionAccuracyGate,
    ErrorDiagnosisAnticipationProportion,
    SessionPacerTargetAndAccuracy,
    SkillAxisReach,
    EnduranceComparison,
    HabitsAssistanceRemovable,
    PrescriptionVerdict,
    SessionNetWpm,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MenuMetric {
    WordOnsetRatio,
    WithinWordIkiCv,
    PostErrorExcessMs,
    RolloverRate,
    AccuracyGatePassed,
    AnticipationRate,
    PacerTargetReached,
    FarHomeIkiRatio,
    SpellingUsageRate,
    EnduranceSecondHalfSlowdown,
    BlindAssistanceRemovable,
    PrescriptionR3,
    NetWpm,
}

const MENU_METRICS: [MenuMetric; 13] = [
    MenuMetric::WordOnsetRatio,
    MenuMetric::WithinWordIkiCv,
    MenuMetric::PostErrorExcessMs,
    MenuMetric::RolloverRate,
    MenuMetric::AccuracyGatePassed,
    MenuMetric::AnticipationRate,
    MenuMetric::PacerTargetReached,
    MenuMetric::FarHomeIkiRatio,
    MenuMetric::SpellingUsageRate,
    MenuMetric::EnduranceSecondHalfSlowdown,
    MenuMetric::BlindAssistanceRemovable,
    MenuMetric::PrescriptionR3,
    MenuMetric::NetWpm,
];

impl MenuMetric {
    pub fn all() -> &'static [Self] {
        &MENU_METRICS
    }

    pub fn source(self) -> MetricSource {
        match self {
            Self::WordOnsetRatio => MetricSource::TypingStyleWordStartRatio,
            Self::WithinWordIkiCv => MetricSource::TypingStyleWithinWordCv,
            Self::PostErrorExcessMs => MetricSource::ErrorDiagnosisPostErrorExcessMs,
            Self::RolloverRate => MetricSource::TypingStyleRolloverRate,
            Self::AccuracyGatePassed => MetricSource::SessionAccuracyGate,
            Self::AnticipationRate => MetricSource::ErrorDiagnosisAnticipationProportion,
            Self::PacerTargetReached => MetricSource::SessionPacerTargetAndAccuracy,
            Self::FarHomeIkiRatio => MetricSource::SkillAxisReach,
            // Japanese analysis exposes suggestions, but not usage frequency.
            Self::SpellingUsageRate => MetricSource::Unavailable,
            Self::EnduranceSecondHalfSlowdown => MetricSource::EnduranceComparison,
            Self::BlindAssistanceRemovable => MetricSource::HabitsAssistanceRemovable,
            Self::PrescriptionR3 => MetricSource::PrescriptionVerdict,
            Self::NetWpm => MetricSource::SessionNetWpm,
        }
    }

    pub fn direction(self) -> Direction {
        match self {
            Self::WordOnsetRatio
            | Self::WithinWordIkiCv
            | Self::PostErrorExcessMs
            | Self::AnticipationRate
            | Self::FarHomeIkiRatio
            | Self::EnduranceSecondHalfSlowdown => Direction::Lower,
            Self::RolloverRate
            | Self::AccuracyGatePassed
            | Self::PacerTargetReached
            | Self::SpellingUsageRate
            | Self::BlindAssistanceRemovable
            | Self::PrescriptionR3
            | Self::NetWpm => Direction::Higher,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AdvanceRule {
    LeOwnMedian,
    GeOwnMedian,
    GatePassed,
    R3Effective,
    TargetWithAccuracy,
    GeThreshold,
    LtThreshold,
    Removable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AdvanceSource {
    Check,
    Step,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdvanceCondition {
    pub metric: MenuMetric,
    pub rule: AdvanceRule,
    #[serde(default)]
    pub threshold: Option<f64>,
    pub sessions: u8,
    pub source: AdvanceSource,
    pub note: LocalizedText,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrescriptionLevel {
    pub mode: String,
    pub minutes: u8,
    #[serde(default)]
    pub settings: BTreeMap<String, toml::Value>,
    pub focus: LocalizedText,
    pub advance: AdvanceCondition,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MenuPrescription {
    pub id: String,
    pub component: MenuComponent,
    pub title: LocalizedText,
    pub evidence: EvidenceStrength,
    pub evidence_note: LocalizedText,
    pub alternate: String,
    #[serde(rename = "level")]
    pub levels: Vec<PrescriptionLevel>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MenuRole {
    Warmup,
    PrimaryA,
    PrimaryB,
    Check,
    Finish,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FixedStep {
    pub role: MenuRole,
    pub id: String,
    pub title: LocalizedText,
    pub mode: String,
    pub minutes: u8,
    #[serde(default)]
    pub settings: BTreeMap<String, toml::Value>,
    pub focus: LocalizedText,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MenuCatalog {
    pub version: String,
    #[serde(rename = "fixed_step")]
    pub fixed_steps: Vec<FixedStep>,
    #[serde(rename = "prescription")]
    pub prescriptions: Vec<MenuPrescription>,
}

impl MenuCatalog {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != MENU_VERSION {
            return Err(format!("menu version must be {MENU_VERSION}"));
        }
        let mut ids = BTreeSet::new();
        for step in &self.fixed_steps {
            if !ids.insert(step.id.as_str()) {
                return Err(format!("duplicate menu id: {}", step.id));
            }
            if !matches!(step.role, MenuRole::Warmup | MenuRole::Check | MenuRole::Finish) {
                return Err(format!("invalid fixed-step role for {}", step.id));
            }
            validate_mode(&step.mode, &step.id)?;
            validate_settings(&step.settings, &step.id)?;
            step.title.validate(&format!("{}.title", step.id))?;
            step.focus.validate(&format!("{}.focus", step.id))?;
        }
        for row in &self.prescriptions {
            if !ids.insert(row.id.as_str()) {
                return Err(format!("duplicate menu id: {}", row.id));
            }
            if row.levels.is_empty() {
                return Err(format!("prescription {} has zero levels", row.id));
            }
            row.title.validate(&format!("{}.title", row.id))?;
            row.evidence_note.validate(&format!("{}.evidence_note", row.id))?;
            for (index, level) in row.levels.iter().enumerate() {
                let owner = format!("{} level {}", row.id, index + 1);
                validate_mode(&level.mode, &owner)?;
                validate_settings(&level.settings, &owner)?;
                level.focus.validate(&format!("{owner}.focus"))?;
                level.advance.note.validate(&format!("{owner}.advance.note"))?;
                if level.minutes == 0 || level.advance.sessions == 0 {
                    return Err(format!("{owner} has invalid minutes or advance sessions"));
                }
            }
        }
        let prescription_ids = self.prescriptions.iter().map(|row| row.id.as_str()).collect::<BTreeSet<_>>();
        for row in &self.prescriptions {
            if !prescription_ids.contains(row.alternate.as_str()) {
                return Err(format!("alternate reference from {} to {} is missing", row.id, row.alternate));
            }
        }
        for role in [MenuRole::Warmup, MenuRole::Check, MenuRole::Finish] {
            if self.fixed_steps.iter().filter(|step| step.role == role).count() != 1 {
                return Err(format!("catalog must contain exactly one {role:?} fixed step"));
            }
        }
        Ok(())
    }

    pub fn prescription(&self, id: &str) -> Option<&MenuPrescription> {
        self.prescriptions.iter().find(|row| row.id == id)
    }

    fn fixed(&self, role: MenuRole) -> Result<&FixedStep, String> {
        self.fixed_steps
            .iter()
            .find(|step| step.role == role)
            .ok_or_else(|| format!("missing {role:?} fixed step"))
    }
}

pub fn parse_catalog(raw: &str) -> Result<MenuCatalog, String> {
    let catalog = toml::from_str::<MenuCatalog>(raw).map_err(|error| format!("menu catalog parse error: {error}"))?;
    catalog.validate()?;
    Ok(catalog)
}

pub fn bundled_catalog() -> Result<&'static MenuCatalog, String> {
    static CATALOG: OnceLock<Result<MenuCatalog, String>> = OnceLock::new();
    CATALOG.get_or_init(|| parse_catalog(MENU_DATA)).as_ref().map_err(Clone::clone)
}

fn validate_mode(mode: &str, owner: &str) -> Result<(), String> {
    if mode_kind(mode).is_some() {
        Ok(())
    } else {
        Err(format!("unknown mode {mode:?} in {owner}"))
    }
}

fn mode_kind(mode: &str) -> Option<ModeKind> {
    Some(match mode {
        "random" => ModeKind::Random,
        "words" => ModeKind::Words,
        "romaji" => ModeKind::Romaji,
        "sentence" => ModeKind::Sentence,
        "phrase" => ModeKind::Phrase,
        "ngram" => ModeKind::Ngram,
        "adaptive" => ModeKind::Adaptive,
        "unlock" => ModeKind::Unlock,
        "rhythm" => ModeKind::Rhythm,
        "burst" => ModeKind::Burst,
        "endurance" => ModeKind::Endurance,
        _ => return None,
    })
}

fn validate_settings(settings: &BTreeMap<String, toml::Value>, owner: &str) -> Result<(), String> {
    let config = toml::Value::try_from(Config::default()).map_err(|error| error.to_string())?;
    for (key, actual) in settings {
        let expected = dotted_value(&config, key).ok_or_else(|| format!("unknown setting {key:?} in {owner}"))?;
        if std::mem::discriminant(expected) != std::mem::discriminant(actual) {
            return Err(format!(
                "wrong type for setting {key:?} in {owner}: expected {}, got {}",
                value_kind(expected),
                value_kind(actual)
            ));
        }
    }
    Ok(())
}

fn dotted_value<'a>(root: &'a toml::Value, path: &str) -> Option<&'a toml::Value> {
    path.split('.').try_fold(root, |value, key| value.as_table()?.get(key))
}

fn value_kind(value: &toml::Value) -> &'static str {
    match value {
        toml::Value::String(_) => "string",
        toml::Value::Integer(_) => "integer",
        toml::Value::Float(_) => "float",
        toml::Value::Boolean(_) => "boolean",
        toml::Value::Datetime(_) => "datetime",
        toml::Value::Array(_) => "array",
        toml::Value::Table(_) => "table",
    }
}

/// Apply one menu level to an in-memory configuration. This never writes the
/// user's configuration file.
pub fn config_for_step(base: &Config, mode: &str, settings: &BTreeMap<String, toml::Value>) -> Result<Config, String> {
    let mut value = toml::Value::try_from(base.clone()).map_err(|error| error.to_string())?;
    set_dotted_value(&mut value, "mode.kind", toml::Value::String(mode.to_owned()))?;
    for (key, setting) in settings {
        set_dotted_value(&mut value, key, setting.clone())?;
    }
    let raw = toml::to_string(&value).map_err(|error| error.to_string())?;
    let mut config = Config::from_toml(&raw).map_err(|error| error.to_string())?;
    config.menu = base.menu.clone();
    Ok(config)
}

pub fn config_for_menu_step(base: &Config, step: &MenuStep) -> Result<Config, String> {
    let mut config = config_for_step(base, &step.mode, &step.settings)?;
    let language = language_code(step.language).to_owned();
    config.adaptive.lang.clone_from(&language);
    config.mode.sentence.lang.clone_from(&language);
    config.mode.phrase.lang = language;
    Ok(config)
}

fn set_dotted_value(root: &mut toml::Value, path: &str, setting: toml::Value) -> Result<(), String> {
    let mut parts = path.split('.').peekable();
    let mut current = root;
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            let table = current
                .as_table_mut()
                .ok_or_else(|| format!("setting parent for {path:?} is not a table"))?;
            if !table.contains_key(part) {
                return Err(format!("unknown setting {path:?}"));
            }
            table.insert(part.to_owned(), setting);
            return Ok(());
        }
        current = current
            .as_table_mut()
            .and_then(|table| table.get_mut(part))
            .ok_or_else(|| format!("unknown setting {path:?}"))?;
    }
    Err("setting path must not be empty".to_owned())
}

#[derive(Clone, Debug, PartialEq)]
pub struct MenuBuildInput {
    pub as_of: NaiveDate,
    pub language: String,
    pub grade: Option<LanguageGrade>,
    pub logged_keystrokes: u64,
    pub most_common_error: Option<ErrorKind>,
    pub reach_percentile: Option<f64>,
    pub japanese_percentile: Option<f64>,
    pub has_spelling_suggestion: bool,
    pub endurance_break_recommended: bool,
    /// Display-only context for preview prescriptions; never used for ranking.
    pub reaction_ms: Option<f64>,
    /// Display-only context for preview prescriptions; never used for ranking.
    pub reaction_median_ms: Option<f64>,
    pub last_bench_date: Option<NaiveDate>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PastMenu {
    pub date: NaiveDate,
    pub language: Option<String>,
    pub primary_prescription_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MenuLanguageInput {
    pub input: MenuBuildInput,
    pub states: BTreeMap<String, PrescriptionState>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PrescriptionJudgment {
    #[default]
    None,
    Ineffective,
    Maintenance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrescriptionState {
    pub level: u8,
    pub judgment: PrescriptionJudgment,
    pub paused: bool,
}

impl Default for PrescriptionState {
    fn default() -> Self {
        Self {
            level: 1,
            judgment: PrescriptionJudgment::None,
            paused: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MenuStep {
    pub role: MenuRole,
    pub language: Lang,
    pub prescription_id: String,
    pub title: String,
    pub level: Option<u8>,
    pub mode: String,
    pub minutes: u8,
    pub reason: String,
    pub focus: String,
    pub advance: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advance_condition: Option<AdvanceCondition>,
    pub evidence: Option<String>,
    pub evidence_note: Option<String>,
    pub settings: BTreeMap<String, toml::Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DailyMenu {
    pub date: NaiveDate,
    pub language: String,
    pub catalog_version: String,
    pub requested_minutes: u8,
    pub introductory: bool,
    pub bench_recommended: bool,
    pub steps: Vec<MenuStep>,
}

pub fn build_menu(
    catalog: &MenuCatalog,
    input: &MenuBuildInput,
    menu: &MenuConfig,
    history: &[PastMenu],
    states: &BTreeMap<String, PrescriptionState>,
) -> Result<DailyMenu, String> {
    let display = parse_language(&input.language)?;
    build_menu_for_languages(
        catalog,
        &[MenuLanguageInput {
            input: input.clone(),
            states: states.clone(),
        }],
        display,
        menu,
        history,
    )
}

#[derive(Clone, Debug)]
struct CandidateChoice {
    prescription_id: String,
    component: MenuComponent,
    overall_gain: f64,
}

#[derive(Clone, Copy)]
struct StepLanguage {
    display: Lang,
    practice: Lang,
    show_label: bool,
}

struct LanguagePlan<'a> {
    source: &'a MenuLanguageInput,
    language: Lang,
    sufficient: bool,
    candidates: Vec<CandidateChoice>,
    low_accuracy: Option<(Level, f64)>,
}

pub fn build_menu_for_languages(
    catalog: &MenuCatalog,
    inputs: &[MenuLanguageInput],
    display: Lang,
    menu: &MenuConfig,
    history: &[PastMenu],
) -> Result<DailyMenu, String> {
    if !matches!(menu.minutes, 10 | 15 | 20) {
        return Err("menu minutes must be 10, 15, or 20".to_owned());
    }
    if inputs.is_empty() || inputs.len() > 2 {
        return Err("a menu requires one or two language inputs".to_owned());
    }
    let as_of = inputs[0].input.as_of;
    if inputs.iter().any(|input| input.input.as_of != as_of) {
        return Err("all language inputs must use the same date".to_owned());
    }
    let mut plans = inputs
        .iter()
        .map(|source| build_language_plan(catalog, source, history))
        .collect::<Result<Vec<_>, _>>()?;
    plans.sort_by_key(|plan| match plan.language {
        Lang::En => 0,
        Lang::Ja => 1,
    });
    if plans.windows(2).any(|pair| pair[0].language == pair[1].language) {
        return Err("menu language inputs must not contain duplicates".to_owned());
    }
    let bench_recommended = plans.iter().any(|plan| {
        plan.source
            .input
            .last_bench_date
            .is_none_or(|date| as_of.signed_duration_since(date).num_days() >= BENCH_REMINDER_DAYS)
    });
    let multi = plans.len() == 2;
    if plans.iter().all(|plan| !plan.sufficient) {
        let selected = alternating_index(as_of, plans.len());
        let plan = &plans[selected];
        return introductory_menu(
            catalog,
            &plan.source.input,
            menu,
            StepLanguage {
                display,
                practice: plan.language,
                show_label: multi || plan.language != display,
            },
            &plan.source.states,
            bench_recommended,
        );
    }

    let a_index = choose_primary_language(&plans, as_of, menu.minutes, history);
    let a_plan = &plans[a_index];
    let b_plan = if menu.minutes == 10 {
        None
    } else if plans.len() == 2 {
        Some(&plans[1 - a_index])
    } else {
        Some(a_plan)
    };
    let a_candidate = a_plan.candidates.first();
    let b_candidate = b_plan.and_then(|plan| {
        if plans.len() == 1 {
            plan.candidates.get(1)
        } else {
            plan.candidates.first()
        }
    });

    let mut steps = vec![fixed_step(catalog.fixed(MenuRole::Warmup)?, display, a_plan.language, "-")];
    steps.push(primary_step(
        catalog,
        a_plan,
        a_candidate,
        MenuRole::PrimaryA,
        primary_minutes(menu.minutes),
        display,
        multi || a_plan.language != display,
    )?);
    if let Some(plan) = b_plan {
        steps.push(primary_step(
            catalog,
            plan,
            b_candidate,
            MenuRole::PrimaryB,
            4,
            display,
            multi || plan.language != display,
        )?);
    }
    let check_language = plans[alternating_index(as_of, plans.len())].language;
    let mut check = fixed_step(catalog.fixed(MenuRole::Check)?, display, check_language, "-");
    if menu.minutes == 20 {
        check.minutes = 4;
    }
    steps.push(check);
    let finish_language = b_plan.map_or(a_plan.language, |plan| plan.language);
    steps.push(fixed_step(catalog.fixed(MenuRole::Finish)?, display, finish_language, "-"));
    Ok(DailyMenu {
        date: as_of,
        language: language_code(a_plan.language).to_owned(),
        catalog_version: catalog.version.clone(),
        requested_minutes: menu.minutes,
        introductory: false,
        bench_recommended,
        steps,
    })
}

fn build_language_plan<'a>(catalog: &MenuCatalog, source: &'a MenuLanguageInput, history: &[PastMenu]) -> Result<LanguagePlan<'a>, String> {
    let input = &source.input;
    let language = parse_language(&input.language)?;
    let sufficient = input.grade.as_ref().and_then(|grade| grade.overall.as_ref()).is_some() && input.logged_keystrokes >= MIN_MENU_KEYSTROKES;
    if !sufficient {
        return Ok(LanguagePlan {
            source,
            language,
            sufficient,
            candidates: Vec::new(),
            low_accuracy: None,
        });
    }
    let grade = input.grade.as_ref().expect("sufficiency checked grade");
    let opportunities = ranked_components(&grade.components);
    let mut candidates = ranked_components(&grade.components)
        .into_iter()
        .map(|row| MenuComponent::from_grade(row.component))
        .collect::<Vec<_>>();
    let accuracy = grade.components.iter().find(|row| row.component == Component::Accuracy);
    let low_accuracy = accuracy
        .filter(|row| row.grade <= Level::D || row.value < ACCURACY_PRIORITY_PERCENT)
        .map(|row| (row.grade, row.value));
    let forced_accuracy_id = low_accuracy
        .is_some()
        .then_some(if input.most_common_error == Some(ErrorKind::Anticipation) {
            "accuracy.anticipation"
        } else {
            "accuracy.gate"
        });
    if low_accuracy.is_some() {
        candidates.retain(|component| *component != MenuComponent::Accuracy);
    }

    let top_candidates_maintenance = !candidates.is_empty()
        && candidates
            .iter()
            .take(2)
            .all(|component| component_is_maintenance(catalog, *component, &source.states));
    let weakest_grade_percentile = grade.components.iter().map(|row| row.score * 12.5).min_by(f64::total_cmp);
    if top_candidates_maintenance && input.endurance_break_recommended {
        candidates.insert(0, MenuComponent::Endurance);
    }
    if input.has_spelling_suggestion
        && (top_candidates_maintenance
            || input
                .japanese_percentile
                .zip(weakest_grade_percentile)
                .is_some_and(|(japanese, weakest)| japanese < weakest))
    {
        candidates.insert(0, MenuComponent::Japanese);
    }
    if input.reach_percentile.is_some_and(|percentile| percentile < 25.0)
        && (top_candidates_maintenance
            || input
                .reach_percentile
                .zip(weakest_grade_percentile)
                .is_some_and(|(reach, weakest)| reach < weakest))
    {
        candidates.insert(0, MenuComponent::Reach);
    }

    let language_history = history
        .iter()
        .filter(|past| past.language.as_deref().is_none_or(|value| value == input.language))
        .cloned()
        .collect::<Vec<_>>();
    let repeated = repeated_components(catalog, input.as_of, &language_history);
    for component in MenuComponent::from_grade_components() {
        if repeated.contains(&component) || component_is_maintenance(catalog, component, &source.states) {
            demote_once(&mut candidates, component);
        }
    }

    let mut selected = Vec::<CandidateChoice>::new();
    if let Some(id) = forced_accuracy_id
        && let Some(row) = available_prescription(catalog, id, &source.states)
    {
        selected.push(CandidateChoice {
            prescription_id: row.0.id.clone(),
            component: row.3,
            overall_gain: f64::INFINITY,
        });
    }
    for component in candidates {
        if selected.len() >= 2 {
            break;
        }
        let Some(id) = default_prescription_id(component) else { continue };
        let Some(row) = available_prescription(catalog, id, &source.states) else {
            continue;
        };
        if selected.iter().all(|selected| selected.component != row.0.component) {
            let overall_gain = opportunities
                .iter()
                .find(|opportunity| MenuComponent::from_grade(opportunity.component) == component)
                .map_or(1.0, |opportunity| opportunity.overall_gain);
            selected.push(CandidateChoice {
                prescription_id: row.0.id.clone(),
                component: row.3,
                overall_gain,
            });
        }
    }
    Ok(LanguagePlan {
        source,
        language,
        sufficient,
        candidates: selected,
        low_accuracy,
    })
}

fn choose_primary_language(plans: &[LanguagePlan<'_>], as_of: NaiveDate, minutes: u8, history: &[PastMenu]) -> usize {
    if plans.len() == 1 {
        return 0;
    }
    if minutes == 10 {
        return alternating_index(as_of, plans.len());
    }
    if let Some((index, _)) = plans
        .iter()
        .enumerate()
        .filter_map(|(index, plan)| plan.low_accuracy.map(|accuracy| (index, accuracy)))
        .min_by(|left, right| left.1.0.cmp(&right.1.0).then_with(|| left.1.1.total_cmp(&right.1.1)))
    {
        return index;
    }
    let yesterday_language = history
        .iter()
        .find(|past| past.date == as_of.pred_opt().unwrap_or(as_of))
        .and_then(|past| past.language.as_deref());
    plans
        .iter()
        .enumerate()
        .filter(|(_, plan)| plan.sufficient)
        .max_by(|(_, left), (_, right)| {
            left.candidates
                .first()
                .map_or(f64::NEG_INFINITY, |candidate| candidate.overall_gain)
                .total_cmp(&right.candidates.first().map_or(f64::NEG_INFINITY, |candidate| candidate.overall_gain))
                .then_with(|| {
                    let left_fresh = yesterday_language != Some(language_code(left.language));
                    let right_fresh = yesterday_language != Some(language_code(right.language));
                    left_fresh.cmp(&right_fresh)
                })
                .then_with(|| language_code(right.language).cmp(language_code(left.language)))
        })
        .map_or_else(|| alternating_index(as_of, plans.len()), |(index, _)| index)
}

fn alternating_index(as_of: NaiveDate, count: usize) -> usize {
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).expect("valid epoch");
    as_of.signed_duration_since(epoch).num_days().rem_euclid(count as i64) as usize
}

fn parse_language(language: &str) -> Result<Lang, String> {
    Lang::from_str(language).ok_or_else(|| format!("unknown menu language {language:?}"))
}

fn language_code(language: Lang) -> &'static str {
    match language {
        Lang::En => "en",
        Lang::Ja => "ja",
    }
}

impl MenuComponent {
    fn from_grade_components() -> [Self; 7] {
        [
            Self::Speed,
            Self::Accuracy,
            Self::Rhythm,
            Self::Transition,
            Self::Preview,
            Self::Recovery,
            Self::Overlap,
        ]
    }
}

fn introductory_menu(
    catalog: &MenuCatalog,
    input: &MenuBuildInput,
    menu: &MenuConfig,
    language: StepLanguage,
    states: &BTreeMap<String, PrescriptionState>,
    bench_recommended: bool,
) -> Result<DailyMenu, String> {
    let StepLanguage {
        display,
        practice,
        show_label,
    } = language;
    let intro_reason = match display {
        Lang::En => "Introductory menu: fewer than 2,000 logged keystrokes or no overall grade",
        Lang::Ja => "入門メニュー: 打鍵ログ 2,000 未満または総合なし",
    };
    let mut steps = vec![fixed_step(
        catalog.fixed(MenuRole::Warmup)?,
        display,
        practice,
        &reason_with_language(intro_reason, practice, display, show_label),
    )];
    let selected = available_prescription(catalog, "transition.adaptive", states)
        .or_else(|| available_prescription(catalog, "transition.ngram", states))
        .ok_or_else(|| "no introductory transition prescription is available".to_owned())?;
    let reason = match display {
        Lang::En => "Introductory routine: practice weak transitions while collecting baseline data",
        Lang::Ja => "入門ルーチン: 基準データを集めながら弱い遷移を練習",
    };
    let reason = reason_with_language(reason, practice, display, show_label);
    steps.push(prescription_step_with_reason(
        &selected,
        MenuRole::PrimaryA,
        menu.minutes.saturating_sub(7),
        display,
        practice,
        &reason,
    ));
    let mut check = fixed_step(catalog.fixed(MenuRole::Check)?, display, practice, &reason);
    check.minutes = 3;
    steps.push(check);
    let mut finish = fixed_step(catalog.fixed(MenuRole::Finish)?, display, practice, &reason);
    finish.minutes = 2;
    steps.push(finish);
    Ok(DailyMenu {
        date: input.as_of,
        language: language_code(practice).to_owned(),
        catalog_version: catalog.version.clone(),
        requested_minutes: menu.minutes,
        introductory: true,
        bench_recommended,
        steps,
    })
}

fn primary_minutes(minutes: u8) -> u8 {
    match minutes {
        10 => 4,
        20 => 7,
        _ => 5,
    }
}

fn fixed_step(step: &FixedStep, display: Lang, practice: Lang, reason: &str) -> MenuStep {
    MenuStep {
        role: step.role,
        language: practice,
        prescription_id: step.id.clone(),
        title: step.title.text(display).to_owned(),
        level: None,
        mode: step.mode.clone(),
        minutes: step.minutes,
        reason: reason.to_owned(),
        focus: step.focus.text(display).to_owned(),
        advance: "-".to_owned(),
        advance_condition: None,
        evidence: None,
        evidence_note: None,
        settings: step.settings.clone(),
    }
}

fn prescription_step(
    selected: &(&MenuPrescription, &PrescriptionLevel, u8, MenuComponent),
    role: MenuRole,
    minutes: u8,
    grade: &LanguageGrade,
    input: &MenuBuildInput,
    language: StepLanguage,
) -> MenuStep {
    let StepLanguage {
        display,
        practice,
        show_label,
    } = language;
    let component = selected.3;
    let reason = grade_reason(component, grade, input, display);
    let reason = reason_with_language(&reason, practice, display, show_label);
    prescription_step_with_reason(selected, role, minutes, display, practice, &reason)
}

fn prescription_step_with_reason(
    selected: &(&MenuPrescription, &PrescriptionLevel, u8, MenuComponent),
    role: MenuRole,
    minutes: u8,
    display: Lang,
    practice: Lang,
    reason: &str,
) -> MenuStep {
    let (prescription, level, level_number, _) = selected;
    MenuStep {
        role,
        language: practice,
        prescription_id: prescription.id.clone(),
        title: prescription.title.text(display).to_owned(),
        level: Some(*level_number),
        mode: level.mode.clone(),
        minutes,
        reason: reason.to_owned(),
        focus: level.focus.text(display).to_owned(),
        advance: level.advance.note.text(display).to_owned(),
        advance_condition: Some(level.advance.clone()),
        evidence: Some(prescription.evidence.label(display).to_owned()),
        evidence_note: Some(prescription.evidence_note.text(display).to_owned()),
        settings: level.settings.clone(),
    }
}

fn primary_step(
    catalog: &MenuCatalog,
    plan: &LanguagePlan<'_>,
    candidate: Option<&CandidateChoice>,
    role: MenuRole,
    minutes: u8,
    display: Lang,
    show_language: bool,
) -> Result<MenuStep, String> {
    if let Some(candidate) = candidate {
        let selected = available_prescription(catalog, &candidate.prescription_id, &plan.source.states)
            .ok_or_else(|| format!("prescription {:?} became unavailable", candidate.prescription_id))?;
        let grade = plan
            .source
            .input
            .grade
            .as_ref()
            .ok_or_else(|| "candidate has no language grade".to_owned())?;
        return Ok(prescription_step(
            &selected,
            role,
            minutes,
            grade,
            &plan.source.input,
            StepLanguage {
                display,
                practice: plan.language,
                show_label: show_language,
            },
        ));
    }
    let selected = available_prescription(catalog, "transition.adaptive", &plan.source.states)
        .or_else(|| available_prescription(catalog, "transition.ngram", &plan.source.states))
        .ok_or_else(|| "no introductory transition prescription is available".to_owned())?;
    let reason = match display {
        Lang::En => "Introductory routine: practice weak transitions while collecting baseline data",
        Lang::Ja => "入門ルーチン: 基準データを集めながら弱い遷移を練習",
    };
    let reason = reason_with_language(reason, plan.language, display, show_language);
    Ok(prescription_step_with_reason(&selected, role, minutes, display, plan.language, &reason))
}

fn reason_with_language(reason: &str, practice: Lang, display: Lang, show_language: bool) -> String {
    if !show_language {
        return reason.to_owned();
    }
    let label = match (practice, display) {
        (Lang::En, Lang::En) => "EN",
        (Lang::Ja, Lang::En) => "JA",
        (Lang::En, Lang::Ja) => "英",
        (Lang::Ja, Lang::Ja) => "日",
    };
    format!("{label}: {reason}")
}

fn grade_reason(component: MenuComponent, grade: &LanguageGrade, input: &MenuBuildInput, lang: Lang) -> String {
    let row = grade.components.iter().find(|row| MenuComponent::from_grade(row.component) == component);
    if component == MenuComponent::Accuracy && row.is_some_and(|row| row.value < ACCURACY_PRIORITY_PERCENT) {
        return match lang {
            Lang::En => format!("Accuracy {:.1}% (< {:.1}%)", row.unwrap().value, ACCURACY_PRIORITY_PERCENT),
            Lang::Ja => format!("正確さ {:.1}%（{:.1}% 未満）", row.unwrap().value, ACCURACY_PRIORITY_PERCENT),
        };
    }
    let mut reason = if let Some(row) = row {
        let value = format_grade_measure(row.component, row.value, &row.unit);
        format!("{} {} ({value})", component.label(lang), row.grade.as_str())
    } else {
        match component {
            MenuComponent::Reach => format!("{} {:.1} percentile", component.label(lang), input.reach_percentile.unwrap_or_default()),
            MenuComponent::Japanese => format!("{}: suggested spelling available", component.label(lang)),
            MenuComponent::Endurance => format!("{}: second-half slowdown detected", component.label(lang)),
            _ => component.label(lang).to_owned(),
        }
    };
    if component == MenuComponent::Preview
        && let (Some(reaction), Some(median)) = (input.reaction_ms, input.reaction_median_ms)
    {
        match lang {
            Lang::En => reason.push_str(&format!("; reaction {reaction:.0} ms (own median {median:.0} ms)")),
            Lang::Ja => reason.push_str(&format!("；反応 {reaction:.0} ms（自分の中央値 {median:.0} ms）")),
        }
    }
    reason
}

fn format_grade_measure(component: Component, value: f64, unit: &str) -> String {
    let number = match component {
        Component::Rhythm | Component::Preview => format!("{value:.2}"),
        Component::Recovery => format!("{value:.0}"),
        _ => format!("{value:.1}"),
    };
    if unit.is_empty() { number } else { format!("{number} {unit}") }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdvanceOutcome {
    Met,
    NotMet,
    CannotDetermine,
}

impl AdvanceOutcome {
    pub fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Met, Lang::En) => "met",
            (Self::Met, Lang::Ja) => "満たした",
            (Self::NotMet, Lang::En) => "not met",
            (Self::NotMet, Lang::Ja) => "満たさない",
            (Self::CannotDetermine, Lang::En) => "cannot determine",
            (Self::CannotDetermine, Lang::Ja) => "判定できない",
        }
    }
}

/// Evaluate only the current session. Rules that require a personal median,
/// multiple sessions, or R3 controls deliberately remain indeterminate in M3.
pub fn evaluate_current_advance(condition: &AdvanceCondition, record: &crate::record::SessionRecord) -> AdvanceOutcome {
    let params = serde_json::from_str::<serde_json::Value>(&record.header.mode_params).unwrap_or_default();
    let met = match condition.rule {
        AdvanceRule::GatePassed => params.get("score_eligible").and_then(serde_json::Value::as_bool),
        AdvanceRule::TargetWithAccuracy => params
            .get("pacer_target_wpm")
            .and_then(serde_json::Value::as_f64)
            .map(|target| record.header.accuracy >= 0.95 && record.header.net_wpm >= target),
        AdvanceRule::GeThreshold | AdvanceRule::LtThreshold => {
            current_metric_value(condition.metric, record)
                .zip(condition.threshold)
                .map(|(value, threshold)| match condition.rule {
                    AdvanceRule::GeThreshold => value >= threshold,
                    AdvanceRule::LtThreshold => value < threshold,
                    _ => unreachable!(),
                })
        }
        AdvanceRule::LeOwnMedian | AdvanceRule::GeOwnMedian | AdvanceRule::R3Effective | AdvanceRule::Removable => None,
    };
    match met {
        Some(true) => AdvanceOutcome::Met,
        Some(false) => AdvanceOutcome::NotMet,
        None => AdvanceOutcome::CannotDetermine,
    }
}

fn current_metric_value(metric: MenuMetric, record: &crate::record::SessionRecord) -> Option<f64> {
    match metric {
        MenuMetric::NetWpm => Some(record.header.net_wpm),
        MenuMetric::EnduranceSecondHalfSlowdown => {
            let comparison = crate::practice::session_variants::compare_endurance_log(&record.log);
            (comparison.first_iki_ms > 0.0 && comparison.second_iki_ms > 0.0)
                .then_some((comparison.second_iki_ms - comparison.first_iki_ms) / comparison.first_iki_ms)
        }
        _ => None,
    }
}

fn default_prescription_id(component: MenuComponent) -> Option<&'static str> {
    Some(match component {
        MenuComponent::Speed => "speed.pacer",
        MenuComponent::Accuracy => "accuracy.gate",
        MenuComponent::Rhythm => "rhythm.metronome",
        MenuComponent::Transition => "transition.adaptive",
        MenuComponent::Preview => "preview.wide",
        MenuComponent::Recovery => "recovery.retry",
        MenuComponent::Overlap => "overlap.burst",
        MenuComponent::Reach => "reach.unlock",
        MenuComponent::Japanese => "japanese.spelling",
        MenuComponent::Endurance => "endurance.split",
        MenuComponent::Blind => "blind.fade",
    })
}

fn available_prescription<'a>(
    catalog: &'a MenuCatalog,
    id: &str,
    states: &BTreeMap<String, PrescriptionState>,
) -> Option<(&'a MenuPrescription, &'a PrescriptionLevel, u8, MenuComponent)> {
    let mut id = id;
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert(id.to_owned()) {
            return None;
        }
        let prescription = catalog.prescription(id)?;
        let state = states.get(id).copied().unwrap_or_default();
        if state.paused {
            return None;
        }
        if state.judgment == PrescriptionJudgment::Ineffective {
            id = &prescription.alternate;
            continue;
        }
        let level_number = state.level.max(1).min(prescription.levels.len() as u8);
        let level = prescription.levels.get(usize::from(level_number - 1))?;
        return Some((prescription, level, level_number, prescription.component));
    }
}

fn component_is_maintenance(catalog: &MenuCatalog, component: MenuComponent, states: &BTreeMap<String, PrescriptionState>) -> bool {
    let Some(id) = default_prescription_id(component) else { return false };
    let Some(row) = catalog.prescription(id) else { return false };
    states
        .get(&row.id)
        .is_some_and(|state| state.judgment == PrescriptionJudgment::Maintenance)
}

fn repeated_components(catalog: &MenuCatalog, as_of: NaiveDate, history: &[PastMenu]) -> BTreeSet<MenuComponent> {
    let mut by_day = BTreeMap::<NaiveDate, BTreeSet<MenuComponent>>::new();
    for past in history {
        let components = past
            .primary_prescription_ids
            .iter()
            .filter_map(|id| catalog.prescription(id).map(|row| row.component))
            .collect();
        by_day.insert(past.date, components);
    }
    let yesterday = as_of.pred_opt();
    let two_days_ago = as_of.checked_sub_signed(chrono::Duration::days(2));
    match (yesterday.and_then(|day| by_day.get(&day)), two_days_ago.and_then(|day| by_day.get(&day))) {
        (Some(left), Some(right)) => left.intersection(right).copied().collect(),
        _ => BTreeSet::new(),
    }
}

fn demote_once(candidates: &mut [MenuComponent], component: MenuComponent) {
    if let Some(index) = candidates.iter().position(|candidate| *candidate == component)
        && index + 1 < candidates.len()
    {
        candidates.swap(index, index + 1);
    }
}

pub fn build_from_storage(
    path: &std::path::Path,
    home: &std::path::Path,
    config: &Config,
    display: Lang,
    as_of: NaiveDate,
) -> crate::error::AppResult<DailyMenu> {
    use crate::analysis::DiagnosisOptions;
    use crate::analysis::japanese::SpellingDecision;
    use crate::analysis::metrics::{AnalysisSettings, analyze_records};
    use crate::analysis::progress::{ProgressOptions, ReferencePrompts, analyze_progress};
    use crate::analysis::summary::{SkillAxis, skill_axis_values};
    use crate::bench::{BenchPart, generate_version};
    use crate::data::{DataCatalog, TrainingDataSource};
    use crate::engine::session::KeystrokeKind;
    use crate::error::AppError;
    use crate::layout::builtin_layout;
    use crate::layout::geometry::geometry;
    use crate::record::db;

    let records = db::load_records_filtered(path, None, None)?;
    let layout = builtin_layout(&config.keyboard.layout).unwrap_or_else(|_| builtin_layout("qwerty").expect("bundled qwerty"));
    let keyboard_geometry = geometry(&config.keyboard.geometry).unwrap_or_else(|| geometry("ansi").expect("bundled ansi"));
    let corpus = DataCatalog::load(&crate::config::DataConfig::default(), home)?;
    let model = AnalysisSettings {
        pause_ms: config.analysis.pause_ms,
        shrinkage_k: config.analysis.shrinkage_k,
        half_life_days: config.analysis.half_life_days,
    };
    let at = as_of.and_hms_opt(23, 59, 59).expect("valid end of day").and_utc();
    let reference_version = |part| generate_version(part, 1, &corpus, &layout, &keyboard_geometry);
    let references = ReferencePrompts {
        english: reference_version(BenchPart::A)
            .prompts
            .into_iter()
            .map(|prompt| crate::analysis::model::ExpectedPrompt::english(prompt.target))
            .collect(),
        japanese: reference_version(BenchPart::C)
            .prompts
            .into_iter()
            .filter_map(|prompt| prompt.reading.map(crate::analysis::model::ExpectedPrompt::japanese))
            .collect(),
    };
    let progress = analyze_progress(
        &records,
        &layout,
        &keyboard_geometry,
        ProgressOptions {
            model,
            min_transition_n: config.analysis.min_transition_n,
        },
        at,
        &references,
    );
    let grades = crate::analysis::grades::analyze_grades(
        &records,
        &progress.forms,
        &layout,
        &keyboard_geometry,
        crate::analysis::grades::GradeOptions { model },
        at,
        corpus.words_en(),
        corpus.words_ja(),
    );
    let mut states = BTreeMap::new();
    if db::load_prescriptions(path)?
        .iter()
        .rev()
        .find(|row| row.kind == "adaptive")
        .is_some_and(|row| row.status == "ineffective")
    {
        states.insert(
            "transition.adaptive".to_owned(),
            PrescriptionState {
                judgment: PrescriptionJudgment::Ineffective,
                ..PrescriptionState::default()
            },
        );
    }
    let mut inputs = Vec::new();
    for practice in config.menu.practice_languages(display) {
        let language = language_code(practice);
        let language_records = records
            .iter()
            .filter(|record| crate::analysis::model::session_language(record) == language)
            .cloned()
            .collect::<Vec<_>>();
        let diagnosis = crate::analysis::diagnose(
            &language_records,
            &layout,
            &keyboard_geometry,
            DiagnosisOptions {
                model,
                min_transition_n: config.analysis.min_transition_n,
                spelling_suggestions: config.analysis.spelling_suggestions,
            },
            at,
            corpus.words_ja(),
        );
        let axes = skill_axis_values(&language_records, &layout, config);
        let reach_percentile = axes
            .iter()
            .find(|value| value.axis == SkillAxis::Reach)
            .and_then(|value| value.percentile);
        let japanese_percentile = axes
            .iter()
            .find(|value| value.axis == SkillAxis::Japanese)
            .and_then(|value| value.percentile);
        let logged_keystrokes = language_records
            .iter()
            .filter(|record| record.header.metrics_version > 0)
            .flat_map(|record| &record.log.keystrokes)
            .filter(|key| key.kind == KeystrokeKind::Char)
            .count() as u64;
        let maximum_error_count = diagnosis.errors.kinds.iter().map(|row| row.n).max().unwrap_or_default();
        let most_common_error = (maximum_error_count > 0)
            .then(|| {
                diagnosis
                    .errors
                    .kinds
                    .iter()
                    .find(|row| row.n == maximum_error_count && row.kind == ErrorKind::Anticipation)
                    .map(|row| row.kind)
                    .or_else(|| diagnosis.errors.kinds.iter().find(|row| row.n == maximum_error_count).map(|row| row.kind))
            })
            .flatten();
        let has_spelling_suggestion = diagnosis.japanese.spelling.iter().any(|row| row.decision == SpellingDecision::Suggested);
        let endurance_break_recommended = language_records
            .iter()
            .rev()
            .find(|record| record.header.mode == "endurance")
            .is_some_and(|record| crate::practice::session_variants::compare_endurance_log(&record.log).warn);
        let details = analyze_records(&language_records, &layout, &keyboard_geometry, model);
        let reaction_median_ms = (details.reaction_median_ms > 0.0).then_some(details.reaction_median_ms);
        let reaction_ms = language_records.last().and_then(|record| {
            let details = analyze_records(std::slice::from_ref(record), &layout, &keyboard_geometry, model);
            (details.reaction_median_ms > 0.0).then_some(details.reaction_median_ms)
        });
        let last_bench_date = language_records
            .iter()
            .rev()
            .find(|record| record.header.bench_id.is_some())
            .and_then(|record| chrono::DateTime::parse_from_rfc3339(&record.header.started_at).ok())
            .map(|time| time.date_naive());
        inputs.push(MenuLanguageInput {
            input: MenuBuildInput {
                as_of,
                language: language.to_owned(),
                grade: grades.languages.iter().find(|grade| grade.language == language).cloned(),
                logged_keystrokes,
                most_common_error,
                reach_percentile,
                japanese_percentile,
                has_spelling_suggestion,
                endurance_break_recommended,
                reaction_ms,
                reaction_median_ms,
                last_bench_date,
            },
            states: states.clone(),
        });
    }
    build_menu_for_languages(bundled_catalog().map_err(AppError::Record)?, &inputs, display, &config.menu, &[]).map_err(AppError::Record)
}

pub fn render_from_storage(
    path: &std::path::Path,
    home: &std::path::Path,
    config: &Config,
    lang: Lang,
    json: bool,
    as_of: NaiveDate,
) -> crate::error::AppResult<String> {
    let menu = build_from_storage(path, home, config, lang, as_of)?;
    if json {
        let mut output = serde_json::to_string_pretty(&menu)?;
        output.push('\n');
        Ok(output)
    } else {
        Ok(render_text(&menu, lang))
    }
}

fn render_text(menu: &DailyMenu, lang: Lang) -> String {
    use std::fmt::Write;

    let mut output = String::new();
    match lang {
        Lang::En => {
            let _ = writeln!(output, "Today's practice menu — {} minutes", menu.requested_minutes);
            if menu.bench_recommended {
                let _ = writeln!(output, "Benchmark recommended first (about 4 minutes).");
            }
        }
        Lang::Ja => {
            let _ = writeln!(output, "今日の練習メニュー — {} 分", menu.requested_minutes);
            if menu.bench_recommended {
                let _ = writeln!(output, "先にベンチ（約 4 分）を勧めます．");
            }
        }
    }
    for (index, step) in menu.steps.iter().enumerate() {
        let level = step.level.map_or_else(|| "-".to_owned(), |level| level.to_string());
        let evidence = step.evidence.as_deref().unwrap_or("-");
        match lang {
            Lang::En => {
                let _ = writeln!(output, "\nStep: {} {}", index + 1, role_label(step.role, lang));
                let _ = writeln!(output, "Language: {}", language_row_label(step.language, lang));
                let _ = writeln!(output, "Prescription ID: {}", step.prescription_id);
                let _ = writeln!(output, "Title: {}", step.title);
                let _ = writeln!(output, "Level: {level}");
                let _ = writeln!(output, "Mode: {}", step.mode);
                let _ = writeln!(output, "Time: {} min", step.minutes);
                let _ = writeln!(output, "Reason selected: {}", step.reason);
                let _ = writeln!(output, "Focus: {}", step.focus);
                let _ = writeln!(output, "Advance condition: {}", step.advance);
                let _ = writeln!(output, "Evidence strength: {evidence}");
            }
            Lang::Ja => {
                let _ = writeln!(output, "\n段: {} {}", index + 1, role_label(step.role, lang));
                let _ = writeln!(output, "言語: {}", language_row_label(step.language, lang));
                let _ = writeln!(output, "処方 ID: {}", step.prescription_id);
                let _ = writeln!(output, "題: {}", step.title);
                let _ = writeln!(output, "レベル: {level}");
                let _ = writeln!(output, "モード: {}", step.mode);
                let _ = writeln!(output, "時間: {} 分", step.minutes);
                let _ = writeln!(output, "選んだ理由: {}", step.reason);
                let _ = writeln!(output, "意識すること: {}", step.focus);
                let _ = writeln!(output, "卒業の条件: {}", step.advance);
                let _ = writeln!(output, "根拠の強さ: {evidence}");
            }
        }
    }
    output
}

fn language_row_label(practice: Lang, display: Lang) -> &'static str {
    match (practice, display) {
        (Lang::En, Lang::En) => "EN",
        (Lang::Ja, Lang::En) => "JA",
        (Lang::En, Lang::Ja) => "英",
        (Lang::Ja, Lang::Ja) => "日",
    }
}

fn role_label(role: MenuRole, lang: Lang) -> &'static str {
    match (role, lang) {
        (MenuRole::Warmup, Lang::En) => "Warm-up",
        (MenuRole::Warmup, Lang::Ja) => "ウォームアップ",
        (MenuRole::PrimaryA, Lang::En) => "Primary A",
        (MenuRole::PrimaryA, Lang::Ja) => "主処方 A",
        (MenuRole::PrimaryB, Lang::En) => "Primary B",
        (MenuRole::PrimaryB, Lang::Ja) => "主処方 B",
        (MenuRole::Check, Lang::En) => "Check",
        (MenuRole::Check, Lang::Ja) => "確認",
        (MenuRole::Finish, Lang::En) => "Finish",
        (MenuRole::Finish, Lang::Ja) => "仕上げ",
    }
}

#[allow(dead_code)]
fn _level_name(level: Level) -> &'static str {
    level.as_str()
}
