use std::path::Path;
use std::time::{Duration, Instant};

use ratatui::layout::Size;
use ratatui::style::{Color, Modifier, Style};
use rs_typing_trainer::graphics::{CellPixels, GraphicsDetection, GraphicsMode, GraphicsReason};
use rs_typing_trainer::ui::bigtext::{BigTextSize, StyledCharacter};
use rs_typing_trainer::ui::bigtext_image::{BigTextImageCache, BigTileSpec, resolve_big_font};

fn detection() -> GraphicsDetection {
    GraphicsDetection {
        mode: GraphicsMode::Kitty,
        cell_pixels: Some(CellPixels::new(9, 20)),
        reason: GraphicsReason::Explicit(GraphicsMode::Kitty),
    }
}

fn spec(character: char, color: Color) -> BigTileSpec {
    BigTileSpec {
        size: BigTextSize::Dot16,
        cells: Size::new(if character.is_ascii() { 4 } else { 8 }, 4),
        character: StyledCharacter::new(character, Style::default().fg(color).add_modifier(Modifier::BOLD)),
    }
}

fn median(values: &mut [Duration]) -> Duration {
    values.sort_unstable();
    values[values.len() / 2]
}

#[test]
fn prewarmed_one_keystroke_changes_stay_within_budget() {
    let detected = detection();
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let mut cache = BigTextImageCache::new();
    for character in "タイピングtraining".chars() {
        cache.prepare(&detected, &font, &spec(character, Color::DarkGray)).unwrap();
    }

    let mut samples = Vec::with_capacity(200);
    for index in 0..200u8 {
        let start = Instant::now();
        cache
            .prepare(&detected, &font, &spec('亜', Color::Rgb(index, 220u8.wrapping_sub(index), 80)))
            .unwrap();
        cache
            .prepare(&detected, &font, &spec('A', Color::Rgb(80, index, 220u8.wrapping_sub(index))))
            .unwrap();
        samples.push(start.elapsed());
    }
    let median = median(&mut samples);
    println!("bigtext prewarmed one-keystroke median over 200 runs: {median:?}");
    if !cfg!(debug_assertions) {
        assert!(median <= Duration::from_millis(2), "median {median:?} exceeds 2 ms");
    }
}

#[test]
fn reports_cold_cjk_tile_creation_time() {
    let detected = detection();
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let mut cache = BigTextImageCache::new();
    let start = Instant::now();
    cache.prepare(&detected, &font, &spec('亜', Color::White)).unwrap();
    println!("bigtext cold CJK tile creation: {:?}", start.elapsed());
}
