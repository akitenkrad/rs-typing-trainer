//! MYTASK-3638: individual training as a table of skills (rows) × prompt
//! units (columns), with one practice language for every cell.

use std::collections::HashSet;
use std::fs;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::app::{App, Screen};
use rs_typing_trainer::config::{Config, DrillCell, DrillSkill, ErrorMode, ModeKind, PromptUnit};
use rs_typing_trainer::data::{DataCatalog, PromptSource, TrainingDataSource};
use rs_typing_trainer::engine::modes::ModeGenerator;
use rs_typing_trainer::engine::session::ItemKind;
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::practice::drill::{self, Availability};
use rs_typing_trainer::record::db;
use rs_typing_trainer::ui;
use tempfile::TempDir;

use crate::common::{TestApp, test_app};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn render(app: &App, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect()
}

/// Wide characters are followed by a blank cell in the test buffer.
fn compact(rows: &[String]) -> String {
    rows.join("\n").chars().filter(|character| *character != ' ').collect()
}

fn catalog() -> DataCatalog {
    DataCatalog::load(&Default::default(), std::path::Path::new("")).unwrap()
}

/// The expected availability (○ existing / △ extended / — none), as agreed.
fn expected_availability(skill: DrillSkill, unit: PromptUnit) -> Availability {
    use Availability::{Existing as O, Extended as T, Unavailable as N};
    let row = match skill {
        DrillSkill::Normal | DrillSkill::Accuracy | DrillSkill::Speed | DrillSkill::Recovery | DrillSkill::Blind => [O, O, O, O, O, O],
        // MYTASK-3640: text that reads on (spelling: Japanese only).
        DrillSkill::Anticipation | DrillSkill::Preview | DrillSkill::Overlap | DrillSkill::Spelling => [N, N, O, O, O, O],
        DrillSkill::Weak => [N, O, O, T, T, N],
        DrillSkill::Rhythm => [N, T, O, T, T, N],
        DrillSkill::Burst => [N, N, O, T, T, N],
        DrillSkill::Endurance => [N, N, T, T, O, T],
        DrillSkill::Unlock => [O, N, O, N, N, N],
    };
    row[PromptUnit::ALL.iter().position(|candidate| *candidate == unit).unwrap()]
}

/// The mode each cell records (`sessions.mode`).
fn expected_mode(cell: DrillCell, lang: Lang) -> ModeKind {
    match cell.skill {
        DrillSkill::Weak => ModeKind::Adaptive,
        DrillSkill::Rhythm => ModeKind::Rhythm,
        DrillSkill::Burst => ModeKind::Burst,
        DrillSkill::Endurance => ModeKind::Endurance,
        DrillSkill::Unlock => ModeKind::Unlock,
        _ => match cell.unit {
            PromptUnit::Char => ModeKind::Random,
            PromptUnit::Ngram => ModeKind::Ngram,
            PromptUnit::Word if lang == Lang::Ja => ModeKind::Romaji,
            PromptUnit::Word => ModeKind::Words,
            PromptUnit::Phrase => ModeKind::Phrase,
            PromptUnit::Sentence | PromptUnit::Passage => ModeKind::Sentence,
        },
    }
}

fn all_cells() -> Vec<DrillCell> {
    DrillSkill::ALL
        .iter()
        .flat_map(|skill| PromptUnit::ALL.iter().map(move |unit| DrillCell::new(*skill, *unit)))
        .collect()
}

fn config(lang: Lang) -> Config {
    let mut config = Config::default();
    config.general.lang = lang;
    config.ui.animation_enabled = false;
    config.session.result_input_delay_ms = 0;
    config
}

/// Title → table, then Enter on `cell` in `practice` (through the key
/// picker when the cell uses it).
fn choose(app: &mut App, cell: DrillCell, practice: Lang, now: Instant) {
    app.handle_key_at(key(KeyCode::Enter), now);
    assert_eq!(app.screen, Screen::ModeSelect);
    app.drill_cell = cell;
    if app.practice_lang != practice {
        app.handle_key_at(key(KeyCode::Char('L')), now);
    }
    assert_eq!(app.practice_lang, practice);
    app.handle_key_at(key(KeyCode::Enter), now);
    if app.screen == Screen::KeyPicker {
        app.handle_key_at(key(KeyCode::Enter), now);
    }
}

fn start(cell: DrillCell, practice: Lang) -> (TestApp, Instant) {
    let mut app = test_app(config(Lang::En));
    let now = Instant::now();
    choose(&mut app, cell, practice, now);
    assert!(matches!(app.screen, Screen::Countdown { .. }), "{cell:?} {practice:?}: {:?}", app.screen);
    app.on_tick_at(Duration::from_secs(3), now).unwrap();
    app.on_tick_at(Duration::from_millis(1), now).unwrap();
    assert_eq!(app.screen, Screen::Playing, "{cell:?}");
    (app, now)
}

#[test]
fn the_table_has_the_agreed_availability_and_modes() {
    assert_eq!(DrillSkill::ALL.len(), 14);
    for cell in all_cells() {
        assert_eq!(drill::availability(cell), expected_availability(cell.skill, cell.unit), "{cell:?}");
        for lang in [Lang::En, Lang::Ja] {
            let mode = drill::mode_for(cell, lang);
            let english_spelling = cell.skill == DrillSkill::Spelling && lang == Lang::En;
            assert_eq!(
                drill::is_available_in(cell, lang),
                expected_availability(cell.skill, cell.unit) != Availability::Unavailable && !english_spelling,
                "{cell:?} {lang:?}"
            );
            if expected_availability(cell.skill, cell.unit) == Availability::Unavailable || english_spelling {
                assert_eq!(mode, None, "{cell:?}");
            } else {
                assert_eq!(mode, Some(expected_mode(cell, lang)), "{cell:?} {lang:?}");
            }
        }
    }
    // Letters, combinations and unlocking use English letters in any language.
    assert!(!drill::uses_language(DrillCell::new(DrillSkill::Normal, PromptUnit::Char)));
    assert!(!drill::uses_language(DrillCell::new(DrillSkill::Rhythm, PromptUnit::Ngram)));
    assert!(!drill::uses_language(DrillCell::new(DrillSkill::Unlock, PromptUnit::Word)));
    assert!(drill::uses_language(DrillCell::new(DrillSkill::Weak, PromptUnit::Ngram)));
}

/// Every ○ and △ cell starts with its mode, its settings and prompts of its
/// unit, in both practice languages.
#[test]
fn every_available_cell_starts_with_its_mode_settings_and_prompts() {
    let data = catalog();
    let phrases_en = data.phrases_en().iter().cloned().collect::<HashSet<_>>();
    let phrases_ja = data.phrases_ja().iter().map(|row| row.text.clone()).collect::<HashSet<_>>();
    let sentences_en = data.sentences_en().iter().cloned().collect::<HashSet<_>>();
    let sentences_ja = data.sentences_ja().iter().map(|row| row.text.clone()).collect::<HashSet<_>>();
    for lang in [Lang::En, Lang::Ja] {
        for cell in all_cells().into_iter().filter(|cell| drill::is_available_in(*cell, lang)) {
            if cell.unit == PromptUnit::Passage && lang == Lang::Ja && !rs_typing_trainer::data::has_bundled_japanese_passages() {
                continue;
            }
            let (app, now) = start(cell, lang);
            let label = format!("{cell:?} {lang:?}");
            let config = &app.config;
            assert_eq!(config.mode.kind, expected_mode(cell, lang), "{label}");
            assert_eq!(config.mode.drill, Some(cell), "{label}");
            let language = if drill::uses_language(cell) { lang } else { Lang::En };
            let code = if language == Lang::Ja { "ja" } else { "en" };
            assert_eq!(config.mode.sentence.lang, code, "{label}");
            assert_eq!(config.mode.phrase.lang, code, "{label}");
            assert_eq!(config.adaptive.lang, code, "{label}");
            match cell.skill {
                DrillSkill::Accuracy => {
                    assert_eq!(config.session.error_mode, ErrorMode::Stop, "{label}");
                    assert_eq!(config.gate.min_accuracy, 0.97, "{label}");
                    assert!(!config.pacer.enabled, "{label}");
                }
                DrillSkill::Speed => {
                    assert!(config.pacer.enabled, "{label}");
                    assert_eq!(config.pacer.ratio, 1.05, "{label}");
                    assert!(config.pacer.target_wpm.is_some(), "{label}: the pacer runs on every unit");
                }
                DrillSkill::Recovery => {
                    assert_eq!(config.session.error_mode, ErrorMode::Advance, "{label}");
                    assert!(config.session.retry_on_error, "{label}");
                }
                DrillSkill::Blind => assert_eq!(config.keyboard.show, "auto", "{label}"),
                // MYTASK-3640: the first level of each row's prescription.
                DrillSkill::Anticipation => {
                    assert!(config.pacer.enabled, "{label}");
                    assert_eq!(config.pacer.ratio, 0.90, "{label}");
                    assert!(config.pacer.target_wpm.is_some(), "{label}: the pacer runs on every unit");
                }
                DrillSkill::Preview => {
                    assert_eq!(config.session.preview_items, 3, "{label}");
                    assert!(config.mode.sentence.hide_typed, "{label}");
                    assert!(!config.pacer.enabled, "{label}");
                }
                DrillSkill::Spelling => {
                    assert_eq!(config.mode.key_filter, rs_typing_trainer::config::KeyFilter::Include, "{label}");
                    assert!(!config.pacer.enabled, "{label}");
                }
                _ => {
                    assert!(!config.pacer.enabled, "{label}");
                    assert_eq!(config.gate.min_accuracy, 0.0, "{label}");
                }
            }
            let prompt = &app.game.as_ref().unwrap().current_prompt;
            let japanese = language == Lang::Ja;
            // Weak transitions without records fall back to plain words.
            let fallback = cell.skill == DrillSkill::Weak && matches!(cell.unit, PromptUnit::Ngram | PromptUnit::Word);
            match cell.unit {
                _ if fallback => assert_eq!(prompt.reading.is_some(), japanese, "{label}"),
                PromptUnit::Char => {
                    let keys = match cell.skill {
                        DrillSkill::Unlock => app.unlock_state.as_ref().unwrap().unlocked.clone(),
                        _ => config.mode.keys.clone(),
                    };
                    assert!(prompt.reading.is_none(), "{label}");
                    assert!(prompt.text.chars().all(|character| keys.contains(character)), "{label}: {}", prompt.text);
                }
                PromptUnit::Ngram => {
                    assert_eq!(prompt.kind, ItemKind::Drill, "{label}");
                    assert!((2..=3).contains(&prompt.text.chars().count()), "{label}: {}", prompt.text);
                }
                PromptUnit::Word => {
                    assert_eq!(prompt.reading.is_some(), japanese, "{label}");
                    assert!(!prompt.text.contains(' '), "{label}: {}", prompt.text);
                }
                PromptUnit::Phrase => {
                    let pool = if japanese { &phrases_ja } else { &phrases_en };
                    assert!(pool.contains(&prompt.text), "{label}: {}", prompt.text);
                }
                PromptUnit::Sentence => {
                    let pool = if japanese { &sentences_ja } else { &sentences_en };
                    assert!(pool.contains(&prompt.text), "{label}: {}", prompt.text);
                    assert!(prompt.source.is_some(), "{label}: bundled sentences show their source");
                }
                PromptUnit::Passage if japanese => {
                    assert!(prompt.reading.is_some(), "{label}");
                    assert!(data.passages_ja().iter().any(|passage| passage.text == prompt.text), "{label}");
                    let Some(PromptSource::Aozora { work_id, author, title }) = &prompt.source else {
                        panic!("{label}: {:?}", prompt.source);
                    };
                    assert!(!work_id.is_empty() && !author.is_empty() && !title.is_empty(), "{label}");
                }
                PromptUnit::Passage => {
                    assert!(!sentences_en.contains(&prompt.text), "{label}: a passage is not one sentence");
                    let Some(PromptSource::TatoebaSet { sentence_ids }) = &prompt.source else {
                        panic!("{label}: {:?}", prompt.source);
                    };
                    assert!((3..=5).contains(&sentence_ids.len()), "{label}: {sentence_ids:?}");
                }
            }
            // Running text has no per-item timer: six idle seconds keep the
            // first prompt.
            if cell.unit.is_running_text() {
                let mut app = app;
                let text = app.game.as_ref().unwrap().current_prompt.text.clone();
                app.on_tick_at(Duration::from_secs(6), now + Duration::from_secs(6)).unwrap();
                assert_eq!(app.game.as_ref().unwrap().current_prompt.text, text, "{label}");
            }
        }
    }
}

#[test]
fn unavailable_cells_do_not_start_and_say_why() {
    for lang in [Lang::En, Lang::Ja] {
        // The table opens in English (the configured mode is `words`).
        for cell in all_cells().into_iter().filter(|cell| !drill::is_available_in(*cell, Lang::En)) {
            let mut app = test_app(config(lang));
            let now = Instant::now();
            app.handle_key_at(key(KeyCode::Enter), now);
            assert_eq!(app.practice_lang, Lang::En);
            app.drill_cell = cell;
            let screen = compact(&render(&app, 80, 24));
            let reason = rs_typing_trainer::ui::mode_select::unavailable_reason(cell, Lang::En, lang).replace(' ', "");
            assert!(screen.contains(&reason), "{cell:?}:\n{screen}");
            app.handle_key_at(key(KeyCode::Enter), now);
            assert_eq!(app.screen, Screen::ModeSelect, "{cell:?}");
            assert!(app.mode_warning.is_some(), "{cell:?}");
            assert!(app.game.is_none());
        }
    }
}

#[test]
fn the_cursor_moves_with_arrows_and_hjkl_and_stops_at_the_edges() {
    let mut app = test_app(config(Lang::En));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.drill_cell, DrillCell::new(DrillSkill::Normal, PromptUnit::Char));
    app.handle_key(key(KeyCode::Up));
    app.handle_key(key(KeyCode::Char('h')));
    assert_eq!(app.drill_cell, DrillCell::new(DrillSkill::Normal, PromptUnit::Char));
    for _ in 0..3 {
        app.handle_key(key(KeyCode::Char('j')));
    }
    app.handle_key(key(KeyCode::Right));
    app.handle_key(key(KeyCode::Char('l')));
    assert_eq!(app.drill_cell, DrillCell::new(DrillSkill::Recovery, PromptUnit::Word));
    app.handle_key(key(KeyCode::Char('k')));
    app.handle_key(key(KeyCode::Left));
    assert_eq!(app.drill_cell, DrillCell::new(DrillSkill::Speed, PromptUnit::Ngram));
    for _ in 0..20 {
        app.handle_key(key(KeyCode::Down));
        app.handle_key(key(KeyCode::Right));
    }
    assert_eq!(app.drill_cell, DrillCell::new(DrillSkill::Unlock, PromptUnit::Passage));
    // The cursor stops on a `—` cell, which cannot start.
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.screen, Screen::ModeSelect);
}

#[test]
fn the_cell_opens_on_the_configured_mode_when_nothing_is_saved() {
    for (kind, cell, lang) in [
        (ModeKind::Random, DrillCell::new(DrillSkill::Normal, PromptUnit::Char), Lang::En),
        (ModeKind::Romaji, DrillCell::new(DrillSkill::Normal, PromptUnit::Word), Lang::Ja),
        (ModeKind::Adaptive, DrillCell::new(DrillSkill::Weak, PromptUnit::Word), Lang::En),
        (ModeKind::Rhythm, DrillCell::new(DrillSkill::Rhythm, PromptUnit::Word), Lang::En),
        (ModeKind::Endurance, DrillCell::new(DrillSkill::Endurance, PromptUnit::Sentence), Lang::En),
        (ModeKind::Unlock, DrillCell::new(DrillSkill::Unlock, PromptUnit::Word), Lang::En),
    ] {
        let mut config = config(Lang::Ja);
        config.mode.kind = kind;
        let mut app = test_app(config);
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.drill_cell, cell, "{kind:?}");
        assert_eq!(app.practice_lang, lang, "{kind:?}");
    }
}

/// `L` switches the language of every cell; the language and the cell are
/// saved and come back on the next start.
#[test]
fn l_switches_the_practice_language_and_the_choice_is_saved() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("config.toml");
    fs::write(&path, "# kept\n[keyboard]\nlayout = \"qwerty\"\n").unwrap();
    let mut first = test_app(Config::from_path(&path).unwrap());
    first.config_path = Some(path.clone());
    first.config.ui.animation_enabled = false;
    let now = Instant::now();
    first.handle_key_at(key(KeyCode::Enter), now);
    assert_eq!(first.practice_lang, Lang::En);
    first.handle_key_at(key(KeyCode::Char('L')), now);
    assert_eq!(first.practice_lang, Lang::Ja);
    for code in [
        &first.config.mode.sentence.lang,
        &first.config.mode.phrase.lang,
        &first.config.adaptive.lang,
    ] {
        assert_eq!(code, "ja");
    }
    let screen = compact(&render(&first, 80, 24));
    assert!(screen.contains("Practicelanguage:Japanese"), "{screen}");
    let saved = fs::read_to_string(&path).unwrap();
    assert!(saved.contains("# kept"), "{saved}");
    assert!(saved.contains("[drill]") && saved.contains("lang = \"ja\""), "{saved}");

    // Rhythm × sentences, then the next start opens there in Japanese.
    first.drill_cell = DrillCell::new(DrillSkill::Rhythm, PromptUnit::Sentence);
    first.handle_key_at(key(KeyCode::Enter), now);
    assert!(matches!(first.screen, Screen::Countdown { .. }));
    let saved = fs::read_to_string(&path).unwrap();
    assert!(saved.contains("skill = \"rhythm\""), "{saved}");
    assert!(saved.contains("unit = \"sentence\""), "{saved}");
    assert!(saved.contains("kind = \"rhythm\""), "{saved}");
    // The cell's own settings (pacer off etc.) are never written.
    assert!(!saved.contains("retry_on_error"), "{saved}");

    let mut second = test_app(Config::from_path(&path).unwrap());
    second.handle_key(key(KeyCode::Enter));
    assert_eq!(second.drill_cell, DrillCell::new(DrillSkill::Rhythm, PromptUnit::Sentence));
    assert_eq!(second.practice_lang, Lang::Ja);
    // Back to English.
    second.config_path = Some(path.clone());
    second.handle_key(key(KeyCode::Char('L')));
    assert_eq!(second.practice_lang, Lang::En);
    let third = test_app(Config::from_path(&path).unwrap());
    assert_eq!(third.practice_lang, Lang::En);
}

/// Type the current prompt's target correctly, `step` apart; returns the time
/// after the last key.
fn type_prompt(app: &mut App, mut now: Instant, step: Duration) -> Instant {
    let target = app.game.as_ref().unwrap().current_prompt.target.clone();
    for character in target.chars() {
        now += step;
        app.handle_key_at(key(KeyCode::Char(character)), now);
        if app.screen != Screen::Playing {
            return now;
        }
    }
    // The next prompt comes in on the following tick.
    now += Duration::from_millis(300);
    app.on_tick_at(Duration::from_millis(300), now).unwrap();
    now
}

/// △ rhythm cells: the metronome measures the drift on combos, phrases and
/// sentences as on words.
#[test]
fn rhythm_measures_the_beat_on_the_new_units() {
    for (unit, lang) in [
        (PromptUnit::Ngram, Lang::En),
        (PromptUnit::Phrase, Lang::En),
        (PromptUnit::Phrase, Lang::Ja),
        (PromptUnit::Sentence, Lang::En),
        (PromptUnit::Sentence, Lang::Ja),
    ] {
        let cell = DrillCell::new(DrillSkill::Rhythm, unit);
        let (mut app, start_at) = start(cell, lang);
        assert!(app.rhythm_bpm.is_some(), "{unit:?}");
        let mut now = start_at;
        for _ in 0..2 {
            now = type_prompt(&mut app, now, Duration::from_millis(150));
        }
        let end = start_at + Duration::from_secs(app.config.session.duration_seconds + 1);
        app.on_tick_at(Duration::from_secs(1), end).unwrap();
        assert_eq!(app.screen, Screen::Result, "{unit:?}");
        let deviation = app.rhythm_deviation_ms.expect("rhythm deviation");
        assert!(deviation.is_finite(), "{unit:?}");
        let record = app.result_record.as_ref().unwrap();
        assert_eq!(record.header.mode, "rhythm");
        let params: serde_json::Value = serde_json::from_str(&record.header.mode_params).unwrap();
        assert_eq!(params["unit"], unit.as_str(), "{unit:?}");
        assert_eq!(params["drill"]["skill"], "rhythm");
    }
}

/// △ burst cells: a sprint is a few phrases or one sentence, and the series
/// counts the sprints as before.
#[test]
fn burst_counts_sprints_of_phrases_and_sentences() {
    for (unit, items) in [(PromptUnit::Phrase, 4), (PromptUnit::Sentence, 1)] {
        let cell = DrillCell::new(DrillSkill::Burst, unit);
        let (mut app, start_at) = start(cell, Lang::En);
        assert_eq!(drill::burst_items(&app.config), items, "{unit:?}");
        let mut now = start_at;
        for _ in 0..items {
            assert_eq!(app.screen, Screen::Playing, "{unit:?}");
            now = type_prompt(&mut app, now, Duration::from_millis(80));
        }
        assert_eq!(app.screen, Screen::Result, "{unit:?}: the sprint ends after {items} items");
        assert_eq!(app.burst_series.run_count(), 1, "{unit:?}");
        let record = app.result_record.as_ref().unwrap();
        assert_eq!(record.header.mode, "burst");
        assert_eq!(record.header.completed_words as usize, items);
        // The next sprint plays the same cell.
        app.handle_key_at(key(KeyCode::Enter), now + Duration::from_secs(1));
        assert!(matches!(app.screen, Screen::Countdown { .. }));
        assert_eq!(app.config.mode.drill, Some(cell));
    }
}

/// △ endurance cells: words, phrases and passages are compared by halves.
#[test]
fn endurance_compares_the_halves_on_the_new_units() {
    for (unit, lang) in [
        (PromptUnit::Word, Lang::En),
        (PromptUnit::Word, Lang::Ja),
        (PromptUnit::Phrase, Lang::Ja),
        (PromptUnit::Passage, Lang::En),
    ] {
        let cell = DrillCell::new(DrillSkill::Endurance, unit);
        let (mut app, start_at) = start(cell, lang);
        let minutes = app.config.endurance.minutes;
        let mut now = start_at;
        // Type through both halves of the session.
        while now < start_at + Duration::from_secs(minutes * 60 - 5) && app.screen == Screen::Playing {
            now = type_prompt(&mut app, now, Duration::from_millis(400));
        }
        app.on_tick_at(Duration::from_secs(1), start_at + Duration::from_secs(minutes * 60 + 1))
            .unwrap();
        assert_eq!(app.screen, Screen::Result, "{unit:?}");
        let comparison = app.endurance_comparison.expect("endurance comparison");
        assert!(
            comparison.first_iki_ms > 0.0 && comparison.second_iki_ms > 0.0,
            "{unit:?}: {comparison:?}"
        );
        let record = app.result_record.as_ref().unwrap();
        assert_eq!(record.header.mode, "endurance");
        assert_eq!(record.header.duration_ms / 1000, (minutes * 60) as i64, "{unit:?}");
    }
}

/// △ weak-transition cells: phrases and sentences rich in the weak
/// transitions are preferred.
#[test]
fn weak_transitions_prefer_phrases_and_sentences_that_contain_them() {
    for unit in [PromptUnit::Phrase, PromptUnit::Sentence] {
        let mut config = Config::default();
        config.mode.kind = ModeKind::Adaptive;
        config.mode.drill = Some(DrillCell::new(DrillSkill::Weak, unit));
        config.mode.phrase.course = "long".to_owned();
        let layout = rs_typing_trainer::layout::builtin_layout("qwerty").unwrap();
        let mut generator = ModeGenerator::new(config.mode.clone(), catalog(), layout, 7);
        let transitions = vec![("t".to_owned(), "h".to_owned())];
        generator.prefer_transitions(&transitions);
        let candidates = generator.weighted_candidates();
        assert!(candidates.len() >= 20, "{unit:?}");
        for (prompt, weight) in &candidates {
            let count = prompt.target.matches("th").count();
            assert!(count > 0, "{unit:?}: {}", prompt.target);
            assert_eq!(*weight, 1 + 4 * count, "{unit:?}: {}", prompt.target);
        }
        let prompt = generator.next_prompt();
        assert!(prompt.target.contains("th"), "{unit:?}");
    }
}

#[test]
fn records_keep_their_mode_and_add_the_unit() {
    // Existing cells record the same mode as before.
    for (cell, lang, mode) in [
        (DrillCell::new(DrillSkill::Normal, PromptUnit::Word), Lang::En, "words"),
        (DrillCell::new(DrillSkill::Normal, PromptUnit::Word), Lang::Ja, "romaji"),
        (DrillCell::new(DrillSkill::Normal, PromptUnit::Sentence), Lang::Ja, "sentence"),
        (DrillCell::new(DrillSkill::Accuracy, PromptUnit::Word), Lang::En, "words"),
        (DrillCell::new(DrillSkill::Rhythm, PromptUnit::Word), Lang::En, "rhythm"),
        (DrillCell::new(DrillSkill::Endurance, PromptUnit::Sentence), Lang::En, "endurance"),
        (DrillCell::new(DrillSkill::Normal, PromptUnit::Passage), Lang::En, "sentence"),
        (DrillCell::new(DrillSkill::Rhythm, PromptUnit::Sentence), Lang::Ja, "rhythm"),
        (DrillCell::new(DrillSkill::Normal, PromptUnit::Char), Lang::Ja, "random"),
    ] {
        let (mut app, start_at) = start(cell, lang);
        type_prompt(&mut app, start_at, Duration::from_millis(120));
        let end = start_at + Duration::from_secs(app.config.session.duration_seconds.max(app.config.endurance.minutes * 60) + 1);
        app.on_tick_at(Duration::from_secs(1), end).unwrap();
        assert_eq!(app.screen, Screen::Result, "{cell:?}");
        let db_path = app.config.storage.db_path.clone().unwrap();
        let records = db::load_records_filtered(&db_path, None, Some(mode)).unwrap();
        assert_eq!(records.len(), 1, "{cell:?}: recorded as {mode}");
        let record = &records[0];
        let params: serde_json::Value = serde_json::from_str(&record.header.mode_params).unwrap();
        assert_eq!(params["unit"], cell.unit.as_str(), "{cell:?}");
        // The language of the series: the practice language, except for the
        // language-independent cells.
        let language = if drill::uses_language(cell) && lang == Lang::Ja { "ja" } else { "en" };
        assert_eq!(rs_typing_trainer::analysis::model::session_language(record), language, "{cell:?}");
        // The configuration is the user's own again.
        assert_eq!(app.config.mode.drill, None);
        assert!(!app.config.session.retry_on_error);
    }
}

#[test]
fn records_from_before_the_table_still_read_the_same() {
    // A record written before MYTASK-3638 has no `drill`/`unit` keys.
    let mut app = test_app(config(Lang::En));
    let mut legacy = Config::default();
    legacy.mode.kind = ModeKind::Rhythm;
    let log = rs_typing_trainer::engine::session::SessionLog {
        duration_us: 60_000_000,
        error_mode: ErrorMode::Stop,
        items: vec![],
        keystrokes: vec![],
    };
    let record = rs_typing_trainer::record::SessionRecord::from_log(&legacy, log, chrono::Utc::now());
    let params: serde_json::Value = serde_json::from_str(&record.header.mode_params).unwrap();
    assert!(params.get("unit").is_none() && params.get("drill").is_none());
    let db_path = app.config.storage.db_path.clone().unwrap();
    db::save(&db_path, &record).unwrap();
    let loaded = db::load_records_filtered(&db_path, None, Some("rhythm")).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(rs_typing_trainer::analysis::model::session_language(&loaded[0]), "en");
    // The history and analysis screens open on it as before.
    app.handle_key(key(KeyCode::Char('h')));
    assert_eq!(app.screen, Screen::History);
    let history = compact(&render(&app, 100, 24));
    assert!(history.contains("rhythm") || history.contains("Rhythm"), "{history}");
}

#[test]
fn the_table_fits_80_by_24_in_both_languages() {
    for lang in [Lang::En, Lang::Ja] {
        let mut app = test_app(config(lang));
        app.handle_key(key(KeyCode::Enter));
        app.mode_grade_line = Some(match lang {
            Lang::En => "EN B · JA E (provisional)".to_owned(),
            Lang::Ja => "英 B・日 E（仮）".to_owned(),
        });
        app.drill_cell = DrillCell::new(DrillSkill::Burst, PromptUnit::Phrase);
        let rows = render(&app, 80, 24);
        if std::env::var_os("RS_TYPING_TRAINER_DUMP_UI").is_some() {
            println!("\n--- {lang:?} 80x24 ---\n{}", rows.join("\n"));
        }
        let screen = compact(&rows);
        for skill in DrillSkill::ALL {
            let label = rs_typing_trainer::ui::mode_select::skill_label(skill, lang).replace(' ', "");
            assert!(screen.contains(&label), "{lang:?} {skill:?}:\n{}", rows.join("\n"));
        }
        for unit in PromptUnit::ALL {
            let label = rs_typing_trainer::ui::mode_select::unit_label(unit, lang);
            assert!(screen.contains(label), "{lang:?} {unit:?}");
        }
        let (description, hint, warmup) = match lang {
            Lang::En => ("Burst:threeall-outsprintsof4phrases", "[ESC]Back", "[W]1-minutewarm-up"),
            Lang::Ja => ("バースト:句4個を全力で3本", "[ESC]戻る", "[W]1分のウォームアップ"),
        };
        for text in [description, hint, warmup] {
            assert!(screen.contains(text), "{lang:?} {text}:\n{}", rows.join("\n"));
        }
        // The frame is intact: corners and both side borders on every row.
        assert!(rows[0].starts_with('┌') && rows[0].ends_with('┐'), "{}", rows[0]);
        assert!(rows[23].starts_with('└') && rows[23].ends_with('┘'), "{}", rows[23]);
        for row in &rows[1..23] {
            assert!(row.starts_with('│') && row.ends_with('│'), "{lang:?}: {row}");
        }
        // Every table row has its six cells at the same columns.
        let header = rows
            .iter()
            .position(|row| {
                row.replace(' ', "")
                    .contains(rs_typing_trainer::ui::mode_select::unit_label(PromptUnit::Char, lang))
            })
            .unwrap();
        let marks = |row: &String| {
            row.chars()
                .enumerate()
                .filter(|(_, character)| matches!(character, '○' | '—'))
                .map(|(index, _)| index)
                .collect::<Vec<_>>()
        };
        let reference = marks(&rows[header + 1]);
        assert_eq!(reference.len(), 6, "{}", rows[header + 1]);
        for row in &rows[header + 1..header + 15] {
            assert_eq!(marks(row), reference, "{lang:?}: {row}");
        }
    }
}

#[test]
fn every_bundled_japanese_passage_cites_its_work() {
    let data = catalog();
    assert!(rs_typing_trainer::data::has_bundled_japanese_passages());
    assert!(data.passages_ja().len() > 1_000);
    for passage in data.passages_ja() {
        let Some(PromptSource::Aozora { title, .. }) = &passage.source else {
            panic!("{passage:?}");
        };
        assert!(!title.is_empty(), "{passage:?}");
    }
}

/// The longest Japanese passage (its romaji runs past 200 letters) wraps
/// inside 80×24 in both prompt styles, and the window follows the typing to
/// its end.
#[test]
fn the_longest_japanese_passage_fits_80_by_24_in_both_prompt_styles() {
    let data = catalog();
    let longest = data
        .passages_ja()
        .iter()
        .max_by_key(|passage| {
            rs_typing_trainer::engine::romaji::RomajiJudge::new(&passage.reading)
                .guide()
                .chars()
                .count()
        })
        .unwrap()
        .clone();
    let guide = rs_typing_trainer::engine::romaji::RomajiJudge::new(&longest.reading).guide().to_owned();
    assert!(guide.chars().count() > 200, "{}", guide.chars().count());
    // Played as the only sentence, through the same Japanese running-text
    // layout as a passage.
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sentences_ja.tsv");
    fs::write(&path, format!("{}\t{}\n", longest.text, longest.reading)).unwrap();
    for style in ["large", "normal"] {
        let mut config = config(Lang::Ja);
        config.data.sentences_ja = Some(path.clone());
        config.display.prompt_style = style.to_owned();
        let mut app = test_app(config);
        let now = Instant::now();
        choose(&mut app, DrillCell::new(DrillSkill::Endurance, PromptUnit::Sentence), Lang::Ja, now);
        app.on_tick_at(Duration::from_secs(3), now).unwrap();
        app.on_tick_at(Duration::from_millis(1), now).unwrap();
        assert_eq!(app.screen, Screen::Playing);
        assert_eq!(app.game.as_ref().unwrap().current_prompt.text, longest.text);
        let total = guide.chars().count();
        let mut typed = 0;
        for stop in [0, total / 2, total - 3] {
            for character in guide.chars().skip(typed).take(stop - typed) {
                typed += 1;
                app.handle_key_at(key(KeyCode::Char(character)), now + Duration::from_millis(typed as u64 * 50 + 50));
            }
            let rows = render(&app, 80, 24);
            let text = rows.join("\n");
            assert!(rows[0].starts_with('┌') && rows[23].starts_with('└'), "{style} {stop}:\n{text}");
            let lines = rows
                .iter()
                .map(|row| row.chars().filter(|character| !character.is_whitespace()).collect::<String>())
                .collect::<Vec<_>>();
            let upcoming = guide.chars().skip(typed).take(3).collect::<String>();
            assert!(
                lines.iter().any(|row| row.contains(&upcoming)),
                "{style} {stop}: the romaji row must show {upcoming:?}:\n{text}"
            );
            assert!(compact(&rows).contains("[ESC]中断"), "{style} {stop}:\n{text}");
        }
    }
}
