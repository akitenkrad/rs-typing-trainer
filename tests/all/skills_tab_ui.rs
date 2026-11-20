//! M6: the analysis screen's «skills» tab (design §8.4).

use std::time::Duration;

use chrono::{NaiveDate, TimeZone, Utc};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::analysis::grades::Component;
use rs_typing_trainer::app::{ANALYSIS_SKILLS_TAB, ANALYSIS_TAB_COUNT, Screen};
use rs_typing_trainer::config::Config;
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::graphics::{CellPixels, GraphicsDetection, GraphicsMode, GraphicsReason};
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::practice::menu::skills::{BENCH_OVERLAY_COMPONENTS, bench_component_points};
use rs_typing_trainer::practice::menu::{DailyMenu, MenuRole, MenuStep, bundled_catalog};
use rs_typing_trainer::record::SessionRecord;
use rs_typing_trainer::record::db::{self, MenuStepRecord, SkillSnapshotRecord};
use rs_typing_trainer::ui;

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

fn text(buffer: &Buffer) -> String {
    (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn compact(text: &str) -> String {
    text.chars().filter(|character| !character.is_whitespace()).collect()
}

/// 2026-11-02 is a Monday; «today» is day 9.
fn day(offset: i64) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 2).unwrap() + chrono::Duration::days(offset)
}

fn session(date: NaiveDate, bench_id: Option<&str>, wpm: f64) -> SessionRecord {
    let config = Config::default();
    let layout = rs_typing_trainer::layout::builtin_layout("qwerty").unwrap();
    let mut session = Session::new(Duration::from_secs(1), Duration::ZERO, Item::exact("as"));
    session.activate(Duration::ZERO);
    session.input_char_with_code(Duration::from_millis(100), "a", layout.stroke_for('a').map(|stroke| stroke.code));
    session.input_char_with_code(Duration::from_millis(250), "s", layout.stroke_for('s').map(|stroke| stroke.code));
    session.tick(Duration::from_secs(1));
    let mut record = SessionRecord::from_log(&config, session.into_log(), Utc.from_utc_datetime(&date.and_hms_opt(9, 0, 0).unwrap()));
    record.header.net_wpm = wpm;
    if let Some(id) = bench_id {
        record.header.mode = "bench".into();
        record.header.bench_id = Some(id.into());
    }
    record
}

fn primary(id: &str, role: MenuRole, language: Lang) -> MenuStep {
    let prescription = bundled_catalog().unwrap().prescription(id).unwrap();
    let row = &prescription.levels[0];
    MenuStep {
        role,
        language,
        prescription_id: id.to_owned(),
        title: prescription.title.text(Lang::En).to_owned(),
        level: Some(1),
        mode: row.mode.clone(),
        minutes: row.minutes,
        reason: "-".to_owned(),
        focus: String::new(),
        advance: String::new(),
        advance_condition: Some(row.advance.clone()),
        evidence: None,
        evidence_note: None,
        settings: row.settings.clone(),
    }
}

fn snapshot(run: i64, date: NaiveDate, language: &str, component: &str, value: f64) -> SkillSnapshotRecord {
    SkillSnapshotRecord {
        run_id: run,
        date,
        lang: language.to_owned(),
        component: component.to_owned(),
        raw_value: value,
        n: 100,
        source: "check".to_owned(),
        corrected: component == "speed",
    }
}

/// Ten English menu days (preview.wide as primary A on days 0–1, then
/// accuracy.gate) with speed, accuracy and preview snapshots, two Japanese
/// days, and English bench sessions (part A) on days 1 and 8.
fn seeded(lang: Lang, graphics: GraphicsMode) -> (tempfile::TempDir, common::TestApp) {
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    for offset in 0..10 {
        let language = if offset % 5 == 4 { "ja" } else { "en" };
        let practice = if language == "ja" { Lang::Ja } else { Lang::En };
        let id = if offset < 2 { "preview.wide" } else { "accuracy.gate" };
        let menu = DailyMenu {
            date: day(offset),
            language: language.to_owned(),
            catalog_version: "menu-v1".to_owned(),
            requested_minutes: 15,
            introductory: false,
            bench_recommended: false,
            steps: vec![primary(id, MenuRole::PrimaryA, practice)],
        };
        let run = db::ensure_menu_run(&path, menu.date, "qwerty", "en,ja", "menu-v1", 15, &serde_json::to_string(&menu).unwrap())
            .unwrap()
            .id;
        let session_id = db::save(&path, &session(day(offset), None, 50.0)).unwrap();
        db::record_menu_step(
            &path,
            &MenuStepRecord {
                run_id: run,
                idx: 1,
                role: "primary-a".to_owned(),
                prescription_id: Some(id.to_owned()),
                level: Some(1),
                session_id,
                metric: None,
                value: None,
                advance_met: None,
                level_after: Some(1),
            },
        )
        .unwrap();
        let shift = offset as f64;
        db::replace_skill_snapshots(
            &path,
            run,
            &[
                snapshot(run, day(offset), language, "speed", 50.0 + shift),
                snapshot(run, day(offset), language, "accuracy", 95.0 + shift * 0.2),
                snapshot(run, day(offset), language, "preview", 1.7 - shift * 0.02),
            ],
        )
        .unwrap();
    }
    for (offset, wpm, index) in [(1, 52.0, "01"), (8, 61.0, "02")] {
        db::save(&path, &session(day(offset), Some(&format!("bench-v1/A/{index}")), wpm)).unwrap();
    }
    let mut config = Config::default();
    config.general.lang = lang;
    config.storage.db_path = Some(path);
    let mut app = test_app_at(config, day(9));
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

fn open_skills(app: &mut rs_typing_trainer::app::App) {
    app.handle_key(key(KeyCode::Char('a')));
    assert_eq!(app.screen, Screen::Analysis);
    // The screen opens on the coach tab; ← reaches «today» (the first tab)
    // and wraps from there to the last tab, the skills tab.
    app.handle_key(key(KeyCode::Left));
    app.handle_key(key(KeyCode::Left));
    assert_eq!(app.analysis_tab, ANALYSIS_SKILLS_TAB);
}

#[test]
fn skills_tab_is_the_last_tab_and_tab_cycles_through_it() {
    let (_directory, mut app) = seeded(Lang::Ja, GraphicsMode::Off);
    app.handle_key(key(KeyCode::Char('a')));
    for _ in rs_typing_trainer::app::ANALYSIS_COACH_TAB..ANALYSIS_SKILLS_TAB {
        app.handle_key(key(KeyCode::Tab));
    }
    assert_eq!(app.analysis_tab, ANALYSIS_SKILLS_TAB);
    let output = text(&render_buffer(&app, 80, 24));
    // The display language's series is shown first.
    for expected in [
        "スキル",
        "日本語—",
        "処方の履歴（参考）",
        "速さ",
        "正確さ",
        "先読み",
        "重なり",
        "[L]言語:日本語",
    ] {
        assert!(compact(&output).contains(expected), "{expected}: {output}");
    }
    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.analysis_tab, (ANALYSIS_SKILLS_TAB + 1) % ANALYSIS_TAB_COUNT);
}

#[test]
fn no_records_show_the_check_step_guidance() {
    for (lang, expected) in [
        (Lang::Ja, "メニューの確認段を終えると表示"),
        (Lang::En, "Shownafterfinishingthecheckstep"),
    ] {
        let mut config = Config::default();
        config.general.lang = lang;
        let mut app = common::test_app(config);
        open_skills(&mut app);
        let output = text(&render_buffer(&app, 80, 24));
        assert!(compact(&output).contains(expected), "{output}");
    }
}

#[test]
fn l_switches_the_series_language() {
    let (_directory, mut app) = seeded(Lang::En, GraphicsMode::Off);
    open_skills(&mut app);
    let english = text(&render_buffer(&app, 80, 24));
    assert!(compact(&english).contains("English—"), "{english}");
    assert!(english.contains("[L] language: English"), "{english}");
    app.handle_key(key(KeyCode::Char('l')));
    let japanese = text(&render_buffer(&app, 80, 24));
    assert!(compact(&japanese).contains("Japanese—"), "{japanese}");
    assert!(japanese.contains("[L] language: Japanese"), "{japanese}");
    app.handle_key(key(KeyCode::Char('L')));
    assert!(text(&render_buffer(&app, 80, 24)).contains("[L] language: English"));
    // `L` on another tab keeps its meaning there (nothing on the «today» tab,
    // which follows the skills tab when Tab wraps).
    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.analysis_tab, rs_typing_trainer::app::ANALYSIS_TODAY_TAB);
    app.handle_key(key(KeyCode::Char('l')));
    assert_eq!(app.screen, Screen::Analysis);
}

#[test]
fn p_limits_the_period_including_the_prescription_history() {
    let (_directory, mut app) = seeded(Lang::En, GraphicsMode::Off);
    open_skills(&mut app);
    // Analysis opens on 30 days (MYTASK-3630); the seeded history fits in it.
    let month = compact(&text(&render_buffer(&app, 80, 24)));
    assert!(month.contains("Widenpreview"), "{month}");
    assert!(month.contains("Accuracygate"), "{month}");
    app.handle_key(key(KeyCode::Char('p')));
    // 7 days: day 3 onwards — preview.wide (days 0–1) is out of the period.
    let week = compact(&text(&render_buffer(&app, 80, 24)));
    assert!(week.contains("7days") || week.contains("7d"), "{week}");
    assert!(!week.contains("Widenpreview"), "{week}");
    assert!(week.contains("Accuracygate"), "{week}");
}

#[test]
fn bench_sessions_are_overlaid_with_another_marker_only_for_speed_and_accuracy() {
    assert_eq!(BENCH_OVERLAY_COMPONENTS, [Component::Speed, Component::Accuracy]);
    let records = vec![session(day(1), Some("bench-v1/A/01"), 52.0), session(day(2), None, 40.0)];
    assert_eq!(bench_component_points(&records, "en", Component::Speed), vec![(day(1), 52.0)]);
    assert_eq!(bench_component_points(&records, "en", Component::Accuracy).len(), 1);
    assert!(bench_component_points(&records, "en", Component::Preview).is_empty());
    assert!(bench_component_points(&records, "ja", Component::Speed).is_empty());

    let (_directory, mut app) = seeded(Lang::En, GraphicsMode::Off);
    open_skills(&mut app);
    let buffer = render_buffer(&app, 80, 24);
    let output = text(&buffer);
    assert!(output.contains("█ bench"), "{output}");
    let rows = output.lines().collect::<Vec<_>>();
    let block_rows = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.contains('█') && !row.contains("bench"))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let speed = rows.iter().position(|row| row.contains("│Speed ")).unwrap();
    let accuracy = rows.iter().position(|row| row.contains("│Accuracy ")).unwrap();
    let preview = rows.iter().position(|row| row.contains("│Preview ")).unwrap();
    assert!(block_rows.iter().any(|row| (speed..accuracy).contains(row)), "{output}");
    assert!(
        !block_rows.iter().any(|row| *row >= preview),
        "bench is not overlaid on preview:\n{output}"
    );
}

#[test]
fn skills_tab_renders_with_and_without_images_and_fits_80_by_24() {
    for lang in [Lang::Ja, Lang::En] {
        for graphics in [GraphicsMode::Off, GraphicsMode::Kitty] {
            let (_directory, mut app) = seeded(lang, graphics);
            open_skills(&mut app);
            for (width, height) in [(80, 24), (140, 45), (50, 14)] {
                let output = text(&render_buffer(&app, width, height));
                if (width, height) == (80, 24) {
                    let label = match lang {
                        Lang::Ja => "重なり",
                        Lang::En => "Overlap",
                    };
                    assert!(compact(&output).contains(label), "{lang:?} {graphics:?}: {output}");
                    let legend_end = match lang {
                        Lang::Ja => "帯:段G〜S",
                        Lang::En => "bandsG–S",
                    };
                    assert!(
                        compact(&output).contains(legend_end),
                        "{lang:?} {graphics:?}: legend is cut at 80 columns:\n{output}"
                    );
                    let last = output.lines().last().unwrap();
                    assert!(last.starts_with('└'), "{output}");
                }
            }
        }
    }
}
