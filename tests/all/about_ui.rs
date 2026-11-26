use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::app::Screen;
use rs_typing_trainer::build_info;
use rs_typing_trainer::config::{BenchSeries, Config};
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::ui;

use crate::common::test_app;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn without_whitespace(text: &str) -> String {
    text.chars().filter(|character| !character.is_whitespace()).collect()
}

fn render(app: &rs_typing_trainer::app::App, width: u16, height: u16) -> (String, Vec<String>) {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::about::render(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    let rows = (0..height)
        .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>();
    (rows.join("\n"), rows)
}

fn all_scrolled_text(lang: Lang, width: u16, height: u16) -> String {
    let mut config = Config::default();
    config.general.lang = lang;
    let mut app = test_app(config);
    app.screen = Screen::About;
    let mut output = String::new();
    for _ in 0..100 {
        let (frame, _) = render(&app, width, height);
        output.push_str(&frame);
        let before = app.about_scroll;
        app.handle_key(key(KeyCode::PageDown));
        if app.about_scroll == before {
            break;
        }
    }
    output
}

#[test]
fn about_renders_version_catalogs_and_legal_metadata_at_both_sizes() {
    for lang in [Lang::En, Lang::Ja] {
        for (width, height) in [(80, 24), (120, 40)] {
            let output = all_scrolled_text(lang, width, height);
            for expected in [
                build_info::VERSION,
                build_info::GIT_HASH,
                "MIT",
                env!("CARGO_PKG_REPOSITORY"),
                "menu-v1",
                "briefing-v1",
                "grades-v1 / grades-v2",
                "bench-v3",
            ] {
                assert!(
                    output.contains(expected),
                    "missing {expected:?}: lang={lang:?} size={width}x{height}\n{output}"
                );
            }
            let localized = match lang {
                Lang::En => ["ABOUT", "Version", "Commit", "Repository", "Bundled data sources"],
                Lang::Ja => ["このアプリについて", "バージョン", "コミット", "リポジトリ", "同梱データの出典"],
            };
            for expected in localized {
                assert!(
                    without_whitespace(&output).contains(&without_whitespace(expected)),
                    "missing {expected:?}: lang={lang:?} size={width}x{height}\n{output}"
                );
            }
        }
    }
}

#[test]
fn about_reports_the_configured_benchmark_series() {
    let default_app = test_app(Config::default());
    assert_eq!(ui::about::AboutInfo::from_app(&default_app).bench_series, "bench-v3");

    let mut config = Config::default();
    config.bench.series = BenchSeries::V2;
    let v2_app = test_app(config);
    assert_eq!(ui::about::AboutInfo::from_app(&v2_app).bench_series, "bench-v2");
}

#[test]
fn about_keeps_the_hint_on_the_last_inner_row() {
    for lang in [Lang::En, Lang::Ja] {
        for (width, height) in [(80, 24), (120, 40)] {
            let mut config = Config::default();
            config.general.lang = lang;
            let mut app = test_app(config);
            app.screen = Screen::About;
            let (_, rows) = render(&app, width, height);
            let hint = match lang {
                Lang::En => "[↑/↓] Scroll    [Esc] Back",
                Lang::Ja => "[↑/↓] スクロール    [Esc] 戻る",
            };
            assert!(
                without_whitespace(&rows[usize::from(height - 2)]).contains(&without_whitespace(hint)),
                "lang={lang:?} size={width}x{height}\n{}",
                rows.join("\n")
            );
        }
    }
}

#[test]
fn about_scroll_stops_at_both_edges() {
    let mut app = test_app(Config::default());
    app.screen = Screen::About;
    render(&app, 80, 24);

    for _ in 0..100 {
        app.handle_key(key(KeyCode::PageDown));
        render(&app, 80, 24);
    }
    let bottom = app.about_scroll;
    assert!(bottom > 0);
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.about_scroll, bottom);

    for _ in 0..100 {
        app.handle_key(key(KeyCode::PageUp));
    }
    assert_eq!(app.about_scroll, 0);
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.about_scroll, 0);
}

#[test]
fn every_about_close_key_returns_to_the_title() {
    for code in [KeyCode::Esc, KeyCode::Enter, KeyCode::Char('q'), KeyCode::Char('?')] {
        let mut app = test_app(Config::default());
        app.screen = Screen::About;
        app.handle_key(key(code));
        assert_eq!(app.screen, Screen::Title, "{code:?}");
    }
}

#[test]
fn about_explains_what_key_release_timing_is_for() {
    for (lang, label, value) in [
        (Lang::Ja, "キーを離した時刻", "取れない（この端末では重なりと押下時間を測りません）"),
        (
            Lang::En,
            "Key-release timing",
            "unavailable (this terminal cannot measure rollover or key hold times)",
        ),
    ] {
        // The test app has no terminal, so key releases are unavailable.
        let text = without_whitespace(&all_scrolled_text(lang, 120, 40));
        assert!(text.contains(&without_whitespace(label)), "{lang:?}: {text}");
        assert!(text.contains(&without_whitespace(value)), "{lang:?}: {text}");
    }
}
