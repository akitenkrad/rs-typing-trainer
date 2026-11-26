use std::time::Duration;

use chrono::{TimeZone, Utc};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use rs_typing_trainer::app::{ResultComparison, Screen};
use rs_typing_trainer::config::Config;
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::graphics::{CellPixels, GraphicsDetection, GraphicsMode, GraphicsReason};
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::metrics::stats::GameResult;
use rs_typing_trainer::practice::menu::AdvanceOutcome;
use rs_typing_trainer::record::SessionRecord;
use rs_typing_trainer::ui;

use crate::common;

fn result_record(config: &Config) -> SessionRecord {
    let layout = rs_typing_trainer::layout::builtin_layout("qwerty").unwrap();
    let mut session = Session::new(Duration::from_secs(10), Duration::ZERO, Item::exact("as"));
    session.activate(Duration::ZERO);
    session.input_char_with_code(Duration::from_millis(180), "x", layout.stroke_for('x').map(|stroke| stroke.code));
    session.input_char_with_code(Duration::from_millis(260), "a", layout.stroke_for('a').map(|stroke| stroke.code));
    session.input_char_with_code(Duration::from_millis(420), "s", layout.stroke_for('s').map(|stroke| stroke.code));
    session.advance(Item::exact("df"), Duration::from_millis(500));
    session.activate(Duration::from_millis(600));
    session.input_char_with_code(Duration::from_millis(750), "d", layout.stroke_for('d').map(|stroke| stroke.code));
    session.input_char_with_code(Duration::from_millis(1_050), "f", layout.stroke_for('f').map(|stroke| stroke.code));
    session.advance(Item::exact("jk"), Duration::from_millis(1_200));
    session.activate(Duration::from_millis(1_300));
    session.input_char_with_code(Duration::from_millis(1_750), "j", layout.stroke_for('j').map(|stroke| stroke.code));
    session.input_char_with_code(Duration::from_millis(2_100), "k", layout.stroke_for('k').map(|stroke| stroke.code));
    session.tick(Duration::from_secs(10));
    let mut record = SessionRecord::from_log(config, session.into_log(), Utc.with_ymd_and_hms(2026, 11, 16, 8, 0, 0).unwrap());
    record.header.id = Some(22);
    record
}

fn result_app(graphics: GraphicsMode) -> common::TestApp {
    let config = Config::default();
    let record = result_record(&config);
    let result = GameResult::from_log(&config, &record.log);
    let mut app = common::test_app(config);
    app.screen = Screen::Result;
    app.last_result = Some(result);
    app.last_save_id = Some(22);
    app.result_record = Some(record);
    app.result_comparison = ResultComparison {
        previous_wpm_delta: Some(3.0),
        previous_effective_wpm_delta: None,
        previous_cps_delta: Some(-0.2),
        previous_accuracy_delta: Some(0.01),
        best_wpm: Some(60.0),
        best_cps: Some(6.0),
        best_accuracy: Some(1.0),
        best_wpm_delta: Some(-2.0),
        best_cps_delta: Some(-0.4),
        best_accuracy_delta: Some(-0.02),
        recent_wpm: vec![45.0, 51.0, 58.0],
    };
    app.result_insights = vec!["Keep the left hand relaxed.".into()];
    app.set_graphics_detection(GraphicsDetection {
        mode: graphics,
        cell_pixels: Some(CellPixels::new(9, 20)),
        reason: if graphics == GraphicsMode::Off {
            GraphicsReason::Disabled
        } else {
            GraphicsReason::Explicit(graphics)
        },
    });
    app
}

fn render(app: &rs_typing_trainer::app::App, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    terminal.backend().buffer().clone()
}

fn rows(buffer: &Buffer) -> Vec<String> {
    (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect()
}

#[test]
fn image_result_renders_all_four_visual_sections_at_150_by_40() {
    let app = result_app(GraphicsMode::Kitty);
    let rendered = rows(&render(&app, 150, 40));
    let text = rendered.join("\n");

    for expected in [
        "GAME RESULT",
        "Keystroke speed by item",
        "Reaction Time Distribution",
        "Missed keys (expected)",
        "This session's observation",
        "Saved to DB",
        "Restart",
    ] {
        assert!(text.contains(expected), "missing {expected:?}:\n{text}");
    }
    assert_eq!(rendered.iter().filter(|row| row.contains("Saved to DB")).count(), 1);
    assert_eq!(rendered.iter().filter(|row| row.contains("Restart")).count(), 1);
}

#[test]
fn image_result_uses_the_available_height_at_160_by_50() {
    let rendered = rows(&render(&result_app(GraphicsMode::Kitty), 160, 50));
    let hint_row = rendered.iter().position(|row| row.contains("Restart")).unwrap();
    let blank_rows_after_hint = rendered[hint_row + 1..rendered.len() - 1]
        .iter()
        .filter(|row| row.chars().skip(1).take(158).all(char::is_whitespace))
        .count();

    assert!(blank_rows_after_hint <= 2, "{blank_rows_after_hint} blank rows remain after the hint");
}

#[test]
fn image_keyboard_title_explains_that_counts_are_expected_keys_in_both_languages() {
    let english = rows(&render(&result_app(GraphicsMode::Kitty), 150, 40)).join("\n");
    assert!(english.contains("Missed keys (expected)"));

    let mut japanese = result_app(GraphicsMode::Kitty);
    japanese.config.general.lang = Lang::Ja;
    let japanese = rows(&render(&japanese, 150, 40)).join("\n");
    assert!(japanese.replace(' ', "").contains("誤打したキー（打つべきキー）"));
}

#[test]
fn text_result_keeps_reaction_distribution_next_to_insight_and_separates_bars() {
    let app = result_app(GraphicsMode::Off);
    let rendered = rows(&render(&app, 150, 40));
    let text = rendered.join("\n");
    assert!(text.contains("Reaction Time Distribution"), "{text}");
    assert!(text.contains("This session's observation"), "{text}");
    assert!(text.contains("Accuracy was 85.7%."), "{text}");
    assert!(text.contains("Accuracy was 85.7%. → Slow down a little and confirm"), "{text}");
    assert!(text.contains("each keystroke."), "the wrapped action must remain visible:\n{text}");
    let reaction_row = rendered.iter().position(|row| row.contains("Reaction Time Distribution")).unwrap();
    let bar_rows = rendered[..reaction_row]
        .iter()
        .enumerate()
        .filter_map(|(index, row)| row.contains('▆').then_some(index))
        .collect::<Vec<_>>();
    assert!(
        bar_rows.windows(2).all(|pair| pair[1] > pair[0] + 1),
        "bars touch vertically: {bar_rows:?}\n{text}"
    );
}

#[test]
fn result_is_non_overlapping_at_80_by_24_with_and_without_images() {
    for graphics in [GraphicsMode::Kitty, GraphicsMode::Off] {
        let rendered = rows(&render(&result_app(graphics), 80, 24));
        let text = rendered.join("\n");
        assert!(text.contains("GAME RESULT"), "{graphics:?}: {text}");
        assert!(text.contains("Restart"), "{graphics:?}: {text}");
        assert!(text.contains("[D] Review"), "{graphics:?}: {text}");
        assert_eq!(rendered.iter().filter(|row| row.contains("Restart")).count(), 1, "{graphics:?}: {text}");
        assert_eq!(
            rendered.iter().filter(|row| row.contains("Saved to DB")).count(),
            1,
            "{graphics:?}: {text}"
        );
    }
}

#[test]
fn menu_result_without_a_graduation_condition_keeps_only_the_navigation_hint() {
    let mut app = result_app(GraphicsMode::Off);
    app.menu_active_step = Some(0);
    app.menu_step_outcome = Some(AdvanceOutcome::CannotDetermine);

    let rendered = rows(&render(&app, 80, 24));
    let text = rendered.join("\n");
    assert!(!text.contains("Graduation condition:"), "{text}");
    assert!(!text.contains("This session:"), "{text}");
    assert!(text.contains("[Enter] Next step"), "{text}");
    assert!(text.contains("[Esc] Menu"), "{text}");
    assert_eq!(rendered.iter().filter(|row| row.contains("[Enter] Next step")).count(), 1, "{text}");
}

#[test]
fn image_keyboard_reports_unmapped_mistypes_and_keeps_the_zero_mistype_message() {
    let mut unmapped = result_app(GraphicsMode::Kitty);
    let result = unmapped.last_result.as_mut().unwrap();
    result.metrics.mistypes_total = 6;
    result.metrics.mistypes_by_char.clear();
    result.metrics.mistypes_by_char.insert("a".into(), 2);
    result.metrics.mistypes_by_char.insert("し".into(), 4);
    let text = rows(&render(&unmapped, 150, 40)).join("\n");
    assert!(text.contains("other 4"), "{text}");

    let mut perfect = result_app(GraphicsMode::Kitty);
    let result = perfect.last_result.as_mut().unwrap();
    result.metrics.mistypes_total = 0;
    result.metrics.mistypes_by_char.clear();
    let text = rows(&render(&perfect, 150, 40)).join("\n");
    assert!(text.contains("no mistypes"), "{text}");
}
