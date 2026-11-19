//! MYTASK-3630: long empty stretches are cut out of the trend tab's x axis,
//! and the analysis screen opens on the last 30 days.

use std::time::Duration;

use chrono::{NaiveDate, TimeZone, Utc};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Size;
use rs_typing_trainer::app::{AnalysisPeriod, Screen};
use rs_typing_trainer::config::Config;
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::graphics::{CellPixels, GraphicsDetection, GraphicsMode, GraphicsReason};
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::record::{SessionRecord, db};
use rs_typing_trainer::ui;
use rs_typing_trainer::ui::analysis_image::{
    MetricKind, SERIES_COLORS, TREND_BREAK_WIDTH_DAYS, TrendAxis, TrendDateRange, build_trend_data, render_trend_chart_with_range, shared_trend_axis,
};
use tempfile::TempDir;

use crate::common;

fn ymd(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

fn record(date: NaiveDate, wpm: f64) -> SessionRecord {
    let config = Config::default();
    let mut session = Session::new(Duration::from_secs(1), Duration::ZERO, Item::exact("as"));
    session.activate(Duration::ZERO);
    session.input_char(Duration::from_millis(100), "a");
    session.input_char(Duration::from_millis(200), "s");
    session.tick(Duration::from_secs(1));
    let finished = Utc.from_utc_datetime(&date.and_hms_opt(12, 0, 0).unwrap());
    let mut record = SessionRecord::from_log(&config, session.into_log(), finished);
    record.header.net_wpm = wpm;
    record
}

/// Three days at the end of April and four days in November (no gap inside
/// November longer than 14 days), as the imported history looks.
fn april_and_november() -> Vec<SessionRecord> {
    [
        (ymd(2026, 4, 27), 30.0),
        (ymd(2026, 4, 28), 32.0),
        (ymd(2026, 4, 30), 31.0),
        (ymd(2026, 11, 1), 40.0),
        (ymd(2026, 11, 5), 44.0),
        (ymd(2026, 11, 10), 41.0),
        (ymd(2026, 11, 15), 47.0),
    ]
    .into_iter()
    .map(|(date, wpm)| record(date, wpm))
    .collect()
}

fn trend_app(records: Vec<SessionRecord>) -> common::TestApp {
    let mut app = common::test_app(Config::default());
    app.screen = Screen::Analysis;
    app.analysis_tab = rs_typing_trainer::app::ANALYSIS_TREND_TAB;
    app.analysis_records = records;
    // The records are set directly; show the period they stand for.
    app.analysis_period = AnalysisPeriod::All;
    app
}

fn render_buffer(app: &rs_typing_trainer::app::App, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    terminal.backend().buffer().clone()
}

fn rows(buffer: &Buffer) -> Vec<String> {
    let width = buffer.area.width as usize;
    buffer
        .content()
        .chunks(width)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect()
}

fn cell(buffer: &Buffer, x: u16, y: u16) -> &str {
    buffer.cell((x, y)).unwrap().symbol()
}

fn is_braille(symbol: &str) -> bool {
    symbol.chars().next().is_some_and(|ch| ('\u{2801}'..='\u{28FF}').contains(&ch))
}

/// Graph columns of the first (net WPM) text chart: from the column after the
/// axis corner to the right edge, and the rows above the x axis.
fn first_chart_graph(buffer: &Buffer) -> (u16, u16, u16, u16) {
    let area = buffer.area;
    let (corner_x, axis_row) = (0..area.height)
        .flat_map(|y| (0..area.width).map(move |x| (x, y)))
        .find(|&(x, y)| cell(buffer, x, y) == "└" && (x + 1..area.width - 1).all(|right| matches!(cell(buffer, right, y), "─" | "≈")))
        .expect("x/y axis corner");
    let top = (0..axis_row).rev().find(|&y| cell(buffer, corner_x, y) != "│").map_or(0, |y| y + 1);
    // `area.width - 1` is the screen's outer border.
    (corner_x + 1, area.width - 1, top, axis_row)
}

fn braille_columns(buffer: &Buffer, left: u16, right: u16, top: u16, bottom: u16) -> Vec<u16> {
    (left..right).filter(|&x| (top..bottom).any(|y| is_braille(cell(buffer, x, y)))).collect()
}

#[test]
fn trend_axis_cuts_only_gaps_longer_than_fourteen_days() {
    let axis = TrendAxis::from_dates([ymd(2026, 1, 1), ymd(2026, 1, 15), ymd(2026, 1, 29)]).unwrap();
    assert!(!axis.has_breaks(), "gaps of exactly 14 days are kept: {axis:?}");
    assert_eq!(axis.span(), 28.0);
    assert!(axis.break_fractions().is_empty());

    let axis = TrendAxis::from_dates([ymd(2026, 1, 1), ymd(2026, 1, 16)]).unwrap();
    assert!(axis.has_breaks(), "a 15-day gap is cut: {axis:?}");
    assert_eq!(axis.span(), TREND_BREAK_WIDTH_DAYS as f64);

    let axis = TrendAxis::from_dates(
        april_and_november()
            .iter()
            .map(|record| chrono::DateTime::parse_from_rfc3339(&record.header.finished_at).unwrap().date_naive()),
    )
    .unwrap();
    assert_eq!(
        axis.segments(),
        [
            TrendDateRange {
                start: ymd(2026, 4, 27),
                end: ymd(2026, 4, 30),
            },
            TrendDateRange {
                start: ymd(2026, 11, 1),
                end: ymd(2026, 11, 15),
            },
        ]
    );
    // 3 days + the squeezed gap + 14 days.
    assert_eq!(axis.span(), 3.0 + TREND_BREAK_WIDTH_DAYS as f64 + 14.0);
    assert_eq!(axis.position(ymd(2026, 11, 1)), 3.0 + TREND_BREAK_WIDTH_DAYS as f64);
    assert!(axis.crosses_break(ymd(2026, 4, 30), ymd(2026, 11, 1)));
    assert!(!axis.crosses_break(ymd(2026, 11, 1), ymd(2026, 11, 15)));
    let breaks = axis.break_fractions();
    assert_eq!(breaks.len(), 1);
    assert!(axis.fraction(ymd(2026, 4, 30)) < breaks[0] && breaks[0] < axis.fraction(ymd(2026, 11, 1)));
    // A date inside the cut gap lands inside the squeezed gap.
    let inside = axis.position(ymd(2026, 8, 1));
    assert!(3.0 < inside && inside < 3.0 + TREND_BREAK_WIDTH_DAYS as f64, "{inside}");
    // The real calendar would give November less than a tenth of the width.
    assert!(axis.fraction(ymd(2026, 11, 15)) - axis.fraction(ymd(2026, 11, 1)) > 0.5);
}

#[test]
fn text_trend_cuts_the_gap_marks_it_and_spreads_november_over_half_the_width() {
    let app = trend_app(april_and_november());
    let buffer = render_buffer(&app, 120, 40);
    let text = rows(&buffer).join("\n");
    assert!(text.contains('≈'), "{text}");
    assert!(text.contains("2026-04-27"), "{text}");
    assert!(text.contains("2026-11-15"), "{text}");
    assert!(text.contains("2026-11-01"), "the first date after the break is labelled: {text}");

    let (left, right, top, axis_row) = first_chart_graph(&buffer);
    let mark = (left..right).find(|&x| cell(&buffer, x, axis_row) == "≈").expect("≈ on the x axis");
    let width = right - left;
    let november = braille_columns(&buffer, mark + 1, right, top, axis_row);
    let spread = november.last().unwrap() - november.first().unwrap();
    assert!(spread * 2 >= width, "November spreads over {spread} of {width} columns:\n{text}");

    // No line across the break: the columns between the last April day and
    // the first November day stay empty (one column of slack each side).
    let april_end = *braille_columns(&buffer, left, mark, top, axis_row).last().unwrap();
    let november_start = *november.first().unwrap();
    assert!(november_start > april_end + 3, "{april_end}..{november_start}");
    assert!(
        braille_columns(&buffer, april_end + 2, november_start - 1, top, axis_row).is_empty(),
        "a line was drawn across the break:\n{text}"
    );
}

#[test]
fn text_trend_without_a_long_gap_has_no_break_mark() {
    let records = [(1, 30.0), (8, 35.0), (22, 33.0)]
        .into_iter()
        .map(|(day, wpm)| record(ymd(2026, 11, day), wpm))
        .collect();
    let app = trend_app(records);
    let text = rows(&render_buffer(&app, 120, 40)).join("\n");
    assert!(!text.contains('≈'), "{text}");
    assert!(text.contains("2026-11-01") && text.contains("2026-11-22"), "{text}");
}

#[test]
fn image_trend_cuts_the_gap_and_keeps_the_line_off_it() {
    let records = april_and_november();
    let mut spec = build_trend_data(&records, MetricKind::NetWpm, Lang::En).unwrap();
    spec.cells = Size::new(60, 8);
    let axis = shared_trend_axis(std::iter::once(&spec)).unwrap();
    assert!(axis.has_breaks());
    let image = render_trend_chart_with_range(CellPixels::new(4, 4), &spec, axis.clone()).unwrap();
    let color = match SERIES_COLORS[0] {
        ratatui::style::Color::Cyan => [17, 168, 205, 255],
        other => panic!("unexpected series colour {other:?}"),
    };
    let columns = (0..image.width())
        .filter(|&x| (0..image.height()).any(|y| image.get_pixel(x, y).0 == color))
        .collect::<Vec<_>>();
    let left = 2.0;
    let right = image.width() as f64 - 3.0;
    let x = |date| (left + axis.fraction(date) * (right - left)) as u32;
    let (april_end, november_start, november_end) = (x(ymd(2026, 4, 30)), x(ymd(2026, 11, 1)), x(ymd(2026, 11, 15)));
    assert!(
        (november_end - november_start) * 2 >= image.width(),
        "November spreads over {}..{} of {}",
        november_start,
        november_end,
        image.width()
    );
    assert!(columns.iter().any(|&column| column + 3 >= november_end));
    assert!(
        columns.iter().all(|&column| column <= april_end + 4 || column + 4 >= november_start),
        "series pixels inside the cut gap ({april_end}..{november_start}): {columns:?}"
    );
    // The break itself is drawn (grid-coloured strokes in the gap).
    let middle = x(ymd(2026, 8, 1));
    assert!(
        (middle.saturating_sub(3)..=middle + 3).any(|column| (0..image.height()).any(|y| image.get_pixel(column, y).0[3] > 0)),
        "no break mark in the gap"
    );
}

#[test]
fn image_trend_shows_the_break_mark_as_terminal_text() {
    let mut app = trend_app(april_and_november());
    app.set_graphics_detection(GraphicsDetection {
        mode: GraphicsMode::Kitty,
        cell_pixels: Some(CellPixels::new(9, 20)),
        reason: GraphicsReason::Explicit(GraphicsMode::Kitty),
    });
    let text = rows(&render_buffer(&app, 120, 40)).join("\n");
    assert_eq!(text.matches('≈').count(), 3, "one mark under each of the three charts:\n{text}");
    assert!(text.contains("2026-04-27") && text.contains("2026-11-15"), "{text}");
}

fn seeded_app(dates: &[NaiveDate], today: NaiveDate) -> (TempDir, common::TestApp) {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    for (index, date) in dates.iter().enumerate() {
        db::save(&path, &record(*date, 30.0 + index as f64)).unwrap();
    }
    let mut config = Config::default();
    config.storage.db_path = Some(path);
    let app = common::test_app_at(config, today);
    (directory, app)
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn analysis_opens_on_thirty_days_and_p_reaches_the_whole_period() {
    let dates = [ymd(2026, 4, 27), ymd(2026, 4, 28), ymd(2026, 11, 10), ymd(2026, 11, 15)];
    let (_directory, mut app) = seeded_app(&dates, ymd(2026, 11, 19));
    app.handle_key(key(KeyCode::Char('a')));
    assert_eq!(app.screen, Screen::Analysis);
    assert_eq!(app.analysis_period, AnalysisPeriod::Days30);
    assert_eq!(app.analysis_records.len(), 2, "30 days: only November");
    let header = rows(&render_buffer(&app, 120, 40)).join("\n");
    assert!(header.contains("30 days") || header.contains("30d"), "{header}");

    app.handle_key(key(KeyCode::Char('p')));
    assert_eq!(app.analysis_period, AnalysisPeriod::Days7);
    app.handle_key(key(KeyCode::Char('p')));
    assert_eq!(app.analysis_period, AnalysisPeriod::All);
    assert_eq!(app.analysis_records.len(), 4);
    app.analysis_tab = rs_typing_trainer::app::ANALYSIS_TREND_TAB;
    let text = rows(&render_buffer(&app, 120, 40)).join("\n");
    assert!(text.contains('≈'), "{text}");

    // Leaving and opening again starts from 30 days again.
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(key(KeyCode::Char('a')));
    assert_eq!(app.analysis_period, AnalysisPeriod::Days30);
}

#[test]
fn thirty_days_with_one_or_no_recorded_day_keeps_the_existing_messages() {
    // One day inside the last 30 days: the «two days» message and the value.
    let (_directory, mut app) = seeded_app(&[ymd(2026, 4, 27), ymd(2026, 11, 15)], ymd(2026, 11, 19));
    app.handle_key(key(KeyCode::Char('a')));
    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.analysis_tab, rs_typing_trainer::app::ANALYSIS_TREND_TAB);
    let text = rows(&render_buffer(&app, 80, 24)).join("\n");
    assert!(text.contains("2 days") || text.contains("two days"), "{text}");
    assert!(text.contains("31.0"), "{text}");
    assert!(!text.contains('≈'), "{text}");

    // Nothing inside the last 30 days.
    let (_directory, mut app) = seeded_app(&[ymd(2026, 4, 27), ymd(2026, 4, 28)], ymd(2026, 11, 19));
    app.handle_key(key(KeyCode::Char('a')));
    assert!(app.analysis_records.is_empty());
    for tab in 0..rs_typing_trainer::app::ANALYSIS_TAB_COUNT {
        app.analysis_tab = tab;
        let text = rows(&render_buffer(&app, 80, 24)).join("\n");
        assert!(!text.contains('≈'), "tab {tab}: {text}");
    }
}

/// Prints the text trend screen at the two reference sizes (and Japanese at 80×24) (shown with
/// `cargo test --test all analysis_trend_breaks::trend_samples -- --nocapture`).
#[test]
fn trend_samples_at_80x24_and_160x45() {
    let mut app = trend_app(april_and_november());
    for (lang, width, height) in [(Lang::En, 80, 24), (Lang::En, 160, 45), (Lang::Ja, 80, 24)] {
        app.config.general.lang = lang;
        let text = rows(&render_buffer(&app, width, height)).join("\n");
        // One mark on each chart's x axis (the row of its `└───` corner),
        // never inside the legend box.
        let axis_marks = text
            .lines()
            .filter(|row| row.contains('≈') && row.contains('└') && !row.contains('┘'))
            .count();
        assert_eq!(axis_marks, 3, "{width}x{height}:\n{text}");
        assert_eq!(text.matches('≈').count(), 3, "{width}x{height}:\n{text}");
        assert!(text.contains("2026-11-01"), "{width}x{height}:\n{text}");
        println!("--- {lang:?} {width}x{height}\n{text}");
    }
}
