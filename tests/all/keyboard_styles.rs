use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Color;
use rs_typing_trainer::config::Config;
use rs_typing_trainer::layout::geometry::{KeyboardGeometry, geometry};
use rs_typing_trainer::layout::{KeyStroke, builtin_layout};
use rs_typing_trainer::ui;
use rs_typing_trainer::ui::keyboard::{KeyFeedback, KeyboardState, KeyboardStyle, KeyboardView, resolve_style};

use crate::common;
use common::test_app;

fn rows(terminal: &Terminal<TestBackend>) -> Vec<String> {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect())
        .collect()
}

fn text(terminal: &Terminal<TestBackend>) -> String {
    rows(terminal).join("\n")
}

fn compact(value: &str) -> String {
    value.chars().filter(|character| !character.is_whitespace()).collect()
}

fn render_keyboard(style: &str, geometry_name: &str, state: KeyboardState, width: u16, height: u16) -> Terminal<TestBackend> {
    let layout_name = if geometry_name == "jis" { "jis-qwerty" } else { "qwerty" };
    let layout = builtin_layout(layout_name).unwrap();
    let physical = geometry(geometry_name).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| {
            ui::keyboard::render(
                frame,
                Rect::new(0, 0, width, height),
                &physical,
                &layout,
                &state,
                KeyboardView {
                    show: "full",
                    style,
                    finger_guide: true,
                },
            )
        })
        .unwrap();
    terminal
}

fn key_width(geometry: &KeyboardGeometry, code: &str) -> u16 {
    geometry.rows.iter().flatten().find(|key| key.code == code).unwrap().width
}

#[test]
fn keyboard_style_defaults_to_keycap_and_accepts_all_supported_values() {
    let default = Config::default();
    assert_eq!(default.keyboard.style, "keycap");

    for style in ["keycap", "raised", "plain"] {
        let source = format!("[keyboard]\nstyle = \"{style}\"\n");
        let config = Config::from_toml(&source).unwrap();
        assert_eq!(config.keyboard.style, style);
    }

    let invalid = Config::from_toml("[keyboard]\nstyle = \"flat\"\n");
    assert!(invalid.is_err());
}

#[test]
fn ansi_rows_are_fifteen_units_and_wide_keys_use_physical_ratios() {
    let physical = geometry("ansi").unwrap();
    for row in &physical.rows {
        assert_eq!(row.iter().map(|key| key.width).sum::<u16>(), 60, "{row:?}");
    }
    assert_eq!(key_width(&physical, "Backspace"), 8);
    assert_eq!(key_width(&physical, "Tab"), 6);
    assert_eq!(key_width(&physical, "CapsLock"), 7);
    assert_eq!(key_width(&physical, "Enter"), 9);
    assert_eq!(key_width(&physical, "ShiftLeft"), 9);
    assert_eq!(key_width(&physical, "ShiftRight"), 11);
    assert_eq!(key_width(&physical, "Space"), 25);
}

#[test]
fn keycap_uses_shared_borders_and_every_row_has_the_same_right_edge() {
    let terminal = render_keyboard("keycap", "ansi", KeyboardState::default(), 120, 14);
    let rendered = text(&terminal);
    for junction in ['┬', '┼', '┴'] {
        assert!(rendered.contains(junction), "missing {junction}:\n{rendered}");
    }
    let face_rows = rows(&terminal).into_iter().filter(|row| row.contains('│')).collect::<Vec<_>>();
    assert_eq!(face_rows.len(), 5, "{rendered}");
    let right_edges = face_rows
        .iter()
        .map(|row| {
            row.chars()
                .enumerate()
                .filter_map(|(x, character)| (character == '│').then_some(x))
                .last()
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert!(right_edges.windows(2).all(|pair| pair[0] == pair[1]), "{right_edges:?}\n{rendered}");
}

#[test]
fn all_styles_render_and_preserve_highlight_feedback_and_pressed_motion() {
    let target = KeyboardState {
        next: Some(KeyStroke {
            code: "KeyA".to_owned(),
            shift: true,
        }),
        ..KeyboardState::default()
    };
    for style in ["keycap", "raised", "plain"] {
        let terminal = render_keyboard(style, "ansi", target.clone(), 120, 14);
        let buffer = terminal.backend().buffer();
        let a = buffer
            .content()
            .iter()
            .find(|cell| cell.symbol() == "a" && cell.style().bg == Some(Color::Cyan))
            .unwrap();
        assert_eq!(a.style().bg, Some(Color::Cyan), "{style}:\n{}", text(&terminal));
        let shift = buffer.content().iter().find(|cell| cell.symbol() == "⇧").unwrap();
        assert_eq!(shift.style().bg, Some(Color::Cyan), "{style}:\n{}", text(&terminal));
    }

    let mistake = KeyboardState {
        last: Some(KeyFeedback {
            pressed_code: "KeyS".to_owned(),
            expected_code: Some("KeyA".to_owned()),
            correct: false,
        }),
        ..KeyboardState::default()
    };
    let mistaken = render_keyboard("keycap", "ansi", mistake.clone(), 120, 14);
    let buffer = mistaken.backend().buffer();
    assert!(
        buffer
            .content()
            .iter()
            .any(|cell| cell.symbol() == "s" && cell.style().bg == Some(Color::Red))
    );
    assert!(
        buffer
            .content()
            .iter()
            .any(|cell| cell.symbol() == "a" && cell.style().bg == Some(Color::Red))
    );

    let resting_keycap = text(&render_keyboard("keycap", "ansi", KeyboardState::default(), 120, 14));
    assert_ne!(text(&mistaken), resting_keycap, "pressed keycap frame must change its border");
    assert!(
        text(&mistaken).contains('━'),
        "pressed keycap must use a heavy border:\n{}",
        text(&mistaken)
    );

    let resting_raised = render_keyboard("raised", "ansi", KeyboardState::default(), 120, 14);
    let pressed_raised = render_keyboard("raised", "ansi", mistake, 120, 14);
    let symbol_y = |terminal: &Terminal<TestBackend>, symbol: &str, background: Color| {
        let buffer = terminal.backend().buffer();
        buffer
            .content()
            .iter()
            .position(|cell| cell.symbol() == symbol && cell.style().bg == Some(background))
            .map(|index| index as u16 / buffer.area.width)
            .unwrap()
    };
    assert_eq!(
        symbol_y(&pressed_raised, "s", Color::Red),
        symbol_y(&resting_raised, "s", Color::Rgb(75, 81, 91)) + 1
    );
    assert!(text(&resting_raised).contains('▀'));
}

#[test]
fn jis_board_includes_its_distinct_keys_and_fifteen_unit_rows() {
    let physical = geometry("jis").unwrap();
    for row in &physical.rows {
        assert_eq!(row.iter().map(|key| key.width).sum::<u16>(), 60, "{row:?}");
    }
    let terminal = render_keyboard("keycap", "jis", KeyboardState::default(), 120, 14);
    let rendered = compact(&text(&terminal));
    for label in ["¥", "ろ", "無変", "変換", "かな", "ENTER"] {
        assert!(rendered.contains(label), "missing {label}:\n{}", text(&terminal));
    }
}

fn playing_terminal(style: &str, width: u16, height: u16) -> Terminal<TestBackend> {
    let mut config = Config::default();
    config.keyboard.style = style.to_owned();
    config.ui.animation_enabled = false;
    let mut app = test_app(config);
    let now = Instant::now();
    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    app.handle_key_at(enter, now);
    app.handle_key_at(enter, now);
    app.on_tick_at(Duration::from_secs(3), now).unwrap();
    app.on_tick_at(Duration::from_millis(1), now).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    terminal
}

#[test]
fn playing_layout_falls_back_without_removing_prompt_or_controls() {
    for (width, height, expected) in [
        (120, 40, KeyboardStyle::Keycap),
        (100, 36, KeyboardStyle::Keycap),
        (80, 24, KeyboardStyle::Keycap),
        (80, 20, KeyboardStyle::Plain),
    ] {
        let terminal = playing_terminal("keycap", width, height);
        let output = text(&terminal);
        match expected {
            KeyboardStyle::Keycap => assert!(output.contains('┬'), "{width}x{height}:\n{output}"),
            KeyboardStyle::Raised => assert!(output.contains('▀'), "{width}x{height}:\n{output}"),
            KeyboardStyle::Plain => {
                assert!(!output.contains('┬') && !output.contains('▀'), "{width}x{height}:\n{output}");
                assert!(compact(&output).contains("asdf"), "{width}x{height}:\n{output}");
            }
        }
        assert!(output.contains("[ESC] Abort"), "{width}x{height}:\n{output}");
        assert!(output.contains("Word timer"), "{width}x{height}:\n{output}");
    }
}

#[test]
fn fallback_chain_checks_raised_plain_and_hidden_boundaries() {
    assert_eq!(resolve_style("keycap", Rect::new(0, 0, 76, 10), true), Some(KeyboardStyle::Raised));
    assert_eq!(resolve_style("keycap", Rect::new(0, 0, 59, 10), true), Some(KeyboardStyle::Plain));
    assert_eq!(resolve_style("keycap", Rect::new(0, 0, 44, 20), true), None);
    assert_eq!(resolve_style("keycap", Rect::new(0, 0, 120, 5), true), None);
}

#[test]
fn key_picker_and_analysis_use_keycap_when_the_terminal_is_large() {
    let mut picker = test_app(Config::default());
    picker.screen = rs_typing_trainer::app::Screen::KeyPicker;
    let mut picker_terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    picker_terminal.draw(|frame| ui::render(frame, &picker)).unwrap();
    assert!(text(&picker_terminal).contains('┬'), "{}", text(&picker_terminal));

    let mut analysis = test_app(Config::default());
    analysis.screen = rs_typing_trainer::app::Screen::Analysis;
    analysis.analysis_tab = rs_typing_trainer::app::ANALYSIS_KEYBOARD_TAB;
    let mut analysis_terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    analysis_terminal.draw(|frame| ui::render(frame, &analysis)).unwrap();
    assert!(text(&analysis_terminal).contains('┬'), "{}", text(&analysis_terminal));
}

#[test]
fn keyboard_text_dumps_cover_both_report_sizes() {
    for style in ["keycap", "raised", "plain"] {
        for (width, height) in [(80, 24), (120, 40)] {
            let terminal = render_keyboard(style, "ansi", KeyboardState::default(), width, height);
            let dump = rows(&terminal)
                .into_iter()
                .filter(|row| !row.trim().is_empty())
                .map(|row| row.trim_end().to_owned())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(!dump.is_empty());
            println!("--- {style} {width}x{height} ---\n{dump}");
        }
    }
}
