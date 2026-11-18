//! M6: the practice-menu summary screen (design §7.1).

use std::time::{Duration, Instant};

use chrono::{NaiveDate, TimeZone, Utc};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;
use rs_typing_trainer::app::Screen;
use rs_typing_trainer::config::{Config, ErrorMode};
use rs_typing_trainer::engine::session::{ItemLog, ItemOutcome, SessionLog};
use rs_typing_trainer::graphics::{CellPixels, GraphicsDetection, GraphicsMode, GraphicsReason};
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::practice::menu::{DailyMenu, MenuRole, MenuStep, PrescriptionJudgment, bundled_catalog};
use rs_typing_trainer::record::db::{self, MenuStepRecord, SkillSnapshotRecord};
use rs_typing_trainer::record::{SessionHeader, SessionRecord};
use rs_typing_trainer::ui;
use rs_typing_trainer::ui::menu_summary::{MAX_CHANGE_LINES, build_summary, change_lines};

use crate::common;
use common::test_app_at;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn render_buffer(app: &rs_typing_trainer::app::App, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    terminal.backend().buffer().clone()
}

fn rows(buffer: &Buffer) -> Vec<String> {
    (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect()
}

fn compact(text: &str) -> String {
    text.chars().filter(|character| !character.is_whitespace()).collect()
}

fn base_config() -> Config {
    let mut config = Config::default();
    config.session.duration_seconds = 17;
    config.session.result_input_delay_ms = 0;
    config.ui.animation_enabled = false;
    config
}

// ---------------------------------------------------------------------------
// Synthetic records
// ---------------------------------------------------------------------------

/// 2026-11-02 is a Monday.
fn day(offset: i64) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 2).unwrap() + chrono::Duration::days(offset)
}

fn session(date: NaiveDate, hour: u32, net_wpm: f64) -> SessionRecord {
    let start = Utc.from_utc_datetime(&date.and_hms_opt(hour, 0, 0).unwrap());
    SessionRecord {
        header: SessionHeader {
            id: None,
            started_at: start.to_rfc3339(),
            finished_at: (start + chrono::Duration::seconds(60)).to_rfc3339(),
            duration_ms: 60_000,
            mode: "sentence".to_owned(),
            mode_params: serde_json::json!({ "lang": "en" }).to_string(),
            bench_id: None,
            layout: "qwerty".to_owned(),
            input_layout: "same".to_owned(),
            geometry: "ansi".to_owned(),
            error_mode: "stop".to_owned(),
            release_events: false,
            app_version: "test".to_owned(),
            metrics_version: 2,
            net_wpm,
            raw_wpm: Some(net_wpm),
            accuracy: 0.97,
            iki_median_ms: 160.0,
            iki_cv: 0.2,
            cps: 5.0,
            consistency: 0.8,
            max_combo: 10,
            completed_words: 1,
            timeout_words: 0,
            mistypes_total: 0,
            legacy_payload: None,
        },
        log: SessionLog {
            duration_us: 60_000_000,
            error_mode: ErrorMode::Stop,
            items: vec![ItemLog {
                idx: 0,
                text: "a".to_owned(),
                reading: None,
                kind: None,
                shown_us: 0,
                done_us: Some(1),
                outcome: ItemOutcome::Completed,
            }],
            keystrokes: Vec::new(),
        },
    }
}

fn fixed_step(role: MenuRole, title: &str) -> MenuStep {
    let id = match role {
        MenuRole::Warmup => "fixed.warmup",
        MenuRole::Check => "fixed.check",
        _ => "fixed.finish",
    };
    MenuStep {
        role,
        language: Lang::En,
        prescription_id: id.to_owned(),
        title: title.to_owned(),
        level: None,
        mode: "sentence".to_owned(),
        minutes: 3,
        reason: "-".to_owned(),
        focus: String::new(),
        advance: String::new(),
        advance_condition: None,
        evidence: None,
        evidence_note: None,
        settings: Default::default(),
    }
}

fn prescription_step(id: &str, role: MenuRole, level: u8, lang: Lang) -> MenuStep {
    let prescription = bundled_catalog().unwrap().prescription(id).unwrap();
    let row = &prescription.levels[usize::from(level - 1)];
    MenuStep {
        role,
        language: Lang::En,
        prescription_id: id.to_owned(),
        title: prescription.title.text(lang).to_owned(),
        level: Some(level),
        mode: row.mode.clone(),
        minutes: row.minutes,
        reason: "-".to_owned(),
        focus: row.focus.text(lang).to_owned(),
        advance: row.advance.note.text(lang).to_owned(),
        advance_condition: Some(row.advance.clone()),
        evidence: None,
        evidence_note: None,
        settings: row.settings.clone(),
    }
}

/// Five steps: warm-up, accuracy gate ①, preview wide (`level`), check, finish.
fn plan(date: NaiveDate, preview_level: u8, lang: Lang) -> DailyMenu {
    DailyMenu {
        date,
        language: "en".to_owned(),
        catalog_version: "menu-v1".to_owned(),
        requested_minutes: 15,
        introductory: false,
        bench_recommended: false,
        steps: vec![
            fixed_step(MenuRole::Warmup, "words"),
            prescription_step("accuracy.gate", MenuRole::PrimaryA, 1, lang),
            prescription_step("preview.wide", MenuRole::PrimaryB, preview_level, lang),
            fixed_step(MenuRole::Check, "sentence"),
            fixed_step(MenuRole::Finish, "burst"),
        ],
    }
}

struct Seed {
    path: std::path::PathBuf,
    run: i64,
}

/// Store one run. `steps` lists (index, value, net WPM, advance_met,
/// level_after) of the played steps.
/// (index, value, net WPM, advance_met, level_after) of one played step.
type PlayedStep = (usize, Option<f64>, f64, Option<bool>, Option<u8>);

fn store_run(path: &std::path::Path, menu: &DailyMenu, steps: &[PlayedStep]) -> i64 {
    let run = db::ensure_menu_run(
        path,
        menu.date,
        "qwerty",
        "en",
        "menu-v1",
        menu.requested_minutes,
        &serde_json::to_string(menu).unwrap(),
    )
    .unwrap()
    .id;
    for (index, value, wpm, met, after) in steps {
        let step = &menu.steps[*index];
        let session_id = db::save(path, &session(menu.date, 8 + *index as u32, *wpm)).unwrap();
        db::record_menu_step(
            path,
            &MenuStepRecord {
                run_id: run,
                idx: index + 1,
                role: step.role.as_str().to_owned(),
                prescription_id: Some(step.prescription_id.clone()),
                level: step.level,
                session_id,
                metric: step.advance_condition.as_ref().map(|condition| condition.metric.as_str().to_owned()),
                value: *value,
                advance_met: *met,
                level_after: after.or(step.level),
            },
        )
        .unwrap();
    }
    run
}

fn snapshot(run: i64, date: NaiveDate, component: &str, value: f64) -> SkillSnapshotRecord {
    SkillSnapshotRecord {
        run_id: run,
        date,
        lang: "en".to_owned(),
        component: component.to_owned(),
        raw_value: value,
        n: 100,
        source: if matches!(component, "transition" | "recovery") {
            "check+steps".to_owned()
        } else {
            "check".to_owned()
        },
        corrected: component == "speed",
    }
}

/// Seven earlier runs and today's full run (day 9). Preview wide was played
/// at level 2 on day 7 (1.54) and at level 1 on day 8 (1.60); today it is at
/// level 2 again with 1.48, so the same-level previous value is 1.54.
fn seed(lang: Lang) -> (tempfile::TempDir, Seed, Config) {
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    // Skill series for days 0..=8 and today: preview falls (better), speed rises.
    let previews = [1.62, 1.60, 1.61, 1.58, 1.59, 1.57, 1.56, 1.55, 1.56, 1.40];
    let speeds = [55.0, 55.5, 56.0, 56.5, 56.0, 57.0, 57.5, 58.0, 58.5, 59.0];
    let mut runs = Vec::new();
    for offset in 0..9 {
        let (preview_level, values): (u8, Vec<_>) = match offset {
            7 => (2, vec![(2, Some(1.54), 50.0, Some(true), None)]),
            8 => (1, vec![(2, Some(1.60), 50.0, Some(true), None)]),
            _ => (1, vec![]),
        };
        let menu = plan(day(offset), preview_level, lang);
        let mut steps = vec![(0, None, 59.0, None, None), (3, None, 57.0, None, None)];
        steps.extend(values);
        runs.push((store_run(&path, &menu, &steps), day(offset)));
    }
    let today = plan(day(9), 2, lang);
    let run = store_run(
        &path,
        &today,
        &[
            (0, None, 62.0, None, None),
            (1, Some(1.0), 60.0, Some(true), Some(2)),
            (2, Some(1.48), 61.0, Some(true), None),
            (3, None, 58.0, None, None),
            (4, None, 74.0, None, None),
        ],
    );
    runs.push((run, day(9)));
    for (index, (run, date)) in runs.iter().enumerate() {
        db::replace_skill_snapshots(
            &path,
            *run,
            &[
                snapshot(*run, *date, "speed", speeds[index]),
                snapshot(*run, *date, "accuracy", 97.0),
                snapshot(*run, *date, "preview", previews[index]),
            ],
        )
        .unwrap();
    }
    db::update_menu_progress(&path, "qwerty", "en", "accuracy.gate", |state| {
        state.level = 2;
        state.consecutive = 0;
    })
    .unwrap();
    db::update_menu_progress(&path, "qwerty", "en", "preview.wide", |state| {
        state.level = 2;
        state.consecutive = 2;
        state.judgment = PrescriptionJudgment::Improved;
        state.pair_judged_run = Some(run);
    })
    .unwrap();
    let mut config = base_config();
    config.general.lang = lang;
    config.storage.db_path = Some(path.clone());
    (directory, Seed { path, run }, config)
}

fn summary_app(lang: Lang, graphics: GraphicsMode) -> (tempfile::TempDir, common::TestApp) {
    let (directory, seed, config) = seed(lang);
    let mut app = test_app_at(config.clone(), day(9));
    let layout = rs_typing_trainer::layout::builtin_layout("qwerty").unwrap();
    app.menu_summary = build_summary(&seed.path, directory.path(), &config, &layout, seed.run, lang, day(9)).unwrap();
    assert!(app.menu_summary.is_some());
    app.screen = Screen::MenuSummary;
    app.set_graphics_detection(GraphicsDetection {
        mode: graphics,
        cell_pixels: Some(CellPixels::new(9, 20)),
        reason: if graphics == GraphicsMode::Off {
            GraphicsReason::Disabled
        } else {
            GraphicsReason::Explicit(graphics)
        },
    });
    (directory, app)
}

fn row_containing<'a>(rows: &'a [String], needle: &str) -> (usize, &'a String) {
    rows.iter()
        .enumerate()
        .find(|(_, row)| compact(row).contains(needle))
        .unwrap_or_else(|| panic!("no row contains {needle:?}:\n{}", rows.join("\n")))
}

// ---------------------------------------------------------------------------
// Rendering with synthetic records
// ---------------------------------------------------------------------------

#[test]
fn upper_section_compares_with_the_same_prescription_and_level_and_marks_graduation() {
    let (_directory, app) = summary_app(Lang::Ja, GraphicsMode::Off);
    let buffer = render_buffer(&app, 80, 24);
    let rows = rows(&buffer);
    let text = rows.join("\n");
    assert!(compact(&text).contains("今日の練習2026-11-11"), "{text}");
    assert!(compact(&text).contains("5段中5段"), "{text}");

    // accuracy.gate graduated level 1 → «→ ② へ».
    let (_, gate) = row_containing(&rows, "正確さの関門");
    assert!(compact(gate).contains("●●●→②へ"), "{text}");

    // preview.wide level 2: 1.48 vs the level-2 run (1.54), not the level-1
    // run of the day before (1.60). Lower is better: green ▲.
    let (y, preview) = row_containing(&rows, "先読みを広げる②");
    assert!(preview.contains("1.48"), "{text}");
    assert!(preview.contains("▲-0.06"), "{text}");
    assert!(compact(preview).contains("●●○2/3"), "{text}");
    let x = preview.chars().take_while(|character| *character != '▲').count() as u16;
    // Column of the arrow in the buffer: count cells, not characters.
    let cell = (0..buffer.area.width)
        .map(|x| &buffer[(x, y as u16)])
        .find(|cell| cell.symbol() == "▲")
        .unwrap_or_else(|| panic!("no arrow at {x}: {text}"));
    assert_eq!(cell.fg, Color::LightGreen, "{text}");

    // Warm-up: net WPM and its difference with the previous warm-up.
    let (_, warmup) = row_containing(&rows, "1ウォームアップ");
    assert!(warmup.contains("62 WPM") && warmup.contains("▲+3"), "{text}");
}

#[test]
fn lower_is_better_component_falling_is_green_and_up_in_the_middle_section() {
    let (_directory, app) = summary_app(Lang::En, GraphicsMode::Off);
    let buffer = render_buffer(&app, 80, 24);
    let rows = rows(&buffer);
    let text = rows.join("\n");
    let (y, preview) = row_containing(&rows, "Preview");
    // 1.40 − 1.56 = −0.16: lower is better.
    assert!(preview.contains("▲-0.16"), "{text}");
    let cell = (0..buffer.area.width)
        .map(|x| &buffer[(x, y as u16)])
        .find(|cell| cell.symbol() == "▲")
        .unwrap();
    assert_eq!(cell.fg, Color::LightGreen, "{text}");
    // A component without a snapshot shows «no data», never 0.
    let (_, rhythm) = row_containing(&rows, "Rhythm");
    assert!(rhythm.contains("no data"), "{text}");
}

#[test]
fn practice_overall_and_official_bench_grade_are_separate_rows() {
    for lang in [Lang::Ja, Lang::En] {
        let (_directory, app) = summary_app(lang, GraphicsMode::Off);
        let rows = rows(&render_buffer(&app, 80, 24));
        let text = rows.join("\n");
        let (practice, official) = match lang {
            Lang::Ja => ("総合（練習から）", "正式の段（ベンチ）:—（データなし）"),
            Lang::En => ("Overall(frompractice)", "Officialgrade(bench):—(nodata)"),
        };
        let (practice_row, practice_line) = row_containing(&rows, practice);
        let (official_row, _) = row_containing(&rows, official);
        assert_ne!(practice_row, official_row, "{text}");
        assert!(
            !compact(practice_line).contains("ベンチ") && !compact(practice_line).contains("bench"),
            "{text}"
        );
    }
}

#[test]
fn lower_section_never_exceeds_four_lines() {
    let (_directory, app) = summary_app(Lang::Ja, GraphicsMode::Off);
    let data = app.menu_summary.as_ref().unwrap();
    let lines = change_lines(data, Lang::Ja);
    assert!(lines.len() <= MAX_CHANGE_LINES);
    // Level change first, then the reference judgment with its caveat.
    assert!(lines[0].contains("正確さの関門") && lines[0].contains("②"), "{lines:?}");
    assert!(lines.iter().any(|line| line.contains("改善（参考．対照なし）")), "{lines:?}");
    // Preview's −0.16 exceeds its typical change: value and typical change.
    assert!(
        lines.iter().any(|line| line.contains("先読み") && line.contains("ふだんの変動")),
        "{lines:?}"
    );

    let mut many = data.clone();
    for _ in 0..6 {
        many.steps.changes.push(many.steps.changes[0].clone());
    }
    assert_eq!(change_lines(&many, Lang::Ja).len(), MAX_CHANGE_LINES);
    let rows = rows(&render_buffer(&app, 80, 24));
    let start = row_containing(&rows, "今日の変化").0;
    let bullets = rows[start + 1..].iter().filter(|row| row.contains('・')).count();
    assert!(bullets <= MAX_CHANGE_LINES, "{}", rows.join("\n"));
}

#[test]
fn summary_fits_80_by_24_with_and_without_images() {
    for lang in [Lang::Ja, Lang::En] {
        for graphics in [GraphicsMode::Off, GraphicsMode::Kitty] {
            let (_directory, app) = summary_app(lang, graphics);
            for (width, height) in [(80, 24), (130, 40), (40, 12)] {
                let rows = rows(&render_buffer(&app, width, height));
                let text = rows.join("\n");
                if (width, height) == (80, 24) {
                    // Every section is visible and the hint is in the frame.
                    for needle in ["[Enter/Esc]", "Speed", "速さ"].iter().filter(|needle| match lang {
                        Lang::Ja => **needle != "Speed",
                        Lang::En => **needle != "速さ",
                    }) {
                        assert!(compact(&text).contains(&compact(needle)), "{lang:?} {graphics:?}: {text}");
                    }
                    let last = rows.last().unwrap();
                    assert!(last.starts_with('└') && last.ends_with('┘'), "{text}");
                    // The lines stay inside the frame (full-width text measured
                    // by display width).
                    assert!(rows.iter().skip(1).take(22).all(|row| row.ends_with('│')), "{text}");
                }
            }
        }
    }
}

#[test]
fn enter_or_escape_on_the_summary_returns_to_the_menu() {
    for code in [KeyCode::Enter, KeyCode::Esc] {
        let (_directory, mut app) = summary_app(Lang::En, GraphicsMode::Off);
        app.handle_key(key(code));
        assert_eq!(app.screen, Screen::Menu);
        assert!(app.menu_summary.is_none());
    }
}

// ---------------------------------------------------------------------------
// The flow through the app
// ---------------------------------------------------------------------------

fn play_selected_step(app: &mut rs_typing_trainer::app::App, now: &mut Instant) {
    assert!(matches!(app.screen, Screen::MenuFocus { .. }), "{:?}", app.screen);
    *now += Duration::from_millis(10);
    app.handle_key_at(key(KeyCode::Char('x')), *now);
    *now += Duration::from_secs(4);
    app.on_tick_at(Duration::from_secs(3), *now).unwrap();
    assert_eq!(app.screen, Screen::Playing);
    let seconds = app.config.session.duration_seconds;
    *now += Duration::from_secs(seconds + 5);
    app.on_tick_at(Duration::from_secs(seconds), *now).unwrap();
    assert_eq!(app.screen, Screen::Result);
}

fn flow_app(lang: Lang) -> common::TestApp {
    let mut config = base_config();
    config.general.lang = lang;
    let mut app = test_app_at(config, NaiveDate::from_ymd_opt(2026, 11, 18).unwrap());
    app.handle_key(key(KeyCode::Char('m')));
    assert_eq!(app.screen, Screen::Menu);
    app
}

#[test]
fn finishing_the_last_step_opens_the_full_summary() {
    let mut app = flow_app(Lang::Ja);
    let mut now = Instant::now();
    app.handle_key_at(key(KeyCode::Enter), now);
    for _ in 0..20 {
        play_selected_step(&mut app, &mut now);
        now += Duration::from_secs(1);
        app.handle_key_at(key(KeyCode::Enter), now);
        if app.screen == Screen::MenuSummary {
            break;
        }
    }
    assert_eq!(app.screen, Screen::MenuSummary);
    let data = app.menu_summary.as_ref().unwrap();
    assert!(!data.partial);
    assert_eq!(data.steps.completed(), data.steps.steps.len());
    let text = rows(&render_buffer(&app, 80, 24)).join("\n");
    assert!(compact(&text).contains("今日の練習"), "{text}");
    assert!(!compact(&text).contains("未実施"), "{text}");
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.screen, Screen::Menu);
}

#[test]
fn escape_mid_run_shows_the_summary_so_far_with_unplayed_steps() {
    let mut app = flow_app(Lang::Ja);
    let mut now = Instant::now();
    app.handle_key_at(key(KeyCode::Enter), now);
    play_selected_step(&mut app, &mut now);
    app.handle_key_at(key(KeyCode::Esc), now + Duration::from_secs(1));
    assert_eq!(app.screen, Screen::MenuSummary);
    let data = app.menu_summary.as_ref().unwrap();
    assert!(data.partial);
    assert_eq!(data.steps.completed(), 1);
    let text = rows(&render_buffer(&app, 80, 24)).join("\n");
    assert!(compact(&text).contains("ここまでのまとめ"), "{text}");
    assert!(compact(&text).contains("未実施"), "{text}");
    // The check step is not played yet: the middle section is one line.
    assert!(compact(&text).contains("確認段を終えると表示"), "{text}");
    assert!(!compact(&text).contains("総合（練習から）"), "{text}");
}

#[test]
fn escape_while_playing_a_later_step_also_shows_the_summary_so_far() {
    let mut app = flow_app(Lang::En);
    let mut now = Instant::now();
    app.handle_key_at(key(KeyCode::Enter), now);
    play_selected_step(&mut app, &mut now);
    now += Duration::from_secs(1);
    app.handle_key_at(key(KeyCode::Enter), now);
    assert!(matches!(app.screen, Screen::MenuFocus { .. }));
    app.handle_key_at(key(KeyCode::Esc), now + Duration::from_millis(10));
    assert_eq!(app.screen, Screen::MenuSummary);
    let text = rows(&render_buffer(&app, 80, 24)).join("\n");
    assert!(text.contains("SUMMARY SO FAR"), "{text}");
    assert!(text.contains("not played"), "{text}");
}

#[test]
fn escape_before_any_step_is_finished_returns_straight_to_the_menu() {
    let mut app = flow_app(Lang::En);
    let now = Instant::now();
    app.handle_key_at(key(KeyCode::Enter), now);
    assert!(matches!(app.screen, Screen::MenuFocus { .. }));
    app.handle_key_at(key(KeyCode::Esc), now + Duration::from_millis(10));
    assert_eq!(app.screen, Screen::Menu);
    assert!(app.menu_summary.is_none());
}

// ---------------------------------------------------------------------------
// Previous-run differences stay within one language (§4.2)
// ---------------------------------------------------------------------------

/// A one-step run whose only step is a check step: `plan_language` is what
/// the plan says, `typed` the language of the session that was saved.
fn store_check_run(path: &std::path::Path, date: NaiveDate, plan_language: Lang, typed: &str, wpm: f64) -> i64 {
    let mut check = fixed_step(MenuRole::Check, "sentence");
    check.language = plan_language;
    let menu = DailyMenu {
        steps: vec![check],
        ..plan(date, 1, Lang::Ja)
    };
    let run = db::ensure_menu_run(path, date, "qwerty", "en,ja", "menu-v1", 15, &serde_json::to_string(&menu).unwrap())
        .unwrap()
        .id;
    let mut record = session(date, 9, wpm);
    record.header.mode_params = serde_json::json!({ "lang": typed }).to_string();
    let session_id = db::save(path, &record).unwrap();
    db::record_menu_step(
        path,
        &MenuStepRecord {
            run_id: run,
            idx: 1,
            role: "check".to_owned(),
            prescription_id: Some("fixed.check".to_owned()),
            level: None,
            session_id,
            metric: None,
            value: None,
            advance_met: None,
            level_after: None,
        },
    )
    .unwrap();
    run
}

fn check_difference(path: &std::path::Path, run: i64, date: NaiveDate) -> (Option<f64>, Option<f64>) {
    let directory = tempfile::TempDir::new().unwrap();
    let mut config = base_config();
    config.storage.db_path = Some(path.to_path_buf());
    let layout = rs_typing_trainer::layout::builtin_layout("qwerty").unwrap();
    let data = build_summary(path, directory.path(), &config, &layout, run, Lang::Ja, date)
        .unwrap()
        .unwrap();
    let step = &data.steps.steps[0];
    (step.previous, step.difference())
}

#[test]
fn check_step_is_not_compared_with_the_previous_day_in_another_language() {
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    // Yesterday: English check (33.8 WPM). Today: Japanese check.
    store_check_run(&path, day(1), Lang::En, "en", 33.8);
    let today = store_check_run(&path, day(2), Lang::Ja, "ja", 40.0);
    assert_eq!(check_difference(&path, today, day(2)), (None, None));

    let mut config = base_config();
    config.general.lang = Lang::Ja;
    config.storage.db_path = Some(path.clone());
    let mut app = test_app_at(config.clone(), day(2));
    let layout = rs_typing_trainer::layout::builtin_layout("qwerty").unwrap();
    app.menu_summary = build_summary(&path, directory.path(), &config, &layout, today, Lang::Ja, day(2)).unwrap();
    app.screen = Screen::MenuSummary;
    let rows = rows(&render_buffer(&app, 80, 24));
    let (_, check) = row_containing(&rows, "1確認");
    assert!(check.contains("40 WPM") && check.contains('—'), "{}", rows.join("\n"));
    assert!(!check.contains('▼') && !check.contains('▲'), "{}", rows.join("\n"));
}

#[test]
fn check_step_is_compared_with_the_latest_check_in_the_same_language() {
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    store_check_run(&path, day(0), Lang::Ja, "ja", 30.0);
    store_check_run(&path, day(1), Lang::En, "en", 33.8);
    let today = store_check_run(&path, day(2), Lang::Ja, "ja", 40.0);
    let (previous, difference) = check_difference(&path, today, day(2));
    assert_eq!(previous, Some(30.0));
    assert!((difference.unwrap() - 10.0).abs() < 1e-9);
}

#[test]
fn the_typed_language_wins_over_a_plan_that_names_another_language() {
    // Yesterday's plan said Japanese, but the check was typed in English.
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    store_check_run(&path, day(1), Lang::Ja, "en", 33.8);
    let today = store_check_run(&path, day(2), Lang::Ja, "ja", 40.0);
    assert_eq!(check_difference(&path, today, day(2)), (None, None));
}

#[test]
fn practice_overall_previous_is_taken_from_the_same_language_series() {
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    let japanese = store_check_run(&path, day(0), Lang::Ja, "ja", 30.0);
    let english = store_check_run(&path, day(1), Lang::En, "en", 33.8);
    let today = store_check_run(&path, day(2), Lang::Ja, "ja", 40.0);
    let snapshot = |run: i64, date: NaiveDate, lang: &str, component: &str, value: f64| SkillSnapshotRecord {
        lang: lang.to_owned(),
        ..super_snapshot(run, date, component, value)
    };
    for (run, date, lang, speed) in [(japanese, day(0), "ja", 30.0), (english, day(1), "en", 80.0), (today, day(2), "ja", 40.0)] {
        db::replace_skill_snapshots(
            &path,
            run,
            &[snapshot(run, date, lang, "speed", speed), snapshot(run, date, lang, "accuracy", 97.0)],
        )
        .unwrap();
    }
    let mut config = base_config();
    config.storage.db_path = Some(path.clone());
    let layout = rs_typing_trainer::layout::builtin_layout("qwerty").unwrap();
    let data = build_summary(&path, directory.path(), &config, &layout, today, Lang::Ja, day(2))
        .unwrap()
        .unwrap();
    let skills = data.skills.unwrap();
    assert_eq!(skills.language, "ja");
    let speed = &skills.components[0];
    // 40 − 30 (the Japanese run), not 40 − 80 (the English run in between).
    assert!((speed.trend.unwrap().previous_difference.unwrap() - 10.0).abs() < 1e-9);
    let expected = rs_typing_trainer::analysis::grades::overall_from_values(
        "ja",
        &[
            rs_typing_trainer::analysis::grades::ComponentValue::new(rs_typing_trainer::analysis::grades::Component::Speed, 30.0),
            rs_typing_trainer::analysis::grades::ComponentValue::new(rs_typing_trainer::analysis::grades::Component::Accuracy, 97.0),
        ],
    );
    assert_eq!(skills.previous_overall, expected);
}

fn super_snapshot(run: i64, date: NaiveDate, component: &str, value: f64) -> SkillSnapshotRecord {
    snapshot(run, date, component, value)
}
