use ratatui::layout::Size;
use rs_typing_trainer::graphics::CellPixels;
use rs_typing_trainer::graphics::{GraphicsDetection, GraphicsMode, GraphicsReason};
use rs_typing_trainer::ui::bigtext_image::resolve_big_font;
use rs_typing_trainer::ui::theme::Theme;
use rs_typing_trainer::ui::title_image::{TitleImageCache, TitleLogoSpec, render_title_logo};
use std::path::Path;

#[test]
fn title_logo_has_requested_size_accent_key_faces_and_transparent_corners() {
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let spec = TitleLogoSpec::from_theme(Size::new(48, 7), Theme::default());
    let image = render_title_logo(&font, CellPixels::new(9, 20), &spec).unwrap();

    assert_eq!(image.dimensions(), (432, 140));
    assert!(image.pixels().any(|pixel| pixel.0 == [41, 184, 219, 255]));
    for (x, y) in [(0, 0), (431, 0), (0, 139), (431, 139)] {
        assert_eq!(image.get_pixel(x, y).0[3], 0, "corner ({x}, {y}) must stay transparent");
    }
}

#[test]
fn title_logo_key_face_changes_with_theme() {
    let font = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let cells = Size::new(48, 7);
    let default_image = render_title_logo(&font, CellPixels::new(9, 20), &TitleLogoSpec::from_theme(cells, Theme::default())).unwrap();
    let mono_image = render_title_logo(&font, CellPixels::new(9, 20), &TitleLogoSpec::from_theme(cells, Theme::mono())).unwrap();

    assert!(default_image.pixels().any(|pixel| pixel.0 == [41, 184, 219, 255]));
    assert!(mono_image.pixels().any(|pixel| pixel.0 == [255, 255, 255, 255]));
    assert_ne!(default_image, mono_image);
}

#[test]
fn title_logo_cache_key_includes_size_theme_and_font() {
    let graphics = GraphicsDetection {
        mode: GraphicsMode::Kitty,
        cell_pixels: Some(CellPixels::new(9, 20)),
        reason: GraphicsReason::Explicit(GraphicsMode::Kitty),
    };
    let biz = resolve_big_font("biz-ud-gothic", Path::new("/unused"));
    let shinonome = resolve_big_font("shinonome", Path::new("/unused"));
    let default = TitleLogoSpec::from_theme(Size::new(48, 7), Theme::default());
    let resized = TitleLogoSpec::from_theme(Size::new(52, 7), Theme::default());
    let mono = TitleLogoSpec::from_theme(Size::new(52, 7), Theme::mono());
    let mut cache = TitleImageCache::new();

    cache.prepare(&graphics, &biz, &default).unwrap();
    cache.prepare(&graphics, &biz, &default).unwrap();
    assert_eq!(cache.creation_count(), 1);
    cache.prepare(&graphics, &biz, &resized).unwrap();
    cache.prepare(&graphics, &biz, &mono).unwrap();
    cache.prepare(&graphics, &shinonome, &mono).unwrap();
    assert_eq!(cache.creation_count(), 4);
}
