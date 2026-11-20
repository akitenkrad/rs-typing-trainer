//! MYTASK-3629: the warm-up is one minute. The introductory menu still adds up
//! to the configured length, a personalized menu gives the spare minute to
//! primary A within the step limits, and the warm-up can be played on its own
//! from the mode-select screen.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use chrono::NaiveDate;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::analysis::grades::{Component, ComponentGrade, GradeSource, GradeStatus, LanguageGrade, Level, OverallGrade};
use rs_typing_trainer::app::{App, Screen};
use rs_typing_trainer::config::{Config, DrillCell, DrillSkill, MenuConfig, ModeKind, PromptUnit};
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::practice::menu::{
    DailyMenu, MAX_LONG_TEXT_MINUTES, MAX_STEP_MINUTES, MenuBuildInput, MenuRole, PrescriptionState, build_menu, bundled_catalog, is_long_text_mode,
    level_minutes, max_step_minutes,
};
use rs_typing_trainer::practice::reading_report::ReadingReportKind;
use rs_typing_trainer::record::db;
use rs_typing_trainer::ui;

use crate::common::test_app_at;

fn personalized_input(language: &str, weakest: Component) -> MenuBuildInput {
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
    MenuBuildInput {
        as_of: NaiveDate::from_ymd_opt(2026, 11, 19).unwrap(),
        language: language.to_owned(),
        grade: Some(LanguageGrade {
            language: language.to_owned(),
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
        }),
        logged_keystrokes: 5_000,
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

fn personalized(language: &str, weakest: Component, level: u8, minutes: u8) -> DailyMenu {
    let catalog = bundled_catalog().unwrap();
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
    build_menu(
        catalog,
        &personalized_input(language, weakest),
        &MenuConfig {
            minutes,
            ..MenuConfig::default()
        },
        &[],
        &states,
    )
    .unwrap()
}

fn new_player(language: &str) -> MenuBuildInput {
    let mut input = personalized_input(language, Component::Accuracy);
    input.grade = None;
    input.logged_keystrokes = 1_999;
    input
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
    for step in &menu.steps {
        let limit = if is_long_text_mode(&step.mode) {
            MAX_LONG_TEXT_MINUTES
        } else {
            MAX_STEP_MINUTES
        };
        assert!(
            step.minutes <= limit,
            "{} ({}) is {} minutes",
            step.prescription_id,
            step.mode,
            step.minutes
        );
    }
}

fn minutes_of(menu: &DailyMenu) -> Vec<u8> {
    menu.steps.iter().map(|step| step.minutes).collect()
}

#[test]
fn the_catalog_warm_up_is_one_minute() {
    let catalog = bundled_catalog().unwrap();
    let warmup = catalog.fixed_steps.iter().find(|step| step.role == MenuRole::Warmup).unwrap();
    assert_eq!(warmup.minutes, 1);
    assert_eq!(warmup.mode, "words");
}

#[test]
fn introductory_menus_add_up_to_the_configured_length_with_a_one_minute_warm_up() {
    let expected = [(10, vec![1, 4, 3, 2]), (15, vec![1, 5, 4, 3, 2]), (20, vec![1, 5, 5, 4, 3, 2])];
    for language in ["en", "ja"] {
        for (minutes, shape) in &expected {
            let menu = introductory(language, *minutes);
            assert_within_limits(&menu);
            assert_eq!(menu.steps[0].role, MenuRole::Warmup);
            assert_eq!(menu.steps[0].minutes, 1, "{language} {minutes}");
            assert_eq!(menu.total_minutes(), u32::from(*minutes), "{language} {minutes}");
            assert_eq!(&minutes_of(&menu), shape, "{language} {minutes}");
        }
    }
    // English alternates the adaptive and n-gram drills; Japanese stays on
    // the adaptive drill.
    let transitions = |menu: &DailyMenu| {
        menu.steps
            .iter()
            .filter(|step| step.role == MenuRole::PrimaryA)
            .map(|step| step.prescription_id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        transitions(&introductory("en", 20)),
        ["transition.adaptive", "transition.ngram", "transition.adaptive"]
    );
    assert!(transitions(&introductory("ja", 20)).iter().all(|id| id == "transition.adaptive"));
}

#[test]
fn personalized_menus_give_the_spare_warm_up_minute_to_primary_a_within_the_limits() {
    let catalog = bundled_catalog().unwrap();
    for language in ["en", "ja"] {
        for weakest in Component::all().iter().copied() {
            for level in 1..=3 {
                for minutes in [10, 15, 20] {
                    let menu = personalized(language, weakest, level, minutes);
                    assert!(!menu.introductory);
                    assert_within_limits(&menu);
                    assert_eq!(menu.steps[0].role, MenuRole::Warmup);
                    assert_eq!(menu.steps[0].minutes, 1);
                    assert!(menu.total_minutes() <= u32::from(minutes));
                    let primary = menu.steps.iter().find(|step| step.role == MenuRole::PrimaryA).unwrap();
                    let row = catalog.prescription(&primary.prescription_id).unwrap();
                    let catalog_level = &row.levels[usize::from(primary.level.unwrap()) - 1];
                    let frame = match minutes {
                        10 => 4,
                        20 => 7,
                        _ => 5,
                    };
                    let before = level_minutes(catalog_level, frame);
                    let expected = if before < max_step_minutes(&primary.mode) { before + 1 } else { before };
                    assert_eq!(
                        primary.minutes, expected,
                        "{language} {weakest:?} level {level} {minutes} min: {}",
                        primary.prescription_id
                    );
                }
            }
        }
    }
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn render(app: &App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// A configuration whose words settings differ from the warm-up's.
fn custom_config(lang: Lang) -> Config {
    let mut config = Config::default();
    config.general.lang = lang;
    config.session.duration_seconds = 180;
    config.session.result_input_delay_ms = 0;
    config.pacer.enabled = true;
    config.gate.min_accuracy = 0.9;
    config.display.prompt_style = "normal".to_owned();
    config.ui.animation_enabled = false;
    config
}

fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 19).unwrap()
}

/// Title → individual training (MYTASK-3638: the warm-up sits above the
/// table and starts with `W`).
fn select_warmup(app: &mut App, now: Instant) {
    app.handle_key_at(key(KeyCode::Enter), now);
    assert_eq!(app.screen, Screen::ModeSelect);
}

fn start_warmup(app: &mut App, now: Instant) {
    app.handle_key_at(key(KeyCode::Char('w')), now);
    assert!(matches!(app.screen, Screen::Countdown { .. }), "{:?}", app.screen);
    app.on_tick_at(Duration::from_secs(3), now).unwrap();
    assert_eq!(app.screen, Screen::Playing);
}

#[test]
fn the_mode_select_warm_up_starts_a_one_minute_words_session_without_the_pacer() {
    let mut app = test_app_at(custom_config(Lang::En), date());
    let now = Instant::now();
    select_warmup(&mut app, now);
    let screen = render(&app, 80, 24);
    assert!(screen.contains("[W] 1-minute warm-up"), "{screen}");
    assert!(screen.contains("[L] Practice language: English"), "{screen}");
    assert!(!screen.contains("[P]"), "the pacer is the speed row of the table now:\n{screen}");

    start_warmup(&mut app, now);
    assert!(app.warmup_active());
    assert_eq!(app.config.mode.kind, ModeKind::Words);
    assert_eq!(app.config.session.duration_seconds, 60);
    assert!(!app.config.pacer.enabled);
    assert_eq!(app.config.pacer.target_wpm, None);
    assert_eq!(app.config.gate.min_accuracy, 0.0);
    let game = app.game.as_ref().unwrap();
    assert!(game.current_prompt.reading.is_none(), "an English warm-up has no reading");
    assert_eq!(app.reading_report_kind(), None);

    // The session ends after one minute and is recorded as `words`, outside
    // any menu run.
    app.on_tick_at(Duration::from_secs(61), now + Duration::from_secs(61)).unwrap();
    assert_eq!(app.screen, Screen::Result);
    let record = app.result_record.as_ref().unwrap();
    assert_eq!(record.header.mode, "words");
    assert!(!app.warmup_active());
    let db_path = app.config.storage.db_path.clone().unwrap();
    assert!(db::load_all_menu_runs(&db_path).unwrap().is_empty(), "no menu run is recorded");

    // The configuration is back to the user's own.
    assert_eq!(app.config.mode.kind, ModeKind::Random);
    assert_eq!(app.config.session.duration_seconds, 180);
    assert!(app.config.pacer.enabled);
    assert_eq!(app.config.gate.min_accuracy, 0.9);

    // A retry from the result screen plays the warm-up again.
    let later = now + Duration::from_secs(70);
    app.handle_key_at(key(KeyCode::Enter), later);
    assert!(matches!(app.screen, Screen::Countdown { .. }));
    assert!(app.warmup_active());
    assert_eq!(app.config.session.duration_seconds, 60);
    app.on_tick_at(Duration::from_secs(3), later).unwrap();
    assert_eq!(app.screen, Screen::Playing);
    app.on_tick_at(Duration::from_secs(61), later + Duration::from_secs(61)).unwrap();
    assert_eq!(app.screen, Screen::Result);
    assert_eq!(app.config.session.duration_seconds, 180);
    assert!(app.config.pacer.enabled);
    assert!(db::load_all_menu_runs(&db_path).unwrap().is_empty());
}

#[test]
fn ordinary_words_after_a_warm_up_keep_the_users_settings() {
    let mut app = test_app_at(custom_config(Lang::En), date());
    let now = Instant::now();
    select_warmup(&mut app, now);
    start_warmup(&mut app, now);
    app.on_tick_at(Duration::from_secs(61), now + Duration::from_secs(61)).unwrap();
    assert_eq!(app.screen, Screen::Result);

    // Back to the title, then the ordinary `words` mode.
    app.screen = Screen::Title;
    let later = now + Duration::from_secs(70);
    app.handle_key_at(key(KeyCode::Char(' ')), later);
    assert_eq!(app.screen, Screen::ModeSelect);
    assert_eq!(app.drill_cell, DrillCell::new(DrillSkill::Normal, PromptUnit::Char));
    app.handle_key_at(key(KeyCode::Right), later);
    app.handle_key_at(key(KeyCode::Right), later);
    assert_eq!(app.drill_cell, DrillCell::new(DrillSkill::Normal, PromptUnit::Word));
    app.handle_key_at(key(KeyCode::Enter), later);
    assert_eq!(app.screen, Screen::KeyPicker);
    app.handle_key_at(key(KeyCode::Enter), later);
    app.on_tick_at(Duration::from_secs(3), later).unwrap();
    assert_eq!(app.screen, Screen::Playing);
    assert!(!app.warmup_active());
    assert_eq!(app.config.mode.kind, ModeKind::Words);
    assert_eq!(app.config.session.duration_seconds, 180);
    // The «normal» row plays without the pacer (the speed row has it);
    // the user's own gate stays.
    assert!(!app.config.pacer.enabled);
    assert!(app.config.pacer.target_wpm.is_none());
    assert_eq!(app.config.gate.min_accuracy, 0.9);
}

#[test]
fn aborting_a_warm_up_restores_the_users_settings() {
    let mut app = test_app_at(custom_config(Lang::En), date());
    let now = Instant::now();
    select_warmup(&mut app, now);
    start_warmup(&mut app, now);
    app.handle_key_at(key(KeyCode::Esc), now + Duration::from_secs(5));
    app.handle_key_at(key(KeyCode::Esc), now + Duration::from_millis(5_100));
    assert_eq!(app.screen, Screen::Title);
    assert!(!app.warmup_active());
    assert_eq!(app.config.session.duration_seconds, 180);
    assert!(app.config.pacer.enabled);

    // The same during the countdown.
    let later = now + Duration::from_secs(20);
    app.title_selected = 1;
    select_warmup(&mut app, later);
    app.handle_key_at(key(KeyCode::Enter), later);
    assert!(matches!(app.screen, Screen::Countdown { .. }));
    app.handle_key_at(key(KeyCode::Esc), later);
    app.handle_key_at(key(KeyCode::Esc), later + Duration::from_millis(100));
    assert_eq!(app.screen, Screen::Title);
    assert!(!app.warmup_active());
    assert_eq!(app.config.session.duration_seconds, 180);
}

#[test]
fn a_japanese_warm_up_shows_surface_kana_and_romaji_rows() {
    let mut app = test_app_at(custom_config(Lang::Ja), date());
    let now = Instant::now();
    select_warmup(&mut app, now);
    app.handle_key_at(key(KeyCode::Char('L')), now);
    assert_eq!(app.practice_lang, Lang::Ja);
    let screen = render(&app, 80, 24).replace(' ', "");
    assert!(screen.contains("[W]1分のウォームアップ"), "{screen}");
    assert!(screen.contains("[L]練習言語:日本語"), "{screen}");

    start_warmup(&mut app, now);
    assert_eq!(app.config.mode.kind, ModeKind::Words);
    assert_eq!(app.config.session.duration_seconds, 60);
    assert!(!app.config.pacer.enabled);
    let game = app.game.as_ref().unwrap();
    let reading = game.current_prompt.reading.clone().expect("Japanese prompt");
    let surface = game.current_prompt.text.clone();
    assert_eq!(app.reading_report_kind(), Some(ReadingReportKind::Romaji));

    let text = render(&app, 80, 24);
    // Wide characters are followed by a blank cell in the test buffer.
    let compact = text.replace(' ', "");
    for label in ["表記", "かな", "ローマ字", "[Ctrl+R/F2]読みが違う"] {
        assert!(compact.contains(label), "missing {label:?}:\n{text}");
    }
    assert!(compact.contains(&format!("表記{surface}")), "surface {surface:?} not shown:\n{text}");
    assert!(compact.contains(&format!("かな{reading}")), "kana {reading:?} not shown:\n{text}");

    app.on_tick_at(Duration::from_secs(61), now + Duration::from_secs(61)).unwrap();
    assert_eq!(app.screen, Screen::Result);
    assert_eq!(app.result_record.as_ref().unwrap().header.mode, "words");
    assert!(!app.warmup_active());
    let db_path = app.config.storage.db_path.clone().unwrap();
    assert!(db::load_all_menu_runs(&db_path).unwrap().is_empty());
}
