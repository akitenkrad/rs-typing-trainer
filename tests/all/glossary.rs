use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::app::{ANALYSIS_TRANSITIONS_TAB, Screen};
use rs_typing_trainer::config::Config;
use rs_typing_trainer::glossary::{self, GlossaryCatalog};
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::ui;

use crate::common::test_app;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn valid_catalog() -> &'static str {
    r#"
version = "glossary-v1"

[[term]]
id = "example"
category = "speed"
term = { en = "Example", ja = "例" }
short = { en = "An example.", ja = "例です．" }
detail = { en = "More detail.", ja = "詳しい説明です．" }
screens = ["menu", "analysis.transitions"]
"#
}

#[test]
fn bundled_glossary_loads_and_has_unique_valid_entries() {
    let catalog = glossary::bundled().expect("bundled glossary");
    assert_eq!(catalog.version(), "glossary-v1");
    assert!(catalog.terms().len() >= 40);
    assert_eq!(catalog.term("wpm").unwrap().term.en, "WPM");
    for (id, en, ja) in [
        ("tempo", "Tempo", "テンポ"),
        ("effective-wpm", "Accuracy-adjusted speed", "正確さを含む速さ"),
        ("error-margin", "This run's error margin", "この回の誤差の目安"),
    ] {
        let term = catalog.term(id).unwrap_or_else(|| panic!("missing glossary term {id}"));
        assert_eq!(term.term.en, en);
        assert_eq!(term.term.ja, ja);
        assert_eq!(term.category, "bench");
        assert!(term.screens.iter().any(|screen| screen == "bench"));
        assert!(term.screens.iter().any(|screen| screen == "history"));
        assert!(term.screens.iter().any(|screen| screen == "result"));
    }
}

#[test]
fn benchmark_and_grade_terms_cover_v3_and_grades_v2() {
    let catalog = glossary::bundled().expect("bundled glossary");

    let series = catalog.term("bench-series").unwrap();
    assert!(series.term.en.contains("bench-v1 / bench-v2 / bench-v3"));
    assert!(series.term.ja.contains("bench-v1 / bench-v2 / bench-v3"));

    let part = catalog.term("bench-part").unwrap();
    for id in ["W", "EW", "ES", "JW", "JS", "PW"] {
        assert!(part.short.en.contains(id), "English bench-part description is missing {id}");
        assert!(part.short.ja.contains(id), "Japanese bench-part description is missing {id}");
    }
    assert!(part.short.en.contains("A–D"));
    assert!(part.short.ja.contains("A〜D"));

    let grade = catalog.term("grade").unwrap();
    assert!(grade.aliases.iter().any(|alias| alias == "grades-v2"));
    let detail = grade.detail.as_ref().unwrap();
    assert!(detail.en.contains("grades-v2") && detail.en.contains("accuracy-adjusted speed"));
    assert!(detail.ja.contains("grades-v2") && detail.ja.contains("正確さを含む速さ"));
}

#[test]
fn glossary_screen_names_match_the_analysis_tab_labels() {
    assert_eq!(glossary::screen_name("analysis.fingers", Lang::En), Some("Analysis · Fingers and hands"));
    assert_eq!(glossary::screen_name("analysis.fingers", Lang::Ja), Some("分析・指・手"));
    assert_eq!(glossary::screen_name("analysis.skills", Lang::Ja), Some("分析・スキル"));
}

#[test]
fn glossary_validation_rejects_every_invalid_rule() {
    let duplicate = format!("{}\n[[term]]{}", valid_catalog(), valid_catalog().split_once("[[term]]").unwrap().1);
    let cases = [
        (valid_catalog().replace("id = \"example\"", "id = \"例\""), "ASCII"),
        (valid_catalog().replace("category = \"speed\"", "category = \"other\""), "category"),
        (valid_catalog().replace("\"menu\"", "\"other\""), "screen"),
        (valid_catalog().replace("en = \"Example\"", "en = \"\""), "term"),
        (valid_catalog().replace("ja = \"例です．\"", "ja = \"\""), "short"),
        (valid_catalog().replace("en = \"More detail.\"", "en = \"\""), "detail"),
        (duplicate, "duplicate"),
    ];
    for (raw, message) in cases {
        let error = glossary::parse(&raw).unwrap_err();
        assert!(error.to_ascii_lowercase().contains(&message.to_ascii_lowercase()), "{message}: {error}");
    }
}

#[test]
fn transitions_scope_contains_only_terms_for_that_tab_then_tab_shows_all() {
    let mut app = test_app(Config::default());
    app.screen = Screen::Analysis;
    app.analysis_tab = ANALYSIS_TRANSITIONS_TAB;
    app.handle_key(key(KeyCode::Char('?')));

    assert_eq!(app.screen, Screen::Glossary);
    let scoped = app.glossary_visible_terms().unwrap();
    assert!(scoped.iter().any(|term| term.term.ja == "遷移"));
    assert!(scoped.iter().any(|term| term.term.en == "n= / n≈"));
    assert!(
        scoped
            .iter()
            .all(|term| term.screens.iter().any(|screen| screen == "analysis.transitions"))
    );
    let scoped_len = scoped.len();

    app.handle_key(key(KeyCode::Tab));
    let all = app.glossary_visible_terms().unwrap();
    assert!(all.len() > scoped_len);
    assert!(all.iter().any(|term| term.id == "raw-wpm"));
}

#[test]
fn every_analysis_tab_filters_to_its_declared_screen() {
    let screens = [
        "analysis.today",
        "analysis.coach",
        "analysis.trends",
        "analysis.keyboard",
        "analysis.transitions",
        "analysis.fingers",
        "analysis.bench",
        "analysis.skills",
        "analysis.growth",
    ];
    for (tab, screen) in screens.into_iter().enumerate() {
        let mut app = test_app(Config::default());
        app.screen = Screen::Analysis;
        app.analysis_tab = tab;
        app.handle_key(key(KeyCode::Char('?')));
        let terms = app.glossary_visible_terms().unwrap();
        assert!(!terms.is_empty(), "{screen}");
        assert!(
            terms.iter().all(|term| term.screens.iter().any(|candidate| candidate == screen)),
            "{screen}: {:?}",
            terms.iter().map(|term| term.id.as_str()).collect::<Vec<_>>()
        );
    }
}

#[test]
fn search_is_case_insensitive_checks_aliases_and_escape_clears_it() {
    let mut app = test_app(Config::default());
    app.screen = Screen::About;
    app.handle_key(key(KeyCode::Char('G')));
    app.handle_key(key(KeyCode::Char('/')));
    for character in ['i', 'K', 'i'] {
        app.handle_key(key(KeyCode::Char(character)));
    }

    let ids = app
        .glossary_visible_terms()
        .unwrap()
        .into_iter()
        .map(|term| term.id.as_str())
        .collect::<Vec<_>>();
    assert!(ids.contains(&"iki"));
    assert!(ids.contains(&"cv"), "IKI CV is an alias of CV: {ids:?}");

    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.screen, Screen::Glossary);
    assert!(app.glossary_query.is_empty());
    assert!(app.glossary_visible_terms().unwrap().len() > ids.len());
}

#[test]
fn glossary_opens_from_every_required_screen_and_returns_without_losing_state() {
    for (origin, code) in [
        (Screen::About, KeyCode::Char('G')),
        (Screen::Menu, KeyCode::Char('?')),
        (Screen::Briefing, KeyCode::Char('?')),
        (Screen::Review, KeyCode::Char('?')),
    ] {
        let mut app = test_app(Config::default());
        app.screen = origin;
        app.handle_key(key(code));
        assert_eq!(app.screen, Screen::Glossary, "{origin:?}");
        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.screen, origin, "{origin:?}");
    }

    let mut analysis = test_app(Config::default());
    analysis.screen = Screen::Analysis;
    analysis.analysis_tab = ANALYSIS_TRANSITIONS_TAB;
    analysis.analysis_transition_selected = 3;
    analysis.handle_key(key(KeyCode::Char('?')));
    analysis.handle_key(key(KeyCode::Esc));
    assert_eq!(analysis.screen, Screen::Analysis);
    assert_eq!(analysis.analysis_tab, ANALYSIS_TRANSITIONS_TAB);
    assert_eq!(analysis.analysis_transition_selected, 3);
}

fn render(app: &rs_typing_trainer::app::App, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect()
}

#[test]
fn glossary_renders_at_required_sizes_languages_and_keeps_footer_last() {
    for lang in [Lang::En, Lang::Ja] {
        for (width, height) in [(80, 24), (120, 40)] {
            let mut config = Config::default();
            config.general.lang = lang;
            let mut app = test_app(config);
            app.screen = Screen::About;
            app.handle_key(key(KeyCode::Char('G')));
            let rows = render(&app, width, height);
            let text = rows.join("\n");
            let expected_title = if lang == Lang::En { "GLOSSARY" } else { "用語集" };
            let compact = text.chars().filter(|character| !character.is_whitespace()).collect::<String>();
            assert!(compact.contains(expected_title), "{lang:?} {width}x{height}\n{text}");
            assert!(rows[usize::from(height - 2)].contains("[Esc/q]"), "{lang:?} {width}x{height}\n{text}");
        }
    }
}

#[test]
fn selected_detail_reaches_its_last_screen_at_both_sizes() {
    for lang in [Lang::En, Lang::Ja] {
        for (width, height) in [(80, 24), (120, 40)] {
            let mut config = Config::default();
            config.general.lang = lang;
            let mut app = test_app(config);
            app.screen = Screen::About;
            app.handle_key(key(KeyCode::Char('G')));
            let count = app.glossary_visible_terms().unwrap().len();
            for selected in 0..count {
                app.glossary_selected = selected;
                let term = app.glossary_visible_terms().unwrap()[selected];
                let last_screen = term.screens.last().unwrap();
                let expected = glossary::screen_name(last_screen, lang)
                    .unwrap()
                    .rsplit(['·', '・'])
                    .next()
                    .unwrap()
                    .trim();
                let text = render(&app, width, height).join("\n");
                let compact = text
                    .chars()
                    .filter(|character| !character.is_whitespace() && *character != '│')
                    .collect::<String>();
                let label = if lang == Lang::En { "Appears on:" } else { "出てくる画面:" };
                let label = label.chars().filter(|character| !character.is_whitespace()).collect::<String>();
                let expected = expected.chars().filter(|character| !character.is_whitespace()).collect::<String>();
                assert!(
                    compact.contains(&label),
                    "{} missing {label:?}, {lang:?} {width}x{height}\n{text}",
                    term.id
                );
                assert!(
                    compact.contains(&expected),
                    "{} missing {expected:?}, {lang:?} {width}x{height}\n{text}",
                    term.id
                );
            }
        }
    }
}

#[test]
fn pure_detail_builder_wraps_without_losing_the_last_text() {
    let catalog = glossary::bundled().unwrap();
    for lang in [Lang::En, Lang::Ja] {
        for width in [28, 60] {
            for term in catalog.terms() {
                let detail = ui::glossary::build_detail(term, lang, width);
                let compact = detail.join("").chars().filter(|character| !character.is_whitespace()).collect::<String>();
                for expected in [term.short.text(lang), term.detail.as_ref().map(|text| text.text(lang)).unwrap_or("")] {
                    let expected = expected.chars().filter(|character| !character.is_whitespace()).collect::<String>();
                    assert!(compact.contains(&expected), "{} {lang:?} width={width}: {detail:?}", term.id);
                }
            }
        }
    }
}

#[test]
fn generated_glossary_docs_match_checked_in_files() {
    let catalog: &GlossaryCatalog = glossary::bundled().unwrap();
    assert_eq!(
        glossary::generate_docs(catalog, Lang::En),
        include_str!("../../docs/glossary.md"),
        "run `cargo run --example gen_glossary_docs`"
    );
    assert_eq!(
        glossary::generate_docs(catalog, Lang::Ja),
        include_str!("../../docs/glossary.ja.md"),
        "run `cargo run --example gen_glossary_docs`"
    );
}
