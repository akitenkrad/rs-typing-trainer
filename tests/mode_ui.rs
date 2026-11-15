use std::fs;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Style;
use rs_typing_trainer::app::{App, Screen};
use rs_typing_trainer::config::{Config, ModeKind};
use rs_typing_trainer::engine::session::SessionPhase;
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::ui;
use tempfile::TempDir;

mod common;
use common::test_app;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn render(app: &App, width: u16, height: u16) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    terminal
}

fn text(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<Vec<_>>()
        .join("")
}

fn style_of(terminal: &Terminal<TestBackend>, symbol: &str) -> Style {
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .find(|cell| cell.symbol() == symbol)
        .unwrap_or_else(|| panic!("symbol {symbol:?} was not rendered"))
        .style()
}

fn start_selected_mode(app: &mut App, now: Instant) {
    app.handle_key_at(key(KeyCode::Enter), now);
    app.handle_key_at(key(KeyCode::Enter), now);
    assert!(matches!(app.screen, Screen::Countdown { .. }));
    app.on_tick_at(Duration::from_secs(3), now).unwrap();
    app.on_tick_at(Duration::from_millis(1), now).unwrap();
    assert_eq!(app.screen, Screen::Playing);
}

#[test]
fn title_opens_mode_select_then_key_picker() {
    let mut app = test_app(Config::default());
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.screen, Screen::ModeSelect);
    let mode_text = text(&render(&app, 80, 24));
    assert!(mode_text.contains("MODE SELECT"), "{mode_text}");

    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.config.mode.kind, ModeKind::Words);
    assert_eq!(app.screen, Screen::KeyPicker);
    let picker_text = text(&render(&app, 100, 24));
    assert!(picker_text.contains("Space"), "{picker_text}");
    assert!(picker_text.contains("include"), "{picker_text}");
}

#[test]
fn romaji_play_screen_has_surface_kana_and_guide_rows() {
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    config.mode.kind = ModeKind::Romaji;
    config.ui.animation_enabled = false;
    let mut app = test_app(config);
    let now = Instant::now();
    app.handle_key_at(key(KeyCode::Enter), now);
    app.handle_key_at(key(KeyCode::Enter), now);
    assert_eq!(app.screen, Screen::KeyPicker);
    app.handle_key_at(key(KeyCode::Enter), now);
    app.on_tick_at(Duration::from_secs(3), now).unwrap();
    app.on_tick_at(Duration::from_millis(1), now).unwrap();

    for (width, height) in [(80, 24), (80, 20), (80, 16)] {
        let output = text(&render(&app, width, height));
        let compact = output.chars().filter(|character| !character.is_whitespace()).collect::<String>();
        assert!(compact.contains("表記:"), "{width}x{height}: {output}");
        assert!(compact.contains("かな:"), "{width}x{height}: {output}");
        assert!(compact.contains("ローマ字:"), "{width}x{height}: {output}");
        assert!(compact.contains("[ESC]中断"), "{width}x{height}: {output}");
    }
}

#[test]
fn sentence_screen_wraps_a_long_sentence() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sentences.tsv");
    fs::write(
        &path,
        "A patient typist follows the winding trail and keeps a calm steady rhythm through every turn.\n",
    )
    .unwrap();
    let mut config = Config::default();
    config.mode.kind = ModeKind::Sentence;
    config.mode.sentence.lang = "en".to_owned();
    config.data.sentences_en = Some(path);
    config.ui.animation_enabled = false;
    let mut app = test_app(config);
    let now = Instant::now();
    app.handle_key_at(key(KeyCode::Enter), now);
    app.handle_key_at(key(KeyCode::Enter), now);
    assert!(matches!(app.screen, Screen::Countdown { .. }));
    app.on_tick_at(Duration::from_secs(3), now).unwrap();
    app.on_tick_at(Duration::from_millis(1), now).unwrap();

    let terminal = render(&app, 48, 26);
    let buffer = terminal.backend().buffer();
    let positions = buffer
        .content()
        .iter()
        .enumerate()
        .filter(|(_, cell)| cell.symbol() == "A" || cell.symbol() == "r")
        .map(|(index, _)| index as u16 / buffer.area.width)
        .collect::<Vec<_>>();
    let output = text(&terminal);
    assert!(output.contains("patient"), "{output}");
    assert!(output.contains("rhythm"), "{output}");
    assert!(positions.iter().min() != positions.iter().max(), "sentence did not wrap: {positions:?}");
}

#[test]
fn japanese_sentence_shows_three_guides_without_timer_at_common_sizes() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sentences_ja.tsv");
    fs::write(&path, "小さな鳥が庭で休んでいる。\tちいさなとりがにわでやすんでいる。\n").unwrap();
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    config.mode.kind = ModeKind::Sentence;
    config.mode.sentence.lang = "ja".to_owned();
    config.data.sentences_ja = Some(path);
    config.ui.animation_enabled = false;
    let mut app = test_app(config);
    let now = Instant::now();
    start_selected_mode(&mut app, now);

    let before = render(&app, 80, 24);
    let before_kana = style_of(&before, "ち");
    for (offset, character) in ['c', 'h', 'i'].into_iter().enumerate() {
        app.handle_key_at(key(KeyCode::Char(character)), now + Duration::from_millis(offset as u64 + 1));
    }

    for (width, height) in [(80, 24), (80, 20), (80, 16), (120, 40)] {
        let terminal = render(&app, width, height);
        let output = text(&terminal);
        let compact = output.chars().filter(|character| !character.is_whitespace()).collect::<String>();
        assert!(compact.contains("表記:小さな鳥が庭で休んでいる。"), "{width}x{height}: {output}");
        assert!(compact.contains("かな:ちいさなとりがにわでやすんでいる。"), "{width}x{height}: {output}");
        assert!(compact.contains("ローマ字:"), "{width}x{height}: {output}");
        assert!(!compact.contains("単語タイマー"), "{width}x{height}: {output}");
        assert!(compact.contains("[ESC]中断"), "{width}x{height}: {output}");
    }
    let after = render(&app, 80, 24);
    assert_ne!(style_of(&after, "ち"), before_kana, "completed kana must use a distinct style");
}

#[test]
fn sentence_mode_ignores_the_item_timeout() {
    let mut config = Config::default();
    config.mode.kind = ModeKind::Sentence;
    config.mode.sentence.lang = "en".to_owned();
    config.session.item_timeout_seconds = 1;
    config.ui.animation_enabled = false;
    let mut app = test_app(config);
    let now = Instant::now();
    start_selected_mode(&mut app, now);

    app.on_tick_at(Duration::from_secs(2), now + Duration::from_secs(2)).unwrap();

    assert_eq!(app.game.as_ref().unwrap().session.phase(), SessionPhase::Active);
}

#[test]
fn zero_item_timeout_hides_the_timer_bar() {
    let mut config = Config::default();
    config.session.item_timeout_seconds = 0;
    config.ui.animation_enabled = false;
    let mut app = test_app(config);
    let now = Instant::now();
    start_selected_mode(&mut app, now);

    let output = text(&render(&app, 80, 24));
    assert!(!output.contains("Word timer"), "{output}");
    assert!(!output.contains('█'), "timer gauge should not be rendered: {output}");
}

#[test]
fn empty_random_keys_open_the_picker_and_require_a_selection() {
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    config.mode.kind = ModeKind::Random;
    config.mode.keys.clear();
    config.charset_graphemes.clear();
    let mut app = test_app(config);

    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.screen, Screen::KeyPicker);
    let output = text(&render(&app, 80, 24));
    let compact = output.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    assert!(compact.contains("キーを選んでください"), "{output}");

    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.screen, Screen::KeyPicker);
}
