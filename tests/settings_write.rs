use std::fs;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::app::Screen;
use rs_typing_trainer::config::Config;
use rs_typing_trainer::config::save_keyboard_layout;
use rs_typing_trainer::ui;
use tempfile::TempDir;

mod common;
use common::test_app;

#[test]
fn changing_layout_preserves_comments_and_unrelated_values() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        r#"# top comment
[keyboard]
# keep this comment
layout = "qwerty" # selected layout
show = "dim"

[storage]
db_path = "/tmp/isolated-test.db" # untouched
"#,
    )
    .unwrap();

    save_keyboard_layout(&path, "dvorak").unwrap();
    let updated = fs::read_to_string(path).unwrap();
    assert!(updated.contains("# top comment"));
    assert!(updated.contains("# keep this comment"));
    assert!(updated.contains("layout = \"dvorak\" # selected layout"));
    assert!(updated.contains("show = \"dim\""));
    assert!(updated.contains("db_path = \"/tmp/isolated-test.db\" # untouched"));
}

#[test]
fn missing_config_is_created() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("nested/config.toml");
    save_keyboard_layout(&path, "colemak").unwrap();
    let updated = fs::read_to_string(path).unwrap();
    assert!(updated.contains("[keyboard]"));
    assert!(updated.contains("layout = \"colemak\""));
}

#[test]
fn settings_screen_opens_from_title_and_saves_selected_layout() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "# retained\n[keyboard]\nlayout = \"qwerty\"\nshow = \"full\"\n").unwrap();
    let config = Config::from_path(&path).unwrap();
    let mut app = test_app(config);
    app.config_path = Some(path.clone());

    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    assert_eq!(app.screen, Screen::Settings);
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(app.config.keyboard.layout, "dvorak");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.screen, Screen::Title);

    let updated = fs::read_to_string(path).unwrap();
    assert!(updated.contains("# retained"));
    assert!(updated.contains("layout = \"dvorak\""));
    assert!(updated.contains("show = \"full\""));
}

#[test]
fn cancelling_settings_restores_the_in_memory_values() {
    let mut app = test_app(Config::default());
    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(app.config.keyboard.layout, "dvorak");

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert_eq!(app.screen, Screen::Title);
    assert_eq!(app.config.keyboard.layout, "qwerty");
}

#[test]
fn settings_values_share_one_column() {
    let mut app = test_app(Config::default());
    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let value_columns = buffer
        .content()
        .iter()
        .enumerate()
        .filter(|(_, cell)| cell.symbol() == "<")
        .map(|(index, _)| index as u16 % buffer.area.width)
        .collect::<Vec<_>>();

    assert_eq!(value_columns.len(), 6);
    assert!(value_columns.windows(2).all(|pair| pair[0] == pair[1]), "{value_columns:?}");
}

#[test]
fn settings_save_updates_error_mode_without_rewriting_unrelated_keys() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "# retained\n[session]\nerror_mode = \"stop\" # keep\npreview_items = 3\n").unwrap();
    let mut config = Config::from_path(&path).unwrap();
    config.session.error_mode = rs_typing_trainer::config::ErrorMode::Advance;

    rs_typing_trainer::config::save_settings(&path, &config).unwrap();
    let updated = fs::read_to_string(path).unwrap();
    assert!(updated.contains("error_mode = \"advance\" # keep"));
    assert!(updated.contains("preview_items = 3"));
    assert!(updated.contains("# retained"));
}
