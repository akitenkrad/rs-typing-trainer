//! MYTASK-3633: the live goal re-measures the whole step log on every
//! keystroke. Keep that within a frame for a long step.
use std::time::{Duration, Instant};

use chrono::{TimeZone, Utc};
use rs_typing_trainer::config::Config;
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::layout::builtin_layout;
use rs_typing_trainer::practice::menu::live_goal::{GoalSetup, LiveGoal};
use rs_typing_trainer::practice::menu::{MenuRole, MenuStep, bundled_catalog};
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

#[test]
fn live_goal_update_for_a_long_step_stays_within_a_frame() {
    let _guard = crate::perf_lock();
    let config = Config::default();
    let layout = builtin_layout("qwerty").unwrap();
    let mut log = long_log();
    assert!(log.keystrokes.len() >= KEYSTROKES, "only {} keystrokes", log.keystrokes.len());
    log.duration_us = log.keystrokes.last().unwrap().t_us;
    for id in [
        "accuracy.gate",
        "accuracy.anticipation",
        "rhythm.metronome",
        "speed.pacer",
        "endurance.split",
        "fixed.finish",
    ] {
        let goal = LiveGoal::new(
            &step(id),
            GoalSetup {
                gate_min_accuracy: 0.97,
                pacer_target_wpm: Some(60.0),
                planned: Duration::from_secs(300),
                own_median: Some(0.3),
                previous_runs: vec![50.0],
            },
        )
        .unwrap();
        let mut samples = Vec::new();
        for _ in 0..5 {
            let start = Instant::now();
            let record = SessionRecord::from_log(&config, log.clone(), Utc.with_ymd_and_hms(2026, 11, 20, 0, 0, 0).unwrap());
            std::hint::black_box(goal.measure(&record, &layout));
            samples.push(start.elapsed());
        }
        samples.sort();
        let median = samples[samples.len() / 2];
        println!(
            "live goal {id}: {KEYSTROKES} keystrokes median={:.3} ms (debug_assertions={})",
            median.as_secs_f64() * 1000.0,
            cfg!(debug_assertions)
        );
        if !cfg!(debug_assertions) {
            assert!(median <= Duration::from_millis(8), "{id}: median {median:?} exceeds 8 ms");
        }
    }
}
