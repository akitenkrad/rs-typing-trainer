//! Bundled practice-menu catalog and pure daily-menu assembly.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::analysis::errors::ErrorKind;
use crate::analysis::grades::{Component, Direction, LanguageGrade, Level, ranked_components};
use crate::config::{Config, DrillCell, MenuConfig, ModeKind};
use crate::i18n::Lang;

pub mod live_goal;
pub mod progress;
pub mod skills;

pub const MENU_VERSION: &str = "menu-v1";
const MENU_DATA: &str = include_str!("../../data/menu/menu-v1.toml");

/// Design §4.1: a menu needs this many logged character keystrokes.
pub const MIN_MENU_KEYSTROKES: u64 = 2_000;
/// Design §4.1: below 94.5%, accuracy is always primary A.
pub const ACCURACY_PRIORITY_PERCENT: f64 = 94.5;
/// Design §4.2: offer a benchmark after this many days.
pub const BENCH_REMINDER_DAYS: i64 = 7;
/// §4.2 (MYTASK-3626): no menu step runs longer than this.
pub const MAX_STEP_MINUTES: u8 = 5;
/// §4.2 (MYTASK-3626): a step that types long text (`sentence`,
/// `endurance`) tires the arms sooner, so it is capped lower.
pub const MAX_LONG_TEXT_MINUTES: u8 = 3;
/// §3.2 `speed.pacer` (`target-with-accuracy`): the pacer target counts only
/// at this accuracy or better.
pub const PACER_ACCURACY_FLOOR: f64 = 0.95;

/// Whether a menu mode types long text (`sentence` or `endurance`).
pub fn is_long_text_mode(mode: &str) -> bool {
    matches!(mode, "sentence" | "endurance")
}

/// The longest a step of `mode` may run (§4.2, MYTASK-3626).
pub fn max_step_minutes(mode: &str) -> u8 {
    if is_long_text_mode(mode) {
        MAX_LONG_TEXT_MINUTES
    } else {
        MAX_STEP_MINUTES
    }
}

/// `minutes` cut to the step limit of `mode`. Time cut off is not
/// redistributed (the same rule as catalog-level minutes).
pub fn cap_step_minutes(mode: &str, minutes: u8) -> u8 {
    minutes.min(max_step_minutes(mode))
}

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

    /// A name short enough for the one-line progress band (MYTASK-3627).
    fn short_label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Transition, Lang::En) => "Transition",
            (Self::Blind, Lang::En) => "Blind",
            _ => self.label(lang),
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
    /// `analysis::japanese::spelling_usage` of the step's suggested spelling
    /// (MYTASK-3641).
    JapaneseSpellingUsage,
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
            // MYTASK-3641: the use of the step's suggested spelling.
            Self::SpellingUsageRate => MetricSource::JapaneseSpellingUsage,
            Self::EnduranceSecondHalfSlowdown => MetricSource::EnduranceComparison,
            Self::BlindAssistanceRemovable => MetricSource::HabitsAssistanceRemovable,
            Self::PrescriptionR3 => MetricSource::PrescriptionVerdict,
            Self::NetWpm => MetricSource::SessionNetWpm,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::WordOnsetRatio => "word-onset-ratio",
            Self::WithinWordIkiCv => "within-word-iki-cv",
            Self::PostErrorExcessMs => "post-error-excess-ms",
            Self::RolloverRate => "rollover-rate",
            Self::AccuracyGatePassed => "accuracy-gate-passed",
            Self::AnticipationRate => "anticipation-rate",
            Self::PacerTargetReached => "pacer-target-reached",
            Self::FarHomeIkiRatio => "far-home-iki-ratio",
            Self::SpellingUsageRate => "spelling-usage-rate",
            Self::EnduranceSecondHalfSlowdown => "endurance-second-half-slowdown",
            Self::BlindAssistanceRemovable => "blind-assistance-removable",
            Self::PrescriptionR3 => "prescription-r3",
            Self::NetWpm => "net-wpm",
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
    /// The cell of the individual-training table this level plays
    /// (MYTASK-3640): the skill row and what the prompts are made of. When
    /// `mode` is not the cell's own mode, the level overrides it (see
    /// [`crate::practice::drill::runs_as`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell: Option<DrillCell>,
    pub mode: String,
    /// Minutes of the step (§4.2, MYTASK-3619: the level's minutes are
    /// authoritative). `0` means "not given": the step then takes the frame
    /// default of its slot (4–5 or 7 minutes).
    #[serde(default)]
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

impl MenuRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Warmup => "warmup",
            Self::PrimaryA => "primary-a",
            Self::PrimaryB => "primary-b",
            Self::Check => "check",
            Self::Finish => "finish",
        }
    }
}

/// Key of the `menu_runs.lang` column: the configured practice languages
/// (§4.2 `[menu] languages`), not the languages that happen to appear in the
/// assembled steps. An introductory or 10-minute menu may use only one of two
/// configured languages, and the same day's run must still be found.
pub fn menu_language_key(languages: &[Lang]) -> String {
    languages
        .iter()
        .map(|language| language_code(*language))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(",")
}

/// The `menu_runs.lang` key for the current settings (§8.2).
pub fn menu_run_language_key(config: &Config, display: Lang) -> String {
    menu_language_key(&config.menu.practice_languages(display))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FixedStep {
    pub role: MenuRole,
    pub id: String,
    pub title: LocalizedText,
    /// The cell of the individual-training table this step plays
    /// (MYTASK-3640).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell: Option<DrillCell>,
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
            validate_cell(step.cell, &step.id)?;
            validate_settings(&step.settings, &step.id)?;
            validate_step_minutes(&step.mode, i64::from(step.minutes), &step.id)?;
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
                validate_cell(level.cell, &owner)?;
                validate_settings(&level.settings, &owner)?;
                level.focus.validate(&format!("{owner}.focus"))?;
                level.advance.note.validate(&format!("{owner}.advance.note"))?;
                if level.advance.sessions == 0 {
                    return Err(format!("{owner} has invalid advance sessions"));
                }
                validate_step_minutes(&level.mode, i64::from(level.minutes), &owner)?;
                if let Some(mode_minutes) = mode_duration_setting(&level.settings) {
                    validate_step_minutes(&level.mode, mode_minutes, &format!("{owner} endurance.minutes"))?;
                }
                // §4.2 (MYTASK-3619): a mode's own duration setting must not
                // disagree with the step time.
                if let Some(mode_minutes) = mode_duration_setting(&level.settings)
                    && level.minutes != 0
                    && i64::from(level.minutes) != mode_minutes
                {
                    return Err(format!("{owner} has minutes = {} but endurance.minutes = {mode_minutes}", level.minutes));
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

/// §4.2 (MYTASK-3626): a catalog step longer than its mode's limit is a
/// catalog error, not something to cut silently.
fn validate_step_minutes(mode: &str, minutes: i64, owner: &str) -> Result<(), String> {
    let limit = max_step_minutes(mode);
    if minutes > i64::from(limit) {
        Err(format!("{owner} has {minutes} minutes, over the {limit}-minute limit for {mode} steps"))
    } else {
        Ok(())
    }
}

fn validate_mode(mode: &str, owner: &str) -> Result<(), String> {
    if mode_kind(mode).is_some() {
        Ok(())
    } else {
        Err(format!("unknown mode {mode:?} in {owner}"))
    }
}

/// A catalog cell must be one the table can start (MYTASK-3640).
fn validate_cell(cell: Option<DrillCell>, owner: &str) -> Result<(), String> {
    match cell {
        Some(cell) if !crate::practice::drill::is_available(cell) => Err(format!(
            "cell {} × {} in {owner} cannot be played",
            cell.skill.as_str(),
            cell.unit.as_str()
        )),
        _ => Ok(()),
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

/// The duration a mode keeps in its own setting (`endurance.minutes`), when a
/// level sets it (§4.2, MYTASK-3619).
fn mode_duration_setting(settings: &BTreeMap<String, toml::Value>) -> Option<i64> {
    settings.get("endurance.minutes").and_then(toml::Value::as_integer)
}

/// Minutes of a prescription step (§4.2, MYTASK-3619): the catalog level's
/// `minutes` is authoritative, and a mode's own duration setting
/// (`endurance.minutes`) is kept in agreement with it. `frame` (the slot's
/// 4–5 or 7 minutes) is used only when the level gives neither. The result
/// never exceeds the step limit of the level's mode ([`max_step_minutes`],
/// MYTASK-3626), so the 7-minute frame becomes 5 (3 for long text).
pub fn level_minutes(level: &PrescriptionLevel, frame: u8) -> u8 {
    let minutes = if level.minutes > 0 {
        level.minutes
    } else {
        mode_duration_setting(&level.settings)
            .and_then(|minutes| u8::try_from(minutes).ok())
            .filter(|minutes| *minutes > 0)
            .unwrap_or(frame)
    };
    cap_step_minutes(&level.mode, minutes)
}

/// Setting keys under this prefix carry a step's practice target (§4.2,
/// MYTASK-3618). They are not configuration keys: [`config_for_step`] skips
/// them and the app reads them through [`StepTargets::of`].
pub const TARGET_SETTING_PREFIX: &str = "target.";

/// Apply one menu level to an in-memory configuration. This never writes the
/// user's configuration file.
pub fn config_for_step(base: &Config, mode: &str, settings: &BTreeMap<String, toml::Value>) -> Result<Config, String> {
    let mut value = toml::Value::try_from(base.clone()).map_err(|error| error.to_string())?;
    set_dotted_value(&mut value, "mode.kind", toml::Value::String(mode.to_owned()))?;
    for (key, setting) in settings {
        if key.starts_with(TARGET_SETTING_PREFIX) {
            continue;
        }
        set_dotted_value(&mut value, key, setting.clone())?;
    }
    let raw = toml::to_string(&value).map_err(|error| error.to_string())?;
    let mut config = Config::from_toml(&raw).map_err(|error| error.to_string())?;
    config.menu = base.menu.clone();
    Ok(config)
}

/// The in-memory configuration of a menu step. The level's mode and settings
/// are applied as before (the step records the same mode and settings), in
/// the step's language. A step that runs its cell's mode then makes its
/// prompts the way the cell does on the individual-training table
/// (MYTASK-3640, [`crate::practice::drill::apply_cell_prompts`]); a level
/// that overrides the cell's mode keeps the prompts of its own mode.
pub fn config_for_menu_step(base: &Config, step: &MenuStep) -> Result<Config, String> {
    let mut config = config_for_step(base, &step.mode, &step.settings)?;
    let language = language_code(step.language).to_owned();
    config.adaptive.lang.clone_from(&language);
    config.mode.sentence.lang.clone_from(&language);
    config.mode.phrase.lang = language;
    if let Some(cell) = step_cell(step).filter(|cell| step_runs_cell_mode(step, *cell)) {
        // Every cell a menu step runs this way follows the practice language
        // (`uses_language`), so the step's language stays.
        crate::practice::drill::apply_cell_prompts(&mut config, cell, step.language);
    }
    // §4.2 (MYTASK-3619): the endurance mode's own duration follows the step
    // time, so the halves it compares match the session that was played.
    if config.mode.kind == ModeKind::Endurance && (3..=5).contains(&u64::from(step.minutes)) {
        config.endurance.minutes = u64::from(step.minutes);
    }
    Ok(config)
}

/// The cell of the individual-training table a menu step plays
/// (MYTASK-3640): the catalog's `cell` of its fixed step or level. A step
/// whose catalog entry names no cell (or a stored plan of an unknown entry)
/// gets the cell of its mode ([`crate::practice::drill::cell_for_mode`]).
pub fn step_cell(step: &MenuStep) -> Option<DrillCell> {
    let named = bundled_catalog()
        .ok()
        .and_then(|catalog| catalog_cell(catalog, &step.prescription_id, step.level));
    named.or_else(|| mode_kind(&step.mode).map(crate::practice::drill::cell_for_mode))
}

/// The catalog's cell of a fixed step (`level = None`) or of a level of a
/// prescription.
pub fn catalog_cell(catalog: &MenuCatalog, id: &str, level: Option<u8>) -> Option<DrillCell> {
    match level {
        None => catalog.fixed_steps.iter().find(|step| step.id == id)?.cell,
        Some(level) => catalog.prescription(id)?.levels.get(usize::from(level).checked_sub(1)?)?.cell,
    }
}

/// Whether a menu step runs the mode of its cell (and so starts the way the
/// cell does), or its level overrides the cell's mode (MYTASK-3640).
pub fn step_runs_cell_mode(step: &MenuStep, cell: DrillCell) -> bool {
    mode_kind(&step.mode).is_some_and(|kind| crate::practice::drill::runs_as(cell, kind, step.language))
}

/// The mode the prompt generator runs for a menu step. A Japanese step of a
/// mode that draws from the English word pool (`words`, `ngram`, `unlock`,
/// `rhythm`, `burst`) keeps its recorded mode but is fed Japanese words
/// (`romaji` generation; M3).
pub fn generator_kind_for_step(kind: ModeKind, language: Lang) -> ModeKind {
    if language == Lang::Ja
        && matches!(
            kind,
            ModeKind::Words | ModeKind::Ngram | ModeKind::Unlock | ModeKind::Rhythm | ModeKind::Burst
        )
    {
        ModeKind::Romaji
    } else {
        kind
    }
}

/// Design §4.2 / §11 (MYTASK-3618): a step whose target narrows the word pool
/// keeps the narrowing only while at least this many words contain the
/// target. Below it the narrowing is relaxed (the same bound under which the
/// prompt generator warns that items may repeat).
pub const MIN_TARGET_WORDS: usize = 20;

/// The romaji spelling a `japanese.spelling` step practices (§4.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpellingTarget {
    pub kana: String,
    pub current: String,
    pub candidate: String,
}

impl SpellingTarget {
    /// The same wording as the coach's spelling suggestion.
    pub fn label(&self) -> String {
        format!("{}: {} → {}", self.kana, self.current, self.candidate)
    }
}

/// Per-language targets that prescriptions put into their prompts (§4.2,
/// MYTASK-3618).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LanguageTargets {
    /// The weak transitions `transition.adaptive` selects from the player
    /// model of the language (its focus), as `(from, to)`.
    pub transitions: Vec<(String, String)>,
    /// The suggested romaji spelling, when the analysis has one.
    pub spelling: Option<SpellingTarget>,
}

/// The practice target stored in a step's settings (§4.2, MYTASK-3618).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StepTargets {
    /// Target transitions as the two characters joined (`"th"`).
    pub transitions: Vec<String>,
    pub spelling: Option<SpellingTarget>,
    /// Too few words contained the target, so the word pool is not narrowed.
    pub relaxed: bool,
}

impl StepTargets {
    pub fn of(step: &MenuStep) -> Self {
        let transitions = step
            .settings
            .get("target.transitions")
            .and_then(toml::Value::as_array)
            .map(|values| values.iter().filter_map(toml::Value::as_str).map(str::to_owned).collect())
            .unwrap_or_default();
        let spelling = step.settings.get("target.spelling").and_then(toml::Value::as_table).and_then(|table| {
            let text = |key: &str| table.get(key).and_then(toml::Value::as_str).map(str::to_owned);
            Some(SpellingTarget {
                kana: text("kana")?,
                current: text("current")?,
                candidate: text("candidate")?,
            })
        });
        let relaxed = step.settings.get("target.relaxed").and_then(toml::Value::as_bool).unwrap_or(false);
        Self {
            transitions,
            spelling,
            relaxed,
        }
    }

    fn narrows_words(&self) -> bool {
        !self.transitions.is_empty() || self.spelling.is_some()
    }
}

/// The keys of the remote rows (top and bottom letter rows) of a layout; the
/// target of `reach.unlock` (§4.2, MYTASK-3618).
pub fn remote_row_keys(layout: &crate::layout::KeyboardLayout) -> String {
    const REMOTE_CODES: [&str; 17] = [
        "KeyQ", "KeyW", "KeyE", "KeyR", "KeyT", "KeyY", "KeyU", "KeyI", "KeyO", "KeyP", "KeyZ", "KeyX", "KeyC", "KeyV", "KeyB", "KeyN", "KeyM",
    ];
    let mut keys = String::new();
    for code in REMOTE_CODES {
        if let Some(character) = layout.key(code).map(|pair| pair.normal)
            && character.is_alphabetic()
            && !keys.contains(character)
        {
            keys.push(character);
        }
    }
    keys
}

/// The word pool a step's prompts are drawn from, when its target narrows
/// words (English words, or Japanese words for a Japanese step).
fn target_word_kind(step: &MenuStep) -> Option<crate::data::DataKind> {
    let kind = mode_kind(&step.mode)?;
    match generator_kind_for_step(kind, step.language) {
        ModeKind::Romaji => Some(crate::data::DataKind::JapaneseWord),
        ModeKind::Words | ModeKind::Unlock | ModeKind::Rhythm | ModeKind::Burst => Some(crate::data::DataKind::EnglishWord),
        _ => None,
    }
}

/// Keys (`DataItem::key`) of the words that contain the step's target, or
/// `None` when the step does not narrow its word pool.
fn target_word_keys(catalog: &crate::data::DataCatalog, step: &MenuStep, targets: &StepTargets) -> Option<std::collections::HashSet<String>> {
    if !targets.narrows_words() {
        return None;
    }
    let kind = target_word_kind(step)?;
    let keys = catalog
        .fixed_items(kind)
        .into_iter()
        .filter(|item| {
            let typed = match &item.reading {
                Some(reading) => crate::engine::romaji::RomajiJudge::new(reading).guide().to_owned(),
                None => item.text.clone(),
            };
            let transition = targets.transitions.iter().any(|pair| typed.contains(pair.as_str()));
            let spelling = targets
                .spelling
                .as_ref()
                .is_some_and(|spelling| item.reading.as_deref().is_some_and(|reading| reading.contains(spelling.kana.as_str())));
            transition || spelling
        })
        .map(|item| item.key.clone())
        .collect::<std::collections::HashSet<_>>();
    Some(keys)
}

/// Narrow a catalog to the words that contain a menu step's target (§4.2,
/// MYTASK-3618), reusing the daily-deck narrowing (`retain_kind_keys`). Does
/// nothing when the step has no word target, when the target was relaxed, or
/// when no word contains it. Returns whether the catalog was narrowed.
pub fn restrict_catalog_to_step_targets(catalog: &mut crate::data::DataCatalog, step: &MenuStep) -> bool {
    let targets = StepTargets::of(step);
    if targets.relaxed {
        return false;
    }
    let (Some(kind), Some(keys)) = (target_word_kind(step), target_word_keys(catalog, step, &targets)) else {
        return false;
    };
    if keys.is_empty() {
        return false;
    }
    catalog.retain_kind_keys(kind, &keys);
    true
}

/// Number of prompts the generator would draw a step from, after the target
/// narrowing; the same generator and settings the app uses.
fn target_candidate_count(
    catalog: &crate::data::DataCatalog,
    layout: &crate::layout::KeyboardLayout,
    base: &Config,
    step: &MenuStep,
) -> Result<usize, String> {
    let mut config = config_for_menu_step(base, step)?;
    config.mode.kind = generator_kind_for_step(config.mode.kind, step.language);
    let mut catalog = catalog.clone();
    if let (Some(kind), Some(keys)) = (target_word_kind(step), target_word_keys(&catalog, step, &StepTargets::of(step))) {
        if keys.is_empty() {
            return Ok(0);
        }
        catalog.retain_kind_keys(kind, &keys);
    }
    Ok(crate::engine::modes::ModeGenerator::new(config.mode, catalog, layout.clone(), 0).original_candidate_count())
}

/// Put each prescription's target into its step (§4.2, MYTASK-3618).
/// `targets` is keyed by language code (`"en"`, `"ja"`).
///
/// - `reach.unlock`: the remote-row keys become `mode.keys`, so its `only` /
///   `include` filter prefers words with them;
/// - `transition.adaptive` level ③ (`words`) and `transition.ngram`: the
///   language's target transitions; their keys become `mode.keys` and words
///   are narrowed to the ones containing a target transition;
/// - `japanese.spelling`: words containing the kana of the suggested
///   spelling, and the suggestion is shown in the focus and the reason.
///
/// A word target with fewer than [`MIN_TARGET_WORDS`] matching prompts is
/// relaxed (no narrowing, `include` instead of `only`) and the reason says so.
pub fn apply_step_targets(
    menu: &mut DailyMenu,
    targets: &BTreeMap<String, LanguageTargets>,
    catalog: &crate::data::DataCatalog,
    layout: &crate::layout::KeyboardLayout,
    base: &Config,
    display: Lang,
) -> Result<(), String> {
    let empty = LanguageTargets::default();
    for step in menu
        .steps
        .iter_mut()
        .filter(|step| matches!(step.role, MenuRole::PrimaryA | MenuRole::PrimaryB))
    {
        let language = targets.get(language_code(step.language)).unwrap_or(&empty);
        let mut has_target = false;
        match step.prescription_id.as_str() {
            "reach.unlock" => {
                let keys = remote_row_keys(layout);
                if !keys.is_empty() {
                    step.settings.insert("mode.keys".to_owned(), toml::Value::String(keys));
                    has_target = true;
                }
            }
            "transition.adaptive" | "transition.ngram" if matches!(step.mode.as_str(), "words" | "ngram") => {
                if language.transitions.is_empty() {
                    if step.mode == "words" {
                        // No target yet: do not let the free-play key
                        // selection filter the words instead.
                        step.settings.insert("mode.keys".to_owned(), toml::Value::String(String::new()));
                    }
                    step.reason.push_str(match display {
                        Lang::En => "; no target transition yet",
                        Lang::Ja => "；対象の遷移はまだない",
                    });
                } else {
                    let mut keys = String::new();
                    for character in language.transitions.iter().flat_map(|(from, to)| from.chars().chain(to.chars())) {
                        if !keys.contains(character) {
                            keys.push(character);
                        }
                    }
                    let pairs = language.transitions.iter().map(|(from, to)| format!("{from}{to}")).collect::<Vec<_>>();
                    step.settings.insert("mode.keys".to_owned(), toml::Value::String(keys));
                    step.settings.insert(
                        "target.transitions".to_owned(),
                        toml::Value::Array(pairs.iter().cloned().map(toml::Value::String).collect()),
                    );
                    step.reason.push_str(&match display {
                        Lang::En => format!("; target transitions {}", pairs.join(" ")),
                        Lang::Ja => format!("；対象の遷移 {}", pairs.join(" ")),
                    });
                    has_target = true;
                }
            }
            "japanese.spelling" => {
                if let Some(spelling) = &language.spelling {
                    let label = spelling.label();
                    step.focus.push_str(&match display {
                        Lang::En => format!(" ({label})"),
                        Lang::Ja => format!("（{label}）"),
                    });
                    step.reason.push_str(&match display {
                        Lang::En => format!("; suggested spelling {label}"),
                        Lang::Ja => format!("；勧めた綴り {label}"),
                    });
                    if step.mode == "romaji" {
                        let mut table = toml::map::Map::new();
                        table.insert("kana".to_owned(), toml::Value::String(spelling.kana.clone()));
                        table.insert("current".to_owned(), toml::Value::String(spelling.current.clone()));
                        table.insert("candidate".to_owned(), toml::Value::String(spelling.candidate.clone()));
                        step.settings.insert("target.spelling".to_owned(), toml::Value::Table(table));
                        step.settings
                            .insert("mode.keys".to_owned(), toml::Value::String(spelling.candidate.clone()));
                        has_target = true;
                    }
                }
            }
            _ => {}
        }
        if !has_target || target_word_kind(step).is_none() {
            continue;
        }
        let count = target_candidate_count(catalog, layout, base, step)?;
        if count < MIN_TARGET_WORDS {
            step.settings.insert("target.relaxed".to_owned(), toml::Value::Boolean(true));
            if step.settings.get("mode.key_filter").and_then(toml::Value::as_str) == Some("only") {
                step.settings
                    .insert("mode.key_filter".to_owned(), toml::Value::String("include".to_owned()));
            }
            step.reason.push_str(&match display {
                Lang::En => format!("; only {count} words contain the target, so the narrowing is relaxed"),
                Lang::Ja => format!("；対象を含む語が {count} 語しかないため絞り込みを緩めた"),
            });
        }
    }
    Ok(())
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
    /// §4.1 4 (MYTASK-3621): the keyboard display is shown in the settings
    /// (`keyboard.show` is not `hidden` and `session.blind` is off) and the H5
    /// assistance comparison marks it removable for this language.
    pub blind_assistance_removable: bool,
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
    pub check_language: Option<String>,
    pub primary_prescription_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MenuLanguageInput {
    pub input: MenuBuildInput,
    pub states: BTreeMap<String, PrescriptionState>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrescriptionJudgment {
    #[default]
    None,
    Improved,
    NoChange,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrescriptionProgress {
    #[serde(default = "default_progress_level")]
    pub level: u8,
    #[serde(default)]
    pub consecutive: u8,
    #[serde(default)]
    pub judgment: PrescriptionJudgment,
    #[serde(default)]
    pub paused_until: Option<NaiveDate>,
    #[serde(default)]
    pub maintenance_due: Option<NaiveDate>,
    #[serde(default)]
    pub started_on: Option<NaiveDate>,
    /// The `menu_runs.id` whose prescription step and same-language check
    /// step were last judged together (§4.3/§5). Each run is judged once.
    #[serde(default)]
    pub pair_judged_run: Option<i64>,
    /// Identity (`<prescriptions.id>@<evaluated_at>`) of the last R3 verdict
    /// counted toward graduation, so an old verdict is never counted again.
    #[serde(default)]
    pub r3_counted: Option<String>,
}

const fn default_progress_level() -> u8 {
    1
}

impl Default for PrescriptionProgress {
    fn default() -> Self {
        Self {
            level: 1,
            consecutive: 0,
            judgment: PrescriptionJudgment::None,
            paused_until: None,
            maintenance_due: None,
            started_on: None,
            pair_judged_run: None,
            r3_counted: None,
        }
    }
}

impl PrescriptionProgress {
    pub fn build_state(&self, as_of: NaiveDate) -> PrescriptionState {
        let pause_active = self.paused_until.is_some_and(|until| as_of < until);
        let judgment = if self.paused_until.is_some_and(|until| as_of >= until) {
            PrescriptionJudgment::None
        } else {
            self.judgment
        };
        PrescriptionState {
            level: self.level,
            judgment,
            paused: pause_active,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgressUpdate {
    pub outcome: AdvanceOutcome,
    pub consecutive: u8,
    /// `advance.sessions` of the level that was judged.
    pub required: u8,
    pub level_after: u8,
    /// Set only on the session that graduated the level (§4.3).
    pub graduation: Option<Graduation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Graduation {
    /// Moved on to this level.
    Level(u8),
    /// The last level was graduated; the prescription is now in maintenance.
    Maintenance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferenceJudgment {
    InsufficientData,
    Pending,
    Improved,
    NoChange,
    /// The prescription has no reference judgment (§5, MYTASK-3622).
    NotJudged,
}

/// Design §5: a direction-normalized component score must improve by 0.3.
pub const REFERENCE_IMPROVEMENT_THRESHOLD: f64 = 0.3;
/// Design §5: changes strictly inside ±0.1 count as unchanged.
pub const REFERENCE_NO_CHANGE_BAND: f64 = 0.1;
/// Design §5: unchanged results must continue for five checks.
pub const REFERENCE_NO_CHANGE_RUN: usize = 5;
/// Design §4.3 and §5: maintenance rechecks and pauses last 14 days.
pub const MENU_RECHECK_DAYS: i64 = 14;

pub fn apply_advance_outcome(
    state: &mut PrescriptionProgress,
    level_count: u8,
    required: u8,
    outcome: AdvanceOutcome,
    date: NaiveDate,
) -> ProgressUpdate {
    state.started_on.get_or_insert(date);
    state.level = state.level.max(1).min(level_count.max(1));
    let mut graduation = None;
    match outcome {
        AdvanceOutcome::Met => {
            state.consecutive = state.consecutive.saturating_add(1);
            if state.consecutive >= required.max(1) {
                state.consecutive = 0;
                if state.level < level_count.max(1) {
                    state.level += 1;
                    graduation = Some(Graduation::Level(state.level));
                } else {
                    state.judgment = PrescriptionJudgment::Maintenance;
                    state.maintenance_due = date.checked_add_signed(chrono::Duration::days(MENU_RECHECK_DAYS));
                    graduation = Some(Graduation::Maintenance);
                }
            }
        }
        AdvanceOutcome::NotMet => state.consecutive = 0,
        AdvanceOutcome::CannotDetermine => {}
    }
    ProgressUpdate {
        outcome,
        consecutive: state.consecutive,
        required: required.max(1),
        level_after: state.level,
        graduation,
    }
}

/// Design §5: once a 14-day pause has expired, the prescription starts over
/// as if it had just begun. The "three checks before the prescription"
/// baseline is retaken from `date`, so the reference judgment reports
/// insufficient data until three checks after the pause exist, instead of
/// immediately re-pausing on the checks taken before the pause.
pub fn resume_after_pause(state: &mut PrescriptionProgress, date: NaiveDate) -> bool {
    let Some(until) = state.paused_until else {
        return false;
    };
    if date < until {
        return false;
    }
    state.paused_until = None;
    if state.judgment == PrescriptionJudgment::NoChange {
        state.judgment = PrescriptionJudgment::None;
    }
    state.consecutive = 0;
    state.started_on = Some(date);
    true
}

pub fn apply_maintenance_recheck(state: &mut PrescriptionProgress, date: NaiveDate, regressed: Option<bool>) -> AdvanceOutcome {
    if state.judgment != PrescriptionJudgment::Maintenance || state.maintenance_due.is_none_or(|due| date < due) {
        return AdvanceOutcome::CannotDetermine;
    }
    let Some(regressed) = regressed else {
        return AdvanceOutcome::CannotDetermine;
    };
    state.maintenance_due = None;
    if regressed {
        state.judgment = PrescriptionJudgment::None;
        state.consecutive = 0;
        AdvanceOutcome::NotMet
    } else {
        AdvanceOutcome::Met
    }
}

/// What the reference judgment of a prescription compares (§5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferenceBasis {
    /// `transition.*`: the controlled R3 verdict, not a reference judgment.
    R3,
    /// The 0–8 score of one of the seven components on the check steps.
    ComponentScore(Component),
    /// The raw `advance.metric` on the check steps (`reach.*`, MYTASK-3622).
    RawCheck,
    /// The raw `advance.metric` on the prescription step itself, which is
    /// assisted (`endurance.*`, MYTASK-3622).
    AssistedStep,
    /// Not judged (`japanese.spelling`, `blind.fade`; MYTASK-3622).
    NotJudged,
}

pub fn reference_basis(prescription: &MenuPrescription) -> ReferenceBasis {
    if prescription.component == MenuComponent::Transition {
        return ReferenceBasis::R3;
    }
    if let Some(component) = grade_component_for_menu_component(prescription.component) {
        return ReferenceBasis::ComponentScore(component);
    }
    match prescription.component {
        MenuComponent::Reach | MenuComponent::Endurance => match prescription.levels.first().map(|level| level.advance.source) {
            Some(AdvanceSource::Check) => ReferenceBasis::RawCheck,
            Some(AdvanceSource::Step) => ReferenceBasis::AssistedStep,
            None => ReferenceBasis::NotJudged,
        },
        _ => ReferenceBasis::NotJudged,
    }
}

/// Design §5 (MYTASK-3622): a raw value must improve by 5% in its direction.
pub const RAW_REFERENCE_IMPROVEMENT_RATIO: f64 = 0.05;
/// Design §5 (MYTASK-3622): raw changes strictly inside ±2% count as unchanged.
pub const RAW_REFERENCE_NO_CHANGE_RATIO: f64 = 0.02;

/// Design §5 (MYTASK-3622): the reference judgment on a raw `advance.metric`
/// value for prescriptions outside the seven components. The medians of the
/// three values before the prescription and of the latest three after it are
/// compared as a relative change of the baseline median, signed so that a
/// positive change is an improvement in `direction` (the reach IKI ratio and
/// the endurance second-half slowdown are better when smaller). A zero
/// baseline has no relative change and stays pending.
pub fn raw_reference_judgment(baseline: &[f64], after: &[f64], direction: Direction) -> ReferenceJudgment {
    let baseline = baseline.iter().copied().filter(|value| value.is_finite()).collect::<Vec<_>>();
    let after = after.iter().copied().filter(|value| value.is_finite()).collect::<Vec<_>>();
    if baseline.len() < 3 || after.len() < 3 {
        return ReferenceJudgment::InsufficientData;
    }
    let base = median(&baseline[baseline.len() - 3..]);
    if base.abs() <= f64::EPSILON {
        return ReferenceJudgment::Pending;
    }
    let improvement = |value: f64| match direction {
        Direction::Lower => (base - value) / base.abs(),
        Direction::Higher => (value - base) / base.abs(),
    };
    let recent = median(&after[after.len() - 3..]);
    if improvement(recent) + f64::EPSILON >= RAW_REFERENCE_IMPROVEMENT_RATIO {
        return ReferenceJudgment::Improved;
    }
    if after.len() >= REFERENCE_NO_CHANGE_RUN
        && after[after.len() - REFERENCE_NO_CHANGE_RUN..]
            .iter()
            .all(|value| improvement(*value).abs() < RAW_REFERENCE_NO_CHANGE_RATIO)
    {
        return ReferenceJudgment::NoChange;
    }
    ReferenceJudgment::Pending
}

pub fn reference_judgment(baseline: &[f64], after: &[f64]) -> ReferenceJudgment {
    let baseline = baseline.iter().copied().filter(|value| value.is_finite()).collect::<Vec<_>>();
    let after = after.iter().copied().filter(|value| value.is_finite()).collect::<Vec<_>>();
    if baseline.len() < 3 || after.len() < 3 {
        return ReferenceJudgment::InsufficientData;
    }
    let baseline = median(&baseline[baseline.len() - 3..]);
    let recent = median(&after[after.len() - 3..]);
    if recent - baseline + f64::EPSILON >= REFERENCE_IMPROVEMENT_THRESHOLD {
        return ReferenceJudgment::Improved;
    }
    if after.len() >= REFERENCE_NO_CHANGE_RUN
        && after[after.len() - REFERENCE_NO_CHANGE_RUN..]
            .iter()
            .all(|value| (value - baseline).abs() < REFERENCE_NO_CHANGE_BAND)
    {
        return ReferenceJudgment::NoChange;
    }
    ReferenceJudgment::Pending
}

pub fn apply_reference_judgment(
    state: &mut PrescriptionProgress,
    judgment: ReferenceJudgment,
    alternate_or_only_prescription: bool,
    date: NaiveDate,
) {
    match judgment {
        ReferenceJudgment::Improved => state.judgment = PrescriptionJudgment::Improved,
        ReferenceJudgment::NoChange => {
            state.judgment = PrescriptionJudgment::NoChange;
            if alternate_or_only_prescription {
                state.paused_until = date.checked_add_signed(chrono::Duration::days(MENU_RECHECK_DAYS));
            }
        }
        ReferenceJudgment::InsufficientData | ReferenceJudgment::Pending | ReferenceJudgment::NotJudged => {}
    }
}

fn median(values: &[f64]) -> f64 {
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
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

impl DailyMenu {
    /// The actual length of the menu (§4.2, MYTASK-3619). Prescription steps
    /// take their catalog level's minutes, so this can differ from
    /// `requested_minutes`; the difference is not redistributed.
    pub fn total_minutes(&self) -> u32 {
        self.steps.iter().map(|step| u32::from(step.minutes)).sum()
    }
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
    /// Added only to fill the slots after maintenance was removed (§4.1 5).
    filler: bool,
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
        let selected = previous_language_alternate(&plans, as_of, history, |past| past.language.as_deref())
            .unwrap_or_else(|| alternating_index(as_of, plans.len()));
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

    let warmup = fixed_step(catalog.fixed(MenuRole::Warmup)?, display, a_plan.language, "-");
    let spare_minutes = WARMUP_SLOT_MINUTES.saturating_sub(warmup.minutes);
    let mut steps = vec![warmup];
    let mut primary_a = primary_step(
        catalog,
        a_plan,
        a_candidate,
        MenuRole::PrimaryA,
        primary_minutes(menu.minutes),
        display,
        multi || a_plan.language != display,
    )?;
    // MYTASK-3629: the minutes the warm-up leaves go to primary A, but only
    // when the step stays within its limit; otherwise the menu is shorter.
    if spare_minutes > 0 && primary_a.minutes + spare_minutes <= max_step_minutes(&primary_a.mode) {
        primary_a.minutes += spare_minutes;
    }
    steps.push(primary_a);
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
    let check_index = previous_language_alternate(&plans, as_of, history, |past| past.check_language.as_deref())
        .unwrap_or_else(|| alternating_index(as_of, plans.len()));
    let check_language = plans[check_index].language;
    let mut check = fixed_step(catalog.fixed(MenuRole::Check)?, display, check_language, "-");
    if menu.minutes == 20 {
        // The check step is a `sentence` step: its limit (3) wins over the
        // 20-minute frame's 4, and the minute is not redistributed.
        check.minutes = cap_step_minutes(&check.mode, 4);
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

    // §4.1 5 (MYTASK-3620): a prescription in maintenance is not a candidate
    // (§3.1). Earlier versions only moved it down one place.
    candidates.retain(|component| !component_in_maintenance(catalog, *component, &source.states));
    let fewer_than_two = candidates.len() < 2;
    let weakest_grade_percentile = grade.components.iter().map(|row| row.score * 12.5).min_by(f64::total_cmp);
    if fewer_than_two && input.endurance_break_recommended {
        candidates.insert(0, MenuComponent::Endurance);
    }
    if input.has_spelling_suggestion
        && (fewer_than_two
            || input
                .japanese_percentile
                .zip(weakest_grade_percentile)
                .is_some_and(|(japanese, weakest)| japanese < weakest))
    {
        candidates.insert(0, MenuComponent::Japanese);
    }
    if input.reach_percentile.is_some_and(|percentile| percentile < 25.0)
        && (fewer_than_two
            || input
                .reach_percentile
                .zip(weakest_grade_percentile)
                .is_some_and(|(reach, weakest)| reach < weakest))
    {
        candidates.insert(0, MenuComponent::Reach);
    }
    // §4.1 4 (MYTASK-3621): removing the keyboard display only takes away an
    // aid, so it goes to the front as soon as H5 marks it removable.
    if input.blind_assistance_removable {
        candidates.insert(0, MenuComponent::Blind);
    }

    let language_history = history
        .iter()
        .filter(|past| past.language.as_deref().is_none_or(|value| value == input.language))
        .cloned()
        .collect::<Vec<_>>();
    let repeated = repeated_components(catalog, input.as_of, &language_history);
    for component in MenuComponent::from_grade_components() {
        if repeated.contains(&component) {
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
            filler: false,
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
        if prescription_in_maintenance(&row.0.id, &source.states) {
            continue;
        }
        if selected.iter().all(|selected| selected.component != row.0.component) {
            let overall_gain = opportunities
                .iter()
                .find(|opportunity| MenuComponent::from_grade(opportunity.component) == component)
                .map_or(1.0, |opportunity| opportunity.overall_gain);
            selected.push(CandidateChoice {
                prescription_id: row.0.id.clone(),
                component: row.3,
                overall_gain,
                filler: false,
            });
        }
    }
    // §4.1 5 (MYTASK-3620): when removing maintenance leaves too few
    // candidates, fill with `transition.adaptive`.
    if selected.len() < 2
        && selected.iter().all(|selected| selected.component != MenuComponent::Transition)
        && let Some(row) = available_prescription(catalog, "transition.adaptive", &source.states)
        && !prescription_in_maintenance(&row.0.id, &source.states)
    {
        selected.push(CandidateChoice {
            prescription_id: row.0.id.clone(),
            component: row.3,
            overall_gain: 0.0,
            filler: true,
        });
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
        return previous_language_alternate(plans, as_of, history, |past| past.language.as_deref())
            .unwrap_or_else(|| alternating_index(as_of, plans.len()));
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

fn previous_language_alternate(
    plans: &[LanguagePlan<'_>],
    as_of: NaiveDate,
    history: &[PastMenu],
    language: impl Fn(&PastMenu) -> Option<&str>,
) -> Option<usize> {
    let yesterday = as_of.pred_opt()?;
    let previous = history.iter().find(|past| past.date == yesterday).and_then(language)?;
    plans
        .iter()
        .enumerate()
        .find(|(_, plan)| language_code(plan.language) != previous && plan.sufficient)
        .map(|(index, _)| index)
        .or_else(|| plans.iter().position(|plan| language_code(plan.language) != previous))
}

fn alternating_index(as_of: NaiveDate, count: usize) -> usize {
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).expect("valid epoch");
    as_of.signed_duration_since(epoch).num_days().rem_euclid(count as i64) as usize
}

fn parse_language(language: &str) -> Result<Lang, String> {
    Lang::from_str(language).ok_or_else(|| format!("unknown menu language {language:?}"))
}

pub(crate) fn language_code(language: Lang) -> &'static str {
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
    let warmup = fixed_step(
        catalog.fixed(MenuRole::Warmup)?,
        display,
        practice,
        &reason_with_language(intro_reason, practice, display, show_label),
    );
    let adaptive = available_prescription(catalog, "transition.adaptive", states)
        .or_else(|| available_prescription(catalog, "transition.ngram", states))
        .ok_or_else(|| "no introductory transition prescription is available".to_owned())?;
    // §4.2 (MYTASK-3626): the transition time is split into steps of at most
    // five minutes, alternating with the n-gram drill. The n-gram mode draws
    // from the English word pool only (a Japanese n-gram step would become a
    // plain romaji word list), so a Japanese menu stays on the adaptive drill.
    let ngram = (practice == Lang::En)
        .then(|| available_prescription(catalog, "transition.ngram", states))
        .flatten()
        .unwrap_or(adaptive);
    let reason = match display {
        Lang::En => "Introductory routine: practice weak transitions while collecting baseline data",
        Lang::Ja => "入門ルーチン: 基準データを集めながら弱い遷移を練習",
    };
    let reason = reason_with_language(reason, practice, display, show_label);
    let mut check = fixed_step(catalog.fixed(MenuRole::Check)?, display, practice, &reason);
    check.minutes = cap_step_minutes(&check.mode, INTRODUCTORY_CHECK_MINUTES);
    let mut finish = fixed_step(catalog.fixed(MenuRole::Finish)?, display, practice, &reason);
    finish.minutes = cap_step_minutes(&finish.mode, INTRODUCTORY_FINISH_MINUTES);
    // MYTASK-3629: the transition steps take whatever the warm-up, check and
    // finish leave of the configured length, so the menu adds up to it
    // (15 minutes: 1 + [5, 4] + 3 + 2).
    let transition_total = menu
        .minutes
        .saturating_sub(warmup.minutes)
        .saturating_sub(check.minutes)
        .saturating_sub(finish.minutes);
    let mut steps = vec![warmup];
    for (index, minutes) in introductory_transition_minutes(transition_total).into_iter().enumerate() {
        let selected = if index % 2 == 0 { &adaptive } else { &ngram };
        steps.push(prescription_step_with_reason(
            selected,
            MenuRole::PrimaryA,
            minutes,
            display,
            practice,
            &reason,
        ));
    }
    steps.push(check);
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

/// Minutes of the introductory menu's check step (§4.2).
const INTRODUCTORY_CHECK_MINUTES: u8 = 3;
/// Minutes of the introductory menu's finish step (§4.2).
const INTRODUCTORY_FINISH_MINUTES: u8 = 2;
/// The share of a personalized menu set aside for the warm-up (§4.2). A
/// catalog warm-up shorter than this hands the minutes it leaves to primary A
/// (MYTASK-3629: the warm-up became 1 minute), within the step limit.
const WARMUP_SLOT_MINUTES: u8 = 2;

/// The introductory transition time split into the fewest steps of at most
/// [`MAX_STEP_MINUTES`], as evenly as possible with the longer steps first
/// (§4.2, MYTASK-3626): 3 → [3], 8 → [4, 4], 13 → [5, 4, 4].
pub fn introductory_transition_minutes(total: u8) -> Vec<u8> {
    if total == 0 {
        return Vec::new();
    }
    let count = total.div_ceil(MAX_STEP_MINUTES);
    let (base, extra) = (total / count, total % count);
    (0..count).map(|index| base + u8::from(index < extra)).collect()
}

fn primary_minutes(minutes: u8) -> u8 {
    match minutes {
        10 => 4,
        20 => 7,
        _ => 5,
    }
}

/// The catalog's warm-up as a step of its own, for the stand-alone warm-up of
/// the mode-select screen (MYTASK-3629). It has the same mode, minutes and
/// settings as the menu's warm-up; apply it with [`config_for_menu_step`].
pub fn warmup_step(catalog: &MenuCatalog, display: Lang, practice: Lang) -> Result<MenuStep, String> {
    Ok(fixed_step(catalog.fixed(MenuRole::Warmup)?, display, practice, "-"))
}

fn fixed_step(step: &FixedStep, display: Lang, practice: Lang, reason: &str) -> MenuStep {
    MenuStep {
        role: step.role,
        language: practice,
        prescription_id: step.id.clone(),
        title: step.title.text(display).to_owned(),
        level: None,
        mode: step.mode.clone(),
        minutes: cap_step_minutes(&step.mode, step.minutes),
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
    let mut advance = level.advance.note.text(display).to_owned();
    match reference_basis(prescription) {
        ReferenceBasis::NotJudged => advance.push_str(match display {
            Lang::En => " (no reference judgment)",
            Lang::Ja => "（参考の判定なし）",
        }),
        ReferenceBasis::AssistedStep => advance.push_str(match display {
            Lang::En => " (reference judgment uses this assisted step, so it is weaker still)",
            Lang::Ja => "（参考の判定は補助つきの段の値．参考の度合いがさらに弱い）",
        }),
        ReferenceBasis::R3 | ReferenceBasis::ComponentScore(_) | ReferenceBasis::RawCheck => {}
    }
    MenuStep {
        role,
        language: practice,
        prescription_id: prescription.id.clone(),
        title: prescription.title.text(display).to_owned(),
        level: Some(*level_number),
        mode: level.mode.clone(),
        minutes: cap_step_minutes(&level.mode, minutes),
        reason: reason.to_owned(),
        focus: level.focus.text(display).to_owned(),
        advance,
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
        let mut step = prescription_step(
            &selected,
            role,
            level_minutes(selected.1, minutes),
            grade,
            &plan.source.input,
            StepLanguage {
                display,
                practice: plan.language,
                show_label: show_language,
            },
        );
        if plan
            .source
            .states
            .get(&selected.0.id)
            .is_some_and(|state| state.judgment == PrescriptionJudgment::Improved)
        {
            step.reason.push_str(match display {
                Lang::En => "; improved (reference only; no control)",
                Lang::Ja => "；改善（参考．対照なし）",
            });
            if reference_basis(selected.0) == ReferenceBasis::AssistedStep {
                step.reason.push_str(match display {
                    Lang::En => "; from the assisted step itself",
                    Lang::Ja => "．補助つきの段の値",
                });
            }
        }
        if candidate.filler {
            step.reason.push_str(match display {
                Lang::En => "; fills the slot (too few candidates after removing maintenance)",
                Lang::Ja => "；維持を除くと候補が足りないため遷移で埋める",
            });
        }
        return Ok(step);
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
        (Lang::En, Lang::Ja) => "英語",
        (Lang::Ja, Lang::Ja) => "日本語",
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

pub fn evaluate_advance_measurement(condition: &AdvanceCondition, value: Option<f64>, own_median: Option<f64>) -> AdvanceOutcome {
    let met = match condition.rule {
        AdvanceRule::LeOwnMedian => value.zip(own_median).map(|(value, median)| value <= median),
        AdvanceRule::GeOwnMedian => value.zip(own_median).map(|(value, median)| value >= median),
        AdvanceRule::GatePassed | AdvanceRule::R3Effective | AdvanceRule::TargetWithAccuracy | AdvanceRule::Removable => {
            value.map(|value| value >= 1.0)
        }
        AdvanceRule::GeThreshold => value.zip(condition.threshold).map(|(value, threshold)| value >= threshold),
        AdvanceRule::LtThreshold => value.zip(condition.threshold).map(|(value, threshold)| value < threshold),
    };
    match met {
        Some(true) => AdvanceOutcome::Met,
        Some(false) => AdvanceOutcome::NotMet,
        None => AdvanceOutcome::CannotDetermine,
    }
}

pub fn evaluate_advance_from_sources(
    condition: &AdvanceCondition,
    step_value: Option<f64>,
    check_value: Option<f64>,
    own_median: Option<f64>,
) -> AdvanceOutcome {
    let value = match condition.source {
        AdvanceSource::Step => step_value,
        AdvanceSource::Check => check_value,
    };
    evaluate_advance_measurement(condition, value, own_median)
}

pub fn grade_component_for_metric(metric: MenuMetric) -> Option<Component> {
    Some(match metric {
        MenuMetric::WordOnsetRatio => Component::Preview,
        MenuMetric::WithinWordIkiCv => Component::Rhythm,
        MenuMetric::PostErrorExcessMs => Component::Recovery,
        MenuMetric::RolloverRate => Component::Overlap,
        _ => return None,
    })
}

pub fn grade_component_for_menu_component(component: MenuComponent) -> Option<Component> {
    Some(match component {
        MenuComponent::Speed => Component::Speed,
        MenuComponent::Accuracy => Component::Accuracy,
        MenuComponent::Rhythm => Component::Rhythm,
        MenuComponent::Transition => Component::Transition,
        MenuComponent::Preview => Component::Preview,
        MenuComponent::Recovery => Component::Recovery,
        MenuComponent::Overlap => Component::Overlap,
        MenuComponent::Reach | MenuComponent::Japanese | MenuComponent::Endurance | MenuComponent::Blind => return None,
    })
}

pub fn is_alternate_or_only_prescription(catalog: &MenuCatalog, prescription_id: &str) -> bool {
    let Some(prescription) = catalog.prescription(prescription_id) else {
        return false;
    };
    default_prescription_id(prescription.component).is_some_and(|default| default != prescription_id) || prescription.alternate == prescription_id
}

pub fn current_metric_value(metric: MenuMetric, record: &crate::record::SessionRecord) -> Option<f64> {
    match metric {
        MenuMetric::NetWpm => Some(record.header.net_wpm),
        MenuMetric::AccuracyGatePassed => serde_json::from_str::<serde_json::Value>(&record.header.mode_params)
            .ok()
            .and_then(|params| params.get("score_eligible").and_then(serde_json::Value::as_bool))
            .map(|met| if met { 1.0 } else { 0.0 }),
        MenuMetric::PacerTargetReached => serde_json::from_str::<serde_json::Value>(&record.header.mode_params)
            .ok()
            .and_then(|params| params.get("pacer_target_wpm").and_then(serde_json::Value::as_f64))
            .map(|target| {
                if record.header.accuracy >= PACER_ACCURACY_FLOOR && record.header.net_wpm >= target {
                    1.0
                } else {
                    0.0
                }
            }),
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
        if matches!(state.judgment, PrescriptionJudgment::Ineffective | PrescriptionJudgment::NoChange) {
            id = &prescription.alternate;
            continue;
        }
        let level_number = state.level.max(1).min(prescription.levels.len() as u8);
        let level = prescription.levels.get(usize::from(level_number - 1))?;
        return Some((prescription, level, level_number, prescription.component));
    }
}

fn prescription_in_maintenance(id: &str, states: &BTreeMap<String, PrescriptionState>) -> bool {
    states.get(id).is_some_and(|state| state.judgment == PrescriptionJudgment::Maintenance)
}

/// The prescription a component would use is in maintenance (§3.1, §4.1 5).
fn component_in_maintenance(catalog: &MenuCatalog, component: MenuComponent, states: &BTreeMap<String, PrescriptionState>) -> bool {
    let Some(id) = default_prescription_id(component) else { return false };
    available_prescription(catalog, id, states).is_some_and(|row| prescription_in_maintenance(&row.0.id, states))
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
    use crate::analysis::progress::{ProgressOptions, analyze_progress};
    use crate::analysis::summary::{SkillAxis, skill_axis_values};
    use crate::data::{DataCatalog, TrainingDataSource};
    use crate::engine::session::KeystrokeKind;
    use crate::error::AppError;
    use crate::layout::builtin_layout;
    use crate::layout::geometry::geometry;
    use crate::record::db;

    let practice_languages = config.menu.practice_languages(display);
    let language_key = menu_language_key(&practice_languages);
    if let Some(run) = db::find_menu_run_for_day(path, as_of, &config.keyboard.layout, &language_key, MENU_VERSION, config.menu.minutes)? {
        return serde_json::from_str(&run.plan_json).map_err(Into::into);
    }
    let history = db::load_menu_runs_before(path, as_of, &config.keyboard.layout, MENU_VERSION)?
        .into_iter()
        .flat_map(|run| past_menus_from_run(&run))
        .collect::<Vec<_>>();

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
    let references = reference_prompts(&corpus, &layout, &keyboard_geometry);
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
    let prescription_rows = db::load_prescriptions(path)?;
    let excluded_controls = crate::analysis::prescription::prescription_trends(&prescription_rows, at).controls;
    let judge_context = progress::JudgeContext {
        db_path: path,
        home,
        config,
        layout: &layout,
        as_of,
    };
    // §4.1 4 (MYTASK-3621): `blind.fade` is a candidate only while the
    // keyboard display is actually shown.
    let keyboard_shown = config.keyboard.show != "hidden" && !config.session.blind;
    let mut language_targets = BTreeMap::<String, LanguageTargets>::new();
    let r3_ineffective = prescription_rows
        .iter()
        .rev()
        .find(|row| progress::is_r3_adaptive_kind(&row.kind))
        .is_some_and(|row| row.status == "ineffective");
    let mut inputs = Vec::new();
    for practice in practice_languages {
        let language = language_code(practice);
        let mut states = db::load_menu_progresses(path, &config.keyboard.layout, language)?
            .into_iter()
            .map(|(id, progress)| (id, progress.build_state(as_of)))
            .collect::<BTreeMap<_, _>>();
        if r3_ineffective {
            states.insert(
                "transition.adaptive".to_owned(),
                PrescriptionState {
                    judgment: PrescriptionJudgment::Ineffective,
                    ..PrescriptionState::default()
                },
            );
        }
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
        language_targets.insert(
            language.to_owned(),
            LanguageTargets {
                transitions: target_transitions(&details, &corpus, practice, config, &excluded_controls),
                spelling: crate::analysis::japanese::suggested_spelling(&diagnosis.japanese.spelling).map(|row| SpellingTarget {
                    kana: row.kana.clone(),
                    current: row.current.clone(),
                    candidate: row.candidate.clone(),
                }),
            },
        );
        let blind_assistance_removable =
            keyboard_shown && progress::blind_assistance_value(&judge_context, language)?.is_some_and(|value| value >= 1.0);
        let reaction_median_ms = (details.reaction_median_ms > 0.0).then_some(details.reaction_median_ms);
        let reaction_ms = language_records.last().and_then(|record| {
            let details = analyze_records(std::slice::from_ref(record), &layout, &keyboard_geometry, model);
            (details.reaction_median_ms > 0.0).then_some(details.reaction_median_ms)
        });
        let v3_group = match language {
            "en" => crate::analysis::bench_v3::BenchV3Group::English,
            "ja" => crate::analysis::bench_v3::BenchV3Group::Japanese,
            _ => unreachable!("menu languages are validated"),
        };
        let last_bench_date = crate::analysis::bench_v3::compute_bench_v3_group_runs(&language_records, v3_group, config.analysis.pause_ms)
            .last()
            .map(|run| run.date)
            .or_else(|| {
                language_records
                    .iter()
                    .rev()
                    .find(|record| record.header.bench_id.is_some() && !record.header.bench_id.as_deref().is_some_and(crate::bench::is_v3_bench_id))
                    .and_then(|record| chrono::DateTime::parse_from_rfc3339(&record.header.started_at).ok())
                    .map(|time| time.with_timezone(&chrono::Local).date_naive())
            });
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
                blind_assistance_removable,
                reaction_ms,
                reaction_median_ms,
                last_bench_date,
            },
            states,
        });
    }
    let mut menu =
        build_menu_for_languages(bundled_catalog().map_err(AppError::Record)?, &inputs, display, &config.menu, &history).map_err(AppError::Record)?;
    apply_step_targets(&mut menu, &language_targets, &corpus, &layout, config, display).map_err(AppError::Record)?;
    Ok(menu)
}

/// The weak transitions `transition.adaptive` selects for one language (its
/// focus, §4.2 MYTASK-3618), computed by the adaptive planner with a fixed
/// seed so the day's menu is reproducible. Transitions held out as R3
/// controls are never targets, and only letter-to-letter transitions are
/// kept (words and n-gram drills cannot target a space).
fn target_transitions(
    details: &crate::analysis::metrics::DetailedMetrics,
    corpus: &crate::data::DataCatalog,
    language: Lang,
    config: &Config,
    excluded_controls: &[(String, String)],
) -> Vec<(String, String)> {
    use crate::data::TrainingDataSource;
    use crate::practice::adaptive::{AdaptiveHistory, AdaptiveOptions, build_plan_seeded};

    let words = match language {
        Lang::En => corpus.words_en().to_vec(),
        Lang::Ja => corpus
            .words_ja()
            .iter()
            .map(|item| crate::engine::romaji::RomajiJudge::new(&item.reading).guide().to_owned())
            .collect(),
    };
    let plan = build_plan_seeded(
        details,
        &words,
        AdaptiveOptions {
            error_weight_ms: config.adaptive.error_weight,
            drill_ratio: config.adaptive.drill_ratio,
            min_transition_types: 50,
            min_transition_n: config.analysis.min_transition_n,
            max_share: config.adaptive.max_share,
        },
        &[],
        0,
        &AdaptiveHistory {
            excluded_controls: excluded_controls.to_vec(),
            ..AdaptiveHistory::default()
        },
    );
    if plan.fallback_to_words {
        return Vec::new();
    }
    let letter = |value: &str| {
        let mut characters = value.chars();
        characters.next().is_some_and(char::is_alphabetic) && characters.next().is_none()
    };
    plan.focus().into_iter().filter(|(from, to)| letter(from) && letter(to)).collect()
}

/// Bench reference prompts used to correct WPM for difficulty (shared by the
/// menu assembly and the H5 assistance analysis used by `blind.fade`).
pub(crate) fn reference_prompts(
    corpus: &crate::data::DataCatalog,
    layout: &crate::layout::KeyboardLayout,
    geometry: &crate::layout::geometry::KeyboardGeometry,
) -> crate::analysis::progress::ReferencePrompts {
    use crate::bench::{BenchPart, generate_version};
    let reference_version = |part| generate_version(part, 1, corpus, layout, geometry);
    crate::analysis::progress::ReferencePrompts {
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
    }
}

fn past_menus_from_run(run: &crate::record::db::MenuRunRecord) -> Vec<PastMenu> {
    let Ok(menu) = serde_json::from_str::<DailyMenu>(&run.plan_json) else {
        return Vec::new();
    };
    let primary_a = menu.steps.iter().find(|step| step.role == MenuRole::PrimaryA);
    let check_language = menu
        .steps
        .iter()
        .find(|step| step.role == MenuRole::Check)
        .map(|step| language_code(step.language).to_owned());
    let mut languages = Vec::new();
    if let Some(step) = primary_a {
        languages.push(step.language);
    }
    for step in menu
        .steps
        .iter()
        .filter(|step| matches!(step.role, MenuRole::PrimaryA | MenuRole::PrimaryB))
    {
        if !languages.contains(&step.language) {
            languages.push(step.language);
        }
    }
    languages
        .into_iter()
        .map(|language| PastMenu {
            date: menu.date,
            language: Some(language_code(language).to_owned()),
            check_language: check_language.clone(),
            primary_prescription_ids: menu
                .steps
                .iter()
                .filter(|step| matches!(step.role, MenuRole::PrimaryA | MenuRole::PrimaryB) && step.language == language)
                .map(|step| step.prescription_id.clone())
                .collect(),
        })
        .collect()
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
        Ok(render_text(&menu, lang, config.bench.series))
    }
}

/// "13 min (set: 15)" — the actual total, with `[menu] minutes` only when
/// the two differ (§4.2, MYTASK-3619).
pub fn minutes_label(menu: &DailyMenu, lang: Lang) -> String {
    let total = menu.total_minutes();
    let requested = u32::from(menu.requested_minutes);
    match (lang, total == requested) {
        (Lang::En, true) => format!("{total} min"),
        (Lang::En, false) => format!("{total} min (set: {requested})"),
        (Lang::Ja, true) => format!("{total} 分"),
        (Lang::Ja, false) => format!("{total} 分（設定 {requested} 分）"),
    }
}

pub fn benchmark_minutes(series: crate::config::BenchSeries) -> u8 {
    match series {
        crate::config::BenchSeries::V2 => 4,
        crate::config::BenchSeries::V3 => 5,
    }
}

fn render_text(menu: &DailyMenu, lang: Lang, series: crate::config::BenchSeries) -> String {
    use std::fmt::Write;

    let mut output = String::new();
    match lang {
        Lang::En => {
            let _ = writeln!(output, "Today's practice menu — {}", minutes_label(menu, lang));
            if menu.bench_recommended {
                let _ = writeln!(output, "Benchmark recommended first (about {} minutes).", benchmark_minutes(series));
            }
        }
        Lang::Ja => {
            let _ = writeln!(output, "今日の練習メニュー — {}", minutes_label(menu, lang));
            if menu.bench_recommended {
                let _ = writeln!(output, "先にベンチ（約 {} 分）を勧めます．", benchmark_minutes(series));
            }
        }
    }
    for (index, step) in menu.steps.iter().enumerate() {
        let level = step.level.map_or_else(|| "-".to_owned(), |level| level.to_string());
        let cell = step_cell(step)
            .map(|cell| crate::ui::mode_select::cell_label(cell, lang))
            .unwrap_or_else(|| "-".to_owned());
        let briefing = crate::practice::briefing::resolve_menu_step(step).ok();
        let purpose = briefing.as_ref().map_or("-", |entry| entry.purpose.text(lang));
        let goal = briefing.as_ref().map_or("-", |entry| entry.goal.text(lang));
        let content = briefing.as_ref().map_or("-", |entry| entry.content.text(lang));
        let evidence = match (step.evidence.as_deref(), step.evidence_note.as_deref()) {
            (Some(strength), Some(note)) if !note.is_empty() && lang == Lang::Ja => format!("{strength}（{note}）"),
            (Some(strength), Some(note)) if !note.is_empty() => format!("{strength} ({note})"),
            (Some(strength), _) => strength.to_owned(),
            (None, Some(note)) if !note.is_empty() && lang == Lang::Ja => format!("-（{note}）"),
            (None, Some(note)) if !note.is_empty() => format!("- ({note})"),
            (None, _) => "-".to_owned(),
        };
        match lang {
            Lang::En => {
                let _ = writeln!(output, "\nStep: {} {}", index + 1, role_label(step.role, lang));
                let _ = writeln!(output, "Language: {}", language_row_label(step.language, lang));
                let _ = writeln!(output, "Prescription ID: {}", step.prescription_id);
                let _ = writeln!(output, "Title: {}", step.title);
                let _ = writeln!(output, "Level: {level}");
                let _ = writeln!(output, "Mode: {}", step.mode);
                let _ = writeln!(output, "Time: {} min", step.minutes);
                let _ = writeln!(output, "Drill: {cell}");
                let _ = writeln!(output, "Purpose: {purpose}");
                let _ = writeln!(output, "Goal: {goal}");
                let _ = writeln!(output, "Content: {content}");
                let _ = writeln!(output, "Focus: {}", step.focus);
                let _ = writeln!(output, "Graduation condition: {}", step.advance);
                let _ = writeln!(output, "Selected because: {}", step.reason);
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
                let _ = writeln!(output, "訓練: {cell}");
                let _ = writeln!(output, "目的: {purpose}");
                let _ = writeln!(output, "達成すること: {goal}");
                let _ = writeln!(output, "訓練の内容: {content}");
                let _ = writeln!(output, "意識すること: {}", step.focus);
                let _ = writeln!(output, "卒業の条件: {}", step.advance);
                let _ = writeln!(output, "選んだ理由: {}", step.reason);
                let _ = writeln!(output, "根拠の強さ: {evidence}");
            }
        }
    }
    output
}

fn language_row_label(practice: Lang, display: Lang) -> &'static str {
    match (practice, display) {
        (Lang::En, Lang::En) => "English",
        (Lang::Ja, Lang::En) => "Japanese",
        (Lang::En, Lang::Ja) => "英語",
        (Lang::Ja, Lang::Ja) => "日本語",
    }
}

/// A short name of a step for the progress band of today's training
/// (MYTASK-3627): the practice component of a prescription step («遷移»,
/// «Accuracy»), the role of a fixed step («ウォームアップ», «Check»).
pub fn step_short_label(step: &MenuStep, lang: Lang) -> String {
    if matches!(step.role, MenuRole::PrimaryA | MenuRole::PrimaryB)
        && let Some(prescription) = bundled_catalog().ok().and_then(|catalog| catalog.prescription(&step.prescription_id))
    {
        return prescription.component.short_label(lang).to_owned();
    }
    role_label(step.role, lang).to_owned()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_menu_uses_the_briefing_fields_in_the_requested_order() {
        let prescription = bundled_catalog().unwrap().prescription("accuracy.gate").unwrap();
        let level = &prescription.levels[0];
        let selected = (prescription, level, 1, prescription.component);
        let menu = DailyMenu {
            date: NaiveDate::from_ymd_opt(2026, 11, 25).unwrap(),
            language: "en".to_owned(),
            catalog_version: MENU_VERSION.to_owned(),
            requested_minutes: 4,
            introductory: false,
            bench_recommended: false,
            steps: vec![prescription_step_with_reason(
                &selected,
                MenuRole::PrimaryA,
                4,
                Lang::En,
                Lang::En,
                "Accuracy needs attention.",
            )],
        };

        let output = render_text(&menu, Lang::En, crate::config::BenchSeries::V3);
        let headings = [
            "Drill:",
            "Purpose:",
            "Goal:",
            "Content:",
            "Focus:",
            "Graduation condition:",
            "Selected because:",
            "Evidence strength:",
        ];
        let positions = headings.map(|heading| output.find(heading).unwrap_or_else(|| panic!("missing {heading:?}:\n{output}")));
        assert!(
            positions.windows(2).all(|pair| pair[0] < pair[1]),
            "wrong field order: {positions:?}\n{output}"
        );
        let evidence = output.lines().find(|line| line.starts_with("Evidence strength:")).unwrap();
        assert!(
            evidence.contains('(') && evidence.ends_with(')'),
            "evidence note must share the line: {evidence}"
        );
    }
}
