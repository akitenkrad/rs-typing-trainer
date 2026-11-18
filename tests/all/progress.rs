use chrono::{NaiveDate, TimeZone, Utc};
use rs_typing_trainer::analysis::metrics::AnalysisSettings;
use rs_typing_trainer::analysis::model::{ExpectedPrompt, TransitionEstimate, session_language};
use rs_typing_trainer::analysis::progress::{
    ChangePointKind, CurveKind, OperatingPointKind, ProgressOptions, ReferencePrompts, analyze_progress, classify_operating_point,
    detect_changepoints, find_transition_regressions, fit_learning_curve,
};
use rs_typing_trainer::config::ErrorMode;
use rs_typing_trainer::engine::session::{ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};
use rs_typing_trainer::layout::builtin_layout;
use rs_typing_trainer::layout::geometry::geometry;
use rs_typing_trainer::record::{SessionHeader, SessionRecord};

#[test]
fn exponential_curve_is_selected_and_recovers_asymptote() {
    let points = (0..12)
        .map(|index| {
            let minutes = index as f64 * 20.0;
            (minutes, 72.0 - 30.0 * (-0.018 * minutes).exp())
        })
        .collect::<Vec<_>>();
    let curve = fit_learning_curve("A", &points, 20.0).expect("six or more points are estimable");
    assert_eq!(curve.model, CurveKind::Exponential);
    assert!((curve.a - 72.0).abs() <= 7.2, "{curve:?}");
    assert_eq!(curve.forecast_days, 28);
    assert!(curve.band_lo <= curve.forecast && curve.forecast <= curve.band_hi);
}

#[test]
fn curve_requires_six_points() {
    assert!(fit_learning_curve("A", &[(0.0, 40.0); 5], 10.0).is_none());
}

#[test]
fn changepoint_finds_five_wpm_gain_without_false_positive() {
    let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
    let changed = (0..40)
        .map(|day| (start + chrono::Days::new(day), if day < 20 { 50.0 } else { 55.0 }))
        .collect::<Vec<_>>();
    let points = detect_changepoints("A", &changed);
    let gain = points.iter().find(|point| point.kind == ChangePointKind::SuddenGain).expect("gain");
    assert!((18..=22).contains(&(gain.date - start).num_days()), "{points:?}");
    assert!(gain.probability > 0.5);

    let unchanged = (0..40).map(|day| (start + chrono::Days::new(day), 50.0)).collect::<Vec<_>>();
    assert!(detect_changepoints("A", &unchanged).is_empty());
}

#[test]
fn operating_point_uses_documented_thresholds() {
    let start = NaiveDate::from_ymd_opt(2026, 2, 1).unwrap();
    let cautious = [(start, 60.0, 0.97), (start + chrono::Days::new(1), 40.0, 0.99)];
    assert_eq!(classify_operating_point(&cautious).unwrap().kind, OperatingPointKind::TooCautious);
    let aggressive = [(start, 40.0, 0.98), (start + chrono::Days::new(1), 60.0, 0.93)];
    assert_eq!(classify_operating_point(&aggressive).unwrap().kind, OperatingPointKind::TooAggressive);
}

#[test]
fn chronological_form_corrects_prompt_difficulty_and_never_uses_future_records() {
    let start = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
    let training = synthetic_record(1, start, &"abac".repeat(80), 1.0);
    let easy = synthetic_record(2, start + chrono::Duration::days(1), &"ab".repeat(20), 1.0);
    let hard = synthetic_record(3, start + chrono::Duration::days(2), &"ac".repeat(20), 1.0);
    let improved = synthetic_record(4, start + chrono::Duration::days(3), &"abac".repeat(20), 1.0 / 1.1);
    let records = vec![training, easy, hard];
    let references = ReferencePrompts {
        english: vec![ExpectedPrompt::english(format!("{}zx", "abac".repeat(20)))],
        japanese: Vec::new(),
    };
    let layout = builtin_layout("qwerty").unwrap();
    let keyboard = geometry("ansi").unwrap();
    let options = ProgressOptions {
        model: AnalysisSettings {
            pause_ms: 2_000,
            shrinkage_k: 10.0,
            half_life_days: 14.0,
        },
        min_transition_n: 5,
    };
    let before = analyze_progress(&records, &layout, &keyboard, options, start + chrono::Duration::days(4), &references);
    let easy_form = before.forms.iter().find(|row| row.session_id == Some(2)).unwrap();
    let hard_form = before.forms.iter().find(|row| row.session_id == Some(3)).unwrap();
    assert_ne!(easy_form.net_wpm.round(), hard_form.net_wpm.round());
    assert!((easy_form.form - 1.0).abs() <= 0.05, "{easy_form:?}");
    assert!((hard_form.form - 1.0).abs() <= 0.05, "{hard_form:?}");
    assert!(easy_form.corrected_wpm.is_finite());

    let mut with_future = records;
    with_future.push(improved);
    let after = analyze_progress(&with_future, &layout, &keyboard, options, start + chrono::Duration::days(5), &references);
    let unchanged = after.forms.iter().find(|row| row.session_id == Some(2)).unwrap();
    assert!((unchanged.form - easy_form.form).abs() < 1e-12);
    let improved_form = after.forms.iter().find(|row| row.session_id == Some(4)).unwrap();
    assert!((improved_form.form - 1.1).abs() <= 0.05, "{improved_form:?}");
}

#[test]
fn form_requires_three_hundred_prior_keys_and_half_transition_coverage() {
    let start = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
    let layout = builtin_layout("qwerty").unwrap();
    let keyboard = geometry("ansi").unwrap();
    let options = ProgressOptions {
        model: AnalysisSettings {
            pause_ms: 2_000,
            shrinkage_k: 10.0,
            half_life_days: 14.0,
        },
        min_transition_n: 5,
    };
    let references = ReferencePrompts {
        // Neither transition has been observed; the reference must still use
        // hierarchical parents instead of suppressing corrected WPM.
        english: vec![ExpectedPrompt::english("zxy")],
        japanese: Vec::new(),
    };

    let training_300 = synthetic_record(1, start, &ascii_training(300), 1.0);
    let half_covered = synthetic_record(2, start + chrono::Duration::days(1), "abx", 1.0);
    let result = analyze_progress(
        &[training_300.clone(), half_covered],
        &layout,
        &keyboard,
        options,
        start + chrono::Duration::days(2),
        &references,
    );
    let form = result
        .forms
        .iter()
        .find(|row| row.session_id == Some(2))
        .expect("coverage at the threshold");
    assert!((form.coverage - 0.5).abs() < 1e-12, "{form:?}");
    assert!(form.corrected_wpm.is_finite(), "{form:?}");

    let low_coverage = synthetic_record(3, start + chrono::Duration::days(1), "abxyz", 1.0);
    let result = analyze_progress(
        &[training_300, low_coverage],
        &layout,
        &keyboard,
        options,
        start + chrono::Duration::days(2),
        &references,
    );
    assert!(result.forms.iter().all(|row| row.session_id != Some(3)));

    for prior_keys in [299, 300] {
        let training = synthetic_record(10, start, &ascii_training(prior_keys), 1.0);
        let covered = synthetic_record(11, start + chrono::Duration::days(1), "ab", 1.0);
        let result = analyze_progress(
            &[training, covered],
            &layout,
            &keyboard,
            options,
            start + chrono::Duration::days(2),
            &references,
        );
        assert_eq!(
            result.forms.iter().any(|row| row.session_id == Some(11)),
            prior_keys == 300,
            "prior_keys={prior_keys}: {:?}",
            result.forms
        );
    }
}

#[test]
fn transition_regression_requires_fifteen_percent_and_enough_data() {
    let best = transition("a", "b", 100.0, 10.0);
    let recent = transition("a", "b", 116.0, 10.0);
    let sparse = transition("b", "c", 200.0, 2.0);
    let rows = find_transition_regressions(&[best, sparse.clone()], &[recent, sparse], 5);
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].from.as_str(), rows[0].to.as_str()), ("a", "b"));
}

#[test]
fn four_week_trend_uses_as_of_window_and_requires_records_in_two_weeks() {
    let as_of = Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap();
    let mut old_en_1 = synthetic_record(20, Utc.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).unwrap(), "as", 1.0);
    old_en_1.header.accuracy = 0.99;
    let mut old_en_2 = synthetic_record(21, Utc.with_ymd_and_hms(2026, 5, 15, 12, 0, 0).unwrap(), "as", 1.0);
    old_en_2.header.accuracy = 0.90;
    let mut recent_ja_1 = synthetic_record(22, Utc.with_ymd_and_hms(2026, 11, 2, 12, 0, 0).unwrap(), "as", 1.0);
    recent_ja_1.header.mode = "romaji".to_owned();
    recent_ja_1.header.mode_params = serde_json::json!({"lang": "ja"}).to_string();
    recent_ja_1.header.accuracy = 0.95;
    let mut recent_ja_2 = synthetic_record(23, Utc.with_ymd_and_hms(2026, 11, 10, 12, 0, 0).unwrap(), "as", 1.0);
    recent_ja_2.header.mode = "romaji".to_owned();
    recent_ja_2.header.mode_params = serde_json::json!({"lang": "ja"}).to_string();
    recent_ja_2.header.accuracy = 0.97;
    let records = [old_en_1, old_en_2, recent_ja_1, recent_ja_2];
    let result = analyze_progress(
        &records,
        &builtin_layout("qwerty").unwrap(),
        &geometry("ansi").unwrap(),
        ProgressOptions {
            model: AnalysisSettings::default(),
            min_transition_n: 5,
        },
        as_of,
        &ReferencePrompts::default(),
    );
    let en = result.trends.iter().find(|trend| trend.target == "en").unwrap();
    let ja = result.trends.iter().find(|trend| trend.target == "ja").unwrap();
    assert_eq!(en.speed_slope_per_week, None, "old English speed leaked into recent trend: {en:?}");
    assert_eq!(en.accuracy_slope_per_week, None, "old English accuracy leaked into recent trend: {en:?}");
    assert!(ja.accuracy_slope_per_week.is_some(), "{ja:?}");

    let mut same_week_1 = synthetic_record(24, Utc.with_ymd_and_hms(2026, 11, 9, 12, 0, 0).unwrap(), "as", 1.0);
    same_week_1.header.accuracy = 0.95;
    let mut same_week_2 = synthetic_record(25, Utc.with_ymd_and_hms(2026, 11, 10, 12, 0, 0).unwrap(), "as", 1.0);
    same_week_2.header.accuracy = 0.97;
    let same_week = analyze_progress(
        &[same_week_1, same_week_2],
        &builtin_layout("qwerty").unwrap(),
        &geometry("ansi").unwrap(),
        ProgressOptions {
            model: AnalysisSettings::default(),
            min_transition_n: 5,
        },
        as_of,
        &ReferencePrompts::default(),
    );
    let en = same_week.trends.iter().find(|trend| trend.target == "en").unwrap();
    assert_eq!(en.accuracy_slope_per_week, None, "one calendar week is not enough: {en:?}");
}

#[test]
fn four_week_benchmark_trend_uses_only_the_preferred_series() {
    let as_of = Utc.with_ymd_and_hms(2026, 11, 16, 12, 0, 0).unwrap();
    let make_bench = |id, day, bench_id: &str, wpm| {
        let mut record = synthetic_record(id, Utc.with_ymd_and_hms(2026, 11, day, 12, 0, 0).unwrap(), "as", 1.0);
        record.header.mode = "bench".into();
        record.header.bench_id = Some(bench_id.into());
        record.header.net_wpm = wpm;
        record
    };
    let records = [
        make_bench(40, 2, "bench-v1/A/01", 100.0),
        make_bench(41, 9, "bench-v1/A/02", 50.0),
        make_bench(42, 1, "bench-v2/A/01", 10.0),
        make_bench(43, 15, "bench-v2/A/02", 20.0),
    ];
    let result = analyze_progress(
        &records,
        &builtin_layout("qwerty").unwrap(),
        &geometry("ansi").unwrap(),
        ProgressOptions {
            model: AnalysisSettings::default(),
            min_transition_n: 5,
        },
        as_of,
        &ReferencePrompts::default(),
    );
    let trend = result.trends.iter().find(|trend| trend.target == "en").unwrap();
    assert_eq!(trend.speed.iter().map(|point| point.value).collect::<Vec<_>>(), [10.0, 20.0]);
    assert!(trend.speed_slope_per_week.is_some_and(|slope| slope > 0.0), "{trend:?}");
}

#[test]
fn session_language_recognizes_mode_sentence_setting_and_benchmark_c() {
    let at = Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap();
    let mut romaji = synthetic_record(30, at, "as", 1.0);
    romaji.header.mode = "romaji".to_owned();
    romaji.header.mode_params = "{}".to_owned();
    assert_eq!(session_language(&romaji), "ja");

    let mut sentence = synthetic_record(31, at, "as", 1.0);
    sentence.header.mode = "sentence".to_owned();
    sentence.header.mode_params = serde_json::json!({"lang": "ja"}).to_string();
    assert_eq!(session_language(&sentence), "ja");

    let mut bench_c = synthetic_record(32, at, "as", 1.0);
    bench_c.header.bench_id = Some("bench-v1/C/01".to_owned());
    bench_c.header.mode_params = "{}".to_owned();
    assert_eq!(session_language(&bench_c), "ja");
}

fn ascii_training(length: usize) -> String {
    "abac".chars().cycle().take(length).collect()
}

fn transition(from: &str, to: &str, ms: f64, effective_n: f64) -> TransitionEstimate {
    TransitionEstimate {
        from: from.to_owned(),
        to: to.to_owned(),
        from_hand: "left".to_owned(),
        to_hand: "left".to_owned(),
        from_finger: "left-pinky".to_owned(),
        to_finger: "left-ring".to_owned(),
        n: effective_n as u64,
        effective_n,
        attempts: effective_n as u64,
        effective_attempts: effective_n,
        sample_iki_ms: ms,
        parent_iki_ms: ms,
        estimated_iki_ms: ms,
        estimated_iki_lo_ms: ms,
        estimated_iki_hi_ms: ms,
        estimated_error_rate: 0.0,
        relative_iki: 1.0,
    }
}

fn synthetic_record(id: i64, at: chrono::DateTime<Utc>, text: &str, speed_scale: f64) -> SessionRecord {
    let chars = text.chars().collect::<Vec<_>>();
    let mut time_us = 100_000_u64;
    let mut keys = Vec::new();
    for (index, character) in chars.iter().enumerate() {
        if index > 0 {
            let base_ms = match (chars[index - 1], *character) {
                ('a', 'c') | ('c', 'a') => 240.0,
                _ => 120.0,
            };
            time_us += (base_ms * speed_scale * 1_000.0) as u64;
        }
        keys.push(KeystrokeLog {
            seq: index as u32,
            t_us: time_us,
            release_us: None,
            item_idx: 0,
            pos: index as u32,
            kind: KeystrokeKind::Char,
            typed: Some(character.to_string()),
            expected: Some(character.to_string()),
            correct: true,
            code: None,
        });
    }
    let transition_ms = (time_us - 100_000) as f64 / 1_000.0;
    let net_wpm = chars.len() as f64 / 5.0 / (transition_ms / 60_000.0);
    SessionRecord {
        header: SessionHeader {
            id: Some(id),
            started_at: at.to_rfc3339(),
            finished_at: (at + chrono::Duration::milliseconds(time_us as i64 / 1_000)).to_rfc3339(),
            duration_ms: time_us as i64 / 1_000,
            mode: "words".to_owned(),
            mode_params: serde_json::json!({"lang": "en"}).to_string(),
            bench_id: None,
            layout: "qwerty".to_owned(),
            input_layout: "same".to_owned(),
            geometry: "ansi".to_owned(),
            error_mode: "stop".to_owned(),
            release_events: false,
            app_version: "test".to_owned(),
            metrics_version: 2,
            net_wpm,
            raw_wpm: Some(net_wpm),
            accuracy: 1.0,
            iki_median_ms: 120.0,
            iki_cv: 0.0,
            cps: 0.0,
            consistency: 1.0,
            max_combo: chars.len() as u32,
            completed_words: 1,
            timeout_words: 0,
            mistypes_total: 0,
            legacy_payload: None,
        },
        log: SessionLog {
            duration_us: time_us,
            error_mode: ErrorMode::Stop,
            items: vec![ItemLog {
                idx: 0,
                text: text.to_owned(),
                reading: None,
                kind: None,
                shown_us: 0,
                done_us: Some(time_us),
                outcome: ItemOutcome::Completed,
            }],
            keystrokes: keys,
        },
    }
}
