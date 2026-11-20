//! Pixel rendering and fixed-protocol caching for the result screen.

use std::collections::HashMap;

use chrono::NaiveDate;
use image::RgbaImage;
use ratatui::layout::Size;
use ratatui::style::Color;
use ratatui_image::protocol::Protocol;
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke, Transform};
use unicode_width::UnicodeWidthChar;

use crate::graphics::{CellPixels, GraphicsDetection};
use crate::i18n::Lang;
use crate::ui::bigtext::BigTextSize;
use crate::ui::bigtext_image::{FontIdentity, ResolvedBigFont, draw_big_character};
use crate::ui::image_cache::{DEFAULT_TILE_CACHE_LIMIT, ProtocolCache, color_rgb};
use crate::ui::keyboard::KeyTilePlacement;
use crate::ui::keyboard_image::render_key_tile;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResultImageColors {
    pub foreground: Color,
    pub muted: Color,
    pub correct: Color,
    pub mistype: Color,
    pub accent: Color,
    pub warning: Color,
    pub panel: Color,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MetricCardSpec {
    pub label: String,
    pub value: String,
    pub previous_delta: Option<f64>,
    pub best_delta: Option<f64>,
    pub recent: Vec<f64>,
    pub lang: Lang,
    pub delta_suffix: String,
    pub cells: Size,
    pub colors: ResultImageColors,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BenchPartCardSpec {
    pub label: String,
    pub wpm: f64,
    pub accuracy: f64,
    pub previous_wpm_delta: Option<f64>,
    pub best_wpm_delta: Option<f64>,
    pub previous_accuracy_delta: Option<f64>,
    pub best_accuracy_delta: Option<f64>,
    pub version: String,
    pub lang: Lang,
    pub cells: Size,
    pub colors: ResultImageColors,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BenchTrendPoint {
    pub date: NaiveDate,
    pub wpm: f64,
    pub accuracy: f64,
    pub starts_new_series: bool,
    pub current: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BenchTrendSpec {
    pub points: Vec<BenchTrendPoint>,
    pub cells: Size,
    pub colors: ResultImageColors,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricDeltaKind {
    Previous,
    Best,
}

pub fn format_metric_delta(lang: Lang, kind: MetricDeltaKind, delta: f64, suffix: &str) -> String {
    if kind == MetricDeltaKind::Best && delta > 0.0 {
        return match lang {
            Lang::En => "New best!".to_owned(),
            Lang::Ja => "自己ベスト更新!".to_owned(),
        };
    }
    let label = match (lang, kind) {
        (Lang::En, MetricDeltaKind::Previous) => "prev",
        (Lang::En, MetricDeltaKind::Best) => "best",
        (Lang::Ja, MetricDeltaKind::Previous) => "前回",
        (Lang::Ja, MetricDeltaKind::Best) => "自己ベスト",
    };
    if delta == 0.0 {
        format!("{label} ±0{suffix}")
    } else if delta > 0.0 {
        format!("{label} +{delta:.1}{suffix}")
    } else {
        format!("{label} -{:.1}{suffix}", delta.abs())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemSpeedPoint {
    pub wpm: f64,
    pub mistypes: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpeedChartSpec {
    pub points: Vec<ItemSpeedPoint>,
    pub lang: Lang,
    pub cells: Size,
    pub colors: ResultImageColors,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MistypeKeyboardSpec {
    pub placements: Vec<KeyTilePlacement>,
    pub cells: Size,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlotBounds {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReactionHistogramSpec {
    pub reaction_seconds: Vec<f64>,
    pub p50_seconds: f64,
    pub p90_seconds: f64,
    pub cells: Size,
    pub colors: ResultImageColors,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PlacementKey {
    x: u16,
    y: u16,
    cells: Size,
    label: String,
    face: [u8; 3],
    text: [u8; 3],
    bold: bool,
    pressed: bool,
    modifier: bool,
    finger: Option<[u8; 3]>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ResultImageKey {
    BenchPartCard {
        font: FontIdentity,
        label: String,
        wpm: u64,
        accuracy: u64,
        previous_wpm_delta: Option<u64>,
        best_wpm_delta: Option<u64>,
        previous_accuracy_delta: Option<u64>,
        best_accuracy_delta: Option<u64>,
        version: String,
        japanese: bool,
        cells: Size,
        colors: [[u8; 3]; 7],
    },
    BenchTrend {
        font: FontIdentity,
        points: Vec<(NaiveDate, u64, u64, bool, bool)>,
        cells: Size,
        colors: [[u8; 3]; 7],
    },
    MetricCard {
        font: FontIdentity,
        label: String,
        value: String,
        previous_delta: Option<u64>,
        best_delta: Option<u64>,
        recent: Vec<u64>,
        japanese: bool,
        delta_suffix: String,
        cells: Size,
        colors: [[u8; 3]; 7],
    },
    SpeedChart {
        font: FontIdentity,
        points: Vec<(u64, Option<u32>)>,
        japanese: bool,
        cells: Size,
        colors: [[u8; 3]; 7],
    },
    MistypeKeyboard {
        placements: Vec<PlacementKey>,
        cells: Size,
    },
    ReactionHistogram {
        font: FontIdentity,
        reaction_seconds: Vec<u64>,
        p50_seconds: u64,
        p90_seconds: u64,
        cells: Size,
        colors: [[u8; 3]; 7],
    },
}

pub struct ResultImageCache {
    cache: ProtocolCache<ResultImageKey>,
}

impl Default for ResultImageCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ResultImageCache {
    pub fn new() -> Self {
        Self {
            cache: ProtocolCache::new(DEFAULT_TILE_CACHE_LIMIT),
        }
    }

    pub fn creation_count(&self) -> usize {
        self.cache.creation_count()
    }

    pub fn clear(&mut self) {
        self.cache.clear();
    }

    pub fn prepare_metric_card(&mut self, detection: &GraphicsDetection, font: &ResolvedBigFont, spec: &MetricCardSpec) -> Result<(), String> {
        self.cache.prepare(detection, metric_card_key(font, spec), spec.cells, |cell_pixels| {
            render_metric_card(font, cell_pixels, spec)
        })
    }

    pub fn prepare_bench_part_card(&mut self, detection: &GraphicsDetection, font: &ResolvedBigFont, spec: &BenchPartCardSpec) -> Result<(), String> {
        self.cache.prepare(detection, bench_part_card_key(font, spec), spec.cells, |cell_pixels| {
            render_bench_part_card(font, cell_pixels, spec)
        })
    }

    pub fn prepare_bench_trend(&mut self, detection: &GraphicsDetection, font: &ResolvedBigFont, spec: &BenchTrendSpec) -> Result<(), String> {
        self.cache.prepare(detection, bench_trend_key(font, spec), spec.cells, |cell_pixels| {
            render_bench_trend(font, cell_pixels, spec)
        })
    }

    pub fn prepare_speed_chart(&mut self, detection: &GraphicsDetection, font: &ResolvedBigFont, spec: &SpeedChartSpec) -> Result<(), String> {
        self.cache.prepare(detection, speed_chart_key(font, spec), spec.cells, |cell_pixels| {
            render_speed_chart(font, cell_pixels, spec)
        })
    }

    pub fn prepare_mistype_keyboard(&mut self, detection: &GraphicsDetection, spec: &MistypeKeyboardSpec) -> Result<(), String> {
        self.cache.prepare(detection, mistype_keyboard_key(spec), spec.cells, |cell_pixels| {
            render_mistype_keyboard(cell_pixels, spec)
        })
    }

    pub fn prepare_reaction_histogram(
        &mut self,
        detection: &GraphicsDetection,
        font: &ResolvedBigFont,
        spec: &ReactionHistogramSpec,
    ) -> Result<(), String> {
        self.cache
            .prepare(detection, reaction_histogram_key(font, spec), spec.cells, |cell_pixels| {
                render_reaction_histogram(font, cell_pixels, spec)
            })
    }

    pub(crate) fn metric_card_protocol<'a>(
        &'a mut self,
        detection: &GraphicsDetection,
        font: &ResolvedBigFont,
        spec: &MetricCardSpec,
    ) -> Option<&'a Protocol> {
        self.cache.protocol(detection, &metric_card_key(font, spec))
    }

    pub(crate) fn bench_part_card_protocol<'a>(
        &'a mut self,
        detection: &GraphicsDetection,
        font: &ResolvedBigFont,
        spec: &BenchPartCardSpec,
    ) -> Option<&'a Protocol> {
        self.cache.protocol(detection, &bench_part_card_key(font, spec))
    }

    pub(crate) fn bench_trend_protocol<'a>(
        &'a mut self,
        detection: &GraphicsDetection,
        font: &ResolvedBigFont,
        spec: &BenchTrendSpec,
    ) -> Option<&'a Protocol> {
        self.cache.protocol(detection, &bench_trend_key(font, spec))
    }

    pub(crate) fn speed_chart_protocol<'a>(
        &'a mut self,
        detection: &GraphicsDetection,
        font: &ResolvedBigFont,
        spec: &SpeedChartSpec,
    ) -> Option<&'a Protocol> {
        self.cache.protocol(detection, &speed_chart_key(font, spec))
    }

    pub(crate) fn mistype_keyboard_protocol<'a>(&'a mut self, detection: &GraphicsDetection, spec: &MistypeKeyboardSpec) -> Option<&'a Protocol> {
        self.cache.protocol(detection, &mistype_keyboard_key(spec))
    }

    pub(crate) fn reaction_histogram_protocol<'a>(
        &'a mut self,
        detection: &GraphicsDetection,
        font: &ResolvedBigFont,
        spec: &ReactionHistogramSpec,
    ) -> Option<&'a Protocol> {
        self.cache.protocol(detection, &reaction_histogram_key(font, spec))
    }
}

impl ReactionHistogramSpec {
    pub fn plot_bounds(&self, width: u32, height: u32) -> PlotBounds {
        PlotBounds {
            left: (width / 18).max(3),
            top: (height / 5).max(14),
            right: width.saturating_sub((width / 32).max(3)),
            bottom: height.saturating_sub((height / 8).max(10)),
        }
    }

    pub fn marker_x(&self, plot: PlotBounds, seconds: f64) -> u32 {
        let width = plot.right.saturating_sub(plot.left).max(1);
        plot.left + ((seconds.clamp(0.0, 1.5) / 1.5) * f64::from(width)).round() as u32
    }
}

/// Pixel height of a metric card's headline: 1.3 cells.
fn metric_card_label_height(cell_pixels: CellPixels) -> u32 {
    (u32::from(cell_pixels.height) * 13 / 10).max(12)
}

impl MetricCardSpec {
    fn value_layout(&self, width: u32, height: u32, cell_pixels: CellPixels) -> (f32, f32, u32) {
        let label_height = metric_card_label_height(cell_pixels);
        let top = (height as f32 * 0.17).max(label_height as f32 + 5.0);
        let glyph_height = (height as f32 * 0.43).round().max(18.0) as u32;
        let center = if self.recent.len() >= 2 {
            width as f32 * 0.34
        } else {
            width as f32 / 2.0
        };
        (center, top, glyph_height)
    }

    pub fn value_bounds(&self, width: u32, height: u32, cell_pixels: CellPixels) -> PlotBounds {
        let (center, top, glyph_height) = self.value_layout(width, height, cell_pixels);
        let cell_width = (u32::from(cell_pixels.width) * glyph_height / u32::from(cell_pixels.height).max(1)).max(1);
        let text_width = self
            .value
            .chars()
            .map(|character| cell_width * UnicodeWidthChar::width(character).unwrap_or(1).max(1) as u32)
            .sum::<u32>();
        PlotBounds {
            left: (center - text_width as f32 / 2.0).floor().max(0.0) as u32,
            top: top.floor().max(0.0) as u32,
            right: (center + text_width as f32 / 2.0).ceil().min(width as f32) as u32,
            bottom: (top + glyph_height as f32).ceil().min(height as f32) as u32,
        }
    }

    fn sparkline_bounds(&self, width: u32, height: u32, cell_pixels: CellPixels) -> [f32; 4] {
        let (_, top, glyph_height) = self.value_layout(width, height, cell_pixels);
        [
            width as f32 * 0.68,
            top + 6.0,
            width as f32 - 12.0,
            (top + glyph_height as f32 - 6.0).min(height as f32 - 1.0),
        ]
    }

    pub fn sparkline_points(&self, width: u32, height: u32) -> Vec<(u32, u32)> {
        let cell_pixels = CellPixels::new(
            (width / u32::from(self.cells.width.max(1))).max(1) as u16,
            (height / u32::from(self.cells.height.max(1))).max(1) as u16,
        );
        normalized_sparkline_points(&self.recent, self.sparkline_bounds(width, height, cell_pixels))
            .into_iter()
            .map(|(x, y)| (x.round() as u32, y.round() as u32))
            .collect()
    }
}

impl BenchTrendSpec {
    pub fn plot_bounds(&self, width: u32, height: u32, cell_pixels: CellPixels) -> PlotBounds {
        let label_height = (u32::from(cell_pixels.height) * 4 / 5).max(9);
        PlotBounds {
            left: (u32::from(cell_pixels.width) * 4).min(width / 3),
            top: label_height.saturating_add(2).min(height / 3),
            right: width.saturating_sub(8),
            bottom: height.saturating_sub(label_height.saturating_add(7)),
        }
    }

    pub fn point_x(&self, width: u32, index: usize) -> f32 {
        let cell_width = (width / u32::from(self.cells.width.max(1))).max(1) as u16;
        let plot = self.plot_bounds(width, 100, CellPixels::new(cell_width, 20));
        if self.points.len() < 2 {
            return (plot.left + plot.right) as f32 / 2.0;
        }
        plot.left as f32 + index as f32 / (self.points.len() - 1) as f32 * plot.right.saturating_sub(plot.left) as f32
    }

    pub fn wpm_y(&self, height: u32, value: f64) -> f32 {
        let cell_height = (height / u32::from(self.cells.height.max(1))).max(1) as u16;
        let plot = self.plot_bounds(100, height, CellPixels::new(9, cell_height));
        scaled_y(self.points.iter().map(|point| point.wpm), value, plot.top as f32, plot.bottom as f32)
    }

    fn accuracy_y(&self, height: u32, value: f64) -> f32 {
        let cell_height = (height / u32::from(self.cells.height.max(1))).max(1) as u16;
        let plot = self.plot_bounds(100, height, CellPixels::new(9, cell_height));
        scaled_y(self.points.iter().map(|point| point.accuracy), value, plot.top as f32, plot.bottom as f32)
    }
}

impl SpeedChartSpec {
    pub fn average_wpm(&self) -> f64 {
        let (sum, count) = self
            .points
            .iter()
            .map(|point| point.wpm)
            .filter(|value| value.is_finite())
            .fold((0.0, 0_u32), |(sum, count), value| (sum + value, count + 1));
        if count == 0 { 0.0 } else { sum / f64::from(count) }
    }

    fn value_range(&self) -> Option<(f64, f64)> {
        let mut values = self.points.iter().map(|point| point.wpm).filter(|value| value.is_finite());
        let first = values.next()?;
        let (mut minimum, mut maximum) = values.fold((first, first), |(minimum, maximum), value| (minimum.min(value), maximum.max(value)));
        if (maximum - minimum).abs() < f64::EPSILON {
            minimum -= 1.0;
            maximum += 1.0;
        }
        Some((minimum, maximum))
    }

    pub fn plot_bounds(&self, width: u32, height: u32, cell_pixels: CellPixels) -> PlotBounds {
        let label_height = (u32::from(cell_pixels.height) * 4 / 5).max(9);
        PlotBounds {
            left: (u32::from(cell_pixels.width) * 5).max(24),
            top: label_height / 2 + 3,
            right: width.saturating_sub((u32::from(cell_pixels.width) * 10).max(48)),
            bottom: height.saturating_sub(label_height + 6),
        }
    }

    pub fn value_y(&self, plot: PlotBounds, value: f64) -> f32 {
        let Some((minimum, maximum)) = self.value_range() else {
            return plot.bottom as f32;
        };
        (f64::from(plot.bottom) - (value - minimum) / (maximum - minimum) * f64::from(plot.bottom.saturating_sub(plot.top))) as f32
    }
}

fn colors_key(colors: ResultImageColors) -> [[u8; 3]; 7] {
    [
        color_rgb(colors.foreground, [255, 255, 255]),
        color_rgb(colors.muted, [130, 140, 155]),
        color_rgb(colors.correct, [144, 238, 144]),
        color_rgb(colors.mistype, [241, 76, 76]),
        color_rgb(colors.accent, [41, 184, 219]),
        color_rgb(colors.warning, [245, 245, 67]),
        color_rgb(colors.panel, [16, 20, 28]),
    ]
}

fn metric_card_key(font: &ResolvedBigFont, spec: &MetricCardSpec) -> ResultImageKey {
    ResultImageKey::MetricCard {
        font: font.identity(),
        label: spec.label.clone(),
        value: spec.value.clone(),
        previous_delta: spec.previous_delta.map(f64::to_bits),
        best_delta: spec.best_delta.map(f64::to_bits),
        recent: spec.recent.iter().map(|value| value.to_bits()).collect(),
        japanese: spec.lang == Lang::Ja,
        delta_suffix: spec.delta_suffix.clone(),
        cells: spec.cells,
        colors: colors_key(spec.colors),
    }
}

fn bench_part_card_key(font: &ResolvedBigFont, spec: &BenchPartCardSpec) -> ResultImageKey {
    ResultImageKey::BenchPartCard {
        font: font.identity(),
        label: spec.label.clone(),
        wpm: spec.wpm.to_bits(),
        accuracy: spec.accuracy.to_bits(),
        previous_wpm_delta: spec.previous_wpm_delta.map(f64::to_bits),
        best_wpm_delta: spec.best_wpm_delta.map(f64::to_bits),
        previous_accuracy_delta: spec.previous_accuracy_delta.map(f64::to_bits),
        best_accuracy_delta: spec.best_accuracy_delta.map(f64::to_bits),
        version: spec.version.clone(),
        japanese: spec.lang == Lang::Ja,
        cells: spec.cells,
        colors: colors_key(spec.colors),
    }
}

fn bench_trend_key(font: &ResolvedBigFont, spec: &BenchTrendSpec) -> ResultImageKey {
    ResultImageKey::BenchTrend {
        font: font.identity(),
        points: spec
            .points
            .iter()
            .map(|point| {
                (
                    point.date,
                    point.wpm.to_bits(),
                    point.accuracy.to_bits(),
                    point.starts_new_series,
                    point.current,
                )
            })
            .collect(),
        cells: spec.cells,
        colors: colors_key(spec.colors),
    }
}

fn speed_chart_key(font: &ResolvedBigFont, spec: &SpeedChartSpec) -> ResultImageKey {
    ResultImageKey::SpeedChart {
        font: font.identity(),
        points: spec.points.iter().map(|point| (point.wpm.to_bits(), point.mistypes)).collect(),
        japanese: spec.lang == Lang::Ja,
        cells: spec.cells,
        colors: colors_key(spec.colors),
    }
}

fn mistype_keyboard_key(spec: &MistypeKeyboardSpec) -> ResultImageKey {
    ResultImageKey::MistypeKeyboard {
        placements: spec
            .placements
            .iter()
            .map(|placement| PlacementKey {
                x: placement.x,
                y: placement.y,
                cells: placement.spec.cells,
                label: placement.spec.label.clone(),
                face: color_rgb(placement.spec.face_color, [75, 81, 91]),
                text: color_rgb(placement.spec.text_color, [255, 255, 255]),
                bold: placement.spec.bold,
                pressed: placement.spec.pressed,
                modifier: placement.spec.modifier,
                finger: placement.spec.finger_color.map(|color| color_rgb(color, [128, 128, 128])),
            })
            .collect(),
        cells: spec.cells,
    }
}

fn reaction_histogram_key(font: &ResolvedBigFont, spec: &ReactionHistogramSpec) -> ResultImageKey {
    ResultImageKey::ReactionHistogram {
        font: font.identity(),
        reaction_seconds: spec.reaction_seconds.iter().map(|value| value.to_bits()).collect(),
        p50_seconds: spec.p50_seconds.to_bits(),
        p90_seconds: spec.p90_seconds.to_bits(),
        cells: spec.cells,
        colors: colors_key(spec.colors),
    }
}

pub fn render_metric_card(font: &ResolvedBigFont, cell_pixels: CellPixels, spec: &MetricCardSpec) -> Result<RgbaImage, String> {
    let (width, height) = pixel_size(spec.cells, cell_pixels);
    let mut pixmap = new_pixmap(width, height, "metric card")?;
    let panel = color_rgb(spec.colors.panel, [16, 20, 28]);
    let muted = color_rgb(spec.colors.muted, [130, 140, 155]);
    let accent = color_rgb(spec.colors.accent, [41, 184, 219]);
    fill_round_rect(&mut pixmap, [1.0, 1.0, width as f32 - 1.0, height as f32 - 1.0], 8.0, panel, 245);
    stroke_round_rect(&mut pixmap, [1.5, 1.5, width as f32 - 1.5, height as f32 - 1.5], 8.0, muted, 180, 1.5);

    let mut glyphs = HashMap::new();
    let label_height = metric_card_label_height(cell_pixels);
    // The headline (WPM / CPS / accuracy) in the foreground colour, larger than
    // the axis labels: the muted 0.8-cell label was hard to read.
    let label = color_rgb(spec.colors.foreground, [235, 238, 242]);
    draw_text(
        &mut pixmap,
        font,
        &spec.label,
        12.0,
        6.0,
        label_height,
        label,
        true,
        TextAlign::Left,
        cell_pixels,
        &mut glyphs,
    )?;

    let (value_center, value_top, value_height) = spec.value_layout(width, height, cell_pixels);
    draw_text(
        &mut pixmap,
        font,
        &spec.value,
        value_center,
        value_top,
        value_height,
        accent,
        true,
        TextAlign::Center,
        cell_pixels,
        &mut glyphs,
    )?;

    if spec.recent.len() >= 2 {
        draw_sparkline(&mut pixmap, &spec.recent, spec.sparkline_bounds(width, height, cell_pixels), accent);
    }

    let delta_y = height as f32 * 0.76;
    let delta_size = (value_height / 3).max(9);
    if let Some(delta) = spec.previous_delta.filter(|value| value.is_finite()) {
        draw_delta(
            &mut pixmap,
            font,
            spec.lang,
            MetricDeltaKind::Previous,
            delta,
            &spec.delta_suffix,
            10.0,
            delta_y,
            delta_size,
            cell_pixels,
            spec.colors,
            &mut glyphs,
        )?;
    }
    if let Some(delta) = spec.best_delta.filter(|value| value.is_finite()) {
        draw_delta(
            &mut pixmap,
            font,
            spec.lang,
            MetricDeltaKind::Best,
            delta,
            &spec.delta_suffix,
            width as f32 * 0.52,
            delta_y,
            delta_size,
            cell_pixels,
            spec.colors,
            &mut glyphs,
        )?;
    }
    rgba_image(pixmap)
}

pub fn render_bench_part_card(font: &ResolvedBigFont, cell_pixels: CellPixels, spec: &BenchPartCardSpec) -> Result<RgbaImage, String> {
    let (width, height) = pixel_size(spec.cells, cell_pixels);
    let mut pixmap = new_pixmap(width, height, "benchmark part card")?;
    let panel = color_rgb(spec.colors.panel, [16, 20, 28]);
    let muted = color_rgb(spec.colors.muted, [130, 140, 155]);
    let accent = color_rgb(spec.colors.accent, [41, 184, 219]);
    let foreground = color_rgb(spec.colors.foreground, [255, 255, 255]);
    fill_round_rect(&mut pixmap, [1.0, 1.0, width as f32 - 1.0, height as f32 - 1.0], 8.0, panel, 245);
    stroke_round_rect(&mut pixmap, [1.5, 1.5, width as f32 - 1.5, height as f32 - 1.5], 8.0, muted, 180, 1.5);
    let mut glyphs = HashMap::new();
    let unit = f32::from(cell_pixels.height);
    let small = (u32::from(cell_pixels.height) * 4 / 5).max(9);
    draw_text(
        &mut pixmap,
        font,
        &spec.label,
        9.0,
        unit * 0.5,
        small,
        foreground,
        true,
        TextAlign::Left,
        cell_pixels,
        &mut glyphs,
    )?;
    draw_text(
        &mut pixmap,
        font,
        &spec.version,
        width as f32 - 9.0,
        unit * 0.5,
        small,
        muted,
        false,
        TextAlign::Right,
        cell_pixels,
        &mut glyphs,
    )?;

    let value_height = (u32::from(cell_pixels.height) * 2).max(28);
    let value = format!("{:.1}", spec.wpm);
    let value_center = width as f32 * 0.34;
    draw_text(
        &mut pixmap,
        font,
        &value,
        value_center,
        unit * 1.6,
        value_height,
        accent,
        true,
        TextAlign::Center,
        cell_pixels,
        &mut glyphs,
    )?;
    let value_cell_width = (u32::from(cell_pixels.width) * value_height / u32::from(cell_pixels.height).max(1)).max(1);
    let value_width = value
        .chars()
        .map(|character| value_cell_width * UnicodeWidthChar::width(character).unwrap_or(1).max(1) as u32)
        .sum::<u32>();
    draw_text(
        &mut pixmap,
        font,
        "WPM",
        value_center + value_width as f32 / 2.0 + 4.0,
        unit * 1.6 + value_height as f32 - small as f32,
        small,
        foreground,
        true,
        TextAlign::Left,
        cell_pixels,
        &mut glyphs,
    )?;
    draw_text(
        &mut pixmap,
        font,
        &match spec.lang {
            Lang::En => format!("Accuracy {:.1}%", spec.accuracy * 100.0),
            Lang::Ja => format!("正確さ {:.1}%", spec.accuracy * 100.0),
        },
        9.0,
        unit * 3.8,
        small,
        foreground,
        false,
        TextAlign::Left,
        cell_pixels,
        &mut glyphs,
    )?;

    let delta_size = small;
    let rows = [
        (spec.previous_wpm_delta, MetricDeltaKind::Previous, unit * 5.1),
        (spec.best_wpm_delta, MetricDeltaKind::Best, unit * 6.35),
    ];
    for (delta, kind, y) in rows {
        if let Some(delta) = delta.filter(|value| value.is_finite()) {
            draw_delta(
                &mut pixmap,
                font,
                spec.lang,
                kind,
                delta,
                " WPM",
                9.0,
                y,
                delta_size,
                cell_pixels,
                spec.colors,
                &mut glyphs,
            )?;
            let accuracy_delta = match kind {
                MetricDeltaKind::Previous => spec.previous_accuracy_delta,
                MetricDeltaKind::Best => spec.best_accuracy_delta,
            };
            if let Some(accuracy_delta) = accuracy_delta.filter(|value| value.is_finite()) {
                let color = if accuracy_delta > 0.0 {
                    color_rgb(spec.colors.correct, [144, 238, 144])
                } else if accuracy_delta < 0.0 {
                    color_rgb(spec.colors.mistype, [241, 76, 76])
                } else {
                    muted
                };
                let text = if accuracy_delta == 0.0 {
                    "±0pt".to_owned()
                } else {
                    format!("{:+.1}pt", accuracy_delta * 100.0)
                };
                draw_text(
                    &mut pixmap,
                    font,
                    &text,
                    width as f32 * 0.69,
                    y - 2.0,
                    delta_size,
                    color,
                    false,
                    TextAlign::Left,
                    cell_pixels,
                    &mut glyphs,
                )?;
            }
        } else if kind == MetricDeltaKind::Previous {
            let initial = match spec.lang {
                Lang::En => "Initial",
                Lang::Ja => "初回",
            };
            draw_text(
                &mut pixmap,
                font,
                initial,
                9.0,
                y - 2.0,
                delta_size,
                foreground,
                true,
                TextAlign::Left,
                cell_pixels,
                &mut glyphs,
            )?;
        }
    }
    rgba_image(pixmap)
}

fn trend_wpm_range(spec: &BenchTrendSpec) -> Option<(f64, f64)> {
    let minimum = spec
        .points
        .iter()
        .map(|point| point.wpm)
        .filter(|value| value.is_finite())
        .min_by(f64::total_cmp)?;
    let maximum = spec
        .points
        .iter()
        .map(|point| point.wpm)
        .filter(|value| value.is_finite())
        .max_by(f64::total_cmp)?;
    if (maximum - minimum).abs() < f64::EPSILON {
        Some((minimum - 1.0, maximum + 1.0))
    } else {
        Some((minimum, maximum))
    }
}

fn short_date(date: chrono::NaiveDate) -> String {
    use chrono::Datelike;

    format!("{}/{}", date.month(), date.day())
}

pub fn render_bench_trend(font: &ResolvedBigFont, cell_pixels: CellPixels, spec: &BenchTrendSpec) -> Result<RgbaImage, String> {
    let (width, height) = pixel_size(spec.cells, cell_pixels);
    let mut pixmap = new_pixmap(width, height, "benchmark trend")?;
    if spec.points.is_empty() {
        return rgba_image(pixmap);
    }
    let plot = spec.plot_bounds(width, height, cell_pixels);
    let left = plot.left as f32;
    let top = plot.top as f32;
    let right = plot.right as f32;
    let bottom = plot.bottom as f32;
    let grid = color_rgb(spec.colors.muted, [130, 140, 155]);
    let wpm = color_rgb(spec.colors.accent, [41, 184, 219]);
    let accuracy = color_rgb(spec.colors.correct, [144, 238, 144]);
    let warning = color_rgb(spec.colors.warning, [245, 245, 67]);
    let label_height = (u32::from(cell_pixels.height) * 4 / 5).max(9);
    let mut glyphs = HashMap::new();
    if let Some((minimum, maximum)) = trend_wpm_range(spec) {
        for step in 0..2 {
            let fraction = f64::from(step) / 2.0;
            let value = maximum - fraction * (maximum - minimum);
            let y = top + (bottom - top) * step as f32 / 2.0;
            stroke_line(&mut pixmap, left, y, right, y, grid, 55, 1.0);
            draw_text(
                &mut pixmap,
                font,
                &format!("{value:.0}"),
                left - 6.0,
                (y - label_height as f32 / 2.0).clamp(0.0, height.saturating_sub(label_height) as f32),
                label_height,
                grid,
                false,
                TextAlign::Right,
                cell_pixels,
                &mut glyphs,
            )?;
        }
    }
    let first = spec.points.first().expect("non-empty trend");
    let last = spec.points.last().expect("non-empty trend");
    draw_text(
        &mut pixmap,
        font,
        &short_date(first.date),
        left,
        bottom + 5.0,
        label_height,
        grid,
        false,
        TextAlign::Left,
        cell_pixels,
        &mut glyphs,
    )?;
    if last.date != first.date {
        draw_text(
            &mut pixmap,
            font,
            &short_date(last.date),
            right,
            bottom + 5.0,
            label_height,
            grid,
            false,
            TextAlign::Right,
            cell_pixels,
            &mut glyphs,
        )?;
    }
    for (index, _) in spec.points.iter().enumerate().filter(|(_, point)| point.starts_new_series) {
        let x = spec.point_x(width, index).round();
        let mut y = top;
        while y < bottom {
            stroke_line(&mut pixmap, x, y, x, (y + 5.0).min(bottom), warning, 255, 3.0);
            y += 9.0;
        }
        draw_text(
            &mut pixmap,
            font,
            "v2",
            x - 3.0,
            1.0,
            label_height,
            warning,
            true,
            TextAlign::Right,
            cell_pixels,
            &mut glyphs,
        )?;
    }
    for index in 1..spec.points.len() {
        if spec.points[index].starts_new_series {
            continue;
        }
        let previous = spec.points[index - 1];
        let current = spec.points[index];
        stroke_line(
            &mut pixmap,
            spec.point_x(width, index - 1),
            spec.wpm_y(height, previous.wpm),
            spec.point_x(width, index),
            spec.wpm_y(height, current.wpm),
            wpm,
            255,
            2.5,
        );
        stroke_line(
            &mut pixmap,
            spec.point_x(width, index - 1),
            spec.accuracy_y(height, previous.accuracy),
            spec.point_x(width, index),
            spec.accuracy_y(height, current.accuracy),
            accuracy,
            185,
            2.0,
        );
    }
    for (index, point) in spec.points.iter().enumerate() {
        let radius = if point.current { 8.0 } else { 3.0 };
        fill_circle(&mut pixmap, spec.point_x(width, index), spec.wpm_y(height, point.wpm), radius, wpm);
        fill_circle(
            &mut pixmap,
            spec.point_x(width, index),
            spec.accuracy_y(height, point.accuracy),
            if point.current { 6.5 } else { 2.5 },
            accuracy,
        );
    }
    rgba_image(pixmap)
}

pub fn render_speed_chart(font: &ResolvedBigFont, cell_pixels: CellPixels, spec: &SpeedChartSpec) -> Result<RgbaImage, String> {
    let (width, height) = pixel_size(spec.cells, cell_pixels);
    let mut pixmap = new_pixmap(width, height, "speed chart")?;
    if spec.points.len() < 2 {
        return rgba_image(pixmap);
    }
    let plot = spec.plot_bounds(width, height, cell_pixels);
    let Some((minimum, maximum)) = spec.value_range() else {
        return rgba_image(pixmap);
    };
    let point = |index: usize, value: f64| {
        let x = f64::from(plot.left) + index as f64 / (spec.points.len() - 1) as f64 * f64::from(plot.right.saturating_sub(plot.left));
        (x as f32, spec.value_y(plot, value))
    };
    let grid = color_rgb(spec.colors.muted, [130, 140, 155]);
    let accent = color_rgb(spec.colors.accent, [41, 184, 219]);
    let mistype = color_rgb(spec.colors.mistype, [241, 76, 76]);
    let mut glyphs = HashMap::new();
    let label_height = (u32::from(cell_pixels.height) * 4 / 5).max(9);
    for step in 0..=2 {
        let value = maximum - f64::from(step) / 2.0 * (maximum - minimum);
        let y = spec.value_y(plot, value);
        stroke_line(&mut pixmap, plot.left as f32, y, plot.right as f32, y, grid, 65, 1.0);
        draw_text(
            &mut pixmap,
            font,
            &format!("{value:.0}"),
            plot.left.saturating_sub(10) as f32,
            (y - label_height as f32 / 2.0).clamp(0.0, height.saturating_sub(label_height) as f32),
            label_height,
            grid,
            false,
            TextAlign::Right,
            cell_pixels,
            &mut glyphs,
        )?;
    }
    let average_wpm = spec.average_wpm();
    let (_, mean_y) = point(0, average_wpm);
    stroke_line(&mut pixmap, plot.left as f32, mean_y, plot.right as f32, mean_y, grid, 230, 2.0);
    draw_text(
        &mut pixmap,
        font,
        &match spec.lang {
            Lang::En => format!("avg {average_wpm:.0}"),
            Lang::Ja => format!("平均 {average_wpm:.0}"),
        },
        plot.right as f32 + 5.0,
        (mean_y - label_height as f32 / 2.0).clamp(0.0, height.saturating_sub(label_height) as f32),
        label_height,
        grid,
        true,
        TextAlign::Left,
        cell_pixels,
        &mut glyphs,
    )?;
    let mut path = PathBuilder::new();
    for (index, item) in spec.points.iter().enumerate() {
        let (x, y) = point(index, item.wpm);
        if index == 0 {
            path.move_to(x, y);
        } else {
            path.line_to(x, y);
        }
    }
    if let Some(path) = path.finish() {
        let mut paint = Paint::default();
        paint.set_color_rgba8(accent[0], accent[1], accent[2], 255);
        let stroke = Stroke {
            width: 3.0,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
    for (index, item) in spec.points.iter().enumerate() {
        let (x, y) = point(index, item.wpm);
        fill_circle(
            &mut pixmap,
            x,
            y,
            if item.mistypes.unwrap_or(0) > 0 { 5.0 } else { 3.0 },
            if item.mistypes.unwrap_or(0) > 0 { mistype } else { accent },
        );
    }
    let mut tick_indexes = vec![0, spec.points.len() / 2, spec.points.len() - 1];
    tick_indexes.dedup();
    for index in tick_indexes {
        let (x, _) = point(index, minimum);
        draw_text(
            &mut pixmap,
            font,
            &(index + 1).to_string(),
            x,
            plot.bottom as f32 + 4.0,
            label_height,
            grid,
            false,
            TextAlign::Center,
            cell_pixels,
            &mut glyphs,
        )?;
    }
    rgba_image(pixmap)
}

pub fn render_mistype_keyboard(cell_pixels: CellPixels, spec: &MistypeKeyboardSpec) -> Result<RgbaImage, String> {
    let (width, height) = pixel_size(spec.cells, cell_pixels);
    let mut image = RgbaImage::new(width.max(1), height.max(1));
    for placement in &spec.placements {
        let tile = render_key_tile(cell_pixels, &placement.spec);
        image::imageops::overlay(
            &mut image,
            &tile,
            i64::from(placement.x) * i64::from(cell_pixels.width),
            i64::from(placement.y) * i64::from(cell_pixels.height),
        );
    }
    Ok(image)
}

pub fn render_reaction_histogram(font: &ResolvedBigFont, cell_pixels: CellPixels, spec: &ReactionHistogramSpec) -> Result<RgbaImage, String> {
    let (width, height) = pixel_size(spec.cells, cell_pixels);
    let mut pixmap = new_pixmap(width, height, "reaction histogram")?;
    let plot = spec.plot_bounds(width, height);
    let mut buckets = [0_u32; 15];
    for value in spec.reaction_seconds.iter().copied().filter(|value| value.is_finite() && *value >= 0.0) {
        let index = ((value / 0.1).floor() as usize).min(buckets.len() - 1);
        buckets[index] += 1;
    }
    let maximum = buckets.iter().copied().max().unwrap_or(1).max(1);
    let accent = color_rgb(spec.colors.accent, [41, 184, 219]);
    let muted = color_rgb(spec.colors.muted, [130, 140, 155]);
    let correct = color_rgb(spec.colors.correct, [144, 238, 144]);
    let warning = color_rgb(spec.colors.warning, [245, 245, 67]);
    let plot_width = plot.right.saturating_sub(plot.left) as f32;
    let slot = plot_width / buckets.len() as f32;
    for (index, count) in buckets.into_iter().enumerate() {
        let bar_height = count as f32 / maximum as f32 * plot.bottom.saturating_sub(plot.top) as f32;
        let left = plot.left as f32 + index as f32 * slot + 1.0;
        let right = plot.left as f32 + (index + 1) as f32 * slot - 1.0;
        fill_rect(
            &mut pixmap,
            [left, plot.bottom as f32 - bar_height, right.max(left + 1.0), plot.bottom as f32],
            accent,
            210,
        );
    }
    stroke_line(
        &mut pixmap,
        plot.left as f32,
        plot.bottom as f32,
        plot.right as f32,
        plot.bottom as f32,
        muted,
        180,
        1.0,
    );
    let p50_x = spec.marker_x(plot, spec.p50_seconds);
    let p90_x = spec.marker_x(plot, spec.p90_seconds);
    stroke_line(
        &mut pixmap,
        p50_x as f32,
        plot.top as f32,
        p50_x as f32,
        plot.bottom as f32,
        correct,
        255,
        2.0,
    );
    stroke_line(
        &mut pixmap,
        p90_x as f32,
        plot.top as f32,
        p90_x as f32,
        plot.bottom as f32,
        warning,
        255,
        2.0,
    );

    let mut glyphs = HashMap::new();
    let axis_label_height = (u32::from(cell_pixels.height) * 4 / 5).max(9);
    for (seconds, label, align) in [
        (0.0, "0", TextAlign::Left),
        (0.5, "0.5", TextAlign::Center),
        (1.0, "1.0", TextAlign::Center),
        (1.5, "≥1.5 s", TextAlign::Right),
    ] {
        let x = spec.marker_x(plot, seconds);
        stroke_line(
            &mut pixmap,
            x as f32,
            plot.bottom as f32,
            x as f32,
            plot.bottom as f32 + 3.0,
            muted,
            210,
            1.0,
        );
        draw_text(
            &mut pixmap,
            font,
            label,
            x as f32,
            plot.bottom as f32 + 4.0,
            axis_label_height,
            muted,
            false,
            align,
            cell_pixels,
            &mut glyphs,
        )?;
    }
    let label_height = (u32::from(cell_pixels.height) * 4 / 5).max(9);
    draw_text(
        &mut pixmap,
        font,
        &format!("p50 {:.2}s", spec.p50_seconds),
        p50_x as f32,
        2.0,
        label_height,
        correct,
        false,
        TextAlign::Center,
        cell_pixels,
        &mut glyphs,
    )?;
    draw_text(
        &mut pixmap,
        font,
        &format!("p90 {:.2}s", spec.p90_seconds),
        p90_x as f32,
        (label_height + 4) as f32,
        label_height,
        warning,
        false,
        TextAlign::Center,
        cell_pixels,
        &mut glyphs,
    )?;
    rgba_image(pixmap)
}

fn pixel_size(cells: Size, cell_pixels: CellPixels) -> (u32, u32) {
    (
        u32::from(cells.width).saturating_mul(u32::from(cell_pixels.width)).max(1),
        u32::from(cells.height).saturating_mul(u32::from(cell_pixels.height)).max(1),
    )
}

fn new_pixmap(width: u32, height: u32, name: &str) -> Result<Pixmap, String> {
    Pixmap::new(width.max(1), height.max(1)).ok_or_else(|| format!("{name} dimensions are too large"))
}

fn rgba_image(pixmap: Pixmap) -> Result<RgbaImage, String> {
    let width = pixmap.width();
    let height = pixmap.height();
    RgbaImage::from_raw(width, height, pixmap.take_demultiplied()).ok_or_else(|| "pixmap dimensions do not match its data".to_owned())
}

fn fill_rect(pixmap: &mut Pixmap, bounds: [f32; 4], color: [u8; 3], alpha: u8) {
    let [left, top, right, bottom] = bounds;
    let Some(rect) = tiny_skia::Rect::from_ltrb(left, top, right, bottom) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], alpha);
    pixmap.fill_rect(rect, &paint, Transform::identity(), None);
}

fn round_rect_path(bounds: [f32; 4], radius: f32) -> Option<tiny_skia::Path> {
    let [left, top, right, bottom] = bounds;
    let radius = radius.max(0.0).min((right - left) / 2.0).min((bottom - top) / 2.0);
    let k = 0.552_284_8 * radius;
    let mut path = PathBuilder::new();
    path.move_to(left + radius, top);
    path.line_to(right - radius, top);
    path.cubic_to(right - radius + k, top, right, top + radius - k, right, top + radius);
    path.line_to(right, bottom - radius);
    path.cubic_to(right, bottom - radius + k, right - radius + k, bottom, right - radius, bottom);
    path.line_to(left + radius, bottom);
    path.cubic_to(left + radius - k, bottom, left, bottom - radius + k, left, bottom - radius);
    path.line_to(left, top + radius);
    path.cubic_to(left, top + radius - k, left + radius - k, top, left + radius, top);
    path.close();
    path.finish()
}

fn fill_round_rect(pixmap: &mut Pixmap, bounds: [f32; 4], radius: f32, color: [u8; 3], alpha: u8) {
    let Some(path) = round_rect_path(bounds, radius) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], alpha);
    pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
}

fn stroke_round_rect(pixmap: &mut Pixmap, bounds: [f32; 4], radius: f32, color: [u8; 3], alpha: u8, width: f32) {
    let Some(path) = round_rect_path(bounds, radius) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], alpha);
    pixmap.stroke_path(&path, &paint, &Stroke { width, ..Stroke::default() }, Transform::identity(), None);
}

#[allow(clippy::too_many_arguments)]
fn stroke_line(pixmap: &mut Pixmap, x1: f32, y1: f32, x2: f32, y2: f32, color: [u8; 3], alpha: u8, width: f32) {
    let mut path = PathBuilder::new();
    path.move_to(x1, y1);
    path.line_to(x2, y2);
    let Some(path) = path.finish() else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], alpha);
    pixmap.stroke_path(
        &path,
        &paint,
        &Stroke {
            width,
            line_cap: LineCap::Round,
            ..Stroke::default()
        },
        Transform::identity(),
        None,
    );
}

fn fill_circle(pixmap: &mut Pixmap, x: f32, y: f32, radius: f32, color: [u8; 3]) {
    let Some(path) = PathBuilder::from_circle(x, y, radius) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], 255);
    pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
}

fn normalized_sparkline_points(values: &[f64], bounds: [f32; 4]) -> Vec<(f32, f32)> {
    let finite = values.iter().copied().filter(|value| value.is_finite()).collect::<Vec<_>>();
    if finite.len() < 2 {
        return Vec::new();
    }
    let minimum = finite.iter().copied().fold(f64::INFINITY, f64::min);
    let maximum = finite.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let span = (maximum - minimum).max(f64::EPSILON);
    let [left, top, right, bottom] = bounds;
    finite
        .iter()
        .copied()
        .enumerate()
        .map(|(index, value)| {
            let x = left + index as f32 / (finite.len() - 1) as f32 * (right - left);
            let y = bottom - ((value - minimum) / span) as f32 * (bottom - top);
            (x, y)
        })
        .collect()
}

fn scaled_y(values: impl Iterator<Item = f64>, value: f64, top: f32, bottom: f32) -> f32 {
    let finite = values.filter(|value| value.is_finite()).collect::<Vec<_>>();
    let minimum = finite.iter().copied().fold(f64::INFINITY, f64::min);
    let maximum = finite.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if !minimum.is_finite() || !maximum.is_finite() || (maximum - minimum).abs() < f64::EPSILON {
        return (top + bottom) / 2.0;
    }
    bottom - ((value - minimum) / (maximum - minimum)).clamp(0.0, 1.0) as f32 * (bottom - top)
}

fn draw_sparkline(pixmap: &mut Pixmap, values: &[f64], bounds: [f32; 4], color: [u8; 3]) {
    let points = normalized_sparkline_points(values, bounds);
    if points.len() < 2 {
        return;
    }
    let mut path = PathBuilder::new();
    for (index, (x, y)) in points.iter().copied().enumerate() {
        if index == 0 {
            path.move_to(x, y);
        } else {
            path.line_to(x, y);
        }
    }
    if let Some(path) = path.finish() {
        let mut paint = Paint::default();
        paint.set_color_rgba8(color[0], color[1], color[2], 220);
        pixmap.stroke_path(
            &path,
            &paint,
            &Stroke {
                width: 2.0,
                line_cap: LineCap::Round,
                line_join: LineJoin::Round,
                ..Stroke::default()
            },
            Transform::identity(),
            None,
        );
    }
    for (index, (x, y)) in points.iter().copied().enumerate() {
        fill_circle(pixmap, x, y, if index + 1 == points.len() { 5.0 } else { 3.0 }, color);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_delta(
    pixmap: &mut Pixmap,
    font: &ResolvedBigFont,
    lang: Lang,
    kind: MetricDeltaKind,
    delta: f64,
    suffix: &str,
    x: f32,
    y: f32,
    size: u32,
    cell_pixels: CellPixels,
    colors: ResultImageColors,
    glyphs: &mut HashMap<SmallGlyphKey, Pixmap>,
) -> Result<(), String> {
    let good = delta > 0.0;
    let color = color_rgb(
        if delta == 0.0 {
            colors.muted
        } else if good {
            colors.correct
        } else {
            colors.mistype
        },
        [255, 255, 255],
    );
    let text_y = y - 2.0;
    let icon = size as f32 * 0.8;
    let icon_y = text_y + (size as f32 - icon) / 2.0;
    if delta != 0.0 {
        let mut triangle = PathBuilder::new();
        if good {
            triangle.move_to(x, icon_y + icon);
            triangle.line_to(x + icon, icon_y + icon);
            triangle.line_to(x + icon / 2.0, icon_y);
        } else {
            triangle.move_to(x, icon_y);
            triangle.line_to(x + icon, icon_y);
            triangle.line_to(x + icon / 2.0, icon_y + icon);
        }
        triangle.close();
        if let Some(path) = triangle.finish() {
            let mut paint = Paint::default();
            paint.set_color_rgba8(color[0], color[1], color[2], 255);
            pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
        }
    }
    let text_x = if delta == 0.0 { x } else { x + icon + 4.0 };
    draw_text(
        pixmap,
        font,
        &format_metric_delta(lang, kind, delta, suffix),
        text_x,
        text_y,
        size,
        color,
        kind == MetricDeltaKind::Best && delta > 0.0,
        TextAlign::Left,
        cell_pixels,
        glyphs,
    )
}

#[derive(Clone, Copy)]
enum TextAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct SmallGlyphKey {
    character: char,
    width: u32,
    height: u32,
    color: [u8; 3],
    bold: bool,
}

#[allow(clippy::too_many_arguments)]
fn draw_text(
    pixmap: &mut Pixmap,
    font: &ResolvedBigFont,
    text: &str,
    anchor_x: f32,
    y: f32,
    glyph_height: u32,
    color: [u8; 3],
    bold: bool,
    align: TextAlign,
    cell_pixels: CellPixels,
    glyphs: &mut HashMap<SmallGlyphKey, Pixmap>,
) -> Result<(), String> {
    let glyph_height = glyph_height.max(1);
    let cell_width = (u32::from(cell_pixels.width) * glyph_height / u32::from(cell_pixels.height).max(1)).max(1);
    let widths = text
        .chars()
        .map(|character| cell_width * UnicodeWidthChar::width(character).unwrap_or(1).max(1) as u32)
        .collect::<Vec<_>>();
    let total_width = widths.iter().sum::<u32>();
    let mut x = match align {
        TextAlign::Left => anchor_x,
        TextAlign::Center => anchor_x - total_width as f32 / 2.0,
        TextAlign::Right => anchor_x - total_width as f32,
    };
    for (character, glyph_width) in text.chars().zip(widths) {
        if !character.is_whitespace() {
            let key = SmallGlyphKey {
                character,
                width: glyph_width.max(1),
                height: glyph_height,
                color,
                bold,
            };
            if let std::collections::hash_map::Entry::Vacant(entry) = glyphs.entry(key) {
                let mut glyph = new_pixmap(key.width, key.height, "label glyph")?;
                let rendered = draw_big_character(&mut glyph, font, BigTextSize::Dot12, character, color, bold)?;
                if !rendered && character == '-' {
                    let margin = (key.width / 5).max(1) as f32;
                    let thickness = (key.height / 10).max(2) as f32;
                    let center_y = key.height as f32 / 2.0;
                    fill_rect(
                        &mut glyph,
                        [margin, center_y - thickness / 2.0, key.width as f32 - margin, center_y + thickness / 2.0],
                        color,
                        255,
                    );
                }
                entry.insert(glyph);
            }
            let glyph = glyphs.get(&key).expect("inserted glyph");
            pixmap.draw_pixmap(
                x.round() as i32,
                y.round() as i32,
                glyph.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
        x += glyph_width as f32;
    }
    Ok(())
}
