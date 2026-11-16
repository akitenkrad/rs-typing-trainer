use std::time::{Duration, Instant};

use chrono::{TimeZone, Utc};
use ratatui::layout::Size;
use rs_typing_trainer::config::Config;
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::graphics::{CellPixels, GraphicsDetection, GraphicsMode, GraphicsReason};
use rs_typing_trainer::record::SessionRecord;
use rs_typing_trainer::ui::analysis_image::{AnalysisImageCache, MetricKind, TransitionArrowSpec, TransitionKeyboardSpec, build_trend_data};
use rs_typing_trainer::ui::analysis_image::{RadarAxis, RadarChartSpec, RadarRing};
use rs_typing_trainer::ui::bigtext_image::resolve_big_font;
use rs_typing_trainer::ui::keyboard::{KeyboardState, KeyboardView, keycap_center_cells, keycap_tile_placements};
use rs_typing_trainer::{layout, layout::geometry::geometry};
use std::path::Path;
use std::sync::Mutex;

static PERF_LOCK: Mutex<()> = Mutex::new(());

fn record(day: u32, series: usize) -> SessionRecord {
    let config = Config::default();
    let mut session = Session::new(Duration::from_secs(1), Duration::ZERO, Item::exact("as"));
    session.activate(Duration::ZERO);
    session.input_char(Duration::from_millis(100), "a");
    session.input_char(Duration::from_millis(200), "s");
    session.tick(Duration::from_secs(1));
    let mut record = SessionRecord::from_log(&config, session.into_log(), Utc.with_ymd_and_hms(2026, 1, day, 0, 0, 0).unwrap());
    record.header.mode = ["words", "random", "sentence"][series].into();
    record.header.net_wpm = 30.0 + day as f64 * 0.7 + series as f64 * 8.0;
    record
}

#[test]
fn reports_cold_and_cached_speed_chart_creation_time() {
    let _guard = PERF_LOCK.lock().unwrap();
    let records = (1..=30).flat_map(|day| (0..3).map(move |series| record(day, series))).collect::<Vec<_>>();
    let mut spec = build_trend_data(&records, MetricKind::NetWpm, rs_typing_trainer::i18n::Lang::En).unwrap();
    spec.cells = Size::new(100, 10);
    let graphics = GraphicsDetection {
        mode: GraphicsMode::Kitty,
        cell_pixels: Some(CellPixels::new(9, 20)),
        reason: GraphicsReason::Explicit(GraphicsMode::Kitty),
    };
    let mut cache = AnalysisImageCache::new();

    let started = Instant::now();
    cache.prepare_chart(&graphics, &spec).unwrap();
    let cold = started.elapsed();
    let started = Instant::now();
    cache.prepare_chart(&graphics, &spec).unwrap();
    let cached = started.elapsed();
    eprintln!(
        "analysis speed chart: cold={:.3} ms cached={:.3} ms images={} (debug_assertions={})",
        cold.as_secs_f64() * 1_000.0,
        cached.as_secs_f64() * 1_000.0,
        cache.creation_count(),
        cfg!(debug_assertions),
    );

    if !cfg!(debug_assertions) {
        assert!(cold <= Duration::from_millis(30), "cold creation {cold:?} exceeded 30 ms");
        assert!(cached <= Duration::from_millis(1), "cached lookup {cached:?} exceeded 1 ms");
    }
}

#[test]
fn reports_cold_and_cached_radar_creation_time() {
    let _guard = PERF_LOCK.lock().unwrap();
    let graphics = GraphicsDetection {
        mode: GraphicsMode::Kitty,
        cell_pixels: Some(CellPixels::new(9, 20)),
        reason: GraphicsReason::Explicit(GraphicsMode::Kitty),
    };
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let spec = RadarChartSpec {
        axes: [
            "Sustained speed",
            "Accuracy",
            "Rhythm",
            "Transitions",
            "Reach",
            "Reaction",
            "Correction",
            "Japanese",
        ]
        .into_iter()
        .enumerate()
        .map(|(index, label)| RadarAxis {
            label: label.into(),
            value: (index != 6).then_some(12.5 * (index + 1) as f64),
        })
        .collect(),
        max_value: 100.0,
        rings: [25.0, 50.0, 75.0, 100.0]
            .into_iter()
            .map(|value| RadarRing {
                value,
                label: Some(format!("{value:.0}")),
            })
            .collect(),
        color: ratatui::style::Color::LightCyan,
        cells: Size::new(50, 14),
        missing_label: "under 3".into(),
    };
    let mut cache = AnalysisImageCache::new();

    let started = Instant::now();
    cache.prepare_radar(&graphics, &font, &spec).unwrap();
    let cold = started.elapsed();
    let started = Instant::now();
    cache.prepare_radar(&graphics, &font, &spec).unwrap();
    let cached = started.elapsed();
    eprintln!(
        "analysis radar: cold={:.3} ms cached={:.3} ms images={} (debug_assertions={})",
        cold.as_secs_f64() * 1_000.0,
        cached.as_secs_f64() * 1_000.0,
        cache.creation_count(),
        cfg!(debug_assertions),
    );

    if !cfg!(debug_assertions) {
        assert!(cold <= Duration::from_millis(30), "cold creation {cold:?} exceeded 30 ms");
        assert!(cached <= Duration::from_millis(1), "cached lookup {cached:?} exceeded 1 ms");
    }
}

#[test]
fn reports_cold_and_cached_transition_keyboard_creation_time() {
    let _guard = PERF_LOCK.lock().unwrap();
    let graphics = GraphicsDetection {
        mode: GraphicsMode::Kitty,
        cell_pixels: Some(CellPixels::new(9, 20)),
        reason: GraphicsReason::Explicit(GraphicsMode::Kitty),
    };
    let physical = geometry("ansi").unwrap();
    let keyboard_layout = layout::builtin_layout("qwerty").unwrap();
    let view = KeyboardView {
        show: "full",
        style: "keycap",
        finger_guide: false,
    };
    let arrow = TransitionArrowSpec {
        cells: Size::new(76, 11),
        from: keycap_center_cells(&physical, "KeyA"),
        to: keycap_center_cells(&physical, "KeyL"),
        same_key: false,
        color: ratatui::style::Color::LightCyan,
    };
    let keyboard = TransitionKeyboardSpec {
        cells: arrow.cells,
        tiles: keycap_tile_placements(&physical, &keyboard_layout, &KeyboardState::default(), view, true),
        arrow,
        selection: Some(("a".into(), "l".into())),
    };
    let mut analysis_cache = AnalysisImageCache::new();

    let started = Instant::now();
    analysis_cache.prepare_transition_keyboard(&graphics, &keyboard).unwrap();
    let cold = started.elapsed();
    let started = Instant::now();
    analysis_cache.prepare_transition_keyboard(&graphics, &keyboard).unwrap();
    let cached = started.elapsed();
    eprintln!(
        "analysis transition keyboard: cold={:.3} ms cached={:.3} ms combined_images={} (debug_assertions={})",
        cold.as_secs_f64() * 1_000.0,
        cached.as_secs_f64() * 1_000.0,
        analysis_cache.creation_count(),
        cfg!(debug_assertions),
    );
    if !cfg!(debug_assertions) {
        assert!(cold <= Duration::from_millis(30), "cold creation {cold:?} exceeded 30 ms");
        assert!(cached <= Duration::from_millis(1), "cached lookup {cached:?} exceeded 1 ms");
    }
}
