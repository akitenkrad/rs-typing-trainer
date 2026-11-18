use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::app::Screen;
use rs_typing_trainer::config::Config;
use rs_typing_trainer::graphics::{CellPixels, GraphicsDetection, GraphicsMode, GraphicsReason};
use rs_typing_trainer::i18n::{Lang, Msg};
use rs_typing_trainer::ui;

use crate::common;
use common::test_app;

fn render_title(lang: Lang) -> String {
    let mut config = Config::default();
    config.general.lang = lang;
    render_title_with_config(config)
}

fn render_title_with_config(config: Config) -> String {
    let app = test_app(config);
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| ui::title::render(frame, &app)).unwrap();

    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<Vec<_>>()
        .join("")
}

fn without_whitespace(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn render_app(app: &rs_typing_trainer::app::App) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| ui::title::render(frame, app)).unwrap();
    terminal
}

fn render_app_size(app: &rs_typing_trainer::app::App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::title::render(frame, app)).unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<Vec<_>>()
        .join("")
}

fn symbol_column(terminal: &Terminal<TestBackend>, symbol: &str) -> u16 {
    let buffer = terminal.backend().buffer();
    buffer
        .content()
        .iter()
        .enumerate()
        .find(|(_, cell)| cell.symbol() == symbol)
        .map(|(index, _)| index as u16 % buffer.area.width)
        .unwrap_or_else(|| panic!("symbol {symbol:?} was not rendered"))
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

fn row_containing(terminal: &Terminal<TestBackend>, phrase: &str) -> u16 {
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        let row = (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>();
        if without_whitespace(&row).contains(&without_whitespace(phrase)) {
            return y;
        }
    }
    panic!("phrase {phrase:?} was not rendered");
}

fn colon_columns(terminal: &Terminal<TestBackend>, row: u16) -> Vec<u16> {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.width).filter(|x| buffer[(*x, row)].symbol() == ":").collect()
}

#[test]
fn title_uses_english_messages() {
    let rendered = render_title(Lang::En);

    assert!(without_whitespace(&rendered).contains(&without_whitespace(Msg::TitleLogo.text(Lang::En))));
    assert!(without_whitespace(&rendered).contains(&without_whitespace(Msg::TitleTagline.text(Lang::En))));
    assert!(rendered.contains("Benchmark  [B]"), "{rendered}");
}

#[test]
fn title_uses_japanese_messages() {
    let rendered = render_title(Lang::Ja);

    assert!(without_whitespace(&rendered).contains(&without_whitespace(Msg::TitleLogo.text(Lang::Ja))));
    assert!(
        without_whitespace(&rendered).contains(&without_whitespace(Msg::TitleTagline.text(Lang::Ja))),
        "rendered buffer:\n{rendered}"
    );
}

#[test]
fn title_uses_the_product_name_and_localizes_the_practice_label() {
    for lang in [Lang::En, Lang::Ja] {
        let rendered = render_title(lang);
        assert!(
            rendered.contains(concat!("rs-typing-trainer  v", env!("CARGO_PKG_VERSION"))),
            "{lang:?}: {rendered}"
        );
        assert!(!rendered.contains("RS -"), "{lang:?}: {rendered}");
        assert!(
            without_whitespace(&rendered).contains(&without_whitespace(Msg::Practice.text(lang))),
            "{lang:?}: {rendered}"
        );
    }
    assert!(!render_title(Lang::Ja).contains("Practice"));
}

#[test]
fn title_information_uses_unicode_width_to_align_label_and_value_columns() {
    for lang in [Lang::En, Lang::Ja] {
        let mut config = Config::default();
        config.general.lang = lang;
        let app = test_app(config);
        let terminal = render_app(&app);
        let primary_labels = [Msg::Practice, Msg::Charset, Msg::Duration, Msg::WordLength, Msg::Database];
        let primary_colons = primary_labels
            .iter()
            .map(|message| {
                let row = row_containing(&terminal, message.text(lang));
                colon_columns(&terminal, row)[0]
            })
            .collect::<Vec<_>>();
        assert!(
            primary_colons.windows(2).all(|columns| columns[0] == columns[1]),
            "{lang:?}: {primary_colons:?}"
        );

        let secondary_colons = [Msg::WordTimeout, Msg::Animation]
            .iter()
            .map(|message| {
                let row = row_containing(&terminal, message.text(lang));
                *colon_columns(&terminal, row).last().unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(secondary_colons[0], secondary_colons[1], "{lang:?}: {secondary_colons:?}");
    }
}

#[test]
fn title_truncates_unicode_charset_at_grapheme_boundaries() {
    let mut config = Config::default();
    config.characters.charset = format!("{}🍎xyz", "a".repeat(39));

    let rendered = render_title_with_config(config);

    assert!(rendered.contains('🍎'));
}

#[test]
fn title_shows_database_copy_notice_in_both_languages() {
    for lang in [Lang::En, Lang::Ja] {
        let mut config = Config::default();
        config.general.lang = lang;
        let mut app = test_app(config);
        app.storage_notice = true;
        let backend = TestBackend::new(100, 28);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| ui::title::render(frame, &app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<Vec<_>>()
            .join("");
        assert!(without_whitespace(&rendered).contains(&without_whitespace(Msg::LegacyDatabaseCopied.text(lang))));
    }
}

#[test]
fn title_replaces_the_old_recommendations_with_one_introductory_menu_line_at_80_by_24() {
    let mut app = test_app(Config::default());
    app.storage_notice = true;
    app.recommend_preview = true;
    app.recommend_rhythm = true;
    let terminal = render_app(&app);
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<Vec<_>>()
        .join("");

    assert!(rendered.to_lowercase().contains("copied"), "{rendered}");
    assert!(rendered.contains("Today's training  [M]"), "{rendered}");
    // The yellow «Today's menu — M to open» line under the settings is gone.
    assert!(!rendered.contains("M to open"), "{rendered}");
    assert!(!rendered.contains("preview items"), "{rendered}");
    assert!(!rendered.contains("Rhythm practice"), "{rendered}");
}

#[test]
fn title_lists_todays_training_first_and_individual_training_second_in_japanese() {
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    let rendered = render_title_with_config(config);
    let compact = without_whitespace(&rendered);

    let today = compact.find("今日の訓練[M]").expect(&rendered);
    let individual = compact.find("個別に訓練[Space]").expect(&rendered);
    assert!(today < individual, "{rendered}");
    assert!(!compact.contains("Mで開く"), "{rendered}");
    assert!(!compact.contains("今日のメニュー"), "{rendered}");
}

#[test]
fn title_menu_moves_with_arrows_and_vi_keys_and_enter_runs_the_selection() {
    let mut app = test_app(Config::default());

    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.screen, Screen::BenchSelect);

    let mut app = test_app(Config::default());
    app.handle_key(key(KeyCode::Char('j')));
    app.handle_key(key(KeyCode::Char('j')));
    app.handle_key(key(KeyCode::Char('k')));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.screen, Screen::BenchSelect);
}

#[test]
fn title_menu_keeps_all_shortcuts() {
    for (code, expected) in [
        (KeyCode::Char(' '), Some(Screen::ModeSelect)),
        (KeyCode::Char('b'), Some(Screen::BenchSelect)),
        (KeyCode::Char('a'), Some(Screen::Analysis)),
        (KeyCode::Char('m'), Some(Screen::Menu)),
        (KeyCode::Char('h'), Some(Screen::History)),
        (KeyCode::Char('s'), Some(Screen::Settings)),
    ] {
        let mut app = test_app(Config::default());
        app.handle_key(key(code));
        assert_eq!(Some(app.screen), expected);
    }

    for code in [KeyCode::Char('q'), KeyCode::Esc] {
        let mut app = test_app(Config::default());
        app.handle_key(key(code));
        assert!(app.should_quit);
    }
}

#[test]
fn title_menu_is_left_aligned_and_only_the_selection_has_an_arrow_in_both_languages() {
    for lang in [Lang::En, Lang::Ja] {
        let mut config = Config::default();
        config.general.lang = lang;
        let app = test_app(config);
        let terminal = render_app(&app);
        let buffer = terminal.backend().buffer();
        let arrows = buffer.content().iter().filter(|cell| cell.symbol() == "▶").count();
        assert_eq!(arrows, 1);
        assert_eq!(symbol_column(&terminal, "▶"), 3);

        let columns = match lang {
            Lang::En => [
                "Today's training",
                "Individual training",
                "Benchmark",
                "Analysis",
                "History",
                "Settings",
                "Quit",
            ]
            .iter()
            .map(|phrase| phrase_column(&terminal, phrase))
            .collect::<Vec<_>>(),
            Lang::Ja => ["今", "個", "ベ", "分", "履", "設", "終"]
                .iter()
                .map(|symbol| symbol_column(&terminal, symbol))
                .collect::<Vec<_>>(),
        };
        assert!(columns.windows(2).all(|pair| pair[0] == pair[1]), "{lang:?}: {columns:?}");
        assert_eq!(columns[0], 6);
    }
}

#[test]
fn title_screen_keeps_all_actions_with_and_without_images_at_large_and_small_sizes() {
    for lang in [Lang::En, Lang::Ja] {
        let expected = match lang {
            Lang::En => [
                "Today'straining",
                "Individualtraining",
                "Benchmark",
                "Analysis",
                "History",
                "Settings",
                "Quit",
            ],
            Lang::Ja => ["今日の訓練", "個別に訓練", "ベンチマーク", "分析", "履歴", "設定", "終了"],
        };
        for images in [false, true] {
            for (width, height) in [(150, 40), (80, 24)] {
                let mut config = Config::default();
                config.general.lang = lang;
                let mut app = test_app(config);
                if images {
                    app.set_graphics_detection(GraphicsDetection {
                        mode: GraphicsMode::Kitty,
                        cell_pixels: Some(CellPixels::new(9, 20)),
                        reason: GraphicsReason::Explicit(GraphicsMode::Kitty),
                    });
                }

                let output = render_app_size(&app, width, height);
                let compact = without_whitespace(&output);
                for label in expected {
                    assert!(
                        compact.contains(label),
                        "missing {label:?}: lang={lang:?} images={images} size={width}x{height}"
                    );
                }
                assert!(compact.contains(&without_whitespace(Msg::TitleTagline.text(lang))));
                let uses_image = images && (width, height) == (150, 40);
                assert_eq!(
                    compact.contains(&without_whitespace(Msg::TitleLogo.text(lang))),
                    !uses_image,
                    "unexpected logo mode: lang={lang:?} images={images} size={width}x{height}"
                );
            }
        }
    }
}

#[test]
fn a_new_app_starts_on_todays_training_and_enter_opens_the_menu() {
    let home = tempfile::TempDir::new().expect("temporary home");
    let mut config = Config::default();
    config.storage.db_path = Some(home.path().join("data").join("sessions.db"));
    let mut app = rs_typing_trainer::app::App::new(config, home.path().to_path_buf());
    assert_eq!(app.title_selected, 0);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.screen, Screen::Menu);
}
