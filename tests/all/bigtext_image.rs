use std::path::Path;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::layout::Size;
use ratatui::style::{Color, Modifier, Style};
use rs_typing_trainer::app::Screen;
use rs_typing_trainer::config::Config;
use rs_typing_trainer::graphics::{CellPixels, GraphicsDetection, GraphicsMode, GraphicsReason};
use rs_typing_trainer::ui;
use rs_typing_trainer::ui::bigtext::{BigTextSize, StyledCharacter};
use rs_typing_trainer::ui::bigtext_image::{
    BigFontKind, BigTextImageCache, BigTileSpec, bundled_outline_font, image_cell_height, render_big_tile, resolve_big_font,
};
use tempfile::TempDir;

use crate::common;

fn detection() -> GraphicsDetection {
    GraphicsDetection {
        mode: GraphicsMode::Kitty,
        cell_pixels: Some(CellPixels::new(9, 20)),
        reason: GraphicsReason::Explicit(GraphicsMode::Kitty),
    }
}

fn spec(character: char, style: Style) -> BigTileSpec {
    BigTileSpec {
        size: BigTextSize::Dot16,
        cells: Size::new(if character.is_ascii() { 4 } else { 8 }, 4),
        character: StyledCharacter::new(character, style),
    }
}

fn foreground_pixels(image: &image::RgbaImage) -> usize {
    image.pixels().filter(|pixel| pixel.0[3] != 0).count()
}

fn playing_app() -> common::TestApp {
    let mut config = Config::default();
    config.ui.animation_enabled = false;
    let mut app = common::test_app(config);
    let now = Instant::now();
    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    app.handle_key_at(enter, now);
    app.handle_key_at(enter, now);
    if app.screen == Screen::KeyPicker {
        app.handle_key_at(enter, now);
    }
    app.on_tick_at(Duration::from_secs(3), now).unwrap();
    app.on_tick_at(Duration::from_millis(1), now).unwrap();
    assert_eq!(app.screen, Screen::Playing);
    app
}

#[test]
fn bundled_biz_ud_contains_sample_and_generated_glyph_count() {
    let font = bundled_outline_font().unwrap();

    assert_eq!(font.glyph_count(), 7_421);
    for character in "あア亜Aa".chars() {
        assert!(font.contains(character), "missing {character}");
    }
}

#[test]
fn biz_ud_tiles_have_foreground_near_the_middle_and_transparent_corners() {
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    for character in ['亜', 'A'] {
        let tile = render_big_tile(
            &font,
            CellPixels::new(9, 20),
            &spec(character, Style::default().fg(Color::Rgb(20, 180, 240))),
        )
        .unwrap();
        for &(x, y) in &[
            (0, 0),
            (tile.width() - 1, 0),
            (0, tile.height() - 1),
            (tile.width() - 1, tile.height() - 1),
        ] {
            assert_eq!(tile.get_pixel(x, y).0[3], 0, "{character} corner ({x}, {y})");
        }
        let middle_has_ink =
            (tile.width() / 4..tile.width() * 3 / 4).any(|x| (tile.height() / 4..tile.height() * 3 / 4).any(|y| tile.get_pixel(x, y).0[3] != 0));
        assert!(middle_has_ink, "{character} has no foreground near the middle");
    }
}

#[test]
fn bold_overdraw_adds_foreground_pixels() {
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let normal = render_big_tile(&font, CellPixels::new(9, 20), &spec('亜', Style::default().fg(Color::White))).unwrap();
    let bold = render_big_tile(
        &font,
        CellPixels::new(9, 20),
        &spec('亜', Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
    )
    .unwrap();

    assert!(foreground_pixels(&bold) > foreground_pixels(&normal));
}

#[test]
fn shinonome_uses_an_integer_nearest_neighbour_scale() {
    let font = resolve_big_font("shinonome", Path::new("/unused"));
    let tile = render_big_tile(&font, CellPixels::new(9, 20), &spec('A', Style::default().fg(Color::White))).unwrap();
    // Dot16 A is 8x16 dots. A 36x80 halfwidth box therefore uses scale 4.
    // Every source dot becomes one uniform 4x4 alpha block.
    let origin_x = (tile.width() - 8 * 4) / 2;
    let origin_y = (tile.height() - 16 * 4) / 2;
    for block_y in 0..16 {
        for block_x in 0..8 {
            let x0 = origin_x + block_x * 4;
            let y0 = origin_y + block_y * 4;
            let alpha = tile.get_pixel(x0, y0).0[3];
            for y in y0..y0 + 4 {
                for x in x0..x0 + 4 {
                    assert_eq!(tile.get_pixel(x, y).0[3], alpha, "nonuniform block ({block_x}, {block_y})");
                }
            }
        }
    }
}

#[test]
fn image_mode_uses_four_rows_while_text_mode_keeps_eight() {
    assert_eq!(image_cell_height(BigTextSize::Dot16, CellPixels::new(9, 20)), 4);
    assert_eq!(BigTextSize::Dot16.cell_height(), 8);
}

#[test]
fn playing_layout_uses_image_rows_only_when_a_protocol_is_available() {
    let mut app = playing_app();
    let text_layout = ui::playing::resolve_playing_layout(&app, Rect::new(0, 0, 80, 40));
    assert_eq!(text_layout.prompt_rows, 9);

    app.set_graphics_detection(detection());
    let image_layout = ui::playing::resolve_playing_layout(&app, Rect::new(0, 0, 80, 40));
    assert_eq!(image_layout.prompt_rows, 5);
}

#[test]
fn graphics_off_ignores_big_font_and_keeps_the_exact_text_buffer() {
    let mut app = playing_app();
    app.config.display.big_font = "biz-ud-gothic".to_owned();
    let mut first = Terminal::new(TestBackend::new(100, 32)).unwrap();
    first.draw(|frame| ui::render(frame, &app)).unwrap();

    app.config.display.big_font = "shinonome".to_owned();
    let mut second = Terminal::new(TestBackend::new(100, 32)).unwrap();
    second.draw(|frame| ui::render(frame, &app)).unwrap();

    assert_eq!(second.backend().buffer(), first.backend().buffer());
}

#[test]
fn missing_or_broken_user_font_falls_back_with_a_reason() {
    let missing = resolve_big_font("/does/not/exist/font.ttf", Path::new("/unused"));
    assert_eq!(missing.kind(), BigFontKind::BizUdGothic);
    assert!(missing.fallback_reason().unwrap().contains("font.ttf"));

    let directory = TempDir::new().unwrap();
    let broken_path = directory.path().join("broken.otf");
    std::fs::write(&broken_path, b"not a font").unwrap();
    let broken = resolve_big_font(broken_path.to_str().unwrap(), Path::new("/unused"));
    assert_eq!(broken.kind(), BigFontKind::BizUdGothic);
    assert!(broken.fallback_reason().unwrap().contains("parse"));
}

#[test]
fn cache_reuses_identical_tiles_and_only_creates_changed_colours() {
    let detected = detection();
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let mut cache = BigTextImageCache::new();
    let yellow = spec('A', Style::default().fg(Color::Yellow));
    cache.prepare(&detected, &font, &yellow).unwrap();
    cache.prepare(&detected, &font, &yellow).unwrap();
    assert_eq!(cache.creation_count(), 1);

    let green = spec('A', Style::default().fg(Color::Green));
    cache.prepare(&detected, &font, &green).unwrap();
    assert_eq!(cache.creation_count(), 2);
    assert_eq!(cache.len(), 2);
}
