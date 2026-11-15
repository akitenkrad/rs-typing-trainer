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

fn buffer_rows(terminal: &Terminal<TestBackend>) -> Vec<String> {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect())
        .collect()
}

fn compact_row(row: &str) -> String {
    row.chars().filter(|character| !character.is_whitespace()).collect()
}

fn row_containing(terminal: &Terminal<TestBackend>, needle: &str) -> u16 {
    buffer_rows(terminal)
        .iter()
        .position(|row| row.contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} was not rendered")) as u16
}

fn row_containing_keys(terminal: &Terminal<TestBackend>, keys: &[char]) -> u16 {
    buffer_rows(terminal)
        .iter()
        .position(|row| {
            let positions = keys
                .iter()
                .filter_map(|key| row.chars().position(|character| character == *key))
                .collect::<Vec<_>>();
            positions.len() == keys.len() && positions.windows(2).all(|pair| pair[0] < pair[1])
        })
        .unwrap_or_else(|| panic!("keys {keys:?} were not rendered")) as u16
}

fn has_home_row(terminal: &Terminal<TestBackend>) -> bool {
    buffer_rows(terminal).iter().any(|row| {
        let positions = ['a', 's', 'd', 'f']
            .into_iter()
            .filter_map(|key| row.chars().position(|character| character == key))
            .collect::<Vec<_>>();
        positions.len() == 4 && positions.windows(2).all(|pair| pair[0] < pair[1])
    })
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
                KeyboardView {
                    show,
                    style: "plain",
                    finger_guide: true,
                },
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
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains("Time"), "{text}");
    assert!(text.contains("Word timer"), "{text}");
    assert!(text.chars().any(|character| matches!(
        character,
        '▘' | '▝' | '▖' | '▗' | '▀' | '▄' | '▌' | '▐' | '▚' | '▞' | '▛' | '▜' | '▙' | '▟' | '█'
    )));
    assert!(has_home_row(&terminal), "{text}");
}

#[test]
fn play_screen_orders_keyboard_timer_stats_and_centered_prompt() {
    let mut config = Config::default();
    config.mode.keys = "z".to_owned();
    config.mode.base_keys = "none".to_owned();
    config.mode.random.word_length = 8;
    config.characters.charset = "z".to_owned();
    config.charset_graphemes = vec!["z".to_owned()];
    config.game.word_length = 8;
    config.ui.animation_enabled = false;
    let mut app = test_app(config);
    let now = Instant::now();
    app.handle_key_at(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), now);
    app.handle_key_at(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), now);
    app.on_tick_at(Duration::from_secs(3), now).unwrap();
    app.on_tick_at(Duration::from_millis(1), now).unwrap();

    let terminal = {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| ui::render(frame, &app)).unwrap();
        terminal
    };
    let keyboard_row = row_containing_keys(&terminal, &['z', 'x', 'c']);
    let timer_row = row_containing(&terminal, "Word timer");
    let stats_row = row_containing(&terminal, "Combo");
    let buffer = terminal.backend().buffer();
    let prompt_row = (stats_row + 1..buffer.area.height)
        .find(|y| {
            (0..buffer.area.width).any(|x| {
                matches!(
                    buffer[(x, *y)].symbol(),
                    "▘" | "▝" | "▖" | "▗" | "▀" | "▄" | "▌" | "▐" | "▚" | "▞" | "▛" | "▜" | "▙" | "▟" | "█"
                )
            })
        })
        .unwrap();
    assert!(keyboard_row < timer_row, "keyboard={keyboard_row}, timer={timer_row}");
    assert!(timer_row < stats_row, "timer={timer_row}, stats={stats_row}");
    assert!(stats_row < prompt_row, "stats={stats_row}, prompt={prompt_row}");
    // The prompt is top-aligned right below the metrics (a lowercase glyph
    // may leave its first block row blank).
    assert!(prompt_row <= stats_row + 2, "stats={stats_row}, prompt={prompt_row}");

    let prompt_columns = (0..buffer.area.width)
        .filter(|x| buffer[(*x, prompt_row)].symbol() != " ")
        .collect::<Vec<_>>();
    let prompt_center = f32::from(prompt_columns[0] + prompt_columns[prompt_columns.len() - 1]) / 2.0;
    let screen_center = f32::from(buffer.area.width - 1) / 2.0;
    assert!(
        (prompt_center - screen_center).abs() <= 1.0,
        "prompt center={prompt_center}, screen center={screen_center}"
    );
}

#[test]
fn short_play_screens_hide_the_keyboard_first_and_keep_prompt_and_footer() {
    let mut config = Config::default();
    config.ui.animation_enabled = false;
    let mut app = test_app(config);
    let now = Instant::now();
    app.handle_key_at(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), now);
    app.handle_key_at(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), now);
    app.on_tick_at(Duration::from_secs(3), now).unwrap();
    app.on_tick_at(Duration::from_millis(1), now).unwrap();
    for (height, keyboard_expected) in [(24, true), (20, true), (16, false)] {
        let mut terminal = Terminal::new(TestBackend::new(80, height)).unwrap();
        terminal.draw(|frame| ui::render(frame, &app)).unwrap();
        let output = buffer_text(&terminal);
        let keyboard_present = has_home_row(&terminal);
        assert_eq!(keyboard_present, keyboard_expected, "80x{height}: {output}");
        assert!(output.contains("Word timer"), "80x{height}: {output}");
        assert!(output.contains("Combo"), "80x{height}: {output}");
        assert!(output.chars().any(|character| matches!(
            character,
            '▘' | '▝' | '▖' | '▗' | '▀' | '▄' | '▌' | '▐' | '▚' | '▞' | '▛' | '▜' | '▙' | '▟' | '█'
        )));
        assert!(output.contains("[ESC] Abort"), "80x{height}: {output}");
    }

    app.config.keyboard.show = "hidden".to_owned();
    let mut hidden = Terminal::new(TestBackend::new(80, 24)).unwrap();
    hidden.draw(|frame| ui::render(frame, &app)).unwrap();
    assert!(!buffer_rows(&hidden).iter().any(|row| compact_row(row).contains("asdf")));
    let header_row = row_containing(&hidden, "Time");
    assert_eq!(
        row_containing(&hidden, "Word timer"),
        header_row + 1,
        "hidden keyboard space should collapse"
    );
    assert!(row_containing(&hidden, "Word timer") < row_containing(&hidden, "Combo"));
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
                    style: "plain",
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
