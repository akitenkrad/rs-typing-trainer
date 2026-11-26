//! All-time growth comparisons derived from the existing daily progress series.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Datelike, Duration, NaiveDate};

use crate::analysis::grades::Level;
use crate::analysis::model::session_language;
use crate::analysis::progress::{DailyPoint, DailyTrend, DailyTrendSource, Progress};
use crate::bench::{BenchPart, is_preferred_bench_series, parse_bench_id, preferred_bench_series};
use crate::i18n::Lang;
use crate::record::SessionRecord;
use crate::record::db::SkillSnapshotRecord;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Change {
    pub speed: f64,
    pub speed_percent: Option<f64>,
    pub accuracy_points: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrendPeriod {
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub speed: Option<f64>,
    pub accuracy: Option<f64>,
}

impl TrendPeriod {
    pub fn first_seven_days(trend: &DailyTrend) -> Option<Self> {
        let start = trend.speed.iter().map(|point| point.date).min()?;
        Some(Self::from_range(trend, start, start + Duration::days(6)))
    }

    pub fn recent_seven_days(trend: &DailyTrend, as_of: NaiveDate) -> Option<Self> {
        let start = as_of - Duration::days(6);
        let period = Self::from_range(trend, start, as_of);
        (period.speed.is_some() || period.accuracy.is_some()).then_some(period)
    }

    fn anchor(trend: &DailyTrend, target: NaiveDate) -> Option<Self> {
        let period = Self::from_range(trend, target - Duration::days(3), target + Duration::days(3));
        (period.speed.is_some() || period.accuracy.is_some()).then_some(period)
    }

    fn from_range(trend: &DailyTrend, start: NaiveDate, end: NaiveDate) -> Self {
        Self {
            start,
            end,
            speed: points_median(&trend.speed, start, end),
            accuracy: points_median(&trend.accuracy, start, end),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LanguageGrowth {
    pub language: String,
    pub source: DailyTrendSource,
    pub recorded_days: usize,
    pub first: Option<TrendPeriod>,
    pub recent: Option<TrendPeriod>,
    pub month_ago: Option<TrendPeriod>,
    pub quarter_ago: Option<TrendPeriod>,
    pub change: Option<Change>,
    pub points: Vec<DailyPoint>,
    pub benches: Vec<BenchGrowth>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BenchPoint {
    pub date: NaiveDate,
    pub wpm: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BenchGrowth {
    pub part: BenchPart,
    pub first: BenchPoint,
    pub latest: BenchPoint,
    pub best: BenchPoint,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PersonalBest {
    pub language: String,
    pub part: BenchPart,
    pub date: NaiveDate,
    pub wpm: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Accumulation {
    pub duration_seconds: u64,
    pub month_duration_seconds: u64,
    pub practice_days: usize,
    pub current_streak: usize,
    pub longest_streak: usize,
    pub keystrokes: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GrowthReport {
    pub languages: Vec<LanguageGrowth>,
    pub accumulation: Accumulation,
    pub personal_bests: Vec<PersonalBest>,
    pub promotions: Vec<SkillPromotion>,
    pub graduations: Vec<GraduationEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillPromotion {
    pub date: NaiveDate,
    pub language: String,
    pub from: Level,
    pub to: Level,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraduationEvent {
    pub date: NaiveDate,
    pub language: Lang,
    pub title: String,
    pub level: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeadlineKind {
    Improved,
    Declined,
    Insufficient,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Headline {
    pub kind: HeadlineKind,
    pub text: String,
}

pub fn build_growth(records: &[SessionRecord], progress: &Progress, as_of: NaiveDate) -> GrowthReport {
    let mut languages = progress
        .trends
        .iter()
        .filter_map(|trend| language_growth(trend, records, as_of))
        .collect::<Vec<_>>();
    let (benches, personal_bests) = benchmark_growth(records, as_of);
    for (language, bench) in benches {
        let target = languages.iter_mut().find(|growth| growth.language == language);
        if let Some(target) = target {
            target.benches.push(bench);
        } else {
            languages.push(LanguageGrowth {
                language,
                source: DailyTrendSource::Bench,
                recorded_days: 0,
                first: None,
                recent: None,
                month_ago: None,
                quarter_ago: None,
                change: None,
                points: Vec::new(),
                benches: vec![bench],
            });
        }
    }
    languages.sort_by(|left, right| left.language.cmp(&right.language));
    GrowthReport {
        languages,
        accumulation: accumulation(records, as_of),
        personal_bests,
        promotions: Vec::new(),
        graduations: Vec::new(),
    }
}

pub fn skill_promotions(rows: &[SkillSnapshotRecord]) -> Vec<SkillPromotion> {
    let mut runs = rows
        .iter()
        .map(|row| (row.date, row.run_id, row.lang.clone()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    runs.sort();
    let mut promotions = runs
        .into_iter()
        .filter_map(|(date, run_id, language)| {
            let summary = crate::practice::menu::skills::run_skill_summary(rows, run_id, date, &language);
            let (previous, current) = (summary.previous_overall?, summary.overall?);
            (current.grade > previous.grade).then_some(SkillPromotion {
                date,
                language,
                from: previous.grade,
                to: current.grade,
            })
        })
        .collect::<Vec<_>>();
    promotions.sort_by(|left, right| right.date.cmp(&left.date).then_with(|| left.language.cmp(&right.language)));
    promotions.truncate(5);
    promotions
}

pub fn menu_graduations(summaries: &[crate::practice::menu::progress::RunStepsSummary]) -> Vec<GraduationEvent> {
    use crate::practice::menu::progress::RunChange;

    let mut events = summaries
        .iter()
        .flat_map(|summary| {
            summary.changes.iter().filter_map(move |change| match change {
                RunChange::LevelUp { title, language, level } => Some(GraduationEvent {
                    date: summary.date,
                    language: *language,
                    title: title.clone(),
                    level: Some(*level),
                }),
                RunChange::Maintenance { title, language } => Some(GraduationEvent {
                    date: summary.date,
                    language: *language,
                    title: title.clone(),
                    level: None,
                }),
                _ => None,
            })
        })
        .collect::<Vec<_>>();
    events.sort_by(|left, right| right.date.cmp(&left.date).then_with(|| left.title.cmp(&right.title)));
    events
}

fn language_growth(trend: &DailyTrend, records: &[SessionRecord], as_of: NaiveDate) -> Option<LanguageGrowth> {
    let points = trend.speed.iter().filter(|point| point.date <= as_of).cloned().collect::<Vec<_>>();
    let recorded_days = points.iter().map(|point| point.date).collect::<BTreeSet<_>>().len();
    if points.is_empty() && trend.accuracy.iter().all(|point| point.date > as_of) {
        return None;
    }
    let filtered = DailyTrend {
        target: trend.target.clone(),
        source: trend.source,
        speed: points.clone(),
        accuracy: trend.accuracy.iter().filter(|point| point.date <= as_of).cloned().collect(),
        speed_slope_per_week: trend.speed_slope_per_week,
        accuracy_slope_per_week: trend.accuracy_slope_per_week,
        retest_correlation: trend.retest_correlation,
    };
    let mut first = TrendPeriod::first_seven_days(&filtered);
    let mut recent = TrendPeriod::recent_seven_days(&filtered, as_of);
    if let Some(period) = &mut first {
        period.accuracy = record_accuracy(records, &trend.target, period.start, period.end);
    }
    if let Some(period) = &mut recent {
        period.accuracy = record_accuracy(records, &trend.target, period.start, period.end);
    }
    let change = (recorded_days >= 7).then(|| period_change(first.as_ref()?, recent.as_ref()?)).flatten();
    Some(LanguageGrowth {
        language: trend.target.clone(),
        source: trend.source,
        recorded_days,
        first,
        recent,
        month_ago: TrendPeriod::anchor(&filtered, as_of - Duration::days(30)),
        quarter_ago: TrendPeriod::anchor(&filtered, as_of - Duration::days(90)),
        change,
        points,
        benches: Vec::new(),
    })
}

fn record_accuracy(records: &[SessionRecord], language: &str, start: NaiveDate, end: NaiveDate) -> Option<f64> {
    median(records.iter().filter_map(|record| {
        if session_language(record) != language || !record.header.accuracy.is_finite() {
            return None;
        }
        let date = DateTime::parse_from_rfc3339(&record.header.started_at).ok()?.date_naive();
        (start..=end).contains(&date).then_some(record.header.accuracy)
    }))
}

fn benchmark_growth(records: &[SessionRecord], as_of: NaiveDate) -> (Vec<(String, BenchGrowth)>, Vec<PersonalBest>) {
    let mut benches = Vec::new();
    let mut personal_bests = Vec::new();
    for part in [BenchPart::A, BenchPart::B, BenchPart::C, BenchPart::D] {
        let preferred = preferred_bench_series(records.iter().filter_map(|record| record.header.bench_id.as_deref()), part);
        let mut rows = records
            .iter()
            .filter_map(|record| {
                let (series, candidate, _) = parse_bench_id(record.header.bench_id.as_deref()?)?;
                if candidate != part || !is_preferred_bench_series(series, part, preferred) {
                    return None;
                }
                let date = DateTime::parse_from_rfc3339(&record.header.started_at).ok()?.date_naive();
                (date <= as_of && record.header.net_wpm.is_finite()).then_some((date, record))
            })
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.header.id.cmp(&right.1.header.id)));
        let Some((first_date, first)) = rows.first().copied() else {
            continue;
        };
        let (latest_date, latest) = rows.last().copied().expect("a first benchmark implies a latest benchmark");
        let (best_date, best) = rows
            .iter()
            .copied()
            .max_by(|left, right| {
                left.1
                    .header
                    .net_wpm
                    .total_cmp(&right.1.header.net_wpm)
                    .then_with(|| right.0.cmp(&left.0))
            })
            .expect("non-empty benchmark rows");
        let language = session_language(first).to_owned();
        benches.push((
            language.clone(),
            BenchGrowth {
                part,
                first: BenchPoint {
                    date: first_date,
                    wpm: first.header.net_wpm,
                },
                latest: BenchPoint {
                    date: latest_date,
                    wpm: latest.header.net_wpm,
                },
                best: BenchPoint {
                    date: best_date,
                    wpm: best.header.net_wpm,
                },
            },
        ));
        let daily_highs = rows.into_iter().fold(BTreeMap::<NaiveDate, f64>::new(), |mut days, (date, record)| {
            days.entry(date)
                .and_modify(|value| *value = value.max(record.header.net_wpm))
                .or_insert(record.header.net_wpm);
            days
        });
        let mut best_so_far = f64::NEG_INFINITY;
        for (date, wpm) in daily_highs {
            if wpm > best_so_far {
                best_so_far = wpm;
                personal_bests.push(PersonalBest {
                    language: language.clone(),
                    part,
                    date,
                    wpm,
                });
            }
        }
    }
    personal_bests.sort_by(|left, right| right.date.cmp(&left.date).then_with(|| right.part.cmp(&left.part)));
    personal_bests.truncate(5);
    (benches, personal_bests)
}

fn period_change(first: &TrendPeriod, recent: &TrendPeriod) -> Option<Change> {
    let (first_speed, recent_speed) = (first.speed?, recent.speed?);
    Some(Change {
        speed: recent_speed - first_speed,
        speed_percent: (first_speed.abs() > f64::EPSILON).then_some((recent_speed - first_speed) / first_speed * 100.0),
        accuracy_points: first.accuracy.zip(recent.accuracy).map(|(first, recent)| (recent - first) * 100.0),
    })
}

fn accumulation(records: &[SessionRecord], as_of: NaiveDate) -> Accumulation {
    let month_start = NaiveDate::from_ymd_opt(as_of.year(), as_of.month(), 1).expect("an analysis date has a valid first day of month");
    let records = records.iter().filter_map(|record| {
        let at = DateTime::parse_from_rfc3339(&record.header.started_at).ok()?;
        (at.date_naive() <= as_of).then_some((record, at.date_naive()))
    });
    let mut duration_seconds = 0_u64;
    let mut month_duration_seconds = 0_u64;
    let mut keystrokes = 0_u64;
    let mut days = BTreeSet::new();
    for (record, date) in records {
        let seconds = u64::try_from(record.header.duration_ms.max(0)).unwrap_or_default() / 1_000;
        duration_seconds = duration_seconds.saturating_add(seconds);
        if date >= month_start {
            month_duration_seconds = month_duration_seconds.saturating_add(seconds);
        }
        keystrokes = keystrokes.saturating_add(record.log.keystrokes.len() as u64);
        days.insert(date);
    }
    let (current_streak, longest_streak) = streaks(&days, as_of);
    Accumulation {
        duration_seconds,
        month_duration_seconds,
        practice_days: days.len(),
        current_streak,
        longest_streak,
        keystrokes,
    }
}

fn streaks(days: &BTreeSet<NaiveDate>, as_of: NaiveDate) -> (usize, usize) {
    let mut longest = 0;
    let mut run = 0;
    let mut previous = None;
    for date in days {
        run = if previous.is_some_and(|previous| *date == previous + Duration::days(1)) {
            run + 1
        } else {
            1
        };
        longest = longest.max(run);
        previous = Some(*date);
    }
    let mut current = 0;
    let mut date = if days.contains(&as_of) || !days.contains(&(as_of - Duration::days(1))) {
        as_of
    } else {
        as_of - Duration::days(1)
    };
    while days.contains(&date) {
        current += 1;
        date -= Duration::days(1);
    }
    (current, longest)
}

pub fn headline(report: &GrowthReport, lang: Lang) -> Headline {
    let duration = format_duration(report.accumulation.duration_seconds, lang);
    let accumulation = match lang {
        Lang::En => format!("Practice {duration}, {} days.", report.accumulation.practice_days),
        Lang::Ja => format!("練習 {duration}，{} 日．", report.accumulation.practice_days),
    };
    let best_daily_improvement = report
        .languages
        .iter()
        .filter_map(|language| Some((language, language.change?)))
        .filter(|(_, change)| rounded_tenth(change.speed) > 0.0)
        .max_by(|left, right| left.1.speed.total_cmp(&right.1.speed));
    if let Some((_language, change)) = best_daily_improvement {
        let percent_ja = change.speed_percent.map_or(String::new(), |value| format!("（{value:+.0}%）"));
        let percent_en = change.speed_percent.map_or(String::new(), |value| format!(" ({value:+.0}%)"));
        let text = match lang {
            Lang::En => format!(
                "You are {:+.1} WPM{percent_en} faster than when you started. {accumulation}",
                change.speed
            ),
            Lang::Ja => format!("始めた頃より {:+.1} WPM{percent_ja}速くなりました．{accumulation}", change.speed),
        };
        Headline {
            kind: HeadlineKind::Improved,
            text,
        }
    } else if let Some((bench, delta, percent)) = best_benchmark_improvement(report) {
        let text = match lang {
            Lang::En => format!(
                "Benchmark {} is {delta:+.1} WPM ({percent:+.0}%) faster than its first result. {accumulation}",
                bench.part.as_str()
            ),
            Lang::Ja => format!(
                "ベンチ {} は初回より {delta:+.1} WPM（{percent:+.0}%）速くなりました．{accumulation}",
                bench.part.as_str()
            ),
        };
        Headline {
            kind: HeadlineKind::Improved,
            text,
        }
    } else if let Some((_language, change)) = report
        .languages
        .iter()
        .filter_map(|language| Some((language, language.change?)))
        .filter(|(_, change)| rounded_tenth(change.speed) < 0.0)
        .max_by(|left, right| left.1.speed.total_cmp(&right.1.speed))
    {
        let accuracy = change
            .accuracy_points
            .filter(|value| rounded_tenth(*value) > 0.0)
            .map(|value| match lang {
                Lang::En => format!(" Accuracy is up {value:+.1} pt."),
                Lang::Ja => format!("正確さは {value:+.1}pt 上がっています．"),
            });
        let Some(accuracy) = accuracy else {
            return Headline {
                kind: HeadlineKind::Declined,
                text: accumulation,
            };
        };
        let text = match lang {
            Lang::En => format!(
                "Compared with when you started, speed is {:+.1} WPM.{} {accumulation}",
                change.speed, accuracy
            ),
            Lang::Ja => format!("始めた頃と比べて {:+.1} WPM です．{accuracy}{accumulation}", change.speed),
        };
        Headline {
            kind: HeadlineKind::Declined,
            text,
        }
    } else if let Some(language) = report
        .languages
        .iter()
        .filter(|language| language.recorded_days < 7)
        .max_by_key(|language| language.recorded_days)
    {
        Headline {
            kind: HeadlineKind::Insufficient,
            text: format!(
                "{accumulation}{}",
                comparison_guidance(language.source, language.recorded_days, lang, false)
            ),
        }
    } else {
        Headline {
            kind: HeadlineKind::Insufficient,
            text: accumulation,
        }
    }
}

fn best_benchmark_improvement(report: &GrowthReport) -> Option<(&BenchGrowth, f64, f64)> {
    report
        .languages
        .iter()
        .flat_map(|language| &language.benches)
        .filter_map(|bench| {
            let delta = bench.latest.wpm - bench.first.wpm;
            if rounded_tenth(delta) <= 0.0 || bench.first.wpm.abs() <= f64::EPSILON {
                return None;
            }
            Some((bench, delta, delta / bench.first.wpm * 100.0))
        })
        .max_by(|left, right| left.2.total_cmp(&right.2))
}

fn rounded_tenth(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

pub fn comparison_guidance(source: DailyTrendSource, recorded_days: usize, lang: Lang, include_count: bool) -> String {
    match (source, lang, include_count) {
        (DailyTrendSource::Bench, Lang::Ja, true) => {
            format!("ベンチの記録が 7 日分たまると，はじめの頃と比べられます（今 {recorded_days} 日分．ベンチはタイトルの [B] で測れます）")
        }
        (DailyTrendSource::Bench, Lang::Ja, false) => {
            format!("ベンチの記録が 7 日分たまると，はじめの頃と比べられます（今 {recorded_days} 日分．ベンチはタイトルの [B] で測れます）．")
        }
        (DailyTrendSource::Bench, Lang::En, true) => format!(
            "Once 7 days of benchmark records are available, you can compare with when you started ({recorded_days} days so far. Run a benchmark with [B] in the title)"
        ),
        (DailyTrendSource::Bench, Lang::En, false) => format!(
            " Once 7 days of benchmark records are available, you can compare with when you started ({recorded_days} days so far. Run a benchmark with [B] in the title)."
        ),
        (DailyTrendSource::CorrectedWpm, Lang::Ja, true) => {
            format!("速さを比べられる記録が 7 日分たまると，はじめの頃と比べられます（今 {recorded_days} 日分）")
        }
        (DailyTrendSource::CorrectedWpm, Lang::Ja, false) => {
            format!("速さを比べられる記録が 7 日分たまると，はじめの頃と比べられます（今 {recorded_days} 日分）．")
        }
        (DailyTrendSource::CorrectedWpm, Lang::En, true) => {
            format!("Once 7 days of comparable speed records are available, you can compare with when you started ({recorded_days} days so far)")
        }
        (DailyTrendSource::CorrectedWpm, Lang::En, false) => {
            format!(" Once 7 days of comparable speed records are available, you can compare with when you started ({recorded_days} days so far).")
        }
    }
}

pub fn format_duration(seconds: u64, lang: Lang) -> String {
    let hours = seconds / 3_600;
    let minutes = seconds % 3_600 / 60;
    match lang {
        Lang::En => format!("{hours} h {minutes} min"),
        Lang::Ja => format!("{hours} 時間 {minutes} 分"),
    }
}

fn points_median(points: &[DailyPoint], start: NaiveDate, end: NaiveDate) -> Option<f64> {
    median(points.iter().filter(|point| (start..=end).contains(&point.date)).map(|point| point.value))
}

fn median(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut values = values.filter(|value| value.is_finite()).collect::<Vec<_>>();
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    match values.len() {
        0 => None,
        length if length.is_multiple_of(2) => Some((values[middle - 1] + values[middle]) / 2.0),
        _ => Some(values[middle]),
    }
}

pub fn language_records<'a>(records: &'a [SessionRecord], language: &'a str) -> impl Iterator<Item = &'a SessionRecord> {
    records.iter().filter(move |record| session_language(record) == language)
}
