use std::collections::HashSet;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;
use rs_typing_trainer::config::{Config, UnlockOrder};
use rs_typing_trainer::data::DataCatalog;
use rs_typing_trainer::engine::modes::ModeGenerator;
use rs_typing_trainer::layout::builtin_layout;
use rs_typing_trainer::layout::geometry::geometry;
use rs_typing_trainer::practice::unlock::{UnlockState, apply_result, initial_keys, mode_config, target_iki_ms, unlock_sequence};
use rs_typing_trainer::record::db;
use rs_typing_trainer::ui::keyboard::{KeyboardState, KeyboardView, UnlockKeyboardState};
use tempfile::TempDir;

#[test]
fn three_consecutive_qualifying_sessions_unlock_one_key() {
    let layout = builtin_layout("qwerty").unwrap();
    let geometry = geometry("ansi").unwrap();
    let mut state = UnlockState::new(initial_keys(&layout));
    let sequence = unlock_sequence(&layout, &geometry, UnlockOrder::Frequency);
    assert_eq!(apply_result(&mut state, 0.97, target_iki_ms(35.0), 35.0, &sequence), None);
    assert_eq!(apply_result(&mut state, 1.00, target_iki_ms(35.0) - 1.0, 35.0, &sequence), None);
    assert_eq!(apply_result(&mut state, 0.99, target_iki_ms(35.0), 35.0, &sequence), Some('e'));
    assert!(state.unlocked.contains('e'));
}

#[test]
fn a_failed_session_breaks_the_streak() {
    let layout = builtin_layout("qwerty").unwrap();
    let geometry = geometry("ansi").unwrap();
    let sequence = unlock_sequence(&layout, &geometry, UnlockOrder::Frequency);
    let mut state = UnlockState::new(initial_keys(&layout));
    assert_eq!(apply_result(&mut state, 1.0, 100.0, 35.0, &sequence), None);
    assert_eq!(apply_result(&mut state, 0.969, 100.0, 35.0, &sequence), None);
    assert_eq!(state.streak, 0);
}

#[test]
fn a_session_without_an_iki_sample_does_not_advance_the_streak() {
    let layout = builtin_layout("qwerty").unwrap();
    let geometry = geometry("ansi").unwrap();
    let sequence = unlock_sequence(&layout, &geometry, UnlockOrder::Frequency);
    let mut state = UnlockState::new(initial_keys(&layout));
    assert_eq!(apply_result(&mut state, 1.0, 0.0, 35.0, &sequence), None);
    assert_eq!(state.streak, 0);
}

#[test]
fn frequency_and_row_orders_choose_different_next_keys() {
    let layout = builtin_layout("qwerty").unwrap();
    let geometry = geometry("ansi").unwrap();
    let frequency = unlock_sequence(&layout, &geometry, UnlockOrder::Frequency);
    let rows = unlock_sequence(&layout, &geometry, UnlockOrder::Rows);
    assert_eq!(frequency[0], 'e');
    assert_eq!(rows[0], 'g');
}

#[test]
fn unlock_state_is_saved_per_layout() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("sessions.db");
    db::save_unlock_state(
        &path,
        "qwerty",
        &UnlockState {
            unlocked: "asdfjkl;e".into(),
            streak: 2,
        },
    )
    .unwrap();
    db::save_unlock_state(
        &path,
        "dvorak",
        &UnlockState {
            unlocked: "aoeuhtns".into(),
            streak: 1,
        },
    )
    .unwrap();
    assert_eq!(db::load_unlock_state(&path, "qwerty").unwrap().unwrap().unlocked, "asdfjkl;e");
    assert_eq!(db::load_unlock_state(&path, "dvorak").unwrap().unwrap().unlocked, "aoeuhtns");
}

#[test]
fn generated_prompts_use_only_unlocked_keys() {
    let dir = TempDir::new().unwrap();
    let catalog = DataCatalog::load(&Config::default().data, dir.path()).unwrap();
    let allowed = "asdfjkl;";
    let config = mode_config(Config::default().mode, allowed);
    let mut generator = ModeGenerator::new(config, catalog, builtin_layout("qwerty").unwrap(), 9);
    let allowed = allowed.chars().collect::<HashSet<_>>();
    for _ in 0..100 {
        let prompt = generator.next_prompt();
        assert!(prompt.target.chars().all(|character| allowed.contains(&character)), "{}", prompt.target);
    }
}

#[test]
fn keyboard_distinguishes_unlocked_next_and_locked_keys() {
    let layout = builtin_layout("qwerty").unwrap();
    let geometry = geometry("ansi").unwrap();
    let state = KeyboardState {
        unlock: Some(UnlockKeyboardState {
            unlocked_codes: ["KeyA".to_owned()].into_iter().collect(),
            next_code: Some("KeyE".to_owned()),
        }),
        ..KeyboardState::default()
    };
    let mut terminal = Terminal::new(TestBackend::new(120, 8)).unwrap();
    terminal
        .draw(|frame| {
            rs_typing_trainer::ui::keyboard::render(
                frame,
                Rect::new(0, 0, 120, 8),
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
    let buffer = terminal.backend().buffer();
    let style = |symbol: &str| buffer.content().iter().find(|cell| cell.symbol() == symbol).unwrap().style();
    assert_eq!(style("a").bg, Some(Color::DarkGray));
    assert_eq!(style("e").bg, Some(Color::Yellow));
    assert_eq!(style("z").fg, Some(Color::DarkGray));
}

#[test]
fn play_screen_shows_unlock_legend_and_progress() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use rs_typing_trainer::config::ModeKind;
    use std::time::{Duration, Instant};
    mod_support::with_app(|mut app| {
        app.config.mode.kind = ModeKind::Unlock;
        app.config.ui.animation_enabled = false;
        let now = Instant::now();
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        app.handle_key_at(enter, now);
        app.handle_key_at(enter, now);
        app.on_tick_at(Duration::from_secs(3), now).unwrap();
        app.on_tick_at(Duration::ZERO, now).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| rs_typing_trainer::ui::render(frame, &app)).unwrap();
        let content = terminal.backend().buffer().content();
        let text = content.iter().map(|cell| cell.symbol()).collect::<String>();
        assert!(text.contains("Unlocked"), "{text}");
        assert!(text.contains("Next unlock: 0 / 3"), "{text}");
        assert!(text.contains("97%"), "{text}");
        let filled = content.iter().filter(|cell| cell.symbol() == "■").collect::<Vec<_>>();
        let locked = content.iter().find(|cell| cell.symbol() == "□").unwrap();
        assert_eq!(filled.len(), 2);
        assert_eq!(filled[0].style().bg, Some(Color::DarkGray));
        assert_eq!(filled[1].style().bg, Some(Color::Yellow));
        assert_eq!(locked.style().fg, Some(Color::DarkGray));
    });
}

mod mod_support {
    use rs_typing_trainer::app::App;
    use rs_typing_trainer::config::Config;
    use tempfile::TempDir;

    pub fn with_app(run: impl FnOnce(App)) {
        let home = TempDir::new().unwrap();
        let mut config = Config::default();
        config.storage.db_path = Some(home.path().join("sessions.db"));
        run(App::new(config, home.path().to_path_buf()));
    }
}
