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
        blind_assistance_removable: false,
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
            check_language: Some("en".to_owned()),
            primary_prescription_ids: vec!["preview.wide".to_owned()],
        },
        PastMenu {
            date: case.as_of - chrono::Duration::days(2),
            language: Some("en".to_owned()),
            check_language: Some("ja".to_owned()),
            primary_prescription_ids: vec!["preview.wide".to_owned()],
        },
    ];

    let menu = build_menu(bundled_catalog().unwrap(), &case, &MenuConfig::default(), &history, &BTreeMap::new()).unwrap();

    assert_ne!(primary_id(&menu, MenuRole::PrimaryA), "preview.wide");
    assert_eq!(primary_id(&menu, MenuRole::PrimaryB), "preview.wide");
}

/// MYTASK-3620 (design §3.1, §4.1 5, 2026-11-18): a prescription in
/// maintenance is removed from the candidates. The earlier expectation (it
/// only drops one place and stays as primary B) followed the old §4.1 5
/// wording, which contradicted §3.1.
#[test]
fn maintenance_prescription_is_removed_from_the_candidates() {
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
    assert_ne!(primary_id(&menu, MenuRole::PrimaryB), "preview.wide");
    assert!(menu.steps.iter().all(|step| step.prescription_id != "preview.wide"));
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
fn reference_improvement_is_labeled_as_uncontrolled_in_the_menu_reason() {
    let mut case = input(97.0, Component::Preview);
    case.language = "ja".to_owned();
    case.grade.as_mut().unwrap().language = "ja".to_owned();
    let mut states = BTreeMap::new();
    states.insert(
        "preview.wide".to_owned(),
        PrescriptionState {
            judgment: PrescriptionJudgment::Improved,
            ..PrescriptionState::default()
        },
    );

    let menu = build_menu(bundled_catalog().unwrap(), &case, &MenuConfig::default(), &[], &states).unwrap();
    let primary = menu.steps.iter().find(|step| step.role == MenuRole::PrimaryA).unwrap();
    assert!(primary.reason.contains("改善（参考．対照なし）"), "{}", primary.reason);
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
    // MYTASK-3619 (design §4.2, 2026-11-18): a prescription step takes its
    // catalog level's minutes (preview.wide: 4); the 20-minute frame of 7 is
    // only the default for a level without minutes. The earlier expectation
    // of 7 stretched every primary A to the frame. MYTASK-3626: preview.wide
    // ① is a sentence step, so the long-text limit of 3 applies; the check
    // step (sentence) is 3 too instead of the 20-minute frame's 4.
    assert_eq!(twenty.steps.iter().find(|step| step.role == MenuRole::PrimaryA).unwrap().minutes, 3);
    assert_eq!(twenty.steps.iter().find(|step| step.role == MenuRole::Check).unwrap().minutes, 3);
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
    assert!(separated.contains("Language: Japanese"), "{separated}");
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

#[test]
fn two_language_daily_rotation_uses_yesterdays_record_instead_of_calendar_parity() {
    let first = language_input("en", 97.0, Component::Speed);
    let second = language_input("ja", 97.0, Component::Preview);
    let config = MenuConfig {
        minutes: 10,
        ..MenuConfig::default()
    };
    let without_history = build_menu_for_languages(bundled_catalog().unwrap(), &[first.clone(), second.clone()], Lang::En, &config, &[]).unwrap();
    let prior_a = without_history
        .steps
        .iter()
        .find(|step| step.role == MenuRole::PrimaryA)
        .unwrap()
        .language;
    let prior_check = without_history.steps.iter().find(|step| step.role == MenuRole::Check).unwrap().language;
    let history = [PastMenu {
        date: first.input.as_of.pred_opt().unwrap(),
        language: Some(
            match prior_a {
                Lang::En => "en",
                Lang::Ja => "ja",
            }
            .to_owned(),
        ),
        check_language: Some(
            match prior_check {
                Lang::En => "en",
                Lang::Ja => "ja",
            }
            .to_owned(),
        ),
        primary_prescription_ids: Vec::new(),
    }];

    let with_history = build_menu_for_languages(bundled_catalog().unwrap(), &[first, second], Lang::En, &config, &history).unwrap();
    let current_a = with_history.steps.iter().find(|step| step.role == MenuRole::PrimaryA).unwrap().language;
    let current_check = with_history.steps.iter().find(|step| step.role == MenuRole::Check).unwrap().language;
    assert_ne!(current_a, prior_a);
    assert_ne!(current_check, prior_check);
}

// ---------------------------------------------------------------------------
// MYTASK-3619〜3622 (design §4.1, §4.2, §5; 2026-11-18)
// ---------------------------------------------------------------------------

fn states_with(entries: &[(&str, PrescriptionState)]) -> BTreeMap<String, PrescriptionState> {
    entries.iter().map(|(id, state)| ((*id).to_owned(), *state)).collect()
}

fn maintenance() -> PrescriptionState {
    PrescriptionState {
        judgment: PrescriptionJudgment::Maintenance,
        ..PrescriptionState::default()
    }
}

/// Every grade component in maintenance except the ones listed.
fn all_in_maintenance_except(keep: &[&str]) -> BTreeMap<String, PrescriptionState> {
    let ids = [
        "speed.pacer",
        "accuracy.gate",
        "rhythm.metronome",
        "transition.adaptive",
        "preview.wide",
        "recovery.retry",
        "overlap.burst",
    ];
    states_with(
        &ids.iter()
            .filter(|id| !keep.contains(id))
            .map(|id| (*id, maintenance()))
            .collect::<Vec<_>>(),
    )
}

#[test]
fn removing_maintenance_fills_a_missing_slot_with_transition_adaptive() {
    // Only preview remains after maintenance is removed: B is filled with
    // transition.adaptive (§4.1 5, MYTASK-3620).
    let mut states = all_in_maintenance_except(&["preview.wide", "transition.adaptive"]);
    states.remove("transition.adaptive");
    let mut case = input(97.0, Component::Preview);
    let grade = case.grade.as_mut().unwrap();
    for row in &mut grade.components {
        if row.component == Component::Transition {
            row.grade = Level::S;
        }
    }
    let menu = build_menu(bundled_catalog().unwrap(), &case, &MenuConfig::default(), &[], &states).unwrap();

    assert_eq!(primary_id(&menu, MenuRole::PrimaryA), "preview.wide");
    assert_eq!(primary_id(&menu, MenuRole::PrimaryB), "transition.adaptive");
    let filler = menu.steps.iter().find(|step| step.role == MenuRole::PrimaryB).unwrap();
    assert!(filler.reason.contains("too few candidates"), "{}", filler.reason);
}

#[test]
fn extra_weaknesses_enter_when_fewer_than_two_candidates_remain_after_maintenance() {
    // §4.1 4 (MYTASK-3620): the condition counts candidates without
    // maintenance. Reach at the 20th percentile is not worse than the
    // weakest component, so it enters only because maintenance left one.
    let mut case = input(97.0, Component::Preview);
    case.reach_percentile = Some(20.0);
    let without = build_menu(bundled_catalog().unwrap(), &case, &MenuConfig::default(), &[], &BTreeMap::new()).unwrap();
    assert!(without.steps.iter().all(|step| step.prescription_id != "reach.unlock"));

    let states = all_in_maintenance_except(&["preview.wide"]);
    let menu = build_menu(bundled_catalog().unwrap(), &case, &MenuConfig::default(), &[], &states).unwrap();
    assert_eq!(primary_id(&menu, MenuRole::PrimaryA), "reach.unlock");
    assert_eq!(primary_id(&menu, MenuRole::PrimaryB), "preview.wide");
}

#[test]
fn removable_keyboard_assistance_puts_blind_fade_first() {
    // §4.1 4 (MYTASK-3621).
    let mut case = input(97.0, Component::Preview);
    case.blind_assistance_removable = true;
    let menu = build_menu(bundled_catalog().unwrap(), &case, &MenuConfig::default(), &[], &BTreeMap::new()).unwrap();
    assert_eq!(primary_id(&menu, MenuRole::PrimaryA), "blind.fade");
    assert_eq!(primary_id(&menu, MenuRole::PrimaryB), "preview.wide");
    let blind = menu.steps.iter().find(|step| step.role == MenuRole::PrimaryA).unwrap();
    assert_eq!(blind.settings.get("keyboard.show").and_then(toml::Value::as_str), Some("auto"));

    // Accuracy priority still wins primary A.
    let mut low = input(94.0, Component::Preview);
    low.blind_assistance_removable = true;
    let menu = build_menu(bundled_catalog().unwrap(), &low, &MenuConfig::default(), &[], &BTreeMap::new()).unwrap();
    assert_eq!(primary_id(&menu, MenuRole::PrimaryA), "accuracy.gate");
    assert_eq!(primary_id(&menu, MenuRole::PrimaryB), "blind.fade");

    // Without the mark it is not a candidate.
    let menu = build_menu(
        bundled_catalog().unwrap(),
        &input(97.0, Component::Preview),
        &MenuConfig::default(),
        &[],
        &BTreeMap::new(),
    )
    .unwrap();
    assert!(menu.steps.iter().all(|step| step.prescription_id != "blind.fade"));
}

#[test]
fn prescription_steps_take_the_catalog_level_minutes_and_the_total_is_shown() {
    // §4.2 (MYTASK-3619): endurance.split level ③ keeps its catalog minutes
    // in a 20-minute menu (frame 7) and in a 10-minute menu (frame 4); the
    // difference is not redistributed and the total says so. MYTASK-3626:
    // every endurance level is 3 minutes (the long-text limit).
    let mut case = input(97.0, Component::Preview);
    case.endurance_break_recommended = true;
    let mut states = all_in_maintenance_except(&[]);
    states.insert(
        "endurance.split".to_owned(),
        PrescriptionState {
            level: 3,
            ..PrescriptionState::default()
        },
    );
    for minutes in [10, 20] {
        let menu = build_menu(
            bundled_catalog().unwrap(),
            &case,
            &MenuConfig {
                minutes,
                ..MenuConfig::default()
            },
            &[],
            &states,
        )
        .unwrap();
        let primary = menu.steps.iter().find(|step| step.role == MenuRole::PrimaryA).unwrap();
        assert_eq!(primary.prescription_id, "endurance.split");
        assert_eq!(primary.minutes, 3, "{minutes}-minute menu");
        assert_eq!(primary.settings.get("endurance.minutes").and_then(toml::Value::as_integer), Some(3));
        let total = menu.steps.iter().map(|step| u32::from(step.minutes)).sum::<u32>();
        assert_eq!(menu.total_minutes(), total);
        assert_ne!(menu.total_minutes(), u32::from(minutes));
        let label = rs_typing_trainer::practice::menu::minutes_label(&menu, Lang::Ja);
        assert_eq!(label, format!("{total} 分（設定 {minutes} 分）"));
    }
}

#[test]
fn a_level_without_minutes_uses_the_frame_and_endurance_minutes_must_match_the_step() {
    use rs_typing_trainer::practice::menu::{level_minutes, parse_catalog};

    let catalog = bundled_catalog().unwrap();
    let mut level = catalog.prescription("accuracy.gate").unwrap().levels[0].clone();
    assert_eq!(level_minutes(&level, 4), 4);
    level.minutes = 0;
    assert_eq!(level_minutes(&level, 4), 4, "no minutes: the frame default");
    assert_eq!(level_minutes(&level, 7), 5, "the 7-minute frame is cut to the 5-minute step limit");
    let mut sentence = catalog.prescription("preview.wide").unwrap().levels[0].clone();
    sentence.minutes = 0;
    assert_eq!(level_minutes(&sentence, 7), 3, "a sentence step is cut to the long-text limit");
    let mut endurance = catalog.prescription("endurance.split").unwrap().levels[1].clone();
    endurance.minutes = 0;
    assert_eq!(level_minutes(&endurance, 7), 3, "the mode's own duration setting");

    let raw = include_str!("../../data/menu/menu-v1.toml");
    let mismatched = raw.replacen(
        "minutes = 3\nsettings = { \"endurance.minutes\" = 3 }",
        "minutes = 2\nsettings = { \"endurance.minutes\" = 3 }",
        1,
    );
    assert_ne!(raw, mismatched);
    let error = parse_catalog(&mismatched).unwrap_err();
    assert!(error.contains("endurance.minutes"), "{error}");
    let without_minutes = raw.replacen(
        "minutes = 3\nsettings = { \"endurance.minutes\" = 3 }",
        "settings = { \"endurance.minutes\" = 3 }",
        1,
    );
    assert!(parse_catalog(&without_minutes).is_ok(), "minutes is optional");
}

#[test]
fn endurance_menu_step_keeps_the_mode_duration_equal_to_the_step_time() {
    use rs_typing_trainer::practice::menu::config_for_menu_step;

    let mut case = input(97.0, Component::Preview);
    case.endurance_break_recommended = true;
    let mut states = all_in_maintenance_except(&[]);
    states.insert(
        "endurance.split".to_owned(),
        PrescriptionState {
            level: 2,
            ..PrescriptionState::default()
        },
    );
    let menu = build_menu(bundled_catalog().unwrap(), &case, &MenuConfig::default(), &[], &states).unwrap();
    let step = menu.steps.iter().find(|step| step.prescription_id == "endurance.split").unwrap();
    assert_eq!(step.minutes, 3);
    let mut base = Config::default();
    base.endurance.minutes = 5;
    let config = config_for_menu_step(&base, step).unwrap();
    assert_eq!(config.endurance.minutes, u64::from(step.minutes));
}

#[test]
fn reference_basis_and_no_reference_note_for_prescriptions_outside_the_seven_components() {
    use rs_typing_trainer::practice::menu::{ReferenceBasis, reference_basis};

    let catalog = bundled_catalog().unwrap();
    let basis = |id: &str| reference_basis(catalog.prescription(id).unwrap());
    assert_eq!(basis("reach.unlock"), ReferenceBasis::RawCheck);
    assert_eq!(basis("endurance.split"), ReferenceBasis::AssistedStep);
    assert_eq!(basis("japanese.spelling"), ReferenceBasis::NotJudged);
    assert_eq!(basis("blind.fade"), ReferenceBasis::NotJudged);
    assert_eq!(basis("transition.adaptive"), ReferenceBasis::R3);
    assert_eq!(basis("preview.wide"), ReferenceBasis::ComponentScore(Component::Preview));

    let mut case = input(97.0, Component::Preview);
    case.blind_assistance_removable = true;
    case.language = "ja".to_owned();
    case.grade.as_mut().unwrap().language = "ja".to_owned();
    let menu = build_menu(catalog, &case, &MenuConfig::default(), &[], &BTreeMap::new()).unwrap();
    let blind = menu.steps.iter().find(|step| step.prescription_id == "blind.fade").unwrap();
    assert!(blind.advance.contains("参考の判定なし"), "{}", blind.advance);

    let mut case = input(97.0, Component::Preview);
    case.endurance_break_recommended = true;
    let menu = build_menu(catalog, &case, &MenuConfig::default(), &[], &all_in_maintenance_except(&[])).unwrap();
    let endurance = menu.steps.iter().find(|step| step.prescription_id == "endurance.split").unwrap();
    assert!(endurance.advance.contains("assisted step"), "{}", endurance.advance);
}
