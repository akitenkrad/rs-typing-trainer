use chrono::{TimeZone, Utc};
use rs_typing_trainer::analysis::metrics::AnalysisSettings;
use rs_typing_trainer::analysis::model::{ModelView, build_player_model};
use rs_typing_trainer::analysis::style::diagnose_style;
use rs_typing_trainer::config::ErrorMode;
use rs_typing_trainer::engine::session::{ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};
use rs_typing_trainer::layout::builtin_layout;
use rs_typing_trainer::layout::geometry::geometry;
use rs_typing_trainer::record::{SessionHeader, SessionRecord};

type TimedKey = (char, u64, Option<u64>);
type TestItem<'a> = (&'a str, Vec<TimedKey>);

fn record(items: Vec<TestItem<'_>>, release_events: bool) -> SessionRecord {
    record_with_preview(items, release_events, None)
}

fn record_with_preview(items: Vec<TestItem<'_>>, release_events: bool, preview_items: Option<usize>) -> SessionRecord {
    let layout = builtin_layout("qwerty").unwrap();
    let mut item_logs = Vec::new();
    let mut keystrokes = Vec::new();
    for (item_idx, (text, keys)) in items.into_iter().enumerate() {
        item_logs.push(ItemLog {
            idx: item_idx as u32,
            text: text.into(),
            reading: None,
            kind: None,
            shown_us: keys.first().map_or(0, |key| key.1.saturating_sub(50_000)),
            done_us: keys.last().map(|key| key.1),
            outcome: ItemOutcome::Completed,
        });
        for (position, (character, t_us, release_us)) in keys.into_iter().enumerate() {
            keystrokes.push(KeystrokeLog {
                seq: keystrokes.len() as u32,
                t_us,
                release_us,
                item_idx: item_idx as u32,
                pos: position as u32,
                kind: KeystrokeKind::Char,
                typed: Some(character.to_string()),
                expected: Some(character.to_string()),
                correct: true,
                code: layout.stroke_for(character).map(|stroke| stroke.code),
            });
        }
    }
    let duration_us = keystrokes.last().map_or(0, |key| key.t_us + 100_000);
    SessionRecord {
        header: SessionHeader {
            id: None,
            started_at: "2026-11-15T00:00:00Z".into(),
            finished_at: "2026-11-15T00:00:10Z".into(),
            duration_ms: (duration_us / 1_000) as i64,
            mode: "words".into(),
            mode_params: preview_items.map_or_else(|| "{}".to_owned(), |count| serde_json::json!({ "preview_items": count }).to_string()),
            bench_id: None,
            layout: "qwerty".into(),
            input_layout: "same".into(),
            geometry: "ansi".into(),
            error_mode: "stop".into(),
            release_events,
            app_version: "test".into(),
            metrics_version: 2,
            net_wpm: 0.0,
            raw_wpm: Some(0.0),
            accuracy: 1.0,
            iki_median_ms: 0.0,
            iki_cv: 0.0,
            cps: 0.0,
            consistency: 0.0,
            max_combo: 0,
            completed_words: item_logs.len() as u32,
            timeout_words: 0,
            mistypes_total: 0,
            legacy_payload: None,
        },
        log: SessionLog {
            duration_us,
            error_mode: ErrorMode::Stop,
            items: item_logs,
            keystrokes,
        },
    }
}

fn model_record(interval_ms: u64, repetitions: usize) -> SessionRecord {
    let mut items = Vec::new();
    let mut time = 100_000_u64;
    for _ in 0..repetitions {
        items.push(("as", vec![('a', time, None), ('s', time + interval_ms * 1_000, None)]));
        time += 500_000;
    }
    record(items, false)
}

#[test]
fn word_start_delay_and_rhythm_use_only_spaces_inside_items() {
    let sample = record_with_preview(
        vec![
            ("as", vec![('a', 100_000, None), ('s', 200_000, None)]),
            ("df", vec![('d', 400_000, None), ('f', 500_000, None)]),
            (
                "as df",
                vec![
                    ('a', 700_000, None),
                    ('s', 800_000, None),
                    (' ', 850_000, None),
                    ('d', 1_000_000, None),
                    ('f', 1_100_000, None),
                ],
            ),
        ],
        false,
        Some(1),
    );
    let layout = builtin_layout("qwerty").unwrap();
    let keyboard = geometry("ansi").unwrap();
    let model = build_player_model(
        std::slice::from_ref(&sample),
        &layout,
        &keyboard,
        AnalysisSettings::default(),
        ModelView::AllTime,
        Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap(),
    );
    let result = diagnose_style(&[sample], &model);

    let word_start = result.word_start.unwrap();
    assert_eq!(word_start.n, 1);
    // Space (850 ms) to the next word's first letter (1,000 ms): one interval, not two.
    assert!((word_start.start_median_ms - 150.0).abs() < 1e-9, "{word_start:?}");
    assert!((word_start.internal_median_ms - 100.0).abs() < 1e-9);
    assert!((word_start.ratio - 1.5).abs() < 1e-9);
    assert_eq!(result.rhythm.between_word_n, 1);
    assert_eq!(result.rhythm.between_word_cv, None);
}

#[test]
fn item_boundaries_are_excluded_regardless_of_preview_but_inside_item_spaces_are_included() {
    let boundary_record = |preview_items| {
        let mut sample = record_with_preview(
            vec![
                ("as", vec![('a', 100_000, None), ('s', 220_000, None)]),
                ("df", vec![('d', 1_100_000, None), ('f', 1_220_000, None)]),
            ],
            false,
            preview_items,
        );
        let previous_done_us = sample.log.items[0].done_us.unwrap();
        sample.log.items[1].shown_us = previous_done_us + 400_000;
        sample
    };
    let visible = boundary_record(Some(1));
    let hidden = boundary_record(Some(0));
    let legacy = boundary_record(None);
    let inside_item = record_with_preview(
        vec![(
            "as df",
            vec![
                ('a', 100_000, None),
                ('s', 220_000, None),
                (' ', 300_000, None),
                ('d', 700_000, None),
                ('f', 820_000, None),
            ],
        )],
        false,
        None,
    );
    let layout = builtin_layout("qwerty").unwrap();
    let keyboard = geometry("ansi").unwrap();
    let model = build_player_model(
        &[visible.clone(), hidden.clone(), legacy.clone(), inside_item.clone()],
        &layout,
        &keyboard,
        AnalysisSettings::default(),
        ModelView::AllTime,
        Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap(),
    );

    for sample in [visible, hidden, legacy] {
        let result = diagnose_style(&[sample], &model);
        assert_eq!(result.word_start, None);
        assert_eq!(result.rhythm.within_word_n, 2);
        assert_eq!(result.rhythm.between_word_n, 0);
        assert_eq!(result.rhythm.between_word_cv, None);
    }

    let inside_result = diagnose_style(&[inside_item], &model);
    let inside_start = inside_result.word_start.unwrap();
    assert_eq!(inside_start.n, 1);
    assert!((inside_start.start_median_ms - 400.0).abs() < 1e-9);
    assert!((inside_start.internal_median_ms - 120.0).abs() < 1e-9);
    assert!((inside_start.ratio - 400.0 / 120.0).abs() < 1e-9);
    assert_eq!(inside_result.rhythm.between_word_n, 1);
}

#[test]
fn repeated_word_timing_is_compared_with_transition_model() {
    let layout = builtin_layout("qwerty").unwrap();
    let keyboard = geometry("ansi").unwrap();
    let baseline = model_record(100, 100);
    let model = build_player_model(
        &[baseline],
        &layout,
        &keyboard,
        AnalysisSettings::default(),
        ModelView::AllTime,
        Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap(),
    );
    let sample = model_record(80, 3);
    let result = diagnose_style(&[sample], &model);
    let word = result.words.iter().find(|row| row.word == "as").unwrap();

    assert_eq!(word.n, 3);
    assert!((word.observed_ms - 80.0).abs() < 1e-9);
    assert!((word.predicted_ms - 100.0).abs() < 1.0, "{word:?}");
    assert!((word.ratio - 0.8).abs() < 0.01, "{word:?}");
}

#[test]
fn rollover_uses_only_sessions_with_release_events_and_reports_overlap_distribution() {
    let measured = record(
        vec![(
            "asd",
            vec![
                ('a', 100_000, Some(250_000)),
                ('s', 200_000, Some(380_000)),
                ('d', 400_000, Some(450_000)),
            ],
        )],
        true,
    );
    let unmeasured = record(vec![("as", vec![('a', 100_000, None), ('s', 200_000, None)])], false);
    let layout = builtin_layout("qwerty").unwrap();
    let keyboard = geometry("ansi").unwrap();
    let model = build_player_model(
        &[measured.clone(), unmeasured.clone()],
        &layout,
        &keyboard,
        AnalysisSettings::default(),
        ModelView::AllTime,
        Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap(),
    );

    let rollover = diagnose_style(&[measured], &model).rollover.unwrap();
    assert_eq!(rollover.comparable_pairs, 2);
    assert_eq!(rollover.overlap_pairs, 1);
    assert!((rollover.rate - 0.5).abs() < 1e-9);
    assert!((rollover.overlap_median_ms - 50.0).abs() < 1e-9);
    assert!((rollover.overlap_p90_ms - 50.0).abs() < 1e-9);
    assert_eq!(diagnose_style(&[unmeasured], &model).rollover, None);
}

#[test]
fn rhythm_is_split_between_within_word_and_inside_item_word_boundaries() {
    let sample = record(
        vec![(
            "as df jk",
            vec![
                ('a', 100_000, None),
                ('s', 200_000, None),
                (' ', 250_000, None),
                ('d', 600_000, None),
                ('f', 800_000, None),
                (' ', 850_000, None),
                ('j', 1_400_000, None),
                ('k', 1_700_000, None),
            ],
        )],
        false,
    );
    let layout = builtin_layout("qwerty").unwrap();
    let keyboard = geometry("ansi").unwrap();
    let model = build_player_model(
        std::slice::from_ref(&sample),
        &layout,
        &keyboard,
        AnalysisSettings::default(),
        ModelView::AllTime,
        Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap(),
    );
    let rhythm = diagnose_style(&[sample], &model).rhythm;

    assert_eq!(rhythm.within_word_n, 3);
    assert_eq!(rhythm.between_word_n, 2);
    assert!((rhythm.within_word_cv.unwrap() - (2.0_f64 / 3.0).sqrt() / 2.0).abs() < 1e-9, "{rhythm:?}");
    // Space-to-first-letter intervals are 350 ms and 550 ms: mean 450, population SD 100.
    assert!((rhythm.between_word_cv.unwrap() - 100.0 / 450.0).abs() < 1e-9, "{rhythm:?}");
}

#[test]
fn within_word_intervals_skip_a_correction_between_correct_keys() {
    let mut sample = record(
        vec![(
            "asdf",
            vec![('a', 100_000, None), ('s', 220_000, None), ('d', 900_000, None), ('f', 1_020_000, None)],
        )],
        false,
    );
    // Mistype 'x' and erase it between 's' and 'd'; the 680 ms from 's' to 'd' is a
    // correction, not an inter-key interval.
    let mistype = KeystrokeLog {
        t_us: 300_000,
        pos: 2,
        typed: Some("x".into()),
        expected: Some("d".into()),
        correct: false,
        ..sample.log.keystrokes[2].clone()
    };
    let backspace = KeystrokeLog {
        t_us: 500_000,
        pos: 2,
        kind: KeystrokeKind::Backspace,
        typed: None,
        expected: None,
        correct: false,
        code: None,
        ..sample.log.keystrokes[2].clone()
    };
    sample.log.keystrokes.splice(2..2, [mistype, backspace]);
    for (seq, key) in sample.log.keystrokes.iter_mut().enumerate() {
        key.seq = seq as u32;
    }
    let layout = builtin_layout("qwerty").unwrap();
    let keyboard = geometry("ansi").unwrap();
    let model = build_player_model(
        std::slice::from_ref(&sample),
        &layout,
        &keyboard,
        AnalysisSettings::default(),
        ModelView::AllTime,
        Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap(),
    );
    let rhythm = diagnose_style(&[sample], &model).rhythm;

    assert_eq!(rhythm.within_word_n, 2, "{rhythm:?}");
    assert!(rhythm.within_word_cv.unwrap().abs() < 1e-9, "{rhythm:?}");
}
