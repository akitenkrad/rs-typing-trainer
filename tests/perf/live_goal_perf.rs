//! MYTASK-3633, MYTASK-3641: the live goal re-measures the whole step log on
//! every keystroke. Keep that within a frame for a long step, for every goal
//! kind and for logs of one long item and of one word per item.
use std::time::{Duration, Instant};

use chrono::{TimeZone, Utc};
use rs_typing_trainer::config::Config;
use rs_typing_trainer::config::{DrillCell, DrillSkill, PromptUnit};
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::engine::session::{ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::layout::builtin_layout;
use rs_typing_trainer::practice::menu::live_goal::{GoalKind, GoalSetup, LiveGoal, TargetTransition};
use rs_typing_trainer::practice::menu::{MenuRole, MenuStep, SpellingTarget, bundled_catalog};
use rs_typing_trainer::record::SessionRecord;

const KEYSTROKES: usize = 2000;

fn step(id: &str) -> MenuStep {
    let catalog = bundled_catalog().unwrap();
    if let Some(fixed) = catalog.fixed_steps.iter().find(|step| step.id == id) {
        return MenuStep {
            role: fixed.role,
            language: Lang::En,
            prescription_id: fixed.id.clone(),
            title: fixed.title.text(Lang::En).to_owned(),
            level: None,
            mode: fixed.mode.clone(),
            minutes: fixed.minutes,
            reason: "-".to_owned(),
            focus: "-".to_owned(),
            advance: "-".to_owned(),
            advance_condition: None,
            evidence: None,
            evidence_note: None,
            settings: fixed.settings.clone(),
        };
    }
    let prescription = catalog.prescription(id).unwrap();
    let row = &prescription.levels[0];
    MenuStep {
        role: MenuRole::PrimaryA,
        language: Lang::En,
        prescription_id: id.to_owned(),
        title: "-".to_owned(),
        level: Some(1),
        mode: row.mode.clone(),
        minutes: row.minutes,
        reason: "-".to_owned(),
        focus: "-".to_owned(),
        advance: "-".to_owned(),
        advance_condition: Some(row.advance.clone()),
        evidence: None,
        evidence_note: None,
        settings: row.settings.clone(),
    }
}

fn long_log() -> rs_typing_trainer::engine::session::SessionLog {
    let words = ["the", "quick", "brown", "fox", "jumps", "over", "lazy", "dog", "while", "typing"];
    let mut text = String::new();
    let mut index = 0;
    while text.chars().count() < KEYSTROKES + 10 {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(words[index % words.len()]);
        index += 1;
    }
    let mut session = Session::new(Duration::from_secs(600), Duration::ZERO, Item::exact(text.clone()));
    session.activate(Duration::ZERO);
    for (position, character) in text.chars().take(KEYSTROKES).enumerate() {
        let now = Duration::from_millis(150 * position as u64 + 100 + (position as u64 * 37) % 60);
        if position % 97 == 50 {
            session.input_char(now, "z");
        }
        session.input_char(now + Duration::from_millis(5), &character.to_string());
    }
    session.into_log()
}

/// A log of one item per word (`text` typed as `typed`), with key releases
/// and a mistype every 97 keystrokes.
fn word_log(words: &[(&str, Option<&str>)]) -> SessionLog {
    let mut items = Vec::new();
    let mut keystrokes = Vec::new();
    let mut time_us = 100_000_u64;
    let mut index = 0_usize;
    while keystrokes.len() < KEYSTROKES {
        let (typed, reading) = words[index % words.len()];
        let idx = index as u32;
        let shown = time_us;
        for (pos, character) in typed.chars().enumerate() {
            if keystrokes.len() % 97 == 50 {
                keystrokes.push(KeystrokeLog {
                    seq: keystrokes.len() as u32,
                    t_us: time_us,
                    release_us: Some(time_us + 60_000),
                    item_idx: idx,
                    pos: pos as u32,
                    kind: KeystrokeKind::Char,
                    typed: Some("z".to_owned()),
                    expected: Some(character.to_string()),
                    correct: false,
                    code: None,
                });
                time_us += 120_000;
            }
            keystrokes.push(KeystrokeLog {
                seq: keystrokes.len() as u32,
                t_us: time_us,
                release_us: Some(time_us + 40_000 + (keystrokes.len() as u64 * 37) % 90_000),
                item_idx: idx,
                pos: pos as u32,
                kind: KeystrokeKind::Char,
                typed: Some(character.to_string()),
                expected: Some(character.to_string()),
                correct: true,
                code: None,
            });
            time_us += 120_000 + (keystrokes.len() as u64 * 53) % 70_000;
        }
        items.push(ItemLog {
            idx,
            text: reading.map_or_else(|| typed.to_owned(), str::to_owned),
            reading: reading.map(str::to_owned),
            kind: None,
            shown_us: shown,
            done_us: Some(time_us),
            outcome: ItemOutcome::Completed,
        });
        time_us += 200_000;
        index += 1;
    }
    SessionLog {
        duration_us: time_us,
        error_mode: Default::default(),
        items,
        keystrokes,
    }
}

fn skill(kind: GoalKind) -> DrillSkill {
    DrillSkill::ALL
        .into_iter()
        .find(|skill| GoalKind::for_skill(*skill) == Some(kind))
        .unwrap()
}

#[test]
fn live_goal_update_for_a_long_step_stays_within_a_frame() {
    let _guard = crate::perf_lock();
    let config = Config::default();
    let layout = builtin_layout("qwerty").unwrap();
    let mut long = long_log();
    assert!(long.keystrokes.len() >= KEYSTROKES, "only {} keystrokes", long.keystrokes.len());
    long.duration_us = long.keystrokes.last().unwrap().t_us;
    let english = word_log(&[
        ("the ", None),
        ("quick ", None),
        ("brown ", None),
        ("fox ", None),
        ("jumps ", None),
        ("over ", None),
        ("lazy ", None),
        ("dog ", None),
    ]);
    let japanese = word_log(&[
        ("shikashi", Some("しかし")),
        ("tsuchi", Some("つち")),
        ("sushi", Some("すし")),
        ("kashi", Some("かし")),
    ]);
    let mut config_ja = config.clone();
    config_ja.mode.kind = rs_typing_trainer::config::ModeKind::Romaji;
    let mut worst = Duration::ZERO;
    for kind in GoalKind::ALL {
        let setup = GoalSetup {
            gate_min_accuracy: 0.97,
            pacer_target_wpm: Some(60.0),
            planned: Duration::from_secs(300),
            own_median: Some(0.3),
            previous_runs: vec![50.0],
            spelling: Some(SpellingTarget {
                kana: "し".to_owned(),
                current: "shi".to_owned(),
                candidate: "si".to_owned(),
            }),
            transitions: ["th", "he", "ow", "qu", "az"]
                .iter()
                .map(|pair| TargetTransition {
                    from: pair[..1].to_owned(),
                    to: pair[1..].to_owned(),
                    prior_ms: Some(150.0),
                })
                .collect(),
            ..GoalSetup::default()
        };
        let goal = LiveGoal::for_cell(DrillCell::new(skill(kind), PromptUnit::Word), setup).unwrap();
        for (name, log, config) in [
            ("one item", &long, &config),
            ("words", &english, &config),
            ("japanese", &japanese, &config_ja),
        ] {
            let mut samples = Vec::new();
            for _ in 0..5 {
                let start = Instant::now();
                let mut record = SessionRecord::from_log(config, log.clone(), Utc.with_ymd_and_hms(2026, 11, 20, 0, 0, 0).unwrap());
                record.header.release_events = true;
                std::hint::black_box(goal.measure(&record, &layout));
                samples.push(start.elapsed());
            }
            samples.sort();
            let median = samples[samples.len() / 2];
            worst = worst.max(median);
            println!(
                "live goal {kind:?} ({name}): {KEYSTROKES} keystrokes median={:.3} ms (debug_assertions={})",
                median.as_secs_f64() * 1000.0,
                cfg!(debug_assertions)
            );
            if !cfg!(debug_assertions) {
                assert!(median <= Duration::from_millis(8), "{kind:?} ({name}): median {median:?} exceeds 8 ms");
            }
        }
    }
    println!("live goal worst median {:.3} ms", worst.as_secs_f64() * 1000.0);
    // The menu steps keep their goals too.
    for id in ["accuracy.gate", "fixed.finish", "recovery.retry", "transition.adaptive"] {
        assert!(GoalKind::for_step(&step(id)).is_some(), "{id}");
    }
}
