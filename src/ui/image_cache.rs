//! Shared fixed-protocol tile cache for terminal images.

use std::collections::HashMap;
use std::hash::Hash;

use image::{DynamicImage, RgbaImage};
use ratatui::layout::Size;
use ratatui::style::Color;
use ratatui_image::protocol::Protocol;
use ratatui_image::{Resize, picker::Picker};

use crate::graphics::{CellPixels, GraphicsDetection, GraphicsMode};

pub(crate) const DEFAULT_TILE_CACHE_LIMIT: usize = 1024;

pub(crate) struct ProtocolCache<K> {
    signature: Option<(GraphicsMode, CellPixels)>,
    picker: Option<Picker>,
    tiles: HashMap<K, Protocol>,
    limit: usize,
    creation_count: usize,
}

impl<K: Eq + Hash> ProtocolCache<K> {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            signature: None,
            picker: None,
            tiles: HashMap::new(),
            limit: limit.max(1),
            creation_count: 0,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.tiles.len()
    }

    pub(crate) fn creation_count(&self) -> usize {
        self.creation_count
    }

    pub(crate) fn clear(&mut self) {
        self.signature = None;
        self.picker = None;
        self.tiles.clear();
    }

    pub(crate) fn prepare(
        &mut self,
        detection: &GraphicsDetection,
        key: K,
        cells: Size,
        render: impl FnOnce(CellPixels) -> Result<RgbaImage, String>,
    ) -> Result<(), String> {
        let cell_pixels = self.synchronize(detection)?;
        if self.tiles.contains_key(&key) {
            return Ok(());
        }
        if self.tiles.len() >= self.limit {
            self.tiles.clear();
        }
        let image = DynamicImage::ImageRgba8(render(cell_pixels)?);
        let protocol = self
            .picker
            .as_ref()
            .ok_or_else(|| "image picker is unavailable".to_owned())?
            .new_protocol(image, cells, Resize::Fit(None))
            .map_err(|error| error.to_string())?;
        self.tiles.insert(key, protocol);
        self.creation_count += 1;
        Ok(())
    }

    pub(crate) fn protocol(&mut self, detection: &GraphicsDetection, key: &K) -> Option<&Protocol> {
        self.synchronize(detection).ok()?;
        self.tiles.get(key)
    }

    fn synchronize(&mut self, detection: &GraphicsDetection) -> Result<CellPixels, String> {
        let cell_pixels = detection.cell_pixels.ok_or_else(|| "cell pixel size is unavailable".to_owned())?;
        if detection.mode == GraphicsMode::Off {
            return Err("image protocol is disabled".to_owned());
        }
        let signature = (detection.mode, cell_pixels);
        if self.signature != Some(signature) {
            self.tiles.clear();
            self.picker = detection.picker();
            self.signature = Some(signature);
        }
        Ok(cell_pixels)
    }
}

pub(crate) fn color_rgb(color: Color, reset: [u8; 3]) -> [u8; 3] {
    match color {
        Color::Reset => reset,
        Color::Black => [0, 0, 0],
        Color::Red => [205, 49, 49],
        Color::Green => [13, 188, 121],
        Color::Yellow => [229, 229, 16],
        Color::Blue => [36, 114, 200],
        Color::Magenta => [188, 63, 188],
        Color::Cyan => [17, 168, 205],
        Color::Gray => [204, 204, 204],
        Color::DarkGray => [102, 102, 102],
        Color::LightRed => [241, 76, 76],
        Color::LightGreen => [144, 238, 144],
        Color::LightYellow => [245, 245, 67],
        Color::LightBlue => [59, 142, 234],
        Color::LightMagenta => [214, 112, 214],
        Color::LightCyan => [41, 184, 219],
        Color::White => [255, 255, 255],
        Color::Rgb(red, green, blue) => [red, green, blue],
        Color::Indexed(index) => indexed_rgb(index),
    }
}

fn indexed_rgb(index: u8) -> [u8; 3] {
    const ANSI: [[u8; 3]; 16] = [
        [0, 0, 0],
        [128, 0, 0],
        [0, 128, 0],
        [128, 128, 0],
        [0, 0, 128],
        [128, 0, 128],
        [0, 128, 128],
        [192, 192, 192],
        [128, 128, 128],
        [255, 0, 0],
        [0, 255, 0],
        [255, 255, 0],
        [0, 0, 255],
        [255, 0, 255],
        [0, 255, 255],
        [255, 255, 255],
    ];
    if index < 16 {
        return ANSI[usize::from(index)];
    }
    if index < 232 {
        let cube = index - 16;
        let value = |component: u8| if component == 0 { 0 } else { 55 + component * 40 };
        return [value(cube / 36), value((cube / 6) % 6), value(cube % 6)];
    }
    let gray = 8 + (index - 232) * 10;
    [gray, gray, gray]
}
