use chrono::{Duration, TimeZone, Utc};
use rs_typing_trainer::analysis::prescription::{
    ControlCandidate, DifficultyState, Evidence, GoalSuggestionInput, Posterior, PrescriptionBaseline, PrescriptionCase, PrescriptionTarget,
    PrescriptionVerdict, adjust_difficulty, evaluate_prescription, prescription_trends, real_word_records_since, record_prescription_session,
    select_controls, suggest_goals,
};
use rs_typing_trainer::config::Config;
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::{ItemKind, Session};
use rs_typing_trainer::record::SessionRecord;
use std::time::Duration as StdDuration;

fn evidence_record(mode: &str, text: &str, kind: Option<ItemKind>, started_at: chrono::DateTime<Utc>) -> SessionRecord {
    let mut session = Session::new(StdDuration::from_secs(10), StdDuration::from_secs(10), Item::exact(text));
    session.activate(StdDuration::ZERO);
    for (index, character) in text.chars().enumerate() {
        session.input_char(StdDuration::from_millis((index as u64 + 1) * 100), &character.to_string());
    }
    session.tick(StdDuration::from_secs(10));
    let mut record = SessionRecord::from_log(&Config::default(), session.into_log(), started_at);
    record.header.mode = mode.to_owned();
    record.log.items[0].kind = kind;
    record
}

fn posterior(ms: f64, variance: f64, n: f64) -> Posterior {
    Posterior {
        estimated_ms: ms,
        log_variance: variance,
        effective_n: n,
    }
}

#[test]
fn difference_in_differences_distinguishes_specific_and_general_improvement() {
    let started = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let case = PrescriptionCase {
        started_at: started,
        sessions: 3,
        target_baseline: posterior(200.0, 0.0004, 20.0),
        control_baselines: vec![posterior(200.0, 0.0004, 20.0)],
    };
    let effective = evaluate_prescription(
        &case,
        &Evidence::real_words(posterior(170.0, 0.0004, 12.0), vec![posterior(200.0, 0.0004, 12.0)]),
        started + Duration::days(3),
    );
    assert_eq!(effective.verdict, PrescriptionVerdict::Effective);
    assert!((effective.target_delta_ms + 30.0).abs() < 1e-9);
    assert!((effective.control_delta_ms - 0.0).abs() < 1e-9);
    assert!(effective.effect < 0.85_f64.ln() + 1e-9);
    assert!(effective.z <= -1.64);

    let general = evaluate_prescription(
        &case,
        &Evidence::real_words(posterior(170.0, 0.0004, 12.0), vec![posterior(170.0, 0.0004, 12.0)]),
        started + Duration::days(3),
    );
    assert_eq!(general.verdict, PrescriptionVerdict::Ineffective);
    assert!(general.effect.abs() < 1e-12);
}

#[test]
fn evidence_threshold_drills_and_fourteen_day_timeout_are_explicit() {
    let started = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let case = PrescriptionCase {
        started_at: started,
        sessions: 3,
        target_baseline: posterior(200.0, 0.001, 20.0),
        control_baselines: vec![posterior(200.0, 0.001, 20.0)],
    };
    let unchanged = Evidence::real_words(posterior(200.0, 0.001, 10.0), vec![posterior(200.0, 0.001, 10.0)]);
    assert_eq!(
        evaluate_prescription(&case, &unchanged, started + Duration::days(2)).verdict,
        PrescriptionVerdict::Ineffective
    );
    let sparse = Evidence::real_words(posterior(160.0, 0.001, 9.9), vec![posterior(200.0, 0.001, 10.0)]);
    assert_eq!(
        evaluate_prescription(&case, &sparse, started + Duration::days(2)).verdict,
        PrescriptionVerdict::InsufficientData
    );
    let drill_only = Evidence::drills(posterior(100.0, 0.0001, 100.0), vec![posterior(200.0, 0.0001, 100.0)]);
    assert_ne!(
        evaluate_prescription(&case, &drill_only, started + Duration::days(2)).verdict,
        PrescriptionVerdict::Effective
    );
    assert!(evaluate_prescription(&case, &sparse, started + Duration::days(14)).closed);
}

#[test]
fn controls_match_kind_frequency_quartile_and_baseline_and_exclude_active_targets() {
    let candidates = vec![
        ControlCandidate::new("a", "b", "same-finger", 2, 105.0),
        ControlCandidate::new("c", "d", "same-finger", 2, 119.0),
        ControlCandidate::new("e", "f", "alternating-hands", 2, 105.0),
        ControlCandidate::new("g", "h", "same-finger", 3, 105.0),
        ControlCandidate::new("i", "j", "same-finger", 2, 121.0),
    ];
    let controls = select_controls("same-finger", 2, 100.0, &candidates, &[("c".into(), "d".into())]);
    assert_eq!(controls.len(), 1);
    assert_eq!((controls[0].from.as_str(), controls[0].to.as_str()), ("a", "b"));
}

#[test]
fn automatic_difficulty_and_goal_suggestions_follow_documented_boundaries() {
    let state = DifficultyState {
        pacer_multiplier: 1.0,
        drill_ratio: 0.30,
    };
    let high = adjust_difficulty(state, &[0.98, 0.99, 0.97], true);
    assert!((high.pacer_multiplier - 1.03).abs() < 1e-12);
    assert!((high.drill_ratio - 0.35).abs() < 1e-12);
    let low = adjust_difficulty(state, &[0.90, 0.91, 0.92], true);
    assert!((low.pacer_multiplier - 0.95).abs() < 1e-12);
    assert!((low.drill_ratio - 0.25).abs() < 1e-12);
    assert_eq!(adjust_difficulty(state, &[1.0, 1.0, 1.0], false), state);

    let goals = suggest_goals(&GoalSuggestionInput {
        recent_wpm: vec![40.0, 42.0, 44.0, 46.0],
        next_grade_wpm: Some(48.0),
        weak_accuracy: Some(0.94),
    });
    assert!(goals.iter().any(|goal| goal.value == 45.0));
    assert!(goals.iter().any(|goal| goal.value == 48.0));
    assert!(goals.len() <= 3);
}

#[test]
fn consecutive_sessions_extend_the_same_active_prescription() {
    let started = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let target = PrescriptionTarget {
        from: "s".into(),
        to: "h".into(),
        kind: "same-finger".into(),
        frequency_quartile: 2,
    };
    let controls = vec![ControlCandidate::new("t", "i", "same-finger", 2, 202.0)];
    let baseline = PrescriptionBaseline {
        target: posterior(200.0, 0.001, 20.0),
        controls: vec![posterior(202.0, 0.001, 20.0)],
    };
    let mut first = record_prescription_session(&[], started, "qwerty", &target, &controls, &baseline);
    first.id = Some(17);
    assert_eq!(first.hypothesis["sessions"], 1);
    let mid_baseline = PrescriptionBaseline {
        target: posterior(190.0, 0.001, 20.0),
        controls: baseline.controls.clone(),
    };
    let continued = record_prescription_session(&[first.clone()], started + Duration::days(1), "qwerty", &target, &controls, &mid_baseline);
    assert_eq!(continued.id, Some(17));
    assert_eq!(continued.created_at, started);
    assert_eq!(continued.hypothesis["sessions"], 2);
    assert_eq!(continued.hypothesis["estimates_ms"].as_array().unwrap().len(), 2);
    assert_eq!(continued.control, first.control);
    let faster_baseline = PrescriptionBaseline {
        target: posterior(180.0, 0.001, 20.0),
        controls: baseline.controls.clone(),
    };
    let third = record_prescription_session(&[continued], started + Duration::days(2), "qwerty", &target, &controls, &faster_baseline);
    assert_eq!(
        prescription_trends(&[third], started + Duration::days(2)).improving,
        vec![("s".into(), "h".into())]
    );
}

#[test]
fn evaluation_uses_all_non_drill_modes_and_excludes_every_adaptive_drill() {
    let started = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let records = vec![
        // A review drill and a drill from an already closed prescription must
        // both be excluded by their persisted kind, independent of ownership.
        evidence_record("adaptive", "xyxyxy", Some(ItemKind::Drill), started + Duration::hours(1)),
        evidence_record("adaptive", "ababab", Some(ItemKind::Drill), started + Duration::hours(2)),
        evidence_record("words", "word", Some(ItemKind::Word), started + Duration::hours(3)),
        evidence_record("ngram", "th", Some(ItemKind::Drill), started + Duration::hours(4)),
        // Old adaptive records have no kind. Only the generic repeated-pair
        // fallback is treated as a drill.
        evidence_record("adaptive", "cdcdcd", None, started + Duration::hours(5)),
        evidence_record("adaptive", "there", None, started + Duration::hours(6)),
        // An explicit kind is authoritative even when the text resembles the
        // legacy adaptive drill shape.
        evidence_record("adaptive", "efefef", Some(ItemKind::Word), started + Duration::hours(7)),
        evidence_record("ngram", "old", None, started + Duration::hours(8)),
    ];

    let evidence = real_word_records_since(&records, started);
    let kept = evidence
        .iter()
        .flat_map(|record| record.log.items.iter().map(move |item| (record.header.mode.as_str(), item.text.as_str())))
        .collect::<Vec<_>>();
    assert_eq!(kept, vec![("words", "word"), ("adaptive", "there"), ("adaptive", "efefef")]);
    assert!(evidence.iter().all(|record| !record.log.keystrokes.is_empty()));
}
