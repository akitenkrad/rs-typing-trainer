use std::path::Path;
use std::time::Duration;

use chrono::{NaiveDate, TimeZone, Utc};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Size;
use ratatui::style::Color;
use rs_typing_trainer::analysis::grades::Level;
use rs_typing_trainer::config::Config;
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::graphics::{CellPixels, GraphicsDetection, GraphicsMode, GraphicsReason};
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::record::SessionRecord;
use rs_typing_trainer::ui;
use rs_typing_trainer::ui::analysis_image::{
    AnalysisImageCache, FORECAST_EN_COLOR, GradeBadgeSpec, MetricKind, SERIES_COLORS, TrendChartSpec, TrendDateRange, TrendForecast, TrendPoint,
    TrendSeries, build_trend_data, render_grade_badge, render_trend_chart, render_trend_chart_with_range, shared_trend_date_range,
};
use rs_typing_trainer::ui::bigtext_image::resolve_big_font;

mod common;

fn date(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 1, day).unwrap()
}

fn detection() -> GraphicsDetection {
    GraphicsDetection {
        mode: GraphicsMode::Kitty,
        cell_pixels: Some(CellPixels::new(9, 20)),
        reason: GraphicsReason::Explicit(GraphicsMode::Kitty),
    }
}

fn rgba(color: Color) -> [u8; 4] {
    match color {
        Color::LightYellow => [245, 245, 67, 255],
        Color::LightGreen => [144, 238, 144, 255],
        Color::Cyan => [17, 168, 205, 255],
        Color::Green => [13, 188, 121, 255],
        Color::Rgb(red, green, blue) => [red, green, blue, 255],
        other => panic!("test has no RGB mapping for {other:?}"),
    }
}

fn has_rgb_near(image: &image::RgbaImage, x: u32, y: u32, color: [u8; 4], radius: u32) -> bool {
    let x0 = x.saturating_sub(radius);
    let y0 = y.saturating_sub(radius);
    let x1 = (x + radius).min(image.width() - 1);
    let y1 = (y + radius).min(image.height() - 1);
    (y0..=y1).any(|row| (x0..=x1).any(|column| image.get_pixel(column, row).0 == color))
}

fn same_rgb_with_rounding(pixel: [u8; 4], color: [u8; 4]) -> bool {
    (0..3).all(|index| pixel[index].abs_diff(color[index]) <= 2)
}

#[test]
fn grade_badge_uses_grade_colour_for_frame_and_glyph_with_transparent_corners() {
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let spec = GradeBadgeSpec {
        level: Level::S,
        color: Color::LightYellow,
        cells: Size::new(16, 7),
    };
    let image = render_grade_badge(&font, CellPixels::new(9, 20), &spec).unwrap();
    let expected = rgba(Color::LightYellow);

    assert_eq!(image.get_pixel(image.width() / 2, 2).0, expected, "top frame");
    let inner_has_glyph =
        (image.height() / 4..image.height() * 3 / 4).any(|y| (image.width() / 4..image.width() * 3 / 4).any(|x| image.get_pixel(x, y).0 == expected));
    assert!(inner_has_glyph, "grade glyph has no grade-coloured pixel near the centre");
    for &(x, y) in &[
        (0, 0),
        (image.width() - 1, 0),
        (0, image.height() - 1),
        (image.width() - 1, image.height() - 1),
    ] {
        assert_eq!(image.get_pixel(x, y).0[3], 0, "corner ({x}, {y})");
    }

    let changed = render_grade_badge(
        &font,
        CellPixels::new(9, 20),
        &GradeBadgeSpec {
            level: Level::A,
            color: Color::LightGreen,
            cells: spec.cells,
        },
    )
    .unwrap();
    assert_eq!(changed.get_pixel(changed.width() / 2, 2).0, rgba(Color::LightGreen));
    assert_ne!(changed.get_pixel(changed.width() / 2, 2), image.get_pixel(image.width() / 2, 2));
}

fn calendar_chart(forecasts: Vec<TrendForecast>) -> TrendChartSpec {
    TrendChartSpec {
        kind: MetricKind::NetWpm,
        cells: Size::new(50, 8),
        series: vec![
            TrendSeries {
                name: "words".into(),
                color: SERIES_COLORS[0],
                points: vec![
                    TrendPoint { date: date(1), value: 20.0 },
                    TrendPoint { date: date(3), value: 30.0 },
                    TrendPoint { date: date(10), value: 40.0 },
                ],
            },
            TrendSeries {
                name: "bench-A".into(),
                color: SERIES_COLORS[1],
                points: vec![TrendPoint { date: date(1), value: 35.0 }, TrendPoint { date: date(10), value: 25.0 }],
            },
        ],
        forecasts,
    }
}

#[test]
fn trend_lines_use_series_colours_and_calendar_proportional_x_positions() {
    let spec = calendar_chart(Vec::new());
    let image = render_trend_chart(CellPixels::new(4, 4), &spec).unwrap();
    let left = 2.0;
    let right = image.width() as f64 - 3.0;
    let day3_x = (left + (right - left) * 2.0 / 9.0).round() as u32;
    let min_y = 20.0;
    let max_y = 40.0;
    let day3_y = (2.0 + (image.height() as f64 - 5.0) * (max_y - 30.0) / (max_y - min_y)).round() as u32;

    assert!(
        has_rgb_near(&image, day3_x, day3_y, rgba(SERIES_COLORS[0]), 3),
        "the third-day point is not at its calendar-proportional position"
    );
    let ordinal_x = (left + (right - left) / 2.0).round() as u32;
    assert!(day3_x + 10 < ordinal_x, "test must distinguish calendar spacing from ordinal spacing");
    assert!(
        image.pixels().any(|pixel| pixel.0 == rgba(SERIES_COLORS[1])),
        "second series colour is absent"
    );
}

#[test]
fn forecast_fan_is_present_only_inside_the_band_and_only_when_a_curve_exists() {
    let forecast = TrendForecast {
        target: "en".into(),
        start: TrendPoint { date: date(10), value: 40.0 },
        end_date: NaiveDate::from_ymd_opt(2026, 2, 7).unwrap(),
        forecast: 50.0,
        band_lo: 45.0,
        band_hi: 55.0,
        forecast_days: 28,
        color: FORECAST_EN_COLOR,
    };
    let mut spec = calendar_chart(vec![forecast]);
    let image = render_trend_chart(CellPixels::new(4, 4), &spec).unwrap();
    let x = image.width() - 3;
    let forecast_rgb = rgba(FORECAST_EN_COLOR);
    let band_pixels = (0..image.height())
        .filter(|&y| {
            let pixel = image.get_pixel(x, y).0;
            same_rgb_with_rounding(pixel, forecast_rgb) && (1..255).contains(&pixel[3])
        })
        .collect::<Vec<_>>();
    assert!(!band_pixels.is_empty(), "forecast column has no translucent band pixels");
    let first = *band_pixels.first().unwrap();
    let last = *band_pixels.last().unwrap();
    assert!(
        first > 1 && last + 2 < image.height(),
        "band escaped the forecast value range: {first}..={last}"
    );
    assert!(
        (0..first.saturating_sub(1)).all(|y| !same_rgb_with_rounding(image.get_pixel(x, y).0, forecast_rgb)),
        "forecast colour appears above band"
    );
    assert!(
        (last + 2..image.height()).all(|y| !same_rgb_with_rounding(image.get_pixel(x, y).0, forecast_rgb)),
        "forecast colour appears below band"
    );

    spec.forecasts.clear();
    let without_curve = render_trend_chart(CellPixels::new(4, 4), &spec).unwrap();
    assert!(
        without_curve.pixels().all(|pixel| !same_rgb_with_rounding(pixel.0, forecast_rgb)),
        "forecast pixels exist without a learning curve"
    );
}

#[test]
fn analysis_image_cache_reuses_equal_input_and_invalidates_when_a_value_changes() {
    let mut cache = AnalysisImageCache::new();
    let graphics = detection();
    let spec = calendar_chart(Vec::new());
    cache.prepare_chart(&graphics, &spec).unwrap();
    cache.prepare_chart(&graphics, &spec).unwrap();
    assert_eq!(cache.creation_count(), 1);

    let mut changed = spec.clone();
    changed.series[0].points[1].value += 0.5;
    cache.prepare_chart(&graphics, &changed).unwrap();
    assert_eq!(cache.creation_count(), 2);

    let extended_range = TrendDateRange {
        start: date(1),
        end: NaiveDate::from_ymd_opt(2026, 2, 7).unwrap(),
    };
    cache.prepare_chart_with_range(&graphics, &changed, extended_range).unwrap();
    assert_eq!(cache.creation_count(), 3);
}

fn record(day: u32, wpm: f64) -> SessionRecord {
    let config = Config::default();
    let mut session = Session::new(Duration::from_secs(1), Duration::ZERO, Item::exact("as"));
    session.activate(Duration::ZERO);
    session.input_char(Duration::from_millis(100), "a");
    session.input_char(Duration::from_millis(200), "s");
    session.tick(Duration::from_secs(1));
    let mut record = SessionRecord::from_log(&config, session.into_log(), Utc.with_ymd_and_hms(2026, 1, day, 0, 0, 0).unwrap());
    record.header.net_wpm = wpm;
    record
}

#[test]
fn three_trend_charts_share_the_forecast_extended_date_range() {
    let mut records = vec![record(1, 20.0), record(10, 30.0)];
    for record in &mut records {
        record.header.metrics_version = 1;
    }
    let mut specs = [MetricKind::NetWpm, MetricKind::Accuracy, MetricKind::Iki].map(|kind| build_trend_data(&records, kind, Lang::En).unwrap());
    for spec in &mut specs {
        spec.cells = Size::new(50, 8);
    }
    specs[0].forecasts.push(TrendForecast {
        target: "en".into(),
        start: TrendPoint { date: date(10), value: 30.0 },
        end_date: NaiveDate::from_ymd_opt(2026, 2, 7).unwrap(),
        forecast: 40.0,
        band_lo: 35.0,
        band_hi: 45.0,
        forecast_days: 28,
        color: FORECAST_EN_COLOR,
    });

    let range = shared_trend_date_range(&specs).unwrap();
    let images = specs
        .iter()
        .map(|spec| render_trend_chart_with_range(CellPixels::new(4, 4), spec, range).unwrap())
        .collect::<Vec<_>>();
    let last_record_columns = images
        .iter()
        .map(|image| {
            (0..image.width())
                .rev()
                .find(|&x| (0..image.height()).any(|y| image.get_pixel(x, y).0 == rgba(SERIES_COLORS[0])))
                .unwrap()
        })
        .collect::<Vec<_>>();

    assert_eq!(
        range,
        TrendDateRange {
            start: date(1),
            end: NaiveDate::from_ymd_opt(2026, 2, 7).unwrap(),
        }
    );
    assert!(last_record_columns.windows(2).all(|columns| columns[0] == columns[1]));
    assert!(
        last_record_columns[0] < images[0].width() / 2,
        "forecast must extend the range beyond the last record: columns={last_record_columns:?}, width={}",
        images[0].width()
    );
}

fn render_buffer(app: &rs_typing_trainer::app::App) -> ratatui::buffer::Buffer {
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    terminal.backend().buffer().clone()
}

#[test]
fn graphics_off_keeps_the_existing_analysis_text_buffer() {
    let mut baseline = common::test_app(Config::default());
    baseline.screen = rs_typing_trainer::app::Screen::Analysis;
    baseline.analysis_tab = 1;
    baseline.analysis_records = vec![record(1, 20.0), record(3, 30.0)];

    let mut explicit_off = common::test_app(Config::default());
    explicit_off.screen = rs_typing_trainer::app::Screen::Analysis;
    explicit_off.analysis_tab = 1;
    explicit_off.analysis_records = baseline.analysis_records.clone();
    explicit_off.config.display.big_font = "shinonome".into();
    explicit_off.set_graphics_detection(GraphicsDetection {
        mode: GraphicsMode::Off,
        cell_pixels: Some(CellPixels::new(9, 20)),
        reason: GraphicsReason::Disabled,
    });

    assert_eq!(render_buffer(&explicit_off), render_buffer(&baseline));
}

#[test]
fn image_chart_keeps_title_legend_and_both_endpoint_dates_as_terminal_text() {
    let mut app = common::test_app(Config::default());
    app.screen = rs_typing_trainer::app::Screen::Analysis;
    app.analysis_tab = 1;
    app.analysis_records = vec![record(1, 20.0), record(10, 30.0)];
    app.set_graphics_detection(detection());

    let buffer = render_buffer(&app);
    let text = buffer.content().iter().map(|cell| cell.symbol()).collect::<String>();
    assert!(text.contains("net WPM / daily median"), "{text}");
    assert!(text.contains("random"), "{text}");
    assert!(text.contains("2026-01-01"), "{text}");
    assert!(text.contains("2026-01-10"), "{text}");
}

#[test]
fn text_chart_keeps_the_existing_unknown_bucket_for_an_invalid_timestamp() {
    let mut app = common::test_app(Config::default());
    app.screen = rs_typing_trainer::app::Screen::Analysis;
    app.analysis_tab = 1;
    let mut invalid = record(10, 30.0);
    invalid.header.finished_at = "not-a-timestamp".into();
    app.analysis_records = vec![record(1, 20.0), invalid];

    let buffer = render_buffer(&app);
    let text = buffer.content().iter().map(|cell| cell.symbol()).collect::<String>();
    assert!(text.contains("2026-01-01"), "{text}");
    assert!(text.contains("unknown"), "{text}");
}
