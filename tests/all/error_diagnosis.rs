use chrono::{TimeZone, Utc};
use rs_typing_trainer::analysis::errors::{ErrorKind, diagnose_errors};
use rs_typing_trainer::analysis::metrics::AnalysisSettings;
use rs_typing_trainer::config::ErrorMode;
use rs_typing_trainer::engine::session::{ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};
use rs_typing_trainer::layout::builtin_layout;
use rs_typing_trainer::layout::geometry::geometry;
use rs_typing_trainer::record::{SessionHeader, SessionRecord};

type TestStroke<'a> = (&'a str, &'a str, bool, &'a str);
type TestItem<'a> = (&'a str, Vec<TestStroke<'a>>);

fn record(layout: &str, input_layout: &str, error_mode: ErrorMode, items: Vec<TestItem<'_>>) -> SessionRecord {
    let mut item_logs = Vec::new();
    let mut keystrokes = Vec::new();
    let mut t_us = 100_000_u64;
    for (item_idx, (text, keys)) in items.into_iter().enumerate() {
        item_logs.push(ItemLog {
            idx: item_idx as u32,
            text: text.to_owned(),
            reading: None,
            kind: None,
            shown_us: t_us.saturating_sub(50_000),
            done_us: Some(t_us + keys.len() as u64 * 100_000),
            outcome: ItemOutcome::Completed,
        });
        let mut position = 0_u32;
        for (typed, expected, correct, code) in keys {
            keystrokes.push(KeystrokeLog {
                seq: keystrokes.len() as u32,
                t_us,
                release_us: None,
                item_idx: item_idx as u32,
                pos: position,
                kind: KeystrokeKind::Char,
                typed: Some(typed.to_owned()),
                expected: Some(expected.to_owned()),
                correct,
                code: Some(code.to_owned()),
            });
            if correct || error_mode == ErrorMode::Advance {
                position += 1;
            }
            t_us += 100_000;
        }
        t_us += 200_000;
    }
    SessionRecord {
        header: SessionHeader {
            id: None,
            started_at: "2026-11-15T00:00:00Z".into(),
            finished_at: "2026-11-15T00:00:10Z".into(),
            duration_ms: 10_000,
            mode: "words".into(),
            mode_params: "{}".into(),
            bench_id: None,
            layout: layout.into(),
            input_layout: input_layout.into(),
            geometry: "ansi".into(),
            error_mode: match error_mode {
                ErrorMode::Stop => "stop",
                ErrorMode::Advance => "advance",
            }
            .into(),
            release_events: false,
            app_version: "test".into(),
            metrics_version: 2,
            net_wpm: 0.0,
            raw_wpm: Some(0.0),
            accuracy: 0.0,
            iki_median_ms: 100.0,
            iki_cv: 0.0,
            cps: 0.0,
            consistency: 0.0,
            max_combo: 0,
            completed_words: item_logs.len() as u32,
            timeout_words: 0,
            mistypes_total: keystrokes.iter().filter(|key| !key.correct).count() as u32,
            legacy_payload: None,
        },
        log: SessionLog {
            duration_us: 10_000_000,
            error_mode,
            items: item_logs,
            keystrokes,
        },
    }
}

fn diagnose(records: &[SessionRecord], layout_name: &str) -> rs_typing_trainer::analysis::errors::ErrorDiagnosis {
    diagnose_errors(
        records,
        &builtin_layout(layout_name).unwrap(),
        &geometry("ansi").unwrap(),
        AnalysisSettings::default(),
        Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap(),
    )
}

fn timed_record(error_mode: ErrorMode, target: &str, keys: &[(u64, u32, char, char, bool)], duration_us: u64) -> SessionRecord {
    let layout = builtin_layout("qwerty").unwrap();
    let mut sample = record("qwerty", "same", error_mode, vec![]);
    sample.header.duration_ms = (duration_us / 1_000) as i64;
    sample.header.completed_words = 1;
    sample.header.mistypes_total = keys.iter().filter(|row| !row.4).count() as u32;
    sample.log.duration_us = duration_us;
    sample.log.items = vec![ItemLog {
        idx: 0,
        text: target.to_owned(),
        reading: None,
        kind: None,
        shown_us: 0,
        done_us: Some(duration_us),
        outcome: ItemOutcome::Completed,
    }];
    sample.log.keystrokes = keys
        .iter()
        .enumerate()
        .map(|(seq, (t_us, pos, typed, expected, correct))| KeystrokeLog {
            seq: seq as u32,
            t_us: *t_us,
            release_us: None,
            item_idx: 0,
            pos: *pos,
            kind: KeystrokeKind::Char,
            typed: Some(typed.to_string()),
            expected: Some(expected.to_string()),
            correct: *correct,
            code: layout.stroke_for(*typed).map(|stroke| stroke.code),
        })
        .collect();
    sample
}

#[test]
fn stop_mode_classifies_six_substitution_patterns_and_repeated_errors() {
    let sample = record(
        "qwerty",
        "same",
        ErrorMode::Stop,
        vec![
            ("as", vec![("s", "a", false, "KeyS"), ("a", "a", true, "KeyA"), ("s", "s", true, "KeyS")]),
            ("as", vec![("a", "a", true, "KeyA"), ("a", "s", false, "KeyA"), ("s", "s", true, "KeyS")]),
            ("fa", vec![("g", "f", false, "KeyG"), ("f", "f", true, "KeyF"), ("a", "a", true, "KeyA")]),
            (
                "fa",
                vec![("5", "f", false, "Digit5"), ("f", "f", true, "KeyF"), ("a", "a", true, "KeyA")],
            ),
            ("fa", vec![("j", "f", false, "KeyJ"), ("f", "f", true, "KeyF"), ("a", "a", true, "KeyA")]),
            ("ad", vec![("k", "a", false, "KeyK"), ("a", "a", true, "KeyA"), ("d", "d", true, "KeyD")]),
            ("da", vec![("x", "d", false, "KeyX"), ("c", "d", false, "KeyC"), ("d", "d", true, "KeyD")]),
        ],
    );
    let result = diagnose(&[sample], "qwerty");

    for kind in [
        ErrorKind::Anticipation,
        ErrorKind::RepeatedPrevious,
        ErrorKind::AdjacentKey,
        ErrorKind::SameFinger,
        ErrorKind::OppositeHand,
        ErrorKind::OtherSubstitution,
        ErrorKind::Consecutive,
    ] {
        assert!(result.kind_count(kind) >= 1, "missing {kind:?}: {result:#?}");
    }
    let first_error_proportion = result
        .kinds
        .iter()
        .filter(|row| row.kind != ErrorKind::Consecutive)
        .map(|row| row.proportion)
        .sum::<f64>();
    let first_error_count = result
        .kinds
        .iter()
        .filter(|row| row.kind != ErrorKind::Consecutive)
        .map(|row| row.n)
        .sum::<u64>();
    let consecutive = result.kinds.iter().find(|row| row.kind == ErrorKind::Consecutive).unwrap();
    assert!((first_error_proportion - 1.0).abs() < 1e-12, "{:#?}", result.kinds);
    assert!((consecutive.proportion - consecutive.n as f64 / first_error_count as f64).abs() < 1e-12);
    assert!(result.confusion.iter().any(|row| row.expected == "f" && row.typed == "g" && row.n == 1));
}

#[test]
fn physical_adjacency_uses_logged_codes_for_dvorak_on_qwerty_input() {
    let dvorak = builtin_layout("dvorak").unwrap();
    let expected = dvorak.output("KeyD", false).unwrap().to_string();
    let typed = dvorak.output("KeyF", false).unwrap().to_string();
    let sample = record(
        "dvorak",
        "qwerty",
        ErrorMode::Stop,
        vec![("zz", vec![(typed.as_str(), expected.as_str(), false, "KeyF")])],
    );

    assert_eq!(diagnose(&[sample], "dvorak").kind_count(ErrorKind::AdjacentKey), 1);
}

#[test]
fn advance_mode_alignment_finds_substitution_insertion_omission_and_transposition() {
    let sample = record(
        "qwerty",
        "same",
        ErrorMode::Advance,
        vec![
            ("cat", vec![("c", "c", true, "KeyC"), ("u", "a", false, "KeyU"), ("t", "t", true, "KeyT")]),
            (
                "cat",
                vec![
                    ("c", "c", true, "KeyC"),
                    ("a", "a", true, "KeyA"),
                    ("r", "t", false, "KeyR"),
                    ("t", "t", true, "KeyT"),
                ],
            ),
            (
                "cart",
                vec![("c", "c", true, "KeyC"), ("a", "a", true, "KeyA"), ("t", "r", false, "KeyT")],
            ),
            (
                "form",
                vec![
                    ("f", "f", true, "KeyF"),
                    ("r", "o", false, "KeyR"),
                    ("o", "r", false, "KeyO"),
                    ("m", "m", true, "KeyM"),
                ],
            ),
        ],
    );
    let result = diagnose(&[sample], "qwerty");

    assert!(result.substitution_count() >= 1, "{result:#?}");
    assert_eq!(result.kind_count(ErrorKind::Insertion), 1, "{result:#?}");
    assert_eq!(result.kind_count(ErrorKind::Omission), 1, "{result:#?}");
    assert_eq!(result.kind_count(ErrorKind::Transposition), 1, "{result:#?}");
    assert!(result.confusion.iter().any(|row| row.expected == "a" && row.typed == "u"));
}

#[test]
fn post_error_slowdown_and_pre_error_rhythm_match_hand_calculation() {
    let mut constant = Vec::new();
    let mut time = 100_000_u64;
    let mut target = String::new();
    for position in 0..80_u32 {
        let character = if position % 2 == 0 { 'a' } else { 's' };
        target.push(character);
        constant.push((time, position, character, character, true));
        time += if position % 2 == 0 { 75_000 } else { 125_000 };
    }
    let constant = timed_record(ErrorMode::Stop, &target, &constant, time);

    let mut rhythm = Vec::new();
    let mut time = 100_000_u64;
    let mut target = String::new();
    for position in 0..40_u32 {
        let character = if position % 2 == 0 { 'a' } else { 's' };
        target.push(character);
        rhythm.push((time, position, character, character, true));
        time += if position % 2 == 0 { 75_000 } else { 125_000 };
    }
    let rhythm = timed_record(ErrorMode::Stop, &target, &rhythm, time);

    let error = timed_record(
        ErrorMode::Stop,
        "asasasas",
        &[
            (100_000, 0, 'a', 'a', true),
            (150_000, 1, 's', 's', true),
            (300_000, 2, 'a', 'a', true),
            (350_000, 3, 's', 's', true),
            (500_000, 4, 'x', 'a', false),
            (700_000, 4, 'a', 'a', true),
            (1_000_000, 5, 's', 's', true),
            (1_400_000, 6, 'a', 'a', true),
        ],
        1_500_000,
    );
    let result = diagnose(&[constant, rhythm, error], "qwerty");
    let slowdown = result.post_error_slowdown.as_ref().unwrap();
    assert!((slowdown.post_error_median_ms - 200.0).abs() < 35.0, "{slowdown:?}");
    assert!(slowdown.baseline_median_ms.abs() < 35.0, "{slowdown:?}");
    assert!((150.0..=250.0).contains(&slowdown.excess_ms), "{slowdown:?}");
    let rhythm = result.pre_error_rhythm.as_ref().unwrap();
    assert!((rhythm.pre_error_sd_median_ms - 50.0).abs() < 1e-6, "{rhythm:?}");
    assert!((rhythm.baseline_sd_median_ms - 25.0).abs() < 1e-6, "{rhythm:?}");
    assert!((rhythm.ratio - 2.0).abs() < 1e-6, "{rhythm:?}");
}

#[test]
fn correction_cost_reports_kspc_time_share_and_advance_residual_rate() {
    let stop = timed_record(
        ErrorMode::Stop,
        "as",
        &[(100_000, 0, 'a', 'a', true), (200_000, 1, 'x', 's', false), (500_000, 1, 's', 's', true)],
        1_000_000,
    );
    let stop_result = diagnose(&[stop], "qwerty");
    assert!((stop_result.correction.kspc - 1.5).abs() < 1e-9, "{:?}", stop_result.correction);
    assert!((stop_result.correction.correction_time_ms - 300.0).abs() < 1e-9);
    assert!((stop_result.correction.correction_time_ratio - 0.3).abs() < 1e-9);
    assert_eq!(stop_result.correction.residual_error_rate, None);

    let advance = timed_record(
        ErrorMode::Advance,
        "cat",
        &[(100_000, 0, 'c', 'c', true), (200_000, 1, 'u', 'a', false), (300_000, 2, 't', 't', true)],
        1_000_000,
    );
    let advance_result = diagnose(&[advance], "qwerty");
    assert_eq!(advance_result.correction.residual_error_rate, Some(1.0 / 3.0));

    let mut corrected_advance = timed_record(
        ErrorMode::Advance,
        "cat",
        &[
            (100_000, 0, 'c', 'c', true),
            (200_000, 1, 'u', 'a', false),
            (400_000, 1, 'a', 'a', true),
            (500_000, 2, 't', 't', true),
        ],
        1_000_000,
    );
    corrected_advance.log.keystrokes.insert(
        2,
        KeystrokeLog {
            seq: 2,
            t_us: 300_000,
            release_us: None,
            item_idx: 0,
            pos: 2,
            kind: KeystrokeKind::Backspace,
            typed: None,
            expected: Some("t".into()),
            correct: false,
            code: Some("Backspace".into()),
        },
    );
    for (seq, key) in corrected_advance.log.keystrokes.iter_mut().enumerate() {
        key.seq = seq as u32;
    }
    let corrected_result = diagnose(&[corrected_advance], "qwerty");
    assert_eq!(corrected_result.correction.residual_error_rate, Some(0.0));
    assert!((corrected_result.correction.correction_time_ms - 200.0).abs() < 1e-9);
}

#[test]
fn attacking_interval_error_rate_is_compared_with_other_intervals() {
    let mut keys = Vec::new();
    let mut target = String::new();
    let mut time = 100_000_u64;
    for (position, index) in (0..20).enumerate() {
        let position = position as u32;
        let expected = if position.is_multiple_of(2) { 'a' } else { 's' };
        target.push(expected);
        let interval = if index == 5 {
            50_000
        } else if index == 15 {
            200_000
        } else {
            100_000
        };
        time += interval;
        let error = index == 5 || index == 15;
        keys.push((time, position, if error { 'x' } else { expected }, expected, !error));
        if error {
            time += 100_000;
            keys.push((time, position, expected, expected, true));
        }
    }
    let result = diagnose(&[timed_record(ErrorMode::Stop, &target, &keys, time + 100_000)], "qwerty");
    let attack = result.attack_errors.as_ref().unwrap();
    assert!((attack.p25_ms - 100.0).abs() < 1e-9, "{attack:?}");
    assert_eq!(attack.fast_error_rate, 1.0);
    assert!(attack.other_error_rate < 0.2, "{attack:?}");
    assert!(attack.ratio > 5.0, "{attack:?}");
}
