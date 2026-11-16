//! Pixel rendering and fixed-protocol caching for large prompt characters.

use std::collections::BTreeMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::UNIX_EPOCH;

use flate2::read::GzDecoder;
use image::RgbaImage;
use ratatui::layout::Size;
use ratatui::style::{Color, Modifier};
use ratatui_image::protocol::Protocol;
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};

use crate::graphics::{CellPixels, GraphicsDetection};
use crate::ui::bigtext::{BigTextSize, StyledCharacter, font as bitmap_font};
use crate::ui::image_cache::{DEFAULT_TILE_CACHE_LIMIT, ProtocolCache, color_rgb};

const OUTLINE_MAGIC: &[u8; 8] = b"BIZOUTL\0";
const OUTLINE_VERSION: u16 = 1;
const BUNDLED_OUTLINES: &[u8] = include_bytes!("../../data/font/bizud-gothic.outl.gz");

static BUNDLED_FONT: OnceLock<Result<OutlineFont, String>> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BigFontKind {
    BizUdGothic,
    Shinonome,
    User,
}

#[derive(Clone, Debug)]
enum FontSource {
    BizUdGothic,
    Shinonome,
    User { path: PathBuf, bytes: Arc<[u8]>, modified_nanos: u128 },
}

#[derive(Clone, Debug)]
pub struct ResolvedBigFont {
    source: FontSource,
    fallback_reason: Option<String>,
}

impl ResolvedBigFont {
    pub fn kind(&self) -> BigFontKind {
        match self.source {
            FontSource::BizUdGothic => BigFontKind::BizUdGothic,
            FontSource::Shinonome => BigFontKind::Shinonome,
            FontSource::User { .. } => BigFontKind::User,
        }
    }

    pub fn fallback_reason(&self) -> Option<&str> {
        self.fallback_reason.as_deref()
    }

    pub(crate) fn identity(&self) -> FontIdentity {
        match &self.source {
            FontSource::BizUdGothic => FontIdentity::BizUdGothic,
            FontSource::Shinonome => FontIdentity::Shinonome,
            FontSource::User { path, modified_nanos, .. } => FontIdentity::User(path.clone(), *modified_nanos),
        }
    }
}

#[derive(Clone, Debug)]
pub struct OutlineFont {
    units_per_em: u16,
    ascender: i16,
    descender: i16,
    glyphs: BTreeMap<char, OutlineGlyph>,
}

impl OutlineFont {
    pub fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }

    pub fn contains(&self, character: char) -> bool {
        self.glyphs.contains_key(&character)
    }
}

#[derive(Clone, Debug)]
struct OutlineGlyph {
    advance: u16,
    commands: Vec<OutlineCommand>,
}

#[derive(Clone, Debug)]
enum OutlineCommand {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    QuadTo(f32, f32, f32, f32),
    CurveTo(f32, f32, f32, f32, f32, f32),
    Close,
}

#[derive(Clone, Debug)]
struct GlyphOutline {
    units_per_em: u16,
    ascender: i16,
    descender: i16,
    advance: u16,
    commands: Vec<OutlineCommand>,
}

#[derive(Clone, Debug)]
pub struct BigTileSpec {
    pub size: BigTextSize,
    pub cells: Size,
    pub character: StyledCharacter,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum FontIdentity {
    BizUdGothic,
    Shinonome,
    User(PathBuf, u128),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct BigTileKey {
    font: FontIdentity,
    character: char,
    foreground: [u8; 3],
    background: Option<[u8; 3]>,
    bold: bool,
    cells: Size,
    size: BigTextSize,
}

pub struct BigTextImageCache {
    cache: ProtocolCache<BigTileKey>,
}

pub(crate) struct BigTextImageState {
    configured: String,
    modified_nanos: u128,
    font: ResolvedBigFont,
    glyph_fallback_reason: Option<String>,
    cache: BigTextImageCache,
}

impl BigTextImageState {
    pub(crate) fn new() -> Self {
        Self {
            configured: String::new(),
            modified_nanos: 0,
            font: resolve_big_font("biz-ud-gothic", Path::new("/")),
            glyph_fallback_reason: None,
            cache: BigTextImageCache::new(),
        }
    }

    pub(crate) fn prepare(&mut self, configured: &str, home: &Path, detection: &GraphicsDetection, spec: &BigTileSpec) -> Result<(), String> {
        self.synchronize_font(configured, home);
        self.record_missing_glyph(spec.size, spec.character.character);
        self.cache.prepare(detection, &self.font, spec)
    }

    pub(crate) fn protocol<'a>(
        &'a mut self,
        configured: &str,
        home: &Path,
        detection: &GraphicsDetection,
        spec: &BigTileSpec,
    ) -> Option<&'a Protocol> {
        self.synchronize_font(configured, home);
        self.cache.protocol(detection, &self.font, spec)
    }

    pub(crate) fn can_render(&mut self, configured: &str, home: &Path, size: BigTextSize, character: char) -> bool {
        self.synchronize_font(configured, home);
        self.record_missing_glyph(size, character);
        if matches!(self.font.source, FontSource::Shinonome) {
            selected_source_has_glyph(&self.font.source, size, character)
        } else {
            source_has_glyph(&self.font.source, character)
                || bundled_outline_font().is_ok_and(|font| font.contains(character))
                || bitmap_font(size).is_ok_and(|font| font.contains(character))
        }
    }

    pub(crate) fn fallback_reason<'a>(&'a mut self, configured: &str, home: &Path) -> Option<&'a str> {
        self.synchronize_font(configured, home);
        self.font.fallback_reason().or(self.glyph_fallback_reason.as_deref())
    }

    pub(crate) fn resolved_font(&mut self, configured: &str, home: &Path) -> ResolvedBigFont {
        self.synchronize_font(configured, home);
        self.font.clone()
    }

    fn synchronize_font(&mut self, configured: &str, home: &Path) {
        let modified_nanos = configured_path(configured, home)
            .and_then(|path| fs::metadata(path).ok())
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |duration| duration.as_nanos());
        if self.configured != configured || self.modified_nanos != modified_nanos {
            self.font = resolve_big_font(configured, home);
            self.configured = configured.to_owned();
            self.modified_nanos = modified_nanos;
            self.glyph_fallback_reason = None;
            self.cache.clear();
        }
    }

    fn record_missing_glyph(&mut self, size: BigTextSize, character: char) {
        if self.glyph_fallback_reason.is_none() && !selected_source_has_glyph(&self.font.source, size, character) {
            self.glyph_fallback_reason = Some(format!("selected font has no glyph for {character:?}; using a fallback"));
        }
    }
}

impl Default for BigTextImageCache {
    fn default() -> Self {
        Self::new()
    }
}

impl BigTextImageCache {
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

    pub fn prepare(&mut self, detection: &GraphicsDetection, font: &ResolvedBigFont, spec: &BigTileSpec) -> Result<(), String> {
        if !spec.character.visible {
            return Ok(());
        }
        let key = tile_key(font, spec);
        self.cache
            .prepare(detection, key, spec.cells, |cell_pixels| render_big_tile(font, cell_pixels, spec))
    }

    pub(crate) fn protocol<'a>(&'a mut self, detection: &GraphicsDetection, font: &ResolvedBigFont, spec: &BigTileSpec) -> Option<&'a Protocol> {
        if !spec.character.visible {
            return None;
        }
        self.cache.protocol(detection, &tile_key(font, spec))
    }
}

pub fn bundled_outline_font() -> Result<&'static OutlineFont, &'static str> {
    match BUNDLED_FONT.get_or_init(|| decode_bundled_font(BUNDLED_OUTLINES)) {
        Ok(font) => Ok(font),
        Err(error) => Err(error.as_str()),
    }
}

pub fn resolve_big_font(value: &str, home: &Path) -> ResolvedBigFont {
    match value {
        "biz-ud-gothic" => ResolvedBigFont {
            source: FontSource::BizUdGothic,
            fallback_reason: None,
        },
        "shinonome" => ResolvedBigFont {
            source: FontSource::Shinonome,
            fallback_reason: None,
        },
        path => {
            let path = expand_home(path, home);
            match fs::read(&path) {
                Ok(bytes) => match ttf_parser::Face::parse(&bytes, 0) {
                    Ok(_) => {
                        let modified_nanos = fs::metadata(&path)
                            .and_then(|metadata| metadata.modified())
                            .ok()
                            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                            .map_or(0, |duration| duration.as_nanos());
                        ResolvedBigFont {
                            source: FontSource::User {
                                path,
                                bytes: bytes.into(),
                                modified_nanos,
                            },
                            fallback_reason: None,
                        }
                    }
                    Err(error) => fallback(format!("failed to parse {}: {error}", path.display())),
                },
                Err(error) => fallback(format!("failed to read {}: {error}", path.display())),
            }
        }
    }
}

fn fallback(reason: String) -> ResolvedBigFont {
    ResolvedBigFont {
        source: FontSource::BizUdGothic,
        fallback_reason: Some(reason),
    }
}

fn expand_home(value: &str, home: &Path) -> PathBuf {
    if value == "~" {
        home.to_owned()
    } else if let Some(rest) = value.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(value)
    }
}

fn configured_path(value: &str, home: &Path) -> Option<PathBuf> {
    (!matches!(value, "biz-ud-gothic" | "shinonome")).then(|| expand_home(value, home))
}

pub fn image_cell_height(size: BigTextSize, cell_pixels: CellPixels) -> u16 {
    let fullwidth_cells: u16 = match size {
        BigTextSize::Dot12 => 6,
        BigTextSize::Dot16 => 8,
    };
    (u32::from(fullwidth_cells) * u32::from(cell_pixels.width))
        .div_ceil(u32::from(cell_pixels.height))
        .max(1)
        .min(u32::from(u16::MAX)) as u16
}

pub fn render_big_tile(font: &ResolvedBigFont, cell_pixels: CellPixels, spec: &BigTileSpec) -> Result<RgbaImage, String> {
    let width = u32::from(spec.cells.width) * u32::from(cell_pixels.width);
    let height = u32::from(spec.cells.height) * u32::from(cell_pixels.height);
    let mut pixmap = Pixmap::new(width.max(1), height.max(1)).ok_or_else(|| "tile dimensions are too large".to_owned())?;
    if !spec.character.visible {
        return rgba_image(pixmap);
    }
    let (foreground, bold) = effective_appearance(&spec.character);
    draw_big_character(&mut pixmap, font, spec.size, spec.character.character, foreground, bold)?;
    rgba_image(pixmap)
}

pub(crate) fn draw_big_character(
    pixmap: &mut Pixmap,
    font: &ResolvedBigFont,
    size: BigTextSize,
    character: char,
    color: [u8; 3],
    bold: bool,
) -> Result<bool, String> {
    let rendered = match &font.source {
        FontSource::Shinonome => draw_shinonome(pixmap, size, character, color, bold),
        FontSource::BizUdGothic => draw_bundled_outline(pixmap, character, color, bold)?,
        source @ FontSource::User { .. } => {
            if let Some(outline) = outline_for(source, character)? {
                draw_outline(pixmap, &outline, color, bold);
                true
            } else if draw_bundled_outline(pixmap, character, color, bold)? {
                true
            } else {
                draw_shinonome(pixmap, size, character, color, bold)
            }
        }
    };
    Ok(rendered)
}

fn rgba_image(pixmap: Pixmap) -> Result<RgbaImage, String> {
    let width = pixmap.width();
    let height = pixmap.height();
    RgbaImage::from_raw(width, height, pixmap.take_demultiplied()).ok_or_else(|| "pixmap dimensions do not match its data".to_owned())
}

fn tile_key(font: &ResolvedBigFont, spec: &BigTileSpec) -> BigTileKey {
    let style = spec.character.style;
    BigTileKey {
        font: font.identity(),
        character: spec.character.character,
        foreground: style.fg.map_or([255, 255, 255], |color| color_rgb(color, [255, 255, 255])),
        background: style.bg.map(|color| color_rgb(color, [0, 0, 0])),
        bold: style.add_modifier.contains(Modifier::BOLD),
        cells: spec.cells,
        size: spec.size,
    }
}

fn effective_appearance(character: &StyledCharacter) -> ([u8; 3], bool) {
    let style = character.style;
    let color = if style.add_modifier.contains(Modifier::REVERSED) {
        style.bg.unwrap_or(Color::White)
    } else {
        style.fg.unwrap_or(Color::White)
    };
    (color_rgb(color, [255, 255, 255]), style.add_modifier.contains(Modifier::BOLD))
}

fn outline_for(source: &FontSource, character: char) -> Result<Option<GlyphOutline>, String> {
    match source {
        FontSource::User { bytes, .. } => {
            let face = ttf_parser::Face::parse(bytes, 0).map_err(|error| format!("failed to parse user font: {error}"))?;
            let Some(glyph_id) = face.glyph_index(character) else {
                return Ok(None);
            };
            let Some(advance) = face.glyph_hor_advance(glyph_id) else {
                return Ok(None);
            };
            let mut builder = CommandBuilder::default();
            face.outline_glyph(glyph_id, &mut builder);
            Ok(Some(GlyphOutline {
                units_per_em: face.units_per_em(),
                ascender: face.ascender(),
                descender: face.descender(),
                advance,
                commands: builder.commands,
            }))
        }
        FontSource::BizUdGothic | FontSource::Shinonome => Ok(None),
    }
}

fn source_has_glyph(source: &FontSource, character: char) -> bool {
    match source {
        FontSource::BizUdGothic => bundled_outline_font().is_ok_and(|font| font.contains(character)),
        FontSource::Shinonome => false,
        FontSource::User { bytes, .. } => ttf_parser::Face::parse(bytes, 0)
            .ok()
            .and_then(|face| face.glyph_index(character))
            .is_some(),
    }
}

fn selected_source_has_glyph(source: &FontSource, size: BigTextSize, character: char) -> bool {
    match source {
        FontSource::Shinonome => bitmap_font(size).is_ok_and(|font| font.contains(character)),
        _ => source_has_glyph(source, character),
    }
}

fn draw_bundled_outline(pixmap: &mut Pixmap, character: char, color: [u8; 3], bold: bool) -> Result<bool, String> {
    let font = bundled_outline_font().map_err(str::to_owned)?;
    let Some(glyph) = font.glyphs.get(&character) else {
        return Ok(false);
    };
    draw_outline_parts(
        pixmap,
        font.units_per_em,
        font.ascender,
        font.descender,
        glyph.advance,
        &glyph.commands,
        color,
        bold,
    );
    Ok(true)
}

fn draw_outline(pixmap: &mut Pixmap, glyph: &GlyphOutline, color: [u8; 3], bold: bool) {
    draw_outline_parts(
        pixmap,
        glyph.units_per_em,
        glyph.ascender,
        glyph.descender,
        glyph.advance,
        &glyph.commands,
        color,
        bold,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_outline_parts(
    pixmap: &mut Pixmap,
    units_per_em: u16,
    ascender: i16,
    descender: i16,
    advance_units: u16,
    commands: &[OutlineCommand],
    color: [u8; 3],
    bold: bool,
) {
    let vertical_units = i32::from(ascender) - i32::from(descender);
    if vertical_units <= 0 || units_per_em == 0 {
        return;
    }
    let scale = pixmap.height() as f32 / f32::from(units_per_em);
    let advance = f32::from(advance_units) * scale;
    let origin_x = (pixmap.width() as f32 - advance) / 2.0;
    let vertical_height = vertical_units as f32 * scale;
    let baseline = (pixmap.height() as f32 - vertical_height) / 2.0 + f32::from(ascender) * scale;
    let map = |x: f32, y: f32| (origin_x + x * scale, baseline - y * scale);
    let mut builder = PathBuilder::new();
    for command in commands {
        match *command {
            OutlineCommand::MoveTo(x, y) => {
                let (x, y) = map(x, y);
                builder.move_to(x, y);
            }
            OutlineCommand::LineTo(x, y) => {
                let (x, y) = map(x, y);
                builder.line_to(x, y);
            }
            OutlineCommand::QuadTo(x1, y1, x, y) => {
                let (x1, y1) = map(x1, y1);
                let (x, y) = map(x, y);
                builder.quad_to(x1, y1, x, y);
            }
            OutlineCommand::CurveTo(x1, y1, x2, y2, x, y) => {
                let (x1, y1) = map(x1, y1);
                let (x2, y2) = map(x2, y2);
                let (x, y) = map(x, y);
                builder.cubic_to(x1, y1, x2, y2, x, y);
            }
            OutlineCommand::Close => builder.close(),
        }
    }
    let Some(path) = builder.finish() else { return };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], 255);
    paint.anti_alias = true;
    pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    if bold {
        pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::from_translate(1.0, 0.0), None);
    }
}

fn draw_shinonome(pixmap: &mut Pixmap, size: BigTextSize, character: char, color: [u8; 3], bold: bool) -> bool {
    let Ok(font) = bitmap_font(size) else { return false };
    let Some((width, height, rows)) = font.glyph_bitmap(character) else {
        return false;
    };
    let scale = (pixmap.width() / u32::from(width)).min(pixmap.height() / u32::from(height));
    if scale == 0 {
        return false;
    }
    let drawn_width = u32::from(width) * scale;
    let drawn_height = u32::from(height) * scale;
    let origin_x = (pixmap.width() - drawn_width) / 2;
    let origin_y = (pixmap.height() - drawn_height) / 2;
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], 255);
    paint.anti_alias = false;
    for (row, bits) in rows.iter().enumerate() {
        for column in 0..width {
            if bits & (1 << (15 - column)) == 0 {
                continue;
            }
            let x = origin_x + u32::from(column) * scale;
            let y = origin_y + row as u32 * scale;
            fill_pixel_block(pixmap, x, y, scale, &paint);
            if bold && x + 1 < pixmap.width() {
                fill_pixel_block(pixmap, x + 1, y, scale, &paint);
            }
        }
    }
    true
}

fn fill_pixel_block(pixmap: &mut Pixmap, x: u32, y: u32, scale: u32, paint: &Paint<'_>) {
    if let Some(rect) = tiny_skia::Rect::from_xywh(x as f32, y as f32, scale as f32, scale as f32) {
        pixmap.fill_rect(rect, paint, Transform::identity(), None);
    }
}

#[derive(Default)]
struct CommandBuilder {
    commands: Vec<OutlineCommand>,
}

impl ttf_parser::OutlineBuilder for CommandBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.commands.push(OutlineCommand::MoveTo(x, y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.commands.push(OutlineCommand::LineTo(x, y));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.commands.push(OutlineCommand::QuadTo(x1, y1, x, y));
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.commands.push(OutlineCommand::CurveTo(x1, y1, x2, y2, x, y));
    }

    fn close(&mut self) {
        self.commands.push(OutlineCommand::Close);
    }
}

fn decode_bundled_font(compressed: &[u8]) -> Result<OutlineFont, String> {
    let mut bytes = Vec::new();
    GzDecoder::new(compressed)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("failed to decompress bundled outline font: {error}"))?;
    decode_outline_font(&bytes)
}

fn decode_outline_font(bytes: &[u8]) -> Result<OutlineFont, String> {
    if bytes.len() < 20 || &bytes[..8] != OUTLINE_MAGIC {
        return Err("bundled outline font has an invalid header".to_owned());
    }
    let version = u16::from_le_bytes(bytes[8..10].try_into().expect("two-byte version"));
    if version != OUTLINE_VERSION {
        return Err(format!("unsupported bundled outline font version {version}"));
    }
    let units_per_em = u16::from_le_bytes(bytes[10..12].try_into().expect("two-byte units"));
    let ascender = i16::from_le_bytes(bytes[12..14].try_into().expect("two-byte ascender"));
    let descender = i16::from_le_bytes(bytes[14..16].try_into().expect("two-byte descender"));
    let count = u32::from_le_bytes(bytes[16..20].try_into().expect("four-byte count"));
    let mut offset = 20usize;
    let mut glyphs = BTreeMap::new();
    for _ in 0..count {
        if offset + 10 > bytes.len() {
            return Err("bundled outline glyph header is truncated".to_owned());
        }
        let codepoint = u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four-byte codepoint"));
        let character = char::from_u32(codepoint).ok_or_else(|| format!("invalid Unicode scalar U+{codepoint:04X}"))?;
        let advance = u16::from_le_bytes(bytes[offset + 4..offset + 6].try_into().expect("two-byte advance"));
        let length = u32::from_le_bytes(bytes[offset + 6..offset + 10].try_into().expect("four-byte length")) as usize;
        offset += 10;
        let end = offset.checked_add(length).ok_or_else(|| "bundled outline length overflow".to_owned())?;
        let data = bytes
            .get(offset..end)
            .ok_or_else(|| "bundled outline glyph data is truncated".to_owned())?;
        let commands = decode_commands(data)?;
        offset = end;
        if glyphs.insert(character, OutlineGlyph { advance, commands }).is_some() {
            return Err(format!("duplicate bundled outline glyph for {character:?}"));
        }
    }
    if offset != bytes.len() {
        return Err("bundled outline font has trailing data".to_owned());
    }
    Ok(OutlineFont {
        units_per_em,
        ascender,
        descender,
        glyphs,
    })
}

fn decode_commands(bytes: &[u8]) -> Result<Vec<OutlineCommand>, String> {
    let mut offset = 0usize;
    let mut previous = (0i32, 0i32);
    let mut commands = Vec::new();
    while offset < bytes.len() {
        let opcode = bytes[offset];
        offset += 1;
        let mut point = || -> Result<(f32, f32), String> {
            let x = previous.0.wrapping_add(unzigzag(decode_varint(bytes, &mut offset)?));
            let y = previous.1.wrapping_add(unzigzag(decode_varint(bytes, &mut offset)?));
            previous = (x, y);
            Ok((x as f32, y as f32))
        };
        commands.push(match opcode {
            0 => {
                let (x, y) = point()?;
                OutlineCommand::MoveTo(x, y)
            }
            1 => {
                let (x, y) = point()?;
                OutlineCommand::LineTo(x, y)
            }
            2 => {
                let (x1, y1) = point()?;
                let (x, y) = point()?;
                OutlineCommand::QuadTo(x1, y1, x, y)
            }
            3 => {
                let (x1, y1) = point()?;
                let (x2, y2) = point()?;
                let (x, y) = point()?;
                OutlineCommand::CurveTo(x1, y1, x2, y2, x, y)
            }
            4 => OutlineCommand::Close,
            value => return Err(format!("invalid bundled outline opcode {value}")),
        });
    }
    Ok(commands)
}

fn decode_varint(bytes: &[u8], offset: &mut usize) -> Result<u32, String> {
    let mut value = 0u32;
    for shift in (0..35).step_by(7) {
        let byte = *bytes.get(*offset).ok_or_else(|| "truncated bundled outline coordinate".to_owned())?;
        *offset += 1;
        if shift == 28 && byte > 0x0f {
            return Err("bundled outline coordinate overflow".to_owned());
        }
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err("bundled outline coordinate overflow".to_owned())
}

fn unzigzag(value: u32) -> i32 {
    ((value >> 1) as i32) ^ -((value & 1) as i32)
}

impl Hash for BigTextSize {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            BigTextSize::Dot12 => 12u8.hash(state),
            BigTextSize::Dot16 => 16u8.hash(state),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BigTextImageState, BigTextSize};
    use std::path::Path;

    #[test]
    fn missing_selected_glyph_records_the_character_fallback_reason() {
        let mut state = BigTextImageState::new();

        assert!(!state.can_render("biz-ud-gothic", Path::new("/unused"), BigTextSize::Dot16, '😀'));
        let reason = state.fallback_reason("biz-ud-gothic", Path::new("/unused")).unwrap();
        assert!(reason.contains("😀"), "{reason}");
    }
}
