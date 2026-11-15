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

fn symbol_columns(terminal: &Terminal<TestBackend>, symbol: &str) -> Vec<u16> {
    let buffer = terminal.backend().buffer();
    buffer
        .content()
        .iter()
        .enumerate()
        .filter(|(_, cell)| cell.symbol() == symbol)
        .map(|(index, _)| index as u16 % buffer.area.width)
        .collect()
}

fn phrase_column(terminal: &Terminal<TestBackend>, phrase: &str) -> u16 {
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let suffix = (x..buffer.area.width).map(|column| buffer[(column, y)].symbol()).collect::<String>();
            if suffix.starts_with(phrase) {
                return x;
            }
        }
    }
    panic!("phrase {phrase:?} was not rendered");
}

fn phrase_position(terminal: &Terminal<TestBackend>, phrase: &str) -> (u16, u16) {
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let suffix = (x..buffer.area.width).map(|column| buffer[(column, y)].symbol()).collect::<String>();
            if suffix.starts_with(phrase) {
                return (x, y);
            }
        }
    }
    panic!("phrase {phrase:?} was not rendered");
}

fn compact_row_position(terminal: &Terminal<TestBackend>, phrase: &str) -> u16 {
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        let row = (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>();
        let compact = row.chars().filter(|character| !character.is_whitespace()).collect::<String>();
        if compact.contains(phrase) {
            return y;
        }
    }
    panic!("compact phrase {phrase:?} was not rendered");
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
fn mode_menu_and_help_are_left_aligned_in_both_languages() {
    for lang in [Lang::En, Lang::Ja] {
        let mut config = Config::default();
        config.general.lang = lang;
        let mut app = test_app(config);
        app.handle_key(key(KeyCode::Enter));
        let terminal = render(&app, 80, 24);
        assert_eq!(symbol_columns(&terminal, "▶"), vec![3]);
        assert!(symbol_columns(&terminal, "[").contains(&3));
    }
}

#[test]
fn benchmark_selection_uses_arrows_and_enter_for_part_d() {
    let mut app = test_app(Config::default());
    app.handle_key(key(KeyCode::Char('b')));
    assert_eq!(app.screen, Screen::BenchSelect);

    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));

    assert_eq!(app.screen, Screen::BenchSelect);
    assert!(app.bench_include_d);
    let terminal = render(&app, 80, 24);
    assert_eq!(symbol_columns(&terminal, "▶"), vec![3]);
}

#[test]
fn key_picker_and_history_instructions_are_left_aligned_at_80_by_24() {
    let mut app = test_app(Config::default());
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.screen, Screen::KeyPicker);
    let picker = render(&app, 80, 24);
    assert_eq!(phrase_column(&picker, "Selected:"), 3);
    assert_eq!(phrase_column(&picker, "[Arrows]"), 3);
    assert_eq!(phrase_column(&picker, "[Enter]"), 3);

    let mut app = test_app(Config::default());
    app.handle_key(key(KeyCode::Char('h')));
    assert_eq!(app.screen, Screen::History);
    let history = render(&app, 80, 24);
    assert_eq!(phrase_column(&history, "Database"), 3);
    assert_eq!(phrase_column(&history, "[↑/↓/k/j]"), 3);
}

#[test]
fn benchmark_result_and_summary_text_are_left_aligned_at_80_by_24() {
    let config = Config::default();
    let mut app = test_app(config.clone());
    app.active_bench_id = Some("bench-v1/A/01".to_owned());
    app.last_result = Some(rs_typing_trainer::metrics::stats::GameResult::from_metrics(
        &config,
        Default::default(),
        Duration::from_secs(60),
    ));
    app.screen = Screen::Result;
    let result = render(&app, 80, 24);
    assert_eq!(phrase_column(&result, "Part A English words"), 3);
    assert_eq!(phrase_column(&result, "Version:"), 3);

    app.active_bench_id = None;
    app.screen = Screen::BenchSummary;
    let summary = render(&app, 80, 24);
    assert_eq!(phrase_column(&summary, "[ENTER]"), 3);
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
fn sentence_body_is_below_the_keyboard_and_vertically_centered() {
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
    start_selected_mode(&mut app, now);

    let terminal = render(&app, 80, 24);
    let keyboard_row = compact_row_position(&terminal, "asdf");
    let (_, stats_row) = phrase_position(&terminal, "Combo");
    let (_, first_text_row) = phrase_position(&terminal, "A patient typist");
    let (_, footer_row) = phrase_position(&terminal, "[ESC] Abort");
    assert!(keyboard_row < stats_row, "keyboard={keyboard_row}, stats={stats_row}");
    assert!(stats_row < first_text_row, "stats={stats_row}, text={first_text_row}");

    let first_line_columns = (1..terminal.backend().buffer().area.width - 1)
        .filter(|x| terminal.backend().buffer()[(*x, first_text_row)].symbol() != " ")
        .collect::<Vec<_>>();
    let text_center = f32::from(first_line_columns[0] + first_line_columns[first_line_columns.len() - 1]) / 2.0;
    let screen_center = f32::from(terminal.backend().buffer().area.width - 1) / 2.0;
    assert!(
        (text_center - screen_center).abs() <= 1.0,
        "text center={text_center}, screen center={screen_center}"
    );

    let output = text(&terminal);
    let last_text_row = (0..terminal.backend().buffer().area.height)
        .filter(|y| {
            let row = (0..terminal.backend().buffer().area.width)
                .map(|x| terminal.backend().buffer()[(x, *y)].symbol())
                .collect::<String>();
            row.contains("every turn.") || row.contains('▲')
        })
        .max()
        .unwrap_or_else(|| panic!("sentence tail was not rendered: {output}"));
    let top_gap = first_text_row - stats_row - 1;
    let bottom_gap = footer_row - last_text_row - 1;
    assert!(top_gap.abs_diff(bottom_gap) <= 1, "top gap={top_gap}, bottom gap={bottom_gap}: {output}");
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
