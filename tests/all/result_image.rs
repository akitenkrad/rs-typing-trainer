use std::path::Path;

use chrono::NaiveDate;
use ratatui::layout::Size;
use ratatui::style::Color;
use rs_typing_trainer::graphics::CellPixels;
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::ui::bigtext_image::resolve_big_font;
use rs_typing_trainer::ui::keyboard::KeyTilePlacement;
use rs_typing_trainer::ui::keyboard_image::KeyTileSpec;
use rs_typing_trainer::ui::result_image::{
    BenchPartCardSpec, BenchTrendPoint, BenchTrendSpec, ItemSpeedPoint, MetricCardSpec, MetricDeltaKind, MistypeKeyboardSpec, ReactionHistogramSpec,
    ResultImageColors, SpeedChartSpec, format_metric_delta, render_bench_part_card, render_bench_trend, render_metric_card, render_mistype_keyboard,
    render_reaction_histogram, render_speed_chart,
};

fn colors() -> ResultImageColors {
    ResultImageColors {
        foreground: Color::White,
        muted: Color::Gray,
        correct: Color::LightGreen,
        mistype: Color::LightRed,
        accent: Color::LightCyan,
        warning: Color::LightYellow,
        panel: Color::Rgb(16, 20, 28),
    }
}

fn rgba(color: Color) -> [u8; 4] {
    match color {
        Color::White => [255, 255, 255, 255],
        Color::Gray => [204, 204, 204, 255],
        Color::LightGreen => [144, 238, 144, 255],
        Color::LightRed => [241, 76, 76, 255],
        Color::LightCyan => [41, 184, 219, 255],
        Color::LightYellow => [245, 245, 67, 255],
        Color::Rgb(red, green, blue) => [red, green, blue, 255],
        other => panic!("test has no RGB mapping for {other:?}"),
    }
}

fn has_rgb(image: &image::RgbaImage, target: [u8; 4]) -> bool {
    image.pixels().any(|pixel| pixel.0 == target)
}

fn color_bounds(image: &image::RgbaImage, target: [u8; 4], area: [u32; 4]) -> Option<[u32; 4]> {
    let [left, top, right, bottom] = area;
    let mut bounds = None::<[u32; 4]>;
    for y in top..bottom.min(image.height()) {
        for x in left..right.min(image.width()) {
            if image.get_pixel(x, y).0 != target {
                continue;
            }
            match &mut bounds {
                Some([minimum_x, minimum_y, maximum_x, maximum_y]) => {
                    *minimum_x = (*minimum_x).min(x);
                    *minimum_y = (*minimum_y).min(y);
                    *maximum_x = (*maximum_x).max(x);
                    *maximum_y = (*maximum_y).max(y);
                }
                None => bounds = Some([x, y, x, y]),
            }
        }
    }
    bounds
}

#[test]
fn metric_delta_text_is_localized_and_formats_new_best_zero_and_accuracy_points() {
    assert_eq!(format_metric_delta(Lang::Ja, MetricDeltaKind::Previous, 1.5, ""), "前回 +1.5");
    assert_eq!(format_metric_delta(Lang::Ja, MetricDeltaKind::Best, -0.8, ""), "自己ベスト -0.8");
    assert_eq!(format_metric_delta(Lang::En, MetricDeltaKind::Previous, 1.5, ""), "prev +1.5");
    assert_eq!(format_metric_delta(Lang::En, MetricDeltaKind::Best, -0.8, ""), "best -0.8");
    assert_eq!(format_metric_delta(Lang::Ja, MetricDeltaKind::Best, 0.1, ""), "自己ベスト更新!");
    assert_eq!(format_metric_delta(Lang::En, MetricDeltaKind::Best, 0.1, ""), "New best!");
    assert_eq!(format_metric_delta(Lang::Ja, MetricDeltaKind::Previous, 0.0, ""), "前回 ±0");
    assert_eq!(format_metric_delta(Lang::En, MetricDeltaKind::Best, 0.0, ""), "best ±0");
    assert_eq!(format_metric_delta(Lang::Ja, MetricDeltaKind::Previous, 4.0, "pt"), "前回 +4.0pt");
}

#[test]
fn metric_card_has_exact_size_and_both_delta_direction_colors() {
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let spec = MetricCardSpec {
        label: "WPM".into(),
        value: "72.4".into(),
        previous_delta: Some(3.2),
        best_delta: Some(-1.6),
        recent: vec![60.0, 65.0, 62.0, 72.4],
        lang: Lang::En,
        delta_suffix: String::new(),
        cells: Size::new(40, 8),
        colors: colors(),
    };
    let image = render_metric_card(&font, CellPixels::new(9, 20), &spec).unwrap();

    assert_eq!((image.width(), image.height()), (360, 160));
    assert!(has_rgb(&image, rgba(spec.colors.correct)), "up delta has no correct-color pixels");
    assert!(has_rgb(&image, rgba(spec.colors.mistype)), "down delta has no mistype-color pixels");
}

#[test]
fn benchmark_part_card_uses_direction_colors_for_wpm_and_accuracy_deltas() {
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let spec = BenchPartCardSpec {
        label: "Part B Coverage".into(),
        wpm: 72.4,
        accuracy: 0.965,
        previous_wpm_delta: Some(3.2),
        best_wpm_delta: Some(-1.6),
        previous_accuracy_delta: Some(-0.01),
        best_accuracy_delta: Some(0.005),
        version: "v2/B/03".into(),
        lang: Lang::En,
        cells: Size::new(34, 9),
        colors: colors(),
    };
    let image = render_bench_part_card(&font, CellPixels::new(9, 20), &spec).unwrap();

    assert_eq!((image.width(), image.height()), (306, 180));
    assert!(has_rgb(&image, rgba(spec.colors.correct)), "positive delta has no correct-color pixels");
    assert!(has_rgb(&image, rgba(spec.colors.mistype)), "negative delta has no mistype-color pixels");
}

#[test]
fn benchmark_part_card_keeps_title_inside_and_draws_readable_metric_rows() {
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let cell_pixels = CellPixels::new(9, 20);
    let spec = BenchPartCardSpec {
        label: "Part A English words".into(),
        wpm: 72.4,
        accuracy: 0.944,
        previous_wpm_delta: None,
        best_wpm_delta: None,
        previous_accuracy_delta: None,
        best_accuracy_delta: None,
        version: "v2/A/03".into(),
        lang: Lang::En,
        cells: Size::new(38, 7),
        colors: colors(),
    };
    let image = render_bench_part_card(&font, cell_pixels, &spec).unwrap();
    let foreground = rgba(spec.colors.foreground);
    let title = color_bounds(&image, foreground, [4, 8, image.width() - 4, 36]).expect("title pixels");
    assert!(title[1] >= 10, "title touches the top edge: {title:?}");
    let wpm_label = color_bounds(&image, foreground, [image.width() / 2, 34, image.width() - 6, 74]).expect("readable WPM label");
    assert!(wpm_label[3] - wpm_label[1] + 1 >= 11, "WPM label is too small: {wpm_label:?}");
    assert!(
        color_bounds(&image, foreground, [8, 70, image.width() - 8, 102]).is_some(),
        "accuracy row is missing"
    );
    assert!(
        color_bounds(&image, foreground, [8, 100, image.width() - 8, image.height() - 6]).is_some(),
        "initial/delta row is missing"
    );
}

#[test]
fn benchmark_part_card_supporting_text_matches_accuracy_size_and_hugs_wpm_value() {
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let cell_pixels = CellPixels::new(9, 20);
    let mut shared_colors = colors();
    shared_colors.correct = Color::White;
    shared_colors.mistype = Color::White;
    let spec = BenchPartCardSpec {
        label: "Part A English words".into(),
        wpm: 12.3,
        accuracy: 0.994,
        previous_wpm_delta: Some(-72.5),
        best_wpm_delta: Some(-72.5),
        previous_accuracy_delta: Some(-0.993),
        best_accuracy_delta: Some(-0.993),
        version: "v2/A/03".into(),
        lang: Lang::En,
        cells: Size::new(40, 9),
        colors: shared_colors,
    };
    let image = render_bench_part_card(&font, cell_pixels, &spec).unwrap();
    let foreground = rgba(Color::White);
    let accent = rgba(spec.colors.accent);
    let accuracy = color_bounds(&image, foreground, [8, 74, image.width() - 8, 100]).expect("accuracy row");
    let accuracy_height = accuracy[3] - accuracy[1] + 1;
    let wpm = color_bounds(&image, foreground, [image.width() * 2 / 5, 34, image.width() - 8, 76]).expect("WPM label");
    let previous = color_bounds(&image, foreground, [30, 96, image.width() * 2 / 3, 117]).expect("previous delta text");
    let best = color_bounds(&image, foreground, [30, 117, image.width() * 2 / 3, 143]).expect("best delta text");
    for (name, bounds) in [("WPM", wpm), ("previous", previous), ("best", best)] {
        let height = bounds[3] - bounds[1] + 1;
        assert!(
            height * 5 >= accuracy_height * 4,
            "{name} text is too small: height={height}, accuracy_height={accuracy_height}, bounds={bounds:?}"
        );
    }

    let value = color_bounds(&image, accent, [0, 28, image.width() * 3 / 5, 80]).expect("large WPM value");
    assert!(
        wpm[0].saturating_sub(value[2]) <= 12,
        "WPM label is too far from the value: value={value:?}, label={wpm:?}"
    );
    assert!(
        wpm[3].abs_diff(value[3]) <= 4,
        "WPM label is not bottom-aligned: value={value:?}, label={wpm:?}"
    );
}

#[test]
fn benchmark_part_card_initial_text_matches_accuracy_size() {
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let cell_pixels = CellPixels::new(9, 20);
    let spec = BenchPartCardSpec {
        label: "Part A English words".into(),
        wpm: 72.4,
        accuracy: 0.944,
        previous_wpm_delta: None,
        best_wpm_delta: None,
        previous_accuracy_delta: None,
        best_accuracy_delta: None,
        version: "v2/A/03".into(),
        lang: Lang::En,
        cells: Size::new(40, 9),
        colors: colors(),
    };
    let image = render_bench_part_card(&font, cell_pixels, &spec).unwrap();
    let foreground = rgba(spec.colors.foreground);
    let accuracy = color_bounds(&image, foreground, [8, 74, image.width() - 8, 100]).expect("accuracy row");
    let initial = color_bounds(&image, foreground, [8, 96, image.width() / 2, 120]).expect("initial row");
    let accuracy_height = accuracy[3] - accuracy[1] + 1;
    let initial_height = initial[3] - initial[1] + 1;
    assert!(
        initial_height * 5 >= accuracy_height * 4,
        "initial text is too small: height={initial_height}, accuracy_height={accuracy_height}, bounds={initial:?}"
    );
}

#[test]
fn benchmark_trend_draws_version_break_without_joining_lines_and_emphasizes_current_point() {
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let spec = BenchTrendSpec {
        points: vec![
            BenchTrendPoint {
                date: NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
                wpm: 50.0,
                accuracy: 0.94,
                starts_new_series: false,
                current: false,
            },
            BenchTrendPoint {
                date: NaiveDate::from_ymd_opt(2026, 10, 8).unwrap(),
                wpm: 50.0,
                accuracy: 0.95,
                starts_new_series: false,
                current: false,
            },
            BenchTrendPoint {
                date: NaiveDate::from_ymd_opt(2026, 10, 15).unwrap(),
                wpm: 50.0,
                accuracy: 0.96,
                starts_new_series: true,
                current: false,
            },
            BenchTrendPoint {
                date: NaiveDate::from_ymd_opt(2026, 10, 22).unwrap(),
                wpm: 60.0,
                accuracy: 0.97,
                starts_new_series: false,
                current: true,
            },
        ],
        cells: Size::new(36, 9),
        colors: colors(),
    };
    let cell_pixels = CellPixels::new(9, 20);
    let image = render_bench_trend(&font, cell_pixels, &spec).unwrap();
    let break_x = spec.point_x(image.width(), 2).round() as u32;
    let warning = rgba(spec.colors.warning);
    let warning_count = (0..image.height()).filter(|y| image.get_pixel(break_x, *y).0 == warning).count();
    assert!(warning_count >= 4, "version break is missing at x={break_x}");

    let accent = rgba(spec.colors.accent);
    let left_x = spec.point_x(image.width(), 1);
    let right_x = spec.point_x(image.width(), 2);
    let y = spec.wpm_y(image.height(), 50.0).round() as u32;
    let middle_x = ((left_x + right_x) / 2.0).round() as u32;
    assert_ne!(image.get_pixel(middle_x, y).0, accent, "WPM line crossed the v2 break");

    let colored_near = |index: usize| {
        let x = spec.point_x(image.width(), index).round() as u32;
        let y = spec.wpm_y(image.height(), spec.points[index].wpm).round() as u32;
        (x.saturating_sub(7)..=(x + 7).min(image.width() - 1))
            .flat_map(|x| (y.saturating_sub(7)..=(y + 7).min(image.height() - 1)).map(move |y| (x, y)))
            .filter(|(x, y)| image.get_pixel(*x, *y).0 == accent)
            .count()
    };
    assert!(colored_near(3) > colored_near(0), "current point is not emphasized");
}

#[test]
fn benchmark_trend_draws_wpm_ticks_and_first_and_last_dates_at_result_chart_size() {
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let cell_pixels = CellPixels::new(9, 20);
    let spec = BenchTrendSpec {
        points: vec![
            BenchTrendPoint {
                date: NaiveDate::from_ymd_opt(2026, 10, 2).unwrap(),
                wpm: 40.0,
                accuracy: 0.92,
                starts_new_series: false,
                current: false,
            },
            BenchTrendPoint {
                date: NaiveDate::from_ymd_opt(2026, 11, 16).unwrap(),
                wpm: 60.0,
                accuracy: 0.97,
                starts_new_series: false,
                current: true,
            },
        ],
        cells: Size::new(40, 9),
        colors: colors(),
    };
    let image = render_bench_trend(&font, cell_pixels, &spec).unwrap();
    let muted = rgba(spec.colors.muted);
    let plot = spec.plot_bounds(image.width(), image.height(), cell_pixels);
    let middle_tick = color_bounds(
        &image,
        muted,
        [0, (plot.top + plot.bottom) / 2 - 18, plot.left, (plot.top + plot.bottom) / 2 + 18],
    )
    .expect("middle WPM tick");
    let dates = color_bounds(&image, muted, [0, plot.bottom + 3, image.width(), image.height()]).expect("date labels");
    assert!(middle_tick[3] - middle_tick[1] + 1 >= 11, "WPM tick is too small: {middle_tick:?}");
    assert!(dates[3] - dates[1] + 1 >= 11, "date labels are too small: {dates:?}");
    assert!(dates[0] <= plot.left + 4, "first date is missing: {dates:?}");
    assert!(dates[2] >= plot.right.saturating_sub(4), "last date is missing: {dates:?}");

    let last_date = color_bounds(
        &image,
        muted,
        [plot.right.saturating_sub(24), plot.bottom + 3, image.width(), image.height()],
    )
    .expect("last date label");
    assert!(last_date[2] <= image.width() - 4, "last date is clipped at the right edge: {last_date:?}");
    assert!(
        color_bounds(
            &image,
            muted,
            [0, plot.bottom.saturating_sub(8), plot.left.saturating_sub(2), plot.bottom + 4],
        )
        .is_none(),
        "minimum WPM tick overlaps the first date"
    );
}

#[test]
fn benchmark_trend_keeps_version_label_left_of_a_right_edge_break() {
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let cell_pixels = CellPixels::new(9, 20);
    let spec = BenchTrendSpec {
        points: vec![
            BenchTrendPoint {
                date: NaiveDate::from_ymd_opt(2026, 10, 10).unwrap(),
                wpm: 40.0,
                accuracy: 0.92,
                starts_new_series: false,
                current: false,
            },
            BenchTrendPoint {
                date: NaiveDate::from_ymd_opt(2026, 11, 17).unwrap(),
                wpm: 60.0,
                accuracy: 0.97,
                starts_new_series: true,
                current: true,
            },
        ],
        cells: Size::new(40, 9),
        colors: colors(),
    };
    let image = render_bench_trend(&font, cell_pixels, &spec).unwrap();
    let plot = spec.plot_bounds(image.width(), image.height(), cell_pixels);
    let break_x = spec.point_x(image.width(), 1).round() as u32;
    let label = color_bounds(&image, rgba(spec.colors.warning), [0, 0, image.width(), plot.top]).expect("v2 label");
    assert!(
        label[2] < break_x,
        "v2 label must be placed left of its break: label={label:?}, break_x={break_x}"
    );
    assert!(label[2] - label[0] + 1 >= 6, "v2 label is clipped: {label:?}");
}

#[test]
fn metric_card_draws_both_signs_and_centers_a_text_height_direction_marker() {
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let cell_pixels = CellPixels::new(9, 20);
    let mut shared_colors = colors();
    shared_colors.correct = Color::White;
    shared_colors.mistype = Color::White;
    let spec = MetricCardSpec {
        label: "WPM".into(),
        value: "9.0".into(),
        previous_delta: Some(6.0),
        best_delta: None,
        recent: Vec::new(),
        lang: Lang::En,
        delta_suffix: String::new(),
        cells: Size::new(40, 8),
        colors: shared_colors,
    };
    let positive = render_metric_card(&font, cell_pixels, &spec).unwrap();
    let mut without_delta_spec = spec.clone();
    without_delta_spec.previous_delta = None;
    let without_delta = render_metric_card(&font, cell_pixels, &without_delta_spec).unwrap();
    let mut negative_spec = spec.clone();
    negative_spec.previous_delta = Some(-6.0);
    let negative = render_metric_card(&font, cell_pixels, &negative_spec).unwrap();
    let text_color = rgba(Color::White);
    let difference = (40..positive.width() / 2)
        .flat_map(|x| (positive.height() * 2 / 3..positive.height()).map(move |y| (x, y)))
        .filter(|(x, y)| positive.get_pixel(*x, *y) != negative.get_pixel(*x, *y))
        .collect::<Vec<_>>();
    assert!(!difference.is_empty(), "positive and negative signs produced identical text pixels");
    assert!(
        difference
            .iter()
            .any(|(x, y)| negative.get_pixel(*x, *y) != without_delta.get_pixel(*x, *y)),
        "negative delta reserved sign space but drew no sign"
    );

    let marker = color_bounds(&negative, text_color, [8, 105, 23, 150]).expect("negative direction marker");
    let marker_height = marker[3] - marker[1] + 1;
    let expected_text_center = 160.0 * 0.76 - 2.0 + 23.0 / 2.0;
    let marker_center = (marker[1] + marker[3]) as f32 / 2.0;
    assert!(marker_height >= 16, "direction marker is only {marker_height}px high");
    assert!(
        (marker_center - expected_text_center).abs() <= 1.5,
        "direction marker center {marker_center:.1}px does not match text center {expected_text_center:.1}px"
    );
}

#[test]
fn metric_card_sparkline_stays_outside_value_bounds_and_emphasizes_latest_point() {
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let cell_pixels = CellPixels::new(9, 20);
    let spec = MetricCardSpec {
        label: "WPM".into(),
        value: "16.5".into(),
        previous_delta: Some(0.0),
        best_delta: Some(-0.8),
        recent: vec![12.0, 16.5],
        lang: Lang::Ja,
        delta_suffix: String::new(),
        cells: Size::new(40, 8),
        colors: colors(),
    };
    let image = render_metric_card(&font, cell_pixels, &spec).unwrap();
    let mut without_sparkline = spec.clone();
    without_sparkline.recent = vec![f64::NAN, f64::NAN];
    let baseline = render_metric_card(&font, cell_pixels, &without_sparkline).unwrap();
    let value = spec.value_bounds(image.width(), image.height(), cell_pixels);

    for y in value.top..value.bottom {
        for x in value.left..value.right {
            assert_eq!(
                image.get_pixel(x, y),
                baseline.get_pixel(x, y),
                "sparkline changed value pixel ({x}, {y})"
            );
        }
    }

    let points = spec.sparkline_points(image.width(), image.height());
    assert_eq!(points.len(), 2);
    let accent = rgba(spec.colors.accent);
    let image_ref = &image;
    let accent_count = |(center_x, center_y): (u32, u32)| {
        (center_x.saturating_sub(6)..=(center_x + 6).min(image_ref.width() - 1))
            .flat_map(|x| {
                (center_y.saturating_sub(6)..=(center_y + 6).min(image_ref.height() - 1))
                    .map(move |y| u32::from(image_ref.get_pixel(x, y).0 == accent))
            })
            .sum::<u32>()
    };
    assert!(accent_count(points[1]) > accent_count(points[0]));
}

#[test]
fn speed_chart_marks_mistyped_items_in_mistype_color() {
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let spec = SpeedChartSpec {
        points: vec![
            ItemSpeedPoint {
                wpm: 42.0,
                mistypes: Some(0),
            },
            ItemSpeedPoint {
                wpm: 61.0,
                mistypes: Some(2),
            },
            ItemSpeedPoint {
                wpm: 50.0,
                mistypes: Some(0),
            },
        ],
        lang: Lang::En,
        cells: Size::new(70, 9),
        colors: colors(),
    };
    let image = render_speed_chart(&font, CellPixels::new(9, 20), &spec).unwrap();

    assert_eq!((image.width(), image.height()), (630, 180));
    assert!(has_rgb(&image, rgba(spec.colors.mistype)), "mistyped item has no mistype-color marker");
}

#[test]
fn speed_chart_average_line_uses_the_mean_of_the_plotted_items() {
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let cell_pixels = CellPixels::new(9, 20);
    let spec = SpeedChartSpec {
        points: [20.0, 60.0, 40.0]
            .into_iter()
            .map(|wpm| ItemSpeedPoint { wpm, mistypes: Some(0) })
            .collect(),
        lang: Lang::Ja,
        cells: Size::new(70, 9),
        colors: colors(),
    };
    let image = render_speed_chart(&font, cell_pixels, &spec).unwrap();
    let plot = spec.plot_bounds(image.width(), image.height(), cell_pixels);
    let average_y = spec.value_y(plot, spec.average_wpm()).round() as u32;
    let probe_x = plot.left + plot.right.saturating_sub(plot.left) / 8;

    assert_eq!(spec.average_wpm(), 40.0);
    assert_eq!(&image.get_pixel(probe_x, average_y).0[..3], &rgba(spec.colors.muted)[..3]);
}

#[test]
fn speed_chart_uses_readable_axis_labels_and_keeps_the_minimum_left_of_the_point() {
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let cell_pixels = CellPixels::new(9, 20);
    let spec = SpeedChartSpec {
        points: [10.0, 15.0, 20.0]
            .into_iter()
            .map(|wpm| ItemSpeedPoint { wpm, mistypes: Some(0) })
            .collect(),
        lang: Lang::Ja,
        cells: Size::new(70, 9),
        colors: colors(),
    };
    let image = render_speed_chart(&font, cell_pixels, &spec).unwrap();
    let plot = spec.plot_bounds(image.width(), image.height(), cell_pixels);
    let muted = rgba(spec.colors.muted);
    let middle_y = spec.value_y(plot, 15.0).round() as u32;
    let middle_label = color_bounds(
        &image,
        muted,
        [0, middle_y.saturating_sub(20), plot.left, (middle_y + 20).min(image.height())],
    )
    .expect("middle vertical-axis label");
    let item_label = color_bounds(
        &image,
        muted,
        [plot.left.saturating_sub(20), plot.bottom + 3, plot.left + 20, image.height()],
    )
    .expect("horizontal-axis item label");
    let average_label = color_bounds(
        &image,
        muted,
        [
            plot.right + 3,
            middle_y.saturating_sub(20),
            image.width(),
            (middle_y + 20).min(image.height()),
        ],
    )
    .expect("average label");
    for (name, bounds) in [
        ("vertical axis", middle_label),
        ("horizontal axis", item_label),
        ("average", average_label),
    ] {
        let height = bounds[3] - bounds[1] + 1;
        assert!(height >= 11, "{name} label is only {height}px high");
    }

    let minimum_label =
        color_bounds(&image, muted, [0, plot.bottom.saturating_sub(20), plot.left, plot.bottom + 3]).expect("minimum vertical-axis label");
    assert!(
        minimum_label[2] <= plot.left.saturating_sub(8),
        "minimum label ends at x={}, too close to point x={} ",
        minimum_label[2],
        plot.left
    );
}

#[test]
fn mistype_keyboard_draws_the_most_frequent_key_with_the_strongest_face() {
    let tile = |x, face_color, label: &str| KeyTilePlacement {
        x,
        y: 0,
        spec: KeyTileSpec {
            cells: Size::new(5, 2),
            label: label.into(),
            face_color,
            text_color: Color::White,
            bold: false,
            pressed: false,
            modifier: false,
            finger_color: None,
        },
    };
    let spec = MistypeKeyboardSpec {
        placements: vec![tile(0, Color::Rgb(76, 16, 20), "a 1"), tile(6, Color::Rgb(220, 0, 20), "s 8")],
        cells: Size::new(12, 2),
    };
    let image = render_mistype_keyboard(CellPixels::new(10, 20), &spec).unwrap();

    assert_eq!((image.width(), image.height()), (120, 40));
    let left_red = (0..50)
        .flat_map(|x| (0..40).map(|y| image.get_pixel(x, y).0).collect::<Vec<_>>())
        .filter(|pixel| pixel[1] < 100)
        .map(|pixel| pixel[0])
        .max()
        .unwrap();
    let right_red = (60..110)
        .flat_map(|x| (0..40).map(|y| image.get_pixel(x, y).0).collect::<Vec<_>>())
        .filter(|pixel| pixel[1] < 100)
        .map(|pixel| pixel[0])
        .max()
        .unwrap();
    assert!(right_red > left_red, "most frequent key is not strongest: {left_red} >= {right_red}");
}

#[test]
fn reaction_histogram_places_p50_and_p90_lines_at_tenth_second_scale_positions() {
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let spec = ReactionHistogramSpec {
        reaction_seconds: vec![0.05, 0.12, 0.24, 0.31, 0.46, 0.72, 1.62],
        p50_seconds: 0.31,
        p90_seconds: 1.20,
        cells: Size::new(45, 9),
        colors: colors(),
    };
    let image = render_reaction_histogram(&font, CellPixels::new(9, 20), &spec).unwrap();
    let plot = spec.plot_bounds(image.width(), image.height());
    let p50_x = spec.marker_x(plot, spec.p50_seconds);
    let p90_x = spec.marker_x(plot, spec.p90_seconds);

    assert_eq!((image.width(), image.height()), (405, 180));
    assert!((plot.top..plot.bottom).any(|y| image.get_pixel(p50_x, y).0 == rgba(spec.colors.correct)));
    assert!((plot.top..plot.bottom).any(|y| image.get_pixel(p90_x, y).0 == rgba(spec.colors.warning)));
    assert!(p50_x < p90_x);
}

#[test]
fn reaction_histogram_draws_the_requested_horizontal_axis_labels() {
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let spec = ReactionHistogramSpec {
        reaction_seconds: vec![0.1, 0.5, 1.0, 1.6],
        p50_seconds: 0.5,
        p90_seconds: 1.0,
        cells: Size::new(45, 9),
        colors: colors(),
    };
    let image = render_reaction_histogram(&font, CellPixels::new(9, 20), &spec).unwrap();
    let plot = spec.plot_bounds(image.width(), image.height());
    let image_ref = &image;

    for seconds in [0.0, 0.5, 1.0, 1.5] {
        let tick_x = spec.marker_x(plot, seconds);
        let left = tick_x.saturating_sub(24);
        let right = (tick_x + 24).min(image.width() - 1);
        let has_label_pixel = (left..=right)
            .flat_map(|x| (plot.bottom + 3..image_ref.height()).map(move |y| image_ref.get_pixel(x, y).0[3]))
            .any(|alpha| alpha > 0);
        assert!(has_label_pixel, "no axis label near {seconds:.1}s");
    }
}

#[test]
fn reaction_histogram_axis_labels_match_the_percentile_label_height() {
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let cell_pixels = CellPixels::new(9, 20);
    let spec = ReactionHistogramSpec {
        reaction_seconds: vec![0.1, 0.5, 1.0, 1.6],
        p50_seconds: 0.3,
        p90_seconds: 1.2,
        cells: Size::new(45, 9),
        colors: colors(),
    };
    let image = render_reaction_histogram(&font, cell_pixels, &spec).unwrap();
    let plot = spec.plot_bounds(image.width(), image.height());
    let axis = color_bounds(&image, rgba(spec.colors.muted), [0, plot.bottom + 4, image.width(), image.height()])
        .expect("reaction-time horizontal-axis labels");
    let percentile = color_bounds(&image, rgba(spec.colors.correct), [0, 0, image.width(), plot.top]).expect("p50 label");
    let axis_height = axis[3] - axis[1] + 1;
    let percentile_height = percentile[3] - percentile[1] + 1;

    assert!(
        axis_height + 2 >= percentile_height,
        "axis labels are {axis_height}px high but p50 is {percentile_height}px high"
    );
}
