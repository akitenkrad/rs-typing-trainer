use chrono::{Duration, NaiveDate, TimeZone, Utc};
use rs_typing_trainer::analysis::bench_v3::{BenchV3Change, BenchV3Group};
use rs_typing_trainer::analysis::growth::{
    Accumulation, BenchGrowth, BenchPoint, BenchV3GrowthTarget, Change, GrowthReport, HeadlineKind, LanguageGrowth, TrendPeriod, build_growth,
    headline,
};
use rs_typing_trainer::analysis::progress::{BenchV3Trend, BenchV3TrendPoint, DailyPoint, DailyTrend, DailyTrendSource, Progress};
use rs_typing_trainer::bench::BenchV3Part;
use rs_typing_trainer::config::Config;
use rs_typing_trainer::engine::session::SessionLog;
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::record::SessionRecord;
use rs_typing_trainer::record::db::SkillSnapshotRecord;

fn day(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, day).unwrap()
}

fn point(day_number: u32, value: f64) -> DailyPoint {
    DailyPoint {
        date: day(day_number),
        value,
    }
}

fn trend(source: DailyTrendSource, speed: &[(u32, f64)], accuracy: &[(u32, f64)]) -> DailyTrend {
    DailyTrend {
        target: "en".into(),
        source,
        speed: speed.iter().map(|&(date, value)| point(date, value)).collect(),
        accuracy: accuracy.iter().map(|&(date, value)| point(date, value)).collect(),
        speed_slope_per_week: None,
        accuracy_slope_per_week: None,
        retest_correlation: None,
    }
}

fn record(day_number: u32, duration_minutes: i64, keystrokes: usize, accuracy: f64) -> SessionRecord {
    let started = Utc.with_ymd_and_hms(2026, 11, day_number, 12, 0, 0).unwrap();
    let mut record = SessionRecord::from_log(
        &Config::default(),
        SessionLog {
            duration_us: (duration_minutes * 60 * 1_000_000) as u64,
            keystrokes: (0..keystrokes)
                .map(|seq| rs_typing_trainer::engine::session::KeystrokeLog {
                    seq: seq as u32,
                    t_us: seq as u64,
                    release_us: None,
                    item_idx: 0,
                    pos: seq as u32,
                    kind: rs_typing_trainer::engine::session::KeystrokeKind::Char,
                    typed: Some("a".into()),
                    expected: Some("a".into()),
                    correct: true,
                    code: None,
                })
                .collect(),
            ..SessionLog::default()
        },
        started,
    );
    record.header.accuracy = accuracy;
    record.header.duration_ms = duration_minutes * 60 * 1_000;
    record
}

fn bench_record(day_number: u32, bench_id: &str, wpm: f64) -> SessionRecord {
    let mut record = record(day_number, 1, 10, 0.98);
    record.header.mode = "bench".into();
    record.header.bench_id = Some(bench_id.into());
    record.header.net_wpm = wpm;
    record
}

fn v3_record(id: i64, day_number: u32, part: BenchV3Part, form: u8, wpm: f64) -> SessionRecord {
    let started = Utc.with_ymd_and_hms(2026, 11, day_number, 12, 0, 0).unwrap();
    let interval_us = (12_000_000.0 / wpm) as u64;
    let mut keystrokes = Vec::new();
    for item_idx in 0..2_u32 {
        let base = u64::from(item_idx) * 1_000_000;
        for pos in 0..4_u32 {
            keystrokes.push(rs_typing_trainer::engine::session::KeystrokeLog {
                seq: item_idx * 4 + pos,
                t_us: base + 100_000 + u64::from(pos) * interval_us,
                release_us: None,
                item_idx,
                pos,
                kind: rs_typing_trainer::engine::session::KeystrokeKind::Char,
                typed: Some("a".into()),
                expected: Some("a".into()),
                correct: true,
                code: None,
            });
        }
    }
    let mut record = SessionRecord::from_log(
        &Config::default(),
        SessionLog {
            duration_us: 2_000_000,
            items: (0..2)
                .map(|idx| rs_typing_trainer::engine::session::ItemLog {
                    idx,
                    text: "aaaa".into(),
                    reading: None,
                    kind: None,
                    shown_us: u64::from(idx) * 1_000_000,
                    done_us: None,
                    outcome: rs_typing_trainer::engine::session::ItemOutcome::Completed,
                })
                .collect(),
            keystrokes,
            ..SessionLog::default()
        },
        started,
    );
    record.header.id = Some(id);
    record.header.mode = "bench".into();
    record.header.bench_id = Some(format!("bench-v3/{}/{form:02}", part.as_str()));
    record
}

#[test]
fn seven_day_windows_use_calendar_boundaries_and_medians() {
    let trend = trend(
        DailyTrendSource::Bench,
        &[(1, 10.0), (2, 20.0), (7, 70.0), (8, 800.0), (20, 30.0), (25, 50.0), (26, 40.0)],
        &[(1, 0.90), (7, 0.98), (20, 0.96), (26, 1.0)],
    );
    let first = TrendPeriod::first_seven_days(&trend).unwrap();
    assert_eq!((first.start, first.end), (day(1), day(7)));
    assert_eq!(first.speed, Some(20.0));
    assert_eq!(first.accuracy, Some(0.94));

    let recent = TrendPeriod::recent_seven_days(&trend, day(26)).unwrap();
    assert_eq!((recent.start, recent.end), (day(20), day(26)));
    assert_eq!(recent.speed, Some(40.0));
    assert_eq!(recent.accuracy, Some(0.98));
}

#[test]
fn first_window_starts_on_the_first_speed_day_not_an_earlier_accuracy_day() {
    let trend = trend(DailyTrendSource::Bench, &[(8, 40.0), (14, 50.0)], &[(1, 0.90), (8, 0.95)]);
    let first = TrendPeriod::first_seven_days(&trend).unwrap();
    assert_eq!((first.start, first.end), (day(8), day(14)));
    assert_eq!(first.speed, Some(45.0));
}

#[test]
fn comparison_accuracy_comes_from_session_records_not_the_speed_series() {
    let speed = (1..=7).map(|date| (date, 40.0)).collect::<Vec<_>>();
    let daily_trend = trend(DailyTrendSource::Bench, &speed, &[(1, 0.50), (7, 0.60)]);
    let report = build_growth(
        &[record(1, 1, 1, 0.96), record(7, 1, 1, 0.98)],
        &Progress {
            trends: vec![daily_trend],
            ..Progress::default()
        },
        day(26),
    );
    assert_eq!(report.languages[0].first.as_ref().and_then(|period| period.accuracy), Some(0.97));

    let without_sessions = build_growth(
        &[],
        &Progress {
            trends: vec![trend(DailyTrendSource::Bench, &speed, &[(1, 0.50), (7, 0.60)])],
            ..Progress::default()
        },
        day(26),
    );
    assert_eq!(without_sessions.languages[0].first.as_ref().and_then(|period| period.accuracy), None);
}

#[test]
fn fewer_than_seven_recorded_days_is_not_comparable() {
    for count in [0_usize, 6] {
        let speed = (0..count).map(|offset| (1 + offset as u32, 30.0 + offset as f64)).collect::<Vec<_>>();
        let report = build_growth(
            &[],
            &Progress {
                trends: if speed.is_empty() {
                    vec![]
                } else {
                    vec![trend(DailyTrendSource::CorrectedWpm, &speed, &[])]
                },
                ..Progress::default()
            },
            day(26),
        );
        assert_eq!(report.languages.first().map(|row| row.recorded_days).unwrap_or(0), count);
        assert!(report.languages.first().is_none_or(|row| row.change.is_none()));
    }
}

#[test]
fn anchor_windows_are_centered_on_30_and_90_days_and_exclude_future_points() {
    let as_of = NaiveDate::from_ymd_opt(2026, 11, 26).unwrap();
    let target_30 = as_of - Duration::days(30);
    let target_90 = as_of - Duration::days(90);
    let trend = DailyTrend {
        target: "en".into(),
        source: DailyTrendSource::Bench,
        speed: vec![
            DailyPoint {
                date: target_90 - Duration::days(3),
                value: 10.0,
            },
            DailyPoint {
                date: target_90 + Duration::days(3),
                value: 20.0,
            },
            DailyPoint {
                date: target_30 - Duration::days(3),
                value: 30.0,
            },
            DailyPoint {
                date: target_30,
                value: 50.0,
            },
            DailyPoint {
                date: target_30 + Duration::days(3),
                value: 70.0,
            },
            DailyPoint {
                date: as_of + Duration::days(1),
                value: 999.0,
            },
        ],
        accuracy: vec![],
        speed_slope_per_week: None,
        accuracy_slope_per_week: None,
        retest_correlation: None,
    };
    let report = build_growth(
        &[],
        &Progress {
            trends: vec![trend],
            ..Progress::default()
        },
        as_of,
    );
    let language = &report.languages[0];
    assert_eq!(language.month_ago.as_ref().and_then(|row| row.speed), Some(50.0));
    assert_eq!(language.quarter_ago.as_ref().and_then(|row| row.speed), Some(15.0));
    assert!(language.points.iter().all(|point| point.date <= as_of));
}

#[test]
fn accumulation_counts_duration_days_current_and_longest_streak_and_keystrokes() {
    let records = vec![
        record(1, 30, 10, 0.9),
        record(2, 45, 20, 0.9),
        record(4, 15, 30, 0.9),
        record(26, 10, 40, 0.9),
    ];
    let report = build_growth(&records, &Progress::default(), day(26));
    assert_eq!(report.accumulation.duration_seconds, 6_000);
    assert_eq!(report.accumulation.practice_days, 4);
    assert_eq!(report.accumulation.current_streak, 1);
    assert_eq!(report.accumulation.longest_streak, 2);
    assert_eq!(report.accumulation.keystrokes, 100);
    assert_eq!(report.accumulation.month_duration_seconds, 6_000);
}

#[test]
fn this_month_duration_uses_the_calendar_month_not_a_rolling_thirty_days() {
    let records = vec![record(1, 20, 1, 1.0), record(26, 10, 1, 1.0), {
        let mut october = record(26, 40, 1, 1.0);
        october.header.started_at = "2026-10-31T12:00:00+00:00".into();
        october
    }];
    let report = build_growth(&records, &Progress::default(), day(26));
    assert_eq!(report.accumulation.duration_seconds, 4_200);
    assert_eq!(report.accumulation.month_duration_seconds, 1_800);
}

#[test]
fn current_streak_remains_live_when_the_last_practice_was_yesterday() {
    let records = vec![record(24, 1, 1, 1.0), record(25, 1, 1, 1.0)];
    let report = build_growth(&records, &Progress::default(), day(26));
    assert_eq!(report.accumulation.current_streak, 2);
}

#[test]
fn headline_covers_improvement_decline_and_insufficient_data() {
    let make = |first: f64, latest: f64, first_accuracy: f64, latest_accuracy: f64| {
        let speed = (1..=7)
            .map(|date| (date, first))
            .chain((20..=26).map(|date| (date, latest)))
            .collect::<Vec<_>>();
        let records = (1..=7)
            .map(|date| record(date, 1, 1, first_accuracy))
            .chain((20..=26).map(|date| record(date, 1, 1, latest_accuracy)))
            .collect::<Vec<_>>();
        build_growth(
            &records,
            &Progress {
                trends: vec![trend(DailyTrendSource::Bench, &speed, &[])],
                ..Progress::default()
            },
            day(26),
        )
    };
    assert_eq!(headline(&make(32.1, 41.5, 0.942, 0.970), Lang::Ja).kind, HeadlineKind::Improved);
    let declined = headline(&make(41.5, 40.3, 0.94, 0.971), Lang::Ja);
    assert_eq!(declined.kind, HeadlineKind::Declined);
    assert!(declined.text.contains("正確さは +3.1pt"), "{}", declined.text);
    assert_eq!(headline(&GrowthReport::default(), Lang::En).kind, HeadlineKind::Insufficient);
}

#[test]
fn insufficient_headline_names_the_daily_series_in_both_languages() {
    let report = |source| GrowthReport {
        languages: vec![LanguageGrowth {
            language: "en".into(),
            source,
            recorded_days: 3,
            first: None,
            recent: None,
            month_ago: None,
            quarter_ago: None,
            change: None,
            points: vec![],
            benches: vec![],
        }],
        accumulation: Accumulation {
            practice_days: 13,
            ..Default::default()
        },
        ..Default::default()
    };

    let bench_ja = headline(&report(DailyTrendSource::Bench), Lang::Ja).text;
    assert!(bench_ja.contains("ベンチの記録が 7 日分"), "{bench_ja}");
    assert!(bench_ja.contains("今 3 日分．ベンチはタイトルの [B] で測れます"), "{bench_ja}");
    let bench_en = headline(&report(DailyTrendSource::Bench), Lang::En).text;
    assert!(bench_en.contains("7 days of benchmark records"), "{bench_en}");
    assert!(bench_en.contains("[B] in the title"), "{bench_en}");

    let v3_ja = headline(&report(DailyTrendSource::BenchV3), Lang::Ja).text;
    assert!(v3_ja.contains("ベンチ v3"), "{v3_ja}");
    assert!(v3_ja.contains("正確さを含む速さ"), "{v3_ja}");
    let v3_en = headline(&report(DailyTrendSource::BenchV3), Lang::En).text;
    assert!(v3_en.contains("benchmark v3"), "{v3_en}");
    assert!(v3_en.contains("accuracy-adjusted speed"), "{v3_en}");

    let adjusted_ja = headline(&report(DailyTrendSource::CorrectedWpm), Lang::Ja).text;
    assert!(adjusted_ja.contains("速さを比べられる記録が 7 日分"), "{adjusted_ja}");
    let adjusted_en = headline(&report(DailyTrendSource::CorrectedWpm), Lang::En).text;
    assert!(adjusted_en.contains("7 days of comparable speed records"), "{adjusted_en}");
}

#[test]
fn headline_prefers_daily_improvement_then_best_benchmark_rate_then_accumulation() {
    let bench = |part, first, latest| BenchGrowth {
        part,
        first: BenchPoint { date: day(1), wpm: first },
        latest: BenchPoint { date: day(20), wpm: latest },
        best: BenchPoint { date: day(20), wpm: latest },
    };
    let language = |change: Option<Change>, benches: Vec<BenchGrowth>| LanguageGrowth {
        language: "en".into(),
        source: DailyTrendSource::Bench,
        recorded_days: if change.is_some() { 14 } else { 3 },
        first: None,
        recent: None,
        month_ago: None,
        quarter_ago: None,
        change,
        points: vec![],
        benches,
    };
    let accumulation = Accumulation {
        duration_seconds: 6_300,
        practice_days: 13,
        ..Default::default()
    };

    let daily = GrowthReport {
        languages: vec![language(
            Some(Change {
                speed: 1.0,
                speed_percent: Some(2.0),
                accuracy_points: None,
            }),
            vec![bench(rs_typing_trainer::bench::BenchPart::B, 20.0, 30.0)],
        )],
        accumulation,
        ..Default::default()
    };
    assert!(headline(&daily, Lang::Ja).text.starts_with("始めた頃より +1.0 WPM"));

    let benchmark = GrowthReport {
        languages: vec![language(
            None,
            vec![
                bench(rs_typing_trainer::bench::BenchPart::A, 30.0, 33.0),
                bench(rs_typing_trainer::bench::BenchPart::B, 20.0, 24.0),
            ],
        )],
        accumulation,
        ..Default::default()
    };
    let bench_text = headline(&benchmark, Lang::Ja).text;
    assert!(bench_text.starts_with("ベンチ B は初回より +4.0 WPM（+20%）"), "{bench_text}");

    let no_improvement = GrowthReport {
        languages: vec![language(None, vec![bench(rs_typing_trainer::bench::BenchPart::A, 30.0, 30.04)])],
        accumulation,
        ..Default::default()
    };
    let accumulation_only = headline(&no_improvement, Lang::Ja).text;
    assert!(accumulation_only.starts_with("練習 1 時間 45 分，13 日．"), "{accumulation_only}");
    assert!(!accumulation_only.contains("ベンチ A は"), "{accumulation_only}");
}

#[test]
fn declining_speed_without_an_improved_metric_shows_only_accumulation() {
    let report = GrowthReport {
        languages: vec![rs_typing_trainer::analysis::growth::LanguageGrowth {
            language: "en".into(),
            source: DailyTrendSource::Bench,
            recorded_days: 14,
            first: None,
            recent: None,
            month_ago: None,
            quarter_ago: None,
            change: Some(rs_typing_trainer::analysis::growth::Change {
                speed: -1.2,
                speed_percent: Some(-3.0),
                accuracy_points: Some(0.04),
            }),
            points: vec![],
            benches: vec![],
        }],
        accumulation: rs_typing_trainer::analysis::growth::Accumulation {
            duration_seconds: 3_600,
            practice_days: 5,
            ..Default::default()
        },
        ..Default::default()
    };
    let text = headline(&report, Lang::Ja).text;
    assert_eq!(text, "練習 1 時間 0 分，5 日．");
}

#[test]
fn benchmark_milestones_reuse_preferred_series_and_report_first_latest_best() {
    let records = vec![
        bench_record(1, "bench-v1/A/01", 90.0),
        bench_record(2, "bench-v2/A/01", 30.0),
        bench_record(3, "bench-v2/A/02", 40.0),
        bench_record(4, "bench-v2/A/03", 35.0),
        bench_record(5, "bench-v1/B/01", 20.0),
        bench_record(6, "bench-v2/B/01", 25.0),
    ];
    let report = build_growth(&records, &Progress::default(), day(26));
    let english = report.languages.iter().find(|language| language.language == "en").unwrap();
    let a = english.benches.iter().find(|bench| bench.part.as_str() == "A").unwrap();
    assert_eq!((a.first.wpm, a.latest.wpm, a.best.wpm), (30.0, 35.0, 40.0));
    assert_eq!(a.best.date, day(3));
    let b = english.benches.iter().find(|bench| bench.part.as_str() == "B").unwrap();
    assert_eq!((b.first.wpm, b.latest.wpm, b.best.wpm), (20.0, 25.0, 25.0));
    assert_eq!(report.personal_bests.len(), 4);
    assert_eq!(report.personal_bests[0].date, day(6));
}

#[test]
fn recent_personal_bests_keep_only_daily_highs_that_strictly_set_a_new_record() {
    let records = vec![
        bench_record(1, "bench-v2/B/01", 20.0),
        bench_record(2, "bench-v2/B/02", 19.0),
        bench_record(3, "bench-v2/B/03", 21.0),
        bench_record(3, "bench-v2/B/04", 25.0),
        bench_record(4, "bench-v2/B/05", 24.0),
    ];
    let report = build_growth(&records, &Progress::default(), day(26));
    let events = report
        .personal_bests
        .iter()
        .map(|event| (event.date, event.part.as_str(), event.wpm))
        .collect::<Vec<_>>();
    assert_eq!(events, vec![(day(3), "B", 25.0), (day(1), "B", 20.0)]);
}

#[test]
fn stored_skill_summaries_and_menu_changes_supply_recent_growth_events() {
    let snapshot = |run_id, date, component: &str, raw_value| SkillSnapshotRecord {
        run_id,
        date,
        lang: "en".into(),
        component: component.into(),
        raw_value,
        n: 100,
        source: "check".into(),
        corrected: false,
    };
    let rows = vec![
        snapshot(1, day(1), "speed", 26.0),
        snapshot(1, day(1), "accuracy", 91.0),
        snapshot(2, day(2), "speed", 37.0),
        snapshot(2, day(2), "accuracy", 94.0),
    ];
    let promotions = rs_typing_trainer::analysis::growth::skill_promotions(&rows);
    assert_eq!(promotions.len(), 1);
    assert!(promotions[0].to > promotions[0].from);

    let summaries = vec![rs_typing_trainer::practice::menu::progress::RunStepsSummary {
        run_id: 1,
        date: day(3),
        minutes: 10,
        bench_recommended: false,
        steps: vec![],
        changes: vec![rs_typing_trainer::practice::menu::progress::RunChange::LevelUp {
            title: "Accuracy gate".into(),
            language: Lang::En,
            level: 2,
        }],
    }];
    let graduations = rs_typing_trainer::analysis::growth::menu_graduations(&summaries);
    assert_eq!(graduations.len(), 1);
    assert_eq!(graduations[0].level, Some(2));
}

#[test]
fn v3_growth_uses_rolling_mainline_and_keeps_group_parts_reliability_and_previous_v2() {
    let values = [(1, 10.0), (8, 20.0), (15, 100.0), (22, 40.0)];
    let mut records = vec![bench_record(1, "bench-v2/A/01", 33.0)];
    for (index, (date, wpm)) in values.into_iter().enumerate() {
        let form = index as u8 + 1;
        records.push(v3_record(100 + index as i64 * 2, date, BenchV3Part::EW, form, wpm));
        records.push(v3_record(101 + index as i64 * 2, date, BenchV3Part::ES, form, wpm));
    }
    let raw = values
        .into_iter()
        .map(|(date, value)| BenchV3TrendPoint {
            date: day(date),
            value,
            accuracy: 1.0,
        })
        .collect::<Vec<_>>();
    let mainline = [(1, 10.0), (8, 15.0), (15, 20.0), (22, 40.0)]
        .into_iter()
        .map(|(date, value)| point(date, value))
        .collect::<Vec<_>>();
    let progress = Progress {
        trends: vec![DailyTrend {
            target: "en".into(),
            source: DailyTrendSource::BenchV3,
            speed: values.into_iter().map(|(date, value)| point(date, value)).collect(),
            accuracy: values.into_iter().map(|(date, _)| point(date, 1.0)).collect(),
            speed_slope_per_week: Some(7.0),
            accuracy_slope_per_week: Some(0.0),
            retest_correlation: None,
        }],
        bench_v3_trends: vec![BenchV3Trend {
            target: "en".into(),
            runs: raw,
            mainline: mainline.clone(),
        }],
        ..Progress::default()
    };

    let report = build_growth(&records, &progress, day(22));
    let english = report.languages.iter().find(|language| language.language == "en").unwrap();
    assert_eq!(english.source, DailyTrendSource::BenchV3);
    assert_eq!(english.points, mainline);
    assert_eq!(english.first.as_ref().and_then(|period| period.speed), Some(10.0));
    assert_eq!(english.recent.as_ref().and_then(|period| period.speed), Some(40.0));
    assert_eq!(english.benches.len(), 1, "v2 remains available as the previous series");
    assert_eq!(english.benches[0].part.as_str(), "A");

    let group = report
        .bench_v3
        .iter()
        .find(|growth| growth.target == BenchV3GrowthTarget::Group(BenchV3Group::English))
        .unwrap();
    assert!((group.first.wpm - 10.0).abs() < 0.01, "{group:?}");
    assert!((group.latest.wpm - 40.0).abs() < 0.01, "{group:?}");
    assert!((group.best.wpm - 100.0).abs() < 0.01, "{group:?}");
    assert_eq!(
        group.mainline.iter().map(|point| point.wpm.round()).collect::<Vec<_>>(),
        [10.0, 15.0, 20.0, 40.0]
    );
    assert_eq!(group.change, BenchV3Change::Increase);
    assert!(group.reliability.is_some_and(|value| value.sample_count == 4 && value.reference_only));

    let ew = report
        .bench_v3
        .iter()
        .find(|growth| growth.target == BenchV3GrowthTarget::Part(BenchV3Part::EW))
        .unwrap();
    assert!((ew.latest.wpm - 40.0).abs() < 0.01, "{ew:?}");
    assert!(ew.reliability.is_some());
}
