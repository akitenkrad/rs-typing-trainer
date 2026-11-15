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

    assert_eq!(value_columns.len(), 8);
    assert!(value_columns.windows(2).all(|pair| pair[0] == pair[1]), "{value_columns:?}");
    let arrow_columns = buffer
        .content()
        .iter()
        .enumerate()
        .filter(|(_, cell)| cell.symbol() == "▶")
        .map(|(index, _)| index as u16 % buffer.area.width)
        .collect::<Vec<_>>();
    assert_eq!(arrow_columns, vec![3]);
}

#[test]
fn every_setting_has_a_bilingual_description_with_options_and_a_default() {
    let expected = [
        ("Practice layout", "練習する配列"),
        ("OS input layout", "OS が使っている配列"),
        ("physical key arrangement", "物理的な並び"),
        ("full: always visible", "full: 常に表示"),
        ("Keyboard appearance", "キーボードの見た目"),
        ("Large text (large)", "大きな文字（large）"),
        ("interface language", "画面の表示言語"),
        ("mistyped character", "誤入力した文字"),
    ];
    let options = [
        ["qwerty", "dvorak", "colemak-dh-ansi", "workman"].as_slice(),
        ["same", "qwerty", "colemak-dh-ansi", "jis-qwerty"].as_slice(),
        ["ansi", "jis"].as_slice(),
        ["full", "auto", "dim", "hidden"].as_slice(),
        ["keycap", "raised", "plain"].as_slice(),
        ["large", "normal", "16", "12"].as_slice(),
        ["English", "Japanese"].as_slice(),
        ["stop", "advance"].as_slice(),
    ];

    for (lang, default_word) in [
        (rs_typing_trainer::i18n::Lang::En, "Default"),
        (rs_typing_trainer::i18n::Lang::Ja, "既定"),
    ] {
        let mut config = Config::default();
        config.general.lang = lang;
        let mut app = test_app(config);
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        for (index, snippets) in expected.iter().enumerate() {
            let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
            terminal.draw(|frame| ui::render(frame, &app)).unwrap();
            let buffer = terminal.backend().buffer();
            let description_row_has_text = |y| {
                (3..77).any(|x| {
                    let symbol = buffer[(x, y)].symbol();
                    !symbol.trim().is_empty()
                })
            };
            assert!(description_row_has_text(11), "setting {index}, {lang:?} has no description");
            assert!(
                description_row_has_text(12),
                "setting {index}, {lang:?} needs at least two description lines"
            );
            assert!(!description_row_has_text(15), "setting {index}, {lang:?} exceeded four description lines");
            let rendered = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<Vec<_>>()
                .join("");
            let compact = rendered.chars().filter(|character| !character.is_whitespace()).collect::<String>();
            let snippet = if lang == rs_typing_trainer::i18n::Lang::En {
                snippets.0
            } else {
                snippets.1
            };
            assert!(
                compact.contains(&snippet.chars().filter(|character| !character.is_whitespace()).collect::<String>()),
                "setting {index}, {lang:?}: compact={compact:?}\n{rendered}"
            );
            for option in options[index] {
                assert!(compact.contains(option), "setting {index} omitted {option}, {lang:?}: {rendered}");
            }
            if lang == rs_typing_trainer::i18n::Lang::En {
                assert!(compact.contains(default_word), "setting {index}, {lang:?}: {rendered}");
            } else {
                assert!(compact.contains('既') && compact.contains('定'), "setting {index}, {lang:?}: {rendered}");
            }
            assert!(compact.contains("Save") || compact.contains("保存"), "80x24 clipped the hint: {rendered}");
            app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
    }
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

#[test]
fn settings_screen_cycles_and_saves_keyboard_style() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "[keyboard]\nstyle = \"keycap\" # keep\n").unwrap();
    let config = Config::from_path(&path).unwrap();
    let mut app = test_app(config);
    app.config_path = Some(path.clone());

    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    for _ in 0..4 {
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(app.config.keyboard.style, "raised");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let updated = fs::read_to_string(path).unwrap();
    assert!(updated.contains("style = \"raised\" # keep"), "{updated}");
}

#[test]
fn settings_screen_cycles_saves_and_cancels_prompt_style() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    // A legacy key: read as `prompt_style = "large"` and rewritten under the new name.
    fs::write(&path, "[display]\nprompt_size = \"auto\" # keep\n").unwrap();
    let config = Config::from_path(&path).unwrap();
    assert_eq!(config.display.prompt_style, "large");
    let mut app = test_app(config);
    app.config_path = Some(path.clone());

    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    for _ in 0..5 {
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(app.config.display.prompt_style, "normal");
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.config.display.prompt_style, "large");

    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    for _ in 0..5 {
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    assert_eq!(app.config.display.prompt_style, "normal");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let updated = fs::read_to_string(&path).unwrap();
    assert!(updated.contains("prompt_style = \"normal\" # keep"), "{updated}");
    assert!(!updated.contains("prompt_size"), "{updated}");
    assert_eq!(Config::from_path(&path).unwrap().display.prompt_style, "normal");
}

#[test]
fn legacy_prompt_size_values_map_to_prompt_style() {
    let dir = TempDir::new().unwrap();
    for (legacy, expected) in [("auto", "large"), ("normal", "normal"), ("large", "large"), ("xlarge", "large")] {
        let path = dir.path().join(format!("{legacy}.toml"));
        fs::write(&path, format!("[display]\nprompt_size = \"{legacy}\"\n")).unwrap();
        assert_eq!(Config::from_path(&path).unwrap().display.prompt_style, expected, "{legacy}");
    }
    let both = dir.path().join("both.toml");
    fs::write(&both, "[display]\nprompt_size = \"normal\"\nprompt_style = \"large\"\n").unwrap();
    assert_eq!(Config::from_path(&both).unwrap().display.prompt_style, "large");
    let new = dir.path().join("new.toml");
    fs::write(&new, "[display]\nprompt_style = \"normal\"\n").unwrap();
    assert_eq!(Config::from_path(&new).unwrap().display.prompt_style, "normal");
    let invalid = dir.path().join("invalid.toml");
    fs::write(&invalid, "[display]\nprompt_style = \"xlarge\"\n").unwrap();
    assert!(Config::from_path(&invalid).is_err());
    assert_eq!(Config::default().display.prompt_style, "large");
}

#[test]
fn adding_sections_keeps_a_trailing_comment_with_its_original_section() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        "[metrics]\ndb_path = \"metrics.db\"\n# Override the SQLite database path for this metrics section.\n",
    )
    .unwrap();

    rs_typing_trainer::config::save_settings(&path, &Config::default()).unwrap();

    let updated = fs::read_to_string(path).unwrap();
    let comment = updated.find("# Override the SQLite database path").unwrap();
    let keyboard = updated.find("[keyboard]").unwrap();
    let general = updated.find("[general]").unwrap();
    let session = updated.find("[session]").unwrap();
    assert!(comment < keyboard, "{updated}");
    assert!(comment < general, "{updated}");
    assert!(comment < session, "{updated}");
}
