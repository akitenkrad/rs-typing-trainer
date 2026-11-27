//! Pixel rendering and fixed-protocol caching for keyboard key tiles.

use image::RgbaImage;
use ratatui::layout::Size;
use ratatui::style::Color;
use ratatui_image::protocol::Protocol;
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Rect, Stroke, Transform};

use crate::graphics::{CellPixels, GraphicsDetection, GraphicsMode};
use crate::practice::adaptive::FocusTransitionPhase;
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
const FOCUS_FROM_SUFFIX: &str = "\u{e000}1";
const FOCUS_TO_SUFFIX: &str = "\u{e000}2";
const ARROW_COORD_SCALE: f64 = 256.0;

#[derive(Clone, Copy)]
struct FaceBounds {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FocusKeyMarker {
    From,
    To,
}

pub fn encode_focus_marker(label: String, marker: FocusKeyMarker) -> String {
    let (label, _) = split_focus_label(&label);
    let suffix = match marker {
        FocusKeyMarker::From => FOCUS_FROM_SUFFIX,
        FocusKeyMarker::To => FOCUS_TO_SUFFIX,
    };
    format!("{label}{suffix}")
}

#[derive(Clone, Debug, PartialEq)]
pub struct KeyTileSpec {
    pub cells: Size,
    pub label: String,
    pub face_color: Color,
    pub text_color: Color,
    pub bold: bool,
    pub pressed: bool,
    pub modifier: bool,
    pub finger_color: Option<Color>,
    pub focus_arrow: Option<TileArrowOverlay>,
}

/// A clipped piece of the focus arrow, in fixed-point cell coordinates relative
/// to the tile's top-left corner.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TileArrowOverlay {
    pub from: (i32, i32),
    pub to: (i32, i32),
    pub color: Color,
    pub alpha: u8,
    pub draw_head: bool,
}

impl TileArrowOverlay {
    pub fn from_cell_coordinates(from: (f64, f64), to: (f64, f64), color: Color, phase: FocusTransitionPhase, draw_head: bool) -> Self {
        Self {
            from: (fixed_cell_coordinate(from.0), fixed_cell_coordinate(from.1)),
            to: (fixed_cell_coordinate(to.0), fixed_cell_coordinate(to.1)),
            color,
            alpha: match phase {
                FocusTransitionPhase::BeforeFrom => 210,
                FocusTransitionPhase::AfterFrom => 245,
            },
            draw_head,
        }
    }
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
    focus_marker: Option<FocusKeyMarker>,
    focus_arrow: Option<TileArrowKey>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TileArrowKey {
    from: (i32, i32),
    to: (i32, i32),
    color: [u8; 3],
    alpha: u8,
    draw_head: bool,
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
        let key = self.tile_key(detection, spec)?;
        self.cache
            .prepare(detection, key, spec.cells, |cell_pixels| Ok(render_key_tile(cell_pixels, spec)))
    }

    pub(crate) fn protocol<'a>(&'a mut self, detection: &GraphicsDetection, spec: &KeyTileSpec) -> Option<&'a Protocol> {
        let key = self.tile_key(detection, spec).ok()?;
        self.cache.protocol(detection, &key)
    }

    fn tile_key(&self, detection: &GraphicsDetection, spec: &KeyTileSpec) -> Result<TileKey, String> {
        let cell_pixels = detection.cell_pixels.ok_or_else(|| "cell pixel size is unavailable".to_owned())?;
        if detection.mode == GraphicsMode::Off {
            return Err("image protocol is disabled".to_owned());
        }
        let mut face = color_rgb(spec.face_color, DEFAULT_FACE);
        if spec.modifier {
            face = darken(face, 85);
        }
        let (label, focus_marker) = split_focus_label(&spec.label);
        Ok(TileKey {
            mode: detection.mode,
            cell_pixels,
            cells_width: spec.cells.width,
            cells_height: spec.cells.height,
            label: label.to_owned(),
            face,
            text: color_rgb(spec.text_color, [255, 255, 255]),
            bold: spec.bold,
            pressed: spec.pressed,
            finger: spec.finger_color.map(|color| color_rgb(color, [128, 128, 128])),
            focus_marker,
            focus_arrow: spec.focus_arrow.as_ref().map(|arrow| TileArrowKey {
                from: arrow.from,
                to: arrow.to,
                color: color_rgb(arrow.color, [255, 255, 255]),
                alpha: arrow.alpha,
                draw_head: arrow.draw_head,
            }),
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
    if let Some(arrow) = spec.focus_arrow.as_ref() {
        draw_tile_arrow(&mut pixmap, cell_pixels, arrow);
    }
    let (label, focus_marker) = split_focus_label(&spec.label);
    draw_label(
        &mut pixmap,
        label,
        color_rgb(spec.text_color, [255, 255, 255]),
        spec.bold,
        FaceBounds {
            left,
            right,
            top: face_top,
            bottom: face_bottom,
        },
    );
    if let Some(marker) = focus_marker {
        draw_focus_marker(
            &mut pixmap,
            marker,
            FaceBounds {
                left,
                right,
                top: face_top,
                bottom: face_bottom,
            },
        );
    }

    RgbaImage::from_raw(width.max(1), height.max(1), pixmap.take_demultiplied()).expect("pixmap dimensions match its byte buffer")
}

fn split_focus_label(label: &str) -> (&str, Option<FocusKeyMarker>) {
    if let Some(label) = label.strip_suffix(FOCUS_FROM_SUFFIX) {
        (label, Some(FocusKeyMarker::From))
    } else if let Some(label) = label.strip_suffix(FOCUS_TO_SUFFIX) {
        (label, Some(FocusKeyMarker::To))
    } else {
        (label, None)
    }
}

fn draw_tile_arrow(pixmap: &mut Pixmap, cell_pixels: CellPixels, spec: &TileArrowOverlay) {
    let from = (
        spec.from.0 as f32 / ARROW_COORD_SCALE as f32 * f32::from(cell_pixels.width),
        spec.from.1 as f32 / ARROW_COORD_SCALE as f32 * f32::from(cell_pixels.height),
    );
    let to = (
        spec.to.0 as f32 / ARROW_COORD_SCALE as f32 * f32::from(cell_pixels.width),
        spec.to.1 as f32 / ARROW_COORD_SCALE as f32 * f32::from(cell_pixels.height),
    );
    let dx = to.0 - from.0;
    let dy = to.1 - from.1;
    let length = dx.hypot(dy);
    if length <= f32::EPSILON {
        return;
    }
    let unit = (dx / length, dy / length);
    let perpendicular = (-unit.1, unit.0);
    let cell_width = f32::from(cell_pixels.width);
    let head_length = (cell_width * 1.1).max(7.0).min(length * 0.4);
    let head_width = (cell_width * 0.7).max(5.0).min(length * 0.3);
    // Stop short of the destination key's centre so the arrowhead sits on
    // the key face beside the label instead of on top of it.
    let pull_back = (cell_width * 1.3).min(length * 0.3);
    let tip = (to.0 - unit.0 * pull_back, to.1 - unit.1 * pull_back);
    let head_base = (tip.0 - unit.0 * head_length, tip.1 - unit.1 * head_length);
    let color = color_rgb(spec.color, [255, 255, 255]);

    let mut shaft = PathBuilder::new();
    shaft.move_to(from.0, from.1);
    shaft.line_to(
        if spec.draw_head { head_base.0 } else { to.0 },
        if spec.draw_head { head_base.1 } else { to.1 },
    );
    if let Some(path) = shaft.finish() {
        stroke_path(pixmap, &path, color, spec.alpha, (cell_width * 0.36).max(3.0));
    }
    if spec.draw_head {
        let mut head = PathBuilder::new();
        head.move_to(tip.0, tip.1);
        head.line_to(head_base.0 + perpendicular.0 * head_width, head_base.1 + perpendicular.1 * head_width);
        head.line_to(head_base.0 - perpendicular.0 * head_width, head_base.1 - perpendicular.1 * head_width);
        head.close();
        if let Some(path) = head.finish() {
            fill_path_alpha(pixmap, &path, color, spec.alpha);
        }
    }
}

fn fixed_cell_coordinate(value: f64) -> i32 {
    (value * ARROW_COORD_SCALE).round().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

fn draw_focus_marker(pixmap: &mut Pixmap, marker: FocusKeyMarker, bounds: FaceBounds) {
    let available_height = bounds.bottom - bounds.top;
    let available_width = bounds.right - bounds.left;
    if available_width < 16.0 || available_height < 14.0 {
        return;
    }
    let radius = (available_height * 0.19).clamp(5.0, 6.5);
    let center = (bounds.right - radius - 2.0, bounds.top + radius + 2.0);
    let badge_color = match marker {
        FocusKeyMarker::From => [104, 224, 205],
        FocusKeyMarker::To => [255, 194, 93],
    };
    if let Some(circle) = PathBuilder::from_circle(center.0, center.1, radius) {
        fill_path(pixmap, &circle, badge_color);
    }
    let ink = [24, 27, 34];
    let x = center.0.round() - 2.0;
    let y = center.1.round() - 3.0;
    match marker {
        FocusKeyMarker::From => {
            fill_rect(pixmap, x + 2.0, y, 1.5, 7.0, ink);
            fill_rect(pixmap, x + 1.0, y + 1.0, 1.5, 1.5, ink);
            fill_rect(pixmap, x + 1.0, y + 6.0, 4.0, 1.0, ink);
        }
        FocusKeyMarker::To => {
            fill_rect(pixmap, x + 1.0, y, 3.0, 1.5, ink);
            fill_rect(pixmap, x + 3.5, y + 1.0, 1.5, 2.0, ink);
            fill_rect(pixmap, x + 1.0, y + 3.0, 3.5, 1.5, ink);
            fill_rect(pixmap, x, y + 4.0, 1.5, 2.0, ink);
            fill_rect(pixmap, x, y + 6.0, 5.0, 1.0, ink);
        }
    }
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
    fill_path_alpha(pixmap, path, color, 255);
}

fn fill_path_alpha(pixmap: &mut Pixmap, path: &Path, color: [u8; 3], alpha: u8) {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], alpha);
    paint.anti_alias = true;
    pixmap.fill_path(path, &paint, FillRule::Winding, Transform::identity(), None);
}

fn stroke_path(pixmap: &mut Pixmap, path: &Path, color: [u8; 3], alpha: u8, width: f32) {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], alpha);
    paint.anti_alias = true;
    let stroke = Stroke {
        width,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    pixmap.stroke_path(path, &paint, &stroke, Transform::identity(), None);
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
