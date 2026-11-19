//! MYTASK-3626: no menu step runs longer than five minutes, and a step that
//! types long text (`sentence`, `endurance`) no longer than three.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use rs_typing_trainer::config::MenuConfig;
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::practice::menu::{
    DailyMenu, MAX_LONG_TEXT_MINUTES, MAX_STEP_MINUTES, MenuBuildInput, MenuLanguageInput, MenuRole, build_menu, build_menu_for_languages,
    bundled_catalog, introductory_transition_minutes, is_long_text_mode, level_minutes, parse_catalog,
};

fn new_player(language: &str) -> MenuBuildInput {
    MenuBuildInput {
        as_of: NaiveDate::from_ymd_opt(2026, 11, 19).unwrap(),
        language: language.to_owned(),
        grade: None,
        logged_keystrokes: 1_999,
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

fn introductory(language: &str, minutes: u8) -> DailyMenu {
    let menu = build_menu(
        bundled_catalog().unwrap(),
        &new_player(language),
        &MenuConfig {
            minutes,
            ..MenuConfig::default()
        },
        &[],
        &BTreeMap::new(),
    )
    .unwrap();
    assert!(menu.introductory);
    menu
}

fn assert_within_limits(menu: &DailyMenu) {
    for (index, step) in menu.steps.iter().enumerate() {
        assert!(
            step.minutes <= MAX_STEP_MINUTES,
            "step {} ({}) is {} minutes",
            index + 1,
            step.prescription_id,
            step.minutes
        );
        if is_long_text_mode(&step.mode) {
            assert!(
                step.minutes <= MAX_LONG_TEXT_MINUTES,
                "long-text step {} ({}, {}) is {} minutes",
                index + 1,
                step.prescription_id,
                step.mode,
                step.minutes
            );
        }
    }
}

fn shape(menu: &DailyMenu) -> Vec<(MenuRole, String, u8)> {
    menu.steps
        .iter()
        .map(|step| (step.role, step.prescription_id.clone(), step.minutes))
        .collect()
}

#[test]
fn the_introductory_transition_time_is_split_into_steps_of_at_most_five_minutes() {
    assert_eq!(introductory_transition_minutes(3), vec![3]);
    assert_eq!(introductory_transition_minutes(8), vec![4, 4]);
    assert_eq!(introductory_transition_minutes(13), vec![5, 4, 4]);
    assert_eq!(introductory_transition_minutes(5), vec![5]);
    assert_eq!(introductory_transition_minutes(6), vec![3, 3]);
    assert_eq!(introductory_transition_minutes(11), vec![4, 4, 3]);
    assert!(introductory_transition_minutes(0).is_empty());
    for total in 1..=40 {
        let parts = introductory_transition_minutes(total);
        assert_eq!(parts.iter().map(|part| u32::from(*part)).sum::<u32>(), u32::from(total));
        assert!(parts.iter().all(|part| (1..=MAX_STEP_MINUTES).contains(part)), "{total}: {parts:?}");
        assert!(
            parts.windows(2).all(|pair| pair[0] >= pair[1] && pair[0] - pair[1] <= 1),
            "{total}: {parts:?}"
        );
        assert_eq!(parts.len(), usize::from(total.div_ceil(MAX_STEP_MINUTES)));
    }
}

#[test]
fn introductory_menus_keep_their_total_and_every_step_within_the_limits() {
    let transition = |id: &str, minutes: u8| (MenuRole::PrimaryA, id.to_owned(), minutes);
    let fixed = |role: MenuRole, id: &str, minutes: u8| (role, id.to_owned(), minutes);
    let head = [fixed(MenuRole::Warmup, "fixed.warmup", 2)];
    let tail = [fixed(MenuRole::Check, "fixed.check", 3), fixed(MenuRole::Finish, "fixed.finish", 2)];
    let english = [
        (10, vec![transition("transition.adaptive", 3)]),
        (15, vec![transition("transition.adaptive", 4), transition("transition.ngram", 4)]),
        (
            20,
            vec![
                transition("transition.adaptive", 5),
                transition("transition.ngram", 4),
                transition("transition.adaptive", 4),
            ],
        ),
    ];
    for (minutes, middle) in english {
        let menu = introductory("en", minutes);
        assert_within_limits(&menu);
        assert_eq!(
            menu.total_minutes(),
            u32::from(minutes),
            "{minutes}-minute introductory menu keeps its length"
        );
        let expected = head.iter().cloned().chain(middle).chain(tail.iter().cloned()).collect::<Vec<_>>();
        assert_eq!(shape(&menu), expected, "{minutes}-minute menu");
    }

    // The n-gram drill draws from the English word pool only; a Japanese
    // introductory menu keeps every transition step on the adaptive drill.
    for minutes in [10, 15, 20] {
        let menu = introductory("ja", minutes);
        assert_within_limits(&menu);
        assert_eq!(menu.total_minutes(), u32::from(minutes));
        let transitions = menu.steps.iter().filter(|step| step.role == MenuRole::PrimaryA).collect::<Vec<_>>();
        assert_eq!(transitions.len(), usize::from((minutes - 7).div_ceil(MAX_STEP_MINUTES)));
        assert!(transitions.iter().all(|step| step.prescription_id == "transition.adaptive"));
        assert!(transitions.iter().all(|step| step.language == Lang::Ja));
    }
}

#[test]
fn a_two_language_introductory_menu_splits_the_chosen_language_the_same_way() {
    let inputs = ["en", "ja"].map(|language| MenuLanguageInput {
        input: new_player(language),
        states: BTreeMap::new(),
    });
    let menu = build_menu_for_languages(
        bundled_catalog().unwrap(),
        &inputs,
        Lang::En,
        &MenuConfig {
            minutes: 15,
            languages: Some(vec![Lang::En, Lang::Ja]),
        },
        &[],
    )
    .unwrap();
    assert!(menu.introductory);
    assert_within_limits(&menu);
    assert_eq!(menu.total_minutes(), 15);
    let language = menu.steps[0].language;
    assert!(menu.steps.iter().all(|step| step.language == language), "one language a day");
    let transitions = menu
        .steps
        .iter()
        .filter(|step| step.role == MenuRole::PrimaryA)
        .map(|step| step.minutes)
        .collect::<Vec<_>>();
    assert_eq!(transitions, vec![4, 4]);
    assert!(menu.steps[1].reason.starts_with(if language == Lang::En { "EN:" } else { "JA:" }));
}

#[test]
fn every_catalog_level_and_fixed_step_is_within_the_limits() {
    let catalog = bundled_catalog().unwrap();
    for step in &catalog.fixed_steps {
        let limit = if is_long_text_mode(&step.mode) {
            MAX_LONG_TEXT_MINUTES
        } else {
            MAX_STEP_MINUTES
        };
        assert!(step.minutes <= limit, "{} is {} minutes", step.id, step.minutes);
    }
    for row in &catalog.prescriptions {
        for (index, level) in row.levels.iter().enumerate() {
            let limit = if is_long_text_mode(&level.mode) {
                MAX_LONG_TEXT_MINUTES
            } else {
                MAX_STEP_MINUTES
            };
            assert!(level.minutes <= limit, "{} level {} is {} minutes", row.id, index + 1, level.minutes);
            // Whatever frame the slot offers (4–5, or 7 in a 20-minute menu).
            for frame in [4, 5, 7] {
                assert!(
                    level_minutes(level, frame) <= limit,
                    "{} level {} in a {frame}-minute slot",
                    row.id,
                    index + 1
                );
            }
            if level.mode == "endurance" {
                assert_eq!(level.minutes, 3, "{} level {}", row.id, index + 1);
                assert_eq!(
                    level.settings.get("endurance.minutes").and_then(toml::Value::as_integer),
                    Some(3),
                    "{} level {}",
                    row.id,
                    index + 1
                );
            }
        }
    }
}

#[test]
fn a_catalog_with_a_step_over_the_limit_does_not_load() {
    let raw = include_str!("../../data/menu/menu-v1.toml");
    let replace = |from: &str, to: &str| {
        let changed = raw.replacen(from, to, 1);
        assert_ne!(changed, raw, "{from:?} not found");
        parse_catalog(&changed).unwrap_err()
    };

    // A words level of 6 minutes (limit 5).
    let error = replace(
        "mode = \"words\"\nminutes = 4\nsettings = { \"session.error_mode\" = \"stop\", \"gate.min_accuracy\" = 0.97",
        "mode = \"words\"\nminutes = 6\nsettings = { \"session.error_mode\" = \"stop\", \"gate.min_accuracy\" = 0.97",
    );
    assert!(error.contains("accuracy.gate level 1") && error.contains("limit"), "{error}");

    // A sentence level of 4 minutes (limit 3).
    let error = replace(
        "mode = \"sentence\"\nminutes = 3\nsettings = { \"session.preview_items\" = 3",
        "mode = \"sentence\"\nminutes = 4\nsettings = { \"session.preview_items\" = 3",
    );
    assert!(error.contains("preview.wide level 1") && error.contains("limit"), "{error}");

    // An endurance level of 4 minutes, with the mode's own duration agreeing.
    let error = replace(
        "minutes = 3\nsettings = { \"endurance.minutes\" = 3 }",
        "minutes = 4\nsettings = { \"endurance.minutes\" = 4 }",
    );
    assert!(error.contains("endurance.split level 1") && error.contains("limit"), "{error}");

    // An endurance level that leaves `minutes` out but sets 5 minutes itself.
    let error = replace(
        "minutes = 3\nsettings = { \"endurance.minutes\" = 3 }",
        "settings = { \"endurance.minutes\" = 5 }",
    );
    assert!(error.contains("endurance.split level 1") && error.contains("limit"), "{error}");

    // A fixed step: the check (sentence) of 4 minutes.
    let error = replace(
        "id = \"fixed.check\"\ntitle = { en = \"Unassisted check\", ja = \"補助なしの確認\" }\nmode = \"sentence\"\nminutes = 3",
        "id = \"fixed.check\"\ntitle = { en = \"Unassisted check\", ja = \"補助なしの確認\" }\nmode = \"sentence\"\nminutes = 4",
    );
    assert!(error.contains("fixed.check") && error.contains("limit"), "{error}");
}

#[test]
fn personalized_menus_keep_every_step_within_the_limits() {
    use rs_typing_trainer::analysis::grades::{Component, ComponentGrade, GradeSource, GradeStatus, LanguageGrade, Level, OverallGrade};
    use rs_typing_trainer::practice::menu::PrescriptionState;

    let catalog = bundled_catalog().unwrap();
    for weakest in Component::all().iter().copied() {
        let components = Component::all()
            .iter()
            .copied()
            .map(|candidate| ComponentGrade {
                component: candidate,
                value: if candidate == Component::Accuracy { 97.0 } else { 1.5 },
                unit: String::new(),
                grade: Level::C,
                score: if candidate == weakest { 1.0 } else { 6.0 },
                next_grade: Some(Level::B),
                next_threshold: None,
                gap: None,
                weight: 1.0,
            })
            .collect();
        let mut input = new_player("en");
        input.logged_keystrokes = 5_000;
        input.endurance_break_recommended = true;
        input.grade = Some(LanguageGrade {
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
        });
        // Every level of every prescription, at the highest level too.
        for level in 1..=3 {
            let states = catalog
                .prescriptions
                .iter()
                .map(|row| {
                    (
                        row.id.clone(),
                        PrescriptionState {
                            level,
                            ..PrescriptionState::default()
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>();
            for minutes in [10, 15, 20] {
                let menu = build_menu(
                    catalog,
                    &input,
                    &MenuConfig {
                        minutes,
                        ..MenuConfig::default()
                    },
                    &[],
                    &states,
                )
                .unwrap();
                assert!(!menu.introductory);
                assert_within_limits(&menu);
                if minutes == 20 {
                    let check = menu.steps.iter().find(|step| step.role == MenuRole::Check).unwrap();
                    assert_eq!(check.minutes, 3, "the 20-minute check is cut to the long-text limit");
                }
            }
        }
    }
}
