use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;
use rs_typing_trainer::config::{Config, ModeKind};
use rs_typing_trainer::engine::modes::GeneratedPrompt;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::practice::adaptive::{FocusReason, FocusTransitionPhase, TransitionCost, focus_transition_at};
use rs_typing_trainer::ui;

use crate::common;
use common::test_app;

#[test]
fn focus_transition_tracks_before_and_after_the_from_key() {
    let focus = [('g', 'h')];

    assert_eq!(
        focus_transition_at(&focus, "ghost", 0),
        Some(('g', 'h', FocusTransitionPhase::BeforeFrom))
    );
    assert_eq!(focus_transition_at(&focus, "ghost", 1), Some(('g', 'h', FocusTransitionPhase::AfterFrom)));
    assert_eq!(focus_transition_at(&focus, "ghost", 2), None);
}

#[test]
fn focus_transition_uses_the_first_match_in_the_remaining_prompt() {
    let focus = [('o', 's'), ('g', 'h'), ('h', 'o')];

    assert_eq!(
        focus_transition_at(&focus, "ghost", 0),
        Some(('g', 'h', FocusTransitionPhase::BeforeFrom))
    );
}

#[test]
fn focus_transition_returns_none_for_empty_and_past_end_inputs() {
    assert_eq!(focus_transition_at(&[], "ghost", 0), None);
    assert_eq!(focus_transition_at(&[('g', 'h')], "", 0), None);
    assert_eq!(focus_transition_at(&[('g', 'h')], "ghost", 4), None);
    assert_eq!(focus_transition_at(&[('g', 'h')], "ghost", 5), None);
    assert_eq!(focus_transition_at(&[('g', 'h')], "ghost", usize::MAX), None);
}

#[test]
fn focus_transition_cursor_is_a_character_index() {
    assert_eq!(
        focus_transition_at(&[('g', 'h')], "éghost", 1),
        Some(('g', 'h', FocusTransitionPhase::BeforeFrom))
    );
    assert_eq!(
        focus_transition_at(&[('g', 'h')], "éghost", 2),
        Some(('g', 'h', FocusTransitionPhase::AfterFrom))
    );
}

#[test]
fn focus_transition_preserves_case() {
    assert_eq!(focus_transition_at(&[('g', 'h')], "Ghost", 0), None);
    assert_eq!(
        focus_transition_at(&[('G', 'h')], "Ghost", 0),
        Some(('G', 'h', FocusTransitionPhase::BeforeFrom))
    );
}

#[test]
fn adaptive_playing_marks_focus_keys_and_uses_a_short_legend() {
    let app = adaptive_playing_app("auto");
    let terminal = render(&app);
    let output = rows(&terminal).join("\n");
    let buffer = terminal.backend().buffer();

    let from = buffer
        .content()
        .iter()
        .find(|cell| cell.symbol() == "①")
        .unwrap_or_else(|| panic!("focus from key marker was not rendered:\n{output}"));
    let to = buffer
        .content()
        .iter()
        .find(|cell| cell.symbol() == "②")
        .unwrap_or_else(|| panic!("focus to key marker was not rendered:\n{output}"));
    assert!(!matches!(from.style().bg, None | Some(Color::Reset)));
    assert!(!matches!(to.style().bg, None | Some(Color::Reset)));
    assert_ne!(from.style().bg, Some(Color::Cyan), "focus must differ from the next-key color");
    assert_ne!(to.style().bg, Some(Color::Cyan), "focus must differ from the next-key color");
    assert_ne!(from.style().bg, to.style().bg, "from and to keys need distinguishable colors");

    let legend = rows(&terminal)
        .into_iter()
        .find(|row| row.contains('→'))
        .unwrap_or_else(|| panic!("focus legend missing:\n{output}"));
    assert!(legend.contains('h') && legend.contains('o'), "{legend}");
    assert!(!legend.contains("Slower than your target"), "{legend}");
    assert!(!output.contains("Slower than your target"), "{output}");
}

#[test]
fn adaptive_playing_keeps_focus_reasons_when_the_keyboard_is_hidden() {
    let app = adaptive_playing_app("hidden");
    let terminal = render(&app);
    let output = rows(&terminal).join("\n");
    let focus_row = rows(&terminal)
        .into_iter()
        .find(|row| row.contains("Slower than your target"))
        .unwrap_or_else(|| panic!("reasoned focus row missing:\n{output}"));

    assert!(focus_row.contains('→'), "{focus_row}");
    assert!(focus_row.contains('h') && focus_row.contains('o'), "{focus_row}");
    assert!(
        !output.contains('①') && !output.contains('②'),
        "hidden keyboard markers leaked:\n{output}"
    );
}

fn adaptive_playing_app(keyboard_show: &str) -> common::TestApp {
    let mut config = Config::default();
    config.mode.kind = ModeKind::Adaptive;
    config.keyboard.show = keyboard_show.to_owned();
    config.keyboard.style = "keycap".to_owned();
    config.keyboard.finger_guide = false;
    config.ui.animation_enabled = false;
    let duration = config.session.duration();
    let item_timeout = config.session.item_timeout();
    let mut app = test_app(config);
    let now = Instant::now();
    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    app.handle_key_at(enter, now);
    app.handle_key_at(enter, now);
    common::accept_briefing(&mut app, now);
    app.on_tick_at(Duration::from_secs(3), now).unwrap();
    app.on_tick_at(Duration::from_millis(1), now).unwrap();

    let prompt = GeneratedPrompt::fixed("ghost".to_owned(), None, "ghost".to_owned());
    let mut session = Session::new(duration, item_timeout, prompt.clone().into_item());
    session.activate(Duration::ZERO);
    let game = app.game.as_mut().expect("playing game");
    game.session = session;
    game.current_prompt = prompt;
    game.typed_index = 0;
    app.adaptive_focus = vec![transition("h", "o"), transition("t", "r")];
    app
}

fn transition(from: &str, to: &str) -> TransitionCost {
    TransitionCost {
        from: from.to_owned(),
        to: to.to_owned(),
        kind: "alternating-hands".to_owned(),
        frequency: 20,
        frequency_quartile: 4,
        observation_n: 20,
        estimated_iki_ms: 240.0,
        estimated_error_rate: 0.02,
        target_iki_ms: 120.0,
        cost: 120.0,
        weight: 1.0,
        sampled_iki_ms: 240.0,
        sampled_error_rate: 0.02,
        posterior_log_variance: 0.01,
        progress_multiplier: 1.0,
        reason_kind: FocusReason::Slow,
        review_days: None,
    }
}

fn render(app: &rs_typing_trainer::app::App) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    terminal
}

fn rows(terminal: &Terminal<TestBackend>) -> Vec<String> {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect())
        .collect()
}
