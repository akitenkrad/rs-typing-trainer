//! Recording finished menu steps and judging prescriptions (design §4.3, §5).
//!
//! The assembly in the parent module is pure; this module is the storage side
//! of M4. It writes `menu_steps`, updates the per-prescription state in `meta`
//! in the same transaction, and applies three kinds of judgment:
//!
//! - graduation from a step's own value (`advance.source = "step"`) or from
//!   the existing R3 verdict (`transition.*`), when the step finishes;
//! - graduation from the same run's check step (`advance.source = "check"`)
//!   and the uncontrolled reference judgment (§5) for every non-transition
//!   prescription, once per run, as soon as both the prescription step and the
//!   check step of the same language have been recorded, in either order;
//! - the 14-day maintenance recheck (§4.3) when a check step finishes.

use std::path::Path;

use chrono::NaiveDate;

use super::{
    AdvanceCondition, AdvanceOutcome, AdvanceSource, DailyMenu, MenuCatalog, MenuMetric, MenuRole, MenuStep, PrescriptionJudgment,
    PrescriptionProgress, ProgressUpdate, ReferenceBasis, ReferenceJudgment, apply_advance_outcome, apply_maintenance_recheck,
    apply_reference_judgment, bundled_catalog, current_metric_value, evaluate_advance_measurement, grade_component_for_menu_component,
    grade_component_for_metric, is_alternate_or_only_prescription, language_code, raw_reference_judgment, reference_judgment, resume_after_pause,
};
use crate::analysis::grades::{Component, ComponentGrade, GradeOptions, component_grades_for_session};
use crate::analysis::metrics::AnalysisSettings;
use crate::analysis::model::Prescription;
use crate::config::Config;
use crate::error::{AppError, AppResult};
use crate::i18n::Lang;
use crate::layout::KeyboardLayout;
use crate::layout::geometry::KeyboardGeometry;
use crate::record::SessionRecord;
use crate::record::db::{self, MenuStepAdvance, MenuStepRecord, MenuStepWrite};

/// Everything the judgments read besides the database rows.
pub struct JudgeContext<'a> {
    pub db_path: &'a Path,
    /// Home directory used to load the bundled training data.
    pub home: &'a Path,
    pub config: &'a Config,
    /// The practice layout that the sessions were typed with.
    pub layout: &'a KeyboardLayout,
    /// The menu day (local date).
    pub as_of: NaiveDate,
}

/// A judgment applied to one prescription step during this call.
#[derive(Clone, Debug, PartialEq)]
pub struct StepJudgment {
    /// Zero-based index of the prescription step in the run's plan.
    pub step_index: usize,
    pub prescription_id: String,
    pub title: String,
    /// Graduation progress, when the graduation condition was judged now.
    pub progress: Option<ProgressUpdate>,
    /// Reference judgment (§5), when it was taken now.
    pub reference: Option<ReferenceJudgment>,
}

/// Why a finished prescription step was not judged yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JudgmentDeferral {
    /// The same-language check step of this run has not been played yet.
    AwaitingCheck,
    /// Today's check step is in another language (§4.2 rotation), so this
    /// prescription is judged on a later day.
    CheckLanguage(Lang),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StepCompletionReport {
    /// The step had already been recorded in this run; nothing was counted.
    pub already_recorded: bool,
    /// Judgment of the step that just finished (its own prescription).
    pub own: Option<StepJudgment>,
    /// Judgments of other steps made now (when the check step finishes).
    pub others: Vec<StepJudgment>,
    pub deferred: Option<JudgmentDeferral>,
}

/// Record one finished menu step of `run_id` and apply every judgment that
/// became possible. `record` must already be saved (it needs its id).
pub fn record_step_completion(ctx: &JudgeContext<'_>, run_id: i64, index: usize, record: &SessionRecord) -> AppResult<StepCompletionReport> {
    let run = db::load_menu_run(ctx.db_path, run_id)?.ok_or_else(|| AppError::Record(format!("menu run {run_id} does not exist")))?;
    let menu = serde_json::from_str::<DailyMenu>(&run.plan_json)?;
    let step = menu
        .steps
        .get(index)
        .cloned()
        .ok_or_else(|| AppError::Record(format!("menu run {run_id} has no step {}", index + 1)))?;
    let session_id = record
        .header
        .id
        .ok_or_else(|| AppError::Record("a menu step session must be saved before it is recorded".to_owned()))?;
    if db::load_menu_steps(ctx.db_path, run_id)?.iter().any(|row| row.idx == index + 1) {
        return Ok(StepCompletionReport {
            already_recorded: true,
            ..StepCompletionReport::default()
        });
    }
    let catalog = bundled_catalog().map_err(AppError::Record)?;
    let language = language_code(step.language);
    let condition = step.advance_condition.clone();
    let mut row = MenuStepRecord {
        run_id,
        idx: index + 1,
        role: step.role.as_str().to_owned(),
        prescription_id: Some(step.prescription_id.clone()),
        level: step.level,
        session_id,
        metric: condition.as_ref().map(|condition| condition.metric.as_str().to_owned()),
        // The H5 mark is a comparison across sessions, not a value of this
        // step; it is read only when the check step judges `blind.fade`.
        value: condition
            .as_ref()
            .filter(|condition| condition.metric != MenuMetric::BlindAssistanceRemovable)
            .and_then(|condition| session_metric_value(ctx, condition.metric, record, language).ok().flatten()),
        advance_met: None,
        level_after: step.level,
    };

    let mut own_progress = None;
    if let Some(prescription) = is_prescription_step(&step).then(|| catalog.prescription(&step.prescription_id)).flatten() {
        let level_count = level_count(prescription.levels.len());
        let r3 = if condition.as_ref().is_some_and(|condition| condition.metric == MenuMetric::PrescriptionR3) {
            latest_r3_verdict(&db::load_prescriptions(ctx.db_path)?)
        } else {
            None
        };
        let as_of = ctx.as_of;
        own_progress = db::update_menu_progress_and_step(ctx.db_path, &ctx.config.keyboard.layout, language, &step.prescription_id, |state| {
            resume_after_pause(state, as_of);
            state.started_on.get_or_insert(as_of);
            let progress = condition.as_ref().and_then(|condition| {
                if condition.metric == MenuMetric::PrescriptionR3 {
                    // Count an R3 verdict only once: a verdict that was
                    // already counted is "cannot determine" until R3 closes
                    // another evaluation.
                    let outcome = match &r3 {
                        Some((identity, outcome)) if state.r3_counted.as_deref() != Some(identity.as_str()) => {
                            state.r3_counted = Some(identity.clone());
                            *outcome
                        }
                        _ => AdvanceOutcome::CannotDetermine,
                    };
                    Some(apply_advance_outcome(state, level_count, condition.sessions, outcome, as_of))
                } else if condition.source == AdvanceSource::Step {
                    let outcome = evaluate_advance_measurement(condition, row.value, None);
                    Some(apply_advance_outcome(state, level_count, condition.sessions, outcome, as_of))
                } else {
                    None
                }
            });
            if let Some(progress) = progress {
                row.advance_met = advance_met(progress.outcome);
                row.level_after = Some(progress.level_after);
            }
            (progress, MenuStepWrite::Insert(row.clone()))
        })?;
    } else {
        db::record_menu_step(ctx.db_path, &row)?;
    }

    let judged = judge_pairs(ctx, run_id, &menu, catalog)?;
    if step.role == MenuRole::Check {
        recheck_maintenance(ctx, run_id, language, catalog)?;
    }

    let mut report = StepCompletionReport::default();
    let mut own = own_progress.map(|progress| StepJudgment {
        step_index: index,
        prescription_id: step.prescription_id.clone(),
        title: step.title.clone(),
        progress: Some(progress),
        reference: None,
    });
    for judgment in judged {
        if judgment.step_index == index {
            let entry = own.get_or_insert_with(|| StepJudgment {
                progress: None,
                reference: None,
                ..judgment.clone()
            });
            entry.progress = entry.progress.or(judgment.progress);
            entry.reference = judgment.reference;
        } else {
            report.others.push(judgment);
        }
    }
    if own.as_ref().is_none_or(|own| own.progress.is_none())
        && is_prescription_step(&step)
        && condition.as_ref().is_some_and(|condition| condition.source == AdvanceSource::Check)
        && let Some(check) = menu.steps.iter().find(|row| row.role == MenuRole::Check)
    {
        report.deferred = Some(if check.language == step.language {
            JudgmentDeferral::AwaitingCheck
        } else {
            JudgmentDeferral::CheckLanguage(check.language)
        });
    }
    report.own = own;
    Ok(report)
}

fn is_prescription_step(step: &MenuStep) -> bool {
    matches!(step.role, MenuRole::PrimaryA | MenuRole::PrimaryB)
}

fn level_count(levels: usize) -> u8 {
    levels.clamp(1, usize::from(u8::MAX)) as u8
}

fn advance_met(outcome: AdvanceOutcome) -> Option<bool> {
    match outcome {
        AdvanceOutcome::Met => Some(true),
        AdvanceOutcome::NotMet => Some(false),
        AdvanceOutcome::CannotDetermine => None,
    }
}

/// R3 rows written by the adaptive mode. M3 looked for `"adaptive"`; the
/// rows the adaptive mode actually writes use `"adaptive-transition"`.
pub fn is_r3_adaptive_kind(kind: &str) -> bool {
    matches!(kind, "adaptive" | "adaptive-transition")
}

/// The latest adaptive R3 row, when it has closed with a verdict, together
/// with an identity that changes only when R3 closes a new evaluation.
pub fn latest_r3_verdict(rows: &[Prescription]) -> Option<(String, AdvanceOutcome)> {
    let row = rows.iter().rev().find(|row| is_r3_adaptive_kind(&row.kind))?;
    let outcome = match row.status.as_str() {
        "effective" => AdvanceOutcome::Met,
        "ineffective" => AdvanceOutcome::NotMet,
        _ => return None,
    };
    let identity = format!(
        "{}@{}",
        row.id.map_or_else(|| "-".to_owned(), |id| id.to_string()),
        row.evaluated_at.map_or_else(|| row.created_at.to_rfc3339(), |at| at.to_rfc3339())
    );
    Some((identity, outcome))
}

/// Check-step values of one language: earlier runs and the current run.
struct CheckSeries {
    language: &'static str,
    previous: Vec<CheckPoint>,
    current: Option<CheckPoint>,
}

struct CheckPoint {
    date: NaiveDate,
    record: SessionRecord,
    grades: Vec<ComponentGrade>,
}

impl CheckSeries {
    fn load(ctx: &JudgeContext<'_>, language: &'static str, run_id: i64) -> AppResult<Self> {
        let rows = db::load_menu_check_records(ctx.db_path, language, run_id)?;
        let data = crate::data::DataCatalog::load(&crate::config::DataConfig::default(), ctx.home)?;
        let mut previous = Vec::new();
        let mut current = None;
        for row in rows {
            let grades = check_grades(ctx, &row.record, &data, language);
            let point = CheckPoint {
                date: row.date,
                record: row.record,
                grades,
            };
            if row.run_id == run_id {
                current = Some(point);
            } else {
                previous.push(point);
            }
        }
        Ok(Self { language, previous, current })
    }

    fn score(point: &CheckPoint, component: Component) -> Option<f64> {
        point
            .grades
            .iter()
            .find(|grade| grade.component == component)
            .map(|grade| grade.score)
            .filter(|score| score.is_finite())
    }

    /// Direction-normalized 0–8 scores split at the prescription start:
    /// checks before it are the baseline, checks on or after it (including
    /// the current run) are the "after" series.
    fn split_scores(&self, component: Component, started: NaiveDate) -> (Vec<f64>, Vec<f64>) {
        let mut baseline = Vec::new();
        let mut after = Vec::new();
        for point in self.previous.iter().chain(self.current.iter()) {
            let Some(score) = Self::score(point, component) else { continue };
            if point.date < started {
                baseline.push(score);
            } else {
                after.push(score);
            }
        }
        (baseline, after)
    }

    fn previous_score_median(&self, component: Component) -> Option<f64> {
        median(self.previous.iter().filter_map(|point| Self::score(point, component)).collect())
    }

    fn metric(&self, ctx: &JudgeContext<'_>, metric: MenuMetric, point: &CheckPoint) -> AppResult<Option<f64>> {
        if let Some(component) = grade_component_for_metric(metric) {
            return Ok(point
                .grades
                .iter()
                .find(|grade| grade.component == component)
                .map(|grade| grade.value)
                .filter(|value| value.is_finite()));
        }
        session_metric_value(ctx, metric, &point.record, self.language)
    }
}

fn check_grades(ctx: &JudgeContext<'_>, record: &SessionRecord, data: &crate::data::DataCatalog, language: &str) -> Vec<ComponentGrade> {
    use crate::data::TrainingDataSource;

    let Some(geometry) = record_geometry(record) else {
        return Vec::new();
    };
    component_grades_for_session(
        record,
        ctx.layout,
        &geometry,
        GradeOptions {
            model: analysis_settings(ctx.config),
        },
        end_of_day(ctx.as_of),
        data.words_en(),
        data.words_ja(),
        language,
    )
    .unwrap_or_default()
}

fn record_geometry(record: &SessionRecord) -> Option<KeyboardGeometry> {
    crate::layout::geometry::geometry(&record.header.geometry).or_else(|| crate::layout::geometry::geometry("ansi"))
}

fn analysis_settings(config: &Config) -> AnalysisSettings {
    AnalysisSettings {
        pause_ms: config.analysis.pause_ms,
        shrinkage_k: config.analysis.shrinkage_k,
        half_life_days: config.analysis.half_life_days,
    }
}

fn end_of_day(date: NaiveDate) -> chrono::DateTime<chrono::Utc> {
    date.and_hms_opt(23, 59, 59).expect("valid end of day").and_utc()
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    values.retain(|value| value.is_finite());
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    })
}

/// The value of `metric` for one session. Missing data is `None` — never 0 —
/// so a judgment on it is "cannot determine" (§5: insufficient data never
/// switches or pauses a prescription).
pub fn session_metric_value(ctx: &JudgeContext<'_>, metric: MenuMetric, record: &SessionRecord, language: &str) -> AppResult<Option<f64>> {
    if let Some(component) = grade_component_for_metric(metric) {
        let data = crate::data::DataCatalog::load(&crate::config::DataConfig::default(), ctx.home)?;
        return Ok(check_grades(ctx, record, &data, language)
            .into_iter()
            .find(|grade| grade.component == component)
            .map(|grade| grade.value)
            .filter(|value| value.is_finite()));
    }
    Ok(match metric {
        MenuMetric::AnticipationRate => record_geometry(record).and_then(|geometry| {
            let diagnosis = crate::analysis::errors::diagnose_errors(
                std::slice::from_ref(record),
                ctx.layout,
                &geometry,
                analysis_settings(ctx.config),
                end_of_day(ctx.as_of),
            );
            anticipation_rate(&diagnosis)
        }),
        // The reach axis of this one session. `skill_axis_values` reads only
        // benchmark sessions, so a check step never had a value (MYTASK-3622).
        MenuMetric::FarHomeIkiRatio => {
            crate::analysis::summary::session_axis_value(crate::analysis::summary::SkillAxis::Reach, record, ctx.layout, ctx.config)
                .filter(|value| value.is_finite())
        }
        MenuMetric::BlindAssistanceRemovable => blind_assistance_value(ctx, language)?,
        _ => current_metric_value(metric, record).filter(|value| value.is_finite()),
    })
}

/// Share of anticipation errors among first errors. With no first error the
/// share is undefined (0/0) and therefore `None`; with first errors but no
/// anticipation among them it is a measured 0.
pub fn anticipation_rate(diagnosis: &crate::analysis::errors::ErrorDiagnosis) -> Option<f64> {
    use crate::analysis::errors::ErrorKind;

    let first_errors = diagnosis
        .kinds
        .iter()
        .filter(|row| row.kind != ErrorKind::Consecutive)
        .map(|row| row.n)
        .sum::<u64>();
    (first_errors > 0).then(|| diagnosis.kind_count(ErrorKind::Anticipation) as f64 / first_errors as f64)
}

/// `blind.fade` (§3.2): the H5 assistance comparison marks the on-screen
/// keyboard "removable" when at least five sessions with and five without it
/// exist and form without it is at least 97% of form with it. Returns 1.0 for
/// removable, 0.0 for compared-but-not-removable and `None` while H5 lacks
/// the sessions to compare.
pub fn blind_assistance_value(ctx: &JudgeContext<'_>, language: &str) -> AppResult<Option<f64>> {
    use crate::analysis::habits::AssistanceKind;
    use crate::analysis::progress::{ProgressOptions, analyze_progress};

    let records = db::load_records_filtered(ctx.db_path, None, None)?
        .into_iter()
        .filter(|record| crate::analysis::model::session_language(record) == language)
        .collect::<Vec<_>>();
    if records.is_empty() {
        return Ok(None);
    }
    let geometry = crate::layout::geometry::geometry(&ctx.config.keyboard.geometry)
        .or_else(|| crate::layout::geometry::geometry("ansi"))
        .expect("bundled ansi geometry");
    let data = crate::data::DataCatalog::load(&crate::config::DataConfig::default(), ctx.home)?;
    let references = super::reference_prompts(&data, ctx.layout, &geometry);
    let progress = analyze_progress(
        &records,
        ctx.layout,
        &geometry,
        ProgressOptions {
            model: analysis_settings(ctx.config),
            min_transition_n: ctx.config.analysis.min_transition_n,
        },
        end_of_day(ctx.as_of),
        &references,
    );
    let forms = progress.forms.into_iter().filter(|form| form.language == language).collect::<Vec<_>>();
    let habits = crate::analysis::habits::analyze_habits(&records, &forms);
    Ok(habits
        .assistance
        .iter()
        .find(|effect| effect.kind == AssistanceKind::Keyboard)
        .and_then(assistance_removable_value))
}

/// 1.0 / 0.0 once H5 compared both conditions, `None` before that.
pub fn assistance_removable_value(effect: &crate::analysis::habits::AssistanceEffect) -> Option<f64> {
    let compared = effect.with_n >= 5 && effect.without_n >= 5 && effect.with_form_median.is_some() && effect.without_form_median.is_some();
    compared.then_some(if effect.removable { 1.0 } else { 0.0 })
}

/// Judge every prescription step of the run whose same-language check step
/// is recorded and that was not yet judged in this run (§4.3, §5).
fn judge_pairs(ctx: &JudgeContext<'_>, run_id: i64, menu: &DailyMenu, catalog: &MenuCatalog) -> AppResult<Vec<StepJudgment>> {
    let recorded = db::load_menu_steps(ctx.db_path, run_id)?;
    let Some((check_index, check_step)) = menu.steps.iter().enumerate().find(|(_, step)| step.role == MenuRole::Check) else {
        return Ok(Vec::new());
    };
    if !recorded.iter().any(|row| row.idx == check_index + 1) {
        return Ok(Vec::new());
    }
    let language = language_code(check_step.language);
    let states = db::load_menu_progresses(ctx.db_path, &ctx.config.keyboard.layout, language)?;
    let pairs = menu
        .steps
        .iter()
        .enumerate()
        .filter(|(index, step)| {
            is_prescription_step(step)
                && step.language == check_step.language
                && recorded.iter().any(|row| row.idx == index + 1)
                && step.advance_condition.is_some()
                && catalog.prescription(&step.prescription_id).is_some()
                && states
                    .get(&step.prescription_id)
                    .is_none_or(|state| state.pair_judged_run != Some(run_id))
        })
        .collect::<Vec<_>>();
    if pairs.is_empty() {
        return Ok(Vec::new());
    }
    let checks = CheckSeries::load(ctx, language, run_id)?;
    let Some(current) = checks.current.as_ref() else {
        return Ok(Vec::new());
    };
    let mut judged = Vec::new();
    for (index, step) in pairs {
        let prescription = catalog.prescription(&step.prescription_id).expect("filtered above");
        let condition: &AdvanceCondition = step.advance_condition.as_ref().expect("filtered above");
        let is_r3 = condition.metric == MenuMetric::PrescriptionR3;
        let basis = super::reference_basis(prescription);
        // §5 (MYTASK-3622): dated raw values for prescriptions outside the
        // seven components, split at the prescription start below.
        let raw = raw_reference_series(ctx, &checks, basis, condition.metric, &step.prescription_id)?;
        let advance = if condition.source == AdvanceSource::Check && !is_r3 {
            let value = checks.metric(ctx, condition.metric, current)?;
            let mut previous = Vec::new();
            if matches!(condition.rule, super::AdvanceRule::LeOwnMedian | super::AdvanceRule::GeOwnMedian) {
                for point in &checks.previous {
                    if let Some(value) = checks.metric(ctx, condition.metric, point)? {
                        previous.push(value);
                    }
                }
            }
            let own_median = median(previous);
            Some((value, evaluate_advance_measurement(condition, value, own_median)))
        } else {
            None
        };
        let alternate = is_alternate_or_only_prescription(catalog, &step.prescription_id);
        let levels = level_count(prescription.levels.len());
        let as_of = ctx.as_of;
        let judgment = db::update_menu_progress_and_step(ctx.db_path, &ctx.config.keyboard.layout, language, &step.prescription_id, |state| {
            if state.pair_judged_run == Some(run_id) {
                return (None, MenuStepWrite::Nothing);
            }
            state.pair_judged_run = Some(run_id);
            resume_after_pause(state, as_of);
            let started = *state.started_on.get_or_insert(as_of);
            // §5: transitions are judged by R3, everything else by the
            // uncontrolled reference judgment on the check-step scores.
            let reference = (!is_r3 && state.judgment != PrescriptionJudgment::Maintenance).then(|| {
                let judgment = match basis {
                    ReferenceBasis::ComponentScore(component) => {
                        let (baseline, after) = checks.split_scores(component, started);
                        reference_judgment(&baseline, &after)
                    }
                    ReferenceBasis::RawCheck | ReferenceBasis::AssistedStep => {
                        let (baseline, after) = raw.split(started);
                        raw_reference_judgment(&baseline, &after, condition.metric.direction())
                    }
                    ReferenceBasis::NotJudged => ReferenceJudgment::NotJudged,
                    ReferenceBasis::R3 => ReferenceJudgment::InsufficientData,
                };
                // Swaps and pauses follow the same rules as the seven
                // components (§5).
                apply_reference_judgment(state, judgment, alternate, as_of);
                judgment
            });
            let progress = advance.map(|(_, outcome)| apply_advance_outcome(state, levels, condition.sessions, outcome, as_of));
            let write = match (progress, advance) {
                (Some(progress), Some((value, _))) => MenuStepWrite::Advance(MenuStepAdvance {
                    run_id,
                    idx: index + 1,
                    metric: Some(condition.metric.as_str().to_owned()),
                    value,
                    advance_met: advance_met(progress.outcome),
                    level_after: Some(progress.level_after),
                }),
                _ => MenuStepWrite::Nothing,
            };
            (
                Some(StepJudgment {
                    step_index: index,
                    prescription_id: step.prescription_id.clone(),
                    title: step.title.clone(),
                    progress,
                    reference,
                }),
                write,
            )
        })?;
        judged.extend(judgment);
    }
    Ok(judged)
}

/// Dated raw values for the raw-value reference judgment (§5, MYTASK-3622).
#[derive(Default)]
struct RawSeries {
    /// Candidates for the baseline (only those before the start are used).
    before: Vec<(NaiveDate, f64)>,
    /// Candidates for the "after" series (only those on or after the start).
    after: Vec<(NaiveDate, f64)>,
}

impl RawSeries {
    fn split(&self, started: NaiveDate) -> (Vec<f64>, Vec<f64>) {
        (
            self.before.iter().filter(|(date, _)| *date < started).map(|(_, value)| *value).collect(),
            self.after.iter().filter(|(date, _)| *date >= started).map(|(_, value)| *value).collect(),
        )
    }
}

/// - `RawCheck` (reach): the metric on the check steps, before and after.
/// - `AssistedStep` (endurance): the metric on endurance sessions of the
///   language before the start, and on this prescription's own steps after it
///   (`advance.source = "step"`; the steps are assisted, so the judgment is
///   only a weak reference).
///
/// Missing values are skipped, never 0.
fn raw_reference_series(
    ctx: &JudgeContext<'_>,
    checks: &CheckSeries,
    basis: ReferenceBasis,
    metric: MenuMetric,
    prescription_id: &str,
) -> AppResult<RawSeries> {
    let mut series = RawSeries::default();
    match basis {
        ReferenceBasis::RawCheck => {
            for point in checks.previous.iter().chain(checks.current.iter()) {
                if let Some(value) = checks.metric(ctx, metric, point)? {
                    series.before.push((point.date, value));
                    series.after.push((point.date, value));
                }
            }
        }
        ReferenceBasis::AssistedStep => {
            for record in db::load_records_filtered(ctx.db_path, None, None)?
                .iter()
                .filter(|record| crate::analysis::model::session_language(record) == checks.language)
            {
                let Some(date) = chrono::DateTime::parse_from_rfc3339(&record.header.started_at)
                    .ok()
                    .map(|time| time.date_naive())
                else {
                    continue;
                };
                let params = serde_json::from_str::<serde_json::Value>(&record.header.mode_params).unwrap_or_default();
                let own_step = params.get("menu_prescription_id").and_then(serde_json::Value::as_str) == Some(prescription_id);
                if !own_step && record.header.mode != "endurance" {
                    continue;
                }
                let Some(value) = session_metric_value(ctx, metric, record, checks.language)? else {
                    continue;
                };
                if own_step {
                    series.after.push((date, value));
                } else {
                    series.before.push((date, value));
                }
            }
        }
        ReferenceBasis::R3 | ReferenceBasis::ComponentScore(_) | ReferenceBasis::NotJudged => {}
    }
    Ok(series)
}

/// §4.3: a prescription in maintenance is checked once more 14 days after
/// it graduated. If the check step's score is below the median of the
/// earlier check steps, it resumes from its last level.
fn recheck_maintenance(ctx: &JudgeContext<'_>, run_id: i64, language: &'static str, catalog: &MenuCatalog) -> AppResult<()> {
    let due = db::load_menu_progresses(ctx.db_path, &ctx.config.keyboard.layout, language)?
        .into_iter()
        .filter(|(_, state): &(String, PrescriptionProgress)| {
            state.judgment == PrescriptionJudgment::Maintenance && state.maintenance_due.is_some_and(|due| ctx.as_of >= due)
        })
        .collect::<Vec<_>>();
    if due.is_empty() {
        return Ok(());
    }
    let checks = CheckSeries::load(ctx, language, run_id)?;
    let Some(current) = checks.current.as_ref() else {
        return Ok(());
    };
    for (prescription_id, _) in due {
        let Some(component) = catalog
            .prescription(&prescription_id)
            .and_then(|prescription| grade_component_for_menu_component(prescription.component))
        else {
            continue;
        };
        let regressed = CheckSeries::score(current, component)
            .zip(checks.previous_score_median(component))
            .map(|(current, median)| current < median);
        let as_of = ctx.as_of;
        db::update_menu_progress(ctx.db_path, &ctx.config.keyboard.layout, language, &prescription_id, |state| {
            apply_maintenance_recheck(state, as_of, regressed)
        })?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Readers for the menu summary screen (§7.1, upper and lower sections)
// ---------------------------------------------------------------------------

/// One planned step of a run as the summary screen shows it (§7.1 upper
/// section). Every value comes from `menu_steps` and the prescription state;
/// nothing is recomputed from keystrokes.
#[derive(Clone, Debug, PartialEq)]
pub struct StepSummary {
    /// Zero-based index in the run's plan.
    pub index: usize,
    pub role: MenuRole,
    pub language: Lang,
    pub prescription_id: String,
    pub title: String,
    pub level: Option<u8>,
    /// Planned minutes of the step.
    pub minutes: u8,
    /// Number of levels of the prescription (`None` for fixed steps).
    pub level_count: Option<u8>,
    /// The step has been played in this run.
    pub done: bool,
    /// The value shown: the catalog's `advance.metric` for prescription steps,
    /// `None` (net WPM) for warm-up, check and finish.
    pub metric: Option<MenuMetric>,
    pub value: Option<f64>,
    /// The same value from the latest earlier run of the same prescription at
    /// the same level in the same language.
    pub previous: Option<f64>,
    pub direction: crate::analysis::grades::Direction,
    /// `advance.sessions` of the level that was played.
    pub required: Option<u8>,
    /// Consecutive sessions meeting the condition so far (prescription state).
    pub consecutive: Option<u8>,
    /// Set when this run graduated the level.
    pub graduation: Option<super::Graduation>,
}

impl StepSummary {
    pub fn difference(&self) -> Option<f64> {
        self.value.zip(self.previous).map(|(value, previous)| value - previous)
    }
}

/// A change made by a run (§7.1 lower section), in the order the screen
/// lists them: level changes, swaps and pauses, then judgments.
#[derive(Clone, Debug, PartialEq)]
pub enum RunChange {
    LevelUp {
        title: String,
        language: Lang,
        level: u8,
    },
    LevelDown {
        title: String,
        language: Lang,
        level: u8,
    },
    Maintenance {
        title: String,
        language: Lang,
    },
    /// §5: the prescription is replaced by its alternate from the next menu.
    /// `controlled` is true for R3 (transition) judgments.
    /// `assisted` marks a reference judgment taken on the assisted
    /// prescription step itself (`endurance.*`, MYTASK-3622).
    Swapped {
        title: String,
        language: Lang,
        alternate: String,
        controlled: bool,
        assisted: bool,
    },
    /// §5: the prescription (an alternate) rests until `until`.
    Paused {
        title: String,
        language: Lang,
        until: NaiveDate,
        assisted: bool,
    },
    /// §5: «improved» — controlled (R3) or reference only.
    Improved {
        title: String,
        language: Lang,
        controlled: bool,
        assisted: bool,
    },
    /// §5 (MYTASK-3622): the prescription has no reference judgment
    /// (`japanese.spelling`, `blind.fade`).
    NoReference {
        title: String,
        language: Lang,
    },
}

impl RunChange {
    /// Priority group of §7.1: level changes, then swaps and pauses, then
    /// judgments.
    pub fn group(&self) -> u8 {
        match self {
            Self::LevelUp { .. } | Self::LevelDown { .. } | Self::Maintenance { .. } => 0,
            Self::Swapped { .. } | Self::Paused { .. } => 1,
            Self::Improved { .. } | Self::NoReference { .. } => 2,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RunStepsSummary {
    pub run_id: i64,
    pub date: NaiveDate,
    pub minutes: u8,
    pub bench_recommended: bool,
    pub steps: Vec<StepSummary>,
    pub changes: Vec<RunChange>,
}

impl RunStepsSummary {
    pub fn completed(&self) -> usize {
        self.steps.iter().filter(|step| step.done).count()
    }

    /// The run's check step, when it has been played.
    pub fn check_language(&self) -> Option<Lang> {
        self.steps
            .iter()
            .find(|step| step.role == MenuRole::Check && step.done)
            .map(|step| step.language)
    }
}

/// The steps and changes of `run_id` for the summary screen. `display`
/// localizes the alternate titles. `None` when the run does not exist.
pub fn run_steps_summary(db_path: &Path, layout: &str, run_id: i64, display: Lang) -> AppResult<Option<RunStepsSummary>> {
    let Some(run) = db::load_menu_run(db_path, run_id)? else {
        return Ok(None);
    };
    let menu = serde_json::from_str::<DailyMenu>(&run.plan_json)?;
    let catalog = bundled_catalog().map_err(AppError::Record)?;
    let history = db::load_menu_step_history(db_path)?;
    let mut plans = std::collections::BTreeMap::<i64, Option<DailyMenu>>::new();
    plans.insert(run.id, Some(menu.clone()));
    // The language a played step was actually typed in: the language of its
    // session (as the skill series and every analysis classify it), falling
    // back to the run's plan when the session row is missing. Previous-run
    // differences only compare steps of the same language (§4.2: the check
    // step rotates daily and two-language menus mix languages by step).
    let mut session_languages = std::collections::BTreeMap::<i64, Option<Lang>>::new();
    let mut step_language = |row: &db::MenuStepHistoryRow| -> Option<Lang> {
        let session = *session_languages.entry(row.step.session_id).or_insert_with(|| {
            db::load_record(db_path, row.step.session_id)
                .ok()
                .flatten()
                .map(|record| match crate::analysis::model::session_language(&record) {
                    "ja" => Lang::Ja,
                    _ => Lang::En,
                })
        });
        if session.is_some() {
            return session;
        }
        let plan = plans.entry(row.run_id).or_insert_with(|| {
            db::load_menu_run(db_path, row.run_id)
                .ok()
                .flatten()
                .and_then(|run| serde_json::from_str::<DailyMenu>(&run.plan_json).ok())
        });
        plan.as_ref()
            .and_then(|plan| plan.steps.get(row.step.idx.checked_sub(1)?))
            .map(|step| step.language)
    };
    let earlier = |row: &db::MenuStepHistoryRow| (row.date, row.run_id) < (run.date, run.id);
    let mut steps = Vec::new();
    let mut changes = Vec::new();
    for (index, step) in menu.steps.iter().enumerate() {
        let row = history.iter().find(|row| row.run_id == run.id && row.step.idx == index + 1);
        let prescription = is_prescription_step(step).then(|| catalog.prescription(&step.prescription_id)).flatten();
        let metric = prescription.and(step.advance_condition.as_ref()).map(|condition| condition.metric);
        let value_of = |row: &db::MenuStepHistoryRow| match metric {
            Some(_) => row.step.value,
            None => row.net_wpm,
        };
        let value = row.and_then(value_of).filter(|value| value.is_finite());
        let current_language = row.and_then(&mut step_language).unwrap_or(step.language);
        let mut previous = None;
        for candidate in history.iter().filter(|candidate| earlier(candidate)) {
            if candidate.layout == run.layout
                && candidate.step.prescription_id.as_deref() == Some(step.prescription_id.as_str())
                && candidate.step.level == step.level
                && step_language(candidate) == Some(current_language)
                && let Some(value) = value_of(candidate).filter(|value| value.is_finite())
            {
                previous = Some(value);
            }
        }
        let language = language_code(step.language);
        let state = match prescription {
            Some(_) => db::load_menu_progress(db_path, layout, language, &step.prescription_id)?,
            None => None,
        };
        let required = prescription
            .and(step.advance_condition.as_ref())
            .map(|condition| condition.sessions.max(1));
        let level_count = prescription.map(|prescription| level_count(prescription.levels.len()));
        let graduation = row.and_then(|row| {
            let (level, after) = (row.step.level?, row.step.level_after?);
            if after > level {
                return Some(super::Graduation::Level(after));
            }
            let state = state.as_ref()?;
            (row.step.advance_met == Some(true)
                && state.judgment == PrescriptionJudgment::Maintenance
                && state.maintenance_due == run.date.checked_add_signed(chrono::Duration::days(super::MENU_RECHECK_DAYS)))
            .then_some(super::Graduation::Maintenance)
        });
        let consecutive = match (graduation, required) {
            (Some(_), Some(required)) => Some(required),
            _ => state
                .as_ref()
                .filter(|state| Some(state.level) == step.level && state.judgment != PrescriptionJudgment::Maintenance)
                .map(|state| state.consecutive),
        };
        if let Some(row) = row {
            match graduation {
                Some(super::Graduation::Level(level)) => changes.push(RunChange::LevelUp {
                    title: step.title.clone(),
                    language: step.language,
                    level,
                }),
                Some(super::Graduation::Maintenance) => changes.push(RunChange::Maintenance {
                    title: step.title.clone(),
                    language: step.language,
                }),
                None => {
                    if let (Some(level), Some(after)) = (row.step.level, row.step.level_after)
                        && after < level
                    {
                        changes.push(RunChange::LevelDown {
                            title: step.title.clone(),
                            language: step.language,
                            level: after,
                        });
                    }
                }
            }
            if let Some(prescription) = prescription {
                let alternate = catalog
                    .prescription(&prescription.alternate)
                    .filter(|alternate| alternate.id != prescription.id)
                    .map(|alternate| alternate.title.text(display).to_owned());
                if metric == Some(MenuMetric::PrescriptionR3) {
                    match row.step.advance_met {
                        Some(false) => {
                            if let Some(alternate) = alternate {
                                changes.push(RunChange::Swapped {
                                    title: step.title.clone(),
                                    language: step.language,
                                    alternate,
                                    controlled: true,
                                    assisted: false,
                                });
                            }
                        }
                        Some(true) => changes.push(RunChange::Improved {
                            title: step.title.clone(),
                            language: step.language,
                            controlled: true,
                            assisted: false,
                        }),
                        None => {}
                    }
                } else if let Some(state) = state.as_ref().filter(|state| state.pair_judged_run == Some(run.id)) {
                    let basis = super::reference_basis(prescription);
                    let assisted = basis == super::ReferenceBasis::AssistedStep;
                    if basis == super::ReferenceBasis::NotJudged {
                        changes.push(RunChange::NoReference {
                            title: step.title.clone(),
                            language: step.language,
                        });
                    } else if let Some(until) = state.paused_until.filter(|until| *until > run.date) {
                        changes.push(RunChange::Paused {
                            title: step.title.clone(),
                            language: step.language,
                            until,
                            assisted,
                        });
                    } else if state.judgment == PrescriptionJudgment::NoChange {
                        if let Some(alternate) = alternate {
                            changes.push(RunChange::Swapped {
                                title: step.title.clone(),
                                language: step.language,
                                alternate,
                                controlled: false,
                                assisted,
                            });
                        }
                    } else if state.judgment == PrescriptionJudgment::Improved {
                        changes.push(RunChange::Improved {
                            title: step.title.clone(),
                            language: step.language,
                            controlled: false,
                            assisted,
                        });
                    }
                }
            }
        }
        steps.push(StepSummary {
            index,
            role: step.role,
            language: step.language,
            prescription_id: step.prescription_id.clone(),
            title: step.title.clone(),
            level: step.level,
            minutes: step.minutes,
            level_count,
            done: row.is_some(),
            metric,
            value,
            previous,
            direction: metric.map_or(crate::analysis::grades::Direction::Higher, MenuMetric::direction),
            required,
            consecutive,
            graduation,
        });
    }
    changes.sort_by_key(RunChange::group);
    Ok(Some(RunStepsSummary {
        run_id: run.id,
        date: run.date,
        minutes: run.minutes,
        bench_recommended: menu.bench_recommended,
        steps,
        changes,
    }))
}
