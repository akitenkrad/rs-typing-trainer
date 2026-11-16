use std::time::{Duration, Instant};

use chrono::{TimeZone, Utc};
use ratatui::layout::Size;
use rs_typing_trainer::config::Config;
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::graphics::{CellPixels, GraphicsDetection, GraphicsMode, GraphicsReason};
use rs_typing_trainer::record::SessionRecord;
use rs_typing_trainer::ui::analysis_image::{AnalysisImageCache, MetricKind, build_trend_data};

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
