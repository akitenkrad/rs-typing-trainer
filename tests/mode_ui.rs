use std::fs;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Style;
use rs_typing_trainer::app::{App, Screen};
use rs_typing_trainer::config::{Config, ModeKind};
use rs_typing_trainer::engine::session::SessionPhase;
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::ui;
use rs_typing_trainer::ui::bigtext::{BigTextSize, font, window_range};
use rs_typing_trainer::ui::keyboard::KeyboardStyle;
use rs_typing_trainer::ui::playing::{PromptLayoutStage, resolve_playing_layout};
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

fn keys_row_position(terminal: &Terminal<TestBackend>, keys: &[char]) -> u16 {
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        let row = (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>();
        let positions = keys
            .iter()
            .filter_map(|key| row.chars().position(|character| character == *key))
            .collect::<Vec<_>>();
        if positions.len() == keys.len() && positions.windows(2).all(|pair| pair[0] < pair[1]) {
            return y;
        }
    }
    panic!("keys {keys:?} were not rendered");
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
fn sentence_body_is_below_the_keyboard_and_top_aligned() {
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
    let keyboard_row = keys_row_position(&terminal, &['a', 's', 'd', 'f']);
    let (_, stats_row) = phrase_position(&terminal, "Combo");
    let (_, first_text_row) = phrase_position(&terminal, "A patient typist");
    assert!(keyboard_row < stats_row, "keyboard={keyboard_row}, stats={stats_row}");
    let first_big_row = (stats_row + 1..terminal.backend().buffer().area.height)
        .find(|y| {
            (1..terminal.backend().buffer().area.width - 1).any(|x| {
                matches!(
                    terminal.backend().buffer()[(x, *y)].symbol(),
                    "▘" | "▝" | "▖" | "▗" | "▀" | "▄" | "▌" | "▐" | "▚" | "▞" | "▛" | "▜" | "▙" | "▟" | "█"
                )
            })
        })
        .unwrap();
    assert_eq!(first_big_row, stats_row + 1, "stats={stats_row}, big={first_big_row}");
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
}

#[test]
fn auto_prompt_and_keyboard_stages_match_common_terminal_sizes() {
    let mut config = Config::default();
    config.mode.kind = ModeKind::Random;
    config.ui.animation_enabled = false;
    let mut app = test_app(config);
    start_selected_mode(&mut app, Instant::now());

    for (width, height, stage, size, keyboard) in [
        (
            120,
            40,
            PromptLayoutStage::XlargeSelected,
            Some(BigTextSize::Dot16),
            Some(KeyboardStyle::Keycap),
        ),
        (
            100,
            36,
            PromptLayoutStage::XlargeSelected,
            Some(BigTextSize::Dot16),
            Some(KeyboardStyle::Keycap),
        ),
        (
            80,
            24,
            PromptLayoutStage::LargeSelected,
            Some(BigTextSize::Dot12),
            Some(KeyboardStyle::Keycap),
        ),
        (
            80,
            20,
            PromptLayoutStage::LargeSimplified,
            Some(BigTextSize::Dot12),
            Some(KeyboardStyle::Plain),
        ),
    ] {
        let layout = resolve_playing_layout(&app, Rect::new(1, 1, width - 2, height - 2));
        assert_eq!(layout.stage, stage, "{width}x{height}: {layout:?}");
        assert_eq!(layout.bigtext, size, "{width}x{height}: {layout:?}");
        assert_eq!(layout.keyboard_style, keyboard, "{width}x{height}: {layout:?}");
        let output = text(&render(&app, width, height));
        assert!(output.contains("[ESC] Abort"), "{width}x{height}: {output}");
        assert!(output.chars().any(|character| matches!(
            character,
            '▘' | '▝' | '▖' | '▗' | '▀' | '▄' | '▌' | '▐' | '▚' | '▞' | '▛' | '▜' | '▙' | '▟' | '█'
        )));
    }
}

#[test]
fn sentence_bigtext_window_always_contains_the_current_position() {
    let font = font(BigTextSize::Dot16).unwrap();
    let sentence = "A patient typist follows the winding trail and keeps a calm steady rhythm.";
    let character_count = sentence.chars().count();
    for current in [0, 1, character_count / 2, character_count - 2, character_count - 1] {
        let range = window_range(font, sentence, current, 36);
        assert!(range.contains(&current), "current={current}, range={range:?}");
        let width = font.text_width(&sentence.chars().skip(range.start).take(range.len()).collect::<String>());
        assert!(width <= 36, "current={current}, range={range:?}, width={width}");
    }
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

const QUADRANT_SYMBOLS: [&str; 15] = ["▘", "▝", "▖", "▗", "▀", "▄", "▌", "▐", "▚", "▞", "▛", "▜", "▙", "▟", "█"];

fn start_any_mode(app: &mut App, now: Instant) {
    app.handle_key_at(key(KeyCode::Enter), now);
    app.handle_key_at(key(KeyCode::Enter), now);
    if app.screen == Screen::KeyPicker {
        app.handle_key_at(key(KeyCode::Enter), now);
    }
    assert!(matches!(app.screen, Screen::Countdown { .. }), "{:?}", app.screen);
    app.on_tick_at(Duration::from_secs(3), now).unwrap();
    app.on_tick_at(Duration::from_millis(1), now).unwrap();
    assert_eq!(app.screen, Screen::Playing);
}

fn mode_app(kind: ModeKind, lang: Lang, configure: impl FnOnce(&mut Config)) -> common::TestApp {
    let mut config = Config::default();
    config.general.lang = lang;
    config.mode.kind = kind;
    if kind == ModeKind::Sentence {
        config.mode.sentence.lang = match lang {
            Lang::En => "en".to_owned(),
            Lang::Ja => "ja".to_owned(),
        };
    }
    config.ui.animation_enabled = false;
    configure(&mut config);
    let mut app = test_app(config);
    start_any_mode(&mut app, Instant::now());
    app
}

fn row_has_quadrants(terminal: &Terminal<TestBackend>, y: u16) -> bool {
    let buffer = terminal.backend().buffer();
    (1..buffer.area.width - 1).any(|x| QUADRANT_SYMBOLS.contains(&buffer[(x, y)].symbol()))
}

fn row_has_text(terminal: &Terminal<TestBackend>, y: u16) -> bool {
    let buffer = terminal.backend().buffer();
    (1..buffer.area.width - 1).any(|x| !buffer[(x, y)].symbol().trim().is_empty())
}

/// Finds the top-left cell where the given raster (one string per row) is drawn.
fn find_raster(terminal: &Terminal<TestBackend>, raster: &[String]) -> Option<(u16, u16)> {
    let buffer = terminal.backend().buffer();
    let width = raster[0].chars().count() as u16;
    let height = raster.len() as u16;
    for y in 0..=buffer.area.height.saturating_sub(height) {
        for x in 0..=buffer.area.width.saturating_sub(width) {
            let matches = raster.iter().enumerate().all(|(dy, row)| {
                row.chars()
                    .enumerate()
                    .all(|(dx, symbol)| buffer[(x + dx as u16, y + dy as u16)].symbol() == symbol.to_string())
            });
            if matches {
                return Some((x, y));
            }
        }
    }
    None
}

fn raster_at(terminal: &Terminal<TestBackend>, raster: &[String], x: u16, y: u16) -> bool {
    let buffer = terminal.backend().buffer();
    raster.iter().enumerate().all(|(dy, row)| {
        row.chars()
            .enumerate()
            .all(|(dx, symbol)| buffer[(x + dx as u16, y + dy as u16)].symbol() == symbol.to_string())
    })
}

/// Row whose non-blank cells contain `phrase` (wide characters leave blank
/// continuation cells, so the row is compared without whitespace).
fn compact_row(terminal: &Terminal<TestBackend>, phrase: &str) -> u16 {
    let buffer = terminal.backend().buffer();
    let phrase = phrase.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    (0..buffer.area.height)
        .find(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, *y)].symbol())
                .collect::<String>()
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect::<String>()
                .contains(&phrase)
        })
        .unwrap_or_else(|| panic!("phrase {phrase:?} was not rendered"))
}

fn raster_of(size: BigTextSize, character: char) -> Vec<String> {
    let rendered = font(size)
        .unwrap()
        .render(&[rs_typing_trainer::ui::bigtext::StyledCharacter::new(character, Style::default())]);
    rendered
        .lines
        .iter()
        .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect())
        .collect()
}

#[test]
fn every_prompt_family_picks_the_expected_stage_at_common_sizes() {
    use PromptLayoutStage::*;
    let dot16 = Some(BigTextSize::Dot16);
    let dot12 = Some(BigTextSize::Dot12);
    let keycap = Some(KeyboardStyle::Keycap);
    let plain = Some(KeyboardStyle::Plain);
    let cases = [
        (
            ModeKind::Words,
            Lang::En,
            [
                (120, 40, XlargeSelected, dot16, keycap),
                (100, 36, XlargeSelected, dot16, keycap),
                (80, 24, LargeSelected, dot12, keycap),
                (80, 20, LargeSimplified, dot12, plain),
            ],
        ),
        (
            ModeKind::Romaji,
            Lang::Ja,
            [
                (120, 40, XlargeSelected, dot16, keycap),
                (100, 36, XlargeSelected, dot16, keycap),
                (80, 24, LargeHidden, dot12, None),
                (80, 20, NormalKeyboard, None, keycap),
            ],
        ),
        (
            ModeKind::Sentence,
            Lang::En,
            [
                (120, 40, XlargeSelected, dot16, keycap),
                (100, 36, XlargeSelected, dot16, keycap),
                (80, 24, LargeSimplified, dot12, plain),
                (80, 20, LargeHidden, dot12, None),
            ],
        ),
        (
            ModeKind::Sentence,
            Lang::Ja,
            [
                (120, 40, XlargeSelected, dot16, keycap),
                (100, 36, LargeSelected, dot12, keycap),
                (80, 24, LargeHidden, dot12, None),
                (80, 20, NormalKeyboard, None, keycap),
            ],
        ),
    ];
    for (kind, lang, sizes) in cases {
        let app = mode_app(kind, lang, |_| {});
        let (metrics_label, abort_label) = match lang {
            Lang::En => ("Combo", "[ESC] Abort"),
            Lang::Ja => ("連続正解", "[ESC] 中断"),
        };
        for (width, height, stage, size, keyboard) in sizes {
            let layout = resolve_playing_layout(&app, Rect::new(1, 1, width - 2, height - 2));
            let label = format!("{kind:?}/{lang:?} {width}x{height}");
            assert_eq!(layout.stage, stage, "{label}: {layout:?}");
            assert_eq!(layout.bigtext, size, "{label}: {layout:?}");
            assert_eq!(layout.keyboard_style, keyboard, "{label}: {layout:?}");

            let terminal = render(&app, width, height);
            let output = text(&terminal);
            let compact = output.chars().filter(|character| !character.is_whitespace()).collect::<String>();
            assert!(compact.contains(&abort_label.replace(' ', "")), "{label}: controls missing: {output}");
            let metrics_row = compact_row(&terminal, metrics_label);
            let footer_row = compact_row(&terminal, abort_label);
            let timer_row = (0..height).find(|y| {
                let row = (0..width)
                    .map(|x| terminal.backend().buffer()[(x, *y)].symbol())
                    .collect::<String>()
                    .chars()
                    .filter(|character| !character.is_whitespace())
                    .collect::<String>();
                row.contains("Wordtimer") || row.contains("単語タイマー")
            });
            if keyboard.is_some() {
                let keyboard_row = keys_row_position(&terminal, &['a', 's', 'd', 'f']);
                let next_row = timer_row.unwrap_or(metrics_row);
                assert!(keyboard_row < next_row, "{label}: keyboard={keyboard_row}, next={next_row}: {output}");
            }
            if let Some(timer_row) = timer_row {
                assert!(timer_row < metrics_row, "{label}: timer={timer_row}, metrics={metrics_row}: {output}");
            }
            assert!(
                metrics_row + 1 < footer_row,
                "{label}: metrics={metrics_row}, footer={footer_row}: {output}"
            );
            let game = app.game.as_ref().unwrap();
            let labelled = game.current_prompt.reading.is_some();
            if let Some(size) = size {
                // Romaji and Japanese sentences put the "表記:" label first;
                // otherwise the large glyphs themselves start on that row.
                let glyph_row = metrics_row + 1 + u16::from(labelled);
                if labelled {
                    assert!(row_has_text(&terminal, metrics_row + 1), "{label}: label missing: {output}");
                }
                let first = game.current_prompt.text.chars().next().unwrap();
                let raster = raster_of(size, first);
                assert!(
                    (0..width - raster[0].chars().count() as u16).any(|x| raster_at(&terminal, &raster, x, glyph_row)),
                    "{label}: the first large glyph {first:?} must start on row {glyph_row}: {output}"
                );
            } else {
                assert!(
                    row_has_text(&terminal, metrics_row + 1),
                    "{label}: the prompt must start directly below the metrics row: {output}"
                );
            }
        }
    }
}

#[test]
fn prompt_size_setting_limits_or_disables_large_text() {
    let normal = mode_app(ModeKind::Words, Lang::En, |config| config.display.prompt_size = "normal".to_owned());
    let layout = resolve_playing_layout(&normal, Rect::new(1, 1, 118, 38));
    assert_eq!(layout.stage, PromptLayoutStage::NormalKeyboard);
    assert_eq!(layout.bigtext, None);
    let terminal = render(&normal, 120, 40);
    let output = text(&terminal);
    let metrics_row = compact_row(&terminal, "Combo");
    assert!(!(metrics_row + 1..metrics_row + 4).any(|y| row_has_quadrants(&terminal, y)), "{output}");
    let compact = output.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    assert!(compact.contains(&normal.game.as_ref().unwrap().current_prompt.target), "{output}");
    let caret_row = phrase_position(&terminal, "▲").1;
    assert!(
        caret_row <= metrics_row + 3,
        "normal-size prompt must sit right below the metrics: {output}"
    );

    let large = mode_app(ModeKind::Words, Lang::En, |config| config.display.prompt_size = "large".to_owned());
    let layout = resolve_playing_layout(&large, Rect::new(1, 1, 118, 38));
    assert_eq!(layout.stage, PromptLayoutStage::LargeSelected);
    assert_eq!(layout.bigtext, Some(BigTextSize::Dot12));

    let xlarge = mode_app(ModeKind::Words, Lang::En, |config| config.display.prompt_size = "xlarge".to_owned());
    assert_eq!(
        resolve_playing_layout(&xlarge, Rect::new(1, 1, 118, 38)).bigtext,
        Some(BigTextSize::Dot16)
    );
    let fallback = resolve_playing_layout(&xlarge, Rect::new(1, 1, 78, 22));
    assert_eq!(
        fallback.bigtext,
        Some(BigTextSize::Dot12),
        "xlarge steps down when it does not fit: {fallback:?}"
    );
}

#[test]
fn sentence_screen_window_follows_the_current_character() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sentences.tsv");
    fs::write(
        &path,
        "A patient typist follows the winding trail and keeps a calm steady rhythm through every turn.\n",
    )
    .unwrap();
    let mut app = mode_app(ModeKind::Sentence, Lang::En, |config| config.data.sentences_en = Some(path));
    let now = Instant::now();
    for step in 0..40 {
        let expected = app.game.as_ref().unwrap().session.expected().unwrap().chars().next().unwrap();
        app.handle_key_at(key(KeyCode::Char(expected)), now + Duration::from_millis(step * 50 + 50));
    }
    let game = app.game.as_ref().unwrap();
    let current = game.current_prompt.text.chars().nth(game.typed_index).unwrap();
    assert_eq!(game.typed_index, 40);

    for (width, height, size) in [(80, 24, BigTextSize::Dot12), (120, 40, BigTextSize::Dot16)] {
        let terminal = render(&app, width, height);
        let output = text(&terminal);
        let cursor_columns = symbol_columns(&terminal, "▔");
        assert!(!cursor_columns.is_empty(), "{width}x{height}: cursor missing: {output}");
        let cursor_row = phrase_position(&terminal, "▔").1;
        let raster = raster_of(size, current);
        assert_eq!(cursor_columns.len(), usize::from(font(size).unwrap().character_width(current)));
        assert!(
            raster_at(&terminal, &raster, cursor_columns[0], cursor_row - raster.len() as u16),
            "{width}x{height}: the current glyph {current:?} is not drawn right above the cursor: {output}"
        );
        assert!(output.contains("patient typist"), "{width}x{height}: full sentence missing: {output}");
    }
}

#[test]
fn romaji_screen_draws_surface_and_romaji_large_with_kana_between() {
    let app = mode_app(ModeKind::Romaji, Lang::Ja, |_| {});
    let game = app.game.as_ref().unwrap();
    let surface_first = game.current_prompt.text.chars().next().unwrap();
    let guide_first = game.session.current_guide().chars().next().unwrap();
    let reading = game.current_prompt.reading.clone().unwrap();
    for (width, height, size) in [(120, 40, BigTextSize::Dot16), (80, 24, BigTextSize::Dot12)] {
        let terminal = render(&app, width, height);
        let output = text(&terminal);
        let surface =
            find_raster(&terminal, &raster_of(size, surface_first)).unwrap_or_else(|| panic!("{width}x{height}: surface glyph missing: {output}"));
        let guide =
            find_raster(&terminal, &raster_of(size, guide_first)).unwrap_or_else(|| panic!("{width}x{height}: romaji glyph missing: {output}"));
        let kana_row = compact_row(&terminal, "かな:");
        assert!(
            surface.1 < kana_row && kana_row < guide.1,
            "{width}x{height}: {surface:?} {kana_row} {guide:?}"
        );
        let compact = output.chars().filter(|character| !character.is_whitespace()).collect::<String>();
        assert!(compact.contains(&format!("かな:{reading}")), "{width}x{height}: {output}");
    }
}

#[test]
fn blind_large_prompt_does_not_draw_typed_characters() {
    let mut app = mode_app(ModeKind::Words, Lang::En, |config| config.session.blind = true);
    let now = Instant::now();
    let expected = app.game.as_ref().unwrap().session.expected().unwrap().chars().next().unwrap();
    app.handle_key_at(key(KeyCode::Char(expected)), now + Duration::from_millis(50));
    assert_eq!(app.game.as_ref().unwrap().typed_index, 1);

    let terminal = render(&app, 80, 24);
    let buffer = terminal.backend().buffer();
    let cursor_row = phrase_position(&terminal, "▔").1;
    let cursor_column = symbol_columns(&terminal, "▔")[0];
    let typed_width = font(BigTextSize::Dot12).unwrap().character_width(expected);
    for y in cursor_row - 6..cursor_row {
        for x in cursor_column - typed_width..cursor_column {
            assert_eq!(buffer[(x, y)].symbol(), " ", "typed glyph drawn at ({x}, {y}): {}", text(&terminal));
        }
    }
    assert!(
        (cursor_row - 6..cursor_row).any(|y| row_has_quadrants(&terminal, y)),
        "the remaining characters must still be drawn"
    );
}

#[test]
fn long_japanese_sentence_keeps_every_guide_row_filled_at_common_sizes() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sentences_ja.tsv");
    fs::write(
        &path,
        "そして両手に赤と青の旗をもってそらを見上げて信号しているのでした。\tそしてりょうてにあかとあおのはたをもってそらをみあげてしんごうしているのでした。\n",
    )
    .unwrap();
    let mut app = mode_app(ModeKind::Sentence, Lang::Ja, |config| config.data.sentences_ja = Some(path));
    let now = Instant::now();
    for (step, character) in "soshiteryoute".chars().enumerate() {
        app.handle_key_at(key(KeyCode::Char(character)), now + Duration::from_millis(step as u64 * 50 + 50));
    }
    assert_eq!(app.game.as_ref().unwrap().session.completed_reading_chars(), 7);

    for (width, height) in [(120, 40), (100, 36), (80, 24), (80, 20)] {
        let terminal = render(&app, width, height);
        let buffer = terminal.backend().buffer();
        let rows = (0..height)
            .map(|y| {
                (1..width - 1)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .chars()
                    .filter(|character| !character.is_whitespace())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        let output = rows.join("\n");
        let kana_rows = rows.iter().filter(|row| row.starts_with("かな:")).collect::<Vec<_>>();
        assert!(!kana_rows.is_empty(), "{width}x{height}: {output}");
        for row in kana_rows {
            assert!(row.contains("に"), "{width}x{height}: the kana row must show the current kana: {output}");
        }
        assert!(
            rows.iter().any(|row| row.starts_with("ローマ字:") && row.contains("ni")),
            "{width}x{height}: the romaji row must show the current position: {output}"
        );
        assert!(
            rows.iter()
                .any(|row| row.starts_with("表記:") && row.chars().count() > "表記:".chars().count()),
            "{width}x{height}: {output}"
        );
        assert!(output.contains("[ESC]中断"), "{width}x{height}: {output}");
    }
}

#[test]
fn every_mode_orders_keyboard_timer_metrics_prompt_and_footer() {
    let modes = [
        (ModeKind::Random, Lang::En),
        (ModeKind::Words, Lang::En),
        (ModeKind::Romaji, Lang::Ja),
        (ModeKind::Sentence, Lang::En),
        (ModeKind::Sentence, Lang::Ja),
        (ModeKind::Ngram, Lang::En),
        (ModeKind::Adaptive, Lang::En),
        (ModeKind::Unlock, Lang::En),
        (ModeKind::Rhythm, Lang::En),
        (ModeKind::Burst, Lang::En),
        (ModeKind::Endurance, Lang::En),
    ];
    for (kind, lang) in modes {
        let app = mode_app(kind, lang, |_| {});
        for (width, height) in [(120, 40), (100, 36), (80, 24), (80, 20)] {
            let label = format!("{kind:?}/{lang:?} {width}x{height}");
            let layout = resolve_playing_layout(&app, Rect::new(1, 1, width - 2, height - 2));
            let terminal = render(&app, width, height);
            let output = text(&terminal);
            let buffer = terminal.backend().buffer();
            let row_text = |y: u16| (0..width).map(|x| buffer[(x, y)].symbol()).collect::<String>();
            let metrics_row = compact_row(&terminal, if lang == Lang::Ja { "連続正解" } else { "Combo" });
            let footer_row = compact_row(&terminal, if lang == Lang::Ja { "[ESC]中断" } else { "[ESC]Abort" });
            let timer_row = (0..height).find(|y| {
                let row = row_text(*y).chars().filter(|character| !character.is_whitespace()).collect::<String>();
                row.contains("Wordtimer") || row.contains("単語タイマー")
            });
            assert_eq!(timer_row.is_some(), layout.show_timer, "{label}: {output}");
            // The prompt is top-aligned on the row right below the metrics: a
            // label row ("表記:") for Japanese prompts, otherwise the first
            // large glyph itself (whose top block rows may be blank).
            let game = app.game.as_ref().unwrap();
            let prompt_row = metrics_row + 1;
            if let (None, Some(size)) = (game.current_prompt.reading.as_ref(), layout.bigtext) {
                let first = game.current_prompt.target.chars().next().unwrap();
                let raster = raster_of(size, first);
                assert!(
                    (0..width - raster[0].chars().count() as u16).any(|x| raster_at(&terminal, &raster, x, prompt_row)),
                    "{label}: first large glyph {first:?} must start on row {prompt_row}: {output}"
                );
            } else {
                assert!(
                    row_has_text(&terminal, prompt_row),
                    "{label}: prompt must start below the metrics: {output}"
                );
            }
            if layout.keyboard_style.is_some() {
                let keyboard_row = keys_row_position(&terminal, &['a', 's', 'd', 'f']);
                assert!(
                    keyboard_row < timer_row.unwrap_or(metrics_row),
                    "{label}: keyboard={keyboard_row}, timer={timer_row:?}: {output}"
                );
            }
            if let Some(timer_row) = timer_row {
                assert!(timer_row < metrics_row, "{label}: timer={timer_row}, metrics={metrics_row}");
            }
            assert!(metrics_row < prompt_row && prompt_row < footer_row, "{label}");
        }
    }
}
