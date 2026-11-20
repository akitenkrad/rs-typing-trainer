//! MYTASK-3642: the trend tab draws every benchmark run as one «bench» series,
//! and labels the x axis with tick dates (`MM/DD`) between the endpoints, with
//! a faint vertical grid line at each tick on the image chart.

use std::time::Duration;

use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Size;
use rs_typing_trainer::app::{AnalysisPeriod, Screen};
use rs_typing_trainer::config::Config;
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::graphics::{CellPixels, GraphicsDetection, GraphicsMode, GraphicsReason};
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::record::SessionRecord;
use rs_typing_trainer::ui;
use rs_typing_trainer::ui::analysis_image::{
    AnalysisImageCache, MetricKind, TrendAxis, TrendTickStep, build_trend_data, render_trend_chart_with_range, shared_trend_axis,
    trend_tick_candidates,
};

use crate::common;

fn ymd(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

fn record(date: NaiveDate, wpm: f64, bench_id: Option<&str>) -> SessionRecord {
    let config = Config::default();
    let mut session = Session::new(Duration::from_secs(1), Duration::ZERO, Item::exact("as"));
    session.activate(Duration::ZERO);
    session.input_char(Duration::from_millis(100), "a");
    session.input_char(Duration::from_millis(200), "s");
    session.tick(Duration::from_secs(1));
    let finished = Utc.from_utc_datetime(&date.and_hms_opt(12, 0, 0).unwrap());
    let mut record = SessionRecord::from_log(&config, session.into_log(), finished);
    record.header.net_wpm = wpm;
    if let Some(id) = bench_id {
        record.header.mode = "bench".into();
        record.header.bench_id = Some(id.into());
    }
    record
}

/// Benchmark runs on three days across both versions and all four parts,
/// plus ordinary practice.
fn bench_records() -> Vec<SessionRecord> {
    let mut records = Vec::new();
    for (day, parts) in [
        (1, vec![("bench-v1/A/01", 20.0), ("bench-v1/B/01", 60.0), ("bench-v2/C/01", 40.0)]),
        (2, vec![("bench-v2/A/01", 50.0), ("bench-v2/D/01", 30.0), ("bench-v1/C/02", 10.0)]),
        (3, vec![("bench-v2/B/02", 45.0)]),
    ] {
        for (id, wpm) in parts {
            records.push(record(ymd(2026, 11, day), wpm, Some(id)));
        }
        records.push(record(ymd(2026, 11, day), 33.0, None));
    }
    records
}

/// Records on every day from 10/21 to 11/19 (30 days, no cut gap).
fn thirty_days(spike: Option<NaiveDate>) -> Vec<SessionRecord> {
    ymd(2026, 10, 21)
        .iter_days()
        .take(30)
        .map(|date| {
            let wpm = if Some(date) == spike { 80.0 } else { 30.0 + f64::from(date.day() % 3) };
            record(date, wpm, None)
        })
        .collect()
}

/// Three days at the end of April and four days in November.
fn april_and_november() -> Vec<SessionRecord> {
    [
        ymd(2026, 4, 27),
        ymd(2026, 4, 28),
        ymd(2026, 4, 30),
        ymd(2026, 11, 1),
        ymd(2026, 11, 5),
        ymd(2026, 11, 10),
        ymd(2026, 11, 15),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, date)| record(date, 30.0 + index as f64, None))
    .collect()
}

fn trend_app(records: Vec<SessionRecord>, image: bool) -> common::TestApp {
    let mut app = common::test_app(Config::default());
    app.screen = Screen::Analysis;
    app.analysis_tab = rs_typing_trainer::app::ANALYSIS_TREND_TAB;
    app.analysis_records = records;
    app.analysis_period = AnalysisPeriod::All;
    if image {
        app.set_graphics_detection(GraphicsDetection {
            mode: GraphicsMode::Kitty,
            cell_pixels: Some(CellPixels::new(9, 20)),
            reason: GraphicsReason::Explicit(GraphicsMode::Kitty),
        });
    }
    app
}

fn render_buffer(app: &rs_typing_trainer::app::App, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    terminal.backend().buffer().clone()
}

fn cell(buffer: &Buffer, x: u16, y: u16) -> &str {
    buffer.cell((x, y)).unwrap().symbol()
}

/// One row as a plain string, wide image-protocol cells reduced to their
/// first character so that the string index is the column.
fn row_text(buffer: &Buffer, y: u16) -> Vec<char> {
    (0..buffer.area.width).map(|x| cell(buffer, x, y).chars().next().unwrap_or(' ')).collect()
}

fn rows(buffer: &Buffer) -> Vec<String> {
    (0..buffer.area.height).map(|y| row_text(buffer, y).into_iter().collect()).collect()
}

fn compact(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn is_braille(symbol: &str) -> bool {
    symbol.chars().next().is_some_and(|ch| ('\u{2801}'..='\u{28FF}').contains(&ch))
}

/// `MM/DD` tick labels on a row (not part of a `YYYY/MM/DD` endpoint date):
/// (centre column, month, day).
fn tick_labels(row: &[char]) -> Vec<(u16, u32, u32)> {
    let mut labels = Vec::new();
    for start in 0..row.len().saturating_sub(4) {
        let slice = &row[start..start + 5];
        let digits = |range: std::ops::Range<usize>| slice[range].iter().all(char::is_ascii_digit);
        if !(digits(0..2) && slice[2] == '/' && digits(3..5)) {
            continue;
        }
        let before = start.checked_sub(1).map(|index| row[index]);
        let after = row.get(start + 5).copied();
        if before.is_some_and(|ch| ch.is_ascii_digit() || ch == '/') || after.is_some_and(|ch| ch.is_ascii_digit() || ch == '/') {
            continue;
        }
        let number = |range: std::ops::Range<usize>| slice[range].iter().collect::<String>().parse::<u32>().unwrap();
        labels.push(((start + 2) as u16, number(0..2), number(3..5)));
    }
    labels
}

/// Graph of the first (net WPM) text chart: (left, right, top, axis row).
fn first_text_graph(buffer: &Buffer) -> (u16, u16, u16, u16) {
    let area = buffer.area;
    let (corner_x, axis_row) = (0..area.height)
        .flat_map(|y| (0..area.width).map(move |x| (x, y)))
        .find(|&(x, y)| cell(buffer, x, y) == "└" && (x + 1..area.width - 1).all(|right| matches!(cell(buffer, right, y), "─" | "≈")))
        .expect("x/y axis corner");
    let top = (0..axis_row).rev().find(|&y| cell(buffer, corner_x, y) != "│").map_or(0, |y| y + 1);
    (corner_x + 1, area.width - 1, top, axis_row)
}

/// Label row of the first image chart: the row with the `YYYY/MM/DD` start
/// date; returns (row, first plot column, plot width).
fn first_image_label_row(buffer: &Buffer, start: NaiveDate) -> (u16, u16, u16) {
    let needle = start.format("%Y/%m/%d").to_string();
    let end = buffer.area.width - 1;
    for y in 0..buffer.area.height {
        let text = row_text(buffer, y).into_iter().collect::<String>();
        if let Some(index) = text.find(&needle) {
            // The label row spans the plot: from the start date to the
            // column before the screen's right border.
            let left = text[..index].chars().count() as u16;
            return (y, left, end - left);
        }
    }
    panic!("no start date {needle}:\n{}", rows(buffer).join("\n"));
}

#[test]
fn bench_runs_of_every_version_and_part_form_one_series_of_daily_medians() {
    let records = bench_records();
    let spec = build_trend_data(&records, MetricKind::NetWpm, Lang::En).unwrap();
    let bench = spec.series.iter().filter(|series| series.name.contains("bench")).collect::<Vec<_>>();
    assert_eq!(bench.len(), 1, "{:?}", spec.series.iter().map(|series| &series.name).collect::<Vec<_>>());
    assert_eq!(bench[0].name, "bench");
    let values = bench[0].points.iter().map(|point| (point.date.day(), point.value)).collect::<Vec<_>>();
    // Day 1: median of 20, 60, 40; day 2: of 50, 30, 10; day 3: one run.
    assert_eq!(values, [(1, 40.0), (2, 30.0), (3, 45.0)]);
    assert!(spec.series.iter().any(|series| series.name == "random"));
    assert_eq!(spec.series.len(), 2);

    let japanese = build_trend_data(&records, MetricKind::NetWpm, Lang::Ja).unwrap();
    assert_eq!(japanese.series.iter().filter(|series| series.name == "ベンチ").count(), 1);
    assert!(japanese.series.iter().all(|series| !series.name.contains("bench")));

    // The text chart's legend (graphics off) and the image legend show it once.
    for image in [false, true] {
        let app = trend_app(records.clone(), image);
        // Tall enough for the text chart to show its legend.
        let text = rows(&render_buffer(&app, 160, 60)).join("\n");
        assert!(!text.contains("bench-v"), "image={image}:\n{text}");
        let first_chart = text.split("accuracy %").next().unwrap();
        assert_eq!(compact(first_chart).matches("bench").count(), 1, "image={image}:\n{text}");
    }
}

#[test]
fn tick_step_is_the_smallest_whose_labels_fit() {
    assert_eq!(TrendTickStep::choose(7.0), Some(TrendTickStep::Days(1)));
    assert_eq!(TrendTickStep::choose(6.9), Some(TrendTickStep::Days(2)));
    assert_eq!(TrendTickStep::choose(3.5), Some(TrendTickStep::Days(2)));
    assert_eq!(TrendTickStep::choose(3.4), Some(TrendTickStep::Weeks(1)));
    assert_eq!(TrendTickStep::choose(0.5), Some(TrendTickStep::Weeks(2)));
    assert_eq!(TrendTickStep::choose(0.25), Some(TrendTickStep::Months(1)));
    assert_eq!(TrendTickStep::choose(0.0001), None);

    let axis = TrendAxis::from(rs_typing_trainer::ui::analysis_image::TrendDateRange {
        start: ymd(2026, 10, 21),
        end: ymd(2026, 11, 19),
    });
    // Weekly ticks fall on Mondays, monthly ticks on the first.
    let weekly = trend_tick_candidates(&axis, 2.0);
    assert_eq!(weekly, [ymd(2026, 10, 26), ymd(2026, 11, 2), ymd(2026, 11, 9), ymd(2026, 11, 16)]);
    assert_eq!(trend_tick_candidates(&axis, 0.25), [ymd(2026, 11, 1)]);
    let every_other_day = trend_tick_candidates(&axis, 4.0);
    assert_eq!(every_other_day.first(), Some(&ymd(2026, 10, 21)));
    assert!(every_other_day.windows(2).all(|pair| (pair[1] - pair[0]).num_days() == 2));
}

#[test]
fn text_trend_over_thirty_days_labels_dates_between_the_endpoints_under_their_points() {
    for (width, height) in [(80, 24), (160, 50)] {
        let app = trend_app(thirty_days(None), false);
        let buffer = render_buffer(&app, width, height);
        let text = rows(&buffer).join("\n");
        let (_, _, _, axis_row) = first_text_graph(&buffer);
        let label_row = row_text(&buffer, axis_row + 1);
        let label_text = label_row.iter().collect::<String>();
        assert!(label_text.contains("2026/10/21"), "{width}x{height}:\n{text}");
        assert!(label_text.contains("11/19"), "{width}x{height}:\n{text}");
        let ticks = tick_labels(&label_row)
            .into_iter()
            .filter(|&(_, month, day)| (month, day) != (11, 19))
            .collect::<Vec<_>>();
        assert!(ticks.len() >= 2, "{width}x{height}: ticks {ticks:?}\n{text}");

        // Each tick sits over its own day's point: a spike on that day peaks
        // in the label's column (±1).
        for &(column, month, day) in &ticks {
            let date = ymd(2026, month, day);
            let app = trend_app(thirty_days(Some(date)), false);
            let buffer = render_buffer(&app, width, height);
            let (left, right, top, _) = first_text_graph(&buffer);
            let peak = (left..right).filter(|&x| is_braille(cell(&buffer, x, top))).collect::<Vec<_>>();
            assert!(!peak.is_empty(), "{width}x{height}: no point on the top row");
            assert!(
                peak.iter().any(|&x| x.abs_diff(column) <= 1),
                "{width}x{height}: tick {month:02}/{day:02} at column {column}, peak at {peak:?}\n{}",
                rows(&buffer).join("\n")
            );
        }
    }
}

#[test]
fn image_trend_over_thirty_days_labels_ticks_where_the_image_places_their_day() {
    let records = thirty_days(None);
    for (width, height) in [(80, 24), (160, 50)] {
        let app = trend_app(records.clone(), true);
        let buffer = render_buffer(&app, width, height);
        let text = rows(&buffer).join("\n");
        let (row, left, plot_width) = first_image_label_row(&buffer, ymd(2026, 10, 21));
        let label_row = row_text(&buffer, row);
        let ticks = tick_labels(&label_row)
            .into_iter()
            .filter(|&(_, month, day)| (month, day) != (11, 19))
            .collect::<Vec<_>>();
        assert!(ticks.len() >= 2, "{width}x{height}: ticks {ticks:?}\n{text}");
        let spec = build_trend_data(&records, MetricKind::NetWpm, Lang::En).unwrap();
        let axis = shared_trend_axis(std::iter::once(&spec)).unwrap();
        for &(column, month, day) in &ticks {
            let fraction = axis.fraction(ymd(2026, month, day));
            let expected = left + (fraction * f64::from(plot_width)).floor() as u16;
            assert!(
                column.abs_diff(expected) <= 1,
                "{width}x{height}: {month:02}/{day:02} at {column}, its day at {expected}\n{text}"
            );
        }
    }
}

#[test]
fn image_trend_draws_a_vertical_grid_line_at_each_tick() {
    let records = thirty_days(None);
    let mut spec = build_trend_data(&records, MetricKind::NetWpm, Lang::En).unwrap();
    spec.cells = Size::new(60, 8);
    let axis = shared_trend_axis(std::iter::once(&spec)).unwrap();
    let plain = render_trend_chart_with_range(CellPixels::new(4, 4), &spec, axis.clone()).unwrap();
    let tick = ymd(2026, 11, 2);
    spec.x_ticks = vec![tick];
    let image = render_trend_chart_with_range(CellPixels::new(4, 4), &spec, axis.clone()).unwrap();
    let left = 2.0;
    let right = image.width() as f64 - 3.0;
    let x = (left + axis.fraction(tick) * (right - left)).floor() as u32;
    // Rows between the horizontal grid lines and away from the series line:
    // the tick column has faint grid pixels there and the plain image does not.
    let changed = (0..image.height()).filter(|&y| image.get_pixel(x, y) != plain.get_pixel(x, y)).count() as u32;
    assert!(
        changed * 2 > image.height(),
        "only {changed} of {} rows changed at the tick",
        image.height()
    );
    let pixel = (0..image.height())
        .map(|y| image.get_pixel(x, y).0)
        .find(|pixel| pixel[3] > 0 && pixel[0].abs_diff(130) <= 4 && pixel[1].abs_diff(140) <= 4 && pixel[2].abs_diff(155) <= 4)
        .expect("a grid-coloured pixel");
    assert!(pixel[3] <= 80, "the grid line must stay faint: {pixel:?}");
    // Elsewhere the images are the same.
    let far = (left + axis.fraction(ymd(2026, 10, 25)) * (right - left)) as u32;
    assert!((0..image.height()).all(|y| image.get_pixel(far, y) == plain.get_pixel(far, y)));

    // The ticks are part of the image cache key.
    let mut cache = AnalysisImageCache::new();
    let detection = GraphicsDetection {
        mode: GraphicsMode::Kitty,
        cell_pixels: Some(CellPixels::new(9, 20)),
        reason: GraphicsReason::Explicit(GraphicsMode::Kitty),
    };
    cache.prepare_chart_with_range(&detection, &spec, axis.clone()).unwrap();
    cache.prepare_chart_with_range(&detection, &spec, axis.clone()).unwrap();
    assert_eq!(cache.creation_count(), 1);
    spec.x_ticks = vec![ymd(2026, 11, 9)];
    cache.prepare_chart_with_range(&detection, &spec, axis).unwrap();
    assert_eq!(cache.creation_count(), 2);
}

#[test]
fn ticks_stay_out_of_a_cut_gap() {
    let records = april_and_november();
    let spec = build_trend_data(&records, MetricKind::NetWpm, Lang::En).unwrap();
    let axis = shared_trend_axis(std::iter::once(&spec)).unwrap();
    assert!(axis.has_breaks());
    let inside = |date: NaiveDate| axis.segments().iter().any(|segment| segment.start <= date && date <= segment.end);
    for columns_per_day in [0.3, 1.0, 2.0, 3.6, 5.0, 8.0] {
        let ticks = trend_tick_candidates(&axis, columns_per_day);
        assert!(ticks.iter().all(|&date| inside(date)), "{columns_per_day}: {ticks:?}");
    }

    for (image, width, height) in [(false, 80, 24), (false, 160, 50), (true, 80, 24), (true, 160, 50)] {
        let app = trend_app(records.clone(), image);
        let buffer = render_buffer(&app, width, height);
        let text = rows(&buffer).join("\n");
        assert!(text.contains('≈'), "{text}");
        let labels = rows(&buffer)
            .iter()
            .flat_map(|row| tick_labels(&row.chars().collect::<Vec<_>>()))
            .collect::<Vec<_>>();
        assert!(!labels.is_empty(), "image={image} {width}x{height}:\n{text}");
        for (_, month, day) in labels {
            assert!(
                inside(ymd(2026, month, day)),
                "image={image} {width}x{height}: {month:02}/{day:02}\n{text}"
            );
        }
    }
}

/// Prints the text trend tab over 30 days at 80×24 and 160×50, and writes the
/// image chart as a PNG when `TREND_PNG` names a file (shown with
/// `cargo test --test all analysis_trend_ticks::trend_tick_samples -- --nocapture`).
#[test]
fn trend_tick_samples() {
    let mut records = thirty_days(None);
    records.extend(
        [
            (ymd(2026, 10, 24), "bench-v1/A/01"),
            (ymd(2026, 11, 4), "bench-v2/B/01"),
            (ymd(2026, 11, 14), "bench-v2/A/02"),
        ]
        .into_iter()
        .map(|(date, id)| record(date, 45.0, Some(id))),
    );
    for (width, height) in [(80, 24), (160, 50)] {
        let app = trend_app(records.clone(), false);
        println!("--- {width}x{height}\n{}", rows(&render_buffer(&app, width, height)).join("\n"));
    }
    let app = trend_app(records.clone(), true);
    println!("--- image 160x50\n{}", rows(&render_buffer(&app, 160, 50)).join("\n"));
    if let Some(path) = std::env::var_os("TREND_PNG") {
        let mut spec = build_trend_data(&records, MetricKind::NetWpm, Lang::En).unwrap();
        spec.cells = Size::new(150, 14);
        let axis = shared_trend_axis(std::iter::once(&spec)).unwrap();
        let (start, end) = (axis.start(), axis.end());
        spec.x_ticks = trend_tick_candidates(&axis, 150.0 / axis.span())
            .into_iter()
            .filter(|date| start < *date && *date < end)
            .collect();
        let image = render_trend_chart_with_range(CellPixels::new(9, 20), &spec, axis).unwrap();
        image.save(path).unwrap();
    }
}
