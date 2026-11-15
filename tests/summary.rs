use chrono::NaiveDate;
use rs_typing_trainer::analysis::metrics::{BigramMetric, DetailedMetrics, TransitionClass};
use rs_typing_trainer::analysis::summary;
use rs_typing_trainer::config::{Config, ErrorMode};
use rs_typing_trainer::engine::session::{ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::layout::builtin_layout;
use rs_typing_trainer::record::{SessionHeader, SessionRecord};

fn key(seq: u32, expected: &str, correct: bool, t_us: u64) -> KeystrokeLog {
    let character = if correct { expected.chars().next().unwrap() } else { 'x' };
    KeystrokeLog {
        seq,
        t_us,
        release_us: Some(t_us + 50_000),
        item_idx: 0,
        pos: seq,
        kind: KeystrokeKind::Char,
        typed: Some(character.to_string()),
        expected: Some(expected.to_owned()),
        correct,
        code: builtin_layout("qwerty").unwrap().stroke_for(character).map(|stroke| stroke.code),
    }
}

fn record(date: &str, mode: &str, bench_id: Option<&str>, wpm: f64, accuracy: f64) -> SessionRecord {
    SessionRecord {
        header: SessionHeader {
            id: None,
            started_at: format!("{date}T00:00:00Z"),
            finished_at: format!("{date}T00:00:05Z"),
            duration_ms: 5_000,
            mode: mode.to_owned(),
            mode_params: "{}".into(),
            bench_id: bench_id.map(str::to_owned),
            layout: "qwerty".into(),
            input_layout: "same".into(),
            geometry: "ansi".into(),
            error_mode: "stop".into(),
            release_events: true,
            app_version: "test".into(),
            metrics_version: 2,
            net_wpm: wpm,
            raw_wpm: Some(wpm + 5.0),
            accuracy,
            iki_median_ms: 100.0,
            iki_cv: 0.1,
            cps: 4.0,
            consistency: 0.9,
            max_combo: 20,
            completed_words: 10,
            timeout_words: 0,
            mistypes_total: u32::from(accuracy < 1.0),
            legacy_payload: None,
        },
        log: SessionLog {
            duration_us: 5_000_000,
            error_mode: ErrorMode::Stop,
            items: vec![ItemLog {
                idx: 0,
                text: "asdf".into(),
                reading: None,
                kind: None,
                shown_us: 0,
                done_us: Some(1_100_000),
                outcome: ItemOutcome::Completed,
            }],
            keystrokes: vec![
                key(0, "a", true, 100_000),
                key(1, "s", true, 900_000),
                key(2, "d", true, 1_000_000),
                key(3, "f", false, 1_100_000),
            ],
        },
    }
}

#[test]
fn japanese_summary_applies_accuracy_transition_and_stale_bench_rules() {
    let records = vec![
        record("2026-10-01", "bench", Some("bench-v1/A/01"), 50.0, 0.98),
        record("2026-10-15", "bench", Some("bench-v1/A/02"), 51.0, 0.98),
        record("2026-11-07", "bench", Some("bench-v1/A/03"), 52.0, 0.98),
        record("2026-11-13", "words", None, 41.0, 0.90),
        record("2026-11-14", "words", None, 40.0, 0.88),
    ];

    let report = summary::generate(&records, &Config::default(), NaiveDate::from_ymd_opt(2026, 11, 15).unwrap(), Lang::Ja);
    let text = report.to_text();

    assert!(text.contains("今の実力"), "{text}");
    assert!(text.contains("正確さの関門"), "{text}");
    assert!(text.contains("ペーサーを外"), "{text}");
    assert!(text.contains("adaptive"), "{text}");
    assert!(text.contains("a→s"), "{text}");
    assert!(text.contains("参考値"), "{text}");
    assert!(text.contains("8 日"), "{text}");
    assert!(text.contains("そろそろベンチを受けましょう"), "{text}");
}

#[test]
fn english_summary_uses_english_rules_and_labels() {
    let records = (0..5).map(|_| record("2026-11-14", "words", None, 40.0, 0.88)).collect::<Vec<_>>();
    let text = summary::generate(&records, &Config::default(), NaiveDate::from_ymd_opt(2026, 11, 15).unwrap(), Lang::En).to_text();

    assert!(text.contains("Current ability"), "{text}");
    assert!(text.contains("accuracy gate"), "{text}");
    assert!(text.contains("adaptive"), "{text}");
    assert!(!text.contains("今の実力"), "{text}");
}

#[test]
fn empty_summary_says_that_no_records_exist_in_both_languages() {
    let date = NaiveDate::from_ymd_opt(2026, 11, 15).unwrap();
    let config = Config::default();

    assert!(summary::generate(&[], &config, date, Lang::Ja).to_text().contains("まだ記録がありません"));
    assert!(summary::generate(&[], &config, date, Lang::En).to_text().contains("No records yet"));
}

#[test]
fn sparse_transitions_are_not_ranked_as_high_cost_candidates() {
    let records = vec![record("2026-11-14", "words", None, 40.0, 0.98)];
    let details = DetailedMetrics {
        iki_median_ms: 100.0,
        bigrams: vec![
            BigramMetric {
                from: "z".into(),
                to: "z".into(),
                class: TransitionClass::SameFinger,
                n: 1,
                median_ms: 900.0,
                error_n: 0,
                attempts: 1,
                error_rate: 0.0,
                estimated_iki_ms: 500.0,
                estimated_error_rate: 0.01,
                estimated_iki_lo_ms: 150.0,
                estimated_iki_hi_ms: 1_200.0,
                relative_iki: 5.0,
            },
            BigramMetric {
                from: "a".into(),
                to: "s".into(),
                class: TransitionClass::SameHandAdjacent,
                n: 5,
                median_ms: 250.0,
                error_n: 0,
                attempts: 5,
                error_rate: 0.0,
                estimated_iki_ms: 225.0,
                estimated_error_rate: 0.01,
                estimated_iki_lo_ms: 180.0,
                estimated_iki_hi_ms: 280.0,
                relative_iki: 2.25,
            },
        ],
        ..DetailedMetrics::default()
    };
    let config = Config::default();
    let text = summary::generate_with_metrics(
        &records,
        &config,
        &builtin_layout("qwerty").unwrap(),
        &details,
        NaiveDate::from_ymd_opt(2026, 11, 15).unwrap(),
        Lang::Ja,
    )
    .to_text();

    assert!(text.contains("a→s"), "{text}");
    assert!(!text.contains("z→z"), "{text}");
}
