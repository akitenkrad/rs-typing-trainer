//! MYTASK-3633: the live goal of a menu step — the meter while typing and the
//! headline of the result screen — for the six prescriptions of the first
//! batch, and its absence on the «normal» row. The other rows and the table
//! cells are in `live_goal_rows` (MYTASK-3641).

use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::app::{App, Screen};
use rs_typing_trainer::config::Config;
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::practice::menu::live_goal::{GoalKind, Tone};
use rs_typing_trainer::practice::menu::progress::{JudgeContext, session_metric_value};
use rs_typing_trainer::practice::menu::{DailyMenu, MenuMetric, MenuRole, MenuStep, bundled_catalog};
use rs_typing_trainer::ui;

use crate::common::{TestApp, test_app_at};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn render(app: &App, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect()
}

/// The screen without spaces (full-width characters are followed by an
/// empty cell).
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

fn prescription_step(id: &str, level: u8) -> MenuStep {
    let prescription = bundled_catalog().unwrap().prescription(id).unwrap();
    let row = &prescription.levels[usize::from(level - 1)];
    MenuStep {
        role: MenuRole::PrimaryA,
        language: Lang::En,
        prescription_id: id.to_owned(),
        title: prescription.title.text(Lang::En).to_owned(),
        level: Some(level),
        mode: row.mode.clone(),
        minutes: row.minutes,
        reason: "-".to_owned(),
        focus: row.focus.text(Lang::En).to_owned(),
        advance: row.advance.note.text(Lang::En).to_owned(),
        advance_condition: Some(row.advance.clone()),
        evidence: None,
        evidence_note: None,
        settings: row.settings.clone(),
    }
}

fn fixed_step(id: &str) -> MenuStep {
    let catalog = bundled_catalog().unwrap();
    let fixed = catalog.fixed_steps.iter().find(|step| step.id == id).unwrap();
    MenuStep {
        role: fixed.role,
        language: Lang::En,
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
    }
}

/// An app whose menu of today is exactly `steps`, with the menu open.
fn menu_app(lang: Lang, steps: Vec<MenuStep>) -> TestApp {
    let mut app = test_app_at(config(lang), date());
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

/// Start the selected step and return the time the session started.
fn start_step(app: &mut App, now: Instant) -> Instant {
    app.handle_key_at(key(KeyCode::Enter), now);
    if app.screen == Screen::Briefing {
        app.handle_key_at(key(KeyCode::Enter), now + Duration::from_millis(10));
    }
    assert!(matches!(app.screen, Screen::Countdown { .. }));
    let started = now + Duration::from_secs(4);
    app.on_tick_at(Duration::from_secs(3), started).unwrap();
    assert_eq!(app.screen, Screen::Playing);
    started
}

/// Type `count` keystrokes from `at`, `gap(i)` apart. `wrong(i)` presses a
/// key that is not the expected one. Returns the time after the last key.
fn type_keys(app: &mut App, mut at: Instant, count: usize, gap: impl Fn(usize) -> u64, wrong: impl Fn(usize) -> bool) -> Instant {
    for index in 0..count {
        app.on_tick_at(Duration::from_millis(1), at).unwrap();
        if app.screen != Screen::Playing {
            break;
        }
        let Some(expected) = app.game.as_ref().and_then(|game| game.session.expected()) else {
            at += Duration::from_millis(gap(index));
            continue;
        };
        let expected = expected.chars().next().unwrap();
        let pressed = if wrong(index) {
            if expected == 'q' { 'z' } else { 'q' }
        } else {
            expected
        };
        app.handle_key_at(key(KeyCode::Char(pressed)), at);
        at += Duration::from_millis(gap(index));
    }
    at
}

/// End the session by time.
fn finish(app: &mut App, started: Instant) {
    let seconds = app.config.session.duration_seconds;
    app.on_tick_at(Duration::from_secs(1), started + Duration::from_secs(seconds + 1))
        .unwrap();
    assert_eq!(app.screen, Screen::Result);
}

fn uneven(index: usize) -> u64 {
    [110, 190, 140, 260, 120, 170][index % 6]
}

#[test]
fn the_dispatch_follows_the_row_of_the_step_cell() {
    // MYTASK-3641: the goal comes from the row of the step's cell, so the
    // six steps of the first batch keep theirs and the other prescriptions
    // get the goal of their row.
    for (id, kind) in [
        ("accuracy.gate", GoalKind::AccuracyGate),
        ("accuracy.anticipation", GoalKind::Anticipation),
        ("rhythm.metronome", GoalKind::Metronome),
        ("speed.pacer", GoalKind::Pacer),
        ("endurance.split", GoalKind::EnduranceSplit),
        ("preview.wide", GoalKind::Preview),
        ("preview.phrase", GoalKind::Preview),
        ("transition.adaptive", GoalKind::WeakTransition),
        ("transition.ngram", GoalKind::WeakTransition),
        ("recovery.retry", GoalKind::Recovery),
        ("overlap.burst", GoalKind::Overlap),
        ("reach.unlock", GoalKind::Reach),
        ("japanese.spelling", GoalKind::Spelling),
        ("blind.fade", GoalKind::Blind),
    ] {
        assert_eq!(GoalKind::for_step(&prescription_step(id, 1)), Some(kind), "{id}");
    }
    assert_eq!(GoalKind::for_step(&fixed_step("fixed.finish")), Some(GoalKind::Sprint));
    for id in ["fixed.warmup", "fixed.check"] {
        assert_eq!(GoalKind::for_step(&fixed_step(id)), None, "{id}");
    }
}

#[test]
fn individual_training_shows_no_goal_while_playing_or_on_the_result() {
    let mut app = test_app_at(config(Lang::En), date());
    app.config.session.duration_seconds = 20;
    let now = Instant::now();
    app.handle_key_at(key(KeyCode::Enter), now);
    // Individual training: title → mode select → countdown.
    while !matches!(app.screen, Screen::Countdown { .. }) {
        app.handle_key_at(key(KeyCode::Enter), now);
    }
    let started = now + Duration::from_secs(4);
    app.on_tick_at(Duration::from_secs(3), started).unwrap();
    assert_eq!(app.screen, Screen::Playing);
    assert!(app.menu_active_step.is_none());
    type_keys(&mut app, started, 20, uneven, |index| index % 7 == 3);
    assert!(app.menu_live_goal.is_none());
    let playing = render(&app, 80, 24).join("\n");
    assert!(playing.contains("Combo"), "the ordinary sub stats stay:\n{playing}");
    assert!(!playing.contains("gate") && !playing.contains("Streak"), "{playing}");
    finish(&mut app, started);
    let result = render(&app, 80, 24).join("\n");
    assert!(!result.contains("Errors in time") && !result.contains("Passed the"), "{result}");
}

#[test]
fn warm_up_and_check_steps_keep_the_ordinary_screens() {
    for id in ["fixed.warmup", "fixed.check"] {
        let mut app = menu_app(Lang::En, vec![fixed_step(id)]);
        let started = start_step(&mut app, Instant::now());
        type_keys(&mut app, started, 10, uneven, |_| false);
        assert!(app.menu_live_goal.is_none(), "{id}");
        let playing = render(&app, 80, 24).join("\n");
        assert!(playing.contains("Combo") && !playing.contains("Streak"), "{id}:\n{playing}");
    }
}

#[test]
fn accuracy_gate_meter_and_result_match_the_recorded_accuracy() {
    let mut app = menu_app(Lang::En, vec![prescription_step("accuracy.gate", 1)]);
    let started = start_step(&mut app, Instant::now());
    let end = type_keys(&mut app, started, 60, uneven, |index| index % 15 == 7);
    let state = app.menu_live_goal.clone().expect("a live goal");
    assert_eq!(state.goal.kind, GoalKind::AccuracyGate);
    assert_eq!(state.goal.target, Some(0.97));
    let live = state.measure.value.expect("live accuracy");

    let rows = render(&app, 80, 24);
    let text = rows.join("\n");
    assert!(text.contains("Accuracy") && text.contains("gate 97%"), "{text}");
    assert!(text.contains("Streak") && text.contains("gate margin"), "{text}");
    // The meter replaces nothing above it: the prompt and footer are still drawn.
    assert!(rows[22].contains("[ESC] Abort"), "{text}");
    assert!(live < 0.97, "four errors in sixty keys miss the gate: {live}");
    assert_eq!(state.goal.tone(&state.measure), Tone::Bad);

    let _ = end;
    finish(&mut app, started);
    let record = app.result_record.clone().unwrap();
    let finished = app.menu_live_goal.as_ref().unwrap().finished.clone().unwrap();
    assert_eq!(live, record.header.accuracy, "the last live value is the recorded accuracy");
    assert_eq!(finished.value, Some(record.header.accuracy));
    assert_eq!(finished.met, Some(false));

    let rows = render(&app, 80, 24);
    let text = rows.join("\n");
    for expected in [
        "Accuracy gate Lv1",
        "Below the 97% gate",
        "Longest streak",
        "Errors in time",
        "Graduation condition",
    ] {
        assert!(text.contains(expected), "missing {expected:?}:\n{text}");
    }
    assert!(text.contains("in a row: 0/3"), "the 3-in-a-row progress:\n{text}");
    assert!(text.contains('×'), "error marks on the time line:\n{text}");
    let hero_row = rows.iter().position(|row| row.contains("Below the 97% gate")).unwrap();
    let others_row = rows.iter().position(|row| row.contains("CPS")).unwrap();
    assert!(hero_row < others_row, "the goal comes first:\n{text}");
}

#[test]
fn accuracy_gate_is_shown_in_japanese_at_80_by_24() {
    let mut app = menu_app(Lang::Ja, vec![prescription_step("accuracy.gate", 2)]);
    let started = start_step(&mut app, Instant::now());
    type_keys(&mut app, started, 40, uneven, |_| false);
    let text = compact(&render(&app, 80, 24));
    assert!(text.contains("正確さ") && text.contains("関門96%") && text.contains("連続"), "{text}");
    finish(&mut app, started);
    let text = compact(&render(&app, 80, 24));
    for expected in ["関門96%を通過", "最長の連続", "誤打の位置", "3回連続:1/3", "卒業の条件"] {
        assert!(text.contains(expected), "missing {expected:?}:\n{text}");
    }
    assert!(!text.contains("較正"));
}

#[test]
fn metronome_cv_is_the_judged_value_and_has_no_baseline_without_checks() {
    let mut app = menu_app(Lang::En, vec![prescription_step("rhythm.metronome", 3)]);
    let started = start_step(&mut app, Instant::now());
    type_keys(&mut app, started, 80, uneven, |index| index % 20 == 9);
    let state = app.menu_live_goal.clone().unwrap();
    assert_eq!(state.goal.kind, GoalKind::Metronome);
    assert_eq!(state.goal.target, None, "no check steps yet");
    let live = state.measure.value.expect("live CV");
    let text = render(&app, 80, 24).join("\n");
    assert!(
        text.contains("In-word CV") && text.contains("no baseline") && text.contains("Recent intervals"),
        "{text}"
    );

    finish(&mut app, started);
    let record = app.result_record.clone().unwrap();
    let db_path = app.config.storage.db_path.clone().unwrap();
    let home = app.home_dir.clone();
    let config = app.config.clone();
    let layout = app.practice_layout().clone();
    let context = JudgeContext {
        db_path: &db_path,
        home: &home,
        config: &config,
        layout: &layout,
        as_of: date(),
    };
    let judged = session_metric_value(&context, MenuMetric::WithinWordIkiCv, &record, "en")
        .unwrap()
        .unwrap();
    assert!((live - judged).abs() < 1e-12, "live {live} judged {judged}");
    assert_eq!(app.menu_live_goal.as_ref().unwrap().finished.as_ref().unwrap().value, Some(live));

    let text = render(&app, 80, 24).join("\n");
    for expected in [
        "In-word IKI CV",
        "No baseline yet",
        "Intervals",
        "in-word intervals, median",
        "Graduation condition",
    ] {
        assert!(text.contains(expected), "missing {expected:?}:\n{text}");
    }
}

#[test]
fn metronome_in_rhythm_mode_keeps_the_beat_row_and_the_meter() {
    let mut app = menu_app(Lang::Ja, vec![prescription_step("rhythm.metronome", 1)]);
    let started = start_step(&mut app, Instant::now());
    type_keys(&mut app, started, 30, uneven, |_| false);
    let rows = render(&app, 80, 24);
    let text = compact(&rows);
    assert!(text.contains("拍") && text.contains("語中CV") && text.contains("直近の間隔"), "{text}");
    let beat = rows.iter().position(|row| row.contains("BPM")).unwrap();
    let meter = rows.iter().position(|row| row.contains("CV")).unwrap();
    assert_ne!(beat, meter);
}

#[test]
fn anticipation_flashes_and_reports_without_a_baseline() {
    let mut app = menu_app(Lang::En, vec![prescription_step("accuracy.anticipation", 1)]);
    let started = start_step(&mut app, Instant::now());
    let mut at = type_keys(&mut app, started, 6, |_| 150, |_| false);
    // Type the character after the expected one: an anticipation error.
    app.on_tick_at(Duration::from_millis(1), at).unwrap();
    // Move on (typing correctly) until the next two characters differ: a
    // doubled letter («letter») would make the «next» key a correct one. The
    // prompts are random, so this used to fail now and then.
    let mut found = None;
    for _ in 0..200 {
        let game = app.game.as_ref().unwrap();
        let target = game.current_prompt.target.chars().collect::<Vec<_>>();
        let position = game.session.position();
        if position + 1 < target.len() && target[position + 1] != target[position] {
            found = Some(target[position + 1]);
            break;
        }
        at = type_keys(&mut app, at, 1, |_| 150, |_| false);
        app.on_tick_at(Duration::from_millis(1), at).unwrap();
    }
    let next = found.expect("a position whose next character differs");
    app.handle_key_at(key(KeyCode::Char(next)), at);
    let state = app.menu_live_goal.clone().unwrap();
    assert_eq!(state.measure.anticipations, 1, "{:?}", state.measure);
    assert_eq!(state.measure.value, Some(1.0));
    assert_eq!(state.goal.target, None);
    let text = render(&app, 80, 24).join("\n");
    assert!(text.contains("Anticipated!") && text.contains("no baseline"), "{text}");
    // The notice is short.
    app.on_tick_at(Duration::from_secs(2), at + Duration::from_secs(2)).unwrap();
    let text = render(&app, 80, 24).join("\n");
    assert!(!text.contains("Anticipated!"), "{text}");

    finish(&mut app, started);
    let text = render(&app, 80, 24).join("\n");
    for expected in [
        "Anticipation share",
        "No baseline yet",
        "Anticipation 1 of 1 first errors",
        "judged on the check step",
    ] {
        assert!(text.contains(expected), "missing {expected:?}:\n{text}");
    }
}

#[test]
fn pacer_meter_shows_the_gap_and_the_accuracy_floor() {
    let mut app = menu_app(Lang::En, vec![prescription_step("speed.pacer", 1)]);
    let started = start_step(&mut app, Instant::now());
    let target = app.config.pacer.target_wpm.expect("a pacer target");
    type_keys(&mut app, started, 40, |_| 120, |_| false);
    let state = app.menu_live_goal.clone().unwrap();
    assert_eq!(state.goal.target, Some(target));
    let text = render(&app, 80, 24).join("\n");
    assert!(text.contains("Speed") && text.contains("pacer"), "{text}");
    assert!(text.contains("95%+"), "the accuracy floor line:\n{text}");

    finish(&mut app, started);
    let record = app.result_record.clone().unwrap();
    let finished = app.menu_live_goal.as_ref().unwrap().finished.clone().unwrap();
    let expected = record.header.accuracy >= 0.95 && record.header.net_wpm >= target;
    assert_eq!(finished.met, Some(expected));
    let text = render(&app, 80, 24).join("\n");
    assert!(
        text.contains("Speed (WPM)") && text.contains("Accuracy") && text.contains("/ 95%+"),
        "{text}"
    );
    assert!(text.contains(if expected { "Target reached" } else { "Target not reached" }), "{text}");
}

#[test]
fn endurance_measures_the_first_half_then_shows_the_slowdown() {
    let mut app = menu_app(Lang::Ja, vec![prescription_step("endurance.split", 1)]);
    let started = start_step(&mut app, Instant::now());
    let planned = app.config.session.duration_seconds;
    type_keys(&mut app, started, 40, |_| 150, |_| false);
    let text = compact(&render(&app, 80, 24));
    assert!(
        text.contains("前半:基準を測定中") && text.contains("15%未満") && text.contains("後半まで"),
        "{text}"
    );

    let second = started + Duration::from_secs(planned / 2 + 1);
    type_keys(&mut app, second, 40, |_| 190, |_| false);
    let live = app.menu_live_goal.as_ref().unwrap().measure.value.expect("slowdown");
    let text = compact(&render(&app, 80, 24));
    assert!(text.contains("後半の遅れ") && text.contains("前半IKI"), "{text}");

    finish(&mut app, started);
    let record = app.result_record.clone().unwrap();
    let judged = rs_typing_trainer::practice::menu::current_metric_value(MenuMetric::EnduranceSecondHalfSlowdown, &record).unwrap();
    assert!((live - judged).abs() < 1e-12, "live {live} judged {judged}");
    let text = compact(&render(&app, 80, 24));
    assert!(text.contains("後半の遅れが15%以上"), "190 ms against 150 ms is a 27% slowdown:\n{text}");
    assert!(text.contains("IKI前半150ms→後半190ms"), "{text}");
}

#[test]
fn three_sprints_show_the_runs_and_the_fastest_one() {
    let mut app = menu_app(Lang::En, vec![fixed_step("fixed.finish")]);
    let mut now = Instant::now();
    for (run, gap) in [(1, 220_u64), (2, 120)] {
        let started = start_step(&mut app, now);
        let state = app.menu_live_goal.clone().unwrap();
        assert_eq!(state.goal.run_number(), run);
        let text = render(&app, 80, 24).join("\n");
        assert!(text.contains(&format!("Sprint {run}/3")), "{text}");
        if run == 1 {
            assert!(text.contains("1st: set the mark"), "{text}");
        } else {
            assert!(text.contains("best "), "{text}");
        }
        let mut at = started;
        for _ in 0..40 {
            if app.screen != Screen::Playing {
                break;
            }
            at = type_keys(&mut app, at, 10, |_| gap, |_| false);
            if run == 2 && app.screen == Screen::Playing && app.menu_live_goal.as_ref().unwrap().record_flash(app.game.as_ref().unwrap().elapsed) {
                let text = render(&app, 80, 24).join("\n");
                assert!(text.contains("Fastest pace!"), "{text}");
            }
        }
        assert_eq!(app.screen, Screen::Result, "the sprint ends after its words");
        now = at + Duration::from_secs(1);
        if run == 2 {
            let text = render(&app, 80, 24).join("\n");
            for expected in [
                "Fastest sprint (WPM)",
                "New fastest sprint!",
                "Run 1",
                "Run 2",
                "★ best",
                "2 of 3 sprints",
            ] {
                assert!(text.contains(expected), "missing {expected:?}:\n{text}");
            }
        } else {
            // Return to the menu so the next loop iteration starts the same
            // menu step afresh. Result-screen retry itself is covered by the
            // briefing-flow test and deliberately skips the briefing.
            app.screen = Screen::Menu;
        }
    }
}

#[test]
fn every_goal_fits_80_by_24_while_playing_and_on_the_result() {
    let steps = [
        prescription_step("accuracy.gate", 3),
        prescription_step("accuracy.anticipation", 2),
        prescription_step("rhythm.metronome", 2),
        prescription_step("speed.pacer", 2),
        prescription_step("endurance.split", 2),
        fixed_step("fixed.finish"),
    ];
    for lang in [Lang::En, Lang::Ja] {
        for step in &steps {
            let mut app = menu_app(lang, vec![step.clone()]);
            let started = start_step(&mut app, Instant::now());
            type_keys(&mut app, started, 30, uneven, |index| index % 11 == 5);
            if app.screen == Screen::Playing {
                let rows = render(&app, 80, 24);
                let text = rows.join("\n");
                assert!(
                    !text.contains("Combo:"),
                    "{}: the note replaces the sub stats:\n{text}",
                    step.prescription_id
                );
                // The prompt keeps its rows and the footer its line.
                assert!(rows[22].contains("[ESC]"), "{}:\n{text}", step.prescription_id);
                finish(&mut app, started);
            }
            let rows = render(&app, 80, 24);
            let text = rows.join("\n");
            let marker = match lang {
                Lang::En => "Graduation condition",
                Lang::Ja => "卒業の条件",
            };
            assert!(
                compact(&rows).contains(&marker.replace(' ', "")) || step.prescription_id == "fixed.finish",
                "{} {lang:?}: the menu result line stays:\n{text}",
                step.prescription_id
            );
            assert!(
                rows[1].contains(&step.title) || compact(&rows[..3]).contains(&step.title.replace(' ', "")),
                "{}:\n{text}",
                step.prescription_id
            );
        }
    }
}

/// A saved check-step session of an earlier day whose within-word intervals
/// spread by `spread` ms around 150 ms.
fn earlier_check(app: &App, day: chrono::NaiveDate, spread: u64) -> rs_typing_trainer::record::SessionRecord {
    use rs_typing_trainer::engine::session::{ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};
    use rs_typing_trainer::record::db;

    let db_path = app.config.storage.db_path.clone().unwrap();
    let text = "the quick brown fox jumps over the lazy dog and runs back home again";
    let mut time_us = 100_000_u64;
    let mut keystrokes = Vec::new();
    for (index, character) in text.chars().enumerate() {
        if index > 0 {
            time_us += 150_000 + [0, spread, 0, spread / 2][index % 4] * 1_000;
        }
        keystrokes.push(KeystrokeLog {
            seq: index as u32,
            t_us: time_us,
            release_us: None,
            item_idx: 0,
            pos: index as u32,
            kind: KeystrokeKind::Char,
            typed: Some(character.to_string()),
            expected: Some(character.to_string()),
            correct: true,
            code: None,
        });
    }
    let log = SessionLog {
        duration_us: time_us,
        error_mode: Default::default(),
        items: vec![ItemLog {
            idx: 0,
            text: text.to_owned(),
            reading: None,
            kind: None,
            shown_us: 0,
            done_us: Some(time_us),
            outcome: ItemOutcome::Completed,
        }],
        keystrokes,
    };
    let mut config = app.config.clone();
    config.mode.kind = rs_typing_trainer::config::ModeKind::Sentence;
    let started = chrono::TimeZone::from_utc_datetime(&chrono::Utc, &day.and_hms_opt(9, 0, 0).unwrap());
    let mut record = rs_typing_trainer::record::SessionRecord::from_log(&config, log, started);
    record.header.id = Some(db::save(&db_path, &record).unwrap());
    let menu = DailyMenu {
        date: day,
        language: "en".to_owned(),
        catalog_version: "menu-v1".to_owned(),
        requested_minutes: 15,
        introductory: false,
        bench_recommended: false,
        steps: vec![fixed_step("fixed.check")],
    };
    let run = db::ensure_menu_run(&db_path, day, "qwerty", "en", "menu-v1", 15, &serde_json::to_string(&menu).unwrap())
        .unwrap()
        .id;
    let layout = app.practice_layout().clone();
    let context = JudgeContext {
        db_path: &db_path,
        home: &app.home_dir,
        config: &app.config,
        layout: &layout,
        as_of: day,
    };
    rs_typing_trainer::practice::menu::progress::record_step_completion(&context, run, 0, &record).unwrap();
    record
}

#[test]
fn metronome_line_is_the_own_median_of_earlier_check_steps() {
    let mut app = menu_app(Lang::En, vec![prescription_step("rhythm.metronome", 3)]);
    let records = [(3, 20), (2, 60), (1, 120)]
        .into_iter()
        .map(|(days, spread)| earlier_check(&app, date() - chrono::Duration::days(days), spread))
        .collect::<Vec<_>>();
    let db_path = app.config.storage.db_path.clone().unwrap();
    let layout = app.practice_layout().clone();
    let context = JudgeContext {
        db_path: &db_path,
        home: &app.home_dir,
        config: &app.config,
        layout: &layout,
        as_of: date(),
    };
    let mut values = records
        .iter()
        .map(|record| {
            session_metric_value(&context, MenuMetric::WithinWordIkiCv, record, "en")
                .unwrap()
                .unwrap()
        })
        .collect::<Vec<_>>();
    values.sort_by(f64::total_cmp);
    let median = values[1];
    assert!(values[0] < values[1] && values[1] < values[2], "{values:?}");

    let started = start_step(&mut app, Instant::now());
    let state = app.menu_live_goal.clone().unwrap();
    assert_eq!(state.goal.target, Some(median), "the own median of the three earlier checks");
    type_keys(&mut app, started, 30, |_| 150, |_| false);
    let text = render(&app, 80, 24).join("\n");
    assert!(text.contains(&format!("median {median:.2}")), "{text}");
    finish(&mut app, started);
    let text = render(&app, 80, 24).join("\n");
    assert!(text.contains(&format!("your median {median:.2}")), "{text}");
}
