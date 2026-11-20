//! MYTASK-3637: the source of a bundled sentence is shown in one muted row
//! under the prompt being typed — the Aozora Bunko work for Japanese, the
//! Tatoeba number for English — and nowhere for prompts without one.

use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use rs_typing_trainer::app::Screen;
use rs_typing_trainer::config::{Config, DataConfig, ModeKind};
use rs_typing_trainer::data::{DataCatalog, DataKind, PromptSource};
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::ui;
use rs_typing_trainer::ui::playing::resolve_playing_layout;
use tempfile::TempDir;

use crate::common;

const MANIFEST: &str = include_str!("../../data/manifest.toml");
const AOZORA_WORKS_GZ: &[u8] = include_bytes!("../../data/aozora_works.tsv.gz");
const JA_SENTENCES_GZ: &[u8] = include_bytes!("../../data/ja_sentences.tsv.gz");

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn start(kind: ModeKind, lang: Lang, data_lang: &str, configure: impl FnOnce(&mut Config)) -> common::TestApp {
    let mut config = Config::default();
    config.general.lang = lang;
    config.mode.kind = kind;
    config.mode.sentence.lang = data_lang.to_owned();
    config.mode.phrase.lang = data_lang.to_owned();
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

fn render(app: &common::TestApp, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect()
}

/// A row without whitespace (full-width characters are followed by an
/// empty cell).
fn compact(row: &str) -> String {
    row.chars().filter(|character| !character.is_whitespace()).collect()
}

fn row_of(rows: &[String], phrase: &str) -> Option<usize> {
    let phrase = compact(phrase);
    rows.iter().position(|row| compact(row).contains(&phrase))
}

fn screen(rows: &[String]) -> String {
    rows.join("\n")
}

fn source_label(app: &common::TestApp, lang: Lang) -> Option<String> {
    app.game.as_ref().unwrap().current_prompt.source.as_ref().map(|source| source.label(lang))
}

/// The source row sits under the prompt and above the footer, the footer
/// (its abort hint) stays on the last inner row, and the footer's preview of
/// the next prompts carries no source.
fn assert_source_under_prompt(rows: &[String], label: &str, footer_hint: &str, metrics: &str) {
    let output = screen(rows);
    let source = row_of(rows, label).unwrap_or_else(|| panic!("source {label:?} missing:\n{output}"));
    let footer = row_of(rows, footer_hint).unwrap_or_else(|| panic!("footer missing:\n{output}"));
    let metrics = row_of(rows, metrics).unwrap_or_else(|| panic!("metrics missing:\n{output}"));
    assert_eq!(footer, rows.len() - 2, "footer on the last inner row:\n{output}");
    assert!(metrics < source && source < footer, "source between the prompt and the footer:\n{output}");
    assert!(!compact(&rows[footer]).contains(&compact(label)), "no source in the preview:\n{output}");
}

#[test]
fn japanese_sentence_shows_its_aozora_work_under_the_prompt() {
    for style in ["large", "normal"] {
        let app = start(ModeKind::Sentence, Lang::Ja, "ja", |config| {
            config.display.prompt_style = style.to_owned()
        });
        let prompt = app.game.as_ref().unwrap().current_prompt.clone();
        let Some(PromptSource::Aozora { author, title, work_id }) = prompt.source.clone() else {
            panic!("{style}: bundled Japanese sentence without a source: {prompt:?}");
        };
        assert_eq!(work_id.len(), 6, "{work_id}");
        let label = source_label(&app, Lang::Ja).unwrap();
        assert_eq!(label, format!("{author}『{title}』（青空文庫）"));
        for (width, height) in [(120, 40), (80, 24)] {
            let rows = render(&app, width, height);
            let output = screen(&rows);
            assert_source_under_prompt(&rows, &label, "[ESC]中断", "連続正解");
            // The sentence itself is still on screen (as text in the
            // surface row, or in the full-sentence rows of the large style).
            let head = prompt.text.chars().take(4).collect::<String>();
            assert!(row_of(&rows, &head).is_some(), "{style} {width}x{height}: prompt text missing:\n{output}");
            let source = row_of(&rows, &label).unwrap();
            assert!(
                row_of(&rows, &head).unwrap() < source,
                "{style} {width}x{height}: source under the prompt:\n{output}"
            );
        }
    }
}

#[test]
fn japanese_sentence_source_follows_the_display_language() {
    let app = start(ModeKind::Sentence, Lang::En, "ja", |config| {
        config.display.prompt_style = "normal".to_owned()
    });
    let Some(PromptSource::Aozora { author, title, .. }) = app.game.as_ref().unwrap().current_prompt.source.clone() else {
        panic!("bundled Japanese sentence without a source");
    };
    let label = source_label(&app, Lang::En).unwrap();
    assert_eq!(label, format!("{author}, \"{title}\" (Aozora Bunko)"));
    let rows = render(&app, 80, 24);
    assert_source_under_prompt(&rows, &label, "[ESC]Abort", "Combo");
}

#[test]
fn english_sentence_shows_its_tatoeba_number() {
    for style in ["large", "normal"] {
        for kind in [ModeKind::Sentence, ModeKind::Endurance] {
            let app = start(kind, Lang::En, "en", |config| config.display.prompt_style = style.to_owned());
            let Some(PromptSource::Tatoeba { sentence_id }) = app.game.as_ref().unwrap().current_prompt.source.clone() else {
                panic!("{style} {kind:?}: bundled English sentence without a source");
            };
            assert!(sentence_id.chars().all(|character| character.is_ascii_digit()), "{sentence_id}");
            let label = source_label(&app, Lang::En).unwrap();
            assert_eq!(label, format!("Tatoeba #{sentence_id}"));
            for (width, height) in [(120, 40), (80, 24)] {
                let rows = render(&app, width, height);
                assert_source_under_prompt(&rows, &label, "[ESC]Abort", "Combo");
            }
        }
    }
}

#[test]
fn prompts_without_a_source_show_no_source_row() {
    let cases: [(ModeKind, Lang, &str); 3] = [
        (ModeKind::Phrase, Lang::Ja, "ja"),
        (ModeKind::Romaji, Lang::Ja, "ja"),
        (ModeKind::Words, Lang::En, "en"),
    ];
    for (kind, lang, data_lang) in cases {
        for style in ["large", "normal"] {
            let app = start(kind, lang, data_lang, |config| config.display.prompt_style = style.to_owned());
            assert_eq!(app.game.as_ref().unwrap().current_prompt.source, None, "{kind:?}");
            for (width, height) in [(120, 40), (80, 24)] {
                assert!(!resolve_playing_layout(&app, Rect::new(1, 1, width - 2, height - 2)).show_source);
                let output = screen(&render(&app, width, height));
                for credit in ["青空文庫", "AozoraBunko", "Tatoeba#"] {
                    assert!(!compact(&output).contains(credit), "{kind:?} {style} {width}x{height}:\n{output}");
                }
            }
        }
    }
}

#[test]
fn sentences_from_the_users_own_file_have_no_source() {
    let directory = TempDir::new().unwrap();
    let english = directory.path().join("sentences_en.txt");
    fs::write(&english, "The quick brown fox jumps over the lazy dog.\n").unwrap();
    let japanese = directory.path().join("sentences_ja.tsv");
    fs::write(&japanese, "吾輩は猫である。\tわがはいはねこである。\n").unwrap();
    let app = start(ModeKind::Sentence, Lang::En, "en", |config| {
        config.data.sentences_en = Some(english.clone())
    });
    assert_eq!(app.game.as_ref().unwrap().current_prompt.source, None);
    assert!(!compact(&screen(&render(&app, 80, 24))).contains("Tatoeba#"));
    let app = start(ModeKind::Sentence, Lang::Ja, "ja", |config| {
        config.data.sentences_ja = Some(japanese.clone())
    });
    assert_eq!(app.game.as_ref().unwrap().current_prompt.source, None);
    assert!(!compact(&screen(&render(&app, 80, 24))).contains("青空文庫"));
}

#[test]
fn source_row_never_costs_the_prompt_or_the_footer() {
    for (lang, data_lang, hint) in [(Lang::Ja, "ja", "[ESC]中断"), (Lang::En, "en", "[ESC]Abort")] {
        for style in ["large", "normal"] {
            let app = start(ModeKind::Sentence, lang, data_lang, |config| {
                config.display.prompt_style = style.to_owned()
            });
            let label = source_label(&app, lang).unwrap();
            let mut hidden = false;
            for height in (8..=24).rev() {
                let layout = resolve_playing_layout(&app, Rect::new(1, 1, 78, height - 2));
                let rows = render(&app, 80, height);
                let output = screen(&rows);
                assert_eq!(row_of(&rows, hint), Some(rows.len() - 2), "{style} 80x{height}: footer kept:\n{output}");
                if layout.show_source || layout.source_in_helper_row {
                    assert!(row_of(&rows, &label).is_some(), "{style} 80x{height}:\n{output}");
                } else {
                    hidden = true;
                    assert!(row_of(&rows, &label).is_none(), "{style} 80x{height}:\n{output}");
                    assert_eq!(layout.prompt_gap_rows, 0, "{style} 80x{height}: the gap goes before the source");
                }
                if height == 24 {
                    assert!(row_of(&rows, &label).is_some(), "{style} 80x24 shows the source:\n{output}");
                }
            }
            assert!(hidden, "{style}: the source row is left out on a very short screen");
        }
    }
}

#[test]
fn every_bundled_sentence_has_a_source() {
    let home = TempDir::new().unwrap();
    let catalog = DataCatalog::load(&DataConfig::default(), home.path()).unwrap();
    for item in catalog.fixed_items(DataKind::JapaneseSentence) {
        let source = catalog.sentence_source(DataKind::JapaneseSentence, &item.text, item.reading.as_deref());
        let Some(PromptSource::Aozora { work_id, author, title }) = source else {
            panic!("no Aozora work for {:?}", item.key);
        };
        assert_eq!(Some(work_id), item.source_id.as_ref());
        assert_eq!(Some(author), item.author.as_ref(), "{}", item.key);
        assert!(!title.is_empty());
    }
    for item in catalog.fixed_items(DataKind::EnglishSentence) {
        let source = catalog.sentence_source(DataKind::EnglishSentence, &item.text, None);
        assert!(matches!(source, Some(PromptSource::Tatoeba { .. })), "{}", item.key);
    }
    for kind in [
        DataKind::JapanesePhrase,
        DataKind::EnglishPhrase,
        DataKind::JapaneseWord,
        DataKind::EnglishWord,
    ] {
        let item = catalog.fixed_items(kind)[0];
        assert_eq!(catalog.sentence_source(kind, &item.text, item.reading.as_deref()), None);
    }
}

fn gunzip(bytes: &[u8]) -> String {
    let mut text = String::new();
    flate2::read::GzDecoder::new(bytes).read_to_string(&mut text).unwrap();
    text
}

#[test]
fn bundled_aozora_work_table_covers_every_cited_work() {
    let works = gunzip(AOZORA_WORKS_GZ);
    let mut ids = Vec::new();
    for line in works.lines() {
        let fields = line.split('\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 3, "{line}");
        assert!(fields[0].len() == 6 && fields[0].bytes().all(|byte| byte.is_ascii_digit()), "{line}");
        assert!(!fields[1].is_empty() && !fields[2].is_empty(), "{line}");
        ids.push(fields[0]);
    }
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(ids, sorted, "one row per work, in id order");
    let cited = gunzip(JA_SENTENCES_GZ)
        .lines()
        .map(|line| line.split('\t').nth(2).unwrap().to_owned())
        .collect::<BTreeSet<_>>();
    let listed = ids.iter().map(|id| (*id).to_owned()).collect::<BTreeSet<_>>();
    assert_eq!(listed, cited, "the table lists exactly the works the sentences cite");
}

#[test]
fn manifest_counts_and_hashes_match_the_bundled_files() {
    let manifest = MANIFEST.parse::<toml::Value>().unwrap();
    let outputs = manifest["outputs"].as_array().unwrap();
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
    let mut seen = Vec::new();
    for output in outputs {
        let path = output["path"].as_str().unwrap();
        let bytes = fs::read(data.join(path)).unwrap_or_else(|error| panic!("{path}: {error}"));
        let digest = rs_typing_trainer::bench::sha256_digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(output["sha256"].as_str(), Some(digest.as_str()), "{path}");
        if path.ends_with(".tsv.gz") {
            let rows = gunzip(&bytes).lines().filter(|line| !line.is_empty()).count();
            assert_eq!(output["count"].as_integer(), Some(rows as i64), "{path}");
        }
        seen.push(path);
    }
    assert!(seen.contains(&"aozora_works.tsv.gz"), "{seen:?}");
}
