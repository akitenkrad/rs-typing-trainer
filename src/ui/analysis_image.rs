//! Pixel rendering and fixed-protocol caching for analysis badges and charts.

use chrono::{DateTime, NaiveDate};
use image::RgbaImage;
use ratatui::layout::Size;
use ratatui::style::Color;
use ratatui_image::protocol::Protocol;
use std::collections::{BTreeMap, BTreeSet};
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke, StrokeDash, Transform};

use crate::analysis::grades::Level;
use crate::graphics::{CellPixels, GraphicsDetection};
use crate::i18n::Lang;
use crate::record::SessionRecord;
use crate::ui::bigtext::BigTextSize;
use crate::ui::bigtext_image::{FontIdentity, ResolvedBigFont, draw_big_character};
use crate::ui::image_cache::{DEFAULT_TILE_CACHE_LIMIT, ProtocolCache, color_rgb};

pub const SERIES_COLORS: [Color; 6] = [Color::Cyan, Color::Green, Color::Yellow, Color::Magenta, Color::Blue, Color::Red];
pub const FORECAST_EN_COLOR: Color = Color::Rgb(105, 145, 190);
pub const FORECAST_JA_COLOR: Color = Color::Rgb(220, 140, 75);

const BADGE_FACE: [u8; 3] = [16, 20, 28];
const GRID_COLOR: [u8; 3] = [130, 140, 155];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MetricKind {
    NetWpm,
    Accuracy,
    Iki,
}

#[derive(Clone, Debug)]
pub struct GradeBadgeSpec {
    pub level: Level,
    pub color: Color,
    pub cells: Size,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrendPoint {
    pub date: NaiveDate,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrendSeries {
    pub name: String,
    pub color: Color,
    pub points: Vec<TrendPoint>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrendForecast {
    pub target: String,
    pub start: TrendPoint,
    pub end_date: NaiveDate,
    pub forecast: f64,
    pub band_lo: f64,
    pub band_hi: f64,
    pub forecast_days: u32,
    pub color: Color,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrendChartSpec {
    pub kind: MetricKind,
    pub cells: Size,
    pub series: Vec<TrendSeries>,
    pub forecasts: Vec<TrendForecast>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TrendDateRange {
    pub start: NaiveDate,
    pub end: NaiveDate,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum AggregatedDate {
    Known(NaiveDate),
    Unknown,
}

pub(crate) struct TextTrendSeries {
    pub(crate) name: String,
    pub(crate) color: Color,
    pub(crate) points: Vec<(String, f64)>,
}

pub(crate) struct TextTrendData {
    pub(crate) series: Vec<TextTrendSeries>,
}

impl TrendChartSpec {
    pub fn dates(&self) -> Option<(NaiveDate, NaiveDate)> {
        let range = shared_trend_date_range(std::iter::once(self))?;
        Some((range.start, range.end))
    }

    pub fn y_bounds(&self) -> Option<(f64, f64)> {
        let values = self
            .series
            .iter()
            .flat_map(|series| series.points.iter().map(|point| point.value))
            .chain(
                self.forecasts
                    .iter()
                    .flat_map(|forecast| [forecast.start.value, forecast.forecast, forecast.band_lo, forecast.band_hi]),
            )
            .filter(|value| value.is_finite());
        let (mut min, mut max) = values.fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), value| (min.min(value), max.max(value)));
        if !min.is_finite() || !max.is_finite() {
            return None;
        }
        if (max - min).abs() < f64::EPSILON {
            let padding = (max.abs() * 0.05).max(1.0);
            min -= padding;
            max += padding;
        }
        Some((min, max))
    }
}

pub fn shared_trend_date_range<'a>(specs: impl IntoIterator<Item = &'a TrendChartSpec>) -> Option<TrendDateRange> {
    let mut start = None::<NaiveDate>;
    let mut end = None::<NaiveDate>;
    for spec in specs {
        for date in spec.series.iter().flat_map(|series| series.points.iter().map(|point| point.date)) {
            start = Some(start.map_or(date, |current| current.min(date)));
            end = Some(end.map_or(date, |current| current.max(date)));
        }
        for forecast in &spec.forecasts {
            end = Some(end.map_or(forecast.end_date, |current| current.max(forecast.end_date)));
        }
    }
    Some(TrendDateRange { start: start?, end: end? })
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PointKey {
    date: NaiveDate,
    value: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct SeriesKey {
    name: String,
    color: [u8; 3],
    points: Vec<PointKey>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ForecastKey {
    target: String,
    start: PointKey,
    end_date: NaiveDate,
    forecast: u64,
    band_lo: u64,
    band_hi: u64,
    forecast_days: u32,
    color: [u8; 3],
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum AnalysisImageKey {
    Badge {
        font: FontIdentity,
        level: Level,
        color: [u8; 3],
        cells: Size,
    },
    Chart {
        kind: MetricKind,
        cells: Size,
        x_range: Option<TrendDateRange>,
        series: Vec<SeriesKey>,
        forecasts: Vec<ForecastKey>,
    },
}

pub struct AnalysisImageCache {
    cache: ProtocolCache<AnalysisImageKey>,
}

impl Default for AnalysisImageCache {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalysisImageCache {
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

    pub fn prepare_badge(&mut self, detection: &GraphicsDetection, font: &ResolvedBigFont, spec: &GradeBadgeSpec) -> Result<(), String> {
        let key = badge_key(font, spec);
        self.cache
            .prepare(detection, key, spec.cells, |cell_pixels| render_grade_badge(font, cell_pixels, spec))
    }

    pub fn prepare_chart(&mut self, detection: &GraphicsDetection, spec: &TrendChartSpec) -> Result<(), String> {
        let x_range = spec.dates().map(|(start, end)| TrendDateRange { start, end });
        let key = chart_key(spec, x_range);
        self.cache
            .prepare(detection, key, spec.cells, |cell_pixels| render_trend_chart(cell_pixels, spec))
    }

    pub fn prepare_chart_with_range(&mut self, detection: &GraphicsDetection, spec: &TrendChartSpec, x_range: TrendDateRange) -> Result<(), String> {
        let key = chart_key(spec, Some(x_range));
        self.cache.prepare(detection, key, spec.cells, |cell_pixels| {
            render_trend_chart_with_range(cell_pixels, spec, x_range)
        })
    }

    pub(crate) fn badge_protocol<'a>(
        &'a mut self,
        detection: &GraphicsDetection,
        font: &ResolvedBigFont,
        spec: &GradeBadgeSpec,
    ) -> Option<&'a Protocol> {
        self.cache.protocol(detection, &badge_key(font, spec))
    }

    pub(crate) fn chart_protocol_with_range<'a>(
        &'a mut self,
        detection: &GraphicsDetection,
        spec: &TrendChartSpec,
        x_range: TrendDateRange,
    ) -> Option<&'a Protocol> {
        self.cache.protocol(detection, &chart_key(spec, Some(x_range)))
    }
}

fn badge_key(font: &ResolvedBigFont, spec: &GradeBadgeSpec) -> AnalysisImageKey {
    AnalysisImageKey::Badge {
        font: font.identity(),
        level: spec.level,
        color: color_rgb(spec.color, [255, 255, 255]),
        cells: spec.cells,
    }
}

fn chart_key(spec: &TrendChartSpec, x_range: Option<TrendDateRange>) -> AnalysisImageKey {
    let point_key = |point: &TrendPoint| PointKey {
        date: point.date,
        value: point.value.to_bits(),
    };
    AnalysisImageKey::Chart {
        kind: spec.kind,
        cells: spec.cells,
        x_range,
        series: spec
            .series
            .iter()
            .map(|series| SeriesKey {
                name: series.name.clone(),
                color: color_rgb(series.color, [255, 255, 255]),
                points: series.points.iter().map(&point_key).collect(),
            })
            .collect(),
        forecasts: spec
            .forecasts
            .iter()
            .map(|forecast| ForecastKey {
                target: forecast.target.clone(),
                start: point_key(&forecast.start),
                end_date: forecast.end_date,
                forecast: forecast.forecast.to_bits(),
                band_lo: forecast.band_lo.to_bits(),
                band_hi: forecast.band_hi.to_bits(),
                forecast_days: forecast.forecast_days,
                color: color_rgb(forecast.color, [255, 255, 255]),
            })
            .collect(),
    }
}

pub fn render_grade_badge(font: &ResolvedBigFont, cell_pixels: CellPixels, spec: &GradeBadgeSpec) -> Result<RgbaImage, String> {
    let width = u32::from(spec.cells.width) * u32::from(cell_pixels.width);
    let height = u32::from(spec.cells.height) * u32::from(cell_pixels.height);
    let mut pixmap = Pixmap::new(width.max(1), height.max(1)).ok_or_else(|| "badge dimensions are too large".to_owned())?;
    let grade = color_rgb(spec.color, [255, 255, 255]);
    let min_dimension = width.min(height) as f32;
    let margin = 2.0;
    let radius = (min_dimension * 0.13).max(4.0);
    fill_round_rect(
        &mut pixmap,
        [margin, margin, width as f32 - margin, height as f32 - margin],
        radius,
        grade,
        255,
    );
    let border = (min_dimension * 0.035).max(3.0);
    fill_round_rect(
        &mut pixmap,
        [
            margin + border,
            margin + border,
            width as f32 - margin - border,
            height as f32 - margin - border,
        ],
        (radius - border).max(2.0),
        BADGE_FACE,
        255,
    );

    let glyph_side = (min_dimension * 0.62).round().max(1.0) as u32;
    let mut glyph = Pixmap::new(glyph_side, glyph_side).ok_or_else(|| "badge glyph dimensions are too large".to_owned())?;
    draw_big_character(
        &mut glyph,
        font,
        BigTextSize::Dot16,
        spec.level.as_str().chars().next().unwrap_or('?'),
        grade,
        true,
    )?;
    let x = (width.saturating_sub(glyph_side) / 2) as i32;
    let y = (height.saturating_sub(glyph_side) / 2) as i32;
    pixmap.draw_pixmap(x, y, glyph.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    rgba_image(pixmap)
}

pub fn render_trend_chart(cell_pixels: CellPixels, spec: &TrendChartSpec) -> Result<RgbaImage, String> {
    let x_range = spec.dates().map(|(start, end)| TrendDateRange { start, end });
    render_trend_chart_with_optional_range(cell_pixels, spec, x_range)
}

pub fn render_trend_chart_with_range(cell_pixels: CellPixels, spec: &TrendChartSpec, x_range: TrendDateRange) -> Result<RgbaImage, String> {
    render_trend_chart_with_optional_range(cell_pixels, spec, Some(x_range))
}

fn render_trend_chart_with_optional_range(
    cell_pixels: CellPixels,
    spec: &TrendChartSpec,
    x_range: Option<TrendDateRange>,
) -> Result<RgbaImage, String> {
    let width = u32::from(spec.cells.width) * u32::from(cell_pixels.width);
    let height = u32::from(spec.cells.height) * u32::from(cell_pixels.height);
    let mut pixmap = Pixmap::new(width.max(1), height.max(1)).ok_or_else(|| "chart dimensions are too large".to_owned())?;
    let Some(TrendDateRange {
        start: first_date,
        end: last_date,
    }) = x_range
    else {
        return rgba_image(pixmap);
    };
    let Some((min_y, max_y)) = spec.y_bounds() else {
        return rgba_image(pixmap);
    };
    let left = 2.0_f32;
    let right = (width as f32 - 3.0).max(left);
    let top = 2.0_f32;
    let bottom = (height as f32 - 3.0).max(top);
    let total_days = (last_date - first_date).num_days().max(1) as f64;
    let x = |date: NaiveDate| left + ((date - first_date).num_days() as f64 / total_days) as f32 * (right - left);
    let y = |value: f64| {
        let fraction = ((max_y - value) / (max_y - min_y)).clamp(0.0, 1.0) as f32;
        top + fraction * (bottom - top)
    };

    for index in 0..4 {
        let row = top + (bottom - top) * index as f32 / 3.0;
        stroke_line(&mut pixmap, (left, row), (right, row), GRID_COLOR, 55, 1.0, None);
    }

    for forecast in &spec.forecasts {
        let color = color_rgb(forecast.color, [160, 160, 160]);
        let mut fan = PathBuilder::new();
        fan.move_to(x(forecast.start.date), y(forecast.start.value));
        let fan_end_x = (x(forecast.end_date) + 1.0).min(width as f32);
        fan.line_to(fan_end_x, y(forecast.band_hi));
        fan.line_to(fan_end_x, y(forecast.band_lo));
        fan.close();
        if let Some(path) = fan.finish() {
            fill_path(&mut pixmap, &path, color, 82);
        }
        stroke_line(
            &mut pixmap,
            (x(forecast.start.date), y(forecast.start.value)),
            (x(forecast.end_date), y(forecast.forecast)),
            color,
            230,
            2.0,
            StrokeDash::new(vec![7.0, 5.0], 0.0),
        );
    }

    for series in &spec.series {
        let color = color_rgb(series.color, [255, 255, 255]);
        if series.points.len() >= 2 {
            let mut path = PathBuilder::new();
            if let Some(first) = series.points.first() {
                path.move_to(x(first.date), y(first.value));
                for point in &series.points[1..] {
                    path.line_to(x(point.date), y(point.value));
                }
                if let Some(path) = path.finish() {
                    let mut paint = Paint::default();
                    paint.set_color_rgba8(color[0], color[1], color[2], 255);
                    paint.anti_alias = true;
                    let stroke = Stroke {
                        width: 2.0,
                        line_cap: LineCap::Round,
                        line_join: LineJoin::Round,
                        ..Stroke::default()
                    };
                    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
                }
            }
        }
        for point in &series.points {
            if let Some(circle) = PathBuilder::from_circle(x(point.date), y(point.value), 2.6) {
                fill_path(&mut pixmap, &circle, color, 255);
            }
        }
    }
    rgba_image(pixmap)
}

pub fn build_trend_data(records: &[SessionRecord], kind: MetricKind, lang: Lang) -> Option<TrendChartSpec> {
    let groups = aggregate_trend(records, kind, lang);
    let dates = groups
        .values()
        .flat_map(|days| days.keys())
        .filter_map(|date| match date {
            AggregatedDate::Known(date) => Some(*date),
            AggregatedDate::Unknown => None,
        })
        .collect::<BTreeSet<_>>();
    if dates.len() < 2 {
        return None;
    }
    let series = groups
        .into_iter()
        .enumerate()
        .filter_map(|(index, (name, days))| {
            let points = days
                .into_iter()
                .filter_map(|(date, values)| match date {
                    AggregatedDate::Known(date) => Some(TrendPoint {
                        date,
                        value: median(&values),
                    }),
                    AggregatedDate::Unknown => None,
                })
                .collect::<Vec<_>>();
            (!points.is_empty()).then_some(TrendSeries {
                name,
                color: SERIES_COLORS[index % SERIES_COLORS.len()],
                points,
            })
        })
        .collect();
    Some(TrendChartSpec {
        kind,
        cells: Size::new(1, 1),
        series,
        forecasts: Vec::new(),
    })
}

pub(crate) fn build_text_trend_data(records: &[SessionRecord], kind: MetricKind, lang: Lang) -> Option<TextTrendData> {
    let groups = aggregate_trend(records, kind, lang);
    let dates = groups.values().flat_map(|days| days.keys()).collect::<BTreeSet<_>>();
    if dates.len() < 2 {
        return None;
    }
    let series = groups
        .into_iter()
        .enumerate()
        .map(|(index, (name, days))| TextTrendSeries {
            name,
            color: SERIES_COLORS[index % SERIES_COLORS.len()],
            points: days
                .into_iter()
                .map(|(date, values)| {
                    let label = match date {
                        AggregatedDate::Known(date) => date.to_string(),
                        AggregatedDate::Unknown => "unknown".to_owned(),
                    };
                    (label, median(&values))
                })
                .collect(),
        })
        .collect();
    Some(TextTrendData { series })
}

fn aggregate_trend(records: &[SessionRecord], kind: MetricKind, lang: Lang) -> BTreeMap<String, BTreeMap<AggregatedDate, Vec<f64>>> {
    let mut groups = BTreeMap::<String, BTreeMap<AggregatedDate, Vec<f64>>>::new();
    for record in records
        .iter()
        .filter(|record| kind != MetricKind::Iki || record.header.metrics_version != 0)
    {
        let date = DateTime::parse_from_rfc3339(&record.header.finished_at)
            .map(|value| AggregatedDate::Known(value.date_naive()))
            .unwrap_or(AggregatedDate::Unknown);
        let series = record
            .header
            .bench_id
            .as_deref()
            .and_then(|id| id.split('/').nth(1))
            .map(|part| match lang {
                Lang::En => format!("bench-{part}"),
                Lang::Ja => format!("ベンチ-{part}"),
            })
            .unwrap_or_else(|| mode_label(&record.header.mode, lang));
        groups
            .entry(series)
            .or_default()
            .entry(date)
            .or_default()
            .push(metric_value(kind, record));
    }
    groups
}

pub(crate) fn metric_value(kind: MetricKind, record: &SessionRecord) -> f64 {
    match kind {
        MetricKind::NetWpm => record.header.net_wpm,
        MetricKind::Accuracy => record.header.accuracy * 100.0,
        MetricKind::Iki => record.header.iki_median_ms,
    }
}

pub(crate) fn mode_label(mode: &str, lang: Lang) -> String {
    if lang == Lang::En {
        return mode.to_owned();
    }
    match mode {
        "random" => "ランダム",
        "words" => "単語",
        "romaji" => "ローマ字",
        "sentence" => "短文",
        "ngram" => "n-gram",
        "bench" => "ベンチ",
        "adaptive" => "適応",
        "unlock" => "解放",
        "rhythm" => "リズム",
        "burst" => "バースト",
        "endurance" => "持久",
        other => other,
    }
    .to_owned()
}

fn median(values: &[f64]) -> f64 {
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn stroke_line(pixmap: &mut Pixmap, start: (f32, f32), end: (f32, f32), color: [u8; 3], alpha: u8, width: f32, dash: Option<StrokeDash>) {
    let mut builder = PathBuilder::new();
    builder.move_to(start.0, start.1);
    builder.line_to(end.0, end.1);
    let Some(path) = builder.finish() else { return };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], alpha);
    paint.anti_alias = true;
    let stroke = Stroke {
        width,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        dash,
        ..Stroke::default()
    };
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}

fn fill_round_rect(pixmap: &mut Pixmap, bounds: [f32; 4], radius: f32, color: [u8; 3], alpha: u8) {
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
        fill_path(pixmap, &path, color, alpha);
    }
}

fn fill_path(pixmap: &mut Pixmap, path: &tiny_skia::Path, color: [u8; 3], alpha: u8) {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], alpha);
    paint.anti_alias = true;
    pixmap.fill_path(path, &paint, FillRule::Winding, Transform::identity(), None);
}

fn rgba_image(pixmap: Pixmap) -> Result<RgbaImage, String> {
    let width = pixmap.width();
    let height = pixmap.height();
    RgbaImage::from_raw(width, height, pixmap.take_demultiplied()).ok_or_else(|| "pixmap dimensions do not match its data".to_owned())
}
