//! MYTASK-3640: every step of today's menu is a cell of the
//! individual-training table (skill × prompt unit) plus the settings of its
//! level. A step that runs its cell's mode starts the way the cell does; a
//! level whose mode differs overrides the cell's mode and keeps its own
//! prompts. Records, judgments and progress stay as before.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::app::{App, Screen};
use rs_typing_trainer::config::{Config, DrillCell, DrillSkill, ModeKind, PromptUnit};
use rs_typing_trainer::data::{DataCatalog, TrainingDataSource};
use rs_typing_trainer::engine::modes::ModeGenerator;
use rs_typing_trainer::engine::session::ItemKind;
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::practice::drill;
use rs_typing_trainer::practice::menu::{
    DailyMenu, MenuRole, MenuStep, bundled_catalog, catalog_cell, config_for_menu_step, config_for_step, generator_kind_for_step, step_cell,
};
use rs_typing_trainer::record::db;
use rs_typing_trainer::ui;

use crate::common::{TestApp, test_app_at};

fn cell(skill: DrillSkill, unit: PromptUnit) -> DrillCell {
    DrillCell::new(skill, unit)
}

/// The agreed cells of every fixed step and every level, in catalog order.
fn expected_cells() -> BTreeMap<(&'static str, Option<u8>), DrillCell> {
    use DrillSkill as S;
    use PromptUnit as U;
    let mut cells = BTreeMap::new();
    cells.insert(("fixed.warmup", None), cell(S::Normal, U::Word));
    cells.insert(("fixed.check", None), cell(S::Normal, U::Sentence));
    cells.insert(("fixed.finish", None), cell(S::Burst, U::Word));
    let levels: [(&str, &[DrillCell]); 14] = [
        (
            "accuracy.gate",
            &[cell(S::Accuracy, U::Word), cell(S::Accuracy, U::Word), cell(S::Accuracy, U::Sentence)],
        ),
        ("accuracy.anticipation", &[cell(S::Anticipation, U::Word), cell(S::Anticipation, U::Word)]),
        (
            "transition.adaptive",
            &[cell(S::Weak, U::Word), cell(S::Weak, U::Word), cell(S::Weak, U::Word)],
        ),
        ("transition.ngram", &[cell(S::Weak, U::Ngram)]),
        (
            "preview.wide",
            &[
                cell(S::Preview, U::Sentence),
                cell(S::Preview, U::Sentence),
                cell(S::Preview, U::Sentence),
            ],
        ),
        ("preview.phrase", &[cell(S::Preview, U::Phrase), cell(S::Preview, U::Phrase)]),
        (
            "rhythm.metronome",
            &[cell(S::Rhythm, U::Word), cell(S::Rhythm, U::Word), cell(S::Rhythm, U::Word)],
        ),
        ("recovery.retry", &[cell(S::Recovery, U::Sentence), cell(S::Recovery, U::Sentence)]),
        ("overlap.burst", &[cell(S::Overlap, U::Word), cell(S::Overlap, U::Phrase)]),
        ("speed.pacer", &[cell(S::Speed, U::Phrase), cell(S::Speed, U::Phrase)]),
        ("reach.unlock", &[cell(S::Unlock, U::Word), cell(S::Unlock, U::Word)]),
        ("japanese.spelling", &[cell(S::Spelling, U::Word), cell(S::Spelling, U::Sentence)]),
        (
            "endurance.split",
            &[
                cell(S::Endurance, U::Sentence),
                cell(S::Endurance, U::Sentence),
                cell(S::Endurance, U::Sentence),
            ],
        ),
        ("blind.fade", &[cell(S::Blind, U::Word), cell(S::Blind, U::Word), cell(S::Blind, U::Word)]),
    ];
    for (id, row) in levels {
        for (index, cell) in row.iter().enumerate() {
            cells.insert((id, Some(index as u8 + 1)), *cell);
        }
    }
    cells
}

/// Levels whose mode is not their cell's own mode: the level overrides it
/// (the same in both languages).
fn expected_overrides() -> BTreeSet<(&'static str, u8, &'static str)> {
    [
        ("transition.adaptive", 3, "words"),
        ("transition.ngram", 1, "ngram"),
        ("rhythm.metronome", 3, "words"),
        ("overlap.burst", 1, "burst"),
        ("reach.unlock", 1, "words"),
        ("reach.unlock", 2, "words"),
    ]
    .into_iter()
    .collect()
}

/// Every fixed step and level of the catalog as a menu step in `language`.
fn all_steps(language: Lang) -> Vec<MenuStep> {
    let catalog = bundled_catalog().unwrap();
    let mut steps = Vec::new();
    for fixed in &catalog.fixed_steps {
        steps.push(MenuStep {
            role: fixed.role,
            language,
            prescription_id: fixed.id.clone(),
            title: fixed.title.text(Lang::En).to_owned(),
            level: None,
            mode: fixed.mode.clone(),
            minutes: fixed.minutes,
            reason: "-".to_owned(),
            focus: fixed.focus.text(Lang::En).to_owned(),
            advance: "-".to_owned(),
            advance_condition: None,
            evidence: None,
            evidence_note: None,
            settings: fixed.settings.clone(),
        });
    }
    for prescription in &catalog.prescriptions {
        for (index, level) in prescription.levels.iter().enumerate() {
            steps.push(MenuStep {
                role: MenuRole::PrimaryA,
                language,
                prescription_id: prescription.id.clone(),
                title: prescription.title.text(Lang::En).to_owned(),
                level: Some(index as u8 + 1),
                mode: level.mode.clone(),
                minutes: level.minutes,
                reason: "-".to_owned(),
                focus: level.focus.text(Lang::En).to_owned(),
                advance: level.advance.note.text(Lang::En).to_owned(),
                advance_condition: Some(level.advance.clone()),
                evidence: None,
                evidence_note: None,
                settings: level.settings.clone(),
            });
        }
    }
    steps
}

fn label(step: &MenuStep) -> String {
    format!("{} {:?} {:?}", step.prescription_id, step.level, step.language)
}

/// `japanese.spelling` is Japanese only, in the menu and on the table.
fn languages_of(step: &MenuStep) -> bool {
    !(step.prescription_id == "japanese.spelling" && step.language == Lang::En)
}

fn mode_kind(mode: &str) -> ModeKind {
    let config = config_for_step(&Config::default(), mode, &BTreeMap::new()).unwrap();
    config.mode.kind
}

#[test]
fn every_fixed_step_and_level_of_the_catalog_names_its_cell() {
    let catalog = bundled_catalog().unwrap();
    let expected = expected_cells();
    let mut seen = 0;
    for fixed in &catalog.fixed_steps {
        let cell = fixed.cell.unwrap_or_else(|| panic!("{} names no cell", fixed.id));
        assert_eq!(Some(&cell), expected.get(&(fixed.id.as_str(), None)), "{}", fixed.id);
        assert_eq!(catalog_cell(catalog, &fixed.id, None), Some(cell));
        seen += 1;
    }
    for prescription in &catalog.prescriptions {
        for (index, level) in prescription.levels.iter().enumerate() {
            let number = index as u8 + 1;
            let cell = level.cell.unwrap_or_else(|| panic!("{} level {number} names no cell", prescription.id));
            assert_eq!(
                Some(&cell),
                expected.get(&(prescription.id.as_str(), Some(number))),
                "{} level {number}",
                prescription.id
            );
            assert!(drill::is_available(cell), "{cell:?}");
            assert_eq!(catalog_cell(catalog, &prescription.id, Some(number)), Some(cell));
            seen += 1;
        }
    }
    assert_eq!(seen, expected.len(), "every entry is covered");
    // A step built from the catalog finds the same cell.
    for language in [Lang::En, Lang::Ja] {
        for step in all_steps(language) {
            assert_eq!(
                step_cell(&step).as_ref(),
                expected.get(&(step.prescription_id.as_str(), step.level)),
                "{}",
                label(&step)
            );
        }
    }
}

#[test]
fn a_catalog_cell_that_cannot_be_played_is_rejected() {
    let raw = include_str!("../../data/menu/menu-v1.toml");
    let broken = raw.replacen(
        "cell = { skill = \"normal\", unit = \"word\" }",
        "cell = { skill = \"burst\", unit = \"passage\" }",
        1,
    );
    assert_ne!(raw, broken);
    let error = rs_typing_trainer::practice::menu::parse_catalog(&broken).unwrap_err();
    assert!(error.contains("burst × passage") && error.contains("fixed.warmup"), "{error}");
}

#[test]
fn only_the_listed_levels_override_the_mode_of_their_cell() {
    let expected = expected_overrides()
        .into_iter()
        .map(|(id, level, mode)| (id.to_owned(), level, mode.to_owned()))
        .collect::<BTreeSet<_>>();
    for language in [Lang::En, Lang::Ja] {
        let mut overrides = BTreeSet::new();
        for step in all_steps(language).into_iter().filter(languages_of) {
            let cell = step_cell(&step).unwrap();
            if !drill::runs_as(cell, mode_kind(&step.mode), language) {
                let level = step.level.unwrap_or_else(|| panic!("a fixed step overrides its cell: {}", label(&step)));
                overrides.insert((step.prescription_id.clone(), level, step.mode.clone()));
            }
        }
        assert_eq!(overrides, expected, "{language:?}");
    }
}

/// The recorded configuration of every step is what it was before: the
/// level's mode and settings in the step's language. Only `mode.drill` (not
/// part of the configuration file, written to `mode_params`) is new.
#[test]
fn every_step_keeps_its_mode_and_settings() {
    let base = Config::default();
    for language in [Lang::En, Lang::Ja] {
        for step in all_steps(language) {
            let config = config_for_menu_step(&base, &step).unwrap();
            let mut before = config_for_step(&base, &step.mode, &step.settings).unwrap();
            let code = if language == Lang::Ja { "ja" } else { "en" };
            before.adaptive.lang = code.to_owned();
            before.mode.sentence.lang = code.to_owned();
            before.mode.phrase.lang = code.to_owned();
            if before.mode.kind == ModeKind::Endurance {
                before.endurance.minutes = u64::from(step.minutes);
            }
            assert_eq!(config.mode.kind.as_str(), step.mode, "{}", label(&step));
            assert_eq!(toml::to_string(&config).unwrap(), toml::to_string(&before).unwrap(), "{}", label(&step));
        }
    }
}

/// What the prompts of a configuration are drawn from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Pool {
    EnglishWord,
    JapaneseWord,
    EnglishPhrase,
    JapanesePhrase,
    EnglishSentence,
    JapaneseSentence,
    Combination,
}

fn pools(config: &Config, data: &DataCatalog) -> BTreeSet<Pool> {
    let layout = rs_typing_trainer::layout::builtin_layout("qwerty").unwrap();
    let mut generator = ModeGenerator::new(config.mode.clone(), data.clone(), layout, 11);
    let words_en = data.words_en().iter().cloned().collect::<BTreeSet<_>>();
    let words_ja = data.words_ja().iter().map(|row| row.text.clone()).collect::<BTreeSet<_>>();
    let phrases_en = data.phrases_en().iter().cloned().collect::<BTreeSet<_>>();
    let phrases_ja = data.phrases_ja().iter().map(|row| row.text.clone()).collect::<BTreeSet<_>>();
    let sentences_en = data.sentences_en().iter().cloned().collect::<BTreeSet<_>>();
    let sentences_ja = data.sentences_ja().iter().map(|row| row.text.clone()).collect::<BTreeSet<_>>();
    (0..8)
        .map(|_| {
            let prompt = generator.next_prompt();
            if prompt.kind == ItemKind::Drill {
                Pool::Combination
            } else if words_en.contains(&prompt.text) && prompt.reading.is_none() {
                Pool::EnglishWord
            } else if words_ja.contains(&prompt.text) {
                Pool::JapaneseWord
            } else if phrases_en.contains(&prompt.text) {
                Pool::EnglishPhrase
            } else if phrases_ja.contains(&prompt.text) {
                Pool::JapanesePhrase
            } else if sentences_en.contains(&prompt.text) {
                Pool::EnglishSentence
            } else if sentences_ja.contains(&prompt.text) {
                Pool::JapaneseSentence
            } else {
                panic!("unknown prompt {:?}", prompt.text)
            }
        })
        .collect()
}

/// The prompt generator treats modes alike except for these classes: the
/// key filter is off for running text and combinations, and `words` /
/// `unlock` fill a narrow pool with pseudo-words. A Japanese `burst` or
/// `rhythm` step is generated as `romaji` (MYTASK-3628) and the table's cell
/// as `burst` / `rhythm` with Japanese words: the same class.
fn generator_class(kind: ModeKind) -> u8 {
    match kind {
        ModeKind::Words | ModeKind::Unlock => 0,
        ModeKind::Sentence | ModeKind::Phrase | ModeKind::Endurance | ModeKind::Ngram => 1,
        ModeKind::Adaptive => 2,
        ModeKind::Random => 3,
        ModeKind::Romaji | ModeKind::Rhythm | ModeKind::Burst => 4,
    }
}

/// A step that runs its cell's mode makes its prompts exactly as the cell
/// started from the table does (same cell, unit, languages and prompt pool).
/// Excluded: the level's own settings (compared above) and the levels that
/// override the cell's mode, which keep the prompts of their own mode.
#[test]
fn a_step_makes_its_prompts_as_its_cell_does_on_the_table() {
    let base = Config::default();
    let data = DataCatalog::load(&Default::default(), std::path::Path::new("")).unwrap();
    for language in [Lang::En, Lang::Ja] {
        for step in all_steps(language).into_iter().filter(languages_of) {
            let label = label(&step);
            let cell = step_cell(&step).unwrap();
            let menu = config_for_menu_step(&base, &step).unwrap();
            let overridden = expected_overrides()
                .iter()
                .any(|(id, level, _)| *id == step.prescription_id && Some(*level) == step.level);
            if overridden {
                // The level's mode and its own prompts, as before.
                assert_eq!(menu.mode.drill, None, "{label}");
                assert_eq!(menu.mode.prompt_unit(), PromptUnit::natural(menu.mode.kind), "{label}");
                continue;
            }
            let table = drill::config_for_cell(&base, cell, language).unwrap();
            assert_eq!(menu.mode.drill, Some(cell), "{label}");
            assert_eq!(menu.mode.drill, table.mode.drill, "{label}");
            assert_eq!(menu.mode.prompt_unit(), table.mode.prompt_unit(), "{label}");
            assert_eq!(menu.mode.sentence.lang, table.mode.sentence.lang, "{label}");
            assert_eq!(menu.mode.phrase.lang, table.mode.phrase.lang, "{label}");
            assert_eq!(menu.adaptive.lang, table.adaptive.lang, "{label}");
            assert_eq!(drill::japanese_words(&menu.mode), drill::japanese_words(&table.mode), "{label}");
            // The generator runs the cell's mode (a Japanese `words` step is
            // fed Japanese words, as the `romaji` cell is).
            let mut generated = menu.clone();
            generated.mode.kind = generator_kind_for_step(menu.mode.kind, language);
            assert_eq!(generator_class(generated.mode.kind), generator_class(table.mode.kind), "{label}");
            if generated.mode.kind == ModeKind::Adaptive {
                continue; // The adaptive plan needs records; compared above.
            }
            // Same prompt pool once the level's own settings are set alike.
            let mut aligned = table.clone();
            aligned.mode.phrase.course.clone_from(&generated.mode.phrase.course);
            aligned.mode.key_filter = generated.mode.key_filter;
            aligned.mode.base_keys.clone_from(&generated.mode.base_keys);
            aligned.mode.keys.clone_from(&generated.mode.keys);
            assert_eq!(pools(&generated, &data), pools(&aligned, &data), "{label}");
        }
    }
}

// ---------------------------------------------------------------------------
// Playing a menu step
// ---------------------------------------------------------------------------

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

fn date() -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(2026, 11, 20).unwrap()
}

fn config(lang: Lang) -> Config {
    let mut config = Config::default();
    config.general.lang = lang;
    config.session.result_input_delay_ms = 0;
    config.ui.animation_enabled = false;
    config
}

fn find_step(id: &str, level: Option<u8>, language: Lang, display: Lang) -> MenuStep {
    let mut step = all_steps(language)
        .into_iter()
        .find(|step| step.prescription_id == id && step.level == level)
        .unwrap();
    let catalog = bundled_catalog().unwrap();
    if let Some(fixed) = catalog.fixed_steps.iter().find(|fixed| fixed.id == id) {
        step.title = fixed.title.text(display).to_owned();
        step.focus = fixed.focus.text(display).to_owned();
    } else if let Some(prescription) = catalog.prescription(id) {
        step.title = prescription.title.text(display).to_owned();
        let row = &prescription.levels[usize::from(level.unwrap() - 1)];
        step.focus = row.focus.text(display).to_owned();
        step.advance = row.advance.note.text(display).to_owned();
    }
    step
}

/// An app whose menu of today is exactly `steps`, with the menu open.
fn menu_app(display: Lang, steps: Vec<MenuStep>) -> TestApp {
    let mut app = test_app_at(config(display), date());
    app.handle_key(key(KeyCode::Char('m')));
    assert_eq!(app.screen, Screen::Menu);
    app.daily_menu = Some(DailyMenu {
        date: date(),
        language: "en".to_owned(),
        catalog_version: "menu-v1".to_owned(),
        requested_minutes: 15,
        introductory: false,
        bench_recommended: false,
        steps,
    });
    app.menu_selected = 0;
    app
}

fn type_prompt(app: &mut App, mut now: Instant) -> Instant {
    let target = app.game.as_ref().unwrap().current_prompt.target.clone();
    for character in target.chars() {
        now += Duration::from_millis(150);
        app.on_tick_at(Duration::from_millis(1), now).unwrap();
        if app.screen != Screen::Playing {
            return now;
        }
        app.handle_key_at(key(KeyCode::Char(character)), now);
    }
    now
}

/// Start the selected step, type one prompt and end it by time.
fn play_selected(app: &mut App, now: Instant) -> Instant {
    app.handle_key_at(key(KeyCode::Enter), now);
    assert!(matches!(app.screen, Screen::MenuFocus { .. }), "{:?}", app.screen);
    app.handle_key_at(key(KeyCode::Char('x')), now + Duration::from_millis(10));
    let started = now + Duration::from_secs(4);
    app.on_tick_at(Duration::from_secs(3), started).unwrap();
    assert_eq!(app.screen, Screen::Playing);
    let end = type_prompt(app, started);
    if app.screen == Screen::Playing {
        let seconds = app.config.session.duration_seconds;
        app.on_tick_at(Duration::from_secs(1), started + Duration::from_secs(seconds + 1))
            .unwrap();
    }
    assert_eq!(app.screen, Screen::Result);
    end
}

/// Steps played from the menu record the level's mode, and the cell joins
/// `mode_params` when the step ran the cell's mode.
#[test]
fn played_steps_record_the_levels_mode_and_their_cell() {
    for (id, level, language, mode, drilled) in [
        ("fixed.warmup", None, Lang::Ja, "words", true),
        ("accuracy.gate", Some(3), Lang::En, "sentence", true),
        ("accuracy.anticipation", Some(1), Lang::Ja, "words", true),
        ("preview.phrase", Some(1), Lang::En, "phrase", true),
        ("rhythm.metronome", Some(3), Lang::En, "words", false),
        ("reach.unlock", Some(1), Lang::Ja, "words", false),
    ] {
        let step = find_step(id, level, language, Lang::En);
        let expected = step_cell(&step).unwrap();
        let mut app = menu_app(Lang::En, vec![step]);
        play_selected(&mut app, Instant::now());
        let db_path = app.config.storage.db_path.clone().unwrap();
        let records = db::load_records_filtered(&db_path, None, Some(mode)).unwrap();
        assert_eq!(records.len(), 1, "{id} {level:?}: recorded as {mode}");
        let params: serde_json::Value = serde_json::from_str(&records[0].header.mode_params).unwrap();
        let japanese = if language == Lang::Ja { "ja" } else { "en" };
        assert_eq!(params["lang"], japanese, "{id} {level:?}: the step's language");
        if drilled {
            assert_eq!(params["drill"]["skill"], expected.skill.as_str(), "{id} {level:?}");
            assert_eq!(params["unit"], expected.unit.as_str(), "{id} {level:?}");
        } else {
            assert!(params.get("drill").is_none(), "{id} {level:?}: {params}");
        }
        // The user's configuration is back after the step.
        assert_eq!(app.config.mode.drill, None, "{id} {level:?}");
    }
}

/// A Japanese menu step is played in Japanese whatever the table's practice
/// language is.
#[test]
fn a_japanese_step_keeps_its_language_over_the_tables() {
    let step = find_step("preview.wide", Some(1), Lang::Ja, Lang::En);
    let mut app = menu_app(Lang::En, vec![step]);
    app.config.drill.lang = Some(Lang::En);
    app.practice_lang = Lang::En;
    let now = Instant::now();
    app.handle_key_at(key(KeyCode::Enter), now);
    app.handle_key_at(key(KeyCode::Char('x')), now + Duration::from_millis(10));
    app.on_tick_at(Duration::from_secs(3), now + Duration::from_secs(4)).unwrap();
    assert_eq!(app.screen, Screen::Playing);
    assert_eq!(app.config.mode.sentence.lang, "ja");
    assert_eq!(app.config.mode.drill, Some(cell(DrillSkill::Preview, PromptUnit::Sentence)));
    assert!(app.game.as_ref().unwrap().current_prompt.reading.is_some());
}

/// The menu screen, the focus screen before the countdown and the result's
/// step row name the cell; everything fits 80×24 in both languages, and the
/// progress band keeps its short names.
#[test]
fn the_menu_focus_and_result_screens_name_the_cell_at_80_by_24() {
    for display in [Lang::En, Lang::Ja] {
        let steps = vec![
            find_step("fixed.warmup", None, Lang::En, display),
            find_step("accuracy.gate", Some(1), Lang::En, display),
            find_step("japanese.spelling", Some(1), Lang::Ja, display),
            find_step("fixed.check", None, Lang::En, display),
            find_step("fixed.finish", None, Lang::En, display),
        ];
        let mut app = menu_app(display, steps);
        app.menu_selected = 1;
        let (accuracy, spelling, warmup) = match display {
            Lang::En => ("Accuracy×Words", "Spelling×Words", "Normal×Words"),
            Lang::Ja => ("正確さ×単語", "綴り×単語", "ふつう×単語"),
        };
        let rows = render(&app, 80, 24);
        if std::env::var_os("RS_TYPING_TRAINER_DUMP_UI").is_some() {
            println!("\n--- menu {display:?} 80x24 ---\n{}", rows.join("\n"));
        }
        let screen = compact(&rows);
        let drill = match display {
            Lang::En => "Drill:",
            Lang::Ja => "訓練:",
        };
        assert!(screen.contains(&format!("{drill}{accuracy}")), "{display:?}:\n{}", rows.join("\n"));
        assert_frame(&rows, display);
        app.menu_selected = 2;
        let screen = compact(&render(&app, 80, 24));
        assert!(screen.contains(&format!("{drill}{spelling}")), "{display:?}:\n{screen}");

        // Focus screen of the first step.
        app.menu_selected = 0;
        let now = Instant::now();
        app.handle_key_at(key(KeyCode::Enter), now);
        assert!(matches!(app.screen, Screen::MenuFocus { .. }));
        let rows = render(&app, 80, 24);
        if std::env::var_os("RS_TYPING_TRAINER_DUMP_UI").is_some() {
            println!("\n--- focus {display:?} 80x24 ---\n{}", rows.join("\n"));
        }
        assert!(compact(&rows).contains(warmup), "{display:?}:\n{}", rows.join("\n"));
        assert_frame(&rows, display);

        // Result of the accuracy step: the step row names the cell.
        let mut app = menu_app(
            display,
            vec![
                find_step("fixed.warmup", None, Lang::En, display),
                find_step("accuracy.gate", Some(1), Lang::En, display),
            ],
        );
        app.menu_selected = 1;
        play_selected(&mut app, now + Duration::from_secs(1));
        let rows = render(&app, 80, 24);
        if std::env::var_os("RS_TYPING_TRAINER_DUMP_UI").is_some() {
            println!("\n--- result {display:?} 80x24 ---\n{}", rows.join("\n"));
        }
        let screen = compact(&rows);
        assert!(screen.contains(accuracy), "{display:?}:\n{}", rows.join("\n"));
        let condition = match display {
            Lang::En => "Graduationcondition:",
            Lang::Ja => "卒業の条件:",
        };
        assert!(screen.contains(condition), "{display:?}:\n{}", rows.join("\n"));
        assert_frame(&rows, display);
        // The band keeps the short names of the steps.
        let band = rs_typing_trainer::ui::menu::progress_band_after_step(&app, 76).unwrap();
        assert!(!band.contains('×'), "{band}");
    }
}

fn assert_frame(rows: &[String], display: Lang) {
    assert!(rows[0].starts_with('┌') && rows[0].ends_with('┐'), "{display:?}: {}", rows[0]);
    assert!(rows[23].starts_with('└') && rows[23].ends_with('┘'), "{display:?}: {}", rows[23]);
    for row in &rows[1..23] {
        assert!(row.starts_with('│') && row.ends_with('│'), "{display:?}: {row}");
    }
}

/// The fixed steps' result row names the cell even when nothing is judged.
#[test]
fn a_fixed_steps_result_names_its_cell() {
    let step = find_step("fixed.warmup", None, Lang::En, Lang::Ja);
    let mut app = menu_app(Lang::Ja, vec![step]);
    play_selected(&mut app, Instant::now());
    let screen = compact(&render(&app, 80, 24));
    assert!(screen.contains("ふつう×単語・メニュー1/1ウォームアップ"), "{screen}");
}
