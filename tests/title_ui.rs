use ratatui::Terminal;
use ratatui::backend::TestBackend;
use rs_typing_trainer::config::Config;
use rs_typing_trainer::i18n::{Lang, Msg};
use rs_typing_trainer::ui;

mod common;
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

#[test]
fn title_uses_english_messages() {
    let rendered = render_title(Lang::En);

    assert!(without_whitespace(&rendered).contains(&without_whitespace(Msg::TitleLogo.text(Lang::En))));
    assert!(without_whitespace(&rendered).contains(&without_whitespace(Msg::TitleTagline.text(Lang::En))));
    assert!(rendered.contains("[B] Benchmark"), "{rendered}");
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
