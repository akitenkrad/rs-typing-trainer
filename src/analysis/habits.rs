//! Session-state and practice-habit diagnostics.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Duration, Timelike, Utc};
use serde::{Deserialize, Serialize};

use crate::analysis::progress::SessionForm;
use crate::engine::session::KeystrokeKind;
use crate::record::{SessionModeParams, SessionRecord};

#[derive(Clone, Debug, PartialEq)]
pub struct HabitForm {
    pub session_id: Option<i64>,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub form: f64,
    pub mode_params: Option<SessionModeParams>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Warmup {
    pub after_break_n: usize,
    pub other_n: usize,
    pub after_break_form_median: Option<f64>,
    pub other_form_median: Option<f64>,
    pub form_difference: Option<f64>,
    pub first_30s_iki_ratio: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeteriorationBucket {
    pub bucket: String,
    pub iki_median_ms: Option<f64>,
    pub error_rate: Option<f64>,
    pub attempts: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Deterioration {
    pub buckets: Vec<DeteriorationBucket>,
    pub first_deteriorated_bucket: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DailyPractice {
    pub date: chrono::NaiveDate,
    pub minutes: f64,
    pub within_day_gain: Option<f64>,
    pub next_day_retention: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PracticeRelation {
    pub days: Vec<DailyPractice>,
    pub plateau_minutes: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimeOfDay {
    pub bucket: String,
    pub n: usize,
    pub form_median: f64,
    pub exploratory: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AssistanceKind {
    Pacer,
    Ghost,
    Keyboard,
    Preview,
    Blind,
}

impl AssistanceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pacer => "pacer",
            Self::Ghost => "ghost",
            Self::Keyboard => "keyboard",
            Self::Preview => "preview",
            Self::Blind => "blind",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AssistanceEffect {
    pub kind: AssistanceKind,
    pub with_n: usize,
    pub without_n: usize,
    pub with_form_median: Option<f64>,
    pub without_form_median: Option<f64>,
    pub removable: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HabitRow {
    pub kind: String,
    pub bucket: String,
    pub n: usize,
    pub value: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Habits {
    pub warmup: Warmup,
    pub deterioration: Deterioration,
    pub practice: PracticeRelation,
    pub time_of_day: Vec<TimeOfDay>,
    pub assistance: Vec<AssistanceEffect>,
    pub rows: Vec<HabitRow>,
}

pub fn analyze_habits(records: &[SessionRecord], forms: &[SessionForm]) -> Habits {
    let by_id = records
        .iter()
        .filter_map(|record| Some((record.header.id?, record)))
        .collect::<HashMap<_, _>>();
    let habit_forms = forms
        .iter()
        .map(|form| HabitForm {
            session_id: form.session_id,
            started_at: form.started_at,
            finished_at: form.finished_at,
            form: form.form,
            mode_params: form
                .session_id
                .and_then(|id| by_id.get(&id))
                .and_then(|record| serde_json::from_str(&record.header.mode_params).ok()),
        })
        .collect::<Vec<_>>();
    let warmup = analyze_warmup(records, &habit_forms);
    let deterioration = analyze_deterioration(records);
    let practice = analyze_practice(records, &habit_forms);
    let time_of_day = analyze_time_of_day(&habit_forms);
    let assistance = analyze_assistance(&habit_forms);
    let mut rows = Vec::new();
    rows.push(HabitRow {
        kind: "warmup-form".to_owned(),
        bucket: "after-3h-break".to_owned(),
        n: warmup.after_break_n,
        value: warmup.after_break_form_median,
    });
    rows.push(HabitRow {
        kind: "warmup-form".to_owned(),
        bucket: "other".to_owned(),
        n: warmup.other_n,
        value: warmup.other_form_median,
    });
    for bucket in &deterioration.buckets {
        rows.push(HabitRow {
            kind: "deterioration-iki-ms".to_owned(),
            bucket: bucket.bucket.clone(),
            n: bucket.attempts as usize,
            value: bucket.iki_median_ms,
        });
        rows.push(HabitRow {
            kind: "deterioration-error-rate".to_owned(),
            bucket: bucket.bucket.clone(),
            n: bucket.attempts as usize,
            value: bucket.error_rate,
        });
    }
    for row in &time_of_day {
        rows.push(HabitRow {
            kind: "time-of-day".to_owned(),
            bucket: row.bucket.clone(),
            n: row.n,
            value: Some(row.form_median),
        });
    }
    for effect in &assistance {
        rows.push(HabitRow {
            kind: effect.kind.as_str().to_owned(),
            bucket: "with".to_owned(),
            n: effect.with_n,
            value: effect.with_form_median,
        });
        rows.push(HabitRow {
            kind: effect.kind.as_str().to_owned(),
            bucket: "without".to_owned(),
            n: effect.without_n,
            value: effect.without_form_median,
        });
    }
    Habits {
        warmup,
        deterioration,
        practice,
        time_of_day,
        assistance,
        rows,
    }
}

pub fn analyze_assistance(forms: &[HabitForm]) -> Vec<AssistanceEffect> {
    [
        AssistanceKind::Pacer,
        AssistanceKind::Ghost,
        AssistanceKind::Keyboard,
        AssistanceKind::Preview,
        AssistanceKind::Blind,
    ]
    .into_iter()
    .map(|kind| {
        let mut with = Vec::new();
        let mut without = Vec::new();
        for form in forms {
            let Some(params) = &form.mode_params else { continue };
            let enabled = match kind {
                AssistanceKind::Pacer => params.pacer_enabled,
                AssistanceKind::Ghost => params.ghost.as_ref().map(|ghost| ghost.enabled),
                AssistanceKind::Keyboard => params.keyboard_show.as_deref().map(|show| show != "hidden"),
                AssistanceKind::Preview => params.preview_items.map(|items| items > 0),
                AssistanceKind::Blind => params.blind,
            };
            match enabled {
                Some(true) => with.push(form.form),
                Some(false) => without.push(form.form),
                None => {}
            }
        }
        let with_median = median(&with);
        let without_median = median(&without);
        AssistanceEffect {
            kind,
            with_n: with.len(),
            without_n: without.len(),
            with_form_median: with_median,
            without_form_median: without_median,
            removable: with.len() >= 5 && without.len() >= 5 && with_median.zip(without_median).is_some_and(|(with, without)| without >= with * 0.97),
        }
    })
    .collect()
}

fn analyze_warmup(records: &[SessionRecord], forms: &[HabitForm]) -> Warmup {
    let mut ordered = forms.to_vec();
    ordered.sort_by_key(|form| form.started_at);
    let mut after_break = Vec::new();
    let mut other = Vec::new();
    for (index, form) in ordered.iter().enumerate() {
        if index == 0 || form.started_at - ordered[index - 1].finished_at >= Duration::hours(3) {
            after_break.push(form.form);
        } else {
            other.push(form.form);
        }
    }
    let after_break_median = median(&after_break);
    let other_median = median(&other);
    let mut early = Vec::new();
    let mut later = Vec::new();
    for record in records {
        let keys = record
            .log
            .keystrokes
            .iter()
            .filter(|key| key.kind == KeystrokeKind::Char && key.correct)
            .collect::<Vec<_>>();
        for pair in keys.windows(2).filter(|pair| pair[0].item_idx == pair[1].item_idx) {
            let delta = pair[1].t_us.saturating_sub(pair[0].t_us) as f64 / 1_000.0;
            if delta <= 0.0 {
                continue;
            }
            if pair[1].t_us <= 30_000_000 {
                early.push(delta);
            } else {
                later.push(delta);
            }
        }
    }
    let first_30s_iki_ratio = median(&early)
        .zip(median(&later))
        .and_then(|(early, later)| (later > 0.0).then_some(early / later));
    Warmup {
        after_break_n: after_break.len(),
        other_n: other.len(),
        after_break_form_median: after_break_median,
        other_form_median: other_median,
        form_difference: after_break_median.zip(other_median).map(|(after, other)| after - other),
        first_30s_iki_ratio,
    }
}

#[derive(Default)]
struct BucketSamples {
    iki: Vec<f64>,
    errors: u64,
    attempts: u64,
}

fn analyze_deterioration(records: &[SessionRecord]) -> Deterioration {
    let mut ordered = records
        .iter()
        .filter_map(|record| parse_time(&record.header.started_at).map(|started| (started, record)))
        .collect::<Vec<_>>();
    ordered.sort_by_key(|row| row.0);
    let mut buckets = [
        BucketSamples::default(),
        BucketSamples::default(),
        BucketSamples::default(),
        BucketSamples::default(),
    ];
    let mut cluster_start = None;
    let mut previous_end = None;
    for (started, record) in ordered {
        let finished = parse_time(&record.header.finished_at).unwrap_or(started);
        if previous_end.is_none_or(|end| started - end >= Duration::minutes(10)) {
            cluster_start = Some(started);
        }
        previous_end = Some(finished);
        let elapsed_us = (started - cluster_start.unwrap_or(started)).num_microseconds().unwrap_or_default().max(0) as u64;
        let keys = record
            .log
            .keystrokes
            .iter()
            .filter(|key| key.kind == KeystrokeKind::Char)
            .collect::<Vec<_>>();
        for (index, key) in keys.iter().enumerate() {
            let minute = (elapsed_us + key.t_us) as f64 / 60_000_000.0;
            let bucket = if minute < 5.0 {
                0
            } else if minute < 10.0 {
                1
            } else if minute < 20.0 {
                2
            } else {
                3
            };
            buckets[bucket].attempts += 1;
            buckets[bucket].errors += u64::from(!key.correct);
            if index > 0 && keys[index - 1].correct && key.correct && keys[index - 1].item_idx == key.item_idx {
                buckets[bucket].iki.push(key.t_us.saturating_sub(keys[index - 1].t_us) as f64 / 1_000.0);
            }
        }
    }
    let names = ["0-5", "5-10", "10-20", "20+"];
    let rows = buckets
        .iter()
        .zip(names)
        .map(|(samples, name)| DeteriorationBucket {
            bucket: name.to_owned(),
            iki_median_ms: median(&samples.iki),
            error_rate: (samples.attempts > 0).then(|| samples.errors as f64 / samples.attempts as f64),
            attempts: samples.attempts,
        })
        .collect::<Vec<_>>();
    let baseline = rows.first().and_then(|row| row.error_rate);
    let first_deteriorated_bucket = baseline.and_then(|baseline| {
        rows.iter()
            .skip(1)
            .find(|row| {
                row.error_rate
                    .is_some_and(|rate| if baseline > 0.0 { rate >= baseline * 1.3 } else { rate > 0.0 })
            })
            .map(|row| row.bucket.clone())
    });
    Deterioration {
        buckets: rows,
        first_deteriorated_bucket,
    }
}

fn analyze_practice(records: &[SessionRecord], forms: &[HabitForm]) -> PracticeRelation {
    let mut durations = BTreeMap::<chrono::NaiveDate, f64>::new();
    for record in records {
        if let Some(date) = parse_time(&record.header.started_at).map(|time| time.date_naive()) {
            *durations.entry(date).or_default() += record.header.duration_ms.max(0) as f64 / 60_000.0;
        }
    }
    let mut forms_by_day = BTreeMap::<chrono::NaiveDate, Vec<&HabitForm>>::new();
    for form in forms {
        forms_by_day.entry(form.started_at.date_naive()).or_default().push(form);
    }
    for values in forms_by_day.values_mut() {
        values.sort_by_key(|form| form.started_at);
    }
    let mut days = Vec::new();
    for (date, minutes) in durations {
        let values = forms_by_day.get(&date);
        let within_day_gain = values
            .and_then(|values| values.first().zip(values.last()))
            .map(|(first, last)| last.form - first.form);
        let next_day_retention = values
            .and_then(|values| values.last())
            .zip(forms_by_day.get(&date.succ_opt().unwrap_or(date)).and_then(|values| values.first()))
            .and_then(|(last, next)| (last.form != 0.0).then_some(next.form / last.form));
        days.push(DailyPractice {
            date,
            minutes,
            within_day_gain,
            next_day_retention,
        });
    }
    let plateau_minutes = (days.len() >= 14).then(|| segmented_breakpoint(&days)).flatten();
    PracticeRelation { days, plateau_minutes }
}

fn segmented_breakpoint(days: &[DailyPractice]) -> Option<f64> {
    let points = days
        .iter()
        .filter_map(|day| day.within_day_gain.map(|gain| (day.minutes, gain)))
        .collect::<Vec<_>>();
    if points.len() < 6 {
        return None;
    }
    let mut candidates = points.iter().map(|point| point.0).collect::<Vec<_>>();
    candidates.sort_by(f64::total_cmp);
    candidates.dedup_by(|left, right| (*left - *right).abs() < f64::EPSILON);
    candidates
        .into_iter()
        .skip(1)
        .take(points.len().saturating_sub(2))
        .filter_map(|knot| {
            let design = points.iter().map(|(x, y)| ([1.0, *x, (x - knot).max(0.0)], *y)).collect::<Vec<_>>();
            least_squares_3(&design).map(|coefficients| {
                let sse = design
                    .iter()
                    .map(|(x, y)| (y - (coefficients[0] * x[0] + coefficients[1] * x[1] + coefficients[2] * x[2])).powi(2))
                    .sum::<f64>();
                (sse, knot)
            })
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|row| row.1)
}

fn least_squares_3(rows: &[([f64; 3], f64)]) -> Option<[f64; 3]> {
    let mut matrix = [[0.0; 4]; 3];
    for (x, y) in rows {
        for i in 0..3 {
            for j in 0..3 {
                matrix[i][j] += x[i] * x[j];
            }
            matrix[i][3] += x[i] * y;
        }
    }
    for pivot in 0..3 {
        let best = (pivot..3).max_by(|left, right| matrix[*left][pivot].abs().total_cmp(&matrix[*right][pivot].abs()))?;
        matrix.swap(pivot, best);
        if matrix[pivot][pivot].abs() < 1e-12 {
            return None;
        }
        let divisor = matrix[pivot][pivot];
        for value in &mut matrix[pivot][pivot..] {
            *value /= divisor;
        }
        let pivot_row = matrix[pivot];
        for (row, matrix_row) in matrix.iter_mut().enumerate() {
            if row == pivot {
                continue;
            }
            let factor = matrix_row[pivot];
            for (value, pivot_value) in matrix_row[pivot..].iter_mut().zip(&pivot_row[pivot..]) {
                *value -= factor * pivot_value;
            }
        }
    }
    Some([matrix[0][3], matrix[1][3], matrix[2][3]])
}

fn analyze_time_of_day(forms: &[HabitForm]) -> Vec<TimeOfDay> {
    let mut buckets = BTreeMap::<&str, Vec<f64>>::new();
    for form in forms {
        let hour = form.started_at.with_timezone(&chrono::Local).hour();
        let bucket = if (5..11).contains(&hour) {
            "morning"
        } else if (11..17).contains(&hour) {
            "afternoon"
        } else if (17..23).contains(&hour) {
            "evening"
        } else {
            "night"
        };
        buckets.entry(bucket).or_default().push(form.form);
    }
    buckets
        .into_iter()
        .filter_map(|(bucket, values)| {
            Some(TimeOfDay {
                bucket: bucket.to_owned(),
                n: values.len(),
                form_median: median(&values)?,
                exploratory: true,
            })
        })
        .collect()
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
