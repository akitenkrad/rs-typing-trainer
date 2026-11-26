use chrono::{NaiveDate, TimeZone, Utc};
use rs_typing_trainer::analysis::metrics::AnalysisSettings;
use rs_typing_trainer::analysis::model::{ExpectedPrompt, TransitionEstimate, session_language};
use rs_typing_trainer::analysis::progress::{
    ChangePointKind, CurveKind, DailyTrendSource, OperatingPointKind, ProgressOptions, ReferencePrompts, analyze_progress, classify_operating_point,
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
fn bench_v3_builds_the_same_difficulty_correction_form_as_v2() {
    let start = Utc.with_ymd_and_hms(2026, 11, 20, 12, 0, 0).unwrap();
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
        english: vec![ExpectedPrompt::english("zxy")],
        japanese: Vec::new(),
    };
    let training = synthetic_record(1, start, &ascii_training(300), 1.0);
    let mut v2 = synthetic_record(2, start + chrono::Duration::days(1), "abx", 1.0);
    v2.header.mode = "bench".into();
    v2.header.bench_id = Some("bench-v2/A/01".into());
    let mut v3 = synthetic_record(3, start + chrono::Duration::days(2), "abx", 1.0);
    v3.header.mode = "bench".into();
    v3.header.bench_id = Some("bench-v3/EW/01".into());

    let progress = analyze_progress(
        &[training, v2, v3],
        &layout,
        &keyboard,
        options,
        start + chrono::Duration::days(3),
        &references,
    );

    assert!(
        progress.forms.iter().any(|form| form.session_id == Some(2)),
        "v2 must keep producing a form"
    );
    assert!(
        progress.forms.iter().any(|form| form.session_id == Some(3)),
        "v3 must follow the same difficulty-correction rule as v2: {:?}",
        progress.forms
    );
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

/// MYTASK-3665: a benchmark taken while the practice language was Japanese
/// recorded `"lang": "ja"`; its part, not that field, decides its language.
#[test]
fn session_language_follows_the_benchmark_part_over_the_recorded_lang() {
    let at = Utc.with_ymd_and_hms(2026, 11, 20, 1, 0, 0).unwrap();
    for (bench_id, lang, expected) in [
        ("bench-v2/A/04", "ja", "en"),
        ("bench-v2/B/04", "ja", "en"),
        ("bench-v2/D/04", "ja", "en"),
        ("bench-v2/C/04", "en", "ja"),
    ] {
        let mut record = synthetic_record(40, at, "as", 1.0);
        record.header.mode = "bench".to_owned();
        record.header.bench_id = Some(bench_id.to_owned());
        record.header.mode_params = serde_json::json!({ "lang": lang }).to_string();
        assert_eq!(session_language(&record), expected, "{bench_id} recorded with lang {lang}");
    }
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

fn v3_record(id: i64, at: chrono::DateTime<Utc>, part: &str, form: u8, effective_wpm: f64) -> SessionRecord {
    let speed_scale = 100.0 / effective_wpm;
    let mut record = synthetic_record(id, at, "aaaa", speed_scale);
    record.header.mode = "bench".to_owned();
    record.header.bench_id = Some(format!("bench-v3/{part}/{form:02}"));
    record
}

#[test]
fn v3_daily_trend_has_language_priority_and_uses_same_day_run_median() {
    let at = |day, hour| {
        chrono::Local
            .with_ymd_and_hms(2026, 11, day, hour, 0, 0)
            .single()
            .unwrap()
            .with_timezone(&Utc)
    };
    let as_of = at(20, 12);
    let mut old_en = synthetic_record(100, at(1, 12), "aaaa", 1.0);
    old_en.header.mode = "bench".into();
    old_en.header.bench_id = Some("bench-v2/A/01".into());
    old_en.header.net_wpm = 999.0;
    let mut old_ja = synthetic_record(101, at(1, 13), "aaaa", 1.0);
    old_ja.header.mode = "bench".into();
    old_ja.header.mode_params = serde_json::json!({"lang": "ja"}).to_string();
    old_ja.header.bench_id = Some("bench-v2/C/01".into());
    old_ja.header.net_wpm = 42.0;
    let records = vec![
        old_en,
        old_ja,
        v3_record(110, at(8, 9), "EW", 1, 60.0),
        v3_record(111, at(8, 9), "ES", 1, 60.0),
        v3_record(112, at(8, 18), "EW", 2, 90.0),
        v3_record(113, at(8, 18), "ES", 2, 90.0),
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

    let en = result.trends.iter().find(|trend| trend.target == "en").unwrap();
    assert_eq!(en.source, DailyTrendSource::BenchV3);
    assert_eq!(en.speed.len(), 1);
    assert!((en.speed[0].value - 75.0).abs() < 0.01, "{en:?}");
    let v3 = result.bench_v3_trends.iter().find(|trend| trend.target == "en").unwrap();
    assert_eq!(v3.runs.len(), 2);
    assert!((v3.runs[0].accuracy - 1.0).abs() < 1e-12);
    assert_eq!(v3.mainline.len(), 1);
    assert!((v3.mainline[0].value - 67.5).abs() < 0.01, "{v3:?}");
    assert!(en.speed.iter().all(|point| point.value != 999.0));

    let ja = result.trends.iter().find(|trend| trend.target == "ja").unwrap();
    assert_eq!(ja.source, DailyTrendSource::Bench);
    assert_eq!(ja.speed.iter().map(|point| point.value).collect::<Vec<_>>(), [42.0]);
}

#[test]
fn v3_rolling_three_mainline_covers_one_two_three_and_four_runs_and_drives_slope() {
    let as_of = Utc.with_ymd_and_hms(2026, 11, 22, 12, 0, 0).unwrap();
    let mut records = Vec::new();
    for (run, (day, wpm)) in [(1, (1, 10.0)), (2, (8, 20.0)), (3, (15, 100.0)), (4, (22, 40.0))] {
        let at = Utc.with_ymd_and_hms(2026, 11, day, 12, 0, 0).unwrap();
        records.push(v3_record(200 + run * 2, at, "EW", run as u8, wpm));
        records.push(v3_record(201 + run * 2, at, "ES", run as u8, wpm));
    }
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
    let v3 = result.bench_v3_trends.iter().find(|trend| trend.target == "en").unwrap();
    let raw = v3.runs.iter().map(|point| point.value).collect::<Vec<_>>();
    let mainline = v3.mainline.iter().map(|point| point.value).collect::<Vec<_>>();
    assert_eq!(raw.len(), 4);
    for (actual, expected) in raw.iter().zip([10.0, 20.0, 100.0, 40.0]) {
        assert!((*actual - expected).abs() < 0.01, "{raw:?}");
    }
    for (actual, expected) in mainline.iter().zip([10.0, 15.0, 20.0, 40.0]) {
        assert!((*actual - expected).abs() < 0.01, "{mainline:?}");
    }
    assert!(trend.speed_slope_per_week.is_some_and(|slope| slope > 0.0), "{trend:?}");
}

#[test]
fn v3_transition_part_neither_selects_the_english_v3_series_nor_leaks_into_fallback_accuracy() {
    let as_of = Utc.with_ymd_and_hms(2026, 11, 10, 12, 0, 0).unwrap();
    let mut practice = synthetic_record(300, Utc.with_ymd_and_hms(2026, 11, 1, 12, 0, 0).unwrap(), "aaaa", 1.0);
    practice.header.accuracy = 0.8;
    let mut transition = v3_record(301, Utc.with_ymd_and_hms(2026, 11, 2, 12, 0, 0).unwrap(), "PW", 1, 50.0);
    transition.header.accuracy = 0.1;
    let result = analyze_progress(
        &[practice, transition],
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
    assert_eq!(en.source, DailyTrendSource::CorrectedWpm);
    assert_eq!(en.accuracy.len(), 1, "{en:?}");
    assert!((en.accuracy[0].value - 0.8).abs() < 1e-12, "{en:?}");
    assert!(result.bench_v3_trends.is_empty());
}
