use rs_typing_trainer::analysis::metrics::{AnalysisSettings, TransitionClass, analyze_records};
use rs_typing_trainer::config::ErrorMode;
use rs_typing_trainer::engine::session::{ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};
use rs_typing_trainer::layout::builtin_layout;
use rs_typing_trainer::layout::geometry::geometry;
use rs_typing_trainer::record::{SessionHeader, SessionRecord};

fn key(seq: u32, t_us: u64, item_idx: u32, expected: &str, correct: bool) -> KeystrokeLog {
    KeystrokeLog {
        seq,
        t_us,
        release_us: Some(t_us + 50_000),
        item_idx,
        pos: seq,
        kind: KeystrokeKind::Char,
        typed: Some(if correct { expected } else { "x" }.to_owned()),
        expected: Some(expected.to_owned()),
        correct,
        code: builtin_layout("qwerty")
            .unwrap()
            .stroke_for(if correct { expected.chars().next().unwrap() } else { 'x' })
            .map(|stroke| stroke.code),
    }
}

fn record(metrics_version: i64, keystrokes: Vec<KeystrokeLog>) -> SessionRecord {
    SessionRecord {
        header: SessionHeader {
            id: None,
            started_at: "2026-11-14T00:00:00Z".into(),
            finished_at: "2026-11-14T00:00:05Z".into(),
            duration_ms: 5_000,
            mode: "words".into(),
            mode_params: "{}".into(),
            bench_id: None,
            layout: "qwerty".into(),
            input_layout: "same".into(),
            geometry: "ansi".into(),
            error_mode: "stop".into(),
            release_events: true,
            app_version: "test".into(),
            metrics_version,
            net_wpm: 0.0,
            raw_wpm: Some(0.0),
            accuracy: 0.0,
            iki_median_ms: 0.0,
            iki_cv: 0.0,
            cps: 0.0,
            consistency: 0.0,
            max_combo: 0,
            completed_words: 0,
            timeout_words: 0,
            mistypes_total: 0,
            legacy_payload: None,
        },
        log: SessionLog {
            duration_us: 5_000_000,
            error_mode: ErrorMode::Stop,
            items: vec![
                ItemLog {
                    idx: 0,
                    text: "asddf".into(),
                    reading: None,
                    shown_us: 0,
                    done_us: Some(3_000_000),
                    outcome: ItemOutcome::Completed,
                },
                ItemLog {
                    idx: 1,
                    text: "gh".into(),
                    reading: None,
                    shown_us: 4_000_000,
                    done_us: Some(4_400_000),
                    outcome: ItemOutcome::Completed,
                },
                ItemLog {
                    idx: 2,
                    text: "fg".into(),
                    reading: None,
                    shown_us: 4_500_000,
                    done_us: Some(4_750_000),
                    outcome: ItemOutcome::Completed,
                },
            ],
            keystrokes,
        },
    }
}

#[test]
fn detailed_metrics_follow_item_correctness_pause_and_version_rules() {
    let current = record(
        2,
        vec![
            key(0, 100_000, 0, "a", true),
            key(1, 200_000, 0, "s", true),
            key(2, 300_000, 0, "d", false),
            key(3, 400_000, 0, "d", true),
            key(4, 3_000_000, 0, "f", true),
            key(5, 4_200_000, 1, "g", true),
            key(6, 4_400_000, 1, "h", true),
            key(7, 4_600_000, 2, "f", true),
            key(8, 4_750_000, 2, "g", true),
        ],
    );
    let legacy = record(0, vec![key(0, 100_000, 0, "z", false)]);
    let metrics = analyze_records(
        &[current, legacy],
        &builtin_layout("qwerty").unwrap(),
        &geometry("ansi").unwrap(),
        AnalysisSettings {
            pause_ms: 2_000,
            shrinkage_k: 10.0,
        },
    );

    let d = metrics.keys.iter().find(|row| row.key == "d").unwrap();
    assert_eq!((d.correct, d.total), (1, 2));
    assert_eq!(d.accuracy, 0.5);
    assert!(metrics.keys.iter().all(|row| row.key != "z"));

    let as_pair = metrics.bigrams.iter().find(|row| row.from == "a" && row.to == "s").unwrap();
    assert_eq!(as_pair.n, 1);
    assert_eq!(as_pair.median_ms, 100.0);
    assert!(!metrics.bigrams.iter().any(|row| row.from == "s" && row.to == "d"));
    assert!(!metrics.bigrams.iter().any(|row| row.from == "d" && row.to == "d"));
    assert!(
        !metrics
            .bigrams
            .iter()
            .any(|row| row.from == "f" && row.to == "g" && row.median_ms > 2_000.0)
    );

    assert_eq!(metrics.iki_median_ms, 100.0);
    assert!((metrics.iki_cv - 0.308).abs() < 0.01);
    assert_eq!(metrics.reaction_median_ms, 100.0);
    assert_eq!(metrics.correction_median_ms, 100.0);
    assert_eq!(metrics.same_finger_median_ms, 150.0);
    assert_eq!(metrics.alternating_hands_median_ms, 200.0);
    assert!(metrics.bigrams.iter().any(|row| row.class == TransitionClass::AlternatingHands));
    assert!(metrics.fingers.iter().any(|row| row.name == "left-index" && row.n > 0));
    assert_eq!(metrics.hold_median_ms, Some(50.0));
}

#[test]
fn japanese_spelling_choices_are_reported_as_ratios() {
    let mut first = record(2, vec![key(0, 100_000, 0, "s", true), key(1, 200_000, 0, "i", true)]);
    first.log.items.truncate(1);
    first.log.items[0].reading = Some("し".into());
    let mut second = record(
        2,
        vec![
            key(0, 100_000, 0, "s", true),
            key(1, 150_000, 0, "h", true),
            key(2, 200_000, 0, "i", true),
        ],
    );
    second.log.items.truncate(1);
    second.log.items[0].reading = Some("し".into());
    let metrics = analyze_records(
        &[first, second],
        &builtin_layout("qwerty").unwrap(),
        &geometry("ansi").unwrap(),
        AnalysisSettings::default(),
    );

    let si = metrics.spelling_choices.iter().find(|row| row.variant == "si").unwrap();
    let shi = metrics.spelling_choices.iter().find(|row| row.variant == "shi").unwrap();
    assert_eq!((si.count, shi.count), (1, 1));
    assert_eq!(si.ratio, 0.5);
    assert_eq!(shi.ratio, 0.5);
}
