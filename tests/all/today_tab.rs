//! MYTASK-3631: the «today» tab of the analysis screen.

use chrono::{NaiveDate, TimeZone, Utc};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Rect, Size};
use rs_typing_trainer::app::{ANALYSIS_COACH_TAB, ANALYSIS_TODAY_TAB, AnalysisPeriod, Screen};
use rs_typing_trainer::config::{Config, ErrorMode};
use rs_typing_trainer::engine::session::{ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};
use rs_typing_trainer::graphics::{CellPixels, GraphicsDetection, GraphicsMode, GraphicsReason};
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::practice::menu::{DailyMenu, MenuRole, MenuStep, bundled_catalog};
use rs_typing_trainer::record::db::{self, MenuStepRecord};
use rs_typing_trainer::record::{SessionHeader, SessionRecord};
use rs_typing_trainer::ui;
use rs_typing_trainer::ui::analysis_image::{TodayStepsLine, render_today_card, render_today_steps, render_today_timeline, today_timeline_range};
use rs_typing_trainer::ui::bigtext_image::resolve_big_font;
use rs_typing_trainer::ui::theme::Theme;
use rs_typing_trainer::ui::today::{TodayLayout, card_specs, layout_for, steps_lines, timeline_spec};

use crate::common;

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

fn row_containing<'a>(rows: &'a [String], needle: &str) -> &'a String {
    rows.iter()
        .find(|row| compact(row).contains(needle))
        .unwrap_or_else(|| panic!("no row contains {needle:?}:\n{}", rows.join("\n")))
}

/// The outer frame stays intact: every row starts and ends with the border.
fn assert_inside_frame(buffer: &Buffer) {
    let (width, height) = (buffer.area.width, buffer.area.height);
    for y in 1..height - 1 {
        assert_eq!(buffer[(0, y)].symbol(), "│", "row {y}:\n{}", rows(buffer).join("\n"));
        assert_eq!(buffer[(width - 1, y)].symbol(), "│", "row {y}:\n{}", rows(buffer).join("\n"));
    }
}

/// Thursday 2026-11-19.
fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 19).unwrap()
}

/// 9 days before today: outside the 7-day period.
fn previous_day() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 10).unwrap()
}

/// A session at `hour:minute` UTC; the hours used (10–13 UTC) fall on the same
/// local date in every time zone from UTC−10 to UTC+10.
#[allow(clippy::too_many_arguments)]
fn session(date: NaiveDate, hour: u32, minute: u32, mode: &str, lang: &str, net_wpm: f64, accuracy: f64, keystrokes: u32) -> SessionRecord {
    let start = Utc.from_utc_datetime(&date.and_hms_opt(hour, minute, 0).unwrap());
    SessionRecord {
        header: SessionHeader {
            id: None,
            started_at: start.to_rfc3339(),
            finished_at: (start + chrono::Duration::seconds(60)).to_rfc3339(),
            duration_ms: 60_000,
            mode: mode.to_owned(),
            mode_params: serde_json::json!({ "lang": lang }).to_string(),
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
            accuracy,
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
            keystrokes: (0..keystrokes)
                .map(|seq| KeystrokeLog {
                    seq,
                    t_us: u64::from(seq) * 100_000,
                    release_us: None,
                    item_idx: 0,
                    pos: 0,
                    kind: KeystrokeKind::Char,
                    typed: Some("a".to_owned()),
                    expected: Some("a".to_owned()),
                    correct: true,
                    code: None,
                })
                .collect(),
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

fn plan(date: NaiveDate, lang: Lang) -> DailyMenu {
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
            prescription_step("preview.wide", MenuRole::PrimaryB, 1, lang),
            fixed_step(MenuRole::Check, "sentence"),
            fixed_step(MenuRole::Finish, "burst"),
        ],
    }
}

/// (index, value, net WPM, advance_met, level_after, hour, minute).
type Played = (usize, Option<f64>, f64, Option<bool>, Option<u8>, u32, u32);

fn store_run(path: &std::path::Path, menu: &DailyMenu, steps: &[Played]) -> i64 {
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
    for (index, value, wpm, met, after, hour, minute) in steps {
        let step = &menu.steps[*index];
        let session_id = db::save(path, &session(menu.date, *hour, *minute, &step.mode, "en", *wpm, 0.97, 300)).unwrap();
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

/// Today: the warm-up (62 WPM, previous run 59) and the accuracy gate (met,
/// level 1 → 2) of a 5-step menu, one individual Japanese session and one
/// bench session. The previous practice day (9 days earlier) has a run with
/// the warm-up at 59 WPM and two sessions in all.
fn seed(lang: Lang) -> (tempfile::TempDir, Config) {
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    store_run(&path, &plan(previous_day(), lang), &[(0, None, 59.0, None, None, 10, 0)]);
    db::save(&path, &session(previous_day(), 10, 30, "words", "en", 55.0, 0.95, 200)).unwrap();
    store_run(
        &path,
        &plan(today(), lang),
        &[(0, None, 62.0, None, None, 10, 0), (1, Some(1.0), 60.0, Some(true), Some(2), 10, 5)],
    );
    db::save(&path, &session(today(), 11, 30, "romaji", "ja", 48.5, 0.93, 250)).unwrap();
    let mut bench = session(today(), 12, 15, "words", "en", 64.0, 0.98, 320);
    bench.header.bench_id = Some("bench-v1/A/01".to_owned());
    db::save(&path, &bench).unwrap();
    db::update_menu_progress(&path, "qwerty", "en", "accuracy.gate", |state| {
        state.level = 2;
        state.consecutive = 0;
    })
    .unwrap();
    let mut config = Config::default();
    config.general.lang = lang;
    config.storage.db_path = Some(path);
    (directory, config)
}

fn open_today(config: Config, graphics: GraphicsMode) -> common::TestApp {
    let mut app = common::test_app_at(config, today());
    app.set_graphics_detection(GraphicsDetection {
        mode: graphics,
        cell_pixels: Some(CellPixels::new(9, 20)),
        reason: if graphics == GraphicsMode::Off {
            GraphicsReason::Disabled
        } else {
            GraphicsReason::Explicit(graphics)
        },
    });
    app.handle_key(key(KeyCode::Char('a')));
    assert_eq!(app.screen, Screen::Analysis);
    // The screen still opens on the coach tab; «today» is the first tab.
    assert_eq!(app.analysis_tab, ANALYSIS_COACH_TAB);
    app.handle_key(key(KeyCode::Left));
    assert_eq!(app.analysis_tab, ANALYSIS_TODAY_TAB);
    app
}

#[test]
fn text_version_shows_the_three_sections_unplayed_steps_and_the_previous_day() {
    for (width, height) in [(80, 24), (160, 45)] {
        let (_directory, config) = seed(Lang::Ja);
        let app = open_today(config, GraphicsMode::Off);
        let buffer = render_buffer(&app, width, height);
        assert_inside_frame(&buffer);
        let rows = rows(&buffer);
        let text = rows.join("\n");
        for expected in ["今日の訓練2026-11-19", "個別の練習（2回）", "今日の合計—前に練習した日（11/10）と比べる"] {
            assert!(compact(&text).contains(expected), "{width}×{height} {expected}:\n{text}");
        }
        // The tab bar fits: the first and the last tab are both visible.
        assert!(
            compact(&rows[1]).starts_with("今日") || compact(&rows[1]).contains("今日コーチ"),
            "{text}"
        );
        assert!(compact(&rows[1]).contains("スキル"), "{text}");
        // Today's training: played steps with their values, the level-up and
        // the steps not played yet marked «未».
        assert!(compact(row_containing(&rows, "1ウォームアップ")).contains("62WPM▲+3"), "{text}");
        assert!(compact(row_containing(&rows, "正確さの関門①")).contains("→②へ"), "{text}");
        for step in ["3主処方B", "4確認", "5仕上げ"] {
            assert!(compact(row_containing(&rows, step)).ends_with("未│"), "{step}:\n{text}");
        }
        // Individual practice: the Japanese session and the bench, not the menu steps.
        assert!(compact(row_containing(&rows, "ローマ字")).contains("48.5"), "{text}");
        assert!(compact(row_containing(&rows, "[ベンチA]")).contains("64.0"), "{text}");
        // Totals against 11/10: 4 sessions vs 2, 1,170 keystrokes vs 500.
        let totals = compact(row_containing(&rows, "練習時間"));
        assert!(totals.contains("回数4▲+2"), "{totals}");
        assert!(totals.contains("打鍵数1,170▲+670"), "{totals}");
        // English median of 62, 60, 64 = 62 vs the median of 59 and 55 = 57.
        assert!(compact(row_containing(&rows, "英語netWPM")).contains("62.0▲+5.0"), "{text}");
        // Japanese has no earlier day to compare with.
        assert!(compact(row_containing(&rows, "日本語netWPM")).contains("48.5—"), "{text}");
        // The footer says the filters do not apply.
        assert!(compact(&text).contains("期間[P]・モード[M]の絞り込みは効きません"), "{text}");
        assert!(!compact(&text).contains("期間:30日"), "{text}");
    }
}

#[test]
fn english_text_version_fits_80_columns() {
    let (_directory, config) = seed(Lang::En);
    let app = open_today(config, GraphicsMode::Off);
    let buffer = render_buffer(&app, 80, 24);
    assert_inside_frame(&buffer);
    let rows = rows(&buffer);
    let text = rows.join("\n");
    for expected in [
        "Today'straining",
        "Individualpractice(2sessions)",
        "Today'stotals—vsthelastpracticeday(11/10)",
    ] {
        assert!(compact(&text).contains(expected), "{expected}:\n{text}");
    }
    assert!(compact(&rows[1]).contains("Today") && compact(&rows[1]).contains("Skills"), "{text}");
    assert!(compact(row_containing(&rows, "4Check")).ends_with("notyet│"), "{text}");
}

#[test]
fn no_record_today_says_so() {
    for (lang, expected) in [(Lang::Ja, "今日の記録はまだありません"), (Lang::En, "Nopracticerecordedtodayyet")] {
        let mut config = Config::default();
        config.general.lang = lang;
        let app = open_today(config, GraphicsMode::Kitty);
        assert!(app.analysis_today.totals.is_none());
        let buffer = render_buffer(&app, 160, 45);
        assert_inside_frame(&buffer);
        let text = rows(&buffer).join("\n");
        assert!(compact(&text).contains(expected), "{text}");
    }
}

#[test]
fn seven_day_period_keeps_the_previous_day_comparison() {
    let (_directory, config) = seed(Lang::Ja);
    let mut app = open_today(config, GraphicsMode::Off);
    app.handle_key(key(KeyCode::Char('p')));
    assert_eq!(app.analysis_period, AnalysisPeriod::Days7);
    // The period filter drops the 11/10 records from the other tabs …
    assert!(
        app.analysis_records
            .iter()
            .all(|record| !record.header.started_at.starts_with("2026-11-10"))
    );
    // … but not from this tab.
    assert_eq!(app.analysis_today.previous.as_ref().map(|day| day.date), Some(previous_day()));
    let text = rows(&render_buffer(&app, 80, 24)).join("\n");
    assert!(compact(&text).contains("前に練習した日（11/10）と比べる"), "{text}");
    assert!(compact(&text).contains("回数4▲+2"), "{text}");
    // A mode filter does not apply either.
    app.handle_key(key(KeyCode::Char('m')));
    assert!(app.analysis_mode_filter.is_some());
    assert_eq!(app.analysis_today.sessions.len(), 4);
}

#[test]
fn multiple_runs_show_played_ones_newest_first_or_the_latest_unplayed() {
    let (directory, config) = seed(Lang::Ja);
    let path = directory.path().join("sessions.db");
    // A second run of today under other practice languages, not played.
    let mut other = plan(today(), Lang::Ja);
    other.language = "ja".to_owned();
    db::ensure_menu_run(&path, today(), "qwerty", "ja", "menu-v1", 15, &serde_json::to_string(&other).unwrap()).unwrap();
    let app = open_today(config.clone(), GraphicsMode::Off);
    assert_eq!(app.analysis_today.runs.len(), 1, "an unplayed run is left out when another was played");

    // Only unplayed runs today: the latest one is shown as not played.
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    db::ensure_menu_run(
        &path,
        today(),
        "qwerty",
        "en",
        "menu-v1",
        15,
        &serde_json::to_string(&plan(today(), Lang::Ja)).unwrap(),
    )
    .unwrap();
    db::ensure_menu_run(&path, today(), "qwerty", "ja", "menu-v1", 15, &serde_json::to_string(&other).unwrap()).unwrap();
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    config.storage.db_path = Some(path);
    let app = open_today(config, GraphicsMode::Off);
    assert_eq!(app.analysis_today.runs.len(), 1);
    assert_eq!(app.analysis_today.runs[0].languages, "ja");
    let text = rows(&render_buffer(&app, 80, 24)).join("\n");
    assert!(compact(&text).contains("—未実施"), "{text}");
    assert!(compact(&text).contains("今日の記録はまだありません"), "{text}");
}

#[test]
fn image_version_falls_back_to_text_in_the_result_screen_order() {
    let (_directory, config) = seed(Lang::Ja);
    let mut app = open_today(config, GraphicsMode::Kitty);
    let area = |width, height| Rect::new(0, 0, width, height);
    assert_eq!(layout_for(&app, area(158, 39)), TodayLayout::Full);
    assert_eq!(layout_for(&app, area(158, 36)), TodayLayout::Compact);
    assert_eq!(layout_for(&app, area(158, 29)), TodayLayout::Compact);
    assert_eq!(layout_for(&app, area(158, 28)), TodayLayout::Text);
    assert_eq!(layout_for(&app, area(119, 39)), TodayLayout::Text);

    // At 160×45 the training rows are images: the step names are not text.
    let rows_160 = rows(&render_buffer(&app, 160, 45));
    let text = rows_160.join("\n");
    assert!(compact(&text).contains("今日の訓練2026-11-19"), "{text}");
    assert!(compact(&text).contains("今日の練習（時刻順）—個別の練習2回"), "{text}");
    assert!(!compact(&text).contains("1ウォームアップ"), "{text}");
    // At 80×24 the same data is text.
    let rows_80 = rows(&render_buffer(&app, 80, 24));
    assert!(compact(&rows_80.join("\n")).contains("1ウォームアップ"));

    // No graphics, or no cell size: text.
    for detection in [
        GraphicsDetection {
            mode: GraphicsMode::Off,
            cell_pixels: Some(CellPixels::new(9, 20)),
            reason: GraphicsReason::Disabled,
        },
        GraphicsDetection {
            mode: GraphicsMode::Kitty,
            cell_pixels: None,
            reason: GraphicsReason::Explicit(GraphicsMode::Kitty),
        },
    ] {
        app.set_graphics_detection(detection);
        assert_eq!(layout_for(&app, area(158, 39)), TodayLayout::Text);
        let text = rows(&render_buffer(&app, 160, 45)).join("\n");
        assert!(compact(&text).contains("1ウォームアップ"), "{text}");
    }
}

fn opaque_pixels(image: &image::RgbaImage) -> usize {
    image.pixels().filter(|pixel| pixel.0[3] > 0).count()
}

#[test]
fn image_functions_draw_the_three_figures() {
    let (_directory, config) = seed(Lang::Ja);
    let app = open_today(config, GraphicsMode::Kitty);
    let data = &app.analysis_today;
    let theme = Theme::from_name("default");
    let font = resolve_big_font("biz-ud-gothic", std::path::Path::new("/unused"));
    let cell_pixels = CellPixels::new(9, 20);
    let save = |name: &str, image: &image::RgbaImage| {
        // Optional: write the figures for a visual check.
        if let Some(directory) = std::env::var_os("TODAY_TAB_PNG_DIR") {
            image.save(std::path::Path::new(&directory).join(name)).unwrap();
        }
    };

    // Steps: one heading and five rows; two done, the gate leveled up.
    let lines = steps_lines(&data.runs, Lang::Ja);
    assert_eq!(lines.len(), 6);
    let steps = lines.iter().filter_map(|line| match line {
        TodayStepsLine::Step(step) => Some(step),
        TodayStepsLine::Run(_) => None,
    });
    let done = steps.clone().filter(|step| step.done).count();
    // Every step names its language, prescription steps included.
    assert!(
        steps.clone().all(|step| step.label.ends_with("英語")),
        "{:?}",
        steps.clone().map(|step| &step.label).collect::<Vec<_>>()
    );
    assert_eq!(done, 2);
    assert!(steps.clone().any(|step| step.graduation.as_deref() == Some("レベル 2 へ")));
    let spec = rs_typing_trainer::ui::analysis_image::TodayStepsSpec {
        lines,
        not_played: "未".to_owned(),
        cells: Size::new(150, 6),
        colors: rs_typing_trainer::ui::today::image_colors(&theme),
    };
    let image = render_today_steps(&font, cell_pixels, &spec).unwrap();
    assert_eq!((image.width(), image.height()), (150 * 9, 6 * 20));
    assert!(opaque_pixels(&image) > 1_000);
    save("today-steps.png", &image);

    // Timeline: four sessions (two menu steps, one individual, one bench).
    let timeline = timeline_spec(data, Lang::Ja, &theme, Size::new(100, 20));
    assert_eq!(timeline.points.len(), 4);
    // The time range keeps a margin (5%, at least 10 minutes) around the
    // sessions, so the first and last markers stay off the edges.
    let (first, last) = today_timeline_range(&timeline.points);
    let start = timeline.points.iter().map(|point| point.start_minute).fold(f64::INFINITY, f64::min);
    let end = timeline.points.iter().map(|point| point.end_minute).fold(f64::NEG_INFINITY, f64::max);
    assert!(start - first >= 10.0, "{first} .. {start}");
    assert!(last - end >= 10.0, "{end} .. {last}");
    assert!(last - first >= 60.0);
    // A single short session still gets an hour around it.
    let (one_first, one_last) = today_timeline_range(&timeline.points[..1]);
    assert!(timeline.points[0].start_minute - one_first >= 10.0 && one_last - one_first >= 60.0);
    let image = render_today_timeline(&font, cell_pixels, &timeline).unwrap();
    // No marker pixels in the leftmost plot column (6 cells of labels).
    let left = 6 * 9;
    let edge = (0..image.height() - 30).filter(|y| image.get_pixel(left + 1, *y).0[3] > 200).count();
    assert!(edge < 40, "markers touch the left edge: {edge}");
    assert!(opaque_pixels(&image) > 1_000);
    save("today-timeline.png", &image);

    // Cards: time, sessions, keystrokes, and net WPM and accuracy per language.
    let cells = vec![Size::new(22, 7); 7];
    let cards = card_specs(data, Lang::Ja, &theme, &cells);
    assert_eq!(cards.len(), 7);
    assert_eq!(cards[1].value, "4");
    assert_eq!(cards[1].delta, Some(2.0));
    assert_eq!(cards[1].delta_text, "11/10 比 +2");
    for (index, card) in cards.iter().enumerate() {
        let image = render_today_card(&font, cell_pixels, card).unwrap();
        assert!(opaque_pixels(&image) > 1_000, "card {index}");
        save(&format!("today-card-{index}.png"), &image);
    }
}
