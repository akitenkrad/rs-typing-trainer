//! Cached Shinonome bitmap fonts and 2×2-dot quadrant-block rendering.

use std::collections::BTreeMap;
use std::io::Read;
use std::ops::Range;
use std::sync::OnceLock;

use flate2::read::GzDecoder;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

const MAGIC: &[u8; 8] = b"SHNFONT\0";
const FORMAT_VERSION: u8 = 1;
const FONT_12_GZ: &[u8] = include_bytes!("../../data/font/shinonome12.bin.gz");
const FONT_16_GZ: &[u8] = include_bytes!("../../data/font/shinonome16.bin.gz");

static FONT_12: OnceLock<Result<BitmapFont, String>> = OnceLock::new();
static FONT_16: OnceLock<Result<BitmapFont, String>> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BigTextSize {
    Dot12,
    Dot16,
}

impl BigTextSize {
    pub fn cell_height(self) -> u16 {
        match self {
            Self::Dot12 => 6,
            Self::Dot16 => 8,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct StyledCharacter {
    pub character: char,
    pub style: Style,
    pub visible: bool,
}

impl StyledCharacter {
    pub fn new(character: char, style: Style) -> Self {
        Self {
            character,
            style,
            visible: true,
        }
    }

    pub fn hidden(character: char, style: Style) -> Self {
        Self {
            character,
            style,
            visible: false,
        }
    }
}

#[derive(Clone, Debug)]
struct Glyph {
    width: u8,
    rows: Box<[u16]>,
}

#[derive(Clone, Debug)]
pub struct BitmapFont {
    height: u8,
    half_width: u8,
    full_width: u8,
    glyphs: BTreeMap<char, Glyph>,
}

#[derive(Clone, Debug)]
pub struct RenderedBigText {
    pub lines: Vec<Line<'static>>,
    pub width: u16,
    pub character_columns: Vec<u16>,
    character_widths: Vec<u16>,
}

impl RenderedBigText {
    pub(crate) fn character_width(&self, index: usize) -> Option<u16> {
        self.character_widths.get(index).copied()
    }
    pub fn cursor_line(&self, current: usize, style: Style) -> Line<'static> {
        let spans = self
            .character_widths
            .iter()
            .enumerate()
            .map(|(index, width)| {
                let symbol = if index == current { '▔' } else { ' ' };
                Span::styled(
                    symbol.to_string().repeat(usize::from(*width)),
                    if index == current { style } else { Style::default() },
                )
            })
            .collect::<Vec<_>>();
        Line::from(spans)
    }

    pub fn marker_line(&self, markers: &[(usize, char, Style)]) -> Line<'static> {
        let mut cells = vec![(' ', Style::default()); usize::from(self.width)];
        for (character_index, symbol, style) in markers {
            if let Some(column) = self.character_columns.get(*character_index).copied() {
                let column = usize::from(column).min(cells.len().saturating_sub(1));
                cells[column] = (*symbol, *style);
            }
        }
        Line::from(
            cells
                .into_iter()
                .map(|(symbol, style)| Span::styled(symbol.to_string(), style))
                .collect::<Vec<_>>(),
        )
    }
}

pub fn font(size: BigTextSize) -> Result<&'static BitmapFont, &'static str> {
    let result = match size {
        BigTextSize::Dot12 => FONT_12.get_or_init(|| decode_font(FONT_12_GZ)),
        BigTextSize::Dot16 => FONT_16.get_or_init(|| decode_font(FONT_16_GZ)),
    };
    match result {
        Ok(font) => Ok(font),
        Err(error) => Err(error.as_str()),
    }
}

pub fn window_range(font: &BitmapFont, text: &str, current: usize, max_width: u16) -> Range<usize> {
    let widths = text.chars().map(|character| font.character_width(character)).collect::<Vec<_>>();
    window_by_widths(&widths, current, max_width)
}

/// Widest run of consecutive items around `current` whose widths sum to at
/// most `max_width`, grown alternately to the left and right so the current
/// item stays near the middle. The current item is always included.
pub fn window_by_widths(widths: &[u16], current: usize, max_width: u16) -> Range<usize> {
    if widths.is_empty() {
        return 0..0;
    }
    let current = current.min(widths.len() - 1);
    let mut start = current;
    let mut end = current + 1;
    let mut width = widths[current];
    loop {
        let mut changed = false;
        if start > 0 && width.saturating_add(widths[start - 1]) <= max_width {
            start -= 1;
            width += widths[start];
            changed = true;
        }
        if end < widths.len() && width.saturating_add(widths[end]) <= max_width {
            width += widths[end];
            end += 1;
            changed = true;
        }
        if !changed {
            break;
        }
    }
    start..end
}

impl BitmapFont {
    pub(crate) fn glyph_bitmap(&self, character: char) -> Option<(u8, u8, &[u16])> {
        self.glyphs.get(&character).map(|glyph| (glyph.width, self.height, glyph.rows.as_ref()))
    }

    pub fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }

    pub fn halfwidth_glyph_count(&self) -> usize {
        self.glyphs.values().filter(|glyph| glyph.width == self.half_width).count()
    }

    pub fn fullwidth_glyph_count(&self) -> usize {
        self.glyphs.values().filter(|glyph| glyph.width == self.full_width).count()
    }

    pub fn contains(&self, character: char) -> bool {
        self.glyphs.contains_key(&character)
    }

    pub fn text_width(&self, text: &str) -> u16 {
        text.chars().map(|character| self.character_width(character)).sum()
    }

    pub fn character_width(&self, character: char) -> u16 {
        self.glyphs.get(&character).map_or_else(
            || {
                if UnicodeWidthChar::width(character).unwrap_or(0) >= 2 {
                    u16::from(self.full_width).div_ceil(2)
                } else {
                    u16::from(self.half_width).div_ceil(2)
                }
            },
            |glyph| u16::from(glyph.width).div_ceil(2),
        )
    }

    pub fn render(&self, characters: &[StyledCharacter]) -> RenderedBigText {
        let row_count = usize::from(self.height).div_ceil(2);
        let character_widths = characters
            .iter()
            .map(|character| self.character_width(character.character))
            .collect::<Vec<_>>();
        let mut character_columns = Vec::with_capacity(characters.len() + 1);
        let mut width = 0u16;
        for character_width in &character_widths {
            character_columns.push(width);
            width = width.saturating_add(*character_width);
        }
        character_columns.push(width);

        let lines = (0..row_count)
            .map(|cell_y| {
                let spans = characters
                    .iter()
                    .zip(&character_widths)
                    .map(|(character, cell_width)| {
                        let content = if !character.visible {
                            " ".repeat(usize::from(*cell_width))
                        } else if let Some(glyph) = self.glyphs.get(&character.character) {
                            render_glyph_row(glyph, cell_y)
                        } else {
                            render_fallback_row(character.character, *cell_width, row_count, cell_y)
                        };
                        Span::styled(content, character.style)
                    })
                    .collect::<Vec<_>>();
                Line::from(spans)
            })
            .collect::<Vec<_>>();

        RenderedBigText {
            lines,
            width,
            character_columns,
            character_widths,
        }
    }
}

fn render_glyph_row(glyph: &Glyph, cell_y: usize) -> String {
    let top = cell_y * 2;
    let bottom = top + 1;
    (0..usize::from(glyph.width))
        .step_by(2)
        .map(|x| {
            let mask = bit(glyph, top, x) * 8 + bit(glyph, top, x + 1) * 4 + bit(glyph, bottom, x) * 2 + bit(glyph, bottom, x + 1);
            quadrant(mask)
        })
        .collect()
}

fn bit(glyph: &Glyph, row: usize, column: usize) -> u8 {
    if row >= glyph.rows.len() || column >= usize::from(glyph.width) {
        return 0;
    }
    u8::from(glyph.rows[row] & (1 << (15 - column)) != 0)
}

fn quadrant(mask: u8) -> char {
    const QUADRANTS: [char; 16] = [' ', '▗', '▖', '▄', '▝', '▐', '▞', '▟', '▘', '▚', '▌', '▙', '▀', '▜', '▛', '█'];
    QUADRANTS[usize::from(mask)]
}

pub(crate) fn render_fallback_row(character: char, cell_width: u16, row_count: usize, row: usize) -> String {
    let width = usize::from(cell_width);
    if row != row_count / 2 {
        return " ".repeat(width);
    }
    let character_width = UnicodeWidthChar::width(character).unwrap_or(0).min(width);
    let left = width.saturating_sub(character_width) / 2;
    let right = width.saturating_sub(left + character_width);
    format!("{}{}{}", " ".repeat(left), character, " ".repeat(right))
}

fn decode_font(compressed: &[u8]) -> Result<BitmapFont, String> {
    let mut bytes = Vec::new();
    GzDecoder::new(compressed)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("failed to decompress bundled font: {error}"))?;
    if bytes.len() < 20 || &bytes[..8] != MAGIC {
        return Err("bundled font has an invalid header".to_owned());
    }
    if bytes[8] != FORMAT_VERSION {
        return Err(format!("unsupported bundled font version {}", bytes[8]));
    }
    let height = bytes[9];
    let half_width = bytes[10];
    let full_width = bytes[11];
    let count = u32::from_le_bytes(bytes[12..16].try_into().expect("four-byte count")) as usize;
    let record_size = 5 + usize::from(height) * 2;
    let expected = 20usize
        .checked_add(count.checked_mul(record_size).ok_or_else(|| "bundled font size overflow".to_owned())?)
        .ok_or_else(|| "bundled font size overflow".to_owned())?;
    if bytes.len() != expected {
        return Err(format!("bundled font length is {}, expected {expected}", bytes.len()));
    }
    let mut glyphs = BTreeMap::new();
    for record in bytes[20..].chunks_exact(record_size) {
        let codepoint = u32::from_le_bytes(record[..4].try_into().expect("four-byte codepoint"));
        let character = char::from_u32(codepoint).ok_or_else(|| format!("invalid Unicode codepoint U+{codepoint:04X}"))?;
        let width = record[4];
        if !matches!(width, value if value == half_width || value == full_width) {
            return Err(format!("invalid glyph width {width} for {character:?}"));
        }
        let rows = record[5..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|row| u16::from_le_bytes(*row))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        if glyphs.insert(character, Glyph { width, rows }).is_some() {
            return Err(format!("duplicate glyph for {character:?}"));
        }
    }
    Ok(BitmapFont {
        height,
        half_width,
        full_width,
        glyphs,
    })
}
