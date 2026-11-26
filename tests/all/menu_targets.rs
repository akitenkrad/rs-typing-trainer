//! MYTASK-3618 (design §4.2, §11; 2026-11-18): a prescription's target goes
//! into the prompts of its step.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use chrono::NaiveDate;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::analysis::grades::{Component, ComponentGrade, GradeSource, GradeStatus, LanguageGrade, Level, OverallGrade};
use rs_typing_trainer::app::Screen;
use rs_typing_trainer::config::{Config, DataConfig, MenuConfig};
use rs_typing_trainer::data::{DataCatalog, TrainingDataSource};
use rs_typing_trainer::engine::modes::ModeGenerator;
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::layout::builtin_layout;
use rs_typing_trainer::practice::menu::{
    DailyMenu, LanguageTargets, MIN_TARGET_WORDS, MenuBuildInput, MenuRole, MenuStep, PrescriptionJudgment, PrescriptionState, SpellingTarget,
    StepTargets, apply_step_targets, build_menu, bundled_catalog, config_for_menu_step, generator_kind_for_step, remote_row_keys,
    restrict_catalog_to_step_targets,
};
use tempfile::TempDir;

use crate::common;
use common::test_app_at;

fn grade(language: &str, weakest: Component) -> LanguageGrade {
    LanguageGrade {
        language: language.to_owned(),
        source: GradeSource::Bench,
        status: GradeStatus::Official,
        n_sessions: 3,
        components: Component::all()
            .iter()
            .copied()
            .map(|component| ComponentGrade {
                component,
                value: if component == Component::Accuracy { 97.0 } else { 1.5 },
                unit: String::new(),
                grade: Level::C,
                score: if component == weakest { 1.0 } else { 6.0 },
                next_grade: Some(Level::B),
                next_threshold: None,
                gap: None,
                weight: if component == Component::Accuracy { 2.0 } else { 1.0 },
            })
            .collect(),
        overall: Some(OverallGrade {
            grade: Level::C,
            score: 4.0,
            easiest_component: Some(weakest),
        }),
        missing: Vec::new(),
        version: "grades-v1".to_owned(),
    }
}

fn input(language: &str, weakest: Component) -> MenuBuildInput {
    MenuBuildInput {
        as_of: NaiveDate::from_ymd_opt(2026, 11, 18).unwrap(),
        language: language.to_owned(),
        grade: Some(grade(language, weakest)),
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

fn data() -> (TempDir, DataCatalog) {
    let home = TempDir::new().unwrap();
    let catalog = DataCatalog::load(&DataConfig::default(), home.path()).unwrap();
    (home, catalog)
}

fn primary<'a>(menu: &'a DailyMenu, id: &str) -> &'a MenuStep {
    menu.steps
        .iter()
        .find(|step| step.prescription_id == id && matches!(step.role, MenuRole::PrimaryA | MenuRole::PrimaryB))
        .unwrap_or_else(|| {
            panic!(
                "no {id} step in {:?}",
                menu.steps.iter().map(|step| &step.prescription_id).collect::<Vec<_>>()
            )
        })
}

fn targets(language: &str, value: LanguageTargets) -> BTreeMap<String, LanguageTargets> {
    BTreeMap::from([(language.to_owned(), value)])
}

fn pairs(values: &[(&str, &str)]) -> Vec<(String, String)> {
    values.iter().map(|(from, to)| ((*from).to_owned(), (*to).to_owned())).collect()
}

/// Prompts the app's generator draws for a step, after the target narrowing.
fn prompts(catalog: &DataCatalog, step: &MenuStep, count: usize) -> Vec<rs_typing_trainer::engine::modes::GeneratedPrompt> {
    let mut config = config_for_menu_step(&Config::default(), step).unwrap();
    config.mode.kind = generator_kind_for_step(config.mode.kind, step.language);
    let mut catalog = catalog.clone();
    restrict_catalog_to_step_targets(&mut catalog, step);
    let mut generator = ModeGenerator::new(config.mode, catalog, builtin_layout("qwerty").unwrap(), 7);
    (0..count).map(|_| generator.next_prompt()).collect()
}

fn level(level: u8) -> PrescriptionState {
    PrescriptionState {
        level,
        ..PrescriptionState::default()
    }
}

#[test]
fn reach_unlock_targets_the_remote_row_keys() {
    let (_home, catalog) = data();
    let layout = builtin_layout("qwerty").unwrap();
    assert_eq!(remote_row_keys(&layout), "qwertyuiopzxcvbnm");
    let mut case = input("en", Component::Preview);
    case.reach_percentile = Some(5.0);
    let mut menu = build_menu(bundled_catalog().unwrap(), &case, &MenuConfig::default(), &[], &BTreeMap::new()).unwrap();
    assert_eq!(
        primary(&menu, "reach.unlock").settings.get("mode.keys"),
        None,
        "before: the free-play keys"
    );

    apply_step_targets(&mut menu, &BTreeMap::new(), &catalog, &layout, &Config::default(), Lang::En).unwrap();
    let step = primary(&menu, "reach.unlock");
    assert_eq!(step.settings["mode.keys"].as_str(), Some("qwertyuiopzxcvbnm"));
    let config = config_for_menu_step(&Config::default(), step).unwrap();
    assert_eq!(config.mode.keys, "qwertyuiopzxcvbnm");
    assert!(!StepTargets::of(step).relaxed);
}

#[test]
fn transition_adaptive_level_three_prefers_words_with_the_target_transitions() {
    let (_home, catalog) = data();
    let layout = builtin_layout("qwerty").unwrap();
    let states = BTreeMap::from([("transition.adaptive".to_owned(), level(3))]);
    let mut menu = build_menu(
        bundled_catalog().unwrap(),
        &input("en", Component::Transition),
        &MenuConfig::default(),
        &[],
        &states,
    )
    .unwrap();
    let transitions = LanguageTargets {
        transitions: pairs(&[("t", "h"), ("h", "e")]),
        spelling: None,
    };
    apply_step_targets(&mut menu, &targets("en", transitions), &catalog, &layout, &Config::default(), Lang::En).unwrap();

    let step = primary(&menu, "transition.adaptive");
    assert_eq!(step.mode, "words");
    assert_eq!(step.settings["mode.keys"].as_str(), Some("the"));
    assert_eq!(StepTargets::of(step).transitions, vec!["th".to_owned(), "he".to_owned()]);
    assert!(step.reason.contains("target transitions th he"), "{}", step.reason);
    for prompt in prompts(&catalog, step, 200) {
        assert!(
            prompt.target.contains("th") || prompt.target.contains("he"),
            "{} does not contain a target transition",
            prompt.target
        );
    }
}

#[test]
fn transition_ngram_drills_use_the_target_transition_keys() {
    let (_home, catalog) = data();
    let layout = builtin_layout("qwerty").unwrap();
    let states = BTreeMap::from([(
        "transition.adaptive".to_owned(),
        PrescriptionState {
            judgment: PrescriptionJudgment::Ineffective,
            ..PrescriptionState::default()
        },
    )]);
    let mut menu = build_menu(
        bundled_catalog().unwrap(),
        &input("en", Component::Transition),
        &MenuConfig::default(),
        &[],
        &states,
    )
    .unwrap();
    let transitions = LanguageTargets {
        transitions: pairs(&[("q", "u")]),
        spelling: None,
    };
    apply_step_targets(&mut menu, &targets("en", transitions), &catalog, &layout, &Config::default(), Lang::En).unwrap();

    let step = primary(&menu, "transition.ngram");
    assert_eq!(step.settings["mode.keys"].as_str(), Some("qu"));
    let drills = prompts(&catalog, step, 50);
    assert!(drills.iter().any(|prompt| prompt.target == "qu"));
    assert!(
        drills
            .iter()
            .all(|prompt| prompt.target.chars().all(|character| "qu".contains(character)))
    );
}

fn japanese_ngram_menu(transitions: &[(&str, &str)]) -> (TempDir, DataCatalog, DailyMenu) {
    let (home, catalog) = data();
    let layout = builtin_layout("qwerty").unwrap();
    let states = BTreeMap::from([(
        "transition.adaptive".to_owned(),
        PrescriptionState {
            judgment: PrescriptionJudgment::Ineffective,
            ..PrescriptionState::default()
        },
    )]);
    let mut menu = build_menu(
        bundled_catalog().unwrap(),
        &input("ja", Component::Transition),
        &MenuConfig::default(),
        &[],
        &states,
    )
    .unwrap();
    let value = LanguageTargets {
        transitions: pairs(transitions),
        spelling: None,
    };
    apply_step_targets(&mut menu, &targets("ja", value), &catalog, &layout, &Config::default(), Lang::Ja).unwrap();
    (home, catalog, menu)
}

#[test]
fn japanese_ngram_step_narrows_to_japanese_words_containing_the_transition() {
    let (_home, catalog, menu) = japanese_ngram_menu(&[("k", "a")]);
    let step = primary(&menu, "transition.ngram");
    assert_eq!(step.language, Lang::Ja);
    assert!(!StepTargets::of(step).relaxed);
    assert!(step.reason.contains("対象の遷移 ka"), "{}", step.reason);
    for prompt in prompts(&catalog, step, 200) {
        assert!(prompt.reading.is_some(), "a Japanese word: {}", prompt.text);
        assert!(prompt.target.contains("ka"), "{} ({}) has no ka", prompt.text, prompt.target);
    }
}

#[test]
fn too_few_japanese_words_with_the_target_relax_the_narrowing_and_say_so() {
    let (_home, catalog, menu) = japanese_ngram_menu(&[("q", "x")]);
    let step = primary(&menu, "transition.ngram");
    let targets = StepTargets::of(step);
    assert!(targets.relaxed);
    assert!(step.reason.contains("絞り込みを緩めた"), "{}", step.reason);
    let mut narrowed = catalog.clone();
    assert!(!restrict_catalog_to_step_targets(&mut narrowed, step), "relaxed steps keep the pool");
    assert_eq!(narrowed.words_ja().len(), catalog.words_ja().len());
    assert!(prompts(&catalog, step, 30).len() >= MIN_TARGET_WORDS);
}

#[test]
fn japanese_spelling_shows_the_suggestion_and_prefers_words_with_its_kana() {
    let (_home, catalog) = data();
    let layout = builtin_layout("qwerty").unwrap();
    let mut case = input("ja", Component::Preview);
    case.has_spelling_suggestion = true;
    case.japanese_percentile = Some(1.0);
    let mut menu = build_menu(bundled_catalog().unwrap(), &case, &MenuConfig::default(), &[], &BTreeMap::new()).unwrap();
    let value = LanguageTargets {
        transitions: Vec::new(),
        spelling: Some(SpellingTarget {
            kana: "し".to_owned(),
            current: "si".to_owned(),
            candidate: "shi".to_owned(),
        }),
    };
    apply_step_targets(&mut menu, &targets("ja", value), &catalog, &layout, &Config::default(), Lang::Ja).unwrap();

    let step = primary(&menu, "japanese.spelling");
    assert!(step.focus.contains("し: si → shi"), "{}", step.focus);
    assert!(step.reason.contains("勧めた綴り し: si → shi"), "{}", step.reason);
    for prompt in prompts(&catalog, step, 200) {
        assert!(prompt.reading.as_deref().is_some_and(|reading| reading.contains('し')), "{}", prompt.text);
    }
}

#[test]
fn a_words_step_without_target_transitions_ignores_the_free_play_keys() {
    let (_home, catalog) = data();
    let layout = builtin_layout("qwerty").unwrap();
    let states = BTreeMap::from([("transition.adaptive".to_owned(), level(3))]);
    let mut menu = build_menu(
        bundled_catalog().unwrap(),
        &input("en", Component::Transition),
        &MenuConfig::default(),
        &[],
        &states,
    )
    .unwrap();
    apply_step_targets(&mut menu, &BTreeMap::new(), &catalog, &layout, &Config::default(), Lang::En).unwrap();
    let step = primary(&menu, "transition.adaptive");
    assert_eq!(step.settings["mode.keys"].as_str(), Some(""));
    assert!(step.reason.contains("no target transition yet"), "{}", step.reason);
    assert_eq!(config_for_menu_step(&Config::default(), step).unwrap().mode.keys, "");
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn the_app_draws_a_menu_step_from_the_words_that_contain_its_target() {
    let mut config = Config::default();
    config.session.result_input_delay_ms = 0;
    config.ui.animation_enabled = false;
    let mut app = test_app_at(config, NaiveDate::from_ymd_opt(2026, 11, 18).unwrap());
    let now = Instant::now();
    app.handle_key_at(key(KeyCode::Char('m')), now);
    assert_eq!(app.screen, Screen::Menu);
    {
        let step = &mut app.daily_menu.as_mut().unwrap().steps[1];
        step.prescription_id = "transition.adaptive".to_owned();
        step.mode = "words".to_owned();
        step.settings = BTreeMap::from([
            ("mode.key_filter".to_owned(), toml::Value::String("include".to_owned())),
            ("mode.keys".to_owned(), toml::Value::String("qu".to_owned())),
            (
                "target.transitions".to_owned(),
                toml::Value::Array(vec![toml::Value::String("qu".to_owned())]),
            ),
        ]);
    }
    app.menu_selected = 1;
    app.handle_key_at(key(KeyCode::Enter), now);
    app.handle_key_at(key(KeyCode::Enter), now + Duration::from_millis(10));
    app.on_tick_at(Duration::from_secs(3), now + Duration::from_secs(4)).unwrap();
    assert_eq!(app.screen, Screen::Playing);
    let game = app.game.as_ref().unwrap();
    for prompt in std::iter::once(&game.current_prompt).chain(game.preview_prompts.iter()) {
        assert!(prompt.target.contains("qu"), "{}", prompt.target);
    }
    // The target is not a configuration key and is never applied as one.
    assert_eq!(app.config.mode.keys, "qu");
}
