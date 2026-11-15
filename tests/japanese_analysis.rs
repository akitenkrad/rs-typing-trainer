use chrono::{TimeZone, Utc};
use rs_typing_trainer::analysis::japanese::{SpellingDecision, analyze_japanese};
use rs_typing_trainer::analysis::metrics::AnalysisSettings;
use rs_typing_trainer::analysis::{DiagnosisOptions, diagnose};
use rs_typing_trainer::config::ErrorMode;
use rs_typing_trainer::data::JapaneseText;
use rs_typing_trainer::engine::session::{ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};
use rs_typing_trainer::layout::builtin_layout;
use rs_typing_trainer::layout::geometry::geometry;
use rs_typing_trainer::record::{SessionHeader, SessionRecord};

type Key = (u64, u32, char, char, bool);

fn record(lang: &str, items: Vec<(&str, Option<&str>, Vec<Key>)>) -> SessionRecord {
    let layout = builtin_layout("qwerty").unwrap();
    let mut item_logs = Vec::new();
    let mut keystrokes = Vec::new();
    for (item_idx, (text, reading, keys)) in items.into_iter().enumerate() {
        item_logs.push(ItemLog {
            idx: item_idx as u32,
            text: text.into(),
            reading: reading.map(str::to_owned),
            kind: None,
            shown_us: keys.first().map_or(0, |key| key.0.saturating_sub(50_000)),
            done_us: keys.last().map(|key| key.0),
            outcome: ItemOutcome::Completed,
        });
        for (t_us, pos, typed, expected, correct) in keys {
            keystrokes.push(KeystrokeLog {
                seq: keystrokes.len() as u32,
                t_us,
                release_us: None,
                item_idx: item_idx as u32,
                pos,
                kind: KeystrokeKind::Char,
                typed: Some(typed.to_string()),
                expected: Some(expected.to_string()),
                correct,
                code: layout.stroke_for(typed).map(|stroke| stroke.code),
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
            mode: if lang == "ja" { "romaji" } else { "words" }.into(),
            mode_params: serde_json::json!({"lang": lang}).to_string(),
            bench_id: None,
            layout: "qwerty".into(),
            input_layout: "same".into(),
            geometry: "ansi".into(),
            error_mode: "stop".into(),
            release_events: false,
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
            mistypes_total: keystrokes.iter().filter(|key| !key.correct).count() as u32,
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

fn spelling_record(reading: &'static str, spelling: &'static str, repetitions: usize) -> SessionRecord {
    let mut items = Vec::new();
    let mut clock = 100_000_u64;
    for _ in 0..repetitions {
        let keys = spelling
            .chars()
            .enumerate()
            .map(|(position, character)| (clock + position as u64 * 100_000, position as u32, character, character, true))
            .collect();
        items.push((reading, Some(reading), keys));
        clock += 1_000_000;
    }
    record("ja", items)
}

fn pair_record(from: char, to: char, interval_ms: u64, repetitions: usize, lang: &'static str) -> SessionRecord {
    let mut items = Vec::new();
    let mut clock = 100_000_u64;
    for _ in 0..repetitions {
        items.push((
            "pair",
            None,
            vec![(clock, 0, from, from, true), (clock + interval_ms * 1_000, 1, to, to, true)],
        ));
        clock += 1_000_000;
    }
    record(lang, items)
}

fn analyze(
    records: &[SessionRecord],
    corpus: &[JapaneseText],
    min_n: u64,
    enabled: bool,
) -> rs_typing_trainer::analysis::japanese::JapaneseDiagnosis {
    analyze_japanese(
        records,
        &builtin_layout("qwerty").unwrap(),
        &geometry("ansi").unwrap(),
        AnalysisSettings::default(),
        Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap(),
        corpus,
        min_n,
        enabled,
        300.0,
    )
}

#[test]
fn kana_duration_uses_mora_endpoints_and_assigns_errors_to_the_segment() {
    let sample = record(
        "ja",
        vec![(
            "しつ",
            Some("しつ"),
            vec![
                (100_000, 0, 's', 's', true),
                (150_000, 1, 'x', 'i', false),
                (200_000, 1, 'i', 'i', true),
                (350_000, 2, 't', 't', true),
                (450_000, 3, 'u', 'u', true),
            ],
        )],
    );
    let result = analyze(&[sample], &[], 5, true);
    let shi = result.kana.iter().find(|row| row.kana == "し").unwrap();
    let tsu = result.kana.iter().find(|row| row.kana == "つ").unwrap();

    assert_eq!(shi.n, 1);
    assert_eq!(shi.median_ms, None, "the leading mora has no preceding endpoint");
    assert!((shi.error_rate - 1.0 / 3.0).abs() < 1e-9, "{shi:?}");
    assert_eq!(tsu.median_ms, Some(250.0));
    assert_eq!(tsu.error_rate, 0.0);
}

#[test]
fn kana_duration_excludes_intervals_over_the_pause_threshold() {
    let sample = record(
        "ja",
        vec![(
            "しつふ",
            Some("しつふ"),
            vec![
                (100_000, 0, 's', 's', true),
                (200_000, 1, 'i', 'i', true),
                (2_100_000, 2, 't', 't', true),
                (2_200_000, 3, 'u', 'u', true),
                (4_101_000, 4, 'f', 'f', true),
                (4_201_000, 5, 'u', 'u', true),
            ],
        )],
    );
    let result = analyze(&[sample], &[], 5, true);
    let shi = result.kana.iter().find(|row| row.kana == "し").unwrap();
    let tsu = result.kana.iter().find(|row| row.kana == "つ").unwrap();
    let fu = result.kana.iter().find(|row| row.kana == "ふ").unwrap();

    assert_eq!((shi.n, shi.median_ms), (1, None));
    assert_eq!((tsu.n, tsu.median_ms), (1, Some(2_000.0)));
    assert_eq!((fu.n, fu.median_ms), (1, None));
}

fn spelling_fixture(si_ms: u64, sh_ms: u64, hi_ms: u64, transition_n: usize) -> (Vec<SessionRecord>, Vec<JapaneseText>) {
    let records = vec![
        spelling_record("し", "shi", 3),
        spelling_record("し", "si", 1),
        pair_record('s', 'i', si_ms, transition_n, "en"),
        pair_record('s', 'h', sh_ms, transition_n, "en"),
        pair_record('h', 'i', hi_ms, transition_n, "en"),
    ];
    let corpus = vec![JapaneseText {
        text: "詩".into(),
        reading: "し".into(),
    }];
    (records, corpus)
}

#[test]
fn clearly_faster_supported_si_is_suggested_over_current_shi() {
    let (records, corpus) = spelling_fixture(50, 100, 100, 20);
    let result = analyze(&records, &corpus, 5, true);
    let suggestion = result.spelling.iter().find(|row| row.kana == "し" && row.candidate == "si").unwrap();

    assert_eq!(suggestion.current, "shi");
    assert_eq!(suggestion.decision, SpellingDecision::Suggested);
    assert!(suggestion.savings_ms >= 15.0, "{suggestion:?}");
    assert!(
        suggestion.candidate_expected_ms <= suggestion.current_expected_ms * 0.92,
        "{suggestion:?}"
    );
}

#[test]
fn small_spelling_difference_is_not_suggested() {
    let (records, corpus) = spelling_fixture(90, 45, 45, 20);
    let result = analyze(&records, &corpus, 5, true);
    assert!(
        result.spelling.iter().all(|row| row.decision != SpellingDecision::Suggested),
        "{:#?}",
        result.spelling
    );
}

#[test]
fn insufficient_transition_evidence_marks_spelling_as_reference() {
    let (records, corpus) = spelling_fixture(50, 100, 100, 2);
    let result = analyze(&records, &corpus, 5, true);
    assert!(
        result
            .spelling
            .iter()
            .any(|row| row.candidate == "si" && row.decision == SpellingDecision::Reference)
    );
}

#[test]
fn spelling_suggestions_can_be_disabled() {
    let (records, corpus) = spelling_fixture(50, 100, 100, 20);
    assert!(analyze(&records, &corpus, 5, false).spelling.is_empty());
}

#[test]
fn synthetic_small_kana_spellings_are_candidates_only_after_the_player_uses_them() {
    let corpus = vec![JapaneseText {
        text: "客".into(),
        reading: "きゃ".into(),
    }];
    let ordinary = analyze(&[spelling_record("きゃ", "kya", 3)], &corpus, 5, true);
    assert!(
        ordinary.spelling.iter().all(|row| !matches!(row.candidate.as_str(), "kilya" | "kixya")),
        "{:#?}",
        ordinary.spelling
    );

    let observed = analyze(
        &[spelling_record("きゃ", "kya", 3), spelling_record("きゃ", "kilya", 1)],
        &corpus,
        5,
        true,
    );
    assert!(observed.spelling.iter().any(|row| row.candidate == "kilya"));
}

#[test]
fn spelling_rows_require_an_observed_non_small_mora() {
    let records = vec![spelling_record("し", "shi", 1), spelling_record("ぁ", "la", 1)];
    let corpus = vec![
        JapaneseText {
            text: "詩".into(),
            reading: "し".into(),
        },
        JapaneseText {
            text: "血".into(),
            reading: "ち".into(),
        },
        JapaneseText {
            text: "小書き".into(),
            reading: "ぁ".into(),
        },
    ];
    let result = analyze(&records, &corpus, 5, true);

    assert!(!result.spelling.is_empty());
    assert!(result.spelling.iter().all(|row| row.kana == "し"), "{:#?}", result.spelling);
}

#[test]
fn observed_but_unlisted_ordinary_alternatives_are_not_candidates() {
    let records = vec![spelling_record("せ", "se", 3), spelling_record("せ", "ce", 1)];
    let corpus = vec![JapaneseText {
        text: "背".into(),
        reading: "せ".into(),
    }];

    assert!(analyze(&records, &corpus, 5, true).spelling.is_empty());
}

#[test]
fn ku_suggestions_use_only_the_requested_k_and_c_spellings() {
    let records = vec![spelling_record("く", "ku", 1)];
    let corpus = vec![JapaneseText {
        text: "句".into(),
        reading: "く".into(),
    }];
    let result = analyze(&records, &corpus, 5, true);

    assert!(result.spelling.iter().any(|row| row.candidate == "cu"));
    assert!(result.spelling.iter().all(|row| row.candidate != "qu"), "{:#?}", result.spelling);
}

#[test]
fn japanese_to_english_transition_ratio_requires_evidence_in_both_languages() {
    let records = vec![pair_record('a', 's', 100, 30, "en"), pair_record('a', 's', 200, 30, "ja")];
    let result = analyze(&records, &[], 5, true);
    let comparison = result.language_comparison;
    let row = comparison.transitions.iter().find(|row| row.from == "a" && row.to == "s").unwrap();

    assert!((1.8..=2.2).contains(&row.ratio), "{row:?}");
    assert!((1.8..=2.2).contains(&comparison.median_ratio.unwrap()));
}

#[test]
fn public_diagnosis_collects_error_style_and_japanese_results() {
    let (records, corpus) = spelling_fixture(50, 100, 100, 20);
    let result = diagnose(
        &records,
        &builtin_layout("qwerty").unwrap(),
        &geometry("ansi").unwrap(),
        DiagnosisOptions {
            model: AnalysisSettings::default(),
            min_transition_n: 5,
            spelling_suggestions: true,
        },
        Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap(),
        &corpus,
    );

    assert!(!result.japanese.kana.is_empty());
    assert!(result.japanese.spelling.iter().any(|row| row.decision == SpellingDecision::Suggested));
    assert!(!result.player_model.transitions.is_empty());
}
