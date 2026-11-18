use std::time::{Duration, Instant};

use chrono::{TimeZone, Utc};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::app::Screen;
use rs_typing_trainer::config::{Config, ModeKind};
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::practice::menu::{AdvanceOutcome, bundled_catalog, evaluate_current_advance};
use rs_typing_trainer::record::{SessionModeParams, SessionRecord};
use rs_typing_trainer::ui;

mod common;
use common::{test_app, test_app_at};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn render(app: &rs_typing_trainer::app::App, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect()
}

fn base_config() -> Config {
    let mut config = Config::default();
    config.session.duration_seconds = 17;
    config.session.preview_items = 2;
    config.session.result_input_delay_ms = 0;
    config.adaptive.drill_ratio = 0.8;
    config.ui.animation_enabled = false;
    config
}

fn open_menu(app: &mut rs_typing_trainer::app::App, now: Instant) {
    app.handle_key_at(key(KeyCode::Char('m')), now);
    assert_eq!(app.screen, Screen::Menu);
}

fn finish_selected_step(app: &mut rs_typing_trainer::app::App, now: Instant) {
    app.handle_key_at(key(KeyCode::Enter), now);
    assert!(matches!(app.screen, Screen::MenuFocus { .. }));
    app.handle_key_at(key(KeyCode::Char('x')), now + Duration::from_millis(10));
    assert!(matches!(app.screen, Screen::Countdown { .. }));
    app.on_tick_at(Duration::from_secs(3), now + Duration::from_secs(4)).unwrap();
    assert_eq!(app.screen, Screen::Playing);
    let seconds = app.config.session.duration_seconds;
    app.on_tick_at(Duration::from_secs(seconds), now + Duration::from_secs(seconds + 5))
        .unwrap();
    assert_eq!(app.screen, Screen::Result);
}

#[test]
fn menu_screen_fits_at_80_by_24_and_shows_rows_and_selected_details() {
    let mut app = test_app_at(base_config(), chrono::NaiveDate::from_ymd_opt(2026, 11, 18).unwrap());
    open_menu(&mut app, Instant::now());
    app.handle_key(key(KeyCode::Down));

    let rows = render(&app, 80, 24);
    let text = rows.join("\n");
    for expected in [
        "TODAY'S MENU",
        "Warm-up",
        "Adaptive transitions",
        "Focus",
        "Graduation condition",
        "Evidence strength",
        "[Enter] Start",
    ] {
        assert!(text.contains(expected), "missing {expected:?}:\n{text}");
    }
    assert_eq!(rows.iter().filter(|row| row.contains('▶')).count(), 1, "{text}");
    assert_eq!(rows.iter().filter(|row| row.contains("[Enter] Start")).count(), 1, "{text}");
}

#[test]
fn selected_reason_is_complete_in_the_detail_and_not_truncated_in_the_step_row() {
    let mut app = test_app_at(base_config(), chrono::NaiveDate::from_ymd_opt(2026, 11, 18).unwrap());
    open_menu(&mut app, Instant::now());
    app.handle_key(key(KeyCode::Down));
    let reason = app.daily_menu.as_ref().unwrap().steps[1].reason.clone();

    let rows = render(&app, 80, 24);
    let text = rows.join("\n");
    let compact = rows
        .iter()
        .map(|row| row.trim_matches('│'))
        .collect::<String>()
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    let expected = format!("Selected because: {reason}")
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    assert!(compact.contains(&expected), "missing complete reason {reason:?}:\n{text}");

    let step_row = rows.iter().find(|row| row.contains("Adaptive transitions")).unwrap();
    assert!(!step_row.contains(&reason), "reason must be in the detail only:\n{text}");
    assert!(!step_row.contains('…'), "the step row should fit without truncating its reason:\n{text}");
}

#[test]
fn menu_step_time_and_level_labels_are_localized_and_fixed_steps_omit_level() {
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 18).unwrap();

    let mut english = test_app_at(base_config(), date);
    open_menu(&mut english, Instant::now());
    let english_fixed = render(&english, 80, 24);
    let english_fixed = english_fixed.iter().find(|row| row.contains("Warm-up")).unwrap();
    assert!(english_fixed.contains("2 min"), "{english_fixed}");
    assert!(!english_fixed.contains("Lv"), "{english_fixed}");
    english.handle_key(key(KeyCode::Down));
    let english_prescription = render(&english, 80, 24);
    let english_prescription = english_prescription.iter().find(|row| row.contains("Adaptive transitions")).unwrap();
    assert!(english_prescription.contains("Lv 1"), "{english_prescription}");

    let mut japanese_config = base_config();
    japanese_config.general.lang = rs_typing_trainer::i18n::Lang::Ja;
    let mut japanese = test_app_at(japanese_config, date);
    open_menu(&mut japanese, Instant::now());
    let japanese_fixed_title = japanese.daily_menu.as_ref().unwrap().steps[0].title.replace(' ', "");
    let japanese_fixed = render(&japanese, 80, 24).into_iter().map(|row| row.replace(' ', "")).collect::<Vec<_>>();
    let japanese_fixed = japanese_fixed.iter().find(|row| row.contains(&japanese_fixed_title)).unwrap();
    assert!(japanese_fixed.contains("2分"), "{japanese_fixed}");
    assert!(!japanese_fixed.contains("min"), "{japanese_fixed}");
    assert!(!japanese_fixed.contains("レベル"), "{japanese_fixed}");
    assert!(!japanese_fixed.contains("L–") && !japanese_fixed.contains("L-"), "{japanese_fixed}");
    japanese.handle_key(key(KeyCode::Down));
    let japanese_prescription_title = japanese.daily_menu.as_ref().unwrap().steps[1].title.replace(' ', "");
    let japanese_prescription = render(&japanese, 80, 24).into_iter().map(|row| row.replace(' ', "")).collect::<Vec<_>>();
    let japanese_prescription = japanese_prescription
        .iter()
        .find(|row| row.contains(&japanese_prescription_title))
        .unwrap();
    assert!(japanese_prescription.contains("レベル1"), "{japanese_prescription}");
    assert!(japanese_prescription.contains("分"), "{japanese_prescription}");
    assert!(!japanese_prescription.contains("min"), "{japanese_prescription}");
}

#[test]
fn menu_rows_show_practice_language_and_display_language_stays_independent() {
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 18).unwrap();
    let mut config = base_config();
    config.menu.languages = Some(vec![rs_typing_trainer::i18n::Lang::Ja]);
    let mut app = test_app_at(config, date);
    let now = Instant::now();
    open_menu(&mut app, now);

    let text = render(&app, 80, 24).join("\n");
    assert!(text.contains("TODAY'S MENU"), "{text}");
    assert!(text.contains(" JA Warm-up"), "{text}");
    assert!(!text.contains("今日のメニュー"), "{text}");
    assert_eq!(app.daily_menu.as_ref().unwrap().steps[0].language, rs_typing_trainer::i18n::Lang::Ja);
    assert!(app.daily_menu.as_ref().unwrap().steps[0].reason.starts_with("JA:"));

    app.handle_key_at(key(KeyCode::Enter), now + Duration::from_millis(10));
    assert_eq!(app.config.adaptive.lang, "ja");
    assert_eq!(app.config.mode.sentence.lang, "ja");
    assert_eq!(app.config.mode.phrase.lang, "ja");
    app.handle_key_at(key(KeyCode::Char('x')), now + Duration::from_millis(20));
    app.on_tick_at(Duration::from_secs(3), now + Duration::from_secs(4)).unwrap();
    assert_eq!(app.screen, Screen::Playing);
    assert!(
        app.game.as_ref().unwrap().current_prompt.reading.is_some(),
        "Japanese menu step used a non-Japanese prompt"
    );
    let duration = app.config.session.duration_seconds;
    app.on_tick_at(Duration::from_secs(duration), now + Duration::from_secs(duration + 5))
        .unwrap();
    assert_eq!(app.screen, Screen::Result);
    assert_eq!(app.config.adaptive.lang, "en");
    assert_eq!(app.config.mode.sentence.lang, "en");
    assert_eq!(app.config.mode.phrase.lang, "en");
}

#[test]
fn japanese_interface_can_show_an_english_practice_menu() {
    let mut config = base_config();
    config.general.lang = rs_typing_trainer::i18n::Lang::Ja;
    config.menu.languages = Some(vec![rs_typing_trainer::i18n::Lang::En]);
    let mut app = test_app_at(config, chrono::NaiveDate::from_ymd_opt(2026, 11, 18).unwrap());
    open_menu(&mut app, Instant::now());

    let text = render(&app, 80, 24).join("\n");
    let compact = text.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    assert!(compact.contains("今日のメニュー"), "{text}");
    assert!(compact.contains("英ウォームアップ"), "{text}");
    assert!(!text.contains("TODAY'S MENU"), "{text}");
    assert!(app.daily_menu.as_ref().unwrap().steps[0].reason.starts_with("英:"));
}

#[test]
fn japanese_ngram_menu_step_keeps_its_mode_and_uses_japanese_prompts() {
    let mut config = base_config();
    config.menu.languages = Some(vec![rs_typing_trainer::i18n::Lang::Ja]);
    let mut app = test_app_at(config, chrono::NaiveDate::from_ymd_opt(2026, 11, 18).unwrap());
    let now = Instant::now();
    open_menu(&mut app, now);
    app.daily_menu.as_mut().unwrap().steps[0].mode = "ngram".to_owned();

    app.handle_key_at(key(KeyCode::Enter), now + Duration::from_millis(10));
    assert_eq!(app.config.mode.kind, ModeKind::Ngram);
    app.handle_key_at(key(KeyCode::Char('x')), now + Duration::from_millis(20));
    app.on_tick_at(Duration::from_secs(3), now + Duration::from_secs(4)).unwrap();

    assert_eq!(app.screen, Screen::Playing);
    assert!(app.game.as_ref().unwrap().current_prompt.reading.is_some());
}

#[test]
fn changing_display_language_rebuilds_a_default_language_menu_on_next_open() {
    let mut app = test_app_at(base_config(), chrono::NaiveDate::from_ymd_opt(2026, 11, 18).unwrap());
    open_menu(&mut app, Instant::now());
    assert_eq!(app.daily_menu.as_ref().unwrap().steps[0].language, rs_typing_trainer::i18n::Lang::En);
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(key(KeyCode::Char('s')));
    for _ in 0..7 {
        app.handle_key(key(KeyCode::Down));
    }
    app.handle_key(key(KeyCode::Right));
    app.handle_key(key(KeyCode::Enter));
    open_menu(&mut app, Instant::now());

    assert_eq!(app.daily_menu.as_ref().unwrap().steps[0].language, rs_typing_trainer::i18n::Lang::Ja);
}

#[test]
fn menu_runs_steps_in_order_restores_settings_and_keeps_completion_marks() {
    let mut app = test_app_at(base_config(), chrono::NaiveDate::from_ymd_opt(2026, 11, 18).unwrap());
    let now = Instant::now();
    open_menu(&mut app, now);

    finish_selected_step(&mut app, now + Duration::from_secs(1));
    assert_eq!(app.config.mode.kind, ModeKind::Random);
    assert_eq!(app.config.session.duration_seconds, 17);
    assert_eq!(app.config.session.preview_items, 2);
    assert!(app.menu_completed.contains(&0));

    let params: serde_json::Value = serde_json::from_str(&app.result_record.as_ref().unwrap().header.mode_params).unwrap();
    assert_eq!(params["menu_prescription_id"], "fixed.warmup");
    assert_eq!(params["menu_step"], 1);
    assert!(
        serde_json::from_value::<SessionModeParams>(params).is_ok(),
        "H5 reader must accept the added keys"
    );

    app.handle_key_at(key(KeyCode::Enter), now + Duration::from_secs(130));
    assert!(matches!(app.screen, Screen::MenuFocus { .. }));
    assert_eq!(app.menu_active_step, Some(1));
    assert_eq!(app.config.mode.kind, ModeKind::Adaptive);
    assert_eq!(app.config.adaptive.drill_ratio, 0.3);

    app.handle_key_at(key(KeyCode::Char('x')), now + Duration::from_secs(131));
    app.on_tick_at(Duration::from_secs(3), now + Duration::from_secs(135)).unwrap();
    assert_eq!(app.screen, Screen::Playing);
    app.handle_key_at(key(KeyCode::Esc), now + Duration::from_secs(136));
    assert_eq!(app.screen, Screen::Menu);
    assert_eq!(app.config.mode.kind, ModeKind::Random);
    assert_eq!(app.config.session.duration_seconds, 17);
    assert_eq!(app.config.session.preview_items, 2);
    assert_eq!(app.config.adaptive.drill_ratio, 0.8);
    assert_eq!(app.menu_completed.iter().copied().collect::<Vec<_>>(), vec![0]);
}

#[test]
fn escape_from_a_menu_result_returns_to_the_menu_and_done_is_visible() {
    let mut config = base_config();
    config.general.lang = rs_typing_trainer::i18n::Lang::Ja;
    let mut app = test_app(config);
    let now = Instant::now();
    open_menu(&mut app, now);
    finish_selected_step(&mut app, now + Duration::from_secs(1));

    app.handle_key_at(key(KeyCode::Esc), now + Duration::from_secs(130));
    assert_eq!(app.screen, Screen::Menu);
    let text = render(&app, 80, 24).join("\n");
    assert!(text.contains('済'), "{text}");
}

fn record_with_accuracy_gate(accuracy_met: bool) -> SessionRecord {
    let mut config = Config::default();
    config.gate.min_accuracy = 0.95;
    let mut session = Session::new(Duration::from_secs(1), Duration::ZERO, Item::exact("a"));
    session.activate(Duration::ZERO);
    if !accuracy_met {
        session.input_char(Duration::from_millis(100), "x");
    }
    session.input_char(Duration::from_millis(200), "a");
    session.tick(Duration::from_secs(1));
    SessionRecord::from_log(&config, session.into_log(), Utc.with_ymd_and_hms(2026, 11, 18, 8, 0, 0).unwrap())
}

#[test]
fn current_advance_evaluation_has_met_not_met_and_cannot_determine_outcomes() {
    let catalog = bundled_catalog().unwrap();
    let gate = &catalog.prescription("accuracy.gate").unwrap().levels[0].advance;
    assert_eq!(evaluate_current_advance(gate, &record_with_accuracy_gate(true)), AdvanceOutcome::Met);
    assert_eq!(evaluate_current_advance(gate, &record_with_accuracy_gate(false)), AdvanceOutcome::NotMet);

    let spelling = &catalog.prescription("japanese.spelling").unwrap().levels[0].advance;
    assert_eq!(
        evaluate_current_advance(spelling, &record_with_accuracy_gate(true)),
        AdvanceOutcome::CannotDetermine
    );
}

#[test]
fn menu_result_adds_the_graduation_condition_and_current_outcome_line() {
    let mut app = test_app(base_config());
    let now = Instant::now();
    open_menu(&mut app, now);
    app.handle_key(key(KeyCode::Down));
    finish_selected_step(&mut app, now + Duration::from_secs(1));

    let text = render(&app, 80, 24).join("\n");
    assert!(text.contains("Today's focus"), "{text}");
    assert!(text.contains("Graduation condition:"), "{text}");
    assert!(text.contains("This session: cannot determine"), "{text}");
    assert!(text.contains("[Enter] Next step"), "{text}");
}

#[test]
fn menu_result_localizes_all_three_current_outcomes() {
    let mut app = test_app(base_config());
    let now = Instant::now();
    open_menu(&mut app, now);
    app.handle_key(key(KeyCode::Down));
    finish_selected_step(&mut app, now + Duration::from_secs(1));

    for (outcome, expected) in [
        (AdvanceOutcome::Met, "This session: met"),
        (AdvanceOutcome::NotMet, "This session: not met"),
        (AdvanceOutcome::CannotDetermine, "This session: cannot determine"),
    ] {
        app.menu_step_outcome = Some(outcome);
        let text = render(&app, 80, 24).join("\n");
        assert!(text.contains(expected), "missing {expected:?}:\n{text}");
    }
}
