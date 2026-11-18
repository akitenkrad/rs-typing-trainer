use std::collections::BTreeMap;
use std::process::Command as ProcessCommand;

use chrono::NaiveDate;
use clap::Parser;
use rs_typing_trainer::analysis::errors::ErrorKind;
use rs_typing_trainer::analysis::grades::{Component, ComponentGrade, GradeSource, GradeStatus, LanguageGrade, Level, OverallGrade};
use rs_typing_trainer::cli::Cli;
use rs_typing_trainer::config::{Config, MenuConfig};
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::practice::menu::{
    MenuBuildInput, MenuLanguageInput, MenuRole, PastMenu, PrescriptionJudgment, PrescriptionState, build_menu, build_menu_for_languages,
    bundled_catalog, config_for_step,
};
use tempfile::TempDir;

fn component(component: Component, value: f64, score: f64) -> ComponentGrade {
    ComponentGrade {
        component,
        value,
        unit: String::new(),
        grade: Level::C,
        score,
        next_grade: Some(Level::B),
        next_threshold: None,
        gap: None,
        weight: if component == Component::Accuracy { 2.0 } else { 1.0 },
    }
}

fn grade(accuracy: f64, weakest: Component) -> LanguageGrade {
    let components = Component::all()
        .iter()
        .copied()
        .map(|candidate| {
            let value = if candidate == Component::Accuracy { accuracy } else { 1.5 };
            component(candidate, value, if candidate == weakest { 1.0 } else { 6.0 })
        })
        .collect();
    LanguageGrade {
        language: "en".to_owned(),
        source: GradeSource::Bench,
        status: GradeStatus::Official,
        n_sessions: 3,
        components,
        overall: Some(OverallGrade {
            grade: Level::C,
            score: 4.0,
            easiest_component: Some(weakest),
        }),
        missing: Vec::new(),
        version: "grades-v1".to_owned(),
    }
}

fn input(accuracy: f64, weakest: Component) -> MenuBuildInput {
    MenuBuildInput {
        as_of: NaiveDate::from_ymd_opt(2026, 11, 17).unwrap(),
        language: "en".to_owned(),
        grade: Some(grade(accuracy, weakest)),
        logged_keystrokes: 2_000,
        most_common_error: None,
        reach_percentile: None,
        japanese_percentile: None,
        has_spelling_suggestion: false,
        endurance_break_recommended: false,
        reaction_ms: None,
        reaction_median_ms: None,
        last_bench_date: None,
    }
}

fn language_input(language: &str, accuracy: f64, weakest: Component) -> MenuLanguageInput {
    let mut input = input(accuracy, weakest);
    input.language = language.to_owned();
    input.grade.as_mut().unwrap().language = language.to_owned();
    MenuLanguageInput {
        input,
        states: BTreeMap::new(),
    }
}

fn primary_id(menu: &rs_typing_trainer::practice::menu::DailyMenu, role: MenuRole) -> &str {
    &menu.steps.iter().find(|step| step.role == role).unwrap().prescription_id
}

#[test]
fn insufficient_data_builds_introductory_menu() {
    let catalog = bundled_catalog().unwrap();
    let mut case = input(97.0, Component::Preview);
    case.logged_keystrokes = 1_999;

    let menu = build_menu(catalog, &case, &MenuConfig::default(), &[], &BTreeMap::new()).unwrap();

    assert!(menu.introductory);
    assert_eq!(primary_id(&menu, MenuRole::PrimaryA), "transition.adaptive");
}

#[test]
fn low_accuracy_forces_accuracy_gate_or_anticipation() {
    let catalog = bundled_catalog().unwrap();
    let normal = build_menu(catalog, &input(94.0, Component::Preview), &MenuConfig::default(), &[], &BTreeMap::new()).unwrap();
    assert_eq!(primary_id(&normal, MenuRole::PrimaryA), "accuracy.gate");

    let mut anticipation = input(94.0, Component::Preview);
    anticipation.most_common_error = Some(ErrorKind::Anticipation);
    let anticipation = build_menu(catalog, &anticipation, &MenuConfig::default(), &[], &BTreeMap::new()).unwrap();
    assert_eq!(primary_id(&anticipation, MenuRole::PrimaryA), "accuracy.anticipation");
}

#[test]
fn preview_is_selected_when_it_has_the_largest_overall_gain() {
    let menu = build_menu(
        bundled_catalog().unwrap(),
        &input(97.0, Component::Preview),
        &MenuConfig::default(),
        &[],
        &BTreeMap::new(),
    )
    .unwrap();

    assert_eq!(primary_id(&menu, MenuRole::PrimaryA), "preview.wide");
}

#[test]
fn a_component_used_as_primary_on_both_recent_days_drops_one_place() {
    let case = input(97.0, Component::Preview);
    let history = [
        PastMenu {
            date: case.as_of.pred_opt().unwrap(),
            language: Some("en".to_owned()),
            primary_prescription_ids: vec!["preview.wide".to_owned()],
        },
        PastMenu {
            date: case.as_of - chrono::Duration::days(2),
            language: Some("en".to_owned()),
            primary_prescription_ids: vec!["preview.wide".to_owned()],
        },
    ];

    let menu = build_menu(bundled_catalog().unwrap(), &case, &MenuConfig::default(), &history, &BTreeMap::new()).unwrap();

    assert_ne!(primary_id(&menu, MenuRole::PrimaryA), "preview.wide");
    assert_eq!(primary_id(&menu, MenuRole::PrimaryB), "preview.wide");
}

#[test]
fn maintenance_component_drops_one_place_but_remains_available() {
    let mut states = BTreeMap::new();
    states.insert(
        "preview.wide".to_owned(),
        PrescriptionState {
            judgment: PrescriptionJudgment::Maintenance,
            ..PrescriptionState::default()
        },
    );

    let menu = build_menu(
        bundled_catalog().unwrap(),
        &input(97.0, Component::Preview),
        &MenuConfig::default(),
        &[],
        &states,
    )
    .unwrap();

    assert_ne!(primary_id(&menu, MenuRole::PrimaryA), "preview.wide");
    assert_eq!(primary_id(&menu, MenuRole::PrimaryB), "preview.wide");
}

#[test]
fn ineffective_transition_adaptive_uses_ngram_alternate() {
    let mut states = BTreeMap::new();
    states.insert(
        "transition.adaptive".to_owned(),
        PrescriptionState {
            judgment: PrescriptionJudgment::Ineffective,
            ..PrescriptionState::default()
        },
    );
    let menu = build_menu(
        bundled_catalog().unwrap(),
        &input(97.0, Component::Transition),
        &MenuConfig::default(),
        &[],
        &states,
    )
    .unwrap();

    assert_eq!(primary_id(&menu, MenuRole::PrimaryA), "transition.ngram");
}

#[test]
fn menu_minutes_control_primary_b_and_phase_durations() {
    let case = input(97.0, Component::Preview);
    let ten = build_menu(
        bundled_catalog().unwrap(),
        &case,
        &MenuConfig {
            minutes: 10,
            ..MenuConfig::default()
        },
        &[],
        &BTreeMap::new(),
    )
    .unwrap();
    assert!(!ten.steps.iter().any(|step| step.role == MenuRole::PrimaryB));

    let twenty = build_menu(
        bundled_catalog().unwrap(),
        &case,
        &MenuConfig {
            minutes: 20,
            ..MenuConfig::default()
        },
        &[],
        &BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(twenty.steps.iter().find(|step| step.role == MenuRole::PrimaryA).unwrap().minutes, 7);
    assert_eq!(twenty.steps.iter().find(|step| step.role == MenuRole::Check).unwrap().minutes, 4);
}

#[test]
fn missing_components_are_not_candidates() {
    let mut case = input(97.0, Component::Preview);
    let grade = case.grade.as_mut().unwrap();
    grade.components.retain(|row| row.component != Component::Preview);
    grade.missing.push(Component::Preview);

    let menu = build_menu(bundled_catalog().unwrap(), &case, &MenuConfig::default(), &[], &BTreeMap::new()).unwrap();

    assert_ne!(primary_id(&menu, MenuRole::PrimaryA), "preview.wide");
    assert_ne!(primary_id(&menu, MenuRole::PrimaryB), "preview.wide");
}

#[test]
fn stats_menu_cli_parses_json_and_rejects_other_output_modes() {
    let cli = Cli::try_parse_from(["rs-typing-trainer", "stats", "--menu", "--json", "--lang", "ja"]).unwrap();
    assert!(cli.stats_menu);
    assert!(cli.json);
    assert!(Cli::try_parse_from(["rs-typing-trainer", "stats", "--menu", "--summary"]).is_err());
    assert!(Cli::try_parse_from(["rs-typing-trainer", "stats", "--menu", "--csv", "sessions"]).is_err());
    assert!(Cli::try_parse_from(["rs-typing-trainer", "stats", "--json"]).is_err());
}

#[test]
fn stats_menu_outputs_text_and_machine_readable_json_using_only_a_temporary_db() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("config.toml");
    let db_path = dir.path().join("menu.db");
    std::fs::write(
        &config_path,
        format!("[storage]\ndb_path = {:?}\n[menu]\nminutes = 10\n", db_path.to_str().unwrap()),
    )
    .unwrap();

    let run = |extra: &[&str]| {
        ProcessCommand::new(env!("CARGO_BIN_EXE_rs-typing-trainer"))
            .args(["--config", config_path.to_str().unwrap(), "stats", "--menu"])
            .args(extra)
            .output()
            .unwrap()
    };
    let text = run(&["--lang", "ja"]);
    assert!(text.status.success(), "{}", String::from_utf8_lossy(&text.stderr));
    let text = String::from_utf8(text.stdout).unwrap();
    for expected in [
        "段",
        "処方 ID",
        "題",
        "レベル",
        "モード",
        "時間",
        "選んだ理由",
        "意識すること",
        "卒業の条件",
        "根拠の強さ",
    ] {
        assert!(text.contains(expected), "missing {expected:?}: {text}");
    }

    let json = run(&["--json", "--lang", "en"]);
    assert!(json.status.success(), "{}", String::from_utf8_lossy(&json.stderr));
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["requested_minutes"], 10);
    assert!(value["introductory"].as_bool().unwrap());
    assert!(value["steps"].as_array().unwrap().iter().all(|step| step.get("reason").is_some()));

    std::fs::write(
        &config_path,
        format!(
            "[storage]\ndb_path = {:?}\n[menu]\nminutes = 10\nlanguages = [\"ja\"]\n",
            db_path.to_str().unwrap()
        ),
    )
    .unwrap();
    let separated = run(&["--lang", "en"]);
    assert!(separated.status.success(), "{}", String::from_utf8_lossy(&separated.stderr));
    let separated = String::from_utf8(separated.stdout).unwrap();
    assert!(separated.contains("Today's practice menu"), "{separated}");
    assert!(separated.contains("Language: JA"), "{separated}");
    assert!(!separated.contains("今日の練習メニュー"), "{separated}");
}

#[test]
fn menu_minutes_default_to_fifteen_and_accept_only_supported_lengths() {
    assert_eq!(Config::default().menu.minutes, 15);
    for minutes in [10, 15, 20] {
        let config = Config::from_toml(&format!("[menu]\nminutes = {minutes}\n")).unwrap();
        assert_eq!(config.menu.minutes, minutes);
    }
    assert!(Config::from_toml("[menu]\nminutes = 12\n").is_err());
}

#[test]
fn step_settings_are_in_memory_overrides_and_preserve_unrelated_configuration() {
    let mut base = Config::default();
    base.menu.minutes = 10;
    let level = &bundled_catalog().unwrap().prescription("preview.wide").unwrap().levels[0];

    let overridden = config_for_step(&base, &level.mode, &level.settings).unwrap();

    assert_eq!(base.session.preview_items, 1, "the base config must not be mutated");
    assert_eq!(overridden.session.preview_items, 3);
    assert_eq!(overridden.mode.kind, rs_typing_trainer::config::ModeKind::Sentence);
    assert_eq!(overridden.menu.minutes, 10, "unrelated menu configuration must be preserved");
}

#[test]
fn menu_reasons_use_grade_precision_without_a_space_before_an_empty_unit() {
    let mut rhythm_case = input(97.0, Component::Rhythm);
    rhythm_case.language = "ja".to_owned();
    let rhythm = rhythm_case
        .grade
        .as_mut()
        .unwrap()
        .components
        .iter_mut()
        .find(|row| row.component == Component::Rhythm)
        .unwrap();
    rhythm.value = 0.719;
    rhythm.unit.clear();

    let rhythm_menu = build_menu(bundled_catalog().unwrap(), &rhythm_case, &MenuConfig::default(), &[], &BTreeMap::new()).unwrap();
    let rhythm_reason = &rhythm_menu.steps.iter().find(|step| step.role == MenuRole::PrimaryA).unwrap().reason;
    assert!(rhythm_reason.starts_with("リズム C (0.72)"), "{rhythm_reason}");
    assert!(!rhythm_reason.contains("0.72 "), "{rhythm_reason}");

    let mut speed_case = input(97.0, Component::Speed);
    speed_case.language = "ja".to_owned();
    let speed = speed_case
        .grade
        .as_mut()
        .unwrap()
        .components
        .iter_mut()
        .find(|row| row.component == Component::Speed)
        .unwrap();
    speed.value = 49.9;
    speed.unit = "WPM".to_owned();

    let speed_menu = build_menu(bundled_catalog().unwrap(), &speed_case, &MenuConfig::default(), &[], &BTreeMap::new()).unwrap();
    let speed_reason = &speed_menu.steps.iter().find(|step| step.role == MenuRole::PrimaryA).unwrap().reason;
    assert!(speed_reason.starts_with("速さ C (49.9 WPM)"), "{speed_reason}");
}

#[test]
fn two_languages_prioritize_low_english_accuracy_and_take_b_from_japanese() {
    let mut english = language_input("en", 93.0, Component::Speed);
    english
        .input
        .grade
        .as_mut()
        .unwrap()
        .components
        .iter_mut()
        .find(|row| row.component == Component::Accuracy)
        .unwrap()
        .grade = Level::D;
    let japanese = language_input("ja", 97.0, Component::Preview);

    let menu = build_menu_for_languages(bundled_catalog().unwrap(), &[english, japanese], Lang::En, &MenuConfig::default(), &[]).unwrap();

    let a = menu.steps.iter().find(|step| step.role == MenuRole::PrimaryA).unwrap();
    let b = menu.steps.iter().find(|step| step.role == MenuRole::PrimaryB).unwrap();
    assert_eq!(a.language, Lang::En);
    assert!(a.prescription_id.starts_with("accuracy."), "{}", a.prescription_id);
    assert_eq!(b.language, Lang::Ja);
    assert_eq!(b.prescription_id, "preview.wide");
}

#[test]
fn two_languages_choose_largest_overall_gain_for_a_and_other_language_for_b() {
    let mut english = language_input("en", 97.0, Component::Speed);
    english
        .input
        .grade
        .as_mut()
        .unwrap()
        .components
        .iter_mut()
        .find(|row| row.component == Component::Speed)
        .unwrap()
        .score = 0.0;
    let mut japanese = language_input("ja", 97.0, Component::Preview);
    japanese
        .input
        .grade
        .as_mut()
        .unwrap()
        .components
        .iter_mut()
        .find(|row| row.component == Component::Preview)
        .unwrap()
        .score = 2.0;

    let menu = build_menu_for_languages(bundled_catalog().unwrap(), &[english, japanese], Lang::Ja, &MenuConfig::default(), &[]).unwrap();

    let a = menu.steps.iter().find(|step| step.role == MenuRole::PrimaryA).unwrap();
    let b = menu.steps.iter().find(|step| step.role == MenuRole::PrimaryB).unwrap();
    assert_eq!((a.language, a.prescription_id.as_str()), (Lang::En, "speed.pacer"));
    assert_eq!((b.language, b.prescription_id.as_str()), (Lang::Ja, "preview.wide"));
}

#[test]
fn ten_minute_two_language_menu_omits_b_and_alternates_a_language_by_date() {
    let first = language_input("en", 97.0, Component::Speed);
    let second = language_input("ja", 97.0, Component::Preview);
    let config = MenuConfig {
        minutes: 10,
        ..MenuConfig::default()
    };
    let today = build_menu_for_languages(bundled_catalog().unwrap(), &[first.clone(), second.clone()], Lang::En, &config, &[]).unwrap();
    let mut tomorrow_inputs = [first, second];
    for input in &mut tomorrow_inputs {
        input.input.as_of = input.input.as_of.succ_opt().unwrap();
    }
    let tomorrow = build_menu_for_languages(bundled_catalog().unwrap(), &tomorrow_inputs, Lang::En, &config, &[]).unwrap();

    let today_a = today.steps.iter().find(|step| step.role == MenuRole::PrimaryA).unwrap();
    let tomorrow_a = tomorrow.steps.iter().find(|step| step.role == MenuRole::PrimaryA).unwrap();
    assert_ne!(today_a.language, tomorrow_a.language);
    assert!(!today.steps.iter().any(|step| step.role == MenuRole::PrimaryB));
    assert!(!tomorrow.steps.iter().any(|step| step.role == MenuRole::PrimaryB));
}

#[test]
fn insufficient_language_uses_introductory_b_and_both_insufficient_alternate_language() {
    let english = language_input("en", 97.0, Component::Speed);
    let mut japanese = language_input("ja", 97.0, Component::Preview);
    japanese.input.logged_keystrokes = 1_999;
    let mixed = build_menu_for_languages(
        bundled_catalog().unwrap(),
        &[english.clone(), japanese.clone()],
        Lang::En,
        &MenuConfig::default(),
        &[],
    )
    .unwrap();
    let b = mixed.steps.iter().find(|step| step.role == MenuRole::PrimaryB).unwrap();
    assert_eq!(b.language, Lang::Ja);
    assert_eq!(b.prescription_id, "transition.adaptive");
    assert!(b.reason.contains("JA"), "{}", b.reason);

    let mut english_missing = english;
    english_missing.input.logged_keystrokes = 1_999;
    let today = build_menu_for_languages(
        bundled_catalog().unwrap(),
        &[english_missing.clone(), japanese.clone()],
        Lang::En,
        &MenuConfig::default(),
        &[],
    )
    .unwrap();
    let mut tomorrow_inputs = [english_missing, japanese];
    for input in &mut tomorrow_inputs {
        input.input.as_of = input.input.as_of.succ_opt().unwrap();
    }
    let tomorrow = build_menu_for_languages(bundled_catalog().unwrap(), &tomorrow_inputs, Lang::En, &MenuConfig::default(), &[]).unwrap();
    assert!(today.introductory && tomorrow.introductory);
    assert_ne!(today.steps[0].language, tomorrow.steps[0].language);
    assert!(today.steps.iter().all(|step| step.language == today.steps[0].language));
    assert!(tomorrow.steps.iter().all(|step| step.language == tomorrow.steps[0].language));
}

#[test]
fn two_language_check_step_alternates_language_by_date() {
    let first = language_input("en", 97.0, Component::Speed);
    let second = language_input("ja", 97.0, Component::Preview);
    let today = build_menu_for_languages(
        bundled_catalog().unwrap(),
        &[first.clone(), second.clone()],
        Lang::En,
        &MenuConfig::default(),
        &[],
    )
    .unwrap();
    let mut tomorrow_inputs = [first, second];
    for input in &mut tomorrow_inputs {
        input.input.as_of = input.input.as_of.succ_opt().unwrap();
    }
    let tomorrow = build_menu_for_languages(bundled_catalog().unwrap(), &tomorrow_inputs, Lang::En, &MenuConfig::default(), &[]).unwrap();
    let check = |menu: &rs_typing_trainer::practice::menu::DailyMenu| menu.steps.iter().find(|step| step.role == MenuRole::Check).unwrap().language;
    assert_ne!(check(&today), check(&tomorrow));
}
