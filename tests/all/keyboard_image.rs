use std::cell::RefCell;
use std::collections::BTreeSet;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::{Rect, Size};
use ratatui::style::Color;
use rs_typing_trainer::graphics::{CellPixels, GraphicsDetection, GraphicsMode, GraphicsReason};
use rs_typing_trainer::layout::builtin_layout;
use rs_typing_trainer::layout::geometry::geometry;
use rs_typing_trainer::practice::adaptive::FocusTransitionPhase;
use rs_typing_trainer::ui::keyboard::{FocusTransitionKeys, KeyboardImageContext, KeyboardState, KeyboardView, keycap_tile_placements};
use rs_typing_trainer::ui::keyboard_image::{
    FocusKeyMarker, KeyTileSpec, KeyboardImageCache, TileArrowOverlay, encode_focus_marker, render_key_tile,
};

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
        focus_arrow: None,
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
fn focus_markers_draw_distinct_corner_badges_without_hiding_the_label() {
    let cells = CellPixels::new(9, 20);
    let plain = render_key_tile(cells, &tile("g"));
    let plain_label = text_pixels(&plain)
        .into_iter()
        .filter(|(x, y)| (14..=31).contains(x) && (8..=29).contains(y))
        .collect::<BTreeSet<_>>();

    let mut from_spec = tile("g");
    from_spec.label = encode_focus_marker(from_spec.label, FocusKeyMarker::From);
    let from = render_key_tile(cells, &from_spec);
    let mut to_spec = tile("g");
    to_spec.label = encode_focus_marker(to_spec.label, FocusKeyMarker::To);
    let to = render_key_tile(cells, &to_spec);

    assert_eq!(
        text_pixels(&from)
            .into_iter()
            .filter(|(x, y)| (14..=31).contains(x) && (8..=29).contains(y))
            .collect::<BTreeSet<_>>(),
        plain_label,
        "the focus badge must leave the centered key label intact"
    );
    assert_ne!(from, plain, "the from badge must alter tile pixels");
    assert_ne!(to, plain, "the to badge must alter tile pixels");
    assert_ne!(from, to, "the from/to badges must remain distinguishable without relying on face color");
    assert!(
        (30..43).any(|x| (3..17).any(|y| pixel(&from, x, y)[3] > 0 && pixel(&from, x, y) != pixel(&plain, x, y))),
        "the marker should be a compact badge near the upper-right corner"
    );
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
fn cache_recreates_a_tile_when_its_focus_marker_changes() {
    let detected = detection(GraphicsMode::Kitty, CellPixels::new(9, 20));
    let mut cache = KeyboardImageCache::new();
    let plain = tile("g");
    cache.prepare(&detected, &plain).unwrap();

    let mut from = plain.clone();
    from.label = encode_focus_marker(from.label, FocusKeyMarker::From);
    cache.prepare(&detected, &from).unwrap();
    cache.prepare(&detected, &from).unwrap();

    let mut to = from.clone();
    to.label = encode_focus_marker("g".to_owned(), FocusKeyMarker::To);
    cache.prepare(&detected, &to).unwrap();
    assert_eq!(cache.creation_count(), 3);
    assert_eq!(cache.len(), 3);
}

#[test]
fn focus_arrow_is_baked_into_intersected_tiles_only() {
    let physical = geometry("ansi").unwrap();
    let layout = builtin_layout("qwerty").unwrap();
    let state = KeyboardState {
        focus: Some(FocusTransitionKeys {
            from_code: "KeyA".to_owned(),
            to_code: "KeyL".to_owned(),
            phase: FocusTransitionPhase::BeforeFrom,
        }),
        ..KeyboardState::default()
    };
    let view = KeyboardView {
        show: "full",
        style: "keycap",
        finger_guide: true,
    };
    let placements = keycap_tile_placements(&physical, &layout, &state, view, false);
    let overlay_codes = ["KeyA", "KeyS", "KeyD", "KeyF", "KeyG", "KeyH", "KeyJ", "KeyK", "KeyL"];

    for code in overlay_codes {
        let label = layout.output(code, false).unwrap().to_string();
        let placement = placements.iter().find(|placement| placement.spec.label.starts_with(&label)).unwrap();
        assert!(placement.spec.focus_arrow.is_some(), "{code} should contain an arrow segment");
    }
    let q = placements.iter().find(|placement| placement.spec.label == "q").unwrap();
    assert!(q.spec.focus_arrow.is_none(), "a key outside the line must remain unchanged");
    assert_eq!(
        placements
            .iter()
            .filter(|placement| placement.spec.focus_arrow.as_ref().is_some_and(|overlay| overlay.draw_head))
            .count(),
        1,
        "only the destination tile should draw the arrowhead"
    );
}

#[test]
fn arrow_tile_draws_line_below_the_unchanged_label() {
    let cells = CellPixels::new(9, 20);
    let plain_spec = tile("g");
    let plain = render_key_tile(cells, &plain_spec);
    let mut arrow_spec = plain_spec.clone();
    arrow_spec.focus_arrow = Some(TileArrowOverlay::from_cell_coordinates(
        (-2.0, 1.0),
        (7.0, 1.0),
        Color::LightCyan,
        FocusTransitionPhase::BeforeFrom,
        false,
    ));
    let arrow = render_key_tile(cells, &arrow_spec);

    assert_ne!(arrow, plain, "the baked-in shaft must alter tile pixels");
    assert_eq!(
        text_pixels(&arrow),
        text_pixels(&plain),
        "the label must be redrawn unchanged above the shaft"
    );
    assert!(
        (0..arrow.width()).any(|x| pixel(&arrow, x, 20) != pixel(&plain, x, 20)),
        "the clipped shaft must remain visible inside the tile"
    );
}

#[test]
fn focus_arrow_changes_only_affected_tile_cache_entries() {
    let detected = detection(GraphicsMode::Kitty, CellPixels::new(9, 20));
    let mut cache = KeyboardImageCache::new();
    let base = tile("g");
    cache.prepare(&detected, &base).unwrap();
    cache.prepare(&detected, &base).unwrap();
    assert_eq!(cache.creation_count(), 1);

    let mut arrow = base.clone();
    arrow.focus_arrow = Some(TileArrowOverlay::from_cell_coordinates(
        (-2.0, 1.0),
        (7.0, 1.0),
        Color::LightCyan,
        FocusTransitionPhase::BeforeFrom,
        false,
    ));
    cache.prepare(&detected, &arrow).unwrap();
    cache.prepare(&detected, &arrow).unwrap();
    cache.prepare(&detected, &base).unwrap();
    assert_eq!(cache.creation_count(), 2);

    let mut moved = arrow.clone();
    moved.focus_arrow.as_mut().unwrap().to.0 += 1;
    cache.prepare(&detected, &moved).unwrap();
    let mut recolored = moved.clone();
    recolored.focus_arrow.as_mut().unwrap().color = Color::LightYellow;
    cache.prepare(&detected, &recolored).unwrap();
    let mut headed = recolored.clone();
    headed.focus_arrow.as_mut().unwrap().draw_head = true;
    cache.prepare(&detected, &headed).unwrap();

    assert_eq!(cache.creation_count(), 5);
    assert_eq!(cache.len(), 5);
}

#[test]
fn keycap_placements_without_focus_have_no_arrow_overlay() {
    let physical = geometry("ansi").unwrap();
    let layout = builtin_layout("qwerty").unwrap();
    let placements = keycap_tile_placements(
        &physical,
        &layout,
        &KeyboardState::default(),
        KeyboardView {
            show: "full",
            style: "keycap",
            finger_guide: true,
        },
        false,
    );

    assert!(placements.iter().all(|placement| placement.spec.focus_arrow.is_none()));
}

#[test]
fn focused_keycap_image_placements_never_overlap() {
    let physical = geometry("ansi").unwrap();
    let layout = builtin_layout("qwerty").unwrap();
    let state = KeyboardState {
        focus: Some(FocusTransitionKeys {
            from_code: "KeyA".to_owned(),
            to_code: "KeyL".to_owned(),
            phase: FocusTransitionPhase::AfterFrom,
        }),
        ..KeyboardState::default()
    };
    let placements = keycap_tile_placements(
        &physical,
        &layout,
        &state,
        KeyboardView {
            show: "full",
            style: "keycap",
            finger_guide: true,
        },
        false,
    );

    for (index, left) in placements.iter().enumerate() {
        let left_rect = Rect::new(left.x, left.y, left.spec.cells.width, left.spec.cells.height);
        for right in &placements[index + 1..] {
            let right_rect = Rect::new(right.x, right.y, right.spec.cells.width, right.spec.cells.height);
            assert!(
                !left_rect.intersects(right_rect),
                "tile placements overlap: {left_rect:?} and {right_rect:?}"
            );
        }
    }
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
