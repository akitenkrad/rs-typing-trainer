//! Pixel rendering for the title-screen logo.

use image::RgbaImage;
use ratatui::layout::Size;
use ratatui::style::Color;
use ratatui_image::protocol::Protocol;
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, PixmapPaint, Transform};

use crate::graphics::{CellPixels, GraphicsDetection};
use crate::ui::bigtext::BigTextSize;
use crate::ui::bigtext_image::{FontIdentity, ResolvedBigFont, draw_big_character};
use crate::ui::image_cache::{DEFAULT_TILE_CACHE_LIMIT, ProtocolCache, color_rgb};
use crate::ui::theme::Theme;

#[derive(Clone, Debug)]
pub struct TitleLogoSpec {
    pub cells: Size,
    pub face_color: Color,
    pub pressed_face_color: Color,
    pub text_color: Color,
}

impl TitleLogoSpec {
    pub fn from_theme(cells: Size, theme: Theme) -> Self {
        Self {
            cells,
            face_color: theme.accent,
            pressed_face_color: theme.warning,
            text_color: theme.bg_panel,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TitleLogoKey {
    font: FontIdentity,
    cells: Size,
    face: [u8; 3],
    pressed_face: [u8; 3],
    text: [u8; 3],
}

pub struct TitleImageCache {
    cache: ProtocolCache<TitleLogoKey>,
}

impl Default for TitleImageCache {
    fn default() -> Self {
        Self::new()
    }
}

impl TitleImageCache {
    pub fn new() -> Self {
        Self {
            cache: ProtocolCache::new(DEFAULT_TILE_CACHE_LIMIT),
        }
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.len() == 0
    }

    pub fn creation_count(&self) -> usize {
        self.cache.creation_count()
    }

    pub fn clear(&mut self) {
        self.cache.clear();
    }

    pub fn prepare(&mut self, detection: &GraphicsDetection, font: &ResolvedBigFont, spec: &TitleLogoSpec) -> Result<(), String> {
        let key = title_logo_key(font, spec);
        self.cache
            .prepare(detection, key, spec.cells, |cell_pixels| render_title_logo(font, cell_pixels, spec))
    }

    pub(crate) fn protocol<'a>(&'a mut self, detection: &GraphicsDetection, font: &ResolvedBigFont, spec: &TitleLogoSpec) -> Option<&'a Protocol> {
        self.cache.protocol(detection, &title_logo_key(font, spec))
    }
}

fn title_logo_key(font: &ResolvedBigFont, spec: &TitleLogoSpec) -> TitleLogoKey {
    TitleLogoKey {
        font: font.identity(),
        cells: spec.cells,
        face: color_rgb(spec.face_color, [41, 184, 219]),
        pressed_face: color_rgb(spec.pressed_face_color, [245, 245, 67]),
        text: color_rgb(spec.text_color, [12, 16, 24]),
    }
}

pub fn render_title_logo(font: &ResolvedBigFont, cell_pixels: CellPixels, spec: &TitleLogoSpec) -> Result<RgbaImage, String> {
    let width = u32::from(spec.cells.width) * u32::from(cell_pixels.width);
    let height = u32::from(spec.cells.height) * u32::from(cell_pixels.height);
    let mut pixmap = Pixmap::new(width.max(1), height.max(1)).ok_or_else(|| "logo dimensions are too large".to_owned())?;
    if width < 16 || height < 16 {
        return rgba_image(pixmap);
    }

    let face = color_rgb(spec.face_color, [41, 184, 219]);
    let pressed_face = color_rgb(spec.pressed_face_color, [245, 245, 67]);
    let text = color_rgb(spec.text_color, [12, 16, 24]);
    let pressed_text = contrast_text(pressed_face);
    let outer = (width as f32 * 0.018).clamp(2.0, 8.0);
    let row_gap = (height as f32 * 0.035).clamp(3.0, 7.0);
    let row_height = ((height as f32 - outer * 2.0 - row_gap) / 2.0).max(1.0);

    draw_key_row(
        &mut pixmap,
        font,
        "TYPING",
        outer,
        outer,
        width as f32 - outer,
        outer + row_height,
        face,
        pressed_face,
        text,
        pressed_text,
        2,
    )?;

    let lower_top = outer + row_height + row_gap;
    draw_key_row(
        &mut pixmap,
        font,
        "TRAINER",
        outer,
        lower_top,
        width as f32 - outer,
        lower_top + row_height,
        face,
        pressed_face,
        text,
        pressed_text,
        5,
    )?;

    rgba_image(pixmap)
}

#[allow(clippy::too_many_arguments)]
fn draw_key_row(
    pixmap: &mut Pixmap,
    font: &ResolvedBigFont,
    label: &str,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    face: [u8; 3],
    pressed_face: [u8; 3],
    text: [u8; 3],
    pressed_text: [u8; 3],
    pressed_index: usize,
) -> Result<(), String> {
    let count = label.chars().count().max(1) as f32;
    let gap = ((right - left) * 0.012).clamp(2.0, 6.0);
    let key_width = ((right - left) - gap * (count - 1.0)) / count;
    for (index, character) in label.chars().enumerate() {
        let key_left = left + index as f32 * (key_width + gap);
        let is_pressed = index == pressed_index;
        draw_keycap(
            pixmap,
            font,
            character,
            [key_left, top, key_left + key_width, bottom],
            if is_pressed { pressed_face } else { face },
            if is_pressed { pressed_text } else { text },
            is_pressed,
        )?;
    }
    Ok(())
}

fn draw_keycap(
    pixmap: &mut Pixmap,
    font: &ResolvedBigFont,
    character: char,
    bounds: [f32; 4],
    face: [u8; 3],
    text: [u8; 3],
    pressed: bool,
) -> Result<(), String> {
    let [left, top, right, bottom] = bounds;
    let margin = 1.5;
    let shift = if pressed { 3.0 } else { 0.0 };
    let shadow_height = if pressed { 2.5 } else { 5.0 };
    let face_top = top + margin + shift;
    let face_bottom = (bottom - margin - shadow_height).max(face_top + 1.0);
    let radius = ((right - left).min(bottom - top) * 0.11).clamp(3.0, 8.0);
    fill_round_rect(
        pixmap,
        [left + margin, face_top + 3.0, right - margin, bottom - margin],
        radius,
        darken(face, 48),
    );
    fill_round_rect(pixmap, [left + margin, face_top, right - margin, face_bottom], radius, face);

    let glyph_side = ((right - left - 12.0).min(face_bottom - face_top - 10.0)).max(1.0).round() as u32;
    let mut glyph = Pixmap::new(glyph_side, glyph_side).ok_or_else(|| "logo glyph dimensions are too large".to_owned())?;
    draw_big_character(&mut glyph, font, BigTextSize::Dot16, character, text, true)?;
    let x = ((left + right - glyph_side as f32) / 2.0).round() as i32;
    let y = ((face_top + face_bottom - glyph_side as f32) / 2.0).round() as i32;
    pixmap.draw_pixmap(x, y, glyph.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    Ok(())
}

fn fill_round_rect(pixmap: &mut Pixmap, bounds: [f32; 4], radius: f32, color: [u8; 3]) {
    let [left, top, right, bottom] = bounds;
    if right <= left || bottom <= top {
        return;
    }
    let radius = radius.max(0.0).min((right - left) / 2.0).min((bottom - top) / 2.0);
    let mut path = PathBuilder::new();
    path.move_to(left + radius, top);
    path.line_to(right - radius, top);
    path.quad_to(right, top, right, top + radius);
    path.line_to(right, bottom - radius);
    path.quad_to(right, bottom, right - radius, bottom);
    path.line_to(left + radius, bottom);
    path.quad_to(left, bottom, left, bottom - radius);
    path.line_to(left, top + radius);
    path.quad_to(left, top, left + radius, top);
    path.close();
    if let Some(path) = path.finish() {
        let mut paint = Paint::default();
        paint.set_color_rgba8(color[0], color[1], color[2], 255);
        paint.anti_alias = true;
        pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }
}

fn contrast_text(color: [u8; 3]) -> [u8; 3] {
    let luminance = u32::from(color[0]) * 299 + u32::from(color[1]) * 587 + u32::from(color[2]) * 114;
    if luminance >= 150_000 { [12, 16, 24] } else { [255, 255, 255] }
}

fn darken(color: [u8; 3], percentage: u16) -> [u8; 3] {
    color.map(|component| (u16::from(component) * percentage / 100) as u8)
}

fn rgba_image(pixmap: Pixmap) -> Result<RgbaImage, String> {
    let width = pixmap.width();
    let height = pixmap.height();
    RgbaImage::from_raw(width, height, pixmap.take_demultiplied()).ok_or_else(|| "logo dimensions do not match its data".to_owned())
}
