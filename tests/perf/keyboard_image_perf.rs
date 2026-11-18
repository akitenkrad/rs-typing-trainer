use std::time::{Duration, Instant};

use rs_typing_trainer::graphics::{CellPixels, GraphicsDetection, GraphicsMode, GraphicsReason};
use rs_typing_trainer::layout::geometry::geometry;
use rs_typing_trainer::layout::{KeyStroke, builtin_layout};
use rs_typing_trainer::ui::keyboard::{KeyFeedback, KeyboardState, KeyboardView, prepare_keycap_images};
use rs_typing_trainer::ui::keyboard_image::KeyboardImageCache;

#[test]
fn prewarmed_keypress_tile_latency() {
    let _guard = crate::perf_lock();
    let physical = geometry("ansi").unwrap();
    let layout = builtin_layout("qwerty").unwrap();
    let graphics = GraphicsDetection {
        mode: GraphicsMode::Kitty,
        cell_pixels: Some(CellPixels::new(9, 20)),
        reason: GraphicsReason::Explicit(GraphicsMode::Kitty),
    };
    let view = KeyboardView {
        show: "full",
        style: "keycap",
        finger_guide: true,
    };
    let mut cache = KeyboardImageCache::new();
    prepare_keycap_images(&graphics, &mut cache, &physical, &layout, &KeyboardState::default(), view, false).unwrap();

    let codes = physical
        .rows
        .iter()
        .flatten()
        .filter(|key| layout.key(key.code).is_some())
        .map(|key| key.code.to_owned())
        .collect::<Vec<_>>();
    let mut samples = Vec::with_capacity(200);
    for index in 0..200 {
        let expected = codes[(index + 1) % codes.len()].clone();
        let pressed = codes[index % codes.len()].clone();
        let state = KeyboardState {
            next: Some(KeyStroke {
                code: expected.clone(),
                shift: false,
            }),
            last: Some(KeyFeedback {
                pressed_code: pressed,
                expected_code: Some(expected),
                correct: index % 2 == 0,
            }),
            ..KeyboardState::default()
        };
        let started = Instant::now();
        prepare_keycap_images(&graphics, &mut cache, &physical, &layout, &state, view, false).unwrap();
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    let median = samples[samples.len() / 2];
    let maximum = *samples.last().unwrap();
    eprintln!(
        "keyboard image keypress latency: median={:.3} ms max={:.3} ms tiles={} (debug_assertions={})",
        median.as_secs_f64() * 1_000.0,
        maximum.as_secs_f64() * 1_000.0,
        cache.creation_count(),
        cfg!(debug_assertions),
    );
    if !cfg!(debug_assertions) {
        assert!(median <= Duration::from_millis(2), "median {median:?} exceeded 2 ms (max {maximum:?})");
    }
}
