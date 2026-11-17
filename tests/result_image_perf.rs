use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::NaiveDate;
use ratatui::layout::Size;
use ratatui::style::Color;
use rs_typing_trainer::graphics::{CellPixels, GraphicsDetection, GraphicsMode, GraphicsReason};
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::ui::bigtext_image::resolve_big_font;
use rs_typing_trainer::ui::keyboard::KeyTilePlacement;
use rs_typing_trainer::ui::keyboard_image::KeyTileSpec;
use rs_typing_trainer::ui::result_image::{
    BenchPartCardSpec, BenchTrendPoint, BenchTrendSpec, ItemSpeedPoint, MetricCardSpec, MistypeKeyboardSpec, ReactionHistogramSpec, ResultImageCache,
    ResultImageColors, SpeedChartSpec,
};

static PERF_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn reports_cold_and_cached_result_card_creation_time() {
    let _guard = PERF_LOCK.lock().unwrap();
    let graphics = GraphicsDetection {
        mode: GraphicsMode::Kitty,
        cell_pixels: Some(CellPixels::new(9, 20)),
        reason: GraphicsReason::Explicit(GraphicsMode::Kitty),
    };
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let colors = ResultImageColors {
        foreground: Color::White,
        muted: Color::Gray,
        correct: Color::LightGreen,
        mistype: Color::LightRed,
        accent: Color::LightCyan,
        warning: Color::LightYellow,
        panel: Color::Rgb(16, 20, 28),
    };
    let spec = MetricCardSpec {
        label: "WPM".into(),
        value: "72.4".into(),
        previous_delta: Some(3.2),
        best_delta: Some(-1.6),
        recent: (0..10).map(|value| 55.0 + f64::from(value)).collect(),
        lang: Lang::En,
        delta_suffix: String::new(),
        cells: Size::new(48, 8),
        colors,
    };
    let mut cache = ResultImageCache::new();

    let speed = SpeedChartSpec {
        points: (0..25)
            .map(|index| ItemSpeedPoint {
                wpm: 40.0 + f64::from(index % 8),
                mistypes: Some(u32::from(index % 7 == 0)),
            })
            .collect(),
        lang: Lang::En,
        cells: Size::new(80, 9),
        colors,
    };
    let reaction = ReactionHistogramSpec {
        reaction_seconds: (0..100).map(|index| f64::from(index) * 0.018).collect(),
        p50_seconds: 0.45,
        p90_seconds: 1.20,
        cells: Size::new(50, 9),
        colors,
    };
    let keyboard = MistypeKeyboardSpec {
        placements: vec![KeyTilePlacement {
            x: 2,
            y: 2,
            spec: KeyTileSpec {
                cells: Size::new(5, 2),
                label: "a 8".into(),
                face_color: Color::LightRed,
                text_color: Color::White,
                bold: true,
                pressed: false,
                modifier: false,
                finger_color: None,
            },
        }],
        cells: Size::new(80, 10),
    };

    let prepare = |cache: &mut ResultImageCache| {
        cache.prepare_metric_card(&graphics, &font, &spec).unwrap();
        cache.prepare_speed_chart(&graphics, &font, &speed).unwrap();
        cache.prepare_reaction_histogram(&graphics, &font, &reaction).unwrap();
        cache.prepare_mistype_keyboard(&graphics, &keyboard).unwrap();
    };
    let started = Instant::now();
    prepare(&mut cache);
    let cold = started.elapsed();
    let started = Instant::now();
    prepare(&mut cache);
    let cached = started.elapsed();
    eprintln!(
        "result screen images: cold={:.3} ms cached={:.3} ms images={} (debug_assertions={})",
        cold.as_secs_f64() * 1_000.0,
        cached.as_secs_f64() * 1_000.0,
        cache.creation_count(),
        cfg!(debug_assertions),
    );
    assert_eq!(cache.creation_count(), 4);
    if !cfg!(debug_assertions) {
        assert!(cold <= Duration::from_millis(60), "cold creation {cold:?} exceeded 60 ms");
        assert!(cached <= Duration::from_millis(1), "cached lookup {cached:?} exceeded 1 ms");
    }
}

#[test]
fn reports_cold_and_cached_benchmark_summary_image_time() {
    let _guard = PERF_LOCK.lock().unwrap();
    let graphics = GraphicsDetection {
        mode: GraphicsMode::Kitty,
        cell_pixels: Some(CellPixels::new(9, 20)),
        reason: GraphicsReason::Explicit(GraphicsMode::Kitty),
    };
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let colors = ResultImageColors {
        foreground: Color::White,
        muted: Color::Gray,
        correct: Color::LightGreen,
        mistype: Color::LightRed,
        accent: Color::LightCyan,
        warning: Color::LightYellow,
        panel: Color::Rgb(16, 20, 28),
    };
    let cards = (0..4)
        .map(|index| BenchPartCardSpec {
            label: format!("Part {}", char::from(b'A' + index)),
            wpm: 55.0 + f64::from(index),
            accuracy: 0.95,
            previous_wpm_delta: Some(2.0),
            best_wpm_delta: Some(-1.0),
            previous_accuracy_delta: Some(0.01),
            best_accuracy_delta: Some(-0.005),
            version: format!("v2/{}/{:02}", char::from(b'A' + index), index + 1),
            lang: Lang::En,
            cells: Size::new(31, 9),
            colors,
        })
        .collect::<Vec<_>>();
    let trends = (0..4)
        .map(|part| BenchTrendSpec {
            points: (0..20)
                .map(|index| BenchTrendPoint {
                    date: NaiveDate::from_ymd_opt(2026, 10, 1 + index as u32).unwrap(),
                    wpm: 45.0 + f64::from((index + part) % 9),
                    accuracy: 0.94 + f64::from(index % 5) / 100.0,
                    starts_new_series: part % 2 == 0 && index == 10,
                    current: index == 19,
                })
                .collect(),
            cells: Size::new(36, 9),
            colors,
        })
        .collect::<Vec<_>>();
    let mut cache = ResultImageCache::new();
    let prepare = |cache: &mut ResultImageCache| {
        for card in &cards {
            cache.prepare_bench_part_card(&graphics, &font, card).unwrap();
        }
        for trend in &trends {
            cache.prepare_bench_trend(&graphics, &font, trend).unwrap();
        }
    };
    let started = Instant::now();
    prepare(&mut cache);
    let cold = started.elapsed();
    let started = Instant::now();
    prepare(&mut cache);
    let cached = started.elapsed();
    eprintln!(
        "benchmark summary images: cold={:.3} ms cached={:.3} ms images={} (debug_assertions={})",
        cold.as_secs_f64() * 1_000.0,
        cached.as_secs_f64() * 1_000.0,
        cache.creation_count(),
        cfg!(debug_assertions),
    );
    assert_eq!(cache.creation_count(), 8);
    if !cfg!(debug_assertions) {
        assert!(cold <= Duration::from_millis(100), "cold creation {cold:?} exceeded 100 ms");
        assert!(cached <= Duration::from_millis(1), "cached lookup {cached:?} exceeded 1 ms");
    }
}
