use chrono::{Local, TimeZone, Utc};
use rs_typing_trainer::analysis::grades::{
    Component, ComponentValue, GradeChangeKind, GradeHistoryPoint, GradeOptions, GradeSource, GradeStatus, Level, analyze_grades, bundled_thresholds,
    bundled_thresholds_v2, component_grades_for_session, detect_grade_changes, evaluate_language, evaluate_language_with_version, grade_component,
    grade_component_with_version, language_bench_records,
};
use rs_typing_trainer::analysis::metrics::AnalysisSettings;
use rs_typing_trainer::analysis::progress::SessionForm;
use rs_typing_trainer::config::ErrorMode;
use rs_typing_trainer::data::JapaneseText;
use rs_typing_trainer::engine::session::{ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};
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

#[test]
fn grades_v2_thresholds_match_the_accuracy_adjusted_speed_table_and_weights() {
    let v1 = bundled_thresholds().unwrap();
    let thresholds = bundled_thresholds_v2().unwrap();
    assert_eq!(thresholds.version, "grades-v2");
    thresholds.validate().unwrap();

    let english = thresholds.language("en").unwrap();
    assert_eq!(english["speed"].metric, "accuracy_adjusted_wpm");
    assert_eq!(english["speed"].score_zero, 8.0);
    assert_eq!(english["speed"].boundaries, [22.5, 33.5, 45.4, 57.6, 72.8, 88.2, 108.9]);
    assert_eq!(english["speed"].score_eight, 129.7);
    assert_eq!(english["speed"].weight, 2.0);
    assert_eq!(english["accuracy"].weight, 1.0);

    let japanese = thresholds.language("ja").unwrap();
    assert_eq!(japanese["speed"].metric, "accuracy_adjusted_wpm");
    assert_eq!(japanese["speed"].score_zero, 6.4);
    assert_eq!(japanese["speed"].boundaries, [19.8, 29.8, 39.7, 50.9, 64.0, 78.4, 94.1]);
    assert_eq!(japanese["speed"].score_eight, 114.8);
    assert_eq!(japanese["accuracy"].weight, 1.0);

    for language in [english, japanese] {
        for component in ["rhythm", "transition", "preview", "recovery", "overlap"] {
            assert_eq!(language[component].weight, 1.0);
        }
    }

    for language in ["en", "ja"] {
        let before = v1.language(language).unwrap();
        let after = thresholds.language(language).unwrap();
        for component in ["accuracy", "rhythm", "transition", "preview", "recovery", "overlap"] {
            assert_eq!(after[component].metric, before[component].metric);
            assert_eq!(after[component].direction, before[component].direction);
            assert_eq!(after[component].unit, before[component].unit);
            assert_eq!(after[component].score_zero, before[component].score_zero);
            assert_eq!(after[component].boundaries, before[component].boundaries);
            assert_eq!(after[component].score_eight, before[component].score_eight);
        }
    }
}

#[test]
fn grades_v2_component_and_overall_use_the_requested_thresholds_and_weights() {
    let speed = grade_component_with_version("en", Component::Speed, 72.8, "grades-v2").unwrap();
    assert_eq!(speed.grade, Level::B);
    assert!((speed.score - 5.0).abs() < 1e-12);

    let result = evaluate_language_with_version(
        "en",
        &[
            ComponentValue::new(Component::Speed, 88.2),
            ComponentValue::new(Component::Accuracy, 98.0),
            ComponentValue::new(Component::Rhythm, 0.55),
        ],
        GradeSource::Bench,
        GradeStatus::Official,
        3,
        "grades-v2",
    )
    .unwrap();
    assert_eq!(result.version, "grades-v2");
    let expected = 4.0 / (2.0 / 7.0 + 1.0 / 7.0 + 1.0 / 4.0) - 1.0;
    assert!((result.overall.unwrap().score - expected).abs() < 1e-12);
}

#[test]
fn grade_changes_do_not_compare_across_threshold_versions() {
    let start = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
    let rows = vec![
        history_with_version(start, Level::C, 4.2, "grades-v1"),
        history_with_version(start + chrono::Days::new(1), Level::B, 5.1, "grades-v2"),
    ];
    assert!(detect_grade_changes(&rows).is_empty());

    let rows = vec![
        history_with_version(start, Level::C, 4.2, "grades-v1"),
        history_with_version(start + chrono::Days::new(1), Level::B, 5.1, "grades-v2"),
        history_with_version(start + chrono::Days::new(2), Level::A, 6.1, "grades-v2"),
    ];
    let changes = detect_grade_changes(&rows);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].from, Level::B);
    assert_eq!(changes[0].to, Level::A);

    let rows = vec![
        history_with_version(start, Level::C, 4.2, "grades-v2"),
        history_with_version(start + chrono::Days::new(1), Level::B, 5.1, "grades-v1"),
        history_with_version(start + chrono::Days::new(2), Level::A, 6.1, "grades-v2"),
    ];
    assert!(detect_grade_changes(&rows).is_empty());
}

#[test]
fn complete_v3_language_uses_grades_v2_latest_three_runs_without_changing_other_language() {
    let start = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
    let mut records = Vec::new();
    for (day, tempo) in [20.0, 30.0, 40.0, 50.0].into_iter().enumerate() {
        let at = start + chrono::Duration::days(day as i64);
        records.push(v3_record(at, "EW", 1, tempo, 10, 10));
        records.push(v3_record(at, "ES", 1, tempo, 10, 10));
    }
    for day in 0..3 {
        records.push(bench_record(start + chrono::Duration::days(day), "A", 120.0));
        records.push(bench_record(start + chrono::Duration::days(day), "C", 80.0));
    }

    let grades = analyze_grades(
        &records,
        &[],
        &builtin_layout("qwerty").unwrap(),
        &geometry("ansi").unwrap(),
        GradeOptions {
            model: AnalysisSettings::default(),
        },
        start + chrono::Duration::days(5),
        &["abcdef".to_owned()],
        &[JapaneseText {
            text: "あ".to_owned(),
            reading: "abcdef".to_owned(),
        }],
    );

    let english = grades.languages.iter().find(|grade| grade.language == "en").unwrap();
    assert_eq!(english.version, "grades-v2");
    assert_eq!(english.status, GradeStatus::Official);
    assert_eq!(english.n_sessions, 3);
    assert_eq!(component_value(english, Component::Speed), 40.0);
    assert_eq!(component_value(english, Component::Accuracy), 100.0);

    let japanese = grades.languages.iter().find(|grade| grade.language == "ja").unwrap();
    assert_eq!(japanese.version, "grades-v1");
    assert_eq!(japanese.n_sessions, 3);

    assert!(grades.history.iter().any(|point| point.language == "en" && point.version == "grades-v2"));
    assert!(grades.history.iter().any(|point| point.language == "en" && point.version == "grades-v1"));

    let selected = language_bench_records(&records, "en");
    assert_eq!(selected.len(), 8);
    assert!(
        selected
            .iter()
            .all(|record| record.header.bench_id.as_deref().is_some_and(|id| id.starts_with("bench-v3/")))
    );
}

#[test]
fn partial_v3_language_does_not_mix_old_bench_and_uses_accuracy_adjusted_provisional_speed() {
    let start = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
    let mut records = vec![v3_record(start + chrono::Duration::days(3), "EW", 1, 90.0, 10, 10)];
    for day in 0..3 {
        records.push(bench_record(start + chrono::Duration::days(day), "A", 120.0));
    }
    let mut practice = practice_record(start + chrono::Duration::days(2), 0.90);
    practice.header.net_wpm = 60.0;
    records.push(practice);

    let grades = analyze_grades(
        &records,
        &[form(start + chrono::Duration::days(2), 70.0)],
        &builtin_layout("qwerty").unwrap(),
        &geometry("ansi").unwrap(),
        GradeOptions {
            model: AnalysisSettings::default(),
        },
        start + chrono::Duration::days(4),
        &["abcdef".to_owned()],
        &[JapaneseText {
            text: "あ".to_owned(),
            reading: "abcdef".to_owned(),
        }],
    );
    let english = grades.languages.iter().find(|grade| grade.language == "en").unwrap();
    assert_eq!(english.version, "grades-v2");
    assert_eq!(english.source, GradeSource::Provisional);
    assert_eq!(english.status, GradeStatus::Provisional);
    assert_eq!(english.n_sessions, 1);
    assert_eq!(component_value(english, Component::Speed), 63.0);
    assert_eq!(component_value(english, Component::Accuracy), 90.0);
}

#[test]
fn v3_runs_use_local_dates_and_do_not_join_parts_across_local_midnight() {
    let before_midnight = Local.with_ymd_and_hms(2026, 1, 1, 23, 58, 0).single().unwrap().with_timezone(&Utc);
    let after_midnight = Local.with_ymd_and_hms(2026, 1, 2, 0, 1, 0).single().unwrap().with_timezone(&Utc);
    let records = vec![
        v3_record(before_midnight, "EW", 1, 40.0, 10, 10),
        v3_record(after_midnight, "ES", 1, 80.0, 10, 10),
    ];
    let grades = analyze_grades(
        &records,
        &[],
        &builtin_layout("qwerty").unwrap(),
        &geometry("ansi").unwrap(),
        GradeOptions {
            model: AnalysisSettings::default(),
        },
        after_midnight + chrono::Duration::hours(1),
        &["abcdef".to_owned()],
        &[],
    );
    assert!(grades.languages.iter().all(|grade| grade.language != "en"));
}

#[test]
fn v3_latest_three_follow_trial_time_when_form_numbers_wrap_on_the_same_day() {
    let start = Utc.with_ymd_and_hms(2026, 1, 1, 1, 0, 0).unwrap();
    let mut records = Vec::new();
    for (hour, form, tempo) in [(0, 7, 10.0), (1, 8, 20.0), (2, 1, 100.0), (3, 2, 110.0)] {
        let at = start + chrono::Duration::hours(hour);
        records.push(v3_record(at, "EW", form, tempo, 10, 10));
        records.push(v3_record(at, "ES", form, tempo, 10, 10));
    }
    let grades = analyze_grades(
        &records,
        &[],
        &builtin_layout("qwerty").unwrap(),
        &geometry("ansi").unwrap(),
        GradeOptions {
            model: AnalysisSettings::default(),
        },
        start + chrono::Duration::hours(5),
        &["abcdef".to_owned()],
        &[],
    );
    let english = grades.languages.iter().find(|grade| grade.language == "en").unwrap();
    assert_eq!(english.n_sessions, 3);
    assert_eq!(component_value(english, Component::Speed), 100.0);
}

#[test]
fn v3_history_cuts_each_snapshot_at_the_run_finish_instead_of_utc_day_end() {
    let first = Utc.with_ymd_and_hms(2026, 1, 1, 14, 0, 0).unwrap();
    let second = Utc.with_ymd_and_hms(2026, 1, 1, 22, 0, 0).unwrap();
    let records = vec![
        v3_record(first, "EW", 1, 20.0, 10, 10),
        v3_record(first, "ES", 1, 20.0, 10, 10),
        v3_record(second, "EW", 2, 100.0, 10, 10),
        v3_record(second, "ES", 2, 100.0, 10, 10),
    ];
    let grades = analyze_grades(
        &records,
        &[],
        &builtin_layout("qwerty").unwrap(),
        &geometry("ansi").unwrap(),
        GradeOptions {
            model: AnalysisSettings::default(),
        },
        second + chrono::Duration::hours(1),
        &["abcdef".to_owned()],
        &[],
    );
    let first_date = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
    let first_speed = grades
        .history
        .iter()
        .find(|point| point.language == "en" && point.component == "speed" && point.date == first_date && point.version == "grades-v2")
        .unwrap();
    let expected = grade_component_with_version("en", Component::Speed, 20.0, "grades-v2").unwrap();
    assert!((first_speed.score - expected.score).abs() < 1e-12, "{first_speed:?}");
}

fn history(date: chrono::NaiveDate, grade: Level, score: f64) -> GradeHistoryPoint {
    history_with_version(date, grade, score, "grades-v1")
}

fn history_with_version(date: chrono::NaiveDate, grade: Level, score: f64, version: &str) -> GradeHistoryPoint {
    GradeHistoryPoint {
        date,
        language: "en".to_owned(),
        component: "overall".to_owned(),
        grade,
        score,
        version: version.to_owned(),
    }
}

fn component_value(grade: &rs_typing_trainer::analysis::grades::LanguageGrade, component: Component) -> f64 {
    grade.components.iter().find(|row| row.component == component).unwrap().value
}

fn v3_record(at: chrono::DateTime<Utc>, part: &str, form: u8, tempo_wpm: f64, correct: u32, total: u32) -> SessionRecord {
    assert!(correct > 1 && correct <= total);
    let interval_us = (60_000_000.0 / (tempo_wpm * 5.0)).round() as u64;
    let keystrokes = (0..total)
        .map(|index| KeystrokeLog {
            seq: index,
            t_us: (u64::from(index) + 1) * interval_us,
            release_us: None,
            item_idx: 0,
            pos: index.min(correct - 1),
            kind: KeystrokeKind::Char,
            typed: Some(if index < correct { "a" } else { "x" }.to_owned()),
            expected: Some("a".to_owned()),
            correct: index < correct,
            code: None,
        })
        .collect::<Vec<_>>();
    let mut record = practice_record(at, correct as f64 / total as f64);
    record.header.mode = "bench".to_owned();
    record.header.bench_id = Some(format!("bench-v3/{part}/{form:02}"));
    record.header.finished_at = (at + chrono::Duration::seconds(60)).to_rfc3339();
    record.log = SessionLog {
        duration_us: 60_000_000,
        error_mode: ErrorMode::Stop,
        items: vec![ItemLog {
            idx: 0,
            text: "a".repeat(correct as usize),
            reading: None,
            kind: None,
            shown_us: 0,
            done_us: Some(keystrokes.last().unwrap().t_us),
            outcome: ItemOutcome::Completed,
        }],
        keystrokes,
    };
    record
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
