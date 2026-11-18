//! Layout of the prompt in its two styles (`display.prompt_style`): blank rows
//! around the large glyphs, and the framed card of the normal text.

use std::fs;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use rs_typing_trainer::app::Screen;
use rs_typing_trainer::config::{Config, ModeKind};
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::ui;
use rs_typing_trainer::ui::bigtext::{BigTextSize, StyledCharacter, font};
use rs_typing_trainer::ui::playing::resolve_playing_layout;
use tempfile::TempDir;
use unicode_width::UnicodeWidthStr;

use crate::common;

const QUADRANTS: [&str; 15] = ["▘", "▝", "▖", "▗", "▀", "▄", "▌", "▐", "▚", "▞", "▛", "▜", "▙", "▟", "█"];
const BORDER_SYMBOLS: [&str; 6] = ["│", "┌", "┐", "└", "┘", "─"];

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn start(kind: ModeKind, lang: Lang, configure: impl FnOnce(&mut Config)) -> common::TestApp {
    let mut config = Config::default();
    config.general.lang = lang;
    config.mode.kind = kind;
    config.mode.sentence.lang = match lang {
        Lang::En => "en".to_owned(),
        Lang::Ja => "ja".to_owned(),
    };
    config.ui.animation_enabled = false;
    configure(&mut config);
    let mut app = common::test_app(config);
    let now = Instant::now();
    app.handle_key_at(key(KeyCode::Enter), now);
    app.handle_key_at(key(KeyCode::Enter), now);
    if app.screen == Screen::KeyPicker {
        app.handle_key_at(key(KeyCode::Enter), now);
    }
    app.on_tick_at(Duration::from_secs(3), now).unwrap();
    app.on_tick_at(Duration::from_millis(1), now).unwrap();
    assert_eq!(app.screen, Screen::Playing);
    app
}

fn normal(config: &mut Config) {
    config.display.prompt_style = "normal".to_owned();
}

fn render(app: &common::TestApp, width: u16, height: u16) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    terminal
}

fn text(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The row's symbols between the outer border, without whitespace.
fn compact(terminal: &Terminal<TestBackend>, y: u16) -> String {
    let buffer = terminal.backend().buffer();
    (1..buffer.area.width - 1)
        .map(|x| buffer[(x, y)].symbol())
        .collect::<String>()
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn blank_row(terminal: &Terminal<TestBackend>, y: u16) -> bool {
    compact(terminal, y).is_empty()
}

fn row_of(terminal: &Terminal<TestBackend>, phrase: &str) -> u16 {
    let phrase = phrase.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    (0..terminal.backend().buffer().area.height)
        .find(|y| compact(terminal, *y).contains(&phrase))
        .unwrap_or_else(|| panic!("{phrase:?} was not rendered: {}", text(terminal)))
}

fn position_of(terminal: &Terminal<TestBackend>, symbol: &str) -> (u16, u16) {
    let buffer = terminal.backend().buffer();
    let index = buffer
        .content()
        .iter()
        .position(|cell| cell.symbol() == symbol)
        .unwrap_or_else(|| panic!("{symbol:?} was not rendered: {}", text(terminal)));
    (index as u16 % buffer.area.width, index as u16 / buffer.area.width)
}

/// Column of the first cell of `phrase` on row `y` (wide characters keep a
/// blank continuation cell, which `phrase` must not contain).
fn column_of(terminal: &Terminal<TestBackend>, y: u16, first: &str) -> u16 {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.width)
        .find(|x| buffer[(*x, y)].symbol() == first)
        .unwrap_or_else(|| panic!("{first:?} not on row {y}: {}", text(terminal)))
}

fn column_after(terminal: &Terminal<TestBackend>, y: u16, first: &str, from: u16) -> u16 {
    let buffer = terminal.backend().buffer();
    (from..buffer.area.width)
        .find(|x| buffer[(*x, y)].symbol() == first)
        .unwrap_or_else(|| panic!("{first:?} not on row {y} after {from}: {}", text(terminal)))
}

fn has_quadrants(terminal: &Terminal<TestBackend>, y: u16) -> bool {
    let buffer = terminal.backend().buffer();
    (1..buffer.area.width - 1).any(|x| QUADRANTS.contains(&buffer[(x, y)].symbol()))
}

fn raster(size: BigTextSize, character: char) -> Vec<String> {
    font(size)
        .unwrap()
        .render(&[StyledCharacter::new(character, Style::default())])
        .lines
        .iter()
        .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect())
        .collect()
}

fn find_raster(terminal: &Terminal<TestBackend>, raster: &[String]) -> Option<(u16, u16)> {
    let buffer = terminal.backend().buffer();
    let width = raster[0].chars().count() as u16;
    for y in 0..=buffer.area.height.saturating_sub(raster.len() as u16) {
        for x in 0..=buffer.area.width.saturating_sub(width) {
            if raster.iter().enumerate().all(|(dy, row)| {
                row.chars()
                    .enumerate()
                    .all(|(dx, symbol)| buffer[(x + dx as u16, y + dy as u16)].symbol() == symbol.to_string())
            }) {
                return Some((x, y));
            }
        }
    }
    None
}

fn mistype(app: &mut common::TestApp) {
    let now = Instant::now();
    let expected = app.game.as_ref().unwrap().session.expected().unwrap();
    let wrong = if expected.starts_with('Q') { 'W' } else { 'Q' };
    app.handle_key_at(key(KeyCode::Char(wrong)), now + Duration::from_millis(30));
    // Let the mistype flash fade so the steady helper row is shown.
    app.on_tick_at(Duration::ZERO, now + Duration::from_millis(1_500)).unwrap();
}

// ---------------------------------------------------------------------------
// Large text
// ---------------------------------------------------------------------------

#[test]
fn large_romaji_separates_glyph_blocks_and_kana_with_blank_rows() {
    for (width, height, size) in [(120, 40, BigTextSize::Dot16), (80, 24, BigTextSize::Dot12)] {
        let app = start(ModeKind::Romaji, Lang::Ja, |_| {});
        let game = app.game.as_ref().unwrap();
        let reading = game.current_prompt.reading.clone().unwrap();
        let surface_first = game.current_prompt.text.chars().next().unwrap();
        let terminal = render(&app, width, height);
        let output = text(&terminal);
        let label = format!("{width}x{height}");

        let surface = find_raster(&terminal, &raster(size, surface_first)).unwrap_or_else(|| panic!("{label}: {output}"));
        let surface_bottom = surface.1 + size.cell_height();
        let kana_row = row_of(&terminal, &format!("かな{reading}"));
        assert_eq!(kana_row, surface_bottom + 1, "{label}: one blank row after the surface: {output}");
        assert!(blank_row(&terminal, surface_bottom), "{label}: {output}");

        let romaji_label = row_of(&terminal, "ローマ字");
        assert_eq!(romaji_label, kana_row + 2, "{label}: one blank row after the kana: {output}");
        assert!(blank_row(&terminal, kana_row + 1), "{label}: {output}");

        // Labels sit to the left of their block on its top row, not on a row
        // of their own.
        let surface_label = row_of(&terminal, "表記");
        assert_eq!(surface_label, surface.1, "{label}: {output}");
        assert!(column_of(&terminal, surface_label, "表") < surface.0, "{label}: {output}");
        let kana_label = column_of(&terminal, kana_row, "か");
        let body = column_after(&terminal, kana_row, &reading.chars().next().unwrap().to_string(), kana_label + 4);
        assert!(column_of(&terminal, romaji_label, "ロ") < body, "{label}: {output}");
        // The romaji glyphs and their cursor follow the label row.
        assert!(
            (romaji_label..romaji_label + size.cell_height()).any(|y| has_quadrants(&terminal, y)),
            "{label}: {output}"
        );
        let (_, cursor_row) = position_of(&terminal, "▔");
        assert_eq!(cursor_row, romaji_label + size.cell_height(), "{label}: {output}");
    }
}

#[test]
fn large_sentence_leaves_a_blank_row_before_the_full_text() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sentences.tsv");
    fs::write(&path, "Tom is obsessed with the number three.\n").unwrap();
    let app = start(ModeKind::Sentence, Lang::En, |config| config.data.sentences_en = Some(path));
    for (width, height) in [(120, 40), (80, 24)] {
        let terminal = render(&app, width, height);
        let output = text(&terminal);
        let (_, cursor_row) = position_of(&terminal, "▔");
        let full_row = row_of(&terminal, "Tom is obsessed");
        assert_eq!(full_row, cursor_row + 2, "{width}x{height}: {output}");
        assert!(blank_row(&terminal, cursor_row + 1), "{width}x{height}: {output}");
        let style = terminal.backend().buffer()[(column_of(&terminal, full_row, "T"), full_row)].style();
        assert!(
            style.add_modifier.contains(Modifier::REVERSED),
            "{width}x{height}: current position must be reversed"
        );
    }
}

#[test]
fn large_word_helper_row_follows_a_blank_row_when_there_is_room() {
    let mut app = start(ModeKind::Words, Lang::En, |_| {});
    mistype(&mut app);

    let roomy = render(&app, 120, 40);
    let output = text(&roomy);
    let (_, cursor_row) = position_of(&roomy, "▔");
    assert_eq!(row_of(&roomy, "Mistypes in word: 1"), cursor_row + 2, "{output}");
    assert!(blank_row(&roomy, cursor_row + 1), "{output}");

    // At 80x24 the keyboard and timer leave no spare row: the blank row and
    // helper give way, the glyphs and cursor stay.
    let tight = render(&app, 80, 24);
    let output = text(&tight);
    let (_, cursor_row) = position_of(&tight, "▔");
    assert!(!output.contains("Mistypes in word"), "{output}");
    assert!(has_quadrants(&tight, cursor_row - 2), "{output}");
}

// ---------------------------------------------------------------------------
// Normal text
// ---------------------------------------------------------------------------

#[test]
fn normal_word_sits_in_a_rounded_card_with_spaced_letters_and_a_reversed_cursor() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("words.tsv");
    fs::write(&path, "rust\n").unwrap();
    let mut app = start(ModeKind::Words, Lang::En, |config| {
        normal(config);
        config.data.words_en = Some(path);
        config.mode.keys = "r".into();
    });
    assert_eq!(app.game.as_ref().unwrap().current_prompt.target, "rust");
    app.handle_key_at(key(KeyCode::Char('r')), Instant::now() + Duration::from_millis(20));
    for (width, height) in [(120, 40), (80, 24)] {
        let layout = resolve_playing_layout(&app, Rect::new(1, 1, width - 2, height - 2));
        let terminal = render(&app, width, height);
        let output = text(&terminal);
        let buffer = terminal.backend().buffer();
        let row = row_of(&terminal, "rust");
        assert!(text(&terminal).contains("r u s t"), "{output}");
        let r = column_of(&terminal, row, "r");
        for (offset, letter) in ["r", "u", "s", "t"].into_iter().enumerate() {
            assert_eq!(buffer[(r + 2 * offset as u16, row)].symbol(), letter, "{output}");
            if offset < 3 {
                assert_eq!(buffer[(r + 2 * offset as u16 + 1, row)].symbol(), " ", "one blank cell between letters");
            }
        }
        let current = buffer[(r + 2, row)].style();
        assert!(current.add_modifier.contains(Modifier::REVERSED), "{output}");
        assert!(!buffer[(r + 4, row)].style().add_modifier.contains(Modifier::REVERSED));
        let (caret_x, caret_y) = position_of(&terminal, "▲");
        assert_eq!((caret_x, caret_y), (r + 2, row + 1), "caret right under the current letter: {output}");

        let top_left = position_of(&terminal, "╭");
        let bottom_right = position_of(&terminal, "╯");
        assert!(top_left.0 + 2 < r && r + 7 < bottom_right.0, "two columns of padding: {output}");
        assert_eq!(top_left.1 + 2, row, "one row of padding above: {output}");
        assert_eq!(bottom_right.1, caret_y + 2, "one row of padding below: {output}");
        assert_eq!(
            top_left.1,
            row_of(&terminal, "Combo") + 1 + layout.prompt_gap_rows,
            "card right under the metrics gap: {output}"
        );
        let centre = f32::from(top_left.0 + bottom_right.0) / 2.0;
        assert!((centre - f32::from(width - 1) / 2.0).abs() <= 1.0, "card is centred: {output}");
    }
}

#[test]
fn normal_romaji_aligns_labels_and_bodies_in_two_columns_with_blank_rows() {
    for lang in [Lang::Ja, Lang::En] {
        let app = start(ModeKind::Romaji, lang, normal);
        let game = app.game.as_ref().unwrap();
        let surface = game.current_prompt.text.clone();
        let reading = game.current_prompt.reading.clone().unwrap();
        let guide = game.session.current_guide();
        let [surface_label, kana_label, romaji_label] = match lang {
            Lang::Ja => ["表記", "かな", "ローマ字"],
            Lang::En => ["Surface", "Kana", "Romaji"],
        };
        for (width, height) in [(120, 40), (80, 24)] {
            let terminal = render(&app, width, height);
            let output = text(&terminal);
            let label = format!("{lang:?} {width}x{height}");
            let rows = [
                row_of(&terminal, &format!("{surface_label}{surface}")),
                row_of(&terminal, &format!("{kana_label}{reading}")),
                row_of(&terminal, &format!("{romaji_label}{guide}")),
            ];
            assert_eq!(rows[1], rows[0] + 2, "{label}: {output}");
            assert_eq!(rows[2], rows[1] + 2, "{label}: {output}");
            assert!(blank_row_inside_card(&terminal, rows[0] + 1), "{label}: {output}");
            assert!(blank_row_inside_card(&terminal, rows[1] + 1), "{label}: {output}");

            let label_columns = [surface_label, kana_label, romaji_label]
                .iter()
                .zip(rows)
                .map(|(text, row)| column_of(&terminal, row, &text.chars().next().unwrap().to_string()))
                .collect::<Vec<_>>();
            assert!(
                label_columns.windows(2).all(|pair| pair[0] == pair[1]),
                "{label}: {label_columns:?}: {output}"
            );
            // Search after the widest label so a body starting with a letter
            // of its label (e.g. "a" in "Romaji") is found in the body.
            let after = label_columns[0] + UnicodeWidthStr::width(romaji_label.max(surface_label)) as u16;
            let body_columns = [&surface, &reading, &guide]
                .iter()
                .zip(rows)
                .map(|(text, row)| column_after(&terminal, row, &text.chars().next().unwrap().to_string(), after))
                .collect::<Vec<_>>();
            assert!(body_columns.windows(2).all(|pair| pair[0] == pair[1]), "{label}: {body_columns:?}");
            assert!(label_columns[0] < body_columns[0], "{label}");

            let buffer = terminal.backend().buffer();
            assert!(buffer[(body_columns[1], rows[1])].style().add_modifier.contains(Modifier::REVERSED));
            assert!(buffer[(body_columns[2], rows[2])].style().add_modifier.contains(Modifier::REVERSED));
            assert!(buffer[(body_columns[0], rows[0])].style().add_modifier.contains(Modifier::BOLD));
            assert_eq!(buffer[(label_columns[0], rows[0])].style().fg, Some(ratatui::style::Color::DarkGray));
            let top_left = position_of(&terminal, "╭");
            let bottom_right = position_of(&terminal, "╯");
            assert!(top_left.1 < rows[0] && rows[2] < bottom_right.1, "{label}: {output}");
        }
    }
}

fn blank_row_inside_card(terminal: &Terminal<TestBackend>, y: u16) -> bool {
    compact(terminal, y).chars().all(|character| character == '│')
}

#[test]
fn normal_sentence_brightens_the_current_row_and_puts_the_caret_under_it() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sentences.tsv");
    fs::write(
        &path,
        "A patient typist follows the winding trail and keeps a calm steady rhythm through every turn of the long road.\n",
    )
    .unwrap();
    let mut app = start(ModeKind::Sentence, Lang::En, |config| {
        normal(config);
        config.data.sentences_en = Some(path);
    });
    let now = Instant::now();
    for step in 0..80 {
        let expected = app.game.as_ref().unwrap().session.expected().unwrap().chars().next().unwrap();
        app.handle_key_at(key(KeyCode::Char(expected)), now + Duration::from_millis(step * 40 + 40));
    }
    let terminal = render(&app, 80, 24);
    let output = text(&terminal);
    let buffer = terminal.backend().buffer();
    let (caret_x, caret_y) = position_of(&terminal, "▲");
    let current = buffer[(caret_x, caret_y - 1)].style();
    assert!(current.add_modifier.contains(Modifier::REVERSED), "{output}");
    let first_row = row_of(&terminal, "A patient");
    assert!(first_row < caret_y - 1, "the sentence wraps: {output}");
    let first = buffer[(column_of(&terminal, first_row, "A"), first_row)].style();
    assert!(
        first.add_modifier.contains(Modifier::DIM),
        "rows other than the current one are dimmed: {output}"
    );
    let top_left = position_of(&terminal, "╭");
    assert_eq!(column_of(&terminal, first_row, "A"), top_left.0 + 3, "{output}");
}

#[test]
fn normal_blind_word_does_not_draw_typed_letters() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("words.tsv");
    fs::write(&path, "rust\n").unwrap();
    let mut app = start(ModeKind::Words, Lang::En, |config| {
        normal(config);
        config.session.blind = true;
        config.data.words_en = Some(path);
        config.mode.keys = "r".into();
    });
    app.handle_key_at(key(KeyCode::Char('r')), Instant::now() + Duration::from_millis(20));
    let terminal = render(&app, 80, 24);
    let output = text(&terminal);
    assert!(output.contains("  u s t"), "{output}");
    assert!(!output.contains("r u s t"), "{output}");
}

// ---------------------------------------------------------------------------
// Every mode, both styles
// ---------------------------------------------------------------------------

#[test]
fn every_mode_in_both_styles_keeps_the_order_and_stays_inside_the_frame() {
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
    for style in ["large", "normal"] {
        for (kind, lang) in modes {
            let app = start(kind, lang, |config| config.display.prompt_style = style.to_owned());
            for (width, height) in [(120, 40), (100, 36), (80, 24), (80, 20)] {
                let label = format!("{style} {kind:?}/{lang:?} {width}x{height}");
                let layout = resolve_playing_layout(&app, Rect::new(1, 1, width - 2, height - 2));
                if style == "normal" {
                    assert_eq!(layout.bigtext, None, "{label}");
                }
                let terminal = render(&app, width, height);
                let output = text(&terminal);
                let buffer = terminal.backend().buffer();
                let metrics = row_of(&terminal, if lang == Lang::Ja { "連続正解" } else { "Combo" });
                let footer = row_of(&terminal, if lang == Lang::Ja { "[ESC]中断" } else { "[ESC]Abort" });
                let timer = (0..height).find(|y| {
                    let row = compact(&terminal, *y);
                    row.contains("Wordtimer") || row.contains("単語タイマー")
                });
                if layout.keyboard_style.is_some() {
                    let keyboard = (0..height)
                        .find(|y| {
                            let row = (0..width).map(|x| buffer[(x, *y)].symbol()).collect::<String>();
                            ['a', 's', 'd', 'f'].iter().all(|key| row.contains(*key))
                        })
                        .unwrap_or_else(|| panic!("{label}: keyboard missing: {output}"));
                    assert!(keyboard < timer.unwrap_or(metrics), "{label}: {output}");
                }
                if let Some(timer) = timer {
                    assert!(timer < metrics, "{label}: {output}");
                }
                // Large glyphs may have blank top rows; their block starts
                // after the optional metrics gap.
                let prompt_start = metrics + 1 + layout.prompt_gap_rows;
                let first_rows = if layout.bigtext.is_some() {
                    prompt_start..prompt_start + 3
                } else {
                    prompt_start..prompt_start + 1
                };
                assert!(
                    first_rows.clone().any(|y| !blank_row(&terminal, y)),
                    "{label}: prompt must start below the metrics: {output}"
                );
                assert!(metrics + 1 < footer, "{label}: {output}");
                for y in metrics + 1..footer {
                    for x in [0, width - 1] {
                        assert!(BORDER_SYMBOLS.contains(&buffer[(x, y)].symbol()), "{label}: frame overwritten: {output}");
                    }
                    for x in [1, width - 2] {
                        assert_eq!(buffer[(x, y)].symbol(), " ", "{label}: prompt touches the frame at ({x}, {y}): {output}");
                    }
                }
                if style == "normal" {
                    let (_, card_top) = position_of(&terminal, "╭");
                    let (_, card_bottom) = position_of(&terminal, "╯");
                    assert!(metrics < card_top && card_bottom < footer, "{label}: {output}");
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Choosing the style
// ---------------------------------------------------------------------------

#[test]
fn z_on_mode_select_toggles_the_prompt_style_and_saves_it() {
    for lang in [Lang::En, Lang::Ja] {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(&path, "[display]\nprompt_size = \"xlarge\"\n").unwrap();
        let mut config = Config::from_path(&path).unwrap();
        config.general.lang = lang;
        let mut app = common::test_app(config);
        app.config_path = Some(path.clone());
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.screen, Screen::ModeSelect);
        let (hint_large, hint_normal) = match lang {
            Lang::En => ("[Z]Prompttext:Largetext", "[Z]Prompttext:Normaltext"),
            Lang::Ja => ("[Z]問題文の表示:大きな文字", "[Z]問題文の表示:通常の文字"),
        };
        let before = render(&app, 80, 24);
        assert!(text(&before).replace(' ', "").contains(hint_large), "{}", text(&before));

        app.handle_key(key(KeyCode::Char('z')));
        assert_eq!(app.config.display.prompt_style, "normal");
        let after = render(&app, 80, 24);
        assert!(text(&after).replace(' ', "").contains(hint_normal), "{}", text(&after));
        app.handle_key(key(KeyCode::Char('Z')));
        assert_eq!(app.config.display.prompt_style, "large");
        app.handle_key(key(KeyCode::Char('z')));

        // Starting a mode saves the choice under the new key.
        app.handle_key(key(KeyCode::Enter));
        assert!(matches!(app.screen, Screen::Countdown { .. }), "{:?}", app.screen);
        let saved = fs::read_to_string(&path).unwrap();
        assert!(saved.contains("prompt_style = \"normal\""), "{saved}");
        assert!(!saved.contains("prompt_size"), "{saved}");
    }
}

#[test]
fn settings_screen_names_the_prompt_style_in_both_languages() {
    for (lang, name, value, normal_value) in [
        (Lang::En, "Prompt text", "< Large text >", "< Normal text >"),
        (Lang::Ja, "問題文の表示", "< 大きな文字 >", "< 通常の文字 >"),
    ] {
        let mut config = Config::default();
        config.general.lang = lang;
        let mut app = common::test_app(config);
        app.handle_key(key(KeyCode::Char('s')));
        for _ in 0..5 {
            app.handle_key(key(KeyCode::Down));
        }
        let terminal = render(&app, 80, 24);
        let output = text(&terminal);
        let compact_output = output.replace(' ', "");
        assert!(compact_output.contains(&name.replace(' ', "")), "{output}");
        assert!(compact_output.contains(&value.replace(' ', "")), "{output}");
        let shrink = match lang {
            Lang::En => "smaller automatically",
            Lang::Ja => "自動で小さくなる",
        };
        assert!(compact_output.contains(&shrink.replace(' ', "")), "{output}");
        app.handle_key(key(KeyCode::Right));
        let terminal = render(&app, 80, 24);
        assert!(text(&terminal).replace(' ', "").contains(&normal_value.replace(' ', "")));
    }
}
