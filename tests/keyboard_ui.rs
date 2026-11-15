use std::collections::HashMap;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Color;
use rs_typing_trainer::config::Config;
use rs_typing_trainer::layout::geometry::geometry;
use rs_typing_trainer::layout::{KeyStroke, builtin_layout};
use rs_typing_trainer::ui;
use rs_typing_trainer::ui::keyboard::{KeyboardState, KeyboardView};

mod common;
use common::test_app;

fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<Vec<_>>()
        .join("")
}

fn render_keyboard(layout_name: &str, show: &str, next: Option<KeyStroke>, width: u16) -> Terminal<TestBackend> {
    let layout = builtin_layout(layout_name).unwrap();
    let geometry = geometry("ansi").unwrap();
    let state = KeyboardState {
        next,
        last: None,
        heatmap: HashMap::new(),
        fade: HashMap::new(),
        unlock: None,
    };
    let mut terminal = Terminal::new(TestBackend::new(width, 10)).unwrap();
    terminal
        .draw(|frame| {
            ui::keyboard::render(
                frame,
                Rect::new(0, 0, width, 10),
                &geometry,
                &layout,
                &state,
                KeyboardView { show, finger_guide: true },
            )
        })
        .unwrap();
    terminal
}

#[test]
fn next_key_and_shift_are_highlighted_by_cell_style() {
    let terminal = render_keyboard(
        "qwerty",
        "full",
        Some(KeyStroke {
            code: "KeyA".to_owned(),
            shift: true,
        }),
        120,
    );
    let buffer = terminal.backend().buffer();
    let a = buffer.content().iter().find(|cell| cell.symbol() == "a").unwrap();
    assert_eq!(a.style().bg, Some(Color::Cyan));
    let shift = buffer.content().iter().find(|cell| cell.symbol() == "⇧").unwrap();
    assert_eq!(shift.style().bg, Some(Color::Cyan));
}

#[test]
fn switching_layout_changes_the_home_row() {
    let qwerty = buffer_text(&render_keyboard("qwerty", "full", None, 120));
    let dvorak = buffer_text(&render_keyboard("dvorak", "full", None, 120));
    let compact = |text: &str| text.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    assert!(compact(&qwerty).contains("asdf"), "{qwerty}");
    assert!(compact(&dvorak).contains("aoeu"), "{dvorak}");
}

#[test]
fn hidden_keyboard_does_not_draw() {
    let terminal = render_keyboard("qwerty", "hidden", None, 80);
    assert!(terminal.backend().buffer().content().iter().all(|cell| cell.symbol() == " "));
}

#[test]
fn play_screen_and_keyboard_fit_at_eighty_by_twenty_four() {
    let mut config = Config::default();
    config.ui.animation_enabled = false;
    let mut app = test_app(config);
    let now = Instant::now();
    app.handle_key_at(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), now);
    app.handle_key_at(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), now);
    app.on_tick_at(Duration::from_secs(3), now).unwrap();
    app.on_tick_at(Duration::from_millis(1), now).unwrap();
    let word = app.game.as_ref().unwrap().session.current_text().to_owned();

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains("Time"), "{text}");
    assert!(text.contains("Word timer"), "{text}");
    let compact: String = text.chars().filter(|character| !character.is_whitespace()).collect();
    assert!(compact.contains(&word), "{text}");
    assert!(compact.contains("asdf"), "{text}");
}

#[test]
fn heatmap_changes_key_color_intensity() {
    let layout = builtin_layout("qwerty").unwrap();
    let geometry = geometry("ansi").unwrap();
    let mut heatmap = HashMap::new();
    heatmap.insert("KeyA".to_owned(), 1.0);
    let state = KeyboardState {
        next: None,
        last: None,
        heatmap,
        fade: HashMap::new(),
        unlock: None,
    };
    let mut terminal = Terminal::new(TestBackend::new(120, 10)).unwrap();
    terminal
        .draw(|frame| {
            ui::keyboard::render_heatmap(
                frame,
                frame.area(),
                &geometry,
                &layout,
                &state,
                KeyboardView {
                    show: "full",
                    finger_guide: false,
                },
            )
        })
        .unwrap();
    let a = terminal.backend().buffer().content().iter().find(|cell| cell.symbol() == "a").unwrap();
    assert_ne!(a.style().bg, Some(Color::Reset));
}

#[test]
fn top_row_starts_to_the_right_of_number_row() {
    let terminal = render_keyboard("qwerty", "full", None, 120);
    let buffer = terminal.backend().buffer();
    let digit_one_x = buffer
        .content()
        .iter()
        .enumerate()
        .find(|(_, cell)| cell.symbol() == "1")
        .map(|(index, _)| index as u16 % buffer.area.width)
        .unwrap();
    let q_x = buffer
        .content()
        .iter()
        .enumerate()
        .find(|(_, cell)| cell.symbol() == "q")
        .map(|(index, _)| index as u16 % buffer.area.width)
        .unwrap();

    assert!(q_x > digit_one_x, "KeyQ x={q_x}, Digit1 x={digit_one_x}");
}
