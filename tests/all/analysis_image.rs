use std::path::Path;
use std::time::Duration;

use chrono::{NaiveDate, TimeZone, Utc};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect, Size};
use ratatui::style::Color;
use rs_typing_trainer::analysis::bench_v3::rolling_median_3;
use rs_typing_trainer::analysis::grades::Level;
use rs_typing_trainer::analysis::grades::{Component, ComponentValue, GradeSource, GradeStatus, evaluate_language};
use rs_typing_trainer::analysis::metrics::{BigramMetric, GroupMetric, TransitionClass};
use rs_typing_trainer::config::Config;
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::graphics::{CellPixels, GraphicsDetection, GraphicsMode, GraphicsReason};
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::layout::builtin_layout;
use rs_typing_trainer::layout::geometry::geometry;
use rs_typing_trainer::record::SessionRecord;
use rs_typing_trainer::ui;
use rs_typing_trainer::ui::analysis_image::{
    AnalysisImageCache, ComparisonBarsSpec, FORECAST_EN_COLOR, FingerChartSpec, GradeBadgeSpec, GradeLadderSpec, MetricKind, PixelPoint, RadarAxis,
    RadarChartSpec, RadarRing, SERIES_COLORS, SkillChartSpec, SparklineSpec, TransitionArrowSpec, TransitionBar, TransitionBarsSpec,
    TransitionKeyboardSpec, TrendChartSpec, TrendDateRange, TrendForecast, TrendPoint, TrendSeries, build_finger_bars, build_trend_data,
    radar_label_layout, render_comparison_bars, render_finger_chart, render_grade_badge, render_grade_ladder, render_radar_chart, render_skill_chart,
    render_sparkline, render_transition_arrow, render_transition_bars, render_trend_chart, render_trend_chart_with_range, shared_trend_date_range,
};
use rs_typing_trainer::ui::bigtext_image::resolve_big_font;
use rs_typing_trainer::ui::keyboard::{KeyboardState, KeyboardView, keycap_center_cells, keycap_tile_placements};

use crate::common;

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
        Color::LightBlue => [59, 142, 234, 255],
        Color::LightMagenta => [214, 112, 214, 255],
        Color::LightRed => [241, 76, 76, 255],
        Color::LightCyan => [41, 184, 219, 255],
        Color::Blue => [36, 114, 200, 255],
        Color::Magenta => [188, 63, 188, 255],
        Color::Yellow => [229, 229, 16, 255],
        Color::Gray => [204, 204, 204, 255],
        Color::Cyan => [17, 168, 205, 255],
        Color::Green => [13, 188, 121, 255],
        Color::White => [255, 255, 255, 255],
        Color::Rgb(red, green, blue) => [red, green, blue, 255],
        other => panic!("test has no RGB mapping for {other:?}"),
    }
}

fn decode_base64(input: &str) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len() * 3 / 4);
    let mut bits = 0_u32;
    let mut bit_count = 0_u8;
    for byte in input.bytes().filter(|byte| *byte != b'=') {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        bits = (bits << 6) | u32::from(value);
        bit_count += 6;
        while bit_count >= 8 {
            bit_count -= 8;
            output.push((bits >> bit_count) as u8);
            bits &= (1_u32 << bit_count).saturating_sub(1);
        }
    }
    output
}

fn kitty_rgba(symbol: &str) -> Option<(u32, u32, Vec<u8>)> {
    let first = symbol.find("\u{1b}_G")?;
    let header_end = symbol[first..].find(';')? + first;
    let header = &symbol[first..header_end];
    let field = |name: &str| {
        header
            .split(',')
            .find_map(|part| part.strip_prefix(name))
            .and_then(|value| value.parse::<u32>().ok())
    };
    let width = field("s=")?;
    let height = field("v=")?;
    let mut encoded = String::new();
    let mut rest = &symbol[first..];
    while let Some(start) = rest.find("\u{1b}_G") {
        rest = &rest[start..];
        let separator = rest.find(';')?;
        let payload = &rest[separator + 1..];
        let end = payload.find("\u{1b}\\")?;
        encoded.push_str(&payload[..end]);
        rest = &payload[end + 2..];
    }
    Some((width, height, decode_base64(&encoded)))
}

fn group(name: &str, iki_median_ms: f64, accuracy: f64, n: u64) -> GroupMetric {
    GroupMetric {
        name: name.into(),
        correct: 0,
        total: 0,
        accuracy,
        n,
        iki_median_ms,
    }
}

#[test]
fn transition_bars_scale_length_use_error_heatmap_and_outline_selection() {
    let spec = TransitionBarsSpec {
        rows: vec![
            TransitionBar {
                value: 100.0,
                error_rate: 0.0,
                selected: false,
            },
            TransitionBar {
                value: 200.0,
                error_rate: 0.5,
                selected: true,
            },
            TransitionBar {
                value: 400.0,
                error_rate: 0.25,
                selected: false,
            },
        ],
        max_error_rate: 0.5,
        cells: Size::new(40, 3),
    };
    let image = render_transition_bars(CellPixels::new(4, 10), &spec).unwrap();
    let row_height = image.height() / 3;
    let zero_error = rgba(Color::Rgb(40, 20, 20));
    let maximum_error = rgba(Color::Rgb(220, 0, 20));
    let first_y = row_height / 2;
    let second_y = row_height + row_height / 2;

    let zero_body = image.get_pixel(image.width() / 16, first_y).0;
    let brightness = |pixel: [u8; 4]| u16::from(pixel[0]) + u16::from(pixel[1]) + u16::from(pixel[2]);
    assert!(zero_body[3] > 0, "zero-error bar body is transparent");
    assert!(
        brightness(zero_body) >= 240,
        "zero-error bar body is too close to a dark background: {zero_body:?}"
    );
    assert!(has_rgb_near(&image, image.width() / 4 - 3, first_y, zero_error, 3));
    assert!(!has_rgb_near(&image, image.width() / 3, first_y, zero_error, 2));
    assert!(has_rgb_near(&image, image.width() / 2 - 3, second_y, maximum_error, 3));
    assert!(!has_rgb_near(&image, image.width() * 5 / 8, second_y, maximum_error, 2));
    assert!(
        (0..image.width()).any(|x| has_rgb_near(&image, x, row_height, rgba(Color::White), 0)),
        "selected row has no outline"
    );
}

#[test]
fn transition_arrow_marks_both_key_centres_loop_and_missing_key() {
    let cell_pixels = CellPixels::new(10, 20);
    let color = Color::LightCyan;
    let arrow = TransitionArrowSpec {
        cells: Size::new(20, 10),
        from: Some(PixelPoint { x: 4.0, y: 3.0 }),
        to: Some(PixelPoint { x: 15.0, y: 7.0 }),
        same_key: false,
        color,
    };
    let image = render_transition_arrow(cell_pixels, &arrow).unwrap();
    assert!(has_rgb_near(&image, 40, 60, rgba(color), 6));
    assert!(has_rgb_near(&image, 150, 140, rgba(color), 8));

    let loop_spec = TransitionArrowSpec {
        from: Some(PixelPoint { x: 8.0, y: 5.0 }),
        to: Some(PixelPoint { x: 8.0, y: 5.0 }),
        same_key: true,
        ..arrow.clone()
    };
    let loop_image = render_transition_arrow(cell_pixels, &loop_spec).unwrap();
    let loop_radius = 14;
    assert!(has_rgb_near(&loop_image, 80 + loop_radius, 100, rgba(color), 4));
    assert!(has_rgb_near(&loop_image, 80, 100 - loop_radius, rgba(color), 4));

    let missing = TransitionArrowSpec {
        from: None,
        to: None,
        same_key: false,
        ..arrow
    };
    let missing_image = render_transition_arrow(cell_pixels, &missing).unwrap();
    assert!(missing_image.pixels().all(|pixel| pixel.0[3] == 0));
}

#[test]
fn transition_arrow_uses_physical_keycap_centres() {
    let keyboard = geometry("ansi").unwrap();
    let layout = builtin_layout("qwerty").unwrap();
    let from = layout
        .stroke_for('a')
        .and_then(|stroke| keycap_center_cells(&keyboard, &stroke.code))
        .unwrap();
    let to = layout
        .stroke_for('s')
        .and_then(|stroke| keycap_center_cells(&keyboard, &stroke.code))
        .unwrap();
    assert_eq!(from.y, to.y);
    assert!(from.x < to.x);

    let spec = TransitionArrowSpec {
        cells: Size::new(76, 11),
        from: Some(from),
        to: Some(to),
        same_key: false,
        color: Color::LightCyan,
    };
    let cell_pixels = CellPixels::new(9, 20);
    let image = render_transition_arrow(cell_pixels, &spec).unwrap();
    assert!(has_rgb_near(&image, (from.x * 9.0) as u32, (from.y * 20.0) as u32, rgba(spec.color), 6));
    assert!(has_rgb_near(&image, (to.x * 9.0) as u32, (to.y * 20.0) as u32, rgba(spec.color), 8));

    let missing = layout.stroke_for('🙂').and_then(|stroke| keycap_center_cells(&keyboard, &stroke.code));
    let missing_image = render_transition_arrow(
        cell_pixels,
        &TransitionArrowSpec {
            from: Some(from),
            to: missing,
            ..spec
        },
    )
    .unwrap();
    assert!(missing_image.pixels().all(|pixel| pixel.0[3] == 0));
}

#[test]
fn finger_chart_uses_one_centered_bar_for_shared_thumb_and_scales_up_and_down() {
    let metrics = vec![
        group("right-pinky", 500.0, 0.90, 80),
        group("left-index", 200.0, 0.98, 80),
        group("thumb", 150.0, 0.99, 80),
        group("left-pinky", 250.0, 0.95, 10),
        group("right-index", 300.0, 0.97, 80),
    ];
    let bars = build_finger_bars(&metrics);
    assert_eq!(
        bars.iter().map(|bar| bar.name.as_str()).collect::<Vec<_>>(),
        [
            "left-pinky",
            "left-ring",
            "left-middle",
            "left-index",
            "thumb",
            "right-index",
            "right-middle",
            "right-ring",
            "right-pinky",
        ]
    );
    assert!(bars[1].iki_median_ms.is_none());
    assert_eq!(bars[4].iki_median_ms, Some(150.0));

    let spec = FingerChartSpec {
        bars,
        cells: Size::new(45, 10),
        low_sample_n: 30,
    };
    let image = render_finger_chart(CellPixels::new(10, 10), &spec).unwrap();
    let baseline = image.height() / 2;
    let slot_width = image.width() / 9;
    let left_pinky_x = slot_width / 2;
    let left_index_x = slot_width * 3 + slot_width / 2;
    let thumb_x = image.width() / 2;
    let right_pinky_x = slot_width * 8 + slot_width / 2;

    assert!(image.get_pixel(right_pinky_x, 3).0[3] > 0, "largest IKI bar must reach the top");
    assert_eq!(image.get_pixel(left_index_x, 3).0[3], 0, "shorter IKI bar reached too high");
    let thumb_color = rgba(spec.bars[4].color);
    assert!(
        same_rgb_with_rounding(image.get_pixel(thumb_x - slot_width / 8, baseline - 2).0, thumb_color)
            && same_rgb_with_rounding(image.get_pixel(thumb_x + slot_width / 8, baseline - 2).0, thumb_color),
        "shared thumb bar does not straddle the hand separator"
    );
    assert!(
        image.get_pixel(right_pinky_x, image.height() - 3).0[3] > 0,
        "largest error bar must extend down"
    );
    assert_eq!(
        image.get_pixel(left_index_x, image.height() - 3).0[3],
        0,
        "smaller error bar reached too low"
    );
    assert!(
        image.get_pixel(left_pinky_x, baseline - 2).0[3] < image.get_pixel(left_index_x, baseline - 2).0[3],
        "low-sample finger is not translucent"
    );
}

#[test]
fn finger_chart_uses_two_thumb_bars_when_metrics_are_split_by_hand() {
    let metrics = vec![group("left-thumb", 140.0, 0.99, 70), group("right-thumb", 160.0, 0.98, 80)];

    let bars = build_finger_bars(&metrics);

    assert_eq!(bars.len(), 10);
    assert_eq!(bars[4].name, "left-thumb");
    assert_eq!(bars[4].iki_median_ms, Some(140.0));
    assert_eq!(bars[5].name, "right-thumb");
    assert_eq!(bars[5].iki_median_ms, Some(160.0));
}

#[test]
fn analysis_detail_images_cache_equal_inputs_and_include_selected_transition() {
    let graphics = detection();
    let mut cache = AnalysisImageCache::new();
    let bars = TransitionBarsSpec {
        rows: vec![TransitionBar {
            value: 200.0,
            error_rate: 0.1,
            selected: true,
        }],
        max_error_rate: 0.1,
        cells: Size::new(30, 1),
    };
    let arrow = TransitionArrowSpec {
        cells: Size::new(76, 11),
        from: Some(PixelPoint { x: 10.0, y: 3.0 }),
        to: Some(PixelPoint { x: 20.0, y: 5.0 }),
        same_key: false,
        color: Color::LightCyan,
    };
    let physical = geometry("ansi").unwrap();
    let layout = builtin_layout("qwerty").unwrap();
    let keyboard = TransitionKeyboardSpec {
        cells: arrow.cells,
        tiles: keycap_tile_placements(
            &physical,
            &layout,
            &KeyboardState::default(),
            KeyboardView {
                show: "full",
                style: "keycap",
                finger_guide: false,
            },
            true,
        ),
        arrow,
        selection: Some(("a".into(), "s".into())),
    };
    cache.prepare_transition_bars(&graphics, &bars).unwrap();
    cache.prepare_transition_keyboard(&graphics, &keyboard).unwrap();
    cache.prepare_transition_bars(&graphics, &bars).unwrap();
    cache.prepare_transition_keyboard(&graphics, &keyboard).unwrap();
    assert_eq!(cache.creation_count(), 2);

    let changed = TransitionKeyboardSpec {
        arrow: TransitionArrowSpec {
            to: Some(PixelPoint { x: 30.0, y: 7.0 }),
            ..keyboard.arrow.clone()
        },
        selection: Some(("a".into(), "d".into())),
        ..keyboard.clone()
    };
    cache.prepare_transition_bars(&graphics, &bars).unwrap();
    cache.prepare_transition_keyboard(&graphics, &changed).unwrap();
    assert_eq!(cache.creation_count(), 3);

    let same_arrow_different_selection = TransitionKeyboardSpec {
        selection: Some(("A".into(), "D".into())),
        ..changed
    };
    cache.prepare_transition_keyboard(&graphics, &same_arrow_different_selection).unwrap();
    assert_eq!(cache.creation_count(), 4);
}

#[test]
fn comparison_bars_share_one_maximum() {
    let spec = ComparisonBarsSpec {
        values: [100.0, 200.0, 300.0, 400.0],
        colors: [Color::LightCyan, Color::LightGreen, Color::LightMagenta, Color::LightYellow],
        cells: Size::new(40, 4),
    };
    let image = render_comparison_bars(CellPixels::new(4, 10), &spec).unwrap();
    let row_height = image.height() / 4;
    assert!(has_rgb_near(&image, image.width() / 5, row_height / 2, rgba(spec.colors[0]), 2));
    assert!(!has_rgb_near(&image, image.width() / 3, row_height / 2, rgba(spec.colors[0]), 2));
    assert!(has_rgb_near(
        &image,
        image.width() - 4,
        row_height * 3 + row_height / 2,
        rgba(spec.colors[3]),
        3
    ));
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

fn grade_palette() -> [Color; 8] {
    [
        Color::Rgb(70, 76, 86),
        Color::Gray,
        Color::Yellow,
        Color::LightMagenta,
        Color::LightBlue,
        Color::Cyan,
        Color::LightGreen,
        Color::LightYellow,
    ]
}

#[test]
fn grade_ladder_has_eight_coloured_steps_active_emphasis_and_score_proportional_marker() {
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let cell_pixels = CellPixels::new(10, 20);
    let spec = GradeLadderSpec {
        score: 3.21,
        level: Level::D,
        colors: grade_palette(),
        cells: Size::new(80, 4),
    };
    let image = render_grade_ladder(&font, cell_pixels, &spec).unwrap();
    let step_width = image.width() / 8;
    let step_y = image.height() * 3 / 4;

    for (index, color) in spec.colors.into_iter().enumerate() {
        let pixel = image.get_pixel(step_width * index as u32 + step_width / 2, step_y).0;
        assert!(same_rgb_with_rounding(pixel, rgba(color)), "step {index} has {pixel:?}");
    }

    let inactive_alpha = image.get_pixel(step_width / 2, step_y).0[3];
    let active_alpha = image.get_pixel(step_width * 3 + step_width / 2, step_y).0[3];
    assert!(active_alpha > inactive_alpha, "active={active_alpha}, inactive={inactive_alpha}");

    let expected_x = (image.width() as f64 * 3.21 / 8.0).round() as u32;
    let marker_y = image.height() / 3;
    assert!(
        has_rgb_near(&image, expected_x, marker_y, rgba(Color::White), step_width / 8),
        "score marker is not near x={expected_x}"
    );
}

fn radar_spec(values: Vec<Option<f64>>) -> RadarChartSpec {
    RadarChartSpec {
        axes: values
            .into_iter()
            .enumerate()
            .map(|(index, value)| RadarAxis {
                label: format!("Axis {index}"),
                value,
            })
            .collect(),
        max_value: 8.0,
        rings: (1..=8)
            .map(|value| RadarRing {
                value: f64::from(value),
                label: match value {
                    4 => Some("C".into()),
                    8 => Some("S".into()),
                    _ => None,
                },
            })
            .collect(),
        color: Color::LightGreen,
        cells: Size::new(42, 14),
        missing_label: "under 3".into(),
    }
}

#[test]
fn radar_places_maximum_on_outer_ring_zero_at_centre_and_omits_missing_vertex() {
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let cell_pixels = CellPixels::new(10, 20);
    let spec = radar_spec(vec![Some(8.0), None, Some(0.0), Some(5.0), Some(6.0), Some(4.0), Some(7.0)]);
    let image = render_radar_chart(&font, cell_pixels, &spec).unwrap();
    let color = rgba(spec.color);
    let center = (image.width() / 2, image.height() / 2);
    let radius = image.width().min(image.height()) as f64 * 0.30;
    let outer_top = (center.0, (center.1 as f64 - radius).round() as u32);

    assert!(
        has_rgb_near(&image, outer_top.0, outer_top.1, color, 5),
        "maximum vertex is not on the outer ring"
    );
    assert!(has_rgb_near(&image, center.0, center.1, color, 5), "zero vertex is not at the centre");

    let missing_angle = -std::f64::consts::FRAC_PI_2 + std::f64::consts::TAU / 7.0;
    let missing = (
        (center.0 as f64 + radius * missing_angle.cos()).round() as u32,
        (center.1 as f64 + radius * missing_angle.sin()).round() as u32,
    );
    assert!(
        !has_rgb_near(&image, missing.0, missing.1, color, 4),
        "missing axis has a grade-coloured vertex at {missing:?}"
    );
    assert!(
        image.pixels().any(|pixel| same_rgb_with_rounding(pixel.0, color) && pixel.0[3] < 255),
        "radar fill does not use the requested grade colour"
    );
}

fn assert_radar_labels_do_not_overlap(spec: &RadarChartSpec) {
    let layout = radar_label_layout(CellPixels::new(9, 20), spec);
    for (index, label) in layout.rings.iter().enumerate() {
        for other in &layout.rings[index + 1..] {
            assert!(
                !label.bounds.overlaps(other.bounds),
                "ring labels {:?} and {:?} overlap: {:?} / {:?}",
                label.text,
                other.text,
                label.bounds,
                other.bounds
            );
        }
        for axis in &layout.axes {
            assert!(
                !label.bounds.overlaps(axis.bounds),
                "ring label {:?} overlaps axis {:?}: {:?} / {:?}",
                label.text,
                axis.text,
                label.bounds,
                axis.bounds
            );
        }
    }
}

#[test]
fn coach_and_bench_radar_tick_labels_do_not_overlap_each_other_or_axis_names() {
    let coach = radar_spec(vec![Some(8.0), Some(7.0), Some(6.0), Some(5.0), Some(4.0), Some(3.0), Some(2.0)]);
    assert_radar_labels_do_not_overlap(&coach);

    let bench = RadarChartSpec {
        axes: (0..8)
            .map(|index| RadarAxis {
                label: [
                    "speed",
                    "accuracy",
                    "rhythm",
                    "transitions",
                    "reach",
                    "reaction",
                    "correction",
                    "Japanese",
                ][index]
                    .into(),
                value: Some(50.0),
            })
            .collect(),
        max_value: 100.0,
        rings: [25.0, 50.0, 75.0, 100.0]
            .into_iter()
            .map(|value| RadarRing {
                value,
                label: matches!(value as u32, 50 | 100).then(|| format!("{value:.0}")),
            })
            .collect(),
        color: Color::LightCyan,
        cells: Size::new(50, 14),
        missing_label: "under 3".into(),
    };
    assert_radar_labels_do_not_overlap(&bench);
}

#[test]
fn sparkline_emphasises_its_last_point() {
    let spec = SparklineSpec {
        values: vec![20.0, 35.0, 25.0, 45.0],
        color: Color::Cyan,
        cells: Size::new(24, 3),
    };
    let image = render_sparkline(CellPixels::new(10, 20), &spec).unwrap();
    let color = rgba(spec.color);
    let coloured_in_column = |x: u32| {
        (0..image.height())
            .filter(|&y| same_rgb_with_rounding(image.get_pixel(x, y).0, color))
            .count()
    };
    assert!(
        coloured_in_column(image.width() - 3) > coloured_in_column(2),
        "last point is not larger than the first point"
    );
}

#[test]
fn radar_cache_reuses_equal_input_and_invalidates_one_changed_value() {
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let mut cache = AnalysisImageCache::new();
    let graphics = detection();
    let spec = radar_spec(vec![Some(1.0), Some(2.0), None, Some(4.0)]);

    cache.prepare_radar(&graphics, &font, &spec).unwrap();
    cache.prepare_radar(&graphics, &font, &spec).unwrap();
    assert_eq!(cache.creation_count(), 1);

    let mut changed = spec.clone();
    changed.axes[0].value = Some(1.01);
    cache.prepare_radar(&graphics, &font, &changed).unwrap();
    assert_eq!(cache.creation_count(), 2);
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
                raw_points: Vec::new(),
                points: vec![
                    TrendPoint { date: date(1), value: 20.0 },
                    TrendPoint { date: date(3), value: 30.0 },
                    TrendPoint { date: date(10), value: 40.0 },
                ],
            },
            TrendSeries {
                name: "bench-A".into(),
                color: SERIES_COLORS[1],
                raw_points: Vec::new(),
                points: vec![TrendPoint { date: date(1), value: 35.0 }, TrendPoint { date: date(10), value: 25.0 }],
            },
        ],
        forecasts,
        x_ticks: Vec::new(),
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
fn trend_chart_draws_raw_runs_as_faint_points_without_changing_the_mainline() {
    let baseline = calendar_chart(Vec::new());
    let baseline_image = render_trend_chart(CellPixels::new(4, 4), &baseline).unwrap();
    let mut with_raw = baseline.clone();
    with_raw.series[0].raw_points = vec![TrendPoint { date: date(3), value: 38.0 }];
    let raw_image = render_trend_chart(CellPixels::new(4, 4), &with_raw).unwrap();
    assert_ne!(raw_image, baseline_image);
    assert_eq!(with_raw.series[0].points, baseline.series[0].points);
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

fn v3_chart_record(day: u32, part: &str, form: u8, wpm: f64) -> SessionRecord {
    let mut row = record(day, wpm);
    let interval_us = (12_000_000.0 / wpm) as u64;
    row.log.keystrokes[0].t_us = 100_000;
    row.log.keystrokes[1].t_us = 100_000 + interval_us;
    row.header.mode = "bench".into();
    row.header.bench_id = Some(format!("bench-v3/{part}/{form:02}"));
    row
}

#[test]
fn trend_charts_draw_every_benchmark_version_as_one_series() {
    let mut v1 = record(1, 20.0);
    v1.header.mode = "bench".into();
    v1.header.bench_id = Some("bench-v1/A/01".into());
    let mut v2 = record(2, 40.0);
    v2.header.mode = "bench".into();
    v2.header.bench_id = Some("bench-v2/A/01".into());

    // MYTASK-3642: one «bench» series whatever the version and part.
    let spec = build_trend_data(&[v1, v2], MetricKind::NetWpm, Lang::En).unwrap();
    assert_eq!(spec.series.len(), 1);
    assert_eq!(spec.series[0].name, "bench");
    let values = spec.series[0].points.iter().map(|point| point.value).collect::<Vec<_>>();
    assert_eq!(values, [20.0, 40.0]);
}

#[test]
fn v3_trend_keeps_raw_runs_and_draws_the_rolling_three_mainline_separately_from_v2() {
    let mut records = Vec::new();
    for (day, wpm) in [(1, 60.0), (2, 120.0), (3, 180.0), (4, 240.0)] {
        records.push(v3_chart_record(day, "EW", day as u8, wpm));
        records.push(v3_chart_record(day, "ES", day as u8, wpm));
    }
    let mut old = record(1, 35.0);
    old.header.mode = "bench".into();
    old.header.bench_id = Some("bench-v2/A/01".into());
    records.push(old);

    let spec = build_trend_data(&records, MetricKind::NetWpm, Lang::En).unwrap();
    let v3 = spec.series.iter().find(|series| series.name == "bench v3 English").unwrap();
    assert_eq!(v3.raw_points.len(), 4);
    assert_eq!(
        v3.points.iter().map(|point| point.value).collect::<Vec<_>>(),
        rolling_median_3(&v3.raw_points.iter().map(|point| point.value).collect::<Vec<_>>())
    );
    assert!(spec.series.iter().any(|series| series.name == "bench v2 (previous series)"));
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
    render_buffer_size(app, 120, 40)
}

fn render_buffer_size(app: &rs_typing_trainer::app::App, width: u16, height: u16) -> ratatui::buffer::Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    terminal.backend().buffer().clone()
}

fn rendered_rows(app: &rs_typing_trainer::app::App) -> Vec<String> {
    let buffer = render_buffer(app);
    buffer
        .content()
        .chunks(120)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect()
}

fn bigram(from: &str, to: &str, iki: f64, error_rate: f64, n: u64) -> BigramMetric {
    BigramMetric {
        from: from.into(),
        to: to.into(),
        class: TransitionClass::SameHandAdjacent,
        n,
        median_ms: iki,
        error_n: (error_rate * n as f64) as u64,
        attempts: n,
        error_rate,
        estimated_iki_ms: iki,
        estimated_error_rate: error_rate,
        estimated_iki_lo_ms: iki * 0.9,
        estimated_iki_hi_ms: iki * 1.1,
        relative_iki: 1.0,
    }
}

#[test]
fn image_transition_and_finger_tabs_keep_terminal_values_around_the_figures() {
    let mut app = common::test_app(Config::default());
    app.handle_key(ratatui::crossterm::event::KeyEvent::new(
        ratatui::crossterm::event::KeyCode::Char('a'),
        ratatui::crossterm::event::KeyModifiers::NONE,
    ));
    app.analysis_details.bigrams = vec![
        bigram("a", "s", 400.0, 0.05, 100),
        bigram("s", "d", 200.0, 0.10, 100),
        bigram("d", "f", 100.0, 0.20, 100),
        bigram("f", "g", 300.0, 0.15, 100),
        bigram("g", "h", 250.0, 0.08, 100),
    ];
    app.analysis_details.fingers = vec![
        group("left-pinky", 250.0, 0.95, 100),
        group("left-index", 180.0, 0.99, 100),
        group("thumb", 150.0, 0.995, 100),
        group("right-index", 190.0, 0.98, 100),
        group("right-pinky", 300.0, 0.90, 100),
    ];
    app.analysis_details.hands = vec![group("left", 210.0, 0.98, 100), group("right", 230.0, 0.97, 100)];
    app.analysis_details.same_finger_median_ms = 320.0;
    app.analysis_details.alternating_hands_median_ms = 170.0;
    app.set_graphics_detection(detection());

    app.analysis_tab = rs_typing_trainer::app::ANALYSIS_TRANSITIONS_TAB;
    let transitions = rendered_rows(&app).join("\n");
    assert!(transitions.contains("a→s"), "transition labels disappeared:\n{transitions}");
    assert!(transitions.contains("400.0 ms"), "transition values disappeared:\n{transitions}");

    app.analysis_tab = rs_typing_trainer::app::ANALYSIS_FINGERS_TAB;
    let finger_rows = rendered_rows(&app);
    let shared_label_row = &finger_rows[finger_rows
        .iter()
        .position(|row| row.contains("left-pinky     accuracy"))
        .expect("raw finger row")
        - 1];
    assert_eq!(
        shared_label_row.matches('T').count(),
        1,
        "shared thumb must have one label: {shared_label_row}"
    );
    let fingers = finger_rows.join("\n");
    assert!(fingers.contains("IKI ↑"), "finger IKI axis is absent:\n{fingers}");
    assert!(fingers.contains("Error ↓"), "finger error axis is absent:\n{fingers}");
    assert!(fingers.contains("left-pinky"), "raw finger rows disappeared:\n{fingers}");
    assert!(fingers.contains("Same finger"), "raw comparison rows disappeared:\n{fingers}");

    app.analysis_details.fingers = vec![
        group("left-pinky", 250.0, 0.95, 100),
        group("left-thumb", 140.0, 0.99, 100),
        group("right-thumb", 160.0, 0.98, 100),
        group("right-pinky", 300.0, 0.90, 100),
    ];
    let split_rows = rendered_rows(&app);
    let split_label_row = &split_rows[split_rows
        .iter()
        .position(|row| row.contains("left-pinky     accuracy"))
        .expect("raw finger row")
        - 1];
    assert_eq!(
        split_label_row.matches('T').count(),
        2,
        "split thumbs must have two labels: {split_label_row}"
    );
}

#[test]
fn transition_tab_transmits_one_keyboard_image_containing_keycaps_and_arrow() {
    let mut app = common::test_app(Config::default());
    app.handle_key(ratatui::crossterm::event::KeyEvent::new(
        ratatui::crossterm::event::KeyCode::Char('a'),
        ratatui::crossterm::event::KeyModifiers::NONE,
    ));
    app.analysis_tab = rs_typing_trainer::app::ANALYSIS_TRANSITIONS_TAB;
    app.analysis_details.bigrams = vec![
        bigram("a", "s", 400.0, 0.00, 100),
        bigram("s", "d", 300.0, 0.05, 100),
        bigram("d", "f", 200.0, 0.10, 100),
        bigram("f", "g", 150.0, 0.15, 100),
        bigram("g", "h", 100.0, 0.20, 100),
    ];
    app.set_graphics_detection(detection());

    let buffer = render_buffer_size(&app, 160, 50);
    let transmitted = buffer
        .content()
        .iter()
        .filter_map(|cell| kitty_rgba(cell.symbol()))
        .filter(|(width, height, _)| (*width, *height) == (76 * 9, 11 * 20))
        .collect::<Vec<_>>();
    assert_eq!(transmitted.len(), 1, "keyboard and arrow must be one transmitted image");
    let (_, _, rgba) = &transmitted[0];
    let pixel = |x: usize, y: usize| {
        let offset = (y * 76 * 9 + x) * 4;
        [rgba[offset], rgba[offset + 1], rgba[offset + 2], rgba[offset + 3]]
    };
    let key_face = pixel(9, 10);
    assert!(key_face[3] > 0, "the combined image has no keycap pixels: {key_face:?}");
    assert!(
        rgba.as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[0] == 41 && pixel[1] == 184 && pixel[2] == 219 && pixel[3] > 0),
        "the combined image has no transition-arrow pixels"
    );
}

#[test]
fn finger_labels_are_at_every_bar_centre_in_a_160_by_50_terminal() {
    for (lang, expected) in [
        (Lang::En, ["P", "—", "—", "I", "T", "I", "—", "—", "P"]),
        (Lang::Ja, ["小", "—", "—", "人", "親", "人", "—", "—", "小"]),
    ] {
        let mut config = Config::default();
        config.general.lang = lang;
        let mut app = common::test_app(config);
        app.handle_key(ratatui::crossterm::event::KeyEvent::new(
            ratatui::crossterm::event::KeyCode::Char('a'),
            ratatui::crossterm::event::KeyModifiers::NONE,
        ));
        app.analysis_tab = rs_typing_trainer::app::ANALYSIS_FINGERS_TAB;
        app.analysis_details.fingers = vec![
            group("left-pinky", 250.0, 0.95, 100),
            group("left-index", 180.0, 0.99, 100),
            group("thumb", 150.0, 0.995, 100),
            group("right-index", 190.0, 0.98, 100),
            group("right-pinky", 300.0, 0.90, 100),
        ];
        app.analysis_details.hands = vec![group("left", 210.0, 0.98, 100), group("right", 230.0, 0.97, 100)];
        app.analysis_details.same_finger_median_ms = 320.0;
        app.analysis_details.alternating_hands_median_ms = 170.0;
        app.set_graphics_detection(detection());

        let buffer = render_buffer_size(&app, 160, 50);
        let visual_area = Rect::new(1, 3, 158, 12);
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(3, 5), Constraint::Ratio(2, 5)])
            .split(visual_area);
        let chart_x = columns[0].x + 9;
        let chart_cells = columns[0].width - 9;
        let label_y = visual_area.y + visual_area.height - 1;
        let chart_pixels = u32::from(chart_cells) * 9;
        for (index, label) in expected.into_iter().enumerate() {
            let center_pixel = ((index as f64 + 0.5) * f64::from(chart_pixels) / 9.0).floor() as u32;
            let column = chart_x + (center_pixel / 9) as u16;
            assert_eq!(buffer[(column, label_y)].symbol(), label, "{lang:?} bar {index} at column {column}");
        }
    }
}

fn add_test_grades(app: &mut rs_typing_trainer::app::App) {
    let values = [
        ComponentValue::new(Component::Speed, 45.0),
        ComponentValue::new(Component::Accuracy, 97.0),
        ComponentValue::new(Component::Rhythm, 0.25),
    ];
    let report = app.analysis_coach.as_mut().expect("coach report");
    report.grades.languages = vec![
        evaluate_language("en", &values, GradeSource::Provisional, GradeStatus::Provisional, 3).unwrap(),
        evaluate_language("ja", &values, GradeSource::Provisional, GradeStatus::Provisional, 3).unwrap(),
    ];
}

#[test]
fn image_coach_stacks_language_ladders_and_keeps_the_coach_text() {
    let mut app = common::test_app(Config::default());
    app.handle_key(ratatui::crossterm::event::KeyEvent::new(
        ratatui::crossterm::event::KeyCode::Char('a'),
        ratatui::crossterm::event::KeyModifiers::NONE,
    ));
    add_test_grades(&mut app);
    app.set_graphics_detection(detection());

    let rows = rendered_rows(&app);
    let english = rows.iter().position(|row| row.contains("English")).expect("English ladder label");
    let japanese = rows.iter().position(|row| row.contains("Japanese")).expect("Japanese ladder label");
    assert!(
        japanese >= english + 3,
        "language ladders are not vertically stacked: {english}, {japanese}"
    );
    assert!(rows.iter().any(|row| row.contains("WHERE YOU ARE NOW")), "coach prose was removed");
}

#[test]
fn graphics_off_and_goal_picker_keep_the_text_only_coach_and_bench_buffers() {
    let make_app = || {
        let mut app = common::test_app(Config::default());
        app.handle_key(ratatui::crossterm::event::KeyEvent::new(
            ratatui::crossterm::event::KeyCode::Char('a'),
            ratatui::crossterm::event::KeyModifiers::NONE,
        ));
        add_test_grades(&mut app);
        app.analysis_records = (1..=4)
            .map(|day| {
                let mut row = record(day, 30.0 + f64::from(day));
                row.header.bench_id = Some(format!("bench-v1/A/{day:02}"));
                row
            })
            .collect();
        app
    };

    for tab in [rs_typing_trainer::app::ANALYSIS_COACH_TAB, rs_typing_trainer::app::ANALYSIS_BENCH_TAB] {
        let mut baseline = make_app();
        baseline.analysis_tab = tab;
        let mut explicit_off = make_app();
        explicit_off.analysis_tab = tab;
        explicit_off.set_graphics_detection(GraphicsDetection {
            mode: GraphicsMode::Off,
            cell_pixels: Some(CellPixels::new(9, 20)),
            reason: GraphicsReason::Disabled,
        });
        assert_eq!(render_buffer(&explicit_off), render_buffer(&baseline), "tab {tab}");
    }

    let mut baseline_picker = make_app();
    baseline_picker.analysis_goal_picker = true;
    let mut image_picker = make_app();
    image_picker.analysis_goal_picker = true;
    image_picker.set_graphics_detection(detection());
    assert_eq!(render_buffer(&image_picker), render_buffer(&baseline_picker));
}

#[test]
fn image_bench_adds_a_sparkline_row_and_keeps_the_existing_part_value_row() {
    let mut app = common::test_app(Config::default());
    app.handle_key(ratatui::crossterm::event::KeyEvent::new(
        ratatui::crossterm::event::KeyCode::Char('a'),
        ratatui::crossterm::event::KeyModifiers::NONE,
    ));
    app.analysis_tab = rs_typing_trainer::app::ANALYSIS_BENCH_TAB;
    app.analysis_records = (1..=4)
        .map(|day| {
            let mut row = record(day, 30.0 + f64::from(day));
            row.header.bench_id = Some(format!("bench-v1/A/{day:02}"));
            row
        })
        .collect();
    app.set_graphics_detection(detection());

    let text = rendered_rows(&app).join("\n");
    assert_eq!(
        text.matches("Part A English words").count(),
        2,
        "sparkline label and original numeric row must both remain:\n{text}"
    );
    assert!(text.contains("WPM"), "raw WPM row was removed: {text}");
}

#[test]
fn graphics_off_keeps_the_existing_analysis_text_buffer() {
    let mut baseline = common::test_app(Config::default());
    baseline.screen = rs_typing_trainer::app::Screen::Analysis;
    baseline.analysis_tab = rs_typing_trainer::app::ANALYSIS_TREND_TAB;
    baseline.analysis_records = vec![record(1, 20.0), record(3, 30.0)];

    let mut explicit_off = common::test_app(Config::default());
    explicit_off.screen = rs_typing_trainer::app::Screen::Analysis;
    explicit_off.analysis_tab = rs_typing_trainer::app::ANALYSIS_TREND_TAB;
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
    app.analysis_tab = rs_typing_trainer::app::ANALYSIS_TREND_TAB;
    app.analysis_records = vec![record(1, 20.0), record(10, 30.0)];
    app.set_graphics_detection(detection());

    let buffer = render_buffer(&app);
    let text = buffer.content().iter().map(|cell| cell.symbol()).collect::<String>();
    assert!(text.contains("net WPM / daily median"), "{text}");
    assert!(text.contains("random"), "{text}");
    assert!(text.contains("2026/01/01"), "{text}");
    assert!(text.contains("01/10"), "{text}");
}

#[test]
fn text_chart_keeps_the_existing_unknown_bucket_for_an_invalid_timestamp() {
    let mut app = common::test_app(Config::default());
    app.screen = rs_typing_trainer::app::Screen::Analysis;
    app.analysis_tab = rs_typing_trainer::app::ANALYSIS_TREND_TAB;
    let mut invalid = record(10, 30.0);
    invalid.header.finished_at = "not-a-timestamp".into();
    app.analysis_records = vec![record(1, 20.0), invalid];

    let buffer = render_buffer(&app);
    let text = buffer.content().iter().map(|cell| cell.symbol()).collect::<String>();
    assert!(text.contains("2026/01/01"), "{text}");
    assert!(text.contains("unknown"), "{text}");
}

fn skill_chart_spec() -> SkillChartSpec {
    let point = |day: u32, value: f64| TrendPoint { date: date(day), value };
    SkillChartSpec {
        cells: Size::new(30, 3),
        x_axis: TrendDateRange {
            start: date(1),
            end: date(10),
        }
        .into(),
        x_ticks: Vec::new(),
        y_range: (1.3, 1.7),
        points: (1..=10).map(|day| point(day, 1.7 - f64::from(day) * 0.03)).collect(),
        medians: (5..=10).map(|day| point(day, 1.62 - f64::from(day) * 0.02)).collect(),
        bench: vec![point(3, 1.5)],
        boundaries: vec![2.2, 2.0, 1.8, 1.6, 1.5, 1.4, 1.3, 1.2, 1.1],
        current_band: Some(4),
        current_color: Color::LightBlue,
        point_color: Color::White,
        line_color: Color::LightCyan,
        bench_color: Color::LightYellow,
    }
}

#[test]
fn skill_chart_draws_bands_faint_points_a_thick_median_and_bench_diamonds() {
    let spec = skill_chart_spec();
    let cells = CellPixels::new(9, 20);
    let image = render_skill_chart(cells, &spec).unwrap();
    assert_eq!(image.width(), 30 * 9);
    assert_eq!(image.height(), 3 * 20);
    // The bench diamond is drawn in the bench colour (opaque), the run points
    // only faintly.
    let yellow = image
        .pixels()
        .filter(|pixel| pixel[3] == 255 && pixel[0] > 200 && pixel[1] > 200 && pixel[2] < 160)
        .count();
    assert!(yellow > 10, "no bench diamond");
    let faint = image.pixels().filter(|pixel| pixel[3] > 0 && pixel[3] < 140).count();
    assert!(faint > 0, "no faint points or bands");
    let empty = render_skill_chart(
        cells,
        &SkillChartSpec {
            y_range: (1.0, 1.0),
            ..skill_chart_spec()
        },
    )
    .unwrap();
    assert!(empty.pixels().all(|pixel| pixel[3] == 0));
}

#[test]
fn skill_chart_images_are_cached_by_their_content() {
    let mut cache = AnalysisImageCache::new();
    let spec = skill_chart_spec();
    cache.prepare_skill_chart(&detection(), &spec).unwrap();
    cache.prepare_skill_chart(&detection(), &spec).unwrap();
    assert_eq!(cache.creation_count(), 1);
    let mut moved = spec.clone();
    moved.bench.clear();
    cache.prepare_skill_chart(&detection(), &moved).unwrap();
    assert_eq!(cache.creation_count(), 2);
}
