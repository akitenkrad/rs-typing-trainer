use std::time::Duration;

use rs_typing_trainer::analysis::analyze_with_pause;
use rs_typing_trainer::analysis::bench_v3::{BenchV3Group, compute_bench_v3_group_metrics, compute_bench_v3_metrics, previous_effective_wpm_delta};
use rs_typing_trainer::bench::BenchV3Part;
use rs_typing_trainer::config::ErrorMode;
use rs_typing_trainer::engine::session::{ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};
use rs_typing_trainer::record::{SessionHeader, SessionRecord};

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
