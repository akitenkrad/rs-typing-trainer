//! Pixel rendering and fixed-protocol caching for keyboard key tiles.

use image::RgbaImage;
use ratatui::layout::Size;
use ratatui::style::Color;
use ratatui_image::protocol::Protocol;
use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Rect, Transform};

use crate::graphics::{CellPixels, GraphicsDetection, GraphicsMode};
use crate::ui::bigtext::{BigTextSize, BitmapFont, font};
use crate::ui::image_cache::{DEFAULT_TILE_CACHE_LIMIT, ProtocolCache, color_rgb};

const DEFAULT_FACE: [u8; 3] = [75, 81, 91];
const BACKSLASH_16: [u16; 16] = [
    0x8000, 0x8000, 0x4000, 0x4000, 0x2000, 0x2000, 0x1000, 0x1000, 0x0800, 0x0800, 0x0400, 0x0400, 0x0200, 0x0200, 0x0100, 0x0100,
];
const TILDE_16: [u16; 16] = [0, 0, 0, 0, 0, 0, 0x6000, 0x9000, 0x0900, 0x0600, 0, 0, 0, 0, 0, 0];
const BACKSLASH_12: [u16; 12] = [
    0x8000, 0x8000, 0x4000, 0x4000, 0x2000, 0x2000, 0x1000, 0x1000, 0x0800, 0x0800, 0x0400, 0x0400,
];
const TILDE_12: [u16; 12] = [0, 0, 0, 0, 0x6000, 0x9000, 0x0800, 0x0400, 0, 0, 0, 0];

#[derive(Clone, Copy)]
struct FaceBounds {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
}

#[derive(Clone, Debug)]
pub struct KeyTileSpec {
    pub cells: Size,
    pub label: String,
    pub face_color: Color,
    pub text_color: Color,
    pub bold: bool,
    pub pressed: bool,
    pub modifier: bool,
    pub finger_color: Option<Color>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TileKey {
    mode: GraphicsMode,
    cell_pixels: CellPixels,
    cells_width: u16,
    cells_height: u16,
    label: String,
    face: [u8; 3],
    text: [u8; 3],
    bold: bool,
    pressed: bool,
    finger: Option<[u8; 3]>,
}

pub struct KeyboardImageCache {
    cache: ProtocolCache<TileKey>,
}

impl Default for KeyboardImageCache {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyboardImageCache {
    pub fn new() -> Self {
        Self::with_limit(DEFAULT_TILE_CACHE_LIMIT)
    }

    pub fn with_limit(limit: usize) -> Self {
        Self {
            cache: ProtocolCache::new(limit),
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

    pub fn prepare(&mut self, detection: &GraphicsDetection, spec: &KeyTileSpec) -> Result<(), String> {
        let key = self.key(detection, spec)?;
        self.cache
            .prepare(detection, key, spec.cells, |cell_pixels| Ok(render_key_tile(cell_pixels, spec)))
    }

    pub(crate) fn protocol<'a>(&'a mut self, detection: &GraphicsDetection, spec: &KeyTileSpec) -> Option<&'a Protocol> {
        let key = self.key(detection, spec).ok()?;
        self.cache.protocol(detection, &key)
    }

    fn key(&mut self, detection: &GraphicsDetection, spec: &KeyTileSpec) -> Result<TileKey, String> {
        let cell_pixels = detection.cell_pixels.ok_or_else(|| "cell pixel size is unavailable".to_owned())?;
        if detection.mode == GraphicsMode::Off {
            return Err("image protocol is disabled".to_owned());
        }
        let mut face = color_rgb(spec.face_color, DEFAULT_FACE);
        if spec.modifier {
            face = darken(face, 85);
        }
        Ok(TileKey {
            mode: detection.mode,
            cell_pixels,
            cells_width: spec.cells.width,
            cells_height: spec.cells.height,
            label: spec.label.clone(),
            face,
            text: color_rgb(spec.text_color, [255, 255, 255]),
            bold: spec.bold,
            pressed: spec.pressed,
            finger: spec.finger_color.map(|color| color_rgb(color, [128, 128, 128])),
        })
    }
}

pub fn render_key_tile(cell_pixels: CellPixels, spec: &KeyTileSpec) -> RgbaImage {
    let width = u32::from(spec.cells.width) * u32::from(cell_pixels.width);
    let height = u32::from(spec.cells.height) * u32::from(cell_pixels.height);
    let mut pixmap = Pixmap::new(width.max(1), height.max(1)).expect("nonzero tile dimensions");
    let min_dimension = width.min(height) as f32;
    let margin = if min_dimension >= 20.0 { 2.0 } else { 1.0 };
    let press_shift = if spec.pressed { 2.0 } else { 0.0 };
    let shadow_height = if spec.pressed { 2.0 } else { 4.0 };
    let left = margin;
    let right = width as f32 - margin;
    let face_top = margin + press_shift;
    let face_bottom = (height as f32 - margin - shadow_height).max(face_top + 1.0);
    let bottom = height as f32 - margin;
    let radius = 5.0_f32.min((right - left) / 4.0).min((face_bottom - face_top) / 3.0);

    let base_face = color_rgb(spec.face_color, DEFAULT_FACE);
    let face = if spec.modifier { darken(base_face, 85) } else { base_face };
    let shadow = darken(face, 48);
    fill_round_rect(&mut pixmap, left, face_top + 3.0, right, bottom, radius, shadow);
    fill_round_rect(&mut pixmap, left, face_top, right, face_bottom, radius, face);

    let highlight = lighten(face, 28);
    fill_rect(
        &mut pixmap,
        left + radius,
        face_top + 1.0,
        (right - left - radius * 2.0).max(1.0),
        1.0,
        highlight,
    );
    if let Some(color) = spec.finger_color {
        fill_rect(
            &mut pixmap,
            left + radius.min(3.0),
            (face_bottom - 3.0).max(face_top),
            (right - left - radius.min(3.0) * 2.0).max(1.0),
            3.0_f32.min(face_bottom - face_top),
            color_rgb(color, [128, 128, 128]),
        );
    }
    draw_label(
        &mut pixmap,
        &spec.label,
        color_rgb(spec.text_color, [255, 255, 255]),
        spec.bold,
        FaceBounds {
            left,
            right,
            top: face_top,
            bottom: face_bottom,
        },
    );

    RgbaImage::from_raw(width.max(1), height.max(1), pixmap.take_demultiplied()).expect("pixmap dimensions match its byte buffer")
}

fn draw_label(pixmap: &mut Pixmap, label: &str, color: [u8; 3], bold: bool, bounds: FaceBounds) {
    if label.is_empty() {
        return;
    }
    let available_width = (bounds.right - bounds.left - 8.0).max(0.0) as u16;
    let available_height = (bounds.bottom - bounds.top).max(0.0) as u16;
    let selection = [BigTextSize::Dot16, BigTextSize::Dot12].into_iter().find_map(|size| {
        let bitmap_font = font(size).ok()?;
        let widths = label
            .chars()
            .map(|character| character_width(bitmap_font, size, character))
            .collect::<Vec<_>>();
        let label_width = widths.iter().copied().sum::<u16>() + u16::from(bold);
        let (_, label_height) = glyph_dimensions(size);
        (label_width <= available_width && u16::from(label_height) <= available_height).then_some((size, bitmap_font, widths, label_width))
    });
    let Some((size, bitmap_font, widths, label_width)) = selection else {
        let x = (bounds.left + bounds.right) / 2.0 - 1.0;
        let y = (bounds.top + bounds.bottom) / 2.0 - 1.0;
        fill_rect(pixmap, x, y, 2.0, 2.0, color);
        return;
    };
    let mut x = ((bounds.left + bounds.right - f32::from(label_width)) / 2.0).round();
    for (character, character_width) in label.chars().zip(widths) {
        if character == '⇧' {
            let (_, glyph_height) = glyph_dimensions(size);
            draw_shift(
                pixmap,
                x,
                ((bounds.top + bounds.bottom - f32::from(glyph_height)) / 2.0).round(),
                size,
                color,
                bold,
            );
        } else if let Some((glyph_width, glyph_height, rows)) = glyph_bitmap(bitmap_font, size, character) {
            let y = ((bounds.top + bounds.bottom - f32::from(glyph_height)) / 2.0).round();
            draw_glyph(pixmap, x, y, glyph_width, rows, color, bold);
        }
        x += f32::from(character_width);
    }
}

fn glyph_dimensions(size: BigTextSize) -> (u8, u8) {
    match size {
        BigTextSize::Dot12 => (9, 12),
        BigTextSize::Dot16 => (12, 16),
    }
}

fn character_width(bitmap_font: &BitmapFont, size: BigTextSize, character: char) -> u16 {
    if character == '⇧' {
        u16::from(glyph_dimensions(size).0)
    } else {
        glyph_bitmap(bitmap_font, size, character).map_or(0, |glyph| u16::from(glyph.0))
    }
}

fn glyph_bitmap(bitmap_font: &BitmapFont, size: BigTextSize, character: char) -> Option<(u8, u8, &[u16])> {
    match (size, character) {
        (BigTextSize::Dot16, '\\') => Some((8, 16, &BACKSLASH_16)),
        (BigTextSize::Dot16, '~') => Some((8, 16, &TILDE_16)),
        (BigTextSize::Dot12, '\\') => Some((6, 12, &BACKSLASH_12)),
        (BigTextSize::Dot12, '~') => Some((6, 12, &TILDE_12)),
        (_, '¥') => bitmap_font.glyph_bitmap('\\'),
        _ => bitmap_font.glyph_bitmap(character),
    }
}

fn draw_glyph(pixmap: &mut Pixmap, x: f32, y: f32, width: u8, rows: &[u16], color: [u8; 3], bold: bool) {
    for (row, bits) in rows.iter().enumerate() {
        for column in 0..width {
            if bits & (1 << (15 - column)) != 0 {
                fill_rect(pixmap, x + f32::from(column), y + row as f32, 1.0, 1.0, color);
                if bold {
                    fill_rect(pixmap, x + f32::from(column) + 1.0, y + row as f32, 1.0, 1.0, color);
                }
            }
        }
    }
}

fn draw_shift(pixmap: &mut Pixmap, x: f32, y: f32, size: BigTextSize, color: [u8; 3], bold: bool) {
    draw_shift_at(pixmap, x, y, size, color);
    if bold {
        draw_shift_at(pixmap, x + 1.0, y, size, color);
    }
}

fn draw_shift_at(pixmap: &mut Pixmap, x: f32, y: f32, size: BigTextSize, color: [u8; 3]) {
    let scale = match size {
        BigTextSize::Dot12 => 0.75,
        BigTextSize::Dot16 => 1.0,
    };
    let mut path = PathBuilder::new();
    path.move_to(x + 6.0 * scale, y);
    path.line_to(x + 12.0 * scale, y + 7.0 * scale);
    path.line_to(x + 9.0 * scale, y + 7.0 * scale);
    path.line_to(x + 9.0 * scale, y + 15.0 * scale);
    path.line_to(x + 3.0 * scale, y + 15.0 * scale);
    path.line_to(x + 3.0 * scale, y + 7.0 * scale);
    path.line_to(x, y + 7.0 * scale);
    path.close();
    if let Some(path) = path.finish() {
        fill_path(pixmap, &path, color);
    }
}

fn fill_round_rect(pixmap: &mut Pixmap, left: f32, top: f32, right: f32, bottom: f32, radius: f32, color: [u8; 3]) {
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
        fill_path(pixmap, &path, color);
    }
}

fn fill_path(pixmap: &mut Pixmap, path: &Path, color: [u8; 3]) {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], 255);
    paint.anti_alias = true;
    pixmap.fill_path(path, &paint, FillRule::Winding, Transform::identity(), None);
}

fn fill_rect(pixmap: &mut Pixmap, x: f32, y: f32, width: f32, height: f32, color: [u8; 3]) {
    let Some(rect) = Rect::from_xywh(x, y, width, height) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], 255);
    paint.anti_alias = false;
    pixmap.fill_rect(rect, &paint, Transform::identity(), None);
}

fn darken(color: [u8; 3], percentage: u16) -> [u8; 3] {
    color.map(|channel| (u16::from(channel) * percentage / 100) as u8)
}

fn lighten(color: [u8; 3], amount: u8) -> [u8; 3] {
    color.map(|channel| channel.saturating_add(amount))
}
