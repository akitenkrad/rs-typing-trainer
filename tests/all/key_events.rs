use std::time::Duration;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use rs_typing_trainer::config::Config;
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::key_events::handle_key_event;
use rs_typing_trainer::engine::session::{InputEffect, Session};
use rs_typing_trainer::layout::builtin_layout;

use crate::common;
use common::test_app;

fn event(kind: KeyEventKind) -> KeyEvent {
    KeyEvent::new_with_kind(KeyCode::Char('a'), KeyModifiers::NONE, kind)
}

#[test]
fn release_updates_the_press_without_judging_twice() {
    let layout = builtin_layout("qwerty").unwrap();
    let mut session = Session::new(Duration::from_secs(5), Duration::from_secs(2), Item::exact("a"));
    session.activate(Duration::ZERO);

    assert_eq!(
        handle_key_event(&mut session, Duration::from_millis(100), event(KeyEventKind::Press), &layout, None),
        InputEffect::ItemCompleted
    );
    assert_eq!(
        handle_key_event(&mut session, Duration::from_millis(180), event(KeyEventKind::Release), &layout, None),
        InputEffect::Dropped
    );

    assert_eq!(session.log().keystrokes.len(), 1);
    assert_eq!(session.log().keystrokes[0].code.as_deref(), Some("KeyA"));
    assert_eq!(session.log().keystrokes[0].release_us, Some(180_000));
}

#[test]
fn repeat_is_not_judged() {
    let layout = builtin_layout("qwerty").unwrap();
    let mut session = Session::new(Duration::from_secs(5), Duration::from_secs(2), Item::exact("aa"));
    session.activate(Duration::ZERO);

    assert_eq!(
        handle_key_event(&mut session, Duration::from_millis(100), event(KeyEventKind::Press), &layout, None),
        InputEffect::Correct
    );
    assert_eq!(
        handle_key_event(&mut session, Duration::from_millis(120), event(KeyEventKind::Repeat), &layout, None),
        InputEffect::Dropped
    );
    assert_eq!(session.log().keystrokes.len(), 1);
    assert_eq!(session.position(), 1);
}

#[test]
fn remapped_press_logs_the_input_physical_code() {
    let qwerty = builtin_layout("qwerty").unwrap();
    let dvorak = builtin_layout("dvorak").unwrap();
    let mut session = Session::new(Duration::from_secs(5), Duration::from_secs(2), Item::exact("e"));
    session.activate(Duration::ZERO);
    let press = KeyEvent::new_with_kind(KeyCode::Char('d'), KeyModifiers::NONE, KeyEventKind::Press);

    assert_eq!(
        handle_key_event(&mut session, Duration::from_millis(100), press, &dvorak, Some(&qwerty)),
        InputEffect::ItemCompleted
    );
    assert_eq!(session.log().keystrokes[0].typed.as_deref(), Some("e"));
    assert_eq!(session.log().keystrokes[0].code.as_deref(), Some("KeyD"));
}

#[test]
fn release_and_repeat_do_not_operate_non_playing_screens() {
    let mut app = test_app(Config::default());
    for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
        app.handle_key(KeyEvent::new_with_kind(KeyCode::Char('q'), KeyModifiers::NONE, kind));
        assert!(!app.should_quit);
    }
    app.handle_key(KeyEvent::new_with_kind(KeyCode::Char('q'), KeyModifiers::NONE, KeyEventKind::Press));
    assert!(app.should_quit);
}
