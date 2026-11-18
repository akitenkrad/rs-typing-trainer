use std::cell::RefCell;
use std::collections::BTreeSet;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::{Rect, Size};
use ratatui::style::Color;
use rs_typing_trainer::graphics::{CellPixels, GraphicsDetection, GraphicsMode, GraphicsReason};
use rs_typing_trainer::layout::builtin_layout;
use rs_typing_trainer::layout::geometry::geometry;
use rs_typing_trainer::ui::keyboard::{KeyboardImageContext, KeyboardState, KeyboardView};
use rs_typing_trainer::ui::keyboard_image::{KeyTileSpec, KeyboardImageCache, render_key_tile};

fn detection(mode: GraphicsMode, cell_pixels: CellPixels) -> GraphicsDetection {
    GraphicsDetection {
        mode,
        cell_pixels: Some(cell_pixels),
        reason: GraphicsReason::Explicit(mode),
    }
}

fn tile(label: &str) -> KeyTileSpec {
    KeyTileSpec {
        cells: Size::new(5, 2),
        label: label.to_owned(),
        face_color: Color::Rgb(75, 81, 91),
        text_color: Color::White,
        bold: false,
        pressed: false,
        modifier: false,
        finger_color: None,
    }
}

fn pixel(image: &image::RgbaImage, x: u32, y: u32) -> [u8; 4] {
    image.get_pixel(x, y).0
}

fn brightness(value: [u8; 4]) -> u16 {
    u16::from(value[0]) + u16::from(value[1]) + u16::from(value[2])
}

fn text_pixels(image: &image::RgbaImage) -> BTreeSet<(u32, u32)> {
    image
        .enumerate_pixels()
        .filter_map(|(x, y, value)| (value.0 == [255, 255, 255, 255]).then_some((x, y)))
        .collect()
}

fn text_width(image: &image::RgbaImage) -> u32 {
    let pixels = text_pixels(image);
    let left = pixels.iter().map(|(x, _)| *x).min().unwrap();
    let right = pixels.iter().map(|(x, _)| *x).max().unwrap();
    right - left + 1
}

#[test]
fn tile_pixels_show_rounded_face_shadow_press_finger_label_and_shift() {
    let cells = CellPixels::new(9, 20);
    let normal = render_key_tile(cells, &tile("a"));
    assert_eq!(normal.dimensions(), (45, 40));
    for corner in [(0, 0), (44, 0), (0, 39), (44, 39)] {
        assert_eq!(pixel(&normal, corner.0, corner.1)[3], 0, "corner {corner:?}");
    }
    let face = pixel(&normal, 5, 10);
    assert_eq!(&face[..3], &[75, 81, 91]);
    let shadow = pixel(&normal, 22, 37);
    assert!(shadow[3] > 0);
    assert!(brightness(shadow) < brightness(face));
    assert!(normal.pixels().any(|value| value.0 == [255, 255, 255, 255]));

    let mut pressed_spec = tile("a");
    pressed_spec.pressed = true;
    let pressed = render_key_tile(cells, &pressed_spec);
    assert!(pixel(&normal, 22, 2)[3] > 0);
    assert_eq!(pixel(&pressed, 22, 2)[3], 0);
    assert!(pixel(&pressed, 22, 4)[3] > 0);

    let mut guided_spec = tile("a");
    guided_spec.finger_color = Some(Color::LightGreen);
    let guided = render_key_tile(cells, &guided_spec);
    assert_eq!(&pixel(&guided, 5, 33)[..3], &[144, 238, 144]);

    let shift = render_key_tile(cells, &tile("⇧"));
    assert!(shift.pixels().any(|value| value.0 == [255, 255, 255, 255]));
}

#[test]
fn ascii_backslash_and_tilde_use_hand_drawn_glyphs() {
    let cells = CellPixels::new(9, 20);
    let backslash = render_key_tile(cells, &tile("\\"));
    let yen = render_key_tile(cells, &tile("¥"));
    let tilde = render_key_tile(cells, &tile("~"));

    assert_ne!(text_pixels(&backslash), text_pixels(&yen));
    for coordinate in [(19, 10), (22, 17), (26, 25)] {
        assert_eq!(pixel(&backslash, coordinate.0, coordinate.1), [255, 255, 255, 255]);
    }

    for coordinate in [(20, 16), (19, 17), (23, 18), (25, 19)] {
        assert_eq!(pixel(&tilde, coordinate.0, coordinate.1), [255, 255, 255, 255]);
    }
    let tilde_rows = text_pixels(&tilde).into_iter().map(|(_, y)| y).collect::<BTreeSet<_>>();
    assert_eq!(tilde_rows, BTreeSet::from([16, 17, 18, 19]));
}

#[test]
fn ctrl_label_falls_back_to_dot12_before_the_overflow_dot() {
    let mut ctrl = tile("CTRL");
    ctrl.cells = Size::new(6, 2);

    let dot12 = render_key_tile(CellPixels::new(7, 16), &ctrl);
    let dot16 = render_key_tile(CellPixels::new(9, 20), &ctrl);
    let dot12_columns = text_pixels(&dot12).into_iter().map(|(x, _)| x).collect::<BTreeSet<_>>();

    assert!(dot12_columns.len() > 8, "CTRL must not be replaced by a two-pixel dot");
    assert!(
        text_width(&dot16) > text_width(&dot12),
        "9x20 cells must continue to use the wider dot16 glyphs"
    );
}

#[test]
fn cache_reuses_tiles_and_only_creates_the_changed_key() {
    let detected = detection(GraphicsMode::Kitty, CellPixels::new(9, 20));
    let mut cache = KeyboardImageCache::new();
    let a = tile("a");
    let s = tile("s");

    cache.prepare(&detected, &a).unwrap();
    cache.prepare(&detected, &s).unwrap();
    assert_eq!(cache.creation_count(), 2);
    cache.prepare(&detected, &a).unwrap();
    cache.prepare(&detected, &s).unwrap();
    assert_eq!(cache.creation_count(), 2);

    let mut pressed_a = a.clone();
    pressed_a.pressed = true;
    cache.prepare(&detected, &pressed_a).unwrap();
    cache.prepare(&detected, &s).unwrap();
    assert_eq!(cache.creation_count(), 3);
    assert_eq!(cache.len(), 3);
}

#[test]
fn cache_discards_tiles_when_cell_pixels_or_protocol_change() {
    let mut cache = KeyboardImageCache::new();
    let spec = tile("a");
    cache.prepare(&detection(GraphicsMode::Kitty, CellPixels::new(9, 20)), &spec).unwrap();
    cache.prepare(&detection(GraphicsMode::Kitty, CellPixels::new(10, 20)), &spec).unwrap();
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.creation_count(), 2);

    cache.prepare(&detection(GraphicsMode::Iterm2, CellPixels::new(10, 20)), &spec).unwrap();
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.creation_count(), 3);
}

#[test]
fn cache_clears_all_entries_when_its_limit_would_be_exceeded() {
    let detected = detection(GraphicsMode::Kitty, CellPixels::new(9, 20));
    let mut cache = KeyboardImageCache::with_limit(2);
    cache.prepare(&detected, &tile("a")).unwrap();
    cache.prepare(&detected, &tile("s")).unwrap();
    assert_eq!(cache.len(), 2);
    cache.prepare(&detected, &tile("d")).unwrap();
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.creation_count(), 3);
}

#[test]
fn graphics_off_uses_the_exact_existing_text_buffer() {
    let physical = geometry("ansi").unwrap();
    let layout = builtin_layout("qwerty").unwrap();
    let state = KeyboardState::default();
    let view = KeyboardView {
        show: "full",
        style: "keycap",
        finger_guide: true,
    };
    let mut expected = Terminal::new(TestBackend::new(120, 14)).unwrap();
    expected
        .draw(|frame| rs_typing_trainer::ui::keyboard::render(frame, frame.area(), &physical, &layout, &state, view))
        .unwrap();

    let graphics = GraphicsDetection::default();
    let cache = RefCell::new(KeyboardImageCache::new());
    let mut actual = Terminal::new(TestBackend::new(120, 14)).unwrap();
    actual
        .draw(|frame| {
            rs_typing_trainer::ui::keyboard::render_with_images(
                frame,
                Rect::new(0, 0, 120, 14),
                &physical,
                &layout,
                &state,
                view,
                KeyboardImageContext {
                    detection: &graphics,
                    cache: &cache,
                },
            )
        })
        .unwrap();

    assert_eq!(actual.backend().buffer(), expected.backend().buffer());
    assert_eq!(cache.borrow().creation_count(), 0);
}
