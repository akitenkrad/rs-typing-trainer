use std::path::Path;
use std::time::{Duration, Instant};

use ratatui::layout::Size;
use rs_typing_trainer::graphics::{CellPixels, GraphicsDetection, GraphicsMode, GraphicsReason};
use rs_typing_trainer::ui::bigtext_image::resolve_big_font;
use rs_typing_trainer::ui::theme::Theme;
use rs_typing_trainer::ui::title_image::{TitleImageCache, TitleLogoSpec};

#[test]
fn reports_cold_and_cached_title_logo_creation_time() {
    let _guard = crate::perf_lock();
    let graphics = GraphicsDetection {
        mode: GraphicsMode::Kitty,
        cell_pixels: Some(CellPixels::new(9, 20)),
        reason: GraphicsReason::Explicit(GraphicsMode::Kitty),
    };
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let spec = TitleLogoSpec::from_theme(Size::new(80, 7), Theme::default());
    let mut cache = TitleImageCache::new();

    let started = Instant::now();
    cache.prepare(&graphics, &font, &spec).unwrap();
    let cold = started.elapsed();
    let started = Instant::now();
    cache.prepare(&graphics, &font, &spec).unwrap();
    let cached = started.elapsed();
    eprintln!(
        "title logo: cold={:.3} ms cached={:.3} ms images={} (debug_assertions={})",
        cold.as_secs_f64() * 1_000.0,
        cached.as_secs_f64() * 1_000.0,
        cache.creation_count(),
        cfg!(debug_assertions),
    );

    if !cfg!(debug_assertions) {
        assert!(cold <= Duration::from_millis(60), "cold creation {cold:?} exceeded 60 ms");
        assert!(cached <= Duration::from_millis(1), "cached lookup {cached:?} exceeded 1 ms");
    }
}
