use chrono::{Duration, TimeZone, Utc};
use rs_typing_trainer::analysis::bench_summary::{BenchMetric, PracticeSuggestion, WeakFingerReason, analyze_benchmark_run};
use rs_typing_trainer::analysis::metrics::AnalysisSettings;
use rs_typing_trainer::analysis::model::Prescription;
use rs_typing_trainer::config::ErrorMode;
use rs_typing_trainer::engine::session::{ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};
use rs_typing_trainer::layout::builtin_layout;
use rs_typing_trainer::layout::geometry::geometry;
use rs_typing_trainer::record::{SessionHeader, SessionRecord};

fn record(at: chrono::DateTime<Utc>, bench_id: &str, wpm: f64, accuracy: f64, keys: Vec<KeystrokeLog>) -> SessionRecord {
    SessionRecord {
        header: SessionHeader {
            id: None,
            started_at: at.to_rfc3339(),
            finished_at: (at + Duration::minutes(1)).to_rfc3339(),
            duration_ms: 60_000,
            mode: "bench".into(),
            mode_params: "{}".into(),
            bench_id: Some(bench_id.into()),
            layout: "qwerty".into(),
            input_layout: "qwerty".into(),
            geometry: "ansi".into(),
            error_mode: "stop".into(),
            release_events: false,
            app_version: "test".into(),
            metrics_version: 2,
            net_wpm: wpm,
            raw_wpm: Some(wpm),
            accuracy,
            iki_median_ms: 150.0,
            iki_cv: 0.3,
            cps: wpm / 12.0,
            consistency: 0.7,
            max_combo: 0,
            completed_words: 1,
            timeout_words: 0,
            mistypes_total: keys.iter().filter(|key| !key.correct).count() as u32,
            legacy_payload: None,
        },
        log: SessionLog {
            duration_us: 60_000_000,
            error_mode: ErrorMode::Stop,
            items: vec![ItemLog {
                idx: 0,
                text: "sample".into(),
                reading: None,
                kind: None,
                shown_us: 0,
                done_us: Some(2_000_000),
                outcome: ItemOutcome::Completed,
            }],
            keystrokes: keys,
        },
    }
}

fn key(seq: u32, t_us: u64, expected: &str, correct: bool) -> KeystrokeLog {
    KeystrokeLog {
        seq,
        t_us,
        release_us: None,
        item_idx: 0,
        pos: seq,
        kind: KeystrokeKind::Char,
        typed: Some(if correct { expected } else { "z" }.into()),
        expected: Some(expected.into()),
        correct,
        code: None,
    }
}

#[test]
fn current_run_analysis_is_deterministic_and_excludes_sparse_transitions() {
    let start = Utc.with_ymd_and_hms(2026, 11, 14, 8, 0, 0).unwrap();
    let prior_b = record(start, "bench-v1/B/01", 50.0, 0.90, Vec::new());
    let prior_a = record(start, "bench-v1/A/01", 70.0, 0.97, Vec::new());
    let keys = vec![
        key(0, 100_000, "t", true),
        key(1, 420_000, "h", true),
        key(2, 520_000, "t", true),
        key(3, 840_000, "h", true),
        key(4, 940_000, "t", true),
        key(5, 1_260_000, "h", true),
        key(6, 1_360_000, "x", true),
        key(7, 1_760_000, "y", true),
        key(8, 1_860_000, "f", false),
        key(9, 1_960_000, "f", false),
    ];
    let current_b = record(start + Duration::days(1), "bench-v2/B/01", 55.0, 0.94, keys);
    let current_a = record(start + Duration::days(1), "bench-v2/A/01", 65.0, 0.96, Vec::new());
    let current = vec![current_a.clone(), current_b.clone()];
    let history = vec![prior_b, prior_a, current_a, current_b];
    let prescriptions = vec![Prescription {
        id: Some(1),
        created_at: start,
        layout: "qwerty".into(),
        kind: "transition-practice".into(),
        target: serde_json::json!({"from":"t", "to":"h"}),
        control: serde_json::json!({}),
        hypothesis: serde_json::json!({}),
        baseline: serde_json::json!({}),
        status: "active".into(),
        evaluated_at: None,
        result: None,
    }];

    let analysis = analyze_benchmark_run(
        &current,
        &history,
        &builtin_layout("qwerty").unwrap(),
        &geometry("ansi").unwrap(),
        AnalysisSettings::default(),
        3,
        &prescriptions,
    );

    assert_eq!(analysis.slow_transitions.len(), 1);
    assert!(!analysis.transition_counts_insufficient);
    assert_eq!(
        (analysis.slow_transitions[0].from.as_str(), analysis.slow_transitions[0].to.as_str()),
        ("t", "h")
    );
    assert_eq!(analysis.slow_transitions[0].n, 3);
    assert!((analysis.slow_transitions[0].milliseconds - 320.0).abs() < 1e-9);
    assert!(
        analysis
            .weak_fingers
            .iter()
            .any(|finger| { finger.name == "left-index" && finger.reason == WeakFingerReason::ErrorRate })
    );
    assert!(
        analysis
            .changes
            .iter()
            .any(|change| { change.part.as_str() == "B" && change.metric == BenchMetric::Wpm && (change.delta - 5.0).abs() < 1e-9 })
    );
    assert!(
        analysis
            .changes
            .iter()
            .any(|change| { change.part.as_str() == "B" && change.metric == BenchMetric::Accuracy && (change.delta - 4.0).abs() < 1e-9 })
    );
    assert!(!analysis.changes.iter().any(|change| change.part.as_str() == "A"));
    assert_eq!(analysis.mistyped_keys.get("f"), Some(&2));
    assert_eq!(
        analysis.practice.first(),
        Some(&PracticeSuggestion::Transition {
            from: "t".into(),
            to: "h".into()
        })
    );

    let sparse = analyze_benchmark_run(
        &current,
        &history,
        &builtin_layout("qwerty").unwrap(),
        &geometry("ansi").unwrap(),
        AnalysisSettings::default(),
        4,
        &[],
    );
    assert!(sparse.slow_transitions.is_empty());
    assert!(sparse.transition_counts_insufficient);
}
