//! Skill snapshots from the check step and their trends (design §8.1, §8.3).
//!
//! The check step (role `check`) is the daily small measurement: every
//! assistance is off, so its seven component values can be compared across
//! days. This module
//!
//! - computes the raw component values of one menu run (§8.1, pure:
//!   [`compute_run_snapshots`]);
//! - writes them to `skill_snapshots`, replacing the run's rows in one
//!   transaction ([`record_run_snapshots`], [`rebuild_all_snapshots`]);
//! - derives the previous-run difference, the 7-run moving median, the
//!   typical change and the 28-day trend of one series (§8.3, pure:
//!   [`skill_trend`]);
//! - renders `stats --skills` ([`render_from_storage`]).
//!
//! Only raw values are stored. Grades and 0–8 scores are computed on display
//! with the current thresholds (`grades-v1`, §8.2).

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use super::MenuRole;
use crate::analysis::grades::{
    Component, ComponentSample, Direction, GRADE_VERSION, GradeOptions, Level, bundled_thresholds, component_samples_for_sessions, grade_component,
};
use crate::analysis::metrics::AnalysisSettings;
use crate::analysis::model::session_language;
use crate::analysis::progress::{ProgressOptions, SessionForm, analyze_progress};
use crate::data::{JapaneseText, TrainingDataSource};
use crate::error::AppResult;
use crate::i18n::{Lang, Msg};
use crate::layout::KeyboardLayout;
use crate::layout::geometry::KeyboardGeometry;
use crate::practice::menu::progress::JudgeContext;
use crate::record::SessionRecord;
use crate::record::db::{self, MenuRunRecord, SkillSnapshotRecord};

/// §8.3: the moving median covers the latest 7 runs, the current one included.
pub const MOVING_MEDIAN_WINDOW: usize = 7;
/// §8.3: with fewer than 5 runs the moving median is not shown.
pub const MOVING_MEDIAN_MIN_POINTS: usize = 5;
/// §8.3: the typical change is the median absolute value of the latest 20
/// previous-run differences.
pub const TYPICAL_CHANGE_WINDOW: usize = 20;
/// Not in the design (§8.3 gives no lower bound): the typical change needs at
/// least 4 previous-run differences (5 runs, the same amount of data the
/// moving median needs), otherwise it is `None` and nothing is judged
/// against it.
pub const TYPICAL_CHANGE_MIN_DIFFERENCES: usize = 4;
/// §8.3: the trend fits the points of the latest 28 days (the current date
/// and the 27 days before it, as the existing 4-week bench slope).
pub const TREND_WINDOW_DAYS: i64 = 28;
/// §8.3: the trend needs points in at least 2 calendar (ISO) weeks.
pub const TREND_MIN_CALENDAR_WEEKS: usize = 2;
/// §8.3: up, flat and down are split at a quarter of the typical change per week.
pub const TREND_THRESHOLD_FRACTION_OF_TYPICAL: f64 = 0.25;

/// Where a snapshot's keystrokes come from (§8.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SnapshotSource {
    /// The check step alone.
    #[serde(rename = "check")]
    Check,
    /// The check step together with the run's non-prescription steps of the
    /// same language (transition and recovery).
    #[serde(rename = "check+steps")]
    CheckSteps,
}

impl SnapshotSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::CheckSteps => "check+steps",
        }
    }

    /// The source a component is always measured from (§8.1).
    pub fn for_component(component: Component) -> Self {
        match component {
            Component::Transition | Component::Recovery => Self::CheckSteps,
            _ => Self::Check,
        }
    }
}

/// One raw component value of one menu run (§8.1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SkillSnapshot {
    pub run_id: i64,
    pub date: NaiveDate,
    /// Language of the check step (`en` / `ja`).
    pub language: String,
    pub component: Component,
    pub raw_value: f64,
    /// Observations behind `raw_value` (meaning per component on
    /// [`ComponentSample`]: keystrokes for speed and accuracy, intervals or
    /// pairs for the others).
    pub n: u64,
    pub source: SnapshotSource,
    /// Speed only: `true` when `raw_value` is the difficulty-corrected WPM,
    /// `false` when it is the raw net WPM. Always `false` for the others.
    pub corrected: bool,
}

impl SkillSnapshot {
    pub fn to_record(&self) -> SkillSnapshotRecord {
        SkillSnapshotRecord {
            run_id: self.run_id,
            date: self.date,
            lang: self.language.clone(),
            component: self.component.as_str().to_owned(),
            raw_value: self.raw_value,
            n: self.n,
            source: self.source.as_str().to_owned(),
            corrected: self.corrected,
        }
    }
}

/// Everything [`compute_run_snapshots`] reads.
pub struct SnapshotInputs<'a> {
    pub run_id: i64,
    /// The menu day of the run.
    pub date: NaiveDate,
    /// The check step's session.
    pub check: &'a SessionRecord,
    /// Sessions of the same run's steps other than the check step and the
    /// prescription steps (warm-up, finish). Sessions in a language other
    /// than the check step's are ignored.
    pub other_steps: &'a [SessionRecord],
    /// Difficulty-corrected WPM of the check session («form × expected WPM
    /// of the reference bench»), when it could be computed.
    pub corrected_wpm: Option<f64>,
    pub layout: &'a KeyboardLayout,
    pub geometry: &'a KeyboardGeometry,
    pub settings: AnalysisSettings,
    /// `[analysis].min_transition_n`: transition and recovery need at least
    /// this many observations.
    pub min_transition_n: u64,
    pub english_corpus: &'a [String],
    pub japanese_corpus: &'a [JapaneseText],
}

/// Raw values of the seven components of one menu run (§8.1).
///
/// - Speed, accuracy, rhythm, preview and overlap come from the check step
///   alone (`source = check`). Speed is the difficulty-corrected WPM when
///   available (`corrected = true`), otherwise the raw net WPM.
/// - Transition and recovery come from the check step plus the same run's
///   non-prescription steps of the same language (`source = check+steps`),
///   and are left out when fewer than `min_transition_n` observations exist.
/// - A component that cannot be computed is left out — never stored as 0.
pub fn compute_run_snapshots(inputs: &SnapshotInputs<'_>) -> Vec<SkillSnapshot> {
    let language = session_language(inputs.check);
    let combined = std::iter::once(inputs.check)
        .chain(inputs.other_steps.iter().filter(|record| session_language(record) == language))
        .cloned()
        .collect::<Vec<_>>();
    let as_of = combined
        .iter()
        .filter_map(|record| parse_time(&record.header.finished_at))
        .chain(std::iter::once(end_of_day(inputs.date)))
        .max()
        .expect("end of day is always present");
    let samples = |records: &[SessionRecord]| {
        component_samples_for_sessions(
            records,
            inputs.layout,
            inputs.geometry,
            GradeOptions { model: inputs.settings },
            as_of,
            inputs.english_corpus,
            inputs.japanese_corpus,
            language,
        )
    };
    let check = samples(std::slice::from_ref(inputs.check));
    let check_and_steps = samples(&combined);
    let snapshot = |sample: &ComponentSample, corrected: bool, value: f64| SkillSnapshot {
        run_id: inputs.run_id,
        date: inputs.date,
        language: language.to_owned(),
        component: sample.component,
        raw_value: value,
        n: sample.n,
        source: SnapshotSource::for_component(sample.component),
        corrected,
    };
    let mut snapshots = Vec::new();
    for &component in Component::all() {
        let source = SnapshotSource::for_component(component);
        let pool = match source {
            SnapshotSource::Check => &check,
            SnapshotSource::CheckSteps => &check_and_steps,
        };
        let Some(sample) = pool.iter().find(|sample| sample.component == component && sample.value.is_finite()) else {
            continue;
        };
        // No observation behind the value (e.g. a check step without a
        // single character keystroke: net WPM 0 and accuracy 100% are
        // placeholders, not measurements). Never store it.
        if sample.n == 0 {
            continue;
        }
        if source == SnapshotSource::CheckSteps && sample.n < inputs.min_transition_n {
            continue;
        }
        match (component, inputs.corrected_wpm.filter(|value| value.is_finite())) {
            (Component::Speed, Some(corrected)) => snapshots.push(snapshot(sample, true, corrected)),
            _ => snapshots.push(snapshot(sample, false, sample.value)),
        }
    }
    snapshots
}

/// Result of [`record_run_snapshots`] and [`rebuild_all_snapshots`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SnapshotWrite {
    /// Runs whose snapshots were (re)computed — runs with a recorded check step.
    pub runs: usize,
    /// Rows written to `skill_snapshots`.
    pub rows: usize,
}

/// Shared, run-independent inputs of the snapshot computation.
struct SnapshotEnvironment {
    corpus: crate::data::DataCatalog,
    geometry: KeyboardGeometry,
    forms: Vec<SessionForm>,
}

impl SnapshotEnvironment {
    fn load(ctx: &JudgeContext<'_>) -> AppResult<Self> {
        let corpus = crate::data::DataCatalog::load(&crate::config::DataConfig::default(), ctx.home)?;
        let geometry = crate::layout::geometry::geometry(&ctx.config.keyboard.geometry)
            .or_else(|| crate::layout::geometry::geometry("ansi"))
            .expect("bundled ansi geometry");
        // The corrected WPM is the existing «form × expected WPM of the
        // reference bench» from the progress analysis, computed over every
        // record exactly as the menu assembly does. A session's form only
        // depends on the sessions before it.
        let records = db::load_records_filtered(ctx.db_path, None, None)?;
        let references = super::reference_prompts(&corpus, ctx.layout, &geometry);
        let forms = analyze_progress(
            &records,
            ctx.layout,
            &geometry,
            ProgressOptions {
                model: analysis_settings(ctx.config),
                min_transition_n: ctx.config.analysis.min_transition_n,
            },
            end_of_day(ctx.as_of),
            &references,
        )
        .forms;
        Ok(Self { corpus, geometry, forms })
    }

    /// Snapshots of one run, or `None` when its check step is not recorded.
    fn run_snapshots(&self, ctx: &JudgeContext<'_>, run: &MenuRunRecord) -> AppResult<Option<Vec<SkillSnapshot>>> {
        let steps = db::load_menu_steps(ctx.db_path, run.id)?;
        let Some(check_step) = steps.iter().find(|row| row.role == MenuRole::Check.as_str()) else {
            return Ok(None);
        };
        let Some(check) = db::load_record(ctx.db_path, check_step.session_id)? else {
            return Ok(None);
        };
        let excluded = [MenuRole::Check.as_str(), MenuRole::PrimaryA.as_str(), MenuRole::PrimaryB.as_str()];
        let mut other_steps = Vec::new();
        for row in steps.iter().filter(|row| !excluded.contains(&row.role.as_str())) {
            if let Some(record) = db::load_record(ctx.db_path, row.session_id)? {
                other_steps.push(record);
            }
        }
        let run_layout = (run.layout != ctx.config.keyboard.layout)
            .then(|| crate::layout::builtin_layout(&run.layout).ok())
            .flatten();
        let layout = run_layout.as_ref().unwrap_or(ctx.layout);
        let geometry = crate::layout::geometry::geometry(&check.header.geometry).unwrap_or_else(|| self.geometry.clone());
        let corrected_wpm = check
            .header
            .id
            .and_then(|id| self.forms.iter().find(|form| form.session_id == Some(id)))
            .map(|form| form.corrected_wpm);
        Ok(Some(compute_run_snapshots(&SnapshotInputs {
            run_id: run.id,
            date: run.date,
            check: &check,
            other_steps: &other_steps,
            corrected_wpm,
            layout,
            geometry: &geometry,
            settings: analysis_settings(ctx.config),
            min_transition_n: ctx.config.analysis.min_transition_n,
            english_corpus: self.corpus.words_en(),
            japanese_corpus: self.corpus.words_ja(),
        })))
    }
}

/// Recompute and store the snapshots of `run_id` (§8.1). Does nothing until
/// the run's check step is recorded; afterwards every call replaces the
/// run's rows in one transaction, so finishing another step of the same run
/// rewrites them (transition and recovery gain observations) and writing the
/// same run twice gives the same rows.
pub fn record_run_snapshots(ctx: &JudgeContext<'_>, run_id: i64) -> AppResult<SnapshotWrite> {
    let Some(run) = db::load_menu_run(ctx.db_path, run_id)? else {
        return Ok(SnapshotWrite::default());
    };
    if !db::load_menu_steps(ctx.db_path, run_id)?
        .iter()
        .any(|row| row.role == MenuRole::Check.as_str())
    {
        return Ok(SnapshotWrite::default());
    }
    let environment = SnapshotEnvironment::load(ctx)?;
    let Some(snapshots) = environment.run_snapshots(ctx, &run)? else {
        return Ok(SnapshotWrite::default());
    };
    let rows = snapshots.iter().map(SkillSnapshot::to_record).collect::<Vec<_>>();
    db::replace_skill_snapshots(ctx.db_path, run_id, &rows)?;
    Ok(SnapshotWrite { runs: 1, rows: rows.len() })
}

/// `stats --rebuild-skills`: recompute the snapshots of every menu run and
/// replace the whole table in one transaction.
pub fn rebuild_all_snapshots(ctx: &JudgeContext<'_>) -> AppResult<SnapshotWrite> {
    let runs = db::load_all_menu_runs(ctx.db_path)?;
    let environment = SnapshotEnvironment::load(ctx)?;
    let mut rows = Vec::new();
    let mut measured = 0;
    for run in &runs {
        if let Some(snapshots) = environment.run_snapshots(ctx, run)? {
            measured += 1;
            rows.extend(snapshots.iter().map(SkillSnapshot::to_record));
        }
    }
    db::rebuild_skill_snapshots(ctx.db_path, &rows)?;
    Ok(SnapshotWrite {
        runs: measured,
        rows: rows.len(),
    })
}

// ---------------------------------------------------------------------------
// Trends (§8.3)
// ---------------------------------------------------------------------------

/// One run's value in a series of one language and one component.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeriesPoint {
    pub run_id: i64,
    pub date: NaiveDate,
    pub value: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TrendDirection {
    /// Moving in the better direction (down for lower-is-better components).
    Improving,
    Flat,
    Declining,
}

/// §8.3 figures of the latest point of a series, all in raw-value units.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SkillTrend {
    pub latest: f64,
    /// Latest − previous run (raw difference; its sign is not the direction
    /// of improvement).
    pub previous_difference: Option<f64>,
    /// Median of the latest 7 runs; `None` with fewer than 5 runs.
    pub moving_median: Option<f64>,
    /// Median absolute previous-run difference over the latest 20.
    pub typical_change: Option<f64>,
    /// |previous difference| > typical change; `None` when either is missing.
    pub beyond_typical: Option<bool>,
    /// Least-squares slope over the latest 28 days, in units per week.
    pub slope_per_week: Option<f64>,
    pub trend: Option<TrendDirection>,
}

/// §8.3 figures for the latest point of `points` (one language and one
/// component). Points are ordered by date and run id here. `direction` is the
/// component's direction from the grade thresholds.
pub fn skill_trend(points: &[SeriesPoint], direction: Direction) -> Option<SkillTrend> {
    let mut points = points.iter().copied().filter(|point| point.value.is_finite()).collect::<Vec<_>>();
    points.sort_by(|left, right| left.date.cmp(&right.date).then(left.run_id.cmp(&right.run_id)));
    let latest = *points.last()?;
    let differences = points.windows(2).map(|pair| pair[1].value - pair[0].value).collect::<Vec<_>>();
    let previous_difference = differences.last().copied();
    let moving_median = (points.len() >= MOVING_MEDIAN_MIN_POINTS)
        .then(|| median(points.iter().rev().take(MOVING_MEDIAN_WINDOW).map(|point| point.value).collect()))
        .flatten();
    let typical_change = (differences.len() >= TYPICAL_CHANGE_MIN_DIFFERENCES)
        .then(|| {
            median(
                differences
                    .iter()
                    .rev()
                    .take(TYPICAL_CHANGE_WINDOW)
                    .map(|difference| difference.abs())
                    .collect(),
            )
        })
        .flatten();
    let beyond_typical = previous_difference
        .zip(typical_change)
        .map(|(difference, typical)| difference.abs() > typical);
    let slope_per_week = trend_slope_per_week(&points, latest.date);
    let trend = slope_per_week.zip(typical_change).map(|(slope, typical)| {
        let threshold = typical * TREND_THRESHOLD_FRACTION_OF_TYPICAL;
        let improvement = match direction {
            Direction::Higher => slope,
            Direction::Lower => -slope,
        };
        if improvement > threshold {
            TrendDirection::Improving
        } else if improvement < -threshold {
            TrendDirection::Declining
        } else {
            TrendDirection::Flat
        }
    });
    Some(SkillTrend {
        latest: latest.value,
        previous_difference,
        moving_median,
        typical_change,
        beyond_typical,
        slope_per_week,
        trend,
    })
}

/// Slope of the points dated within the latest [`TREND_WINDOW_DAYS`] days,
/// against calendar days, in units per week. `None` unless the points span
/// at least [`TREND_MIN_CALENDAR_WEEKS`] ISO weeks — counted as the existing
/// 4-week bench slope does.
fn trend_slope_per_week(points: &[SeriesPoint], as_of: NaiveDate) -> Option<f64> {
    let start = as_of - chrono::Duration::days(TREND_WINDOW_DAYS - 1);
    let selected = points.iter().filter(|point| (start..=as_of).contains(&point.date)).collect::<Vec<_>>();
    let weeks = selected
        .iter()
        .map(|point| {
            let week = point.date.iso_week();
            (week.year(), week.week())
        })
        .collect::<std::collections::BTreeSet<_>>();
    if weeks.len() < TREND_MIN_CALENDAR_WEEKS {
        return None;
    }
    let xy = selected
        .iter()
        .map(|point| ((point.date - start).num_days() as f64, point.value))
        .collect::<Vec<_>>();
    least_squares_slope(&xy).map(|slope| slope * 7.0)
}

fn least_squares_slope(points: &[(f64, f64)]) -> Option<f64> {
    if points.len() < 2 {
        return None;
    }
    let n = points.len() as f64;
    let mean_x = points.iter().map(|point| point.0).sum::<f64>() / n;
    let mean_y = points.iter().map(|point| point.1).sum::<f64>() / n;
    let covariance = points.iter().map(|point| (point.0 - mean_x) * (point.1 - mean_y)).sum::<f64>();
    let variance = points.iter().map(|point| (point.0 - mean_x).powi(2)).sum::<f64>();
    (variance > 0.0).then(|| covariance / variance).filter(|slope| slope.is_finite())
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

fn analysis_settings(config: &crate::config::Config) -> AnalysisSettings {
    AnalysisSettings {
        pause_ms: config.analysis.pause_ms,
        shrinkage_k: config.analysis.shrinkage_k,
        half_life_days: config.analysis.half_life_days,
    }
}

fn parse_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value).ok().map(|time| time.with_timezone(&Utc))
}

fn end_of_day(date: NaiveDate) -> DateTime<Utc> {
    date.and_hms_opt(23, 59, 59).expect("valid end of day").and_utc()
}

// ---------------------------------------------------------------------------
// `stats --skills`
// ---------------------------------------------------------------------------

/// The latest stored snapshot of a series.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LatestSnapshot {
    pub run_id: i64,
    pub date: NaiveDate,
    pub raw_value: f64,
    pub n: u64,
    pub source: SnapshotSource,
    pub corrected: bool,
}

/// One component row of `stats --skills`. Every field after `component` is
/// `None` (JSON `null`) when there is no data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ComponentSkill {
    pub component: Component,
    pub unit: String,
    /// Runs in the series.
    pub runs: usize,
    pub latest: Option<LatestSnapshot>,
    /// Grade and 0–8 score of the latest raw value under the current
    /// thresholds (computed on display, §8.2).
    pub grade: Option<Level>,
    pub score: Option<f64>,
    pub previous_difference: Option<f64>,
    pub moving_median: Option<f64>,
    pub typical_change: Option<f64>,
    pub beyond_typical: Option<bool>,
    pub slope_per_week: Option<f64>,
    pub trend: Option<TrendDirection>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LanguageSkills {
    pub language: String,
    pub components: Vec<ComponentSkill>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SkillsReport {
    pub grade_version: String,
    pub languages: Vec<LanguageSkills>,
}

/// Build the `stats --skills` report from stored snapshots: one entry per
/// language that has any snapshot, each with all seven components.
pub fn skills_report(rows: &[SkillSnapshotRecord]) -> SkillsReport {
    let mut series = BTreeMap::<(String, Component), Vec<&SkillSnapshotRecord>>::new();
    for row in rows {
        let Some(component) = Component::all().iter().copied().find(|component| component.as_str() == row.component) else {
            continue;
        };
        if row.source != SnapshotSource::for_component(component).as_str() || !row.raw_value.is_finite() {
            continue;
        }
        series.entry((row.lang.clone(), component)).or_default().push(row);
    }
    let mut languages = series.keys().map(|(language, _)| language.clone()).collect::<Vec<_>>();
    languages.dedup();
    let thresholds = bundled_thresholds().ok();
    let languages = languages
        .into_iter()
        .filter_map(|language| {
            let table = thresholds?.language(&language)?;
            let components = Component::all()
                .iter()
                .copied()
                .map(|component| {
                    let threshold = table.get(component.as_str());
                    let unit = threshold.map(|row| row.unit.clone()).unwrap_or_default();
                    let direction = threshold.map_or(Direction::Higher, |row| row.direction);
                    let mut rows = series.get(&(language.clone(), component)).cloned().unwrap_or_default();
                    rows.sort_by(|left, right| left.date.cmp(&right.date).then(left.run_id.cmp(&right.run_id)));
                    let points = rows
                        .iter()
                        .map(|row| SeriesPoint {
                            run_id: row.run_id,
                            date: row.date,
                            value: row.raw_value,
                        })
                        .collect::<Vec<_>>();
                    let trend = skill_trend(&points, direction);
                    let latest = rows.last().map(|row| LatestSnapshot {
                        run_id: row.run_id,
                        date: row.date,
                        raw_value: row.raw_value,
                        n: row.n,
                        source: SnapshotSource::for_component(component),
                        corrected: row.corrected,
                    });
                    let grade = latest
                        .as_ref()
                        .and_then(|latest| grade_component(&language, component, latest.raw_value).ok());
                    ComponentSkill {
                        component,
                        unit,
                        runs: rows.len(),
                        latest,
                        grade: grade.as_ref().map(|grade| grade.grade),
                        score: grade.as_ref().map(|grade| grade.score),
                        previous_difference: trend.and_then(|trend| trend.previous_difference),
                        moving_median: trend.and_then(|trend| trend.moving_median),
                        typical_change: trend.and_then(|trend| trend.typical_change),
                        beyond_typical: trend.and_then(|trend| trend.beyond_typical),
                        slope_per_week: trend.and_then(|trend| trend.slope_per_week),
                        trend: trend.and_then(|trend| trend.trend),
                    }
                })
                .collect();
            Some(LanguageSkills { language, components })
        })
        .collect();
    SkillsReport {
        grade_version: GRADE_VERSION.to_owned(),
        languages,
    }
}

/// `stats --skills [--json]`.
pub fn render_from_storage(path: &Path, lang: Lang, json: bool) -> AppResult<String> {
    let report = skills_report(&db::load_skill_snapshots(path)?);
    if json {
        let mut output = serde_json::to_string_pretty(&report)?;
        output.push('\n');
        return Ok(output);
    }
    Ok(render_text(&report, lang))
}

/// `stats --rebuild-skills`.
pub fn rebuild_from_storage(ctx: &JudgeContext<'_>, lang: Lang) -> AppResult<String> {
    let written = rebuild_all_snapshots(ctx)?;
    Ok(match lang {
        Lang::En => format!("Rebuilt skill snapshots: {} runs, {} rows\n", written.runs, written.rows),
        Lang::Ja => format!("スキルの記録を作り直しました: {} 回のメニュー，{} 行\n", written.runs, written.rows),
    })
}

pub fn render_text(report: &SkillsReport, lang: Lang) -> String {
    use std::fmt::Write;

    let mut output = String::new();
    if report.languages.is_empty() {
        output.push_str(match lang {
            Lang::En => "No skill records yet. Finish the check step of a practice menu to record one.\n",
            Lang::Ja => "スキルの記録はまだありません．練習メニューの確認段を終えると記録されます．\n",
        });
        return output;
    }
    let no_data = Msg::NoData.text(lang);
    let header = match lang {
        Lang::En => [
            "Component",
            "Latest",
            "Grade",
            "Score",
            "Prev diff",
            "Median (7)",
            "Typical",
            "Beyond",
            "Trend (28 d)",
        ],
        Lang::Ja => [
            "構成",
            "今回",
            "段",
            "連続値",
            "前回差",
            "移動中央値",
            "ふだんの変動",
            "誤差超え",
            "傾向（28 日）",
        ],
    };
    for (index, language) in report.languages.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        let heading = match (language.language.as_str(), lang) {
            ("ja", Lang::En) => "Japanese",
            ("ja", Lang::Ja) => "日本語",
            (_, Lang::En) => "English",
            (_, Lang::Ja) => "英語",
        };
        let _ = match lang {
            Lang::En => writeln!(output, "Skills — {heading} (check step; grades {})", report.grade_version),
            Lang::Ja => writeln!(output, "今のスキル — {heading}（確認段から．段は {}）", report.grade_version),
        };
        let mut table = vec![header.iter().map(|cell| (*cell).to_owned()).collect::<Vec<_>>()];
        for row in &language.components {
            let label = crate::analysis::stats::component_name(row.component, lang).to_owned();
            let Some(latest) = &row.latest else {
                table.push(vec![label, no_data.to_owned()]);
                continue;
            };
            let value = |value: Option<f64>| value.map_or_else(|| no_data.to_owned(), |value| format_value(row.component, value, false));
            let mut latest_text = format_value(row.component, latest.raw_value, false);
            if !row.unit.is_empty() {
                latest_text.push(' ');
                latest_text.push_str(&row.unit);
            }
            if row.component == Component::Speed && !latest.corrected {
                latest_text.push_str(match lang {
                    Lang::En => " (uncorrected)",
                    Lang::Ja => "（未補正）",
                });
            }
            let beyond = row.beyond_typical.map_or_else(
                || no_data.to_owned(),
                |beyond| {
                    match (beyond, lang) {
                        (true, Lang::En) => "yes",
                        (false, Lang::En) => "no",
                        (true, Lang::Ja) => "超えた",
                        (false, Lang::Ja) => "範囲内",
                    }
                    .to_owned()
                },
            );
            let per_week = |slope: f64| match lang {
                Lang::En => format!("({}/wk)", format_value(row.component, slope, true)),
                Lang::Ja => format!("（{}/週）", format_value(row.component, slope, true)),
            };
            let trend = match (row.trend, row.slope_per_week) {
                (Some(trend), Some(slope)) => format!("{} {}", trend_label(trend, lang), per_week(slope)),
                (None, Some(slope)) => format!("{no_data} {}", per_week(slope)),
                _ => no_data.to_owned(),
            };
            table.push(vec![
                label,
                latest_text,
                row.grade.map_or_else(|| no_data.to_owned(), |grade| grade.as_str().to_owned()),
                row.score.map_or_else(|| no_data.to_owned(), |score| format!("{score:.2}")),
                row.previous_difference
                    .map_or_else(|| no_data.to_owned(), |value| format_value(row.component, value, true)),
                value(row.moving_median),
                value(row.typical_change),
                beyond,
                trend,
            ]);
        }
        write_table(&mut output, &table);
    }
    output
}

pub fn trend_label(trend: TrendDirection, lang: Lang) -> &'static str {
    match (trend, lang) {
        (TrendDirection::Improving, Lang::En) => "improving",
        (TrendDirection::Flat, Lang::En) => "flat",
        (TrendDirection::Declining, Lang::En) => "declining",
        (TrendDirection::Improving, Lang::Ja) => "上向き",
        (TrendDirection::Flat, Lang::Ja) => "横ばい",
        (TrendDirection::Declining, Lang::Ja) => "下向き",
    }
}

pub fn format_value(component: Component, value: f64, signed: bool) -> String {
    let digits = match component {
        Component::Rhythm => 3,
        Component::Preview => 2,
        Component::Recovery => 0,
        _ => 1,
    };
    if signed {
        format!("{value:+.digits$}")
    } else {
        format!("{value:.digits$}")
    }
}

fn write_table(output: &mut String, rows: &[Vec<String>]) {
    use unicode_width::UnicodeWidthStr;

    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths = (0..columns)
        .map(|column| {
            rows.iter()
                // A "no data" row has two cells; its message spans the rest,
                // so it only sets the width of the component-name column
                // (display width, so full-width labels line up as in `stats`).
                .filter(|row| column == 0 || row.len() == columns)
                .filter_map(|row| row.get(column))
                .map(|cell| cell.width())
                .max()
                .unwrap_or(0)
        })
        .collect::<Vec<_>>();
    for row in rows {
        let mut line = String::new();
        for (column, cell) in row.iter().enumerate() {
            if column > 0 {
                line.push_str("  ");
            }
            line.push_str(cell);
            if column + 1 < row.len() {
                line.push_str(&" ".repeat(widths[column].saturating_sub(cell.width())));
            }
        }
        output.push_str(line.trim_end());
        output.push('\n');
    }
}

// ---------------------------------------------------------------------------
// Series readers for the summary screen, the skills tab and `--csv`
// ---------------------------------------------------------------------------

/// Snapshots of one language and one component, in series order (date, run
/// id). Only the component's own source counts (§8.1), and non-finite values
/// are dropped — exactly the rows [`skills_report`] reads.
pub fn component_series<'a>(rows: &'a [SkillSnapshotRecord], language: &str, component: Component) -> Vec<&'a SkillSnapshotRecord> {
    let mut series = rows
        .iter()
        .filter(|row| {
            row.lang == language
                && row.component == component.as_str()
                && row.source == SnapshotSource::for_component(component).as_str()
                && row.raw_value.is_finite()
        })
        .collect::<Vec<_>>();
    series.sort_by(|left, right| left.date.cmp(&right.date).then(left.run_id.cmp(&right.run_id)));
    series
}

/// Languages with at least one snapshot, in code order.
pub fn snapshot_languages(rows: &[SkillSnapshotRecord]) -> Vec<String> {
    rows.iter()
        .map(|row| row.lang.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// One point of a series with the §8.3 figures as of that point (computed by
/// [`skill_trend`] over the points up to and including it).
#[derive(Clone, Debug, PartialEq)]
pub struct SeriesPointFigures {
    pub run_id: i64,
    pub date: NaiveDate,
    pub raw_value: f64,
    pub corrected: bool,
    pub source: String,
    pub previous_difference: Option<f64>,
    pub moving_median: Option<f64>,
}

/// Every point of one series with the previous difference and the moving
/// median as they stood at that point.
pub fn series_figures(rows: &[SkillSnapshotRecord], language: &str, component: Component, direction: Direction) -> Vec<SeriesPointFigures> {
    let series = component_series(rows, language, component);
    let points = series
        .iter()
        .map(|row| SeriesPoint {
            run_id: row.run_id,
            date: row.date,
            value: row.raw_value,
        })
        .collect::<Vec<_>>();
    series
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let trend = skill_trend(&points[..=index], direction);
            SeriesPointFigures {
                run_id: row.run_id,
                date: row.date,
                raw_value: row.raw_value,
                corrected: row.corrected,
                source: row.source.clone(),
                previous_difference: trend.and_then(|trend| trend.previous_difference),
                moving_median: trend.and_then(|trend| trend.moving_median),
            }
        })
        .collect()
}

/// The direction and unit of a component from the current thresholds
/// (higher-is-better and no unit when the language has no table).
pub fn component_direction_and_unit(language: &str, component: Component) -> (Direction, String) {
    bundled_thresholds()
        .ok()
        .and_then(|thresholds| thresholds.language(language))
        .and_then(|table| table.get(component.as_str()))
        .map_or((Direction::Higher, String::new()), |row| (row.direction, row.unit.clone()))
}

/// One component of a run in the summary's middle section (§7.1).
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentRunSkill {
    pub component: Component,
    pub unit: String,
    pub direction: Direction,
    /// The run's raw value; `None` when this run has no value («no data»).
    pub value: Option<f64>,
    pub corrected: bool,
    pub grade: Option<Level>,
    pub score: Option<f64>,
    /// §8.3 figures as of this run (`None` without a value in this run).
    pub trend: Option<SkillTrend>,
    /// Values of the latest 28 days up to the run, oldest first.
    pub recent: Vec<f64>,
}

/// The middle section of the summary for one run: the check step's language
/// series (§7.1). The overall is computed from the run's raw values with the
/// existing overall-grade computation, and is a practice value — never the
/// official bench grade.
#[derive(Clone, Debug, PartialEq)]
pub struct RunSkillSummary {
    pub run_id: i64,
    pub date: NaiveDate,
    pub language: String,
    pub components: Vec<ComponentRunSkill>,
    pub overall: Option<crate::analysis::grades::OverallGrade>,
    /// The overall of the previous run in the same language series.
    pub previous_overall: Option<crate::analysis::grades::OverallGrade>,
}

fn run_values(rows: &[SkillSnapshotRecord], language: &str, run_id: i64) -> Vec<crate::analysis::grades::ComponentValue> {
    Component::all()
        .iter()
        .filter_map(|component| {
            component_series(rows, language, *component)
                .into_iter()
                .find(|row| row.run_id == run_id)
                .map(|row| crate::analysis::grades::ComponentValue::new(*component, row.raw_value))
        })
        .collect()
}

/// [`RunSkillSummary`] of `run_id` (dated `date`) in `language`. A component
/// without a snapshot in this run has `value: None` («no data»; never 0).
pub fn run_skill_summary(rows: &[SkillSnapshotRecord], run_id: i64, date: NaiveDate, language: &str) -> RunSkillSummary {
    let position = (date, run_id);
    let components = Component::all()
        .iter()
        .copied()
        .map(|component| {
            let (direction, unit) = component_direction_and_unit(language, component);
            let series = component_series(rows, language, component)
                .into_iter()
                .filter(|row| (row.date, row.run_id) <= position)
                .collect::<Vec<_>>();
            let current = series.last().filter(|row| row.run_id == run_id).copied();
            let points = series
                .iter()
                .map(|row| SeriesPoint {
                    run_id: row.run_id,
                    date: row.date,
                    value: row.raw_value,
                })
                .collect::<Vec<_>>();
            let trend = current.and_then(|_| skill_trend(&points, direction));
            let grade = current.and_then(|row| grade_component(language, component, row.raw_value).ok());
            let start = date - chrono::Duration::days(TREND_WINDOW_DAYS - 1);
            ComponentRunSkill {
                component,
                unit,
                direction,
                value: current.map(|row| row.raw_value),
                corrected: current.is_some_and(|row| row.corrected),
                grade: grade.as_ref().map(|grade| grade.grade),
                score: grade.as_ref().map(|grade| grade.score),
                trend,
                recent: series.iter().filter(|row| row.date >= start).map(|row| row.raw_value).collect(),
            }
        })
        .collect::<Vec<_>>();
    let previous_run = rows
        .iter()
        .filter(|row| row.lang == language && (row.date, row.run_id) < position)
        .max_by(|left, right| (left.date, left.run_id).cmp(&(right.date, right.run_id)))
        .map(|row| row.run_id);
    RunSkillSummary {
        run_id,
        date,
        language: language.to_owned(),
        components,
        overall: crate::analysis::grades::overall_from_values(language, &run_values(rows, language, run_id)),
        previous_overall: previous_run
            .and_then(|previous| crate::analysis::grades::overall_from_values(language, &run_values(rows, language, previous))),
    }
}

/// `stats --skills --csv`: one row per stored point of every language and
/// component, with the grade and score under the current thresholds and the
/// moving median and previous difference as they stood at that point. Missing
/// values are empty cells.
pub fn render_csv(rows: &[SkillSnapshotRecord]) -> String {
    use std::fmt::Write;

    let mut output = String::from("date,lang,component,raw_value,grade,score,moving_median,previous_diff,corrected,source\n");
    let optional = |value: Option<f64>| value.map(|value| value.to_string()).unwrap_or_default();
    for language in snapshot_languages(rows) {
        let mut lines = Vec::new();
        for (order, component) in Component::all().iter().copied().enumerate() {
            let (direction, _) = component_direction_and_unit(&language, component);
            for point in series_figures(rows, &language, component, direction) {
                let grade = grade_component(&language, component, point.raw_value).ok();
                let mut line = String::new();
                let _ = write!(
                    line,
                    "{},{},{},{},{},{},{},{},{},{}",
                    point.date,
                    language,
                    component.as_str(),
                    point.raw_value,
                    grade.as_ref().map(|grade| grade.grade.as_str()).unwrap_or_default(),
                    optional(grade.as_ref().map(|grade| grade.score)),
                    optional(point.moving_median),
                    optional(point.previous_difference),
                    point.corrected,
                    point.source,
                );
                lines.push(((point.date, point.run_id, order), line));
            }
        }
        lines.sort_by_key(|line| line.0);
        for (_, line) in lines {
            output.push_str(&line);
            output.push('\n');
        }
    }
    output
}

/// `stats --skills --csv` from storage.
pub fn render_csv_from_storage(path: &Path) -> AppResult<String> {
    Ok(render_csv(&db::load_skill_snapshots(path)?))
}

/// Benchmark values that can be placed on a component series (§8.4): the
/// existing bench trend has net WPM and accuracy per session, which are the
/// speed and accuracy components' raw units. The other five components are
/// not part of the bench trend and are never overlaid.
pub const BENCH_OVERLAY_COMPONENTS: [Component; 2] = [Component::Speed, Component::Accuracy];

/// Bench points (date, raw value) of `component` for `language` from the
/// bench sessions the bench grade uses (part A for English, part C for
/// Japanese). Empty for components outside [`BENCH_OVERLAY_COMPONENTS`].
pub fn bench_component_points(records: &[SessionRecord], language: &str, component: Component) -> Vec<(NaiveDate, f64)> {
    let value = |record: &SessionRecord| match component {
        Component::Speed => Some(record.header.net_wpm),
        Component::Accuracy => Some(record.header.accuracy * 100.0),
        _ => None,
    };
    crate::analysis::grades::language_bench_records(records, language)
        .into_iter()
        .filter_map(|record| {
            let date = DateTime::parse_from_rfc3339(&record.header.started_at).ok()?.date_naive();
            Some((date, value(record).filter(|value| value.is_finite())?))
        })
        .collect()
}

/// One day's prescription step of the history band (§8.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrescriptionDay {
    pub date: NaiveDate,
    pub role: MenuRole,
    pub prescription_id: String,
    pub level: Option<u8>,
}

/// Prescription steps played in `language` (layout `layout`), oldest first,
/// from `menu_steps` and the run plans.
pub fn prescription_history(path: &Path, layout: &str, language: &str) -> AppResult<Vec<PrescriptionDay>> {
    let runs = db::load_all_menu_runs(path)?
        .into_iter()
        .filter(|run| run.layout == layout)
        .filter_map(|run| Some((run.id, serde_json::from_str::<super::DailyMenu>(&run.plan_json).ok()?)))
        .collect::<BTreeMap<_, _>>();
    let mut days = Vec::new();
    for row in db::load_menu_step_history(path)? {
        let Some(plan) = runs.get(&row.run_id) else { continue };
        let Some(step) = row.step.idx.checked_sub(1).and_then(|index| plan.steps.get(index)) else {
            continue;
        };
        if !matches!(step.role, MenuRole::PrimaryA | MenuRole::PrimaryB) || super::language_code(step.language) != language {
            continue;
        }
        days.push(PrescriptionDay {
            date: row.date,
            role: step.role,
            prescription_id: step.prescription_id.clone(),
            level: row.step.level,
        });
    }
    Ok(days)
}

/// What the analysis screen's «skills» tab reads (§8.4), loaded once per
/// reload: the stored snapshots, the bench points that can be overlaid and
/// the prescription history per language.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkillsTabData {
    pub snapshots: Vec<SkillSnapshotRecord>,
    /// (language, component) → bench points (date, raw value).
    pub bench: BTreeMap<(String, Component), Vec<(NaiveDate, f64)>>,
    /// language → prescription steps, oldest first.
    pub prescriptions: BTreeMap<String, Vec<PrescriptionDay>>,
}

impl SkillsTabData {
    pub fn languages(&self) -> Vec<String> {
        snapshot_languages(&self.snapshots)
    }
}

/// Load [`SkillsTabData`]. `bench_records` are the sessions to take bench
/// points from (any non-bench session is ignored).
pub fn load_skills_tab(path: &Path, layout: &str, bench_records: &[SessionRecord]) -> AppResult<SkillsTabData> {
    let snapshots = db::load_skill_snapshots(path)?;
    let mut bench = BTreeMap::new();
    let mut prescriptions = BTreeMap::new();
    for language in snapshot_languages(&snapshots) {
        for component in BENCH_OVERLAY_COMPONENTS {
            let points = bench_component_points(bench_records, &language, component);
            if !points.is_empty() {
                bench.insert((language.clone(), component), points);
            }
        }
        prescriptions.insert(language.clone(), prescription_history(path, layout, &language)?);
    }
    Ok(SkillsTabData {
        snapshots,
        bench,
        prescriptions,
    })
}
