//! MYTASK-3641: every row of the individual-training table except «normal»
//! has a live goal, in today's menu and on the table alike. The meter and the
//! note fit 80×24 in both languages, the last live value is the value that is
//! recorded and judged, and the result of a table cell compares with the
//! cell's previous session and its personal best.

use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use rs_typing_trainer::app::{App, Screen};
use rs_typing_trainer::config::{Config, DrillCell, DrillSkill, PromptUnit};
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::practice::menu::live_goal::{GoalKind, GoalOrigin, GoalSetup, LiveGoal};
use rs_typing_trainer::practice::menu::progress::{JudgeContext, session_metric_value};
use rs_typing_trainer::practice::menu::{DailyMenu, MenuMetric, MenuRole, MenuStep, SpellingTarget, bundled_catalog, current_metric_value};
use rs_typing_trainer::record::{SessionRecord, db};
use rs_typing_trainer::ui;

use crate::common::{TestApp, test_app_at};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn release(code: KeyCode) -> KeyEvent {
    KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Release)
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

/// The cell of each row these tests play.
fn cell_of(skill: DrillSkill) -> DrillCell {
    let unit = match skill {
        DrillSkill::Preview | DrillSkill::Endurance => PromptUnit::Sentence,
        DrillSkill::Speed => PromptUnit::Phrase,
        _ => PromptUnit::Word,
    };
    DrillCell::new(skill, unit)
}

fn practice_lang(skill: DrillSkill) -> Lang {
    if skill == DrillSkill::Spelling { Lang::Ja } else { Lang::En }
}

/// The rows with a goal and the menu prescription of each.
fn rows() -> Vec<(DrillSkill, &'static str)> {
    vec![
        (DrillSkill::Accuracy, "accuracy.gate"),
        (DrillSkill::Speed, "speed.pacer"),
        (DrillSkill::Recovery, "recovery.retry"),
        (DrillSkill::Blind, "blind.fade"),
        (DrillSkill::Anticipation, "accuracy.anticipation"),
        (DrillSkill::Preview, "preview.wide"),
        (DrillSkill::Overlap, "overlap.burst"),
        (DrillSkill::Spelling, "japanese.spelling"),
        (DrillSkill::Weak, "transition.adaptive"),
        (DrillSkill::Rhythm, "rhythm.metronome"),
        (DrillSkill::Burst, "fixed.finish"),
        (DrillSkill::Endurance, "endurance.split"),
        (DrillSkill::Unlock, "reach.unlock"),
    ]
}

/// The meter's label and the result headline's label of each goal.
fn labels(kind: GoalKind, lang: Lang) -> (&'static str, &'static str) {
    match (kind, lang) {
        (GoalKind::AccuracyGate, Lang::En) => ("Accuracy", "Accuracy"),
        (GoalKind::AccuracyGate, Lang::Ja) => ("正確さ", "正確さ"),
        (GoalKind::Anticipation, Lang::En) => ("Anticipation", "Anticipation share"),
        (GoalKind::Anticipation, Lang::Ja) => ("先走り", "先走りの割合"),
        (GoalKind::Metronome, Lang::En) => ("In-word CV", "In-word IKI CV"),
        (GoalKind::Metronome, Lang::Ja) => ("語中CV", "語中 IKI の CV"),
        (GoalKind::Pacer, Lang::En) => ("Speed", "Speed (WPM)"),
        (GoalKind::Pacer, Lang::Ja) => ("速さ", "速さ（WPM）"),
        (GoalKind::EnduranceSplit, Lang::En) => ("Slowdown", "Second-half slowdown"),
        (GoalKind::EnduranceSplit, Lang::Ja) => ("後半の遅れ", "後半の遅れ"),
        (GoalKind::Sprint, Lang::En) => ("Sprint", "Fastest sprint (WPM)"),
        (GoalKind::Sprint, Lang::Ja) => ("本目", "最速の 1 本（WPM）"),
        (GoalKind::Recovery, Lang::En) => ("After-error excess", "Excess after errors (ms)"),
        (GoalKind::Recovery, Lang::Ja) => ("誤打後の余分", "誤打後の余分な時間（ms）"),
        (GoalKind::Blind, Lang::En) => ("Accuracy", "Accuracy (no-look)"),
        (GoalKind::Blind, Lang::Ja) => ("正確さ", "正確さ（見ないで）"),
        (GoalKind::Preview, Lang::En) => ("Word onset", "Word onset (start/inside)"),
        (GoalKind::Preview, Lang::Ja) => ("語頭の遅れ", "語頭の遅れ（語頭/語中）"),
        (GoalKind::Overlap, Lang::En) => ("Rollover", "Rollover rate"),
        (GoalKind::Overlap, Lang::Ja) => ("ロールオーバー", "ロールオーバー率"),
        (GoalKind::Spelling, Lang::En) => ("Spelling use", "Suggested spelling use"),
        (GoalKind::Spelling, Lang::Ja) => ("綴りの使用率", "勧めた綴りの使用率"),
        (GoalKind::WeakTransition, Lang::En) => ("Targets", "Target transitions (ms)"),
        (GoalKind::WeakTransition, Lang::Ja) => ("対象遷移", "対象遷移の平均（ms）"),
        (GoalKind::Reach, Lang::En) => ("Far/home IKI", "Remote/home-row IKI ratio"),
        (GoalKind::Reach, Lang::Ja) => ("遠い段/ホーム", "遠い段/ホーム段の IKI 比"),
    }
}

/// Title → table, then Enter on `cell` in `practice` (through the key
/// picker when the cell uses it); returns the time the session started.
fn start_cell(app: &mut App, cell: DrillCell, practice: Lang, now: Instant) -> Instant {
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
    crate::common::accept_briefing(app, now);
    assert!(matches!(app.screen, Screen::Countdown { .. }), "{cell:?}: {:?}", app.screen);
    let started = now + Duration::from_secs(4);
    app.on_tick_at_date(Duration::from_secs(3), started, date()).unwrap();
    assert_eq!(app.screen, Screen::Playing, "{cell:?}");
    started
}

fn prescription_step(id: &str, language: Lang) -> MenuStep {
    let catalog = bundled_catalog().unwrap();
    if let Some(fixed) = catalog.fixed_steps.iter().find(|step| step.id == id) {
        return MenuStep {
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
        };
    }
    let prescription = catalog.prescription(id).unwrap();
    let row = &prescription.levels[0];
    let mut settings = row.settings.clone();
    match id {
        "japanese.spelling" => {
            let mut table = toml::map::Map::new();
            table.insert("kana".to_owned(), toml::Value::String("し".to_owned()));
            table.insert("current".to_owned(), toml::Value::String("si".to_owned()));
            table.insert("candidate".to_owned(), toml::Value::String("shi".to_owned()));
            settings.insert("target.spelling".to_owned(), toml::Value::Table(table));
        }
        "transition.adaptive" => {
            settings.insert(
                "target.transitions".to_owned(),
                toml::Value::Array(
                    ["th", "he", "in", "er", "an", "re"]
                        .iter()
                        .map(|pair| toml::Value::String((*pair).to_owned()))
                        .collect(),
                ),
            );
        }
        _ => {}
    }
    MenuStep {
        role: MenuRole::PrimaryA,
        language,
        prescription_id: id.to_owned(),
        title: prescription.title.text(Lang::En).to_owned(),
        level: Some(1),
        mode: row.mode.clone(),
        minutes: row.minutes,
        reason: "-".to_owned(),
        focus: row.focus.text(Lang::En).to_owned(),
        advance: row.advance.note.text(Lang::En).to_owned(),
        advance_condition: Some(row.advance.clone()),
        evidence: None,
        evidence_note: None,
        settings,
    }
}

/// An app whose menu of today is exactly `steps`; the first step is started
/// and the session start time returned.
fn start_menu(app: &mut App, steps: Vec<MenuStep>, now: Instant) -> Instant {
    app.handle_key_at(key(KeyCode::Char('m')), now);
    assert_eq!(app.screen, Screen::Menu);
    let language = steps[0].language;
    app.daily_menu = Some(DailyMenu {
        date: date(),
        language: if language == Lang::Ja { "ja" } else { "en" }.to_owned(),
        catalog_version: "menu-v1".to_owned(),
        requested_minutes: 15,
        introductory: false,
        bench_recommended: false,
        steps,
    });
    app.menu_selected = 0;
    app.handle_key_at(key(KeyCode::Enter), now);
    assert!(matches!(app.screen, Screen::Briefing), "{:?}", app.screen);
    app.handle_key_at(key(KeyCode::Enter), now + Duration::from_millis(10));
    let started = now + Duration::from_secs(4);
    app.on_tick_at_date(Duration::from_secs(3), started, date()).unwrap();
    assert_eq!(app.screen, Screen::Playing);
    started
}

/// Type `count` keystrokes from `at`, `gap(i)` apart; `wrong(i)` presses a
/// key that is not the expected one. Every other key is released after the
/// next one is pressed (an overlap), the others before it. Returns the time
/// after the last key.
fn type_keys(app: &mut App, mut at: Instant, count: usize, gap: impl Fn(usize) -> u64, wrong: impl Fn(usize) -> bool) -> Instant {
    let mut previous: Option<char> = None;
    for index in 0..count {
        app.on_tick_at_date(Duration::from_millis(1), at, date()).unwrap();
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
        let overlap = index % 2 == 0;
        if let Some(last) = previous.filter(|_| !overlap) {
            app.handle_key_at(release(KeyCode::Char(last)), at - Duration::from_millis(5));
        }
        app.handle_key_at(key(KeyCode::Char(pressed)), at);
        if let Some(last) = previous.filter(|_| overlap) {
            app.handle_key_at(release(KeyCode::Char(last)), at + Duration::from_millis(5));
        }
        previous = Some(pressed);
        at += Duration::from_millis(gap(index));
    }
    at
}

fn uneven(index: usize) -> u64 {
    [110, 190, 140, 260, 120, 170][index % 6]
}

/// End the session by time (after the planned length of its goal).
fn finish(app: &mut App, started: Instant) {
    let planned = app
        .menu_live_goal
        .as_ref()
        .map_or(Duration::from_secs(app.config.session.duration_seconds), |state| {
            Duration::from_micros(state.goal.planned_us)
        })
        .max(Duration::from_secs(app.config.session.duration_seconds));
    if app.screen == Screen::Playing {
        app.on_tick_at_date(Duration::from_secs(1), started + planned + Duration::from_secs(1), date())
            .unwrap();
    }
    assert_eq!(app.screen, Screen::Result);
}

fn saved_record(app: &App) -> SessionRecord {
    let db_path = app.config.storage.db_path.clone().unwrap();
    db::load_record(&db_path, app.last_save_id.expect("the session is saved"))
        .unwrap()
        .unwrap()
}

/// The value the session was recorded and judged with, through the
/// judgment's own functions (`None` when the row has none per session).
fn recorded_value(app: &App, kind: GoalKind, record: &SessionRecord, language: &str) -> Option<f64> {
    let db_path = app.config.storage.db_path.clone().unwrap();
    let layout = app.practice_layout().clone();
    let context = JudgeContext {
        db_path: &db_path,
        home: &app.home_dir,
        config: &app.config,
        layout: &layout,
        as_of: date(),
    };
    let goal = &app.menu_live_goal.as_ref().unwrap().goal;
    match kind {
        GoalKind::AccuracyGate | GoalKind::Blind => Some(record.header.accuracy),
        GoalKind::Pacer | GoalKind::Sprint => Some(record.header.net_wpm),
        GoalKind::EnduranceSplit => current_metric_value(MenuMetric::EnduranceSecondHalfSlowdown, record),
        GoalKind::Spelling => goal.spelling.as_ref().and_then(|target| {
            rs_typing_trainer::analysis::japanese::spelling_usage(std::slice::from_ref(record), &target.kana, &target.candidate).rate()
        }),
        // Not recorded per session (R3 judges it): the adaptive result's
        // session medians of the targets, as the result screen lists them.
        GoalKind::WeakTransition => goal.measure(record, &layout).value,
        _ => session_metric_value(&context, kind.own_median_metric().unwrap(), record, language).unwrap(),
    }
}

fn assert_same(live: Option<f64>, recorded: Option<f64>, what: &str) {
    match (live, recorded) {
        (Some(live), Some(recorded)) => assert!((live - recorded).abs() < 1e-9, "{what}: live {live} recorded {recorded}"),
        (live, recorded) => assert_eq!(live, recorded, "{what}"),
    }
}

/// Play one session (a table cell or a menu step), check the playing and
/// result screens at 80×24 and the live value against the recorded one.
fn play_and_check(app: &mut TestApp, kind: GoalKind, started: Instant, lang: Lang, language: &str, menu: bool) {
    let what = format!("{kind:?} {lang:?} menu={menu}");
    let state = app.menu_live_goal.clone().unwrap_or_else(|| panic!("{what}: a live goal"));
    assert_eq!(state.goal.kind, kind, "{what}");
    assert_eq!(matches!(state.goal.origin, GoalOrigin::Menu), menu, "{what}");
    type_keys(app, started, 70, uneven, |index| index % 9 == 4);
    let (meter, headline) = labels(kind, lang);
    if app.screen == Screen::Playing {
        let rows = render(app, 80, 24);
        let text = compact(&rows);
        assert!(text.contains(&meter.replace(' ', "")), "{what}: meter {meter:?}:\n{}", rows.join("\n"));
        assert!(
            !rows.join("\n").contains("Combo:"),
            "{what}: the note replaces the sub stats:\n{}",
            rows.join("\n")
        );
        assert!(rows[22].contains("[ESC]"), "{what}: the footer stays:\n{}", rows.join("\n"));
        if kind == GoalKind::Blind {
            assert!(
                text.contains(if lang == Lang::Ja { "キーボード表示" } else { "Keyboard:" }),
                "{what}:\n{text}"
            );
        }
    }
    let live = app.menu_live_goal.as_ref().unwrap().measure.value;
    finish(app, started);
    let record = saved_record(app);
    let recorded = recorded_value(app, kind, &record, language);
    let finished = app.menu_live_goal.as_ref().unwrap().finished.clone().unwrap();
    // Recovery is measured against a player model whose decay depends on
    // `as_of`, so the ticks above advance time on `date()` too: a wall-clock
    // tick would move the live goal's date away from the judged one
    // (MYTASK-3662).
    assert_same(finished.value, recorded, &format!("{what}: finished"));
    // The speed rows show WPM over the time so far, so the value at the
    // last keystroke differs from the value over the whole session; every
    // other row's last live value is the recorded one.
    if !matches!(kind, GoalKind::Pacer | GoalKind::Sprint) {
        assert_same(live, recorded, &format!("{what}: last live value"));
    }
    // A burst-mode step (`overlap.burst` ①) is recorded after its third
    // sprint, so one sprint leaves no menu_steps row to compare.
    let recorded_step = menu && app.daily_menu.as_ref().is_some_and(|menu| menu.steps[0].mode != "burst");
    if recorded_step
        && let Some(metric) = kind.own_median_metric().or(match kind {
            GoalKind::Spelling => Some(MenuMetric::SpellingUsageRate),
            GoalKind::EnduranceSplit => Some(MenuMetric::EnduranceSecondHalfSlowdown),
            _ => None,
        })
    {
        let db_path = app.config.storage.db_path.clone().unwrap();
        let run_id = app.menu_run_id.unwrap_or_else(|| panic!("{what}: a menu run"));
        let session_id = app.last_save_id.unwrap();
        let steps = db::load_menu_steps(&db_path, run_id).unwrap();
        let row = steps
            .iter()
            .find(|row| row.session_id == session_id)
            .unwrap_or_else(|| panic!("{what}: the step is recorded: {steps:?}"));
        assert_eq!(row.metric.as_deref(), Some(metric.as_str()), "{what}");
        assert_same(live, row.value, &format!("{what}: menu_steps.value"));
    }
    let rows = render(app, 80, 24);
    let text = compact(&rows);
    assert!(
        text.contains(&headline.replace(' ', "")),
        "{what}: headline {headline:?}:\n{}",
        rows.join("\n")
    );
    if !menu {
        let first = if lang == Lang::Ja {
            "この升目の初回です"
        } else {
            "Firstsessionofthiscell"
        };
        assert!(
            text.contains(first) || finished.value.is_none(),
            "{what}: no earlier session:\n{}",
            rows.join("\n")
        );
    }
}

#[test]
fn every_row_has_a_goal_on_the_table_and_in_the_menu_at_80_by_24() {
    for (skill, id) in rows() {
        let kind = GoalKind::for_skill(skill).unwrap();
        for lang in [Lang::En, Lang::Ja] {
            let practice = practice_lang(skill);
            let language = if practice == Lang::Ja { "ja" } else { "en" };

            let mut app = test_app_at(config(lang), date());
            app.release_events_supported = true;
            let started = start_cell(&mut app, cell_of(skill), practice, Instant::now());
            play_and_check(&mut app, kind, started, lang, language, false);

            let mut app = test_app_at(config(lang), date());
            app.release_events_supported = true;
            let started = start_menu(&mut app, vec![prescription_step(id, practice)], Instant::now());
            play_and_check(&mut app, kind, started, lang, language, true);
        }
    }
}

#[test]
fn the_normal_row_keeps_the_ordinary_screens() {
    for unit in [PromptUnit::Word, PromptUnit::Sentence] {
        let mut app = test_app_at(config(Lang::En), date());
        let started = start_cell(&mut app, DrillCell::new(DrillSkill::Normal, unit), Lang::En, Instant::now());
        assert!(app.menu_live_goal.is_none());
        type_keys(&mut app, started, 20, uneven, |_| false);
        let playing = render(&app, 80, 24).join("\n");
        assert!(playing.contains("Combo"), "{playing}");
        finish(&mut app, started);
        let result = compact(&render(&app, 80, 24));
        assert!(!result.contains("Firstsessionofthiscell"), "{result}");
    }
}

#[test]
fn the_rollover_row_tells_when_the_terminal_reports_no_releases() {
    let mut app = test_app_at(config(Lang::Ja), date());
    let started = start_cell(&mut app, cell_of(DrillSkill::Overlap), Lang::En, Instant::now());
    type_keys(&mut app, started, 20, uneven, |_| false);
    let text = compact(&render(&app, 80, 24));
    assert!(text.contains("キーを離した時刻が取れない端末です"), "{text}");
    assert_eq!(app.menu_live_goal.as_ref().unwrap().measure.value, None);
}

/// Play one table session of `cell` typing `keys` keys with an error at the
/// indices `wrong` picks; returns the finished value.
fn play_cell(app: &mut TestApp, cell: DrillCell, now: Instant, keys: usize, gap: impl Fn(usize) -> u64, wrong: impl Fn(usize) -> bool) -> f64 {
    let started = if app.screen == Screen::Result {
        // Enter on a cell's result plays the same cell again.
        app.handle_key_at(key(KeyCode::Enter), now);
        if app.screen == Screen::KeyPicker {
            app.handle_key_at(key(KeyCode::Enter), now);
        }
        assert!(matches!(app.screen, Screen::Countdown { .. }), "{:?}", app.screen);
        let started = now + Duration::from_secs(4);
        app.on_tick_at_date(Duration::from_secs(3), started, date()).unwrap();
        assert_eq!(app.screen, Screen::Playing);
        started
    } else {
        start_cell(app, cell, Lang::En, now)
    };
    type_keys(app, started, keys, gap, wrong);
    finish(app, started);
    app.menu_live_goal.as_ref().unwrap().finished.as_ref().unwrap().value.unwrap()
}

#[test]
fn a_cell_result_compares_with_its_previous_session_and_its_best() {
    let cell = DrillCell::new(DrillSkill::Blind, PromptUnit::Word);
    for lang in [Lang::En, Lang::Ja] {
        let mut app = test_app_at(config(lang), date());
        let mut now = Instant::now();
        // First session: nothing to compare.
        let first = play_cell(&mut app, cell, now, 40, |_| 150, |index| index % 10 == 3);
        let text = compact(&render(&app, 80, 24));
        let ja = lang == Lang::Ja;
        assert!(text.contains(if ja { "この升目の初回です" } else { "Firstsessionofthiscell" }), "{text}");
        assert!(text.contains(if ja { "基準なし" } else { "Nobaselineyet" }), "{text}");

        // Second session, better: ▲ against the previous one, a new best.
        now += Duration::from_secs(600);
        let second = play_cell(&mut app, cell, now, 40, |_| 150, |_| false);
        assert!(second > first);
        let state = app.menu_live_goal.clone().unwrap();
        assert_eq!(state.goal.history, vec![first]);
        assert_eq!(state.goal.target, Some(first), "the line is the cell's median");
        assert_eq!(state.goal.baseline_runs, Some(1));
        let text = compact(&render(&app, 80, 24));
        let previous = if ja { "前回" } else { "Previous" };
        assert!(text.contains(&format!("{previous}{:.1}%▲", first * 100.0)), "{text}");
        assert!(text.contains(if ja { "★自己ベスト更新" } else { "★Newpersonalbest" }), "{text}");

        // Third session, worse: ▼ against both.
        now += Duration::from_secs(600);
        let third = play_cell(&mut app, cell, now, 40, |_| 150, |index| index % 5 == 1);
        assert!(third < first);
        let state = app.menu_live_goal.clone().unwrap();
        assert_eq!(state.goal.history, vec![first, second]);
        assert_eq!(state.goal.target, Some((first + second) / 2.0));
        let rows = render(&app, 80, 24);
        let text = compact(&rows);
        assert!(text.contains(&format!("{previous}{:.1}%▼", second * 100.0)), "{text}");
        let best = if ja { "自己ベスト" } else { "Best" };
        assert!(text.contains(&format!("{best}{:.1}%▼", second * 100.0)), "{}", rows.join("\n"));
        assert!(text.contains(if ja { "直近2回" } else { "last2ofthiscell" }), "{text}");
    }
}

#[test]
fn a_lower_is_better_cell_marks_a_smaller_value_as_better() {
    // The rhythm row: an even rhythm (low CV) after an uneven one.
    let cell = DrillCell::new(DrillSkill::Rhythm, PromptUnit::Word);
    let mut app = test_app_at(config(Lang::En), date());
    let now = Instant::now();
    let uneven_cv = play_cell(&mut app, cell, now, 60, uneven, |_| false);
    let even_cv = play_cell(
        &mut app,
        cell,
        now + Duration::from_secs(600),
        60,
        |index| 150 + (index % 2) as u64 * 10,
        |_| false,
    );
    assert!(even_cv < uneven_cv, "{even_cv} < {uneven_cv}");
    let text = compact(&render(&app, 80, 24));
    assert!(text.contains(&format!("Previous{uneven_cv:.2}▲")), "{text}");
    assert!(text.contains("better"), "{text}");
    assert!(text.contains("★Newpersonalbest"), "{text}");
}

#[test]
fn a_cell_without_its_own_sessions_takes_the_line_of_the_check_steps() {
    // The anticipation row's line falls back to the check steps' median of
    // `anticipation-rate`, as a menu step's line is.
    let mut app = test_app_at(config(Lang::En), date());
    let db_path = app.config.storage.db_path.clone().unwrap();
    let layout = app.practice_layout().clone();
    let context = JudgeContext {
        db_path: &db_path,
        home: &app.home_dir,
        config: &app.config,
        layout: &layout,
        as_of: date(),
    };
    let expected =
        rs_typing_trainer::practice::menu::progress::own_median_for_metric(&context, MenuMetric::AnticipationRate, "en", i64::MAX).unwrap();
    let started = start_cell(&mut app, cell_of(DrillSkill::Anticipation), Lang::En, Instant::now());
    let state = app.menu_live_goal.clone().unwrap();
    assert_eq!(state.goal.target, expected);
    assert_eq!(state.goal.baseline_runs, None);
    type_keys(&mut app, started, 10, uneven, |_| false);
}

#[test]
fn spelling_usage_counts_the_kana_typed_with_the_suggested_spelling() {
    use rs_typing_trainer::engine::session::{ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};

    let words = [("shikashi", "しかし"), ("sika", "しか"), ("tsuchi", "つち")];
    let mut items = Vec::new();
    let mut keystrokes = Vec::new();
    let mut time_us = 100_000;
    for (index, (typed, reading)) in words.iter().enumerate() {
        for (pos, character) in typed.chars().enumerate() {
            keystrokes.push(KeystrokeLog {
                seq: keystrokes.len() as u32,
                t_us: time_us,
                release_us: None,
                item_idx: index as u32,
                pos: pos as u32,
                kind: KeystrokeKind::Char,
                typed: Some(character.to_string()),
                expected: Some(character.to_string()),
                correct: true,
                code: None,
            });
            time_us += 150_000;
        }
        items.push(ItemLog {
            idx: index as u32,
            text: (*reading).to_owned(),
            reading: Some((*reading).to_owned()),
            kind: None,
            shown_us: 0,
            done_us: Some(time_us),
            outcome: ItemOutcome::Completed,
        });
    }
    let log = SessionLog {
        duration_us: time_us,
        error_mode: Default::default(),
        items,
        keystrokes,
    };
    let mut config = Config::default();
    config.mode.kind = rs_typing_trainer::config::ModeKind::Romaji;
    let record = SessionRecord::from_log(&config, log, chrono::Utc::now());
    let usage = rs_typing_trainer::analysis::japanese::spelling_usage(std::slice::from_ref(&record), "し", "shi");
    assert_eq!((usage.used, usage.total), (2, 3), "shi, shi and si");
    assert_eq!(usage.rate(), Some(2.0 / 3.0));
    assert_eq!(
        rs_typing_trainer::analysis::japanese::spelling_usage(std::slice::from_ref(&record), "ぬ", "nu").rate(),
        None
    );

    // The judgment reads the same function with the step's suggestion.
    let step = prescription_step("japanese.spelling", Lang::Ja);
    let value = rs_typing_trainer::practice::menu::progress::spelling_usage_value(&record, &step);
    assert_eq!(value, Some(2.0 / 3.0));
    // And the live goal of the row too.
    let goal = LiveGoal::for_cell(
        DrillCell::new(DrillSkill::Spelling, PromptUnit::Word),
        GoalSetup {
            spelling: Some(SpellingTarget {
                kana: "し".to_owned(),
                current: "si".to_owned(),
                candidate: "shi".to_owned(),
            }),
            ..GoalSetup::default()
        },
    )
    .unwrap();
    let layout = rs_typing_trainer::layout::builtin_layout("qwerty").unwrap();
    let measure = goal.measure(&record, &layout);
    assert_eq!(measure.value, value);
    assert_eq!(goal.target, Some(0.80));
    assert_eq!(measure.met, Some(false));
}

#[test]
fn the_goal_card_draws_the_value_in_the_tone_colour() {
    use ratatui::layout::Size;
    use ratatui::style::Color;
    use rs_typing_trainer::graphics::CellPixels;
    use rs_typing_trainer::practice::menu::live_goal::Tone;
    use rs_typing_trainer::ui::live_goal::{GoalReport, goal_card_spec};
    use rs_typing_trainer::ui::result_image::{ResultImageColors, render_metric_card};

    let colors = ResultImageColors {
        foreground: Color::White,
        muted: Color::Gray,
        correct: Color::Rgb(20, 220, 90),
        mistype: Color::Rgb(240, 60, 60),
        accent: Color::Rgb(40, 180, 220),
        warning: Color::Rgb(240, 200, 40),
        panel: Color::Rgb(16, 20, 28),
    };
    let font = rs_typing_trainer::ui::bigtext_image::resolve_big_font("biz-ud-gothic", std::path::Path::new("/unused"));
    for (tone, expected) in [
        (Tone::Good, [20, 220, 90, 255]),
        (Tone::Bad, [240, 60, 60, 255]),
        (Tone::Neutral, [40, 180, 220, 255]),
    ] {
        let report = GoalReport {
            title: " Recovery × Words ".to_owned(),
            label: "Excess after errors (ms)".to_owned(),
            value: "+85".to_owned(),
            tone,
            details: Vec::new(),
            chart: Vec::new(),
            recent: vec![120.0, 95.0, 85.0],
        };
        let spec = goal_card_spec(&report, Lang::En, Size::new(48, 10), colors);
        assert_eq!(spec.label, report.label);
        assert_eq!(spec.value, "+85");
        assert_eq!((spec.previous_delta, spec.best_delta), (None, None));
        let image = render_metric_card(&font, CellPixels::new(9, 20), &spec).unwrap();
        assert_eq!((image.width(), image.height()), (48 * 9, 10 * 20));
        assert!(image.pixels().any(|pixel| pixel.0 == expected), "{tone:?}: no pixel of the tone colour");
    }
}
