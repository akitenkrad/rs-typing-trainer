use std::collections::HashMap;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;
use rs_typing_trainer::analysis::metrics::{DetailedMetrics, KeyMetric};
use rs_typing_trainer::layout::geometry::geometry;
use rs_typing_trainer::layout::{KeyStroke, builtin_layout};
use rs_typing_trainer::practice::adaptive::FocusTransitionPhase;
use rs_typing_trainer::practice::keyboard_fade::{KeyFade, key_fade_states};
use rs_typing_trainer::ui::keyboard::{FocusTransitionKeys, KeyboardState, KeyboardView};

fn metrics() -> DetailedMetrics {
    DetailedMetrics {
        iki_median_ms: 180.0,
        keys: vec![
            KeyMetric {
                key: "a".into(),
                code: "KeyA".into(),
                correct: 100,
                total: 100,
                accuracy: 1.0,
                iki_n: 100,
                iki_median_ms: 120.0,
                iki_p90_ms: 140.0,
            },
            KeyMetric {
                key: "s".into(),
                code: "KeyS".into(),
                correct: 99,
                total: 100,
                accuracy: 0.99,
                iki_n: 15,
                iki_median_ms: 150.0,
                iki_p90_ms: 180.0,
            },
            KeyMetric {
                key: "d".into(),
                code: "KeyD".into(),
                correct: 80,
                total: 100,
                accuracy: 0.8,
                iki_n: 100,
                iki_median_ms: 120.0,
                iki_p90_ms: 140.0,
            },
        ],
        ..DetailedMetrics::default()
    }
}

fn render(show: &str, fade: HashMap<String, KeyFade>, next: Option<KeyStroke>) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let state = KeyboardState {
        next,
        fade,
        ..KeyboardState::default()
    };
    terminal
        .draw(|frame| {
            rs_typing_trainer::ui::keyboard::render(
                frame,
                Rect::new(0, 0, 120, 8),
                &geometry("ansi").unwrap(),
                &builtin_layout("qwerty").unwrap(),
                &state,
                KeyboardView {
                    show,
                    style: "plain",
                    finger_guide: false,
                },
            )
        })
        .unwrap();
    terminal
}

#[test]
fn auto_dims_then_hides_only_mastered_keys_and_never_hides_next_key() {
    let fade = key_fade_states(&metrics(), 10.0);
    assert_eq!(fade.get("KeyA"), Some(&KeyFade::Hidden));
    assert_eq!(fade.get("KeyS"), Some(&KeyFade::Dim));
    assert_eq!(fade.get("KeyD"), None);

    let without_target = render("auto", fade.clone(), None);
    let buffer = without_target.backend().buffer();
    let s_index = buffer.content().iter().position(|cell| cell.symbol() == "s").unwrap();
    let home_row = s_index as u16 / buffer.area.width;
    assert!(
        !buffer
            .content()
            .iter()
            .enumerate()
            .any(|(index, cell)| cell.symbol() == "a" && index as u16 / buffer.area.width == home_row)
    );

    let terminal = render(
        "auto",
        fade,
        Some(KeyStroke {
            code: "KeyA".into(),
            shift: false,
        }),
    );
    let a = terminal.backend().buffer().content().iter().find(|cell| cell.symbol() == "a").unwrap();
    assert_eq!(a.style().bg, Some(Color::Cyan));
    let s = terminal.backend().buffer().content().iter().find(|cell| cell.symbol() == "s").unwrap();
    assert_eq!(s.style().fg, Some(Color::DarkGray));
    assert!(terminal.backend().buffer().content().iter().any(|cell| cell.symbol() == "d"));
}

#[test]
fn auto_keeps_both_focus_keys_visible_and_next_key_style_wins() {
    let mut fade = HashMap::new();
    fade.insert("KeyA".to_owned(), KeyFade::Hidden);
    fade.insert("KeyS".to_owned(), KeyFade::Hidden);
    let state = KeyboardState {
        next: Some(KeyStroke {
            code: "KeyA".to_owned(),
            shift: false,
        }),
        focus: Some(FocusTransitionKeys {
            from_code: "KeyA".to_owned(),
            to_code: "KeyS".to_owned(),
            phase: FocusTransitionPhase::BeforeFrom,
        }),
        fade,
        ..KeyboardState::default()
    };
    let mut terminal = Terminal::new(TestBackend::new(120, 8)).unwrap();
    terminal
        .draw(|frame| {
            rs_typing_trainer::ui::keyboard::render(
                frame,
                Rect::new(0, 0, 120, 8),
                &geometry("ansi").unwrap(),
                &builtin_layout("qwerty").unwrap(),
                &state,
                KeyboardView {
                    show: "auto",
                    style: "plain",
                    finger_guide: false,
                },
            )
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let from = buffer.content().iter().find(|cell| cell.symbol() == "①").unwrap();
    let to = buffer.content().iter().find(|cell| cell.symbol() == "②").unwrap();
    assert_eq!(from.style().bg, Some(Color::Cyan));
    assert_ne!(to.style().bg, Some(Color::Cyan));
}

#[test]
fn legacy_full_dim_and_hidden_views_are_unchanged() {
    let full = render("full", HashMap::new(), None);
    let dim = render("dim", HashMap::new(), None);
    let hidden = render("hidden", HashMap::new(), None);
    let full_a = full.backend().buffer().content().iter().find(|cell| cell.symbol() == "a").unwrap();
    let dim_a = dim.backend().buffer().content().iter().find(|cell| cell.symbol() == "a").unwrap();
    assert_eq!(full_a.style().fg, Some(Color::White));
    assert_eq!(dim_a.style().fg, Some(Color::DarkGray));
    assert!(hidden.backend().buffer().content().iter().all(|cell| cell.symbol() == " "));
}
