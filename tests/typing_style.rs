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
fn word_start_delay_excludes_reaction_and_includes_item_and_space_boundaries() {
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
    assert_eq!(word_start.n, 3);
    assert!((word_start.start_median_ms - 200.0).abs() < 1e-9, "{word_start:?}");
    assert!((word_start.internal_median_ms - 100.0).abs() < 1e-9);
    assert!((word_start.ratio - 2.0).abs() < 1e-9);
}

#[test]
fn word_start_uses_only_boundaries_that_were_visible_before_typing() {
    let items = || {
        vec![
            ("as", vec![('a', 100_000, None), ('s', 200_000, None)]),
            ("df", vec![('d', 500_000, None), ('f', 600_000, None)]),
        ]
    };
    let visible = record_with_preview(items(), false, Some(1));
    let hidden = record_with_preview(items(), false, Some(0));
    let legacy = record_with_preview(items(), false, None);
    let inside_item = record_with_preview(
        vec![(
            "as df",
            vec![
                ('a', 100_000, None),
                ('s', 200_000, None),
                (' ', 250_000, None),
                ('d', 500_000, None),
                ('f', 600_000, None),
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

    let visible_start = diagnose_style(&[visible], &model).word_start.unwrap();
    assert_eq!(visible_start.n, 1);
    assert!((visible_start.start_median_ms - 300.0).abs() < 1e-9);
    assert!(diagnose_style(&[hidden], &model).word_start.is_none());
    assert!(diagnose_style(&[legacy], &model).word_start.is_none());

    let inside_start = diagnose_style(&[inside_item], &model).word_start.unwrap();
    assert_eq!(inside_start.n, 1);
    assert!((inside_start.start_median_ms - 300.0).abs() < 1e-9);
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
fn rhythm_is_split_between_inside_words_and_word_boundaries() {
    let sample = record(
        vec![
            ("as", vec![('a', 100_000, None), ('s', 200_000, None)]),
            ("df", vec![('d', 500_000, None), ('f', 700_000, None)]),
            ("jk", vec![('j', 1_200_000, None), ('k', 1_500_000, None)]),
        ],
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
    assert!((rhythm.between_word_cv.unwrap() - 0.25).abs() < 1e-9, "{rhythm:?}");
}
