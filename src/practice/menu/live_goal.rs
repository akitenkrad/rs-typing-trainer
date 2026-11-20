//! The live goal of a menu step or a table cell (MYTASK-3633, MYTASK-3641).
//!
//! Every row of the individual-training table except «normal» trains one
//! skill, and today's menu plays its steps as cells of that table. This
//! module turns the row into a goal the player can watch while typing — a
//! metric, its current value, a target line and the direction that counts as
//! better — and into the headline of the result screen. A menu step and a
//! session started from the table get the same goal for the same row; only
//! the target line and the comparison on the result screen differ.
//!
//! Every value comes from the computation that records and judges the step
//! (`current_metric_value`, `anticipation_rate_of_kinds`, the rhythm
//! breakdown behind the rhythm grade, `compare_endurance_log`), applied to a
//! [`SessionRecord`] built from the live log with `SessionRecord::from_log`.
//! The last live value therefore equals the value of the finished step.
//!
//! The rows added by MYTASK-3641 read the functions behind their judgment
//! too: `analysis::errors::post_error_detail` (recovery),
//! `analysis::style::word_intervals_ms` / `word_start_from` (look-ahead),
//! `analysis::style::rollover_metric` (rollover),
//! `analysis::japanese::spelling_usage` (spelling),
//! `analysis::summary::session_reach_ratio` (unlock keys) and the bigram
//! medians of `analysis::metrics::analyze_records` (weak transitions).
//!
//! [`GoalKind::for_skill`] is the only place that maps a row to a goal; a
//! new row is added there and in the exhaustive matches that follow.

use std::time::Duration;

use chrono::{DateTime, Utc};

use super::{AdvanceCondition, MenuMetric, MenuStep, PACER_ACCURACY_FLOOR, SpellingTarget, current_metric_value};
use crate::analysis::grades::Direction;
use crate::analysis::metrics::AnalysisSettings;
use crate::config::{DrillCell, DrillSkill};
use crate::engine::session::KeystrokeKind;
use crate::layout::KeyboardLayout;
use crate::practice::session_variants::EnduranceComparison;
use crate::record::SessionRecord;

/// How long a short notice («先走り!», «最速ペース!») stays on screen.
pub const FLASH_DURATION: Duration = Duration::from_millis(1_200);
/// Accuracy within this margin above the gate is shown as a warning.
pub const GATE_WARNING_MARGIN: f64 = 0.01;
/// How many recent within-word intervals the rhythm band shows.
pub const RECENT_INTERVALS: usize = 24;
/// A table cell's own line is the median of this many of its latest
/// sessions (MYTASK-3641).
pub const SAME_CELL_MEDIAN_RUNS: usize = 10;
/// At most this many of a cell's latest sessions are measured for the line,
/// the previous value and the personal best.
pub const SAME_CELL_HISTORY_LIMIT: usize = 200;
/// A word start longer than this many within-word intervals (their median)
/// counts as a stop for the look-ahead row's run of words.
pub const ONSET_STOP_FACTOR: f64 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoalKind {
    /// Accuracy row (`accuracy.gate`): accuracy against the gate.
    AccuracyGate,
    /// Anticipation row (`accuracy.anticipation`): share of anticipation
    /// errors against the player's own median.
    Anticipation,
    /// Rhythm row (`rhythm.metronome`): within-word IKI CV against the own
    /// median.
    Metronome,
    /// Speed row (`speed.pacer`): net WPM against the pacer target, with the
    /// accuracy floor of `target-with-accuracy`.
    Pacer,
    /// Endurance row (`endurance.split`): second-half slowdown against the
    /// threshold.
    EnduranceSplit,
    /// Burst row (`fixed.finish`): three sprints; the best one counts.
    Sprint,
    /// Recovery row (`recovery.retry`): excess time after errors
    /// (`post-error-excess-ms`) against the own median.
    Recovery,
    /// No-look row (`blind.fade`): accuracy against the own median.
    Blind,
    /// Look-ahead row (`preview.*`): word-onset ratio (`word-onset-ratio`)
    /// against the own median.
    Preview,
    /// Rollover row (`overlap.burst`): rollover rate (`rollover-rate`, in
    /// percent) against the own median.
    Overlap,
    /// Spelling row (`japanese.spelling`): use of the suggested spelling
    /// against the 80% line.
    Spelling,
    /// Weak-transition row (`transition.*`): this session's median time of
    /// the target transitions against their estimates.
    WeakTransition,
    /// Unlock-keys row (`reach.unlock`): remote-row/home-row IKI ratio
    /// (`far-home-iki-ratio`) against the own median.
    Reach,
}

impl GoalKind {
    pub const ALL: [Self; 13] = [
        Self::AccuracyGate,
        Self::Anticipation,
        Self::Metronome,
        Self::Pacer,
        Self::EnduranceSplit,
        Self::Sprint,
        Self::Recovery,
        Self::Blind,
        Self::Preview,
        Self::Overlap,
        Self::Spelling,
        Self::WeakTransition,
        Self::Reach,
    ];

    /// The one place that maps a row of the table to its live goal. The
    /// «normal» row has none and keeps the ordinary playing and result
    /// screens.
    pub fn for_skill(skill: DrillSkill) -> Option<Self> {
        Some(match skill {
            DrillSkill::Normal => return None,
            DrillSkill::Accuracy => Self::AccuracyGate,
            DrillSkill::Speed => Self::Pacer,
            DrillSkill::Recovery => Self::Recovery,
            DrillSkill::Blind => Self::Blind,
            DrillSkill::Anticipation => Self::Anticipation,
            DrillSkill::Preview => Self::Preview,
            DrillSkill::Overlap => Self::Overlap,
            DrillSkill::Spelling => Self::Spelling,
            DrillSkill::Weak => Self::WeakTransition,
            DrillSkill::Rhythm => Self::Metronome,
            DrillSkill::Burst => Self::Sprint,
            DrillSkill::Endurance => Self::EnduranceSplit,
            DrillSkill::Unlock => Self::Reach,
        })
    }

    /// The goal of a menu step: the goal of the row of its cell
    /// (`step_cell`). The warm-up and check steps are «normal» cells.
    pub fn for_step(step: &MenuStep) -> Option<Self> {
        super::step_cell(step).and_then(|cell| Self::for_skill(cell.skill))
    }

    /// The catalog entry of the row: its first level gives a table cell the
    /// fixed lines (the gate, the 15% and 80% thresholds).
    pub fn prescription_id(self) -> &'static str {
        match self {
            Self::AccuracyGate => "accuracy.gate",
            Self::Anticipation => "accuracy.anticipation",
            Self::Metronome => "rhythm.metronome",
            Self::Pacer => "speed.pacer",
            Self::EnduranceSplit => "endurance.split",
            Self::Sprint => "fixed.finish",
            Self::Recovery => "recovery.retry",
            Self::Blind => "blind.fade",
            Self::Preview => "preview.wide",
            Self::Overlap => "overlap.burst",
            Self::Spelling => "japanese.spelling",
            Self::WeakTransition => "transition.adaptive",
            Self::Reach => "reach.unlock",
        }
    }

    /// The metric whose own median is the target line, if any. A menu step
    /// reads it once when the step starts (`progress::own_median_for_metric`).
    pub fn own_median_metric(self) -> Option<MenuMetric> {
        match self {
            Self::Anticipation => Some(MenuMetric::AnticipationRate),
            Self::Metronome => Some(MenuMetric::WithinWordIkiCv),
            Self::Recovery => Some(MenuMetric::PostErrorExcessMs),
            Self::Preview => Some(MenuMetric::WordOnsetRatio),
            Self::Overlap => Some(MenuMetric::RolloverRate),
            Self::Reach => Some(MenuMetric::FarHomeIkiRatio),
            Self::AccuracyGate | Self::Pacer | Self::EnduranceSplit | Self::Sprint | Self::Blind | Self::Spelling | Self::WeakTransition => None,
        }
    }

    /// Whether the line is the player's own median (of the check steps, or
    /// of the cell's earlier sessions).
    pub fn uses_own_median(self) -> bool {
        self.own_median_metric().is_some() || self == Self::Blind
    }

    /// Which way of the main value is better.
    pub fn direction(self) -> Direction {
        match self {
            Self::AccuracyGate | Self::Pacer | Self::Sprint | Self::Blind | Self::Overlap | Self::Spelling => Direction::Higher,
            Self::Anticipation | Self::Metronome | Self::EnduranceSplit | Self::Recovery | Self::Preview | Self::WeakTransition | Self::Reach => {
                Direction::Lower
            }
        }
    }

    /// Whether `value` is better than `other` in this goal's direction.
    pub fn better(self, value: f64, other: f64) -> bool {
        match self.direction() {
            Direction::Higher => value > other,
            Direction::Lower => value < other,
        }
    }
}

/// Where the goal was started from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoalOrigin {
    /// A step of today's menu.
    Menu,
    /// A cell of the individual-training table.
    Drill(DrillCell),
}

/// A target transition of the weak-transition row with the estimate it is
/// compared with (the player model's estimated IKI before the session).
#[derive(Clone, Debug, PartialEq)]
pub struct TargetTransition {
    pub from: String,
    pub to: String,
    pub prior_ms: Option<f64>,
}

impl TargetTransition {
    pub fn label(&self) -> String {
        format!("{}{}", self.from, self.to)
    }
}

/// The latest keystroke pair that typed a target transition.
#[derive(Clone, Debug, PartialEq)]
pub struct TransitionHit {
    pub label: String,
    pub ms: f64,
    pub prior_ms: Option<f64>,
}

/// What the goal needs from the step's configuration when it starts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GoalSetup {
    /// `gate.min_accuracy` of the step.
    pub gate_min_accuracy: f64,
    /// `pacer.target_wpm` of the step (set when the session starts).
    pub pacer_target_wpm: Option<f64>,
    /// The planned length of the step.
    pub planned: Duration,
    /// The own median of [`GoalKind::own_median_metric`].
    pub own_median: Option<f64>,
    /// WPM of the sprints already typed in this step (`fixed.finish`).
    pub previous_runs: Vec<f64>,
    /// Values of the cell's earlier sessions, oldest first (a table cell).
    pub history: Vec<f64>,
    /// The own median is the median of this many of the cell's latest
    /// sessions (a table cell); `None` when it comes from the check steps.
    pub baseline_runs: Option<usize>,
    /// The suggested spelling (spelling row).
    pub spelling: Option<SpellingTarget>,
    /// The target transitions (weak-transition row).
    pub transitions: Vec<TargetTransition>,
    /// The analysis settings and time the judgment measures with.
    pub analysis: AnalysisSettings,
    pub as_of: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LiveGoal {
    pub kind: GoalKind,
    pub origin: GoalOrigin,
    pub prescription_id: String,
    pub level: Option<u8>,
    pub condition: Option<AdvanceCondition>,
    /// The target line: the gate, the own median, the pacer target, the
    /// threshold, the best earlier sprint or the transitions' estimate.
    /// `None` draws no line.
    pub target: Option<f64>,
    pub own_median: Option<f64>,
    pub pacer_target_wpm: Option<f64>,
    pub planned_us: u64,
    pub previous_runs: Vec<f64>,
    pub history: Vec<f64>,
    pub baseline_runs: Option<usize>,
    pub spelling: Option<SpellingTarget>,
    pub transitions: Vec<TargetTransition>,
    pub analysis: AnalysisSettings,
    pub as_of: DateTime<Utc>,
}

impl LiveGoal {
    /// The goal of a menu step.
    pub fn new(step: &MenuStep, setup: GoalSetup) -> Option<Self> {
        let kind = GoalKind::for_step(step)?;
        Some(Self::from_parts(
            kind,
            GoalOrigin::Menu,
            step.prescription_id.clone(),
            step.level,
            step.advance_condition.clone(),
            setup,
        ))
    }

    /// The goal of a session started from a cell of the table. The fixed
    /// lines come from the first level of the row's catalog entry.
    pub fn for_cell(cell: DrillCell, setup: GoalSetup) -> Option<Self> {
        let kind = GoalKind::for_skill(cell.skill)?;
        let condition = super::bundled_catalog()
            .ok()
            .and_then(|catalog| catalog.prescription(kind.prescription_id()))
            .and_then(|prescription| prescription.levels.first())
            .map(|level| level.advance.clone());
        Some(Self::from_parts(
            kind,
            GoalOrigin::Drill(cell),
            kind.prescription_id().to_owned(),
            None,
            condition,
            setup,
        ))
    }

    fn from_parts(
        kind: GoalKind,
        origin: GoalOrigin,
        prescription_id: String,
        level: Option<u8>,
        condition: Option<AdvanceCondition>,
        setup: GoalSetup,
    ) -> Self {
        let target = match kind {
            GoalKind::AccuracyGate => (setup.gate_min_accuracy > 0.0).then_some(setup.gate_min_accuracy),
            GoalKind::Anticipation
            | GoalKind::Metronome
            | GoalKind::Recovery
            | GoalKind::Blind
            | GoalKind::Preview
            | GoalKind::Overlap
            | GoalKind::Reach => setup.own_median,
            GoalKind::Pacer => setup.pacer_target_wpm,
            GoalKind::EnduranceSplit | GoalKind::Spelling => condition.as_ref().and_then(|condition| condition.threshold),
            GoalKind::Sprint => setup.previous_runs.iter().copied().max_by(f64::total_cmp),
            GoalKind::WeakTransition => mean(setup.transitions.iter().filter_map(|transition| transition.prior_ms)),
        };
        Self {
            kind,
            origin,
            prescription_id,
            level,
            condition,
            target,
            own_median: setup.own_median,
            pacer_target_wpm: setup.pacer_target_wpm,
            planned_us: setup.planned.as_micros().min(u128::from(u64::MAX)) as u64,
            previous_runs: setup.previous_runs,
            history: setup.history,
            baseline_runs: setup.baseline_runs,
            spelling: setup.spelling,
            transitions: setup.transitions,
            analysis: setup.analysis,
            as_of: setup.as_of,
        }
    }

    /// The table cell, for a session started from the table.
    pub fn cell(&self) -> Option<DrillCell> {
        match self.origin {
            GoalOrigin::Drill(cell) => Some(cell),
            GoalOrigin::Menu => None,
        }
    }

    /// The `duration_us` to give the live log before `SessionRecord::from_log`:
    /// the planned length for the half split of `endurance.split` and for the
    /// recovery row (the finished step has exactly that length), the time so
    /// far otherwise (speed is per elapsed time).
    pub fn live_duration_us(&self, elapsed_us: u64) -> u64 {
        match self.kind {
            // The recovery row's player model weighs the session by its end
            // time (`finished_at`), so it is measured as ending when planned.
            GoalKind::EnduranceSplit | GoalKind::Recovery => self.planned_us.max(elapsed_us),
            _ => elapsed_us,
        }
    }

    /// 1-based number of the sprint being typed (`fixed.finish`).
    pub fn run_number(&self) -> usize {
        self.previous_runs.len() + 1
    }

    /// The value of the cell's previous session (a table cell).
    pub fn previous_value(&self) -> Option<f64> {
        self.history.last().copied()
    }

    /// The best value of the cell's earlier sessions (a table cell).
    pub fn best_value(&self) -> Option<f64> {
        let kind = self.kind;
        self.history
            .iter()
            .copied()
            .reduce(|best, value| if kind.better(value, best) { value } else { best })
    }

    /// Measure `record` with the functions that record and judge the step.
    pub fn measure(&self, record: &SessionRecord, layout: &KeyboardLayout) -> GoalMeasure {
        let records = std::slice::from_ref(record);
        let mut measure = GoalMeasure {
            keystrokes: record.log.keystrokes.iter().filter(|key| key.kind == KeystrokeKind::Char).count() as u64,
            accuracy: record.header.accuracy,
            net_wpm: record.header.net_wpm,
            ..GoalMeasure::default()
        };
        // Streaks of correct keystrokes (the combo of the playing screen
        // counts words, not keystrokes). Backspace neither adds nor breaks.
        for key in record.log.keystrokes.iter().filter(|key| key.kind == KeystrokeKind::Char) {
            measure.streak = if key.correct { measure.streak + 1 } else { 0 };
            measure.longest_streak = measure.longest_streak.max(measure.streak);
        }
        let geometry = || crate::layout::geometry::geometry(&record.header.geometry).or_else(|| crate::layout::geometry::geometry("ansi"));
        match self.kind {
            GoalKind::AccuracyGate => {
                measure.value = (measure.keystrokes > 0).then_some(record.header.accuracy);
                measure.met = current_metric_value(MenuMetric::AccuracyGatePassed, record).map(|value| value >= 1.0);
            }
            GoalKind::Anticipation => {
                if let Some(geometry) = geometry() {
                    let kinds = crate::analysis::errors::error_kind_counts(records, layout, &geometry);
                    measure.value = super::progress::anticipation_rate_of_kinds(&kinds);
                    measure.anticipations = kinds
                        .iter()
                        .filter(|row| row.kind == crate::analysis::errors::ErrorKind::Anticipation)
                        .map(|row| row.n)
                        .sum();
                    measure.first_errors = kinds
                        .iter()
                        .filter(|row| row.kind != crate::analysis::errors::ErrorKind::Consecutive)
                        .map(|row| row.n)
                        .sum();
                }
            }
            GoalKind::Metronome => {
                let rhythm = crate::analysis::style::rhythm_breakdown(records);
                measure.value = rhythm.within_word_cv;
                measure.intervals = crate::analysis::style::within_word_intervals_ms(record);
            }
            GoalKind::Pacer => {
                measure.value = (measure.keystrokes > 0).then_some(record.header.net_wpm);
                measure.met = current_metric_value(MenuMetric::PacerTargetReached, record).map(|value| value >= 1.0);
            }
            GoalKind::EnduranceSplit => {
                let comparison = crate::practice::session_variants::compare_endurance_log(&record.log);
                measure.value = current_metric_value(MenuMetric::EnduranceSecondHalfSlowdown, record);
                measure.endurance = Some(comparison);
            }
            GoalKind::Sprint => {
                measure.value = (measure.keystrokes > 0).then_some(record.header.net_wpm);
                measure.met = self.target.map(|best| record.header.net_wpm > best);
            }
            GoalKind::Recovery => {
                if let Some(geometry) = geometry() {
                    let detail = crate::analysis::errors::post_error_detail(records, layout, &geometry, self.analysis, self.as_of);
                    measure.value = detail.slowdown.as_ref().map(|slowdown| slowdown.excess_ms);
                    measure.recoveries = detail.per_error_excess_ms.len() as u64;
                    measure.last_recovery_ms = detail.per_error_excess_ms.last().copied();
                }
                measure.errors = record
                    .log
                    .keystrokes
                    .iter()
                    .filter(|key| key.kind == KeystrokeKind::Char && !key.correct)
                    .count() as u64;
            }
            GoalKind::Blind => {
                measure.value = (measure.keystrokes > 0).then_some(record.header.accuracy);
            }
            GoalKind::Preview => {
                let (starts, internal) = crate::analysis::style::word_intervals_ms(records);
                let metric = crate::analysis::style::word_start_from(&starts, &internal);
                measure.value = metric.as_ref().map(|metric| metric.ratio);
                measure.words_started = starts.len() as u64;
                if let Some(metric) = metric.as_ref() {
                    let limit = metric.internal_median_ms * ONSET_STOP_FACTOR;
                    for start in &starts {
                        measure.onset_run = if *start <= limit { measure.onset_run + 1 } else { 0 };
                        measure.longest_onset_run = measure.longest_onset_run.max(measure.onset_run);
                    }
                }
            }
            GoalKind::Overlap => {
                if let Some(rollover) = crate::analysis::style::rollover_metric(records) {
                    measure.value = Some(rollover.rate * 100.0);
                    measure.overlap_pairs = rollover.overlap_pairs;
                    measure.comparable_pairs = rollover.comparable_pairs;
                }
            }
            GoalKind::Spelling => {
                if let Some(target) = self.spelling.as_ref() {
                    let usage = crate::analysis::japanese::spelling_usage(records, &target.kana, &target.candidate);
                    measure.value = usage.rate();
                    measure.spelling_used = usage.used;
                    measure.spelling_total = usage.total;
                }
            }
            GoalKind::WeakTransition => {
                if let Some(geometry) = geometry() {
                    let details = crate::analysis::metrics::analyze_records(records, layout, &geometry, self.analysis);
                    let observed = target_transition_medians(&details, &self.transitions);
                    measure.value = mean(observed.iter().map(|(_, median, _)| *median));
                    measure.line = mean(observed.iter().filter_map(|(transition, _, _)| transition.prior_ms));
                    measure.transition_hits = observed.iter().map(|(_, _, n)| n).sum();
                    measure.transition_rows = observed
                        .iter()
                        .map(|(transition, median, n)| (transition.label(), *median, transition.prior_ms, *n))
                        .collect();
                }
                measure.last_hit = last_transition_hit(record, &self.transitions, self.analysis.pause_ms);
            }
            GoalKind::Reach => {
                measure.value = crate::analysis::summary::session_reach_ratio(record, layout, self.analysis);
            }
        }
        if measure.met.is_none() {
            let line = measure.line.or(self.target);
            measure.met = match self.kind {
                GoalKind::AccuracyGate | GoalKind::Pacer | GoalKind::Sprint => None,
                GoalKind::EnduranceSplit => measure.value.zip(line).map(|(value, threshold)| value < threshold),
                kind => measure.value.zip(line).map(|(value, line)| match kind.direction() {
                    Direction::Higher => value >= line,
                    Direction::Lower => value <= line,
                }),
            };
        }
        measure
    }

    /// The line the meter draws: the transitions' estimate of the ones typed
    /// so far for the weak-transition row, the target otherwise.
    pub fn line(&self, measure: &GoalMeasure) -> Option<f64> {
        match self.kind {
            GoalKind::WeakTransition => measure.line.or(self.target),
            _ => self.target,
        }
    }

    /// Whether `elapsed` is in the second half of the step.
    pub fn in_second_half(&self, elapsed: Duration) -> bool {
        let elapsed_us = elapsed.as_micros().min(u128::from(u64::MAX)) as u64;
        self.planned_us > 0 && elapsed_us >= self.planned_us / 2
    }

    /// The tone of the main value: met, close to the line, or off it.
    pub fn tone(&self, measure: &GoalMeasure) -> Tone {
        let Some(value) = measure.value else {
            return Tone::Neutral;
        };
        match self.kind {
            GoalKind::AccuracyGate => match self.target {
                Some(gate) if value < gate => Tone::Bad,
                Some(gate) if value - gate < GATE_WARNING_MARGIN => Tone::Warning,
                _ => Tone::Good,
            },
            GoalKind::Anticipation | GoalKind::Metronome => match self.target {
                None => Tone::Neutral,
                Some(median) if value > median => Tone::Bad,
                Some(median) if value > median * 0.9 => Tone::Warning,
                Some(_) => Tone::Good,
            },
            GoalKind::Pacer => match self.target {
                None => Tone::Neutral,
                Some(target) if value >= target && measure.accuracy >= PACER_ACCURACY_FLOOR => Tone::Good,
                Some(target) if value >= target * 0.95 => Tone::Warning,
                Some(_) => Tone::Bad,
            },
            GoalKind::EnduranceSplit => match self.target {
                None => Tone::Neutral,
                Some(threshold) if value >= threshold => Tone::Bad,
                Some(threshold) if value >= threshold * (2.0 / 3.0) => Tone::Warning,
                Some(_) => Tone::Good,
            },
            GoalKind::Sprint => match self.target {
                Some(best) if value > best => Tone::Good,
                _ => Tone::Neutral,
            },
            GoalKind::Spelling => match self.target {
                None => Tone::Neutral,
                Some(threshold) if value >= threshold => Tone::Good,
                Some(threshold) if value >= threshold - 0.1 => Tone::Warning,
                Some(_) => Tone::Bad,
            },
            // The own-median rows and the transitions' estimate: off the
            // line is bad, within a tenth of it on the good side a warning.
            GoalKind::Recovery | GoalKind::Blind | GoalKind::Preview | GoalKind::Overlap | GoalKind::WeakTransition | GoalKind::Reach => {
                let Some(line) = self.line(measure) else {
                    return Tone::Neutral;
                };
                let margin = if self.kind == GoalKind::Blind {
                    GATE_WARNING_MARGIN
                } else {
                    line.abs() * 0.1
                };
                match self.kind.direction() {
                    Direction::Higher if value < line => Tone::Bad,
                    Direction::Higher if value < line + margin => Tone::Warning,
                    Direction::Lower if value > line => Tone::Bad,
                    Direction::Lower if value > line - margin => Tone::Warning,
                    _ => Tone::Good,
                }
            }
        }
    }

    /// The range of the meter, chosen so that the target line and usual
    /// values are visible.
    pub fn meter_range(&self, measure: &GoalMeasure) -> (f64, f64) {
        let value = measure.value.unwrap_or(0.0);
        let line = self.line(measure);
        match self.kind {
            GoalKind::AccuracyGate => (0.85, 1.0),
            GoalKind::Anticipation => (0.0, self.target.map_or(0.5, |median| (median * 2.0).max(0.2)).max(value)),
            GoalKind::Metronome => (0.0, self.target.map_or(0.8, |median| (median * 2.0).max(0.3)).max(value)),
            GoalKind::Pacer | GoalKind::Sprint => {
                let reference = self.target.unwrap_or(value).max(value).max(1.0);
                (0.0, reference * 1.25)
            }
            GoalKind::EnduranceSplit => (-0.15, self.target.map_or(0.3, |threshold| threshold * 2.0).max(value)),
            GoalKind::Recovery => {
                let reference = line.unwrap_or(value).abs().max(value.abs()).max(50.0);
                let low = value.min(line.unwrap_or(0.0)).min(0.0) - 20.0;
                (low.min(-reference * 0.25), reference * 2.0)
            }
            GoalKind::Blind => (line.map_or(0.85, |line| (line - 0.05).min(0.85)).max(0.0), 1.0),
            GoalKind::Preview => (0.0, line.map_or(3.0, |median| (median * 2.0).max(1.5)).max(value)),
            GoalKind::Overlap => (0.0, line.map_or(60.0, |median| (median * 2.0).max(20.0)).max(value).min(100.0)),
            GoalKind::Spelling => (0.0, 1.0),
            GoalKind::WeakTransition => (0.0, line.unwrap_or(value).max(value).max(1.0) * 1.6),
            GoalKind::Reach => (0.0, line.map_or(2.0, |median| (median * 2.0).max(1.5)).max(value)),
        }
    }
}

/// The session medians of the target transitions typed so far, with their
/// number of intervals: the bigram rows of `analyze_records` (the rows the
/// adaptive result reads), counted only when they have an interval.
pub fn target_transition_medians<'a>(
    details: &crate::analysis::metrics::DetailedMetrics,
    transitions: &'a [TargetTransition],
) -> Vec<(&'a TargetTransition, f64, u64)> {
    transitions
        .iter()
        .filter_map(|transition| {
            details
                .bigrams
                .iter()
                .find(|row| row.from == transition.from && row.to == transition.to && row.n > 0)
                .map(|row| (transition, row.median_ms, row.n))
        })
        .collect()
}

/// The last keystroke pair, when it typed a target transition correctly
/// (the pair `analyze_records` would count: both correct, within the pause).
fn last_transition_hit(record: &SessionRecord, transitions: &[TargetTransition], pause_ms: u64) -> Option<TransitionHit> {
    let mut keys = record.log.keystrokes.iter().rev().filter(|key| key.kind == KeystrokeKind::Char);
    let current = keys.next()?;
    let previous = keys.next()?;
    if !current.correct || !previous.correct || current.item_idx != previous.item_idx {
        return None;
    }
    let delta_us = current.t_us.saturating_sub(previous.t_us);
    if delta_us > pause_ms.saturating_mul(1_000) {
        return None;
    }
    let from = previous.expected.as_deref()?.chars().next()?.to_string();
    let to = current.expected.as_deref()?.chars().next()?.to_string();
    let transition = transitions.iter().find(|transition| transition.from == from && transition.to == to)?;
    Some(TransitionHit {
        label: transition.label(),
        ms: delta_us as f64 / 1_000.0,
        prior_ms: transition.prior_ms,
    })
}

fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let (sum, count) = values
        .filter(|value| value.is_finite())
        .fold((0.0, 0_u32), |(sum, count), value| (sum + value, count + 1));
    (count > 0).then(|| sum / f64::from(count))
}

/// The tone a value is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Good,
    Warning,
    Bad,
    Neutral,
}

/// One measurement of the step so far (or of the finished step).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GoalMeasure {
    /// The main value of the goal; `None` while it cannot be computed.
    pub value: Option<f64>,
    /// Whether the value meets the goal now; `None` when it cannot be told.
    pub met: Option<bool>,
    pub keystrokes: u64,
    pub accuracy: f64,
    pub net_wpm: f64,
    /// Correct keystrokes in a row now, and the longest run of them.
    pub streak: u32,
    pub longest_streak: u32,
    /// `accuracy.anticipation`: anticipation errors and first errors.
    pub anticipations: u64,
    pub first_errors: u64,
    /// `rhythm.metronome`: every within-word interval (ms) in order.
    pub intervals: Vec<f64>,
    /// `endurance.split`: the half comparison.
    pub endurance: Option<EnduranceComparison>,
    /// Recovery: mistyped keystrokes, errors whose recovery was measured,
    /// and the extra time (ms) after the latest one.
    pub errors: u64,
    pub recoveries: u64,
    pub last_recovery_ms: Option<f64>,
    /// Look-ahead: word starts, and words started without a stop in a row.
    pub words_started: u64,
    pub onset_run: u32,
    pub longest_onset_run: u32,
    /// Rollover: overlapping and comparable key pairs.
    pub overlap_pairs: u64,
    pub comparable_pairs: u64,
    /// Spelling: occurrences of the kana typed with the suggested spelling,
    /// and all of them.
    pub spelling_used: u64,
    pub spelling_total: u64,
    /// Weak transitions: intervals of the targets, the estimate of the ones
    /// typed so far (the meter's line) and the latest target pair.
    pub transition_hits: u64,
    pub line: Option<f64>,
    pub last_hit: Option<TransitionHit>,
    /// Weak transitions: each target typed so far with its session median
    /// (ms), its estimate and its intervals.
    pub transition_rows: Vec<(String, f64, Option<f64>, u64)>,
}

/// The goal together with what the screen shows of it.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveGoalState {
    pub goal: LiveGoal,
    /// The latest live measurement.
    pub measure: GoalMeasure,
    /// The measurement of the finished step (set when the step ends).
    pub finished: Option<GoalMeasure>,
    /// Session time of the latest new anticipation error.
    pub anticipation_at: Option<Duration>,
    /// Session time of the latest new record (longest streak, fastest pace,
    /// longest run of words).
    pub record_at: Option<Duration>,
    /// Session time of the latest event the note announces: a recovery
    /// measured after an error, or a target transition typed.
    pub event_at: Option<Duration>,
}

impl LiveGoalState {
    pub fn new(goal: LiveGoal) -> Self {
        Self {
            goal,
            measure: GoalMeasure::default(),
            finished: None,
            anticipation_at: None,
            record_at: None,
            event_at: None,
        }
    }

    /// Update after a keystroke typed at `elapsed`.
    pub fn update(&mut self, record: &SessionRecord, elapsed: Duration, layout: &KeyboardLayout) {
        let measure = self.goal.measure(record, layout);
        if measure.anticipations > self.measure.anticipations {
            self.anticipation_at = Some(elapsed);
        }
        let new_record = match self.goal.kind {
            // A longer streak than any earlier one in this step.
            GoalKind::AccuracyGate | GoalKind::Blind => self.measure.longest_streak >= 10 && measure.longest_streak > self.measure.longest_streak,
            // Faster than the best earlier sprint, once the sprint has begun.
            GoalKind::Sprint => {
                let was_ahead = self.goal.target.zip(self.measure.value).is_some_and(|(best, value)| value > best);
                let ahead = self.goal.target.zip(measure.value).is_some_and(|(best, value)| value > best);
                measure.keystrokes >= 5 && ahead && !was_ahead
            }
            // A longer run of words without a stop.
            GoalKind::Preview => self.measure.longest_onset_run >= 5 && measure.longest_onset_run > self.measure.longest_onset_run,
            _ => false,
        };
        if new_record {
            self.record_at = Some(elapsed);
        }
        let event = match self.goal.kind {
            GoalKind::Recovery => measure.recoveries > self.measure.recoveries,
            GoalKind::WeakTransition => measure.last_hit.is_some() && measure.transition_hits > self.measure.transition_hits,
            _ => false,
        };
        if event {
            self.event_at = Some(elapsed);
        }
        self.measure = measure;
    }

    /// Record the measurement of the finished step.
    pub fn finish(&mut self, record: &SessionRecord, layout: &KeyboardLayout) {
        self.finished = Some(self.goal.measure(record, layout));
    }

    pub fn anticipation_flash(&self, now: Duration) -> bool {
        self.anticipation_at.is_some_and(|at| now.saturating_sub(at) < FLASH_DURATION)
    }

    pub fn record_flash(&self, now: Duration) -> bool {
        self.record_at.is_some_and(|at| now.saturating_sub(at) < FLASH_DURATION)
    }

    pub fn event_flash(&self, now: Duration) -> bool {
        self.event_at.is_some_and(|at| now.saturating_sub(at) < FLASH_DURATION)
    }
}

/// Counts of `values` in `bins` equal-width bins over `[low, high)`; values
/// at or above `high` fall into the last bin.
pub fn histogram(values: &[f64], low: f64, high: f64, bins: usize) -> Vec<u32> {
    let mut counts = vec![0_u32; bins.max(1)];
    let width = (high - low).max(f64::EPSILON) / counts.len() as f64;
    for value in values.iter().copied().filter(|value| value.is_finite()) {
        let index = (((value - low) / width).floor().max(0.0) as usize).min(counts.len() - 1);
        counts[index] += 1;
    }
    counts
}

/// Positions (0..slots) of the mistyped keystrokes along the step's time.
pub fn mistype_slots(record: &SessionRecord, slots: usize) -> Vec<bool> {
    let mut marks = vec![false; slots];
    let duration = record.log.duration_us.max(1);
    if slots == 0 {
        return marks;
    }
    for key in record.log.keystrokes.iter().filter(|key| key.kind == KeystrokeKind::Char && !key.correct) {
        let index = ((key.t_us as f64 / duration as f64) * slots as f64).floor() as usize;
        marks[index.min(slots - 1)] = true;
    }
    marks
}
