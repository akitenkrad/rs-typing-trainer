use std::time::Duration;

use rs_typing_trainer::analysis::analyze_with_pause;
use rs_typing_trainer::analysis::bench_v3::{
    BenchV3Group, bench_v3_run_key, compute_bench_v3_group_metrics, compute_bench_v3_group_reliability, compute_bench_v3_group_runs,
    compute_bench_v3_metrics, compute_bench_v3_part_reliability, previous_effective_wpm_delta,
};
use rs_typing_trainer::bench::BenchV3Part;
use rs_typing_trainer::config::{Config, ErrorMode};
use rs_typing_trainer::engine::session::{ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};
use rs_typing_trainer::record::{SessionHeader, SessionRecord};
use rs_typing_trainer::{analysis::summary::SkillAxis, layout::builtin_layout};

fn key(seq: u32, item_idx: u32, t_ms: u64, correct: bool) -> KeystrokeLog {
    KeystrokeLog {
        seq,
        t_us: t_ms * 1_000,
        release_us: None,
        item_idx,
        pos: seq,
        kind: KeystrokeKind::Char,
        typed: Some(if correct { "a" } else { "x" }.into()),
        expected: Some("a".into()),
        correct,
        code: Some(if correct { "KeyA" } else { "KeyX" }.into()),
    }
}

fn item(idx: u32, shown_ms: u64) -> ItemLog {
    ItemLog {
        idx,
        text: "sample".into(),
        reading: None,
        kind: None,
        shown_us: shown_ms * 1_000,
        done_us: None,
        outcome: ItemOutcome::Completed,
    }
}

fn record(bench_id: &str, items: Vec<ItemLog>, keystrokes: Vec<KeystrokeLog>) -> SessionRecord {
    SessionRecord {
        header: SessionHeader {
            id: None,
            started_at: "2026-11-14T00:00:00Z".into(),
            finished_at: "2026-11-14T00:01:00Z".into(),
            duration_ms: 60_000,
            mode: "bench".into(),
            mode_params: r#"{"lang":"en"}"#.into(),
            bench_id: Some(bench_id.into()),
            layout: "qwerty".into(),
            input_layout: "same".into(),
            geometry: "ansi".into(),
            error_mode: "stop".into(),
            release_events: false,
            app_version: "test".into(),
            metrics_version: 2,
            net_wpm: 40.0,
            raw_wpm: Some(41.0),
            accuracy: 0.123,
            iki_median_ms: 777.0,
            iki_cv: 0.0,
            cps: 0.0,
            consistency: 0.0,
            max_combo: 0,
            completed_words: 0,
            timeout_words: 0,
            mistypes_total: keystrokes.iter().filter(|key| !key.correct).count() as u32,
            legacy_payload: None,
        },
        log: SessionLog {
            duration_us: 60_000_000,
            error_mode: ErrorMode::Stop,
            items,
            keystrokes,
        },
    }
}

#[test]
fn tempo_excludes_item_start_three_correct_keys_after_error_and_long_pauses() {
    let sample = record(
        "bench-v3/EW/01",
        vec![item(0, 0), item(1, 3_900)],
        vec![
            key(0, 0, 100, true),
            key(1, 0, 200, true),
            key(2, 0, 300, false),
            key(3, 0, 400, true),
            key(4, 0, 600, true),
            key(5, 0, 900, true),
            key(6, 0, 1_300, true),
            key(7, 0, 3_600, true),
            key(8, 1, 4_000, true),
            key(9, 1, 4_200, true),
        ],
    );

    let metrics = compute_bench_v3_metrics(&[sample], 2_000);

    assert_eq!(metrics.tempo_interval_count, 3);
    assert_eq!(metrics.tempo_iki_median_ms, Some(200.0));
    assert_eq!(metrics.tempo_wpm, Some(60.0));
    assert!((metrics.accuracy - 0.9).abs() < 1e-12);
    assert_eq!(metrics.effective_wpm, Some(54.0));
    assert_eq!(metrics.reaction_ms, Some(100.0));
}

#[test]
fn tempo_is_none_without_an_eligible_interval() {
    let sample = record(
        "bench-v3/EW/01",
        vec![item(0, 0), item(1, 500)],
        vec![key(0, 0, 100, true), key(1, 1, 600, true)],
    );

    let metrics = compute_bench_v3_metrics(&[sample], 2_000);

    assert_eq!(metrics.tempo_interval_count, 0);
    assert_eq!(metrics.tempo_iki_median_ms, None);
    assert_eq!(metrics.tempo_wpm, None);
    assert_eq!(metrics.effective_wpm, None);
}

#[test]
fn language_summary_pools_intervals_and_keystrokes_instead_of_part_medians() {
    let words = record(
        "bench-v3/EW/01",
        vec![item(0, 0)],
        vec![key(0, 0, 0, true), key(1, 0, 100, true), key(2, 0, 200, true), key(3, 0, 300, true)],
    );
    let sentences = record("bench-v3/ES/01", vec![item(0, 0)], vec![key(0, 0, 0, true), key(1, 0, 300, true)]);

    let summary = compute_bench_v3_group_metrics(&[words, sentences], BenchV3Group::English, 2_000).unwrap();

    assert_eq!(summary.present_parts, vec![BenchV3Part::EW, BenchV3Part::ES]);
    assert!(!summary.partial);
    assert_eq!(summary.metrics.tempo_interval_count, 4);
    assert_eq!(summary.metrics.tempo_iki_median_ms, Some(100.0));
    assert_eq!(summary.metrics.tempo_wpm, Some(120.0));
    assert_eq!(summary.metrics.accuracy, 1.0);
    assert_eq!(summary.metrics.effective_wpm, Some(120.0));
}

#[test]
fn language_summary_uses_the_available_part_and_marks_it_partial() {
    let words = record("bench-v3/EW/01", vec![item(0, 0)], vec![key(0, 0, 0, true), key(1, 0, 200, true)]);

    let summary = compute_bench_v3_group_metrics(&[words], BenchV3Group::English, 2_000).unwrap();

    assert_eq!(summary.present_parts, vec![BenchV3Part::EW]);
    assert!(summary.partial);
    assert_eq!(summary.metrics.tempo_wpm, Some(60.0));
    assert_eq!(summary.metrics.effective_wpm, Some(60.0));
}

#[test]
fn split_half_uses_one_based_odd_and_even_items_and_returns_error_margin() {
    let sample = record(
        "bench-v3/EW/01",
        vec![item(0, 0), item(1, 1_000), item(2, 2_000), item(3, 3_000)],
        vec![
            key(0, 0, 0, true),
            key(1, 0, 100, true),
            key(2, 1, 1_000, true),
            key(3, 1, 1_200, true),
            key(4, 1, 1_300, false),
            key(5, 2, 2_000, true),
            key(6, 2, 2_300, true),
            key(7, 3, 3_000, true),
            key(8, 3, 3_400, true),
        ],
    );

    let metrics = compute_bench_v3_metrics(&[sample], 2_000);

    assert_eq!(metrics.half_a, Some(60.0));
    assert_eq!(metrics.half_b, Some(32.0));
    assert_eq!(metrics.error_margin_wpm, Some(14.0));
}

#[test]
fn split_half_error_margin_is_none_when_either_half_has_no_tempo() {
    let sample = record(
        "bench-v3/EW/01",
        vec![item(0, 0), item(1, 1_000)],
        vec![key(0, 0, 0, true), key(1, 0, 100, true), key(2, 1, 1_000, true)],
    );

    let metrics = compute_bench_v3_metrics(&[sample], 2_000);

    assert_eq!(metrics.half_a, Some(120.0));
    assert_eq!(metrics.half_b, None);
    assert_eq!(metrics.error_margin_wpm, None);
}

#[test]
fn bench_v3_tempo_does_not_change_the_existing_iki_definition() {
    let sample = record(
        "bench-v3/EW/01",
        vec![item(0, 0)],
        vec![key(0, 0, 100, true), key(1, 0, 200, false), key(2, 0, 500, true)],
    );

    let existing = analyze_with_pause(&sample.log, Duration::from_millis(2_000));
    let v3 = compute_bench_v3_metrics(&[sample], 2_000);

    assert_eq!(existing.iki_median, Duration::from_millis(200));
    assert_eq!(v3.tempo_wpm, None);
}

#[test]
fn previous_delta_compares_effective_wpm_for_the_same_v3_part() {
    let mut previous = record("bench-v3/EW/01", vec![item(0, 0)], vec![key(0, 0, 0, true), key(1, 0, 200, true)]);
    previous.header.id = Some(1);
    previous.header.finished_at = "2026-11-13T00:01:00Z".into();
    let mut other_part = previous.clone();
    other_part.header.id = Some(2);
    other_part.header.bench_id = Some("bench-v3/ES/01".into());
    other_part.header.finished_at = "2026-11-14T00:00:00Z".into();
    let mut current = record("bench-v3/EW/02", vec![item(0, 0)], vec![key(0, 0, 0, true), key(1, 0, 100, true)]);
    current.header.id = Some(3);

    assert_eq!(
        previous_effective_wpm_delta(&[previous, other_part, current.clone()], &current, 2_000),
        Some(60.0)
    );
}

/// The local wall-clock time as RFC 3339, so that tests about local days pass in any time zone.
fn local_rfc3339(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> String {
    use chrono::TimeZone;
    chrono::Local
        .with_ymd_and_hms(year, month, day, hour, minute, 0)
        .single()
        .unwrap()
        .to_rfc3339()
}

fn split_record(bench_id: &str, finished_at: &str, half_a_wpm: f64, half_b_wpm: f64) -> SessionRecord {
    let a_ms = (12_000.0 / half_a_wpm).round() as u64;
    let b_ms = (12_000.0 / half_b_wpm).round() as u64;
    let mut sample = record(
        bench_id,
        vec![item(0, 0), item(1, 1_000), item(2, 2_000), item(3, 3_000)],
        vec![
            key(0, 0, 0, true),
            key(1, 0, a_ms, true),
            key(2, 1, 1_000, true),
            key(3, 1, 1_000 + b_ms, true),
            key(4, 2, 2_000, true),
            key(5, 2, 2_000 + a_ms, true),
            key(6, 3, 3_000, true),
            key(7, 3, 3_000 + b_ms, true),
        ],
    );
    sample.header.finished_at = finished_at.into();
    sample
}

#[test]
fn part_reliability_uses_split_half_differences_from_matching_runs_only() {
    let first = split_record("bench-v3/EW/01", "2026-11-13T00:01:00Z", 60.0, 40.0);
    let second = split_record("bench-v3/EW/02", "2026-11-14T00:01:00Z", 60.0, 120.0);
    let other = split_record("bench-v3/ES/02", "2026-11-14T00:02:00Z", 120.0, 40.0);

    let reliability = compute_bench_v3_part_reliability(&[first, second, other], BenchV3Part::EW, 2_000).unwrap();

    assert_eq!(reliability.sample_count, 2);
    assert!((reliability.sem_wpm - 2.0_f64.sqrt() * 20.0).abs() < 1e-12, "{reliability:?}");
    assert!(reliability.reference_only);
}

#[test]
fn language_reliability_uses_pooled_complete_runs() {
    let records = [
        split_record("bench-v3/EW/01", "2026-11-13T00:01:00Z", 60.0, 40.0),
        split_record("bench-v3/ES/01", "2026-11-13T00:02:00Z", 60.0, 40.0),
        split_record("bench-v3/EW/02", "2026-11-14T00:01:00Z", 60.0, 120.0),
        split_record("bench-v3/ES/02", "2026-11-14T00:02:00Z", 60.0, 120.0),
    ];

    let reliability = compute_bench_v3_group_reliability(&records, BenchV3Group::English, 2_000).unwrap();

    assert_eq!(reliability.sample_count, 2);
    assert!((reliability.sem_wpm - 2.0_f64.sqrt() * 20.0).abs() < 1e-12, "{reliability:?}");
}

#[test]
fn group_runs_use_local_day_and_form_as_identity_but_keep_actual_trial_order() {
    let records = [
        split_record("bench-v3/EW/08", &local_rfc3339(2026, 11, 14, 8, 1), 40.0, 40.0),
        split_record("bench-v3/ES/08", &local_rfc3339(2026, 11, 14, 8, 2), 40.0, 40.0),
        split_record("bench-v3/EW/01", &local_rfc3339(2026, 11, 14, 20, 1), 120.0, 120.0),
        split_record("bench-v3/ES/01", &local_rfc3339(2026, 11, 14, 20, 2), 120.0, 120.0),
    ];

    assert_eq!(
        bench_v3_run_key(&records[0]),
        Some((chrono::NaiveDate::from_ymd_opt(2026, 11, 14).unwrap(), 8))
    );

    let runs = compute_bench_v3_group_runs(&records, BenchV3Group::English, 2_000);

    assert_eq!(runs.iter().map(|run| run.form).collect::<Vec<_>>(), [8, 1]);
    assert_eq!(
        runs.iter().map(|run| run.metrics.effective_wpm).collect::<Vec<_>>(),
        [Some(40.0), Some(120.0)]
    );
    assert!(runs.iter().all(|run| run.date == chrono::NaiveDate::from_ymd_opt(2026, 11, 14).unwrap()));
}

#[test]
fn eight_axes_prefer_v3_runs_and_only_add_percentiles_at_three_runs() {
    let mut records = Vec::new();
    for (day, form, english, japanese) in [(12, 8, 40.0, 30.0), (13, 1, 80.0, 60.0), (14, 2, 120.0, 90.0)] {
        for part in ["EW", "ES"] {
            records.push(split_record(
                &format!("bench-v3/{part}/{form:02}"),
                &format!("2026-11-{day:02}T08:00:00+09:00"),
                english,
                english,
            ));
        }
        for part in ["JW", "JS"] {
            records.push(split_record(
                &format!("bench-v3/{part}/{form:02}"),
                &format!("2026-11-{day:02}T08:10:00+09:00"),
                japanese,
                japanese,
            ));
        }
        records.push(split_record(
            &format!("bench-v3/PW/{form:02}"),
            &format!("2026-11-{day:02}T08:20:00+09:00"),
            english,
            english,
        ));
    }
    let mut legacy = split_record("bench-v2/A/01", "2026-11-15T08:00:00+09:00", 999.0, 999.0);
    legacy.header.net_wpm = 999.0;
    records.push(legacy);
    records.reverse();
    let mut config = Config::default();
    config.analysis.min_transition_n = 1;
    let layout = builtin_layout("qwerty").unwrap();

    let axes = rs_typing_trainer::analysis::summary::skill_axis_values(&records, &layout, &config);
    let axis = |target| axes.iter().find(|row| row.axis == target).unwrap();

    assert_eq!(axis(SkillAxis::SustainedSpeed).raw, Some(120.0));
    assert_eq!(axis(SkillAxis::SustainedSpeed).percentile, Some(100.0));
    assert_eq!(axis(SkillAxis::Accuracy).raw, Some(100.0));
    assert!((axis(SkillAxis::Japanese).raw.unwrap() - 90.0).abs() < 0.3);
    assert_eq!(axis(SkillAxis::Japanese).percentile, Some(100.0));
    assert!(axis(SkillAxis::Transitions).raw.is_some(), "PW must feed transitions: {axes:?}");

    let two_runs = rs_typing_trainer::analysis::summary::skill_axis_values(&records[5..], &layout, &config);
    assert!(
        two_runs.iter().filter(|row| row.raw.is_some()).all(|row| row.percentile.is_none()),
        "fewer than three v3 runs must show raw values only: {two_runs:?}"
    );

    let report = rs_typing_trainer::analysis::summary::generate(
        &records,
        &config,
        chrono::NaiveDate::from_ymd_opt(2026, 11, 15).unwrap(),
        rs_typing_trainer::i18n::Lang::En,
    )
    .to_text();
    assert!(report.contains("Strongest:"), "v3 percentiles must feed the summary: {report}");
    assert!(
        report.contains("No practice records in the last 7 days"),
        "v3 benchmark parts must not be reported as practice modes: {report}"
    );
}
