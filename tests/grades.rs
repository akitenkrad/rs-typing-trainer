use chrono::{TimeZone, Utc};
use rs_typing_trainer::analysis::grades::{
    Component, ComponentValue, GradeChangeKind, GradeHistoryPoint, GradeOptions, GradeSource, GradeStatus, Level, analyze_grades, bundled_thresholds,
    component_grades_for_session, detect_grade_changes, evaluate_language, grade_component,
};
use rs_typing_trainer::analysis::metrics::AnalysisSettings;
use rs_typing_trainer::analysis::progress::SessionForm;
use rs_typing_trainer::config::ErrorMode;
use rs_typing_trainer::data::JapaneseText;
use rs_typing_trainer::engine::session::SessionLog;
use rs_typing_trainer::layout::builtin_layout;
use rs_typing_trainer::layout::geometry::geometry;
use rs_typing_trainer::record::{SessionHeader, SessionRecord};

#[test]
fn exact_boundaries_and_endpoints_cover_both_directions() {
    let speed_f = grade_component("en", Component::Speed, 25.0).unwrap();
    assert_eq!(speed_f.grade, Level::F);
    assert!((speed_f.score - 1.0).abs() < 1e-12);
    assert_eq!(grade_component("en", Component::Speed, 24.999).unwrap().grade, Level::G);
    assert_eq!(grade_component("en", Component::Speed, 0.0).unwrap().score, 0.0);
    assert_eq!(grade_component("en", Component::Speed, 200.0).unwrap().score, 8.0);

    let rhythm_f = grade_component("en", Component::Rhythm, 0.72).unwrap();
    assert_eq!(rhythm_f.grade, Level::F);
    assert!((rhythm_f.score - 1.0).abs() < 1e-12);
    assert_eq!(grade_component("en", Component::Rhythm, 0.720_001).unwrap().grade, Level::G);
    assert_eq!(grade_component("en", Component::Rhythm, 1.0).unwrap().score, 0.0);
    assert_eq!(grade_component("en", Component::Rhythm, 0.1).unwrap().score, 8.0);

    let rhythm_c = grade_component("en", Component::Rhythm, 0.45).unwrap();
    assert_eq!(rhythm_c.next_grade, Some(Level::B));
    assert_eq!(rhythm_c.next_threshold, Some(0.42));
    assert!((rhythm_c.gap.unwrap() + 0.03).abs() < 1e-12);
    assert!(grade_component("en", Component::Speed, 130.0).unwrap().next_grade.is_none());
}

#[test]
fn interpolation_is_linear_inside_each_grade() {
    let speed = grade_component("en", Component::Speed, 82.5).unwrap();
    assert_eq!(speed.grade, Level::B);
    assert!((speed.score - 5.5).abs() < 1e-12, "{speed:?}");

    let rhythm = grade_component("en", Component::Rhythm, 0.39).unwrap();
    assert_eq!(rhythm.grade, Level::B);
    assert!((rhythm.score - 5.5).abs() < 1e-12, "{rhythm:?}");
}

#[test]
fn weighted_harmonic_overall_matches_hand_calculation_and_penalizes_weakness() {
    let result = evaluate_language(
        "en",
        &[
            ComponentValue::new(Component::Speed, 100.0),
            ComponentValue::new(Component::Accuracy, 98.0),
            ComponentValue::new(Component::Rhythm, 0.55),
        ],
        GradeSource::Bench,
        GradeStatus::Official,
        3,
    )
    .unwrap();
    let overall = result.overall.unwrap();
    let expected = 5.0 / (2.0 / 7.5 + 2.0 / 7.0 + 1.0 / 4.0) - 1.0;
    assert!((overall.score - expected).abs() < 1e-12, "{overall:?}");
    let arithmetic = (2.0 * 6.5 + 2.0 * 6.0 + 3.0) / 5.0;
    assert!(overall.score < arithmetic);
}

#[test]
fn required_and_optional_missing_components_are_distinguished() {
    let without_speed = evaluate_language(
        "en",
        &[ComponentValue::new(Component::Accuracy, 98.0)],
        GradeSource::Bench,
        GradeStatus::Official,
        3,
    )
    .unwrap();
    assert!(without_speed.overall.is_none());
    assert!(without_speed.missing.contains(&Component::Speed));

    let without_overlap = evaluate_language(
        "en",
        &[
            ComponentValue::new(Component::Speed, 90.0),
            ComponentValue::new(Component::Accuracy, 98.0),
        ],
        GradeSource::Bench,
        GradeStatus::Official,
        3,
    )
    .unwrap();
    assert!(without_overlap.overall.is_some());
    assert!(without_overlap.missing.contains(&Component::Overlap));
}

#[test]
fn japanese_uses_its_own_speed_thresholds() {
    assert_eq!(grade_component("ja", Component::Speed, 95.0).unwrap().grade, Level::S);
    assert_eq!(grade_component("en", Component::Speed, 95.0).unwrap().grade, Level::A);
}

#[test]
fn one_session_component_values_reuse_grade_scoring_for_menu_and_snapshots() {
    let at = Utc.with_ymd_and_hms(2026, 11, 18, 8, 0, 0).unwrap();
    let record = practice_record(at, 0.98);
    let values = component_grades_for_session(
        &record,
        &builtin_layout("qwerty").unwrap(),
        &geometry("ansi").unwrap(),
        GradeOptions {
            model: AnalysisSettings::default(),
        },
        at + chrono::Duration::minutes(2),
        &["ab".to_owned()],
        &[JapaneseText {
            text: "あ".to_owned(),
            reading: "a".to_owned(),
        }],
        "en",
    )
    .unwrap();
    let speed = values.iter().find(|row| row.component == Component::Speed).unwrap();
    assert_eq!(speed.value, 60.0);
    assert_eq!(speed.score, grade_component("en", Component::Speed, 60.0).unwrap().score);
}

#[test]
fn source_selection_distinguishes_official_interim_and_provisional() {
    let start = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
    let layout = builtin_layout("qwerty").unwrap();
    let keyboard = geometry("ansi").unwrap();
    let options = GradeOptions {
        model: AnalysisSettings::default(),
    };
    let english = vec!["ab".to_owned()];
    let japanese = vec![JapaneseText {
        text: "あ".to_owned(),
        reading: "あ".to_owned(),
    }];

    let two = vec![bench_record(start, "A", 60.0), bench_record(start + chrono::Duration::days(1), "A", 80.0)];
    let interim = analyze_grades(
        &two,
        &[],
        &layout,
        &keyboard,
        options,
        start + chrono::Duration::days(2),
        &english,
        &japanese,
    );
    let english_grade = interim.languages.iter().find(|grade| grade.language == "en").unwrap();
    assert_eq!(english_grade.source, GradeSource::Bench);
    assert_eq!(english_grade.status, GradeStatus::Interim);
    assert_eq!(english_grade.n_sessions, 2);

    let mut three = two;
    three.push(bench_record(start + chrono::Duration::days(2), "A", 100.0));
    let official = analyze_grades(
        &three,
        &[],
        &layout,
        &keyboard,
        options,
        start + chrono::Duration::days(3),
        &english,
        &japanese,
    );
    assert_eq!(
        official.languages.iter().find(|grade| grade.language == "en").unwrap().status,
        GradeStatus::Official
    );
    let official_english = official.languages.iter().find(|grade| grade.language == "en").unwrap();
    assert_eq!(
        official_english
            .components
            .iter()
            .find(|row| row.component == Component::Speed)
            .unwrap()
            .value,
        80.0
    );

    let practice = vec![practice_record(start, 0.98), practice_record(start + chrono::Duration::days(1), 0.97)];
    let forms = vec![form(start, 58.0), form(start + chrono::Duration::days(1), 62.0)];
    let provisional = analyze_grades(
        &practice,
        &forms,
        &layout,
        &keyboard,
        options,
        start + chrono::Duration::days(2),
        &english,
        &japanese,
    );
    let english_grade = provisional.languages.iter().find(|grade| grade.language == "en").unwrap();
    assert_eq!(english_grade.source, GradeSource::Provisional);
    assert_eq!(english_grade.status, GradeStatus::Provisional);
    assert_eq!(
        english_grade
            .components
            .iter()
            .find(|row| row.component == Component::Speed)
            .unwrap()
            .value,
        60.0
    );
    assert!(provisional.history.is_empty());

    let mut with_future_practice = practice;
    with_future_practice.push(practice_record(start + chrono::Duration::days(30), 0.50));
    let mut with_future_forms = forms;
    with_future_forms.push(form(start + chrono::Duration::days(30), 200.0));
    let without_future_leak = analyze_grades(
        &with_future_practice,
        &with_future_forms,
        &layout,
        &keyboard,
        options,
        start + chrono::Duration::days(2),
        &english,
        &japanese,
    );
    let english_grade = without_future_leak.languages.iter().find(|grade| grade.language == "en").unwrap();
    assert_eq!(
        english_grade
            .components
            .iter()
            .find(|row| row.component == Component::Speed)
            .unwrap()
            .value,
        60.0
    );
    assert_eq!(
        english_grade
            .components
            .iter()
            .find(|row| row.component == Component::Accuracy)
            .unwrap()
            .value,
        97.5
    );
}

#[test]
fn one_v2_benchmark_makes_the_grade_interim_without_mixing_three_v1_results() {
    let start = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
    let mut records = (0..3)
        .map(|day| bench_record(start + chrono::Duration::days(day), "A", 60.0 + day as f64))
        .collect::<Vec<_>>();
    let mut v2 = bench_record(start + chrono::Duration::days(3), "A", 90.0);
    v2.header.bench_id = Some("bench-v2/A/01".to_owned());
    records.push(v2);
    let grades = analyze_grades(
        &records,
        &[],
        &builtin_layout("qwerty").unwrap(),
        &geometry("ansi").unwrap(),
        GradeOptions {
            model: AnalysisSettings::default(),
        },
        start + chrono::Duration::days(4),
        &["ab".to_owned()],
        &[JapaneseText {
            text: "あ".to_owned(),
            reading: "あ".to_owned(),
        }],
    );
    let english = grades.languages.iter().find(|grade| grade.language == "en").unwrap();
    assert_eq!(english.source, GradeSource::Bench);
    assert_eq!(english.status, GradeStatus::Interim);
    assert_eq!(english.n_sessions, 1);
}

#[test]
fn provisional_speed_falls_back_to_recent_net_wpm_and_prefers_corrected_wpm() {
    let start = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
    let layout = builtin_layout("qwerty").unwrap();
    let keyboard = geometry("ansi").unwrap();
    let options = GradeOptions {
        model: AnalysisSettings::default(),
    };
    let mut practice = vec![
        practice_record(start, 0.98),
        practice_record(start + chrono::Duration::days(1), 0.97),
        practice_record(start + chrono::Duration::days(2), 0.96),
    ];
    practice[0].header.net_wpm = 40.0;
    practice[1].header.net_wpm = 60.0;
    practice[2].header.net_wpm = 80.0;
    let at = start + chrono::Duration::days(3);
    let english = vec!["ab".to_owned()];
    let japanese = vec![JapaneseText {
        text: "あ".to_owned(),
        reading: "あ".to_owned(),
    }];

    let uncorrected = analyze_grades(&practice, &[], &layout, &keyboard, options, at, &english, &japanese);
    let grade = uncorrected.languages.iter().find(|grade| grade.language == "en").unwrap();
    assert_eq!(grade.source.as_str(), "provisional-uncorrected");
    assert!(grade.overall.is_some());
    assert_eq!(grade.components.iter().find(|row| row.component == Component::Speed).unwrap().value, 60.0);

    let corrected = analyze_grades(&practice, &[form(start, 70.0)], &layout, &keyboard, options, at, &english, &japanese);
    let grade = corrected.languages.iter().find(|grade| grade.language == "en").unwrap();
    assert_eq!(grade.source, GradeSource::Provisional);
    assert_eq!(grade.components.iter().find(|row| row.component == Component::Speed).unwrap().value, 70.0);
}

#[test]
fn promotion_demotion_and_best_component_are_reported() {
    let start = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
    let history_rows = vec![
        history(start, Level::C, 4.2),
        history(start + chrono::Days::new(1), Level::B, 5.1),
        history(start + chrono::Days::new(2), Level::C, 4.8),
    ];
    let changes = detect_grade_changes(&history_rows);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].kind, GradeChangeKind::Demotion);
    assert_eq!(changes[0].from, Level::B);
    assert_eq!(changes[0].to, Level::C);

    let promotion = detect_grade_changes(&[history(start, Level::C, 4.2), history(start + chrono::Days::new(1), Level::B, 5.1)]);
    assert_eq!(promotion[0].kind, GradeChangeKind::Promotion);
    assert_eq!(promotion[0].from, Level::C);
    assert_eq!(promotion[0].to, Level::B);

    let result = evaluate_language(
        "en",
        &[
            ComponentValue::new(Component::Speed, 100.0),
            ComponentValue::new(Component::Accuracy, 99.2),
            ComponentValue::new(Component::Rhythm, 0.60),
        ],
        GradeSource::Bench,
        GradeStatus::Official,
        3,
    )
    .unwrap();
    assert_eq!(result.overall.unwrap().easiest_component, Some(Component::Rhythm));
}

#[test]
fn bundled_thresholds_are_complete_monotone_and_versioned() {
    let thresholds = bundled_thresholds().unwrap();
    assert_eq!(thresholds.version, "grades-v1");
    thresholds.validate().unwrap();
    for language in ["en", "ja"] {
        assert_eq!(thresholds.language(language).unwrap().len(), 7);
    }
    let english = thresholds.language("en").unwrap();
    assert_eq!(english["speed"].score_zero, 10.0);
    assert_eq!(english["speed"].score_eight, 130.0);
    assert_eq!(english["speed"].weight, 2.0);
    assert_eq!(english["accuracy"].score_zero, 80.0);
    assert_eq!(english["accuracy"].score_eight, 99.8);
    assert_eq!(english["accuracy"].weight, 2.0);
    assert_eq!(english["overlap"].score_zero, 0.0);
    assert_eq!(english["overlap"].score_eight, 65.0);
    assert_eq!(english["rhythm"].weight, 1.0);
    let japanese = thresholds.language("ja").unwrap();
    assert_eq!(japanese["speed"].score_zero, 8.0);
    assert_eq!(japanese["speed"].score_eight, 115.0);
}

fn history(date: chrono::NaiveDate, grade: Level, score: f64) -> GradeHistoryPoint {
    GradeHistoryPoint {
        date,
        language: "en".to_owned(),
        component: "overall".to_owned(),
        grade,
        score,
        version: "grades-v1".to_owned(),
    }
}

fn bench_record(at: chrono::DateTime<Utc>, part: &str, wpm: f64) -> SessionRecord {
    let mut record = practice_record(at, 0.98);
    record.header.mode = "bench".to_owned();
    record.header.bench_id = Some(format!("bench-v1/{part}/01"));
    record.header.net_wpm = wpm;
    record
}

fn practice_record(at: chrono::DateTime<Utc>, accuracy: f64) -> SessionRecord {
    SessionRecord {
        header: SessionHeader {
            id: None,
            started_at: at.to_rfc3339(),
            finished_at: (at + chrono::Duration::minutes(1)).to_rfc3339(),
            duration_ms: 60_000,
            mode: "words".to_owned(),
            mode_params: r#"{"lang":"en"}"#.to_owned(),
            bench_id: None,
            layout: "qwerty".to_owned(),
            input_layout: "qwerty".to_owned(),
            geometry: "ansi".to_owned(),
            error_mode: "stop".to_owned(),
            release_events: false,
            app_version: "test".to_owned(),
            metrics_version: 2,
            net_wpm: 60.0,
            raw_wpm: Some(60.0),
            accuracy,
            iki_median_ms: 150.0,
            iki_cv: 0.4,
            cps: 5.0,
            consistency: 0.6,
            max_combo: 0,
            completed_words: 0,
            timeout_words: 0,
            mistypes_total: 0,
            legacy_payload: None,
        },
        log: SessionLog {
            duration_us: 60_000_000,
            error_mode: ErrorMode::Stop,
            items: Vec::new(),
            keystrokes: Vec::new(),
        },
    }
}

fn form(at: chrono::DateTime<Utc>, corrected_wpm: f64) -> SessionForm {
    SessionForm {
        session_id: None,
        started_at: at,
        finished_at: at + chrono::Duration::minutes(1),
        mode: "words".to_owned(),
        language: "en".to_owned(),
        net_wpm: corrected_wpm,
        expected_wpm: corrected_wpm,
        form: 1.0,
        corrected_wpm,
        coverage: 1.0,
    }
}
