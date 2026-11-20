//! The live goal of a menu step (MYTASK-3633).
//!
//! A prescription step of today's menu has a graduation condition
//! (`advance`). This module turns it into a goal the player can watch while
//! typing — a metric, its current value, a target line and the direction that
//! counts as better — and into the headline of the result screen.
//!
//! Every value comes from the computation that records and judges the step
//! (`current_metric_value`, `anticipation_rate_of_kinds`, the rhythm
//! breakdown behind the rhythm grade, `compare_endurance_log`), applied to a
//! [`SessionRecord`] built from the live log with `SessionRecord::from_log`.
//! The last live value therefore equals the value of the finished step.
//!
//! [`GoalKind::for_step`] is the only place that maps a step to a goal; a new
//! prescription is added there and in the exhaustive matches that follow.

use std::time::Duration;

use super::{AdvanceCondition, MenuMetric, MenuStep, PACER_ACCURACY_FLOOR, current_metric_value};
use crate::analysis::grades::Direction;
use crate::layout::KeyboardLayout;
use crate::practice::session_variants::EnduranceComparison;
use crate::record::SessionRecord;

/// How long a short notice («先走り!», «最速ペース!») stays on screen.
pub const FLASH_DURATION: Duration = Duration::from_millis(1_200);
/// Accuracy within this margin above the gate is shown as a warning.
pub const GATE_WARNING_MARGIN: f64 = 0.01;
/// How many recent within-word intervals the rhythm band shows.
pub const RECENT_INTERVALS: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoalKind {
    /// `accuracy.gate`: accuracy against the gate of the step.
    AccuracyGate,
    /// `accuracy.anticipation`: share of anticipation errors against the
    /// player's own median.
    Anticipation,
    /// `rhythm.metronome`: within-word IKI CV against the own median.
    Metronome,
    /// `speed.pacer`: net WPM against the pacer target, with the accuracy
    /// floor of `target-with-accuracy`.
    Pacer,
    /// `endurance.split`: second-half slowdown against the threshold.
    EnduranceSplit,
    /// `fixed.finish`: three sprints; the best one counts.
    Sprint,
}

impl GoalKind {
    /// The one place that maps a menu step to its live goal. Steps without
    /// one (warm-up, check and the prescriptions of the second batch) keep
    /// the ordinary playing and result screens.
    pub fn for_step(step: &MenuStep) -> Option<Self> {
        Some(match step.prescription_id.as_str() {
            "accuracy.gate" => Self::AccuracyGate,
            "accuracy.anticipation" => Self::Anticipation,
            "rhythm.metronome" => Self::Metronome,
            "speed.pacer" => Self::Pacer,
            "endurance.split" => Self::EnduranceSplit,
            "fixed.finish" => Self::Sprint,
            _ => return None,
        })
    }

    /// The metric whose own median is the target line, if any. It is read
    /// once when the step starts (`progress::own_median_for_metric`).
    pub fn own_median_metric(self) -> Option<MenuMetric> {
        match self {
            Self::Anticipation => Some(MenuMetric::AnticipationRate),
            Self::Metronome => Some(MenuMetric::WithinWordIkiCv),
            Self::AccuracyGate | Self::Pacer | Self::EnduranceSplit | Self::Sprint => None,
        }
    }

    /// Which way of the main value is better.
    pub fn direction(self) -> Direction {
        match self {
            Self::AccuracyGate | Self::Pacer | Self::Sprint => Direction::Higher,
            Self::Anticipation | Self::Metronome | Self::EnduranceSplit => Direction::Lower,
        }
    }
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
}

#[derive(Clone, Debug, PartialEq)]
pub struct LiveGoal {
    pub kind: GoalKind,
    pub prescription_id: String,
    pub level: Option<u8>,
    pub condition: Option<AdvanceCondition>,
    /// The target line: the gate, the own median, the pacer target, the
    /// slowdown threshold or the best earlier sprint. `None` draws no line.
    pub target: Option<f64>,
    pub own_median: Option<f64>,
    pub pacer_target_wpm: Option<f64>,
    pub planned_us: u64,
    pub previous_runs: Vec<f64>,
}

impl LiveGoal {
    pub fn new(step: &MenuStep, setup: GoalSetup) -> Option<Self> {
        let kind = GoalKind::for_step(step)?;
        let target = match kind {
            GoalKind::AccuracyGate => (setup.gate_min_accuracy > 0.0).then_some(setup.gate_min_accuracy),
            GoalKind::Anticipation | GoalKind::Metronome => setup.own_median,
            GoalKind::Pacer => setup.pacer_target_wpm,
            GoalKind::EnduranceSplit => step.advance_condition.as_ref().and_then(|condition| condition.threshold),
            GoalKind::Sprint => setup.previous_runs.iter().copied().max_by(f64::total_cmp),
        };
        Some(Self {
            kind,
            prescription_id: step.prescription_id.clone(),
            level: step.level,
            condition: step.advance_condition.clone(),
            target,
            own_median: setup.own_median,
            pacer_target_wpm: setup.pacer_target_wpm,
            planned_us: setup.planned.as_micros().min(u128::from(u64::MAX)) as u64,
            previous_runs: setup.previous_runs,
        })
    }

    /// The `duration_us` to give the live log before `SessionRecord::from_log`:
    /// the planned length for the half split of `endurance.split` (the
    /// finished step has exactly that length), the time so far otherwise
    /// (speed is per elapsed time).
    pub fn live_duration_us(&self, elapsed_us: u64) -> u64 {
        match self.kind {
            GoalKind::EnduranceSplit => self.planned_us.max(elapsed_us),
            _ => elapsed_us,
        }
    }

    /// 1-based number of the sprint being typed (`fixed.finish`).
    pub fn run_number(&self) -> usize {
        self.previous_runs.len() + 1
    }

    /// Measure `record` with the functions that record and judge the step.
    pub fn measure(&self, record: &SessionRecord, layout: &KeyboardLayout) -> GoalMeasure {
        let mut measure = GoalMeasure {
            keystrokes: record
                .log
                .keystrokes
                .iter()
                .filter(|key| key.kind == crate::engine::session::KeystrokeKind::Char)
                .count() as u64,
            accuracy: record.header.accuracy,
            net_wpm: record.header.net_wpm,
            ..GoalMeasure::default()
        };
        // Streaks of correct keystrokes (the combo of the playing screen
        // counts words, not keystrokes). Backspace neither adds nor breaks.
        for key in record
            .log
            .keystrokes
            .iter()
            .filter(|key| key.kind == crate::engine::session::KeystrokeKind::Char)
        {
            measure.streak = if key.correct { measure.streak + 1 } else { 0 };
            measure.longest_streak = measure.longest_streak.max(measure.streak);
        }
        match self.kind {
            GoalKind::AccuracyGate => {
                measure.value = (measure.keystrokes > 0).then_some(record.header.accuracy);
                measure.met = current_metric_value(MenuMetric::AccuracyGatePassed, record).map(|value| value >= 1.0);
            }
            GoalKind::Anticipation => {
                let geometry = crate::layout::geometry::geometry(&record.header.geometry).or_else(|| crate::layout::geometry::geometry("ansi"));
                if let Some(geometry) = geometry {
                    let kinds = crate::analysis::errors::error_kind_counts(std::slice::from_ref(record), layout, &geometry);
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
                measure.met = measure.value.zip(self.own_median).map(|(value, median)| value <= median);
            }
            GoalKind::Metronome => {
                let rhythm = crate::analysis::style::rhythm_breakdown(std::slice::from_ref(record));
                measure.value = rhythm.within_word_cv;
                measure.intervals = crate::analysis::style::within_word_intervals_ms(record);
                measure.met = measure.value.zip(self.own_median).map(|(value, median)| value <= median);
            }
            GoalKind::Pacer => {
                measure.value = (measure.keystrokes > 0).then_some(record.header.net_wpm);
                measure.met = current_metric_value(MenuMetric::PacerTargetReached, record).map(|value| value >= 1.0);
            }
            GoalKind::EnduranceSplit => {
                let comparison = crate::practice::session_variants::compare_endurance_log(&record.log);
                measure.value = current_metric_value(MenuMetric::EnduranceSecondHalfSlowdown, record);
                measure.endurance = Some(comparison);
                measure.met = measure.value.zip(self.target).map(|(value, threshold)| value < threshold);
            }
            GoalKind::Sprint => {
                measure.value = (measure.keystrokes > 0).then_some(record.header.net_wpm);
                measure.met = self.target.map(|best| record.header.net_wpm > best);
            }
        }
        measure
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
        }
    }

    /// The range of the meter, chosen so that the target line and usual
    /// values are visible.
    pub fn meter_range(&self, measure: &GoalMeasure) -> (f64, f64) {
        let value = measure.value.unwrap_or(0.0);
        match self.kind {
            GoalKind::AccuracyGate => (0.85, 1.0),
            GoalKind::Anticipation => (0.0, self.target.map_or(0.5, |median| (median * 2.0).max(0.2)).max(value)),
            GoalKind::Metronome => (0.0, self.target.map_or(0.8, |median| (median * 2.0).max(0.3)).max(value)),
            GoalKind::Pacer | GoalKind::Sprint => {
                let reference = self.target.unwrap_or(value).max(value).max(1.0);
                (0.0, reference * 1.25)
            }
            GoalKind::EnduranceSplit => (-0.15, self.target.map_or(0.3, |threshold| threshold * 2.0).max(value)),
        }
    }
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
    /// Session time of the latest new record (longest streak, fastest pace).
    pub record_at: Option<Duration>,
}

impl LiveGoalState {
    pub fn new(goal: LiveGoal) -> Self {
        Self {
            goal,
            measure: GoalMeasure::default(),
            finished: None,
            anticipation_at: None,
            record_at: None,
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
            GoalKind::AccuracyGate => self.measure.longest_streak >= 10 && measure.longest_streak > self.measure.longest_streak,
            // Faster than the best earlier sprint, once the sprint has begun.
            GoalKind::Sprint => {
                let was_ahead = self.goal.target.zip(self.measure.value).is_some_and(|(best, value)| value > best);
                let ahead = self.goal.target.zip(measure.value).is_some_and(|(best, value)| value > best);
                measure.keystrokes >= 5 && ahead && !was_ahead
            }
            _ => false,
        };
        if new_record {
            self.record_at = Some(elapsed);
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
    for key in record
        .log
        .keystrokes
        .iter()
        .filter(|key| key.kind == crate::engine::session::KeystrokeKind::Char && !key.correct)
    {
        let index = ((key.t_us as f64 / duration as f64) * slots as f64).floor() as usize;
        marks[index.min(slots - 1)] = true;
    }
    marks
}
