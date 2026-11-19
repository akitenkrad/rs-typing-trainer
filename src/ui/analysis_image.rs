//! Pixel rendering and fixed-protocol caching for analysis badges and charts.

use chrono::{DateTime, NaiveDate};
use image::RgbaImage;
use ratatui::layout::Size;
use ratatui::style::Color;
use ratatui_image::protocol::Protocol;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke, StrokeDash, Transform};
use unicode_width::UnicodeWidthChar;

use crate::analysis::grades::Level;
use crate::analysis::metrics::GroupMetric;
use crate::bench::parse_bench_id;
use crate::graphics::{CellPixels, GraphicsDetection};
use crate::i18n::Lang;
use crate::layout::geometry::Finger;
use crate::record::SessionRecord;
use crate::ui::bigtext::BigTextSize;
use crate::ui::bigtext_image::{FontIdentity, ResolvedBigFont, draw_big_character};
use crate::ui::image_cache::{DEFAULT_TILE_CACHE_LIMIT, ProtocolCache, color_rgb};
use crate::ui::keyboard::{KeyTilePlacement, finger_color, heatmap_color};
use crate::ui::keyboard_image::render_key_tile;

pub const SERIES_COLORS: [Color; 6] = [Color::Cyan, Color::Green, Color::Yellow, Color::Magenta, Color::Blue, Color::Red];
pub const FORECAST_EN_COLOR: Color = Color::Rgb(105, 145, 190);
pub const FORECAST_JA_COLOR: Color = Color::Rgb(220, 140, 75);

const BADGE_FACE: [u8; 3] = [16, 20, 28];
const GRID_COLOR: [u8; 3] = [130, 140, 155];
const LEVELS: [Level; 8] = [Level::G, Level::F, Level::E, Level::D, Level::C, Level::B, Level::A, Level::S];

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

#[derive(Clone, Debug)]
pub struct GradeLadderSpec {
    pub score: f64,
    pub level: Level,
    pub colors: [Color; 8],
    pub cells: Size,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RadarAxis {
    pub label: String,
    pub value: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RadarRing {
    pub value: f64,
    pub label: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RadarChartSpec {
    pub axes: Vec<RadarAxis>,
    pub max_value: f64,
    pub rings: Vec<RadarRing>,
    pub color: Color,
    pub cells: Size,
    pub missing_label: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelBounds {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl PixelBounds {
    pub fn overlaps(self, other: Self) -> bool {
        self.left < other.right && other.left < self.right && self.top < other.bottom && other.top < self.bottom
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RadarLabelBox {
    pub text: String,
    pub bounds: PixelBounds,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RadarLabelLayout {
    pub rings: Vec<RadarLabelBox>,
    pub axes: Vec<RadarLabelBox>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SparklineSpec {
    pub values: Vec<f64>,
    pub color: Color,
    pub cells: Size,
}

/// One component chart of the «skills» tab (§8.4): each run as a faint point,
/// the 7-run moving median as a thick line, bench sessions as diamonds, and
/// the grade boundaries as faint bands behind them. The x axis is calendar
/// days over `x_range`, as in the trend tab.
#[derive(Clone, Debug, PartialEq)]
pub struct SkillChartSpec {
    pub cells: Size,
    pub x_range: TrendDateRange,
    /// (minimum, maximum) of the y axis.
    pub y_range: (f64, f64),
    pub points: Vec<TrendPoint>,
    pub medians: Vec<TrendPoint>,
    pub bench: Vec<TrendPoint>,
    /// Grade boundaries (raw values) inside or around `y_range`.
    pub boundaries: Vec<f64>,
    pub line_color: Color,
    pub bench_color: Color,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransitionBar {
    pub value: f64,
    pub error_rate: f64,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransitionBarsSpec {
    pub rows: Vec<TransitionBar>,
    pub max_error_rate: f64,
    pub cells: Size,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransitionArrowSpec {
    pub cells: Size,
    pub from: Option<PixelPoint>,
    pub to: Option<PixelPoint>,
    pub same_key: bool,
    pub color: Color,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransitionKeyboardSpec {
    pub cells: Size,
    pub tiles: Vec<KeyTilePlacement>,
    pub arrow: TransitionArrowSpec,
    pub selection: Option<(String, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FingerBar {
    pub name: String,
    pub iki_median_ms: Option<f64>,
    pub error_rate: Option<f64>,
    pub n: u64,
    pub color: Color,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FingerChartSpec {
    pub bars: Vec<FingerBar>,
    pub cells: Size,
    pub low_sample_n: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComparisonBarsSpec {
    pub values: [f64; 4],
    pub colors: [Color; 4],
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

/// Gaps between consecutive recorded days longer than this many days are cut
/// out of the trend tab's x axis.
pub const TREND_BREAK_MIN_GAP_DAYS: i64 = 14;
/// Width, in days, that a cut-out gap keeps on the x axis.
pub const TREND_BREAK_WIDTH_DAYS: i64 = 3;

/// The trend tab's x axis: calendar days, with long empty stretches cut out.
///
/// The axis is a list of segments (inclusive date ranges). Inside a segment a
/// day is one unit wide; between two segments the gap is squeezed to
/// [`TREND_BREAK_WIDTH_DAYS`] units, whatever its real length. Lines must not
/// be drawn across a break, and the break is marked with `≈`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TrendAxis {
    segments: Vec<TrendDateRange>,
}

impl From<TrendDateRange> for TrendAxis {
    fn from(range: TrendDateRange) -> Self {
        Self {
            segments: vec![TrendDateRange {
                start: range.start.min(range.end),
                end: range.start.max(range.end),
            }],
        }
    }
}

impl TrendAxis {
    /// Builds an axis from the recorded days, cutting every gap longer than
    /// [`TREND_BREAK_MIN_GAP_DAYS`]. Returns `None` when there is no date.
    pub fn from_dates(dates: impl IntoIterator<Item = NaiveDate>) -> Option<Self> {
        let dates = dates.into_iter().collect::<BTreeSet<_>>();
        let mut segments = Vec::<TrendDateRange>::new();
        for date in dates {
            match segments.last_mut() {
                Some(last) if (date - last.end).num_days() <= TREND_BREAK_MIN_GAP_DAYS => last.end = date,
                _ => segments.push(TrendDateRange { start: date, end: date }),
            }
        }
        (!segments.is_empty()).then_some(Self { segments })
    }

    /// Extends the last segment up to `date` (used for forecasts, which run
    /// on from the last record and must not be cut).
    pub fn extend_to(&mut self, date: NaiveDate) {
        if let Some(last) = self.segments.last_mut()
            && date > last.end
        {
            last.end = date;
        }
    }

    pub fn segments(&self) -> &[TrendDateRange] {
        &self.segments
    }

    pub fn start(&self) -> NaiveDate {
        self.segments[0].start
    }

    pub fn end(&self) -> NaiveDate {
        self.segments[self.segments.len() - 1].end
    }

    /// Bounding dates of the axis.
    pub fn range(&self) -> TrendDateRange {
        TrendDateRange {
            start: self.start(),
            end: self.end(),
        }
    }

    pub fn has_breaks(&self) -> bool {
        self.segments.len() > 1
    }

    /// Length of the axis in units (days inside segments plus the squeezed gaps).
    pub fn span(&self) -> f64 {
        let days = self.segments.iter().map(|segment| (segment.end - segment.start).num_days()).sum::<i64>();
        (days + TREND_BREAK_WIDTH_DAYS * (self.segments.len() as i64 - 1)) as f64
    }

    /// Position of `date` in units from the start of the axis. Continuous and
    /// monotone: a date inside a cut gap is placed proportionally inside the
    /// squeezed gap, and dates outside the axis extrapolate by one unit a day.
    pub fn position(&self, date: NaiveDate) -> f64 {
        let mut offset = 0.0;
        for (index, segment) in self.segments.iter().enumerate() {
            if date <= segment.end || index + 1 == self.segments.len() {
                if index > 0 && date < segment.start {
                    let previous = self.segments[index - 1];
                    let gap = (segment.start - previous.end).num_days() as f64;
                    let into = (date - previous.end).num_days() as f64;
                    return offset - TREND_BREAK_WIDTH_DAYS as f64 * (1.0 - into / gap);
                }
                return offset + (date - segment.start).num_days() as f64;
            }
            offset += (segment.end - segment.start).num_days() as f64 + TREND_BREAK_WIDTH_DAYS as f64;
        }
        offset
    }

    /// Position of `date` as a fraction of the axis (0 = start, 1 = end).
    pub fn fraction(&self, date: NaiveDate) -> f64 {
        self.position(date) / self.span().max(1.0)
    }

    /// Positions (as fractions of the axis) of the middle of each break.
    pub fn break_fractions(&self) -> Vec<f64> {
        let span = self.span().max(1.0);
        let mut offset = 0.0;
        let mut breaks = Vec::new();
        for (index, segment) in self.segments.iter().enumerate() {
            offset += (segment.end - segment.start).num_days() as f64;
            if index + 1 < self.segments.len() {
                breaks.push((offset + TREND_BREAK_WIDTH_DAYS as f64 / 2.0) / span);
                offset += TREND_BREAK_WIDTH_DAYS as f64;
            }
        }
        breaks
    }

    /// Index of the segment that `date` falls in, or the last segment before
    /// it when it falls in a gap.
    pub fn segment_index(&self, date: NaiveDate) -> usize {
        self.segments.iter().rposition(|segment| segment.start <= date).unwrap_or(0)
    }

    /// Whether a line from `a` to `b` would cross a break.
    pub fn crosses_break(&self, a: NaiveDate, b: NaiveDate) -> bool {
        self.segment_index(a) != self.segment_index(b)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum AggregatedDate {
    Known(NaiveDate),
    Unknown,
}

pub(crate) struct TextTrendSeries {
    pub(crate) name: String,
    pub(crate) color: Color,
    /// `None` is the bucket of records whose timestamp could not be read.
    pub(crate) points: Vec<(Option<NaiveDate>, f64)>,
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
    shared_trend_axis(specs).map(|axis| axis.range())
}

/// The x axis shared by the trend tab's charts: every recorded day of every
/// chart, long gaps cut out, and the last segment extended to the furthest
/// forecast.
pub fn shared_trend_axis<'a>(specs: impl IntoIterator<Item = &'a TrendChartSpec>) -> Option<TrendAxis> {
    let specs = specs.into_iter().collect::<Vec<_>>();
    let dates = specs
        .iter()
        .flat_map(|spec| spec.series.iter().flat_map(|series| series.points.iter().map(|point| point.date)));
    let mut axis = TrendAxis::from_dates(dates)?;
    for forecast in specs.iter().flat_map(|spec| &spec.forecasts) {
        axis.extend_to(forecast.end_date);
    }
    Some(axis)
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
struct RadarAxisKey {
    label: String,
    value: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RadarRingKey {
    value: u64,
    label: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct FingerBarKey {
    name: String,
    iki_median_ms: Option<u64>,
    error_rate: Option<u64>,
    n: u64,
    color: [u8; 3],
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TransitionKeyTileKey {
    x: u16,
    y: u16,
    cells: Size,
    label: String,
    face_color: [u8; 3],
    text_color: [u8; 3],
    bold: bool,
    pressed: bool,
    modifier: bool,
    finger_color: Option<[u8; 3]>,
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
        x_range: Option<TrendAxis>,
        series: Vec<SeriesKey>,
        forecasts: Vec<ForecastKey>,
    },
    Ladder {
        font: FontIdentity,
        score: u64,
        level: Level,
        colors: [[u8; 3]; 8],
        cells: Size,
    },
    Radar {
        font: FontIdentity,
        axes: Vec<RadarAxisKey>,
        max_value: u64,
        rings: Vec<RadarRingKey>,
        color: [u8; 3],
        cells: Size,
        missing_label: String,
    },
    Sparkline {
        values: Vec<u64>,
        color: [u8; 3],
        cells: Size,
    },
    SkillChart {
        cells: Size,
        x_range: TrendDateRange,
        y_range: (u64, u64),
        points: Vec<PointKey>,
        medians: Vec<PointKey>,
        bench: Vec<PointKey>,
        boundaries: Vec<u64>,
        colors: [[u8; 3]; 2],
    },
    TransitionBars {
        rows: Vec<(u64, u64, bool)>,
        max_error_rate: u64,
        cells: Size,
    },
    TransitionKeyboard {
        cells: Size,
        tiles: Vec<TransitionKeyTileKey>,
        from: Option<(u64, u64)>,
        to: Option<(u64, u64)>,
        same_key: bool,
        color: [u8; 3],
        selection: Option<(String, String)>,
    },
    FingerChart {
        bars: Vec<FingerBarKey>,
        cells: Size,
        low_sample_n: u64,
    },
    ComparisonBars {
        values: [u64; 4],
        colors: [[u8; 3]; 4],
        cells: Size,
    },
    /// The «today» tab's figures, keyed by their spec's debug text (the specs
    /// carry floats and nested rows; the text is exact for equal specs).
    Today {
        kind: &'static str,
        font: Option<FontIdentity>,
        spec: String,
        cells: Size,
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
        let key = chart_key(spec, shared_trend_axis(std::iter::once(spec)));
        self.cache
            .prepare(detection, key, spec.cells, |cell_pixels| render_trend_chart(cell_pixels, spec))
    }

    /// Prepares a trend chart drawn over `x_range` — a [`TrendAxis`] (with
    /// breaks) or a plain [`TrendDateRange`] (one segment, no breaks).
    pub fn prepare_chart_with_range(
        &mut self,
        detection: &GraphicsDetection,
        spec: &TrendChartSpec,
        x_range: impl Into<TrendAxis>,
    ) -> Result<(), String> {
        let axis = x_range.into();
        let key = chart_key(spec, Some(axis.clone()));
        self.cache.prepare(detection, key, spec.cells, |cell_pixels| {
            render_trend_chart_with_range(cell_pixels, spec, axis.clone())
        })
    }

    pub fn prepare_ladder(&mut self, detection: &GraphicsDetection, font: &ResolvedBigFont, spec: &GradeLadderSpec) -> Result<(), String> {
        let key = ladder_key(font, spec);
        self.cache
            .prepare(detection, key, spec.cells, |cell_pixels| render_grade_ladder(font, cell_pixels, spec))
    }

    pub fn prepare_radar(&mut self, detection: &GraphicsDetection, font: &ResolvedBigFont, spec: &RadarChartSpec) -> Result<(), String> {
        let key = radar_key(font, spec);
        self.cache
            .prepare(detection, key, spec.cells, |cell_pixels| render_radar_chart(font, cell_pixels, spec))
    }

    pub fn prepare_sparkline(&mut self, detection: &GraphicsDetection, spec: &SparklineSpec) -> Result<(), String> {
        let key = sparkline_key(spec);
        self.cache
            .prepare(detection, key, spec.cells, |cell_pixels| render_sparkline(cell_pixels, spec))
    }

    pub fn prepare_skill_chart(&mut self, detection: &GraphicsDetection, spec: &SkillChartSpec) -> Result<(), String> {
        self.cache.prepare(detection, skill_chart_key(spec), spec.cells, |cell_pixels| {
            render_skill_chart(cell_pixels, spec)
        })
    }

    pub(crate) fn skill_chart_protocol<'a>(&'a mut self, detection: &GraphicsDetection, spec: &SkillChartSpec) -> Option<&'a Protocol> {
        self.cache.protocol(detection, &skill_chart_key(spec))
    }

    pub fn prepare_transition_bars(&mut self, detection: &GraphicsDetection, spec: &TransitionBarsSpec) -> Result<(), String> {
        self.cache.prepare(detection, transition_bars_key(spec), spec.cells, |cell_pixels| {
            render_transition_bars(cell_pixels, spec)
        })
    }

    pub fn prepare_transition_keyboard(&mut self, detection: &GraphicsDetection, spec: &TransitionKeyboardSpec) -> Result<(), String> {
        self.cache.prepare(detection, transition_keyboard_key(spec), spec.cells, |cell_pixels| {
            render_transition_keyboard(cell_pixels, spec)
        })
    }

    pub fn prepare_finger_chart(&mut self, detection: &GraphicsDetection, spec: &FingerChartSpec) -> Result<(), String> {
        self.cache.prepare(detection, finger_chart_key(spec), spec.cells, |cell_pixels| {
            render_finger_chart(cell_pixels, spec)
        })
    }

    pub fn prepare_comparison_bars(&mut self, detection: &GraphicsDetection, spec: &ComparisonBarsSpec) -> Result<(), String> {
        self.cache.prepare(detection, comparison_bars_key(spec), spec.cells, |cell_pixels| {
            render_comparison_bars(cell_pixels, spec)
        })
    }

    pub fn prepare_today_card(&mut self, detection: &GraphicsDetection, font: &ResolvedBigFont, spec: &TodayCardSpec) -> Result<(), String> {
        self.cache.prepare(detection, today_card_key(font, spec), spec.cells, |cell_pixels| {
            render_today_card(font, cell_pixels, spec)
        })
    }

    pub(crate) fn today_card_protocol<'a>(
        &'a mut self,
        detection: &GraphicsDetection,
        font: &ResolvedBigFont,
        spec: &TodayCardSpec,
    ) -> Option<&'a Protocol> {
        self.cache.protocol(detection, &today_card_key(font, spec))
    }

    pub fn prepare_today_steps(&mut self, detection: &GraphicsDetection, font: &ResolvedBigFont, spec: &TodayStepsSpec) -> Result<(), String> {
        self.cache.prepare(detection, today_steps_key(font, spec), spec.cells, |cell_pixels| {
            render_today_steps(font, cell_pixels, spec)
        })
    }

    pub(crate) fn today_steps_protocol<'a>(
        &'a mut self,
        detection: &GraphicsDetection,
        font: &ResolvedBigFont,
        spec: &TodayStepsSpec,
    ) -> Option<&'a Protocol> {
        self.cache.protocol(detection, &today_steps_key(font, spec))
    }

    pub fn prepare_today_timeline(&mut self, detection: &GraphicsDetection, font: &ResolvedBigFont, spec: &TodayTimelineSpec) -> Result<(), String> {
        self.cache.prepare(detection, today_timeline_key(font, spec), spec.cells, |cell_pixels| {
            render_today_timeline(font, cell_pixels, spec)
        })
    }

    pub(crate) fn today_timeline_protocol<'a>(
        &'a mut self,
        detection: &GraphicsDetection,
        font: &ResolvedBigFont,
        spec: &TodayTimelineSpec,
    ) -> Option<&'a Protocol> {
        self.cache.protocol(detection, &today_timeline_key(font, spec))
    }

    pub fn prepare_today_summary(&mut self, detection: &GraphicsDetection, font: &ResolvedBigFont, spec: &TodaySummarySpec) -> Result<(), String> {
        self.cache.prepare(detection, today_summary_key(font, spec), spec.cells, |cell_pixels| {
            render_today_summary(font, cell_pixels, spec)
        })
    }

    pub(crate) fn today_summary_protocol<'a>(
        &'a mut self,
        detection: &GraphicsDetection,
        font: &ResolvedBigFont,
        spec: &TodaySummarySpec,
    ) -> Option<&'a Protocol> {
        self.cache.protocol(detection, &today_summary_key(font, spec))
    }

    pub(crate) fn chart_protocol_with_range<'a>(
        &'a mut self,
        detection: &GraphicsDetection,
        spec: &TrendChartSpec,
        x_range: impl Into<TrendAxis>,
    ) -> Option<&'a Protocol> {
        self.cache.protocol(detection, &chart_key(spec, Some(x_range.into())))
    }

    pub(crate) fn badge_protocol<'a>(
        &'a mut self,
        detection: &GraphicsDetection,
        font: &ResolvedBigFont,
        spec: &GradeBadgeSpec,
    ) -> Option<&'a Protocol> {
        self.cache.protocol(detection, &badge_key(font, spec))
    }

    pub(crate) fn ladder_protocol<'a>(
        &'a mut self,
        detection: &GraphicsDetection,
        font: &ResolvedBigFont,
        spec: &GradeLadderSpec,
    ) -> Option<&'a Protocol> {
        self.cache.protocol(detection, &ladder_key(font, spec))
    }

    pub(crate) fn radar_protocol<'a>(
        &'a mut self,
        detection: &GraphicsDetection,
        font: &ResolvedBigFont,
        spec: &RadarChartSpec,
    ) -> Option<&'a Protocol> {
        self.cache.protocol(detection, &radar_key(font, spec))
    }

    pub(crate) fn sparkline_protocol<'a>(&'a mut self, detection: &GraphicsDetection, spec: &SparklineSpec) -> Option<&'a Protocol> {
        self.cache.protocol(detection, &sparkline_key(spec))
    }

    pub(crate) fn transition_bars_protocol<'a>(&'a mut self, detection: &GraphicsDetection, spec: &TransitionBarsSpec) -> Option<&'a Protocol> {
        self.cache.protocol(detection, &transition_bars_key(spec))
    }

    pub(crate) fn transition_keyboard_protocol<'a>(
        &'a mut self,
        detection: &GraphicsDetection,
        spec: &TransitionKeyboardSpec,
    ) -> Option<&'a Protocol> {
        self.cache.protocol(detection, &transition_keyboard_key(spec))
    }

    pub(crate) fn finger_chart_protocol<'a>(&'a mut self, detection: &GraphicsDetection, spec: &FingerChartSpec) -> Option<&'a Protocol> {
        self.cache.protocol(detection, &finger_chart_key(spec))
    }

    pub(crate) fn comparison_bars_protocol<'a>(&'a mut self, detection: &GraphicsDetection, spec: &ComparisonBarsSpec) -> Option<&'a Protocol> {
        self.cache.protocol(detection, &comparison_bars_key(spec))
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

fn chart_key(spec: &TrendChartSpec, x_range: Option<TrendAxis>) -> AnalysisImageKey {
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

fn ladder_key(font: &ResolvedBigFont, spec: &GradeLadderSpec) -> AnalysisImageKey {
    AnalysisImageKey::Ladder {
        font: font.identity(),
        score: spec.score.to_bits(),
        level: spec.level,
        colors: spec.colors.map(|color| color_rgb(color, [255, 255, 255])),
        cells: spec.cells,
    }
}

fn radar_key(font: &ResolvedBigFont, spec: &RadarChartSpec) -> AnalysisImageKey {
    AnalysisImageKey::Radar {
        font: font.identity(),
        axes: spec
            .axes
            .iter()
            .map(|axis| RadarAxisKey {
                label: axis.label.clone(),
                value: axis.value.map(f64::to_bits),
            })
            .collect(),
        max_value: spec.max_value.to_bits(),
        rings: spec
            .rings
            .iter()
            .map(|ring| RadarRingKey {
                value: ring.value.to_bits(),
                label: ring.label.clone(),
            })
            .collect(),
        color: color_rgb(spec.color, [255, 255, 255]),
        cells: spec.cells,
        missing_label: spec.missing_label.clone(),
    }
}

fn skill_chart_key(spec: &SkillChartSpec) -> AnalysisImageKey {
    let points = |points: &[TrendPoint]| {
        points
            .iter()
            .map(|point| PointKey {
                date: point.date,
                value: point.value.to_bits(),
            })
            .collect::<Vec<_>>()
    };
    AnalysisImageKey::SkillChart {
        cells: spec.cells,
        x_range: spec.x_range,
        y_range: (spec.y_range.0.to_bits(), spec.y_range.1.to_bits()),
        points: points(&spec.points),
        medians: points(&spec.medians),
        bench: points(&spec.bench),
        boundaries: spec.boundaries.iter().map(|value| value.to_bits()).collect(),
        colors: [color_rgb(spec.line_color, [255, 255, 255]), color_rgb(spec.bench_color, [255, 220, 120])],
    }
}

fn sparkline_key(spec: &SparklineSpec) -> AnalysisImageKey {
    AnalysisImageKey::Sparkline {
        values: spec.values.iter().map(|value| value.to_bits()).collect(),
        color: color_rgb(spec.color, [255, 255, 255]),
        cells: spec.cells,
    }
}

fn transition_bars_key(spec: &TransitionBarsSpec) -> AnalysisImageKey {
    AnalysisImageKey::TransitionBars {
        rows: spec
            .rows
            .iter()
            .map(|row| (row.value.to_bits(), row.error_rate.to_bits(), row.selected))
            .collect(),
        max_error_rate: spec.max_error_rate.to_bits(),
        cells: spec.cells,
    }
}

fn point_key(point: PixelPoint) -> (u64, u64) {
    (point.x.to_bits(), point.y.to_bits())
}

fn transition_keyboard_key(spec: &TransitionKeyboardSpec) -> AnalysisImageKey {
    AnalysisImageKey::TransitionKeyboard {
        cells: spec.cells,
        tiles: spec
            .tiles
            .iter()
            .map(|tile| TransitionKeyTileKey {
                x: tile.x,
                y: tile.y,
                cells: tile.spec.cells,
                label: tile.spec.label.clone(),
                face_color: color_rgb(tile.spec.face_color, [75, 81, 91]),
                text_color: color_rgb(tile.spec.text_color, [255, 255, 255]),
                bold: tile.spec.bold,
                pressed: tile.spec.pressed,
                modifier: tile.spec.modifier,
                finger_color: tile.spec.finger_color.map(|color| color_rgb(color, [128, 128, 128])),
            })
            .collect(),
        from: spec.arrow.from.map(point_key),
        to: spec.arrow.to.map(point_key),
        same_key: spec.arrow.same_key,
        color: color_rgb(spec.arrow.color, [255, 255, 255]),
        selection: spec.selection.clone(),
    }
}

fn finger_chart_key(spec: &FingerChartSpec) -> AnalysisImageKey {
    AnalysisImageKey::FingerChart {
        bars: spec
            .bars
            .iter()
            .map(|bar| FingerBarKey {
                name: bar.name.clone(),
                iki_median_ms: bar.iki_median_ms.map(f64::to_bits),
                error_rate: bar.error_rate.map(f64::to_bits),
                n: bar.n,
                color: color_rgb(bar.color, [255, 255, 255]),
            })
            .collect(),
        cells: spec.cells,
        low_sample_n: spec.low_sample_n,
    }
}

fn comparison_bars_key(spec: &ComparisonBarsSpec) -> AnalysisImageKey {
    AnalysisImageKey::ComparisonBars {
        values: spec.values.map(f64::to_bits),
        colors: spec.colors.map(|color| color_rgb(color, [255, 255, 255])),
        cells: spec.cells,
    }
}

pub fn build_finger_bars(metrics: &[GroupMetric]) -> Vec<FingerBar> {
    const SHARED_THUMB_ORDER: [(&str, Finger); 9] = [
        ("left-pinky", Finger::LeftPinky),
        ("left-ring", Finger::LeftRing),
        ("left-middle", Finger::LeftMiddle),
        ("left-index", Finger::LeftIndex),
        ("thumb", Finger::Thumb),
        ("right-index", Finger::RightIndex),
        ("right-middle", Finger::RightMiddle),
        ("right-ring", Finger::RightRing),
        ("right-pinky", Finger::RightPinky),
    ];
    const SPLIT_THUMB_ORDER: [(&str, Finger); 10] = [
        ("left-pinky", Finger::LeftPinky),
        ("left-ring", Finger::LeftRing),
        ("left-middle", Finger::LeftMiddle),
        ("left-index", Finger::LeftIndex),
        ("left-thumb", Finger::Thumb),
        ("right-thumb", Finger::Thumb),
        ("right-index", Finger::RightIndex),
        ("right-middle", Finger::RightMiddle),
        ("right-ring", Finger::RightRing),
        ("right-pinky", Finger::RightPinky),
    ];
    let has_split_thumbs = ["left-thumb", "right-thumb"]
        .into_iter()
        .all(|name| metrics.iter().any(|row| row.name == name));
    let order: &[(&str, Finger)] = if has_split_thumbs { &SPLIT_THUMB_ORDER } else { &SHARED_THUMB_ORDER };
    order
        .iter()
        .map(|(name, finger)| {
            let metric = metrics.iter().find(|row| row.name == *name);
            FingerBar {
                name: (*name).to_owned(),
                iki_median_ms: metric.map(|row| row.iki_median_ms).filter(|value| value.is_finite()),
                error_rate: metric.map(|row| 1.0 - row.accuracy).filter(|value| value.is_finite()),
                n: metric.map_or(0, |row| row.n),
                color: finger_color(*finger),
            }
        })
        .collect()
}

pub fn render_transition_bars(cell_pixels: CellPixels, spec: &TransitionBarsSpec) -> Result<RgbaImage, String> {
    let width = u32::from(spec.cells.width) * u32::from(cell_pixels.width);
    let height = u32::from(spec.cells.height) * u32::from(cell_pixels.height);
    let mut pixmap = Pixmap::new(width.max(1), height.max(1)).ok_or_else(|| "transition bar dimensions are too large".to_owned())?;
    if spec.rows.is_empty() {
        return rgba_image(pixmap);
    }
    let maximum = spec
        .rows
        .iter()
        .map(|row| row.value)
        .filter(|value| value.is_finite())
        .fold(0.0_f64, f64::max)
        .max(f64::EPSILON);
    let maximum_error = if spec.max_error_rate.is_finite() {
        spec.max_error_rate.max(f64::EPSILON)
    } else {
        f64::EPSILON
    };
    let row_height = height as f32 / spec.rows.len() as f32;
    let left = 2.0;
    let available = (width as f32 - 4.0).max(1.0);
    for (index, row) in spec.rows.iter().enumerate() {
        let top = index as f32 * row_height;
        let bottom = (index + 1) as f32 * row_height;
        let fraction = if row.value.is_finite() {
            (row.value / maximum).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let right = left + available * fraction;
        let tag_color = color_rgb(heatmap_color((row.error_rate / maximum_error).clamp(0.0, 1.0)), [255, 255, 255]);
        let body_color = [105, 112, 124];
        let vertical_margin = (row_height * 0.18).clamp(1.0, 4.0);
        fill_round_rect(
            &mut pixmap,
            [left, top + vertical_margin, right.max(left + 1.0), bottom - vertical_margin],
            (row_height * 0.16).max(1.0),
            body_color,
            235,
        );
        let tag_width = f32::from(cell_pixels.width).min((right - left).max(1.0));
        fill_round_rect(
            &mut pixmap,
            [
                (right - tag_width).max(left),
                top + vertical_margin,
                right.max(left + 1.0),
                bottom - vertical_margin,
            ],
            (row_height * 0.12).max(1.0),
            tag_color,
            255,
        );
        if row.selected {
            stroke_rect(
                &mut pixmap,
                [1.0, top + 0.5, (right + 2.0).min(width as f32 - 1.0), bottom - 0.5],
                [255, 255, 255],
                255,
                2.0,
            );
        }
    }
    rgba_image(pixmap)
}

pub fn render_transition_keyboard(cell_pixels: CellPixels, spec: &TransitionKeyboardSpec) -> Result<RgbaImage, String> {
    let width = u32::from(spec.cells.width) * u32::from(cell_pixels.width);
    let height = u32::from(spec.cells.height) * u32::from(cell_pixels.height);
    let mut image = RgbaImage::new(width.max(1), height.max(1));
    for tile in &spec.tiles {
        let rendered = render_key_tile(cell_pixels, &tile.spec);
        image::imageops::overlay(
            &mut image,
            &rendered,
            i64::from(tile.x) * i64::from(cell_pixels.width),
            i64::from(tile.y) * i64::from(cell_pixels.height),
        );
    }
    let arrow = render_transition_arrow(cell_pixels, &spec.arrow)?;
    image::imageops::overlay(&mut image, &arrow, 0, 0);
    Ok(image)
}

pub fn render_transition_arrow(cell_pixels: CellPixels, spec: &TransitionArrowSpec) -> Result<RgbaImage, String> {
    let width = u32::from(spec.cells.width) * u32::from(cell_pixels.width);
    let height = u32::from(spec.cells.height) * u32::from(cell_pixels.height);
    let mut pixmap = Pixmap::new(width.max(1), height.max(1)).ok_or_else(|| "transition arrow dimensions are too large".to_owned())?;
    let (Some(from), Some(to)) = (spec.from, spec.to) else {
        return rgba_image(pixmap);
    };
    let from = (
        from.x as f32 * f32::from(cell_pixels.width),
        from.y as f32 * f32::from(cell_pixels.height),
    );
    let to = (to.x as f32 * f32::from(cell_pixels.width), to.y as f32 * f32::from(cell_pixels.height));
    let color = color_rgb(spec.color, [255, 255, 255]);
    if spec.same_key || ((from.0 - to.0).abs() < f32::EPSILON && (from.1 - to.1).abs() < f32::EPSILON) {
        let radius = (f32::from(cell_pixels.width) * 1.4).max(8.0);
        if let Some(circle) = PathBuilder::from_circle(from.0, from.1, radius) {
            stroke_path(&mut pixmap, &circle, color, 255, 3.5);
        }
        let tip = (from.0 + radius, from.1);
        let mut head = PathBuilder::new();
        head.move_to(tip.0, tip.1);
        head.line_to(tip.0 - 8.0, tip.1 - 6.0);
        head.line_to(tip.0 - 7.0, tip.1 + 7.0);
        head.close();
        if let Some(path) = head.finish() {
            fill_path(&mut pixmap, &path, color, 255);
        }
    } else {
        stroke_line(&mut pixmap, from, to, color, 255, 3.5, None);
        let dx = to.0 - from.0;
        let dy = to.1 - from.1;
        let length = dx.hypot(dy).max(1.0);
        let unit = (dx / length, dy / length);
        let perpendicular = (-unit.1, unit.0);
        let head_length = 12.0;
        let head_width = 7.0;
        let base = (to.0 - unit.0 * head_length, to.1 - unit.1 * head_length);
        let mut head = PathBuilder::new();
        head.move_to(to.0, to.1);
        head.line_to(base.0 + perpendicular.0 * head_width, base.1 + perpendicular.1 * head_width);
        head.line_to(base.0 - perpendicular.0 * head_width, base.1 - perpendicular.1 * head_width);
        head.close();
        if let Some(path) = head.finish() {
            fill_path(&mut pixmap, &path, color, 255);
        }
    }
    rgba_image(pixmap)
}

pub fn render_finger_chart(cell_pixels: CellPixels, spec: &FingerChartSpec) -> Result<RgbaImage, String> {
    let width = u32::from(spec.cells.width) * u32::from(cell_pixels.width);
    let height = u32::from(spec.cells.height) * u32::from(cell_pixels.height);
    let mut pixmap = Pixmap::new(width.max(1), height.max(1)).ok_or_else(|| "finger chart dimensions are too large".to_owned())?;
    if spec.bars.is_empty() {
        return rgba_image(pixmap);
    }
    let maximum_iki = spec
        .bars
        .iter()
        .filter_map(|bar| bar.iki_median_ms)
        .filter(|value| value.is_finite())
        .fold(0.0_f64, f64::max)
        .max(f64::EPSILON);
    let maximum_error = spec
        .bars
        .iter()
        .filter_map(|bar| bar.error_rate)
        .filter(|value| value.is_finite())
        .fold(0.0_f64, f64::max)
        .max(f64::EPSILON);
    let baseline = height as f32 / 2.0;
    let top = 2.0;
    let bottom = (height as f32 - 2.0).max(baseline + 1.0);
    let slot_width = width as f32 / spec.bars.len() as f32;
    stroke_line(&mut pixmap, (0.0, baseline), (width as f32, baseline), GRID_COLOR, 100, 1.0, None);
    stroke_line(
        &mut pixmap,
        (width as f32 / 2.0, top),
        (width as f32 / 2.0, bottom),
        GRID_COLOR,
        190,
        2.0,
        None,
    );
    for (index, bar) in spec.bars.iter().enumerate() {
        let center = (index as f32 + 0.5) * slot_width;
        let half_width = (slot_width * 0.28).max(1.0);
        let color = color_rgb(bar.color, [255, 255, 255]);
        let alpha = if bar.n < spec.low_sample_n { 96 } else { 235 };
        if let Some(value) = bar.iki_median_ms.filter(|value| value.is_finite()) {
            let bar_top = baseline - (baseline - top) * (value / maximum_iki).clamp(0.0, 1.0) as f32;
            fill_round_rect(
                &mut pixmap,
                [center - half_width, bar_top, center + half_width, baseline],
                half_width.min(3.0),
                color,
                alpha,
            );
        }
        if let Some(value) = bar.error_rate.filter(|value| value.is_finite()) {
            let bar_bottom = baseline + (bottom - baseline) * (value / maximum_error).clamp(0.0, 1.0) as f32;
            fill_round_rect(
                &mut pixmap,
                [center - half_width, baseline, center + half_width, bar_bottom],
                half_width.min(3.0),
                color,
                alpha,
            );
        }
    }
    rgba_image(pixmap)
}

pub fn render_comparison_bars(cell_pixels: CellPixels, spec: &ComparisonBarsSpec) -> Result<RgbaImage, String> {
    let width = u32::from(spec.cells.width) * u32::from(cell_pixels.width);
    let height = u32::from(spec.cells.height) * u32::from(cell_pixels.height);
    let mut pixmap = Pixmap::new(width.max(1), height.max(1)).ok_or_else(|| "comparison bar dimensions are too large".to_owned())?;
    let maximum = spec
        .values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .fold(0.0_f64, f64::max)
        .max(f64::EPSILON);
    let row_height = height as f32 / spec.values.len() as f32;
    for (index, (value, color)) in spec.values.into_iter().zip(spec.colors).enumerate() {
        let top = index as f32 * row_height;
        let bottom = (index + 1) as f32 * row_height;
        let right = 2.0 + (width as f32 - 4.0).max(1.0) * (value / maximum).clamp(0.0, 1.0) as f32;
        fill_round_rect(
            &mut pixmap,
            [2.0, top + 2.0, right, bottom - 2.0],
            (row_height * 0.15).max(1.0),
            color_rgb(color, [255, 255, 255]),
            255,
        );
    }
    rgba_image(pixmap)
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

pub fn render_grade_ladder(font: &ResolvedBigFont, cell_pixels: CellPixels, spec: &GradeLadderSpec) -> Result<RgbaImage, String> {
    let width = u32::from(spec.cells.width) * u32::from(cell_pixels.width);
    let height = u32::from(spec.cells.height) * u32::from(cell_pixels.height);
    let mut pixmap = Pixmap::new(width.max(1), height.max(1)).ok_or_else(|| "ladder dimensions are too large".to_owned())?;
    let box_top = height as f32 * 0.50;
    let box_bottom = (height as f32 - 2.0).max(box_top + 1.0);
    let step_width = width as f32 / 8.0;
    let active = level_index(spec.level);
    let mut glyphs = HashMap::new();
    for (index, level) in LEVELS.into_iter().enumerate() {
        let color = color_rgb(spec.colors[index], [255, 255, 255]);
        let left = index as f32 * step_width + 1.0;
        let right = ((index + 1) as f32 * step_width - 1.0).max(left + 1.0);
        fill_rect(
            &mut pixmap,
            [left, box_top, right, box_bottom],
            color,
            if index == active { 225 } else { 95 },
        );
        stroke_rect(
            &mut pixmap,
            [left, box_top, right, box_bottom],
            color,
            255,
            if index == active { 4.0 } else { 1.0 },
        );
        let label_height = u32::from(cell_pixels.height).min((box_bottom - box_top).max(1.0) as u32);
        draw_small_text(
            &mut pixmap,
            font,
            level.as_str(),
            (left + right) / 2.0,
            box_top + 1.0,
            label_height,
            BADGE_FACE,
            true,
            TextAlign::Center,
            cell_pixels,
            &mut glyphs,
        )?;
    }

    let marker_x = (spec.score.clamp(0.0, 8.0) / 8.0 * width as f64) as f32;
    let triangle_top = height as f32 * 0.30;
    let triangle_bottom = box_top - 2.0;
    let half_width = (f32::from(cell_pixels.width) * 0.55).max(3.0);
    let mut marker = PathBuilder::new();
    marker.move_to(marker_x - half_width, triangle_top);
    marker.line_to(marker_x + half_width, triangle_top);
    marker.line_to(marker_x, triangle_bottom);
    marker.close();
    if let Some(path) = marker.finish() {
        fill_path(&mut pixmap, &path, [255, 255, 255], 255);
    }
    let score = format!("{:.2}", spec.score);
    let score_half_width = f32::from(cell_pixels.width) * score.len() as f32 / 2.0;
    let score_x = marker_x.clamp(score_half_width, (width as f32 - score_half_width).max(score_half_width));
    draw_small_text(
        &mut pixmap,
        font,
        &score,
        score_x,
        0.0,
        u32::from(cell_pixels.height).min(triangle_top.max(1.0) as u32),
        [255, 255, 255],
        false,
        TextAlign::Center,
        cell_pixels,
        &mut glyphs,
    )?;
    rgba_image(pixmap)
}

pub fn render_radar_chart(font: &ResolvedBigFont, cell_pixels: CellPixels, spec: &RadarChartSpec) -> Result<RgbaImage, String> {
    let width = u32::from(spec.cells.width) * u32::from(cell_pixels.width);
    let height = u32::from(spec.cells.height) * u32::from(cell_pixels.height);
    let mut pixmap = Pixmap::new(width.max(1), height.max(1)).ok_or_else(|| "radar dimensions are too large".to_owned())?;
    if spec.axes.len() < 3 || !spec.max_value.is_finite() || spec.max_value <= 0.0 {
        return rgba_image(pixmap);
    }
    let center = (width as f32 / 2.0, height as f32 / 2.0);
    let radius = width.min(height) as f32 * 0.30;
    let axis_count = spec.axes.len();
    let angle = |index: usize| -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * index as f32 / axis_count as f32;
    let point = |index: usize, fraction: f32| {
        let angle = angle(index);
        (center.0 + radius * fraction * angle.cos(), center.1 + radius * fraction * angle.sin())
    };
    let mut glyphs = HashMap::new();
    let labels = radar_text_placements(cell_pixels, spec);

    for (ring, label) in spec.rings.iter().zip(&labels.rings) {
        let fraction = (ring.value / spec.max_value).clamp(0.0, 1.0) as f32;
        stroke_polygon(&mut pixmap, (0..axis_count).map(|index| point(index, fraction)), GRID_COLOR, 65, 1.0);
        if let Some(label) = label {
            draw_small_text(
                &mut pixmap,
                font,
                &label.text,
                label.anchor_x,
                label.y,
                u32::from(cell_pixels.height),
                GRID_COLOR,
                false,
                label.align,
                cell_pixels,
                &mut glyphs,
            )?;
        }
    }
    for index in 0..axis_count {
        stroke_line(&mut pixmap, center, point(index, 1.0), GRID_COLOR, 85, 1.0, None);
    }

    let color = color_rgb(spec.color, [255, 255, 255]);
    let vertices = spec
        .axes
        .iter()
        .enumerate()
        .filter_map(|(index, axis)| axis.value.map(|value| point(index, (value / spec.max_value).clamp(0.0, 1.0) as f32)))
        .collect::<Vec<_>>();
    if vertices.len() >= 3 {
        let path = polygon_path(vertices.iter().copied());
        if let Some(path) = path {
            fill_path(&mut pixmap, &path, color, 76);
            stroke_path(&mut pixmap, &path, color, 255, 2.2);
        }
    }
    for (index, axis) in spec.axes.iter().enumerate() {
        if let Some(value) = axis.value {
            let vertex = point(index, (value / spec.max_value).clamp(0.0, 1.0) as f32);
            if let Some(circle) = PathBuilder::from_circle(vertex.0, vertex.1, 3.5) {
                fill_path(&mut pixmap, &circle, color, 255);
            }
        }
        let label = &labels.axes[index];
        draw_small_text(
            &mut pixmap,
            font,
            &label.text,
            label.anchor_x,
            label.y,
            u32::from(cell_pixels.height),
            if axis.value.is_some() { [220, 225, 235] } else { GRID_COLOR },
            false,
            label.align,
            cell_pixels,
            &mut glyphs,
        )?;
    }
    rgba_image(pixmap)
}

pub fn render_sparkline(cell_pixels: CellPixels, spec: &SparklineSpec) -> Result<RgbaImage, String> {
    let width = u32::from(spec.cells.width) * u32::from(cell_pixels.width);
    let height = u32::from(spec.cells.height) * u32::from(cell_pixels.height);
    let mut pixmap = Pixmap::new(width.max(1), height.max(1)).ok_or_else(|| "sparkline dimensions are too large".to_owned())?;
    let finite = spec.values.iter().copied().filter(|value| value.is_finite()).collect::<Vec<_>>();
    if finite.is_empty() {
        return rgba_image(pixmap);
    }
    let min = finite.iter().copied().fold(f64::INFINITY, f64::min);
    let max = finite.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let padding = ((max - min) * 0.1).max(1.0);
    let min = min - padding;
    let max = max + padding;
    let left = 2.0;
    let right = (width as f32 - 3.0).max(left);
    let top = 2.0;
    let bottom = (height as f32 - 3.0).max(top);
    let point = |index: usize, value: f64| {
        let fraction = if spec.values.len() <= 1 {
            1.0
        } else {
            index as f32 / (spec.values.len() - 1) as f32
        };
        let y_fraction = ((max - value) / (max - min)).clamp(0.0, 1.0) as f32;
        (left + (right - left) * fraction, top + (bottom - top) * y_fraction)
    };
    let color = color_rgb(spec.color, [255, 255, 255]);
    let points = spec
        .values
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, value)| value.is_finite())
        .map(|(index, value)| point(index, value))
        .collect::<Vec<_>>();
    if let Some(path) = polygonal_line_path(points.iter().copied()) {
        stroke_path(&mut pixmap, &path, color, 255, 2.0);
    }
    for (index, vertex) in points.iter().enumerate() {
        let radius = if index + 1 == points.len() { 4.5 } else { 2.4 };
        if let Some(circle) = PathBuilder::from_circle(vertex.0, vertex.1, radius) {
            fill_path(&mut pixmap, &circle, color, 255);
        }
    }
    rgba_image(pixmap)
}

pub fn render_skill_chart(cell_pixels: CellPixels, spec: &SkillChartSpec) -> Result<RgbaImage, String> {
    let width = u32::from(spec.cells.width) * u32::from(cell_pixels.width);
    let height = u32::from(spec.cells.height) * u32::from(cell_pixels.height);
    let mut pixmap = Pixmap::new(width.max(1), height.max(1)).ok_or_else(|| "skill chart dimensions are too large".to_owned())?;
    let (min_y, max_y) = spec.y_range;
    if !(min_y.is_finite() && max_y.is_finite()) || max_y <= min_y {
        return rgba_image(pixmap);
    }
    let left = 2.0_f32;
    let right = (width as f32 - 3.0).max(left);
    let top = 2.0_f32;
    let bottom = (height as f32 - 3.0).max(top);
    let total_days = (spec.x_range.end - spec.x_range.start).num_days().max(1) as f64;
    let x = |date: NaiveDate| left + ((date - spec.x_range.start).num_days() as f64 / total_days).clamp(0.0, 1.0) as f32 * (right - left);
    let y = |value: f64| top + ((max_y - value) / (max_y - min_y)).clamp(0.0, 1.0) as f32 * (bottom - top);

    // Grade bands: alternate faint fills between consecutive boundaries,
    // with a faint line on each boundary.
    let mut edges = spec.boundaries.iter().copied().filter(|value| value.is_finite()).collect::<Vec<_>>();
    edges.sort_by(f64::total_cmp);
    for (index, pair) in edges.windows(2).enumerate() {
        let (low, high) = (pair[0].max(min_y), pair[1].min(max_y));
        if high <= low {
            continue;
        }
        if index % 2 == 0
            && let Some(rect) = tiny_skia::Rect::from_ltrb(0.0, y(high), width as f32, y(low))
        {
            fill_path(&mut pixmap, &PathBuilder::from_rect(rect), GRID_COLOR, 22);
        }
    }
    for edge in edges.iter().copied().filter(|edge| (min_y..=max_y).contains(edge)) {
        stroke_line(&mut pixmap, (0.0, y(edge)), (width as f32, y(edge)), GRID_COLOR, 70, 1.0, None);
    }

    let line = color_rgb(spec.line_color, [255, 255, 255]);
    for point in &spec.points {
        if let Some(circle) = PathBuilder::from_circle(x(point.date), y(point.value), 2.2) {
            fill_path(&mut pixmap, &circle, line, 95);
        }
    }
    if let Some(path) = polygonal_line_path(spec.medians.iter().map(|point| (x(point.date), y(point.value)))) {
        stroke_path(&mut pixmap, &path, line, 255, 3.2);
    }
    let bench = color_rgb(spec.bench_color, [255, 220, 120]);
    for point in &spec.bench {
        let (cx, cy) = (x(point.date), y(point.value));
        let size = 4.5;
        let mut diamond = PathBuilder::new();
        diamond.move_to(cx, cy - size);
        diamond.line_to(cx + size, cy);
        diamond.line_to(cx, cy + size);
        diamond.line_to(cx - size, cy);
        diamond.close();
        if let Some(path) = diamond.finish() {
            fill_path(&mut pixmap, &path, bench, 255);
        }
    }
    rgba_image(pixmap)
}

/// Renders a trend chart over its own axis (long gaps between its own
/// records cut out).
pub fn render_trend_chart(cell_pixels: CellPixels, spec: &TrendChartSpec) -> Result<RgbaImage, String> {
    render_trend_chart_with_optional_range(cell_pixels, spec, shared_trend_axis(std::iter::once(spec)))
}

/// Renders a trend chart over `x_range` — a [`TrendAxis`] (with breaks) or a
/// plain [`TrendDateRange`] (one segment, no breaks).
pub fn render_trend_chart_with_range(cell_pixels: CellPixels, spec: &TrendChartSpec, x_range: impl Into<TrendAxis>) -> Result<RgbaImage, String> {
    render_trend_chart_with_optional_range(cell_pixels, spec, Some(x_range.into()))
}

fn render_trend_chart_with_optional_range(cell_pixels: CellPixels, spec: &TrendChartSpec, axis: Option<TrendAxis>) -> Result<RgbaImage, String> {
    let width = u32::from(spec.cells.width) * u32::from(cell_pixels.width);
    let height = u32::from(spec.cells.height) * u32::from(cell_pixels.height);
    let mut pixmap = Pixmap::new(width.max(1), height.max(1)).ok_or_else(|| "chart dimensions are too large".to_owned())?;
    let Some(axis) = axis else {
        return rgba_image(pixmap);
    };
    let Some((min_y, max_y)) = spec.y_bounds() else {
        return rgba_image(pixmap);
    };
    let left = 2.0_f32;
    let right = (width as f32 - 3.0).max(left);
    let top = 2.0_f32;
    let bottom = (height as f32 - 3.0).max(top);
    let x = |date: NaiveDate| left + axis.fraction(date) as f32 * (right - left);
    let y = |value: f64| {
        let fraction = ((max_y - value) / (max_y - min_y)).clamp(0.0, 1.0) as f32;
        top + fraction * (bottom - top)
    };

    for index in 0..4 {
        let row = top + (bottom - top) * index as f32 / 3.0;
        stroke_line(&mut pixmap, (left, row), (right, row), GRID_COLOR, 55, 1.0, None);
    }

    // Axis breaks: a faint dashed vertical line through the plot and a «≈»
    // drawn as two slanted strokes at the bottom edge.
    for fraction in axis.break_fractions() {
        let bx = left + fraction as f32 * (right - left);
        stroke_line(
            &mut pixmap,
            (bx, top),
            (bx, bottom),
            GRID_COLOR,
            110,
            1.0,
            StrokeDash::new(vec![3.0, 3.0], 0.0),
        );
        let half = (f32::from(cell_pixels.width) * 0.45).clamp(2.0, 6.0);
        for offset in [-2.0_f32, 2.0] {
            let cy = bottom - half - 1.0 + offset;
            stroke_line(
                &mut pixmap,
                (bx - half, cy + half * 0.5),
                (bx + half, cy - half * 0.5),
                GRID_COLOR,
                230,
                1.5,
                None,
            );
        }
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
                for pair in series.points.windows(2) {
                    let point = &pair[1];
                    if axis.crosses_break(pair[0].date, point.date) {
                        path.move_to(x(point.date), y(point.value));
                    } else {
                        path.line_to(x(point.date), y(point.value));
                    }
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
                    let date = match date {
                        AggregatedDate::Known(date) => Some(date),
                        AggregatedDate::Unknown => None,
                    };
                    (date, median(&values))
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
            .and_then(parse_bench_id)
            .map(|(bench_series, part, _)| format!("{bench_series}-{}", part.as_str()))
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
        "phrase" => "句",
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

fn level_index(level: Level) -> usize {
    LEVELS.iter().position(|candidate| *candidate == level).unwrap_or(0)
}

#[derive(Clone, Copy, Debug)]
enum TextAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug)]
struct RadarTextPlacement {
    text: String,
    anchor_x: f32,
    y: f32,
    align: TextAlign,
}

struct RadarTextPlacements {
    rings: Vec<Option<RadarTextPlacement>>,
    axes: Vec<RadarTextPlacement>,
}

fn small_text_width(text: &str, glyph_height: u32, cell_pixels: CellPixels) -> u32 {
    let cell_width = (u32::from(cell_pixels.width) * glyph_height.max(1) / u32::from(cell_pixels.height).max(1)).max(1);
    text.chars()
        .map(|character| cell_width * UnicodeWidthChar::width(character).unwrap_or(1).max(1) as u32)
        .sum()
}

fn text_bounds(placement: &RadarTextPlacement, cell_pixels: CellPixels) -> PixelBounds {
    let height = u32::from(cell_pixels.height).max(1);
    let width = small_text_width(&placement.text, height, cell_pixels) as f32;
    let left = match placement.align {
        TextAlign::Left => placement.anchor_x,
        TextAlign::Center => placement.anchor_x - width / 2.0,
        TextAlign::Right => placement.anchor_x - width,
    }
    .round();
    let top = placement.y.round();
    PixelBounds {
        left,
        top,
        right: left + width,
        bottom: top + height as f32,
    }
}

fn radar_text_placements(cell_pixels: CellPixels, spec: &RadarChartSpec) -> RadarTextPlacements {
    let width = f32::from(spec.cells.width) * f32::from(cell_pixels.width);
    let height = f32::from(spec.cells.height) * f32::from(cell_pixels.height);
    let center = (width / 2.0, height / 2.0);
    let radius = width.min(height) * 0.30;
    let axis_count = spec.axes.len().max(1);
    let angle = |index: usize| -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * index as f32 / axis_count as f32;
    let ring_angle = angle(0) + std::f32::consts::TAU / axis_count as f32 / 2.0;
    let rings = spec
        .rings
        .iter()
        .map(|ring| {
            let fraction = (ring.value / spec.max_value).clamp(0.0, 1.0) as f32;
            ring.label.as_ref().map(|label| RadarTextPlacement {
                text: label.clone(),
                anchor_x: center.0 + radius * fraction * ring_angle.cos(),
                y: center.1 + radius * fraction * ring_angle.sin() - f32::from(cell_pixels.height) / 2.0,
                align: TextAlign::Center,
            })
        })
        .collect();
    let axes = spec
        .axes
        .iter()
        .enumerate()
        .map(|(index, axis)| {
            let direction = angle(index);
            RadarTextPlacement {
                text: if axis.value.is_some() || spec.missing_label.is_empty() {
                    axis.label.clone()
                } else {
                    format!("{} {}", axis.label, spec.missing_label)
                },
                anchor_x: center.0 + radius * 1.42 * direction.cos(),
                y: center.1 + radius * 1.42 * direction.sin() - f32::from(cell_pixels.height) / 2.0,
                align: if direction.cos() > 0.25 {
                    TextAlign::Left
                } else if direction.cos() < -0.25 {
                    TextAlign::Right
                } else {
                    TextAlign::Center
                },
            }
        })
        .collect();
    RadarTextPlacements { rings, axes }
}

pub fn radar_label_layout(cell_pixels: CellPixels, spec: &RadarChartSpec) -> RadarLabelLayout {
    let placements = radar_text_placements(cell_pixels, spec);
    RadarLabelLayout {
        rings: placements
            .rings
            .into_iter()
            .flatten()
            .map(|placement| RadarLabelBox {
                text: placement.text.clone(),
                bounds: text_bounds(&placement, cell_pixels),
            })
            .collect(),
        axes: placements
            .axes
            .into_iter()
            .map(|placement| RadarLabelBox {
                text: placement.text.clone(),
                bounds: text_bounds(&placement, cell_pixels),
            })
            .collect(),
    }
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
fn draw_small_text(
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
                let mut glyph = Pixmap::new(key.width, key.height).ok_or_else(|| "label glyph dimensions are too large".to_owned())?;
                draw_big_character(&mut glyph, font, BigTextSize::Dot12, character, color, bold)?;
                entry.insert(glyph);
            }
            let glyph = glyphs.get(&key).expect("inserted label glyph");
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

fn fill_rect(pixmap: &mut Pixmap, bounds: [f32; 4], color: [u8; 3], alpha: u8) {
    let [left, top, right, bottom] = bounds;
    let Some(rect) = tiny_skia::Rect::from_ltrb(left, top, right, bottom) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], alpha);
    paint.anti_alias = true;
    pixmap.fill_rect(rect, &paint, Transform::identity(), None);
}

fn stroke_rect(pixmap: &mut Pixmap, bounds: [f32; 4], color: [u8; 3], alpha: u8, width: f32) {
    let [left, top, right, bottom] = bounds;
    let mut builder = PathBuilder::new();
    builder.move_to(left, top);
    builder.line_to(right, top);
    builder.line_to(right, bottom);
    builder.line_to(left, bottom);
    builder.close();
    if let Some(path) = builder.finish() {
        stroke_path(pixmap, &path, color, alpha, width);
    }
}

fn polygon_path(points: impl IntoIterator<Item = (f32, f32)>) -> Option<tiny_skia::Path> {
    let mut points = points.into_iter();
    let first = points.next()?;
    let mut builder = PathBuilder::new();
    builder.move_to(first.0, first.1);
    for point in points {
        builder.line_to(point.0, point.1);
    }
    builder.close();
    builder.finish()
}

fn polygonal_line_path(points: impl IntoIterator<Item = (f32, f32)>) -> Option<tiny_skia::Path> {
    let mut points = points.into_iter();
    let first = points.next()?;
    let mut builder = PathBuilder::new();
    builder.move_to(first.0, first.1);
    for point in points {
        builder.line_to(point.0, point.1);
    }
    builder.finish()
}

fn stroke_polygon(pixmap: &mut Pixmap, points: impl IntoIterator<Item = (f32, f32)>, color: [u8; 3], alpha: u8, width: f32) {
    if let Some(path) = polygon_path(points) {
        stroke_path(pixmap, &path, color, alpha, width);
    }
}

fn stroke_path(pixmap: &mut Pixmap, path: &tiny_skia::Path, color: [u8; 3], alpha: u8, width: f32) {
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

// ---------------------------------------------------------------------------
// «Today» tab (MYTASK-3631)
// ---------------------------------------------------------------------------

/// The marker of one session in the «today» timeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TodayPointShape {
    /// A step of the practice menu: filled circle.
    Menu,
    /// Individual practice: ring.
    Individual,
    /// A benchmark session: diamond.
    Bench,
}

/// A big-number card of the «today» totals: the label, the value, and the
/// difference from the previous practice day drawn with the result screen's
/// rule (green ▲ better, red ▼ worse, muted ± unchanged; higher is better).
#[derive(Clone, Debug, PartialEq)]
pub struct TodayCardSpec {
    pub label: String,
    pub value: String,
    /// The signed difference; only its sign picks the color and the arrow.
    pub delta: Option<f64>,
    /// The difference as shown, e.g. «11/18 比 +3.0».
    pub delta_text: String,
    pub cells: Size,
    pub colors: crate::ui::result_image::ResultImageColors,
}

/// One planned step of a menu run in the «today» steps figure.
#[derive(Clone, Debug, PartialEq)]
pub struct TodayStepRow {
    /// «1 ウォームアップ».
    pub label: String,
    /// Prescription and level, e.g. «先読みを広げる Lv2».
    pub title: String,
    pub minutes: u8,
    pub done: bool,
    pub value: String,
    /// The difference from the previous run and whether it is an improvement
    /// (`None` when unchanged).
    pub difference: Option<(String, Option<bool>)>,
    /// `advance.sessions` of the level, and how many are met so far.
    pub required: Option<u8>,
    pub met: u8,
    /// «レベル 2 へ», «維持へ» when the run graduated the level (drawn after an
    /// up-arrow; plain digits because the image font has no circled ones).
    pub graduation: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TodayStepsLine {
    /// A run's heading.
    Run(String),
    Step(TodayStepRow),
}

/// The «today's training» figure: one cell row per line, with a done/not-yet
/// badge, a bar for the step's minutes, the value and difference, the
/// graduation condition as circles, and level-ups highlighted.
#[derive(Clone, Debug, PartialEq)]
pub struct TodayStepsSpec {
    pub lines: Vec<TodayStepsLine>,
    /// The label of a step not played yet («未»).
    pub not_played: String,
    pub cells: Size,
    pub colors: crate::ui::result_image::ResultImageColors,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TodayTimelinePoint {
    /// Minutes since local midnight when the session started and ended.
    pub start_minute: f64,
    pub end_minute: f64,
    pub wpm: f64,
    /// 0..1.
    pub accuracy: f64,
    pub color: Color,
    pub shape: TodayPointShape,
}

/// Today's sessions in time order: x = time of day, y = net WPM, color =
/// language, shape = menu step / individual / bench, marker size and the
/// note above it = accuracy.
#[derive(Clone, Debug, PartialEq)]
pub struct TodayTimelineSpec {
    pub points: Vec<TodayTimelinePoint>,
    /// Legend entries for the colors (language name, color).
    pub languages: Vec<(String, Color)>,
    /// Legend labels of the three shapes and the size note.
    pub shape_labels: [String; 3],
    pub size_note: String,
    pub cells: Size,
    pub colors: crate::ui::result_image::ResultImageColors,
}

fn today_card_key(font: &ResolvedBigFont, spec: &TodayCardSpec) -> AnalysisImageKey {
    AnalysisImageKey::Today {
        kind: "card",
        font: Some(font.identity()),
        spec: format!("{spec:?}"),
        cells: spec.cells,
    }
}

fn today_steps_key(font: &ResolvedBigFont, spec: &TodayStepsSpec) -> AnalysisImageKey {
    AnalysisImageKey::Today {
        kind: "steps",
        font: Some(font.identity()),
        spec: format!("{spec:?}"),
        cells: spec.cells,
    }
}

fn today_timeline_key(font: &ResolvedBigFont, spec: &TodayTimelineSpec) -> AnalysisImageKey {
    AnalysisImageKey::Today {
        kind: "timeline",
        font: Some(font.identity()),
        spec: format!("{spec:?}"),
        cells: spec.cells,
    }
}

fn delta_color(delta: Option<f64>, improvement: Option<bool>, colors: crate::ui::result_image::ResultImageColors) -> [u8; 3] {
    let good = improvement.or_else(|| delta.filter(|value| *value != 0.0 && value.is_finite()).map(|value| value > 0.0));
    color_rgb(
        match good {
            Some(true) => colors.correct,
            Some(false) => colors.mistype,
            None => colors.muted,
        },
        [255, 255, 255],
    )
}

/// ▲ (better) or ▼ (worse) at `(x, y)` with side `size`.
fn fill_triangle(pixmap: &mut Pixmap, x: f32, y: f32, size: f32, up: bool, color: [u8; 3]) {
    let points = if up {
        [(x, y + size), (x + size, y + size), (x + size / 2.0, y)]
    } else {
        [(x, y), (x + size, y), (x + size / 2.0, y + size)]
    };
    if let Some(path) = polygon_path(points) {
        fill_path(pixmap, &path, color, 255);
    }
}

fn fill_circle(pixmap: &mut Pixmap, x: f32, y: f32, radius: f32, color: [u8; 3], alpha: u8) {
    if let Some(circle) = PathBuilder::from_circle(x, y, radius.max(0.5)) {
        fill_path(pixmap, &circle, color, alpha);
    }
}

fn stroke_circle(pixmap: &mut Pixmap, x: f32, y: f32, radius: f32, color: [u8; 3], alpha: u8, width: f32) {
    if let Some(circle) = PathBuilder::from_circle(x, y, radius.max(0.5)) {
        stroke_path(pixmap, &circle, color, alpha, width);
    }
}

/// Text truncated to `cells` display cells (with «…»).
fn fit_cells(text: &str, cells: usize) -> String {
    crate::ui::menu_summary::truncate(text, cells)
}

pub fn render_today_card(font: &ResolvedBigFont, cell_pixels: CellPixels, spec: &TodayCardSpec) -> Result<RgbaImage, String> {
    let width = u32::from(spec.cells.width) * u32::from(cell_pixels.width);
    let height = u32::from(spec.cells.height) * u32::from(cell_pixels.height);
    let mut pixmap = Pixmap::new(width.max(1), height.max(1)).ok_or_else(|| "today card dimensions are too large".to_owned())?;
    let (w, h) = (width as f32, height as f32);
    let panel = color_rgb(spec.colors.panel, BADGE_FACE);
    let muted = color_rgb(spec.colors.muted, GRID_COLOR);
    let accent = color_rgb(spec.colors.accent, [41, 184, 219]);
    fill_round_rect(&mut pixmap, [1.0, 1.0, w - 1.0, h - 1.0], 8.0, panel, 245);
    let mut paint_border = PathBuilder::new();
    paint_border.push_rect(tiny_skia::Rect::from_ltrb(1.5, 1.5, (w - 1.5).max(2.0), (h - 1.5).max(2.0)).ok_or("card bounds")?);
    if let Some(path) = paint_border.finish() {
        stroke_path(&mut pixmap, &path, muted, 150, 1.5);
    }
    let mut glyphs = HashMap::new();
    let cell_h = u32::from(cell_pixels.height);
    let label_height = (cell_h * 4 / 5).max(8);
    let label_cells = usize::from(spec.cells.width.saturating_sub(2)) * 5 / 4;
    draw_small_text(
        &mut pixmap,
        font,
        &fit_cells(&spec.label, label_cells),
        8.0,
        5.0,
        label_height,
        muted,
        true,
        TextAlign::Left,
        cell_pixels,
        &mut glyphs,
    )?;
    // The value: as tall as the card allows while fitting its width.
    let value_cells = spec
        .value
        .chars()
        .map(|c| UnicodeWidthChar::width(c).unwrap_or(1).max(1))
        .sum::<usize>()
        .max(1) as f32;
    let by_width = (w - 16.0) / value_cells * u32::from(cell_pixels.height) as f32 / u32::from(cell_pixels.width).max(1) as f32;
    let by_height = h * 0.42;
    let value_height = by_width.min(by_height).max(8.0) as u32;
    let value_top = 5.0 + label_height as f32 + ((h * 0.66 - 5.0 - label_height as f32) - value_height as f32).max(0.0) / 2.0;
    draw_small_text(
        &mut pixmap,
        font,
        &spec.value,
        w / 2.0,
        value_top,
        value_height,
        accent,
        true,
        TextAlign::Center,
        cell_pixels,
        &mut glyphs,
    )?;
    if !spec.delta_text.is_empty() {
        let color = delta_color(spec.delta, None, spec.colors);
        let size = (cell_h * 4 / 5).max(8);
        let y = h - size as f32 - 6.0;
        let mut x = 8.0;
        if let Some(delta) = spec.delta.filter(|value| *value != 0.0 && value.is_finite()) {
            let icon = size as f32 * 0.8;
            fill_triangle(&mut pixmap, x, y + (size as f32 - icon) / 2.0, icon, delta > 0.0, color);
            x += icon + 4.0;
        }
        let text_cells = ((w - x - 6.0) / (u32::from(cell_pixels.width) as f32 * size as f32 / cell_h as f32)).max(1.0) as usize;
        draw_small_text(
            &mut pixmap,
            font,
            &fit_cells(&spec.delta_text, text_cells),
            x,
            y,
            size,
            color,
            false,
            TextAlign::Left,
            cell_pixels,
            &mut glyphs,
        )?;
    }
    rgba_image(pixmap)
}

/// Column offsets (in cells) of the steps figure.
struct TodayStepColumns {
    badge: f32,
    label: f32,
    title: f32,
    bar: f32,
    value: f32,
    difference: f32,
    graduation: f32,
    end: f32,
}

fn today_step_columns(cells: f32) -> TodayStepColumns {
    // Fixed columns; the title and the bar share what is left.
    let fixed = 3.0 + 20.0 + 11.0 + 10.0 + 18.0;
    let free = (cells - fixed).max(8.0);
    let title = (free * 0.6).clamp(6.0, 30.0);
    let bar = (free - title).clamp(2.0, 24.0);
    let badge = 0.0;
    let label = badge + 3.0;
    let title_x = label + 20.0;
    let bar_x = title_x + title;
    let value = bar_x + bar + 1.0;
    let difference = value + 11.0;
    let graduation = difference + 10.0;
    TodayStepColumns {
        badge,
        label,
        title: title_x,
        bar: bar_x,
        value,
        difference,
        graduation,
        end: graduation + 18.0,
    }
}

pub fn render_today_steps(font: &ResolvedBigFont, cell_pixels: CellPixels, spec: &TodayStepsSpec) -> Result<RgbaImage, String> {
    let width = u32::from(spec.cells.width) * u32::from(cell_pixels.width);
    let height = u32::from(spec.cells.height) * u32::from(cell_pixels.height);
    let mut pixmap = Pixmap::new(width.max(1), height.max(1)).ok_or_else(|| "today steps dimensions are too large".to_owned())?;
    let cw = f32::from(cell_pixels.width);
    let ch = f32::from(cell_pixels.height);
    let colors = spec.colors;
    let foreground = color_rgb(colors.foreground, [230, 230, 230]);
    let muted = color_rgb(colors.muted, GRID_COLOR);
    let accent = color_rgb(colors.accent, [41, 184, 219]);
    let correct = color_rgb(colors.correct, [13, 188, 121]);
    let warning = color_rgb(colors.warning, [229, 229, 16]);
    let columns = today_step_columns(f32::from(spec.cells.width));
    let text_height = (ch * 0.8).max(8.0) as u32;
    let text_y = |row: usize| row as f32 * ch + (ch - text_height as f32) / 2.0;
    let mut glyphs = HashMap::new();
    let max_minutes = spec
        .lines
        .iter()
        .filter_map(|line| match line {
            TodayStepsLine::Step(step) => Some(step.minutes),
            TodayStepsLine::Run(_) => None,
        })
        .max()
        .unwrap_or(1)
        .max(1);
    for (row, line) in spec.lines.iter().enumerate() {
        let top = row as f32 * ch;
        if top >= height as f32 {
            break;
        }
        let center_y = top + ch / 2.0;
        match line {
            TodayStepsLine::Run(title) => {
                fill_rect(&mut pixmap, [0.0, top + ch * 0.2, cw * 0.4, top + ch * 0.8], accent, 255);
                draw_small_text(
                    &mut pixmap,
                    font,
                    &fit_cells(title, usize::from(spec.cells.width).saturating_sub(2)),
                    cw,
                    text_y(row),
                    text_height,
                    foreground,
                    true,
                    TextAlign::Left,
                    cell_pixels,
                    &mut glyphs,
                )?;
            }
            TodayStepsLine::Step(step) => {
                let level_up = step.graduation.is_some();
                if level_up {
                    fill_rect(&mut pixmap, [0.0, top + 1.0, width as f32, top + ch - 1.0], warning, 40);
                }
                // Badge: filled with a check when done, a ring when not yet.
                let badge_x = (columns.badge + 1.2) * cw;
                let radius = (ch * 0.36).min(cw * 1.1);
                if step.done {
                    fill_circle(&mut pixmap, badge_x, center_y, radius, if level_up { warning } else { correct }, 255);
                    let mut check = PathBuilder::new();
                    check.move_to(badge_x - radius * 0.5, center_y);
                    check.line_to(badge_x - radius * 0.1, center_y + radius * 0.4);
                    check.line_to(badge_x + radius * 0.55, center_y - radius * 0.45);
                    if let Some(path) = check.finish() {
                        stroke_path(&mut pixmap, &path, BADGE_FACE, 255, (radius * 0.28).max(1.5));
                    }
                } else {
                    stroke_circle(&mut pixmap, badge_x, center_y, radius * 0.85, muted, 220, 1.6);
                }
                let text_color = if step.done { foreground } else { muted };
                draw_small_text(
                    &mut pixmap,
                    font,
                    &fit_cells(&step.label, 24),
                    columns.label * cw,
                    text_y(row),
                    text_height,
                    text_color,
                    false,
                    TextAlign::Left,
                    cell_pixels,
                    &mut glyphs,
                )?;
                let title_cells = ((columns.bar - columns.title - 1.0) * 1.25) as usize;
                draw_small_text(
                    &mut pixmap,
                    font,
                    &fit_cells(&step.title, title_cells),
                    columns.title * cw,
                    text_y(row),
                    text_height,
                    text_color,
                    false,
                    TextAlign::Left,
                    cell_pixels,
                    &mut glyphs,
                )?;
                // The step's minutes as a bar: filled when done, outlined when not yet.
                let bar_full = (columns.value - 1.0 - columns.bar) * cw;
                let bar_width = (bar_full * f32::from(step.minutes) / f32::from(max_minutes)).max(cw * 0.5);
                let bar = [columns.bar * cw, top + ch * 0.25, columns.bar * cw + bar_width, top + ch * 0.75];
                if step.done {
                    fill_round_rect(&mut pixmap, bar, ch * 0.2, if level_up { warning } else { accent }, 230);
                } else {
                    fill_round_rect(&mut pixmap, bar, ch * 0.2, muted, 50);
                    stroke_rect(&mut pixmap, bar, muted, 160, 1.0);
                }
                let value = if step.done { step.value.as_str() } else { spec.not_played.as_str() };
                draw_small_text(
                    &mut pixmap,
                    font,
                    &fit_cells(value, 12),
                    columns.value * cw,
                    text_y(row),
                    text_height,
                    if step.done { foreground } else { muted },
                    step.done,
                    TextAlign::Left,
                    cell_pixels,
                    &mut glyphs,
                )?;
                if step.done
                    && let Some((text, improvement)) = &step.difference
                {
                    let color = delta_color(None, *improvement, colors);
                    let mut x = columns.difference * cw;
                    if let Some(up) = improvement {
                        let icon = text_height as f32 * 0.7;
                        fill_triangle(&mut pixmap, x, center_y - icon / 2.0, icon, *up, color);
                        x += icon + 3.0;
                    }
                    draw_small_text(
                        &mut pixmap,
                        font,
                        &fit_cells(text, 10),
                        x,
                        text_y(row),
                        text_height,
                        color,
                        false,
                        TextAlign::Left,
                        cell_pixels,
                        &mut glyphs,
                    )?;
                }
                // Graduation condition: one circle per required session.
                if let Some(required) = step.required.filter(|required| *required > 0) {
                    let step_x = (cw * 1.6).min(ch * 0.9);
                    let circle_radius = (step_x * 0.36).min(ch * 0.3);
                    let mut x = columns.graduation * cw + circle_radius + 1.0;
                    for index in 0..required {
                        if index < step.met {
                            fill_circle(&mut pixmap, x, center_y, circle_radius, if level_up { warning } else { correct }, 255);
                        } else {
                            stroke_circle(&mut pixmap, x, center_y, circle_radius, muted, 230, 1.5);
                        }
                        x += step_x;
                    }
                    let label = match &step.graduation {
                        Some(graduation) => {
                            // The level-up mark is drawn, not a font glyph.
                            let icon = text_height as f32 * 0.7;
                            fill_triangle(&mut pixmap, x - circle_radius + cw * 0.3, center_y - icon / 2.0, icon, true, warning);
                            x += icon + cw * 0.3;
                            graduation.clone()
                        }
                        None => format!("{}/{}", step.met.min(required), required),
                    };
                    let room = ((columns.end * cw - x) / cw).max(1.0) as usize;
                    draw_small_text(
                        &mut pixmap,
                        font,
                        &fit_cells(&label, room + room / 4),
                        x,
                        text_y(row),
                        text_height,
                        if level_up { warning } else { muted },
                        level_up,
                        TextAlign::Left,
                        cell_pixels,
                        &mut glyphs,
                    )?;
                }
            }
        }
    }
    rgba_image(pixmap)
}

/// The visible time range of the timeline (minutes since local midnight):
/// the sessions plus a margin of 5% of their span, at least 10 minutes, on
/// each side so no marker sits on the edge; at least one hour in all.
pub fn today_timeline_range(points: &[TodayTimelinePoint]) -> (f64, f64) {
    let start = points.iter().map(|point| point.start_minute).fold(f64::INFINITY, f64::min);
    let end = points
        .iter()
        .map(|point| point.end_minute.max(point.start_minute))
        .fold(f64::NEG_INFINITY, f64::max);
    if !start.is_finite() || !end.is_finite() {
        return (0.0, 24.0 * 60.0);
    }
    let margin = ((end - start) * 0.05).max(10.0);
    let (mut first, mut last) = (start - margin, end + margin);
    if last - first < 60.0 {
        let extra = (60.0 - (last - first)) / 2.0;
        first -= extra;
        last += extra;
    }
    (first, last)
}

fn draw_shape(pixmap: &mut Pixmap, shape: TodayPointShape, x: f32, y: f32, radius: f32, color: [u8; 3]) {
    match shape {
        TodayPointShape::Menu => fill_circle(pixmap, x, y, radius, color, 255),
        TodayPointShape::Individual => {
            fill_circle(pixmap, x, y, radius, BADGE_FACE, 255);
            stroke_circle(pixmap, x, y, radius - 1.0, color, 255, (radius * 0.35).max(2.0));
        }
        TodayPointShape::Bench => {
            let r = radius * 1.2;
            if let Some(path) = polygon_path([(x, y - r), (x + r, y), (x, y + r), (x - r, y)]) {
                fill_path(pixmap, &path, color, 255);
            }
        }
    }
}

pub fn render_today_timeline(font: &ResolvedBigFont, cell_pixels: CellPixels, spec: &TodayTimelineSpec) -> Result<RgbaImage, String> {
    let width = u32::from(spec.cells.width) * u32::from(cell_pixels.width);
    let height = u32::from(spec.cells.height) * u32::from(cell_pixels.height);
    let mut pixmap = Pixmap::new(width.max(1), height.max(1)).ok_or_else(|| "today timeline dimensions are too large".to_owned())?;
    let cw = f32::from(cell_pixels.width);
    let ch = f32::from(cell_pixels.height);
    let colors = spec.colors;
    let muted = color_rgb(colors.muted, GRID_COLOR);
    let foreground = color_rgb(colors.foreground, [230, 230, 230]);
    let text_height = (ch * 0.75).max(8.0) as u32;
    let mut glyphs = HashMap::new();

    // Legend on the first row: shapes, then languages, then the size note.
    let legend_y = (ch - text_height as f32) / 2.0;
    let legend_center = ch / 2.0;
    let mut x = cw * 0.5;
    let legend_radius = ch * 0.25;
    for (shape, label) in [TodayPointShape::Menu, TodayPointShape::Individual, TodayPointShape::Bench]
        .into_iter()
        .zip(spec.shape_labels.iter())
    {
        draw_shape(&mut pixmap, shape, x + legend_radius, legend_center, legend_radius, foreground);
        x += legend_radius * 2.0 + cw * 0.5;
        draw_small_text(
            &mut pixmap,
            font,
            label,
            x,
            legend_y,
            text_height,
            muted,
            false,
            TextAlign::Left,
            cell_pixels,
            &mut glyphs,
        )?;
        x += small_text_width(label, text_height, cell_pixels) as f32 + cw * 1.5;
    }
    for (label, color) in &spec.languages {
        let color = color_rgb(*color, [255, 255, 255]);
        fill_rect(&mut pixmap, [x, legend_center - ch * 0.22, x + cw, legend_center + ch * 0.22], color, 255);
        x += cw * 1.5;
        draw_small_text(
            &mut pixmap,
            font,
            label,
            x,
            legend_y,
            text_height,
            color,
            true,
            TextAlign::Left,
            cell_pixels,
            &mut glyphs,
        )?;
        x += small_text_width(label, text_height, cell_pixels) as f32 + cw * 1.5;
    }
    if x + small_text_width(&spec.size_note, text_height, cell_pixels) as f32 <= width as f32 {
        draw_small_text(
            &mut pixmap,
            font,
            &spec.size_note,
            x,
            legend_y,
            text_height,
            muted,
            false,
            TextAlign::Left,
            cell_pixels,
            &mut glyphs,
        )?;
    }

    let points = spec
        .points
        .iter()
        .filter(|point| point.wpm.is_finite() && point.start_minute.is_finite())
        .collect::<Vec<_>>();
    // Plot area: y labels on the left, time labels at the bottom.
    let left = cw * 6.0;
    let right = width as f32 - cw * 2.0;
    let top = ch * 1.8;
    let bottom = height as f32 - ch * 1.3;
    if right <= left + cw || bottom <= top + ch {
        return rgba_image(pixmap);
    }
    let (first, last) = today_timeline_range(&spec.points);
    let max_wpm = points.iter().map(|point| point.wpm).fold(0.0_f64, f64::max);
    let min_wpm = points.iter().map(|point| point.wpm).fold(f64::INFINITY, f64::min);
    // A «nice» grid step (5, 10, 20, 25, 50 WPM) giving at most five lines.
    let (y_low, y_high, step) = if points.is_empty() {
        (0.0, 100.0, 25.0)
    } else {
        let spread = (max_wpm - min_wpm).max(10.0) + 10.0;
        let step = [5.0, 10.0, 20.0, 25.0, 50.0, 100.0]
            .into_iter()
            .find(|step| spread / step <= 4.0)
            .unwrap_or(100.0);
        let low = (((min_wpm - 5.0) / step).floor() * step).max(0.0);
        let high = ((max_wpm + 5.0) / step).ceil() * step;
        (low, high.max(low + step), step)
    };
    let x_of = |minute: f64| left + (right - left) * ((minute - first) / (last - first)).clamp(0.0, 1.0) as f32;
    let y_of = |wpm: f64| bottom - (bottom - top) * ((wpm - y_low) / (y_high - y_low)).clamp(0.0, 1.0) as f32;

    // Grid: the WPM levels and the hours.
    let mut level = y_low;
    while level <= y_high + 0.01 {
        let y = y_of(level);
        stroke_line(&mut pixmap, (left, y), (right, y), GRID_COLOR, 55, 1.0, None);
        draw_small_text(
            &mut pixmap,
            font,
            &format!("{level:.0}"),
            left - cw * 0.6,
            y - text_height as f32 / 2.0,
            text_height,
            muted,
            false,
            TextAlign::Right,
            cell_pixels,
            &mut glyphs,
        )?;
        level += step;
    }
    // Whole hours inside the range (the range has margins, so it does not
    // start on an hour).
    let first_hour = (first / 60.0).ceil() as i64;
    let last_hour = (last / 60.0).floor() as i64;
    let hours = (last_hour - first_hour + 1).max(1) as usize;
    let label_cells = 6.0;
    let max_labels = (((right - left) / cw) / label_cells).floor().max(1.0) as usize;
    let every = hours.div_ceil(max_labels).max(1);
    for (index, hour) in (first_hour..=last_hour).enumerate() {
        let minute = hour as f64 * 60.0;
        let x = x_of(minute);
        stroke_line(
            &mut pixmap,
            (x, top),
            (x, bottom),
            GRID_COLOR,
            if index % every == 0 { 45 } else { 20 },
            1.0,
            None,
        );
        if index % every == 0 {
            let label = format!("{:02}:00", hour.rem_euclid(24));
            draw_small_text(
                &mut pixmap,
                font,
                &label,
                x,
                bottom + ch * 0.2,
                text_height,
                muted,
                false,
                TextAlign::Center,
                cell_pixels,
                &mut glyphs,
            )?;
        }
    }
    stroke_line(&mut pixmap, (left, bottom), (right, bottom), GRID_COLOR, 140, 1.0, None);

    // Sessions: the duration as a band on the baseline, a stem up to the
    // marker, the marker (size by accuracy) and the accuracy above it.
    let annotate = points.len() <= ((right - left) / (cw * 5.0)).max(1.0) as usize;
    for point in &points {
        let color = color_rgb(point.color, [255, 255, 255]);
        let x0 = x_of(point.start_minute);
        let x1 = x_of(point.end_minute.max(point.start_minute)).max(x0 + 2.0);
        fill_rect(&mut pixmap, [x0, bottom - ch * 0.35, x1, bottom - 1.0], color, 120);
        let x = (x0 + x1) / 2.0;
        let y = y_of(point.wpm);
        stroke_line(&mut pixmap, (x, bottom), (x, y), color, 110, (cw * 0.35).max(2.0), None);
        let fraction = ((point.accuracy - 0.85) / 0.15).clamp(0.0, 1.0) as f32;
        let radius = ch * (0.18 + 0.22 * fraction);
        draw_shape(&mut pixmap, point.shape, x, y, radius, color);
        if annotate {
            draw_small_text(
                &mut pixmap,
                font,
                &format!("{:.0}%", point.accuracy * 100.0),
                x,
                (y - radius - text_height as f32 - 2.0).max(top - ch * 0.5),
                text_height,
                color,
                false,
                TextAlign::Center,
                cell_pixels,
                &mut glyphs,
            )?;
        }
    }
    rgba_image(pixmap)
}

// ---------------------------------------------------------------------------
// «Today» Summary screen (MYTASK-3632)
// ---------------------------------------------------------------------------

/// A value with its difference from the last practice day.
#[derive(Clone, Debug, PartialEq)]
pub struct SummaryFigure {
    pub label: String,
    pub value: String,
    /// The signed difference; only its sign picks the color and the arrow.
    pub delta: Option<f64>,
    /// The difference as shown, e.g. «過去 7 日平均比 +5.0»; empty for none.
    pub delta_text: String,
    /// The value compared with, e.g. «平均 57.0»; empty when not shown.
    pub average: String,
}

/// The screenshot-friendly Summary of the «today» tab: a heading band, a
/// decorated frame with one or two very large net WPM values, a row of
/// supporting figures, and a dark band with the day's events at the bottom.
#[derive(Clone, Debug, PartialEq)]
pub struct TodaySummarySpec {
    pub heading: String,
    pub subheading: String,
    /// One per language practised today (at most two); empty without records.
    pub mains: Vec<SummaryFigure>,
    pub unit: String,
    pub details: Vec<SummaryFigure>,
    /// One or two lines; the first is highlighted when `highlight` is set.
    pub events: Vec<String>,
    /// Without events: the menu's steps (short name, done), drawn with
    /// arrows between them; `chain_short` («段 2/5») when they do not fit.
    pub chain: Vec<(String, bool)>,
    pub chain_short: String,
    pub highlight: bool,
    pub cells: Size,
    pub colors: crate::ui::result_image::ResultImageColors,
}

fn today_summary_key(font: &ResolvedBigFont, spec: &TodaySummarySpec) -> AnalysisImageKey {
    AnalysisImageKey::Today {
        kind: "summary",
        font: Some(font.identity()),
        spec: format!("{spec:?}"),
        cells: spec.cells,
    }
}

/// Pixel height of the main value's glyphs: about a fifth of the image
/// height, reduced only when the values would not fit the frame's width.
pub fn today_summary_value_height(spec: &TodaySummarySpec, cell_pixels: CellPixels) -> f32 {
    let width = f32::from(spec.cells.width) * f32::from(cell_pixels.width);
    let height = f32::from(spec.cells.height) * f32::from(cell_pixels.height);
    let target = height * 0.2;
    let columns = spec.mains.len().max(1) as f32;
    let column_width = width * 0.86 / columns;
    // Widest value in display cells, plus room for the unit.
    let cells = spec
        .mains
        .iter()
        .map(|main| main.value.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(1).max(1)).sum::<usize>())
        .max()
        .unwrap_or(1)
        .max(1) as f32
        + 2.0;
    let aspect = f32::from(cell_pixels.width) / f32::from(cell_pixels.height).max(1.0);
    let by_width = column_width * 0.9 / (cells * aspect);
    target.min(by_width).max(8.0)
}

fn text_cells_width(text: &str, glyph_height: f32, cell_pixels: CellPixels) -> f32 {
    small_text_width(text, glyph_height.max(1.0) as u32, cell_pixels) as f32
}

/// Draw `delta_text` after a ▲/▼ in the result screen's colors, centered at `x`.
#[allow(clippy::too_many_arguments)]
fn draw_delta_centered(
    pixmap: &mut Pixmap,
    font: &ResolvedBigFont,
    figure: &SummaryFigure,
    x: f32,
    y: f32,
    size: f32,
    colors: crate::ui::result_image::ResultImageColors,
    cell_pixels: CellPixels,
    glyphs: &mut HashMap<SmallGlyphKey, Pixmap>,
) -> Result<(), String> {
    if figure.delta_text.is_empty() {
        return Ok(());
    }
    let color = delta_color(figure.delta, None, colors);
    let arrow = figure.delta.filter(|delta| *delta != 0.0 && delta.is_finite());
    let icon = size * 0.75;
    let text_width = text_cells_width(&figure.delta_text, size, cell_pixels);
    let total = text_width + if arrow.is_some() { icon + 4.0 } else { 0.0 };
    let mut left = x - total / 2.0;
    if let Some(delta) = arrow {
        fill_triangle(pixmap, left, y + (size - icon) / 2.0, icon, delta > 0.0, color);
        left += icon + 4.0;
    }
    draw_small_text(
        pixmap,
        font,
        &figure.delta_text,
        left,
        y,
        size as u32,
        color,
        false,
        TextAlign::Left,
        cell_pixels,
        glyphs,
    )
}

/// An octagon: a rectangle with its corners cut by `cut`.
fn chamfered(bounds: [f32; 4], cut: f32) -> [(f32, f32); 8] {
    let [left, top, right, bottom] = bounds;
    [
        (left + cut, top),
        (right - cut, top),
        (right, top + cut),
        (right, bottom - cut),
        (right - cut, bottom),
        (left + cut, bottom),
        (left, bottom - cut),
        (left, top + cut),
    ]
}

pub fn render_today_summary(font: &ResolvedBigFont, cell_pixels: CellPixels, spec: &TodaySummarySpec) -> Result<RgbaImage, String> {
    let width = u32::from(spec.cells.width) * u32::from(cell_pixels.width);
    let height = u32::from(spec.cells.height) * u32::from(cell_pixels.height);
    let mut pixmap = Pixmap::new(width.max(1), height.max(1)).ok_or_else(|| "summary dimensions are too large".to_owned())?;
    let (w, h) = (width as f32, height as f32);
    let colors = spec.colors;
    let foreground = color_rgb(colors.foreground, [230, 230, 230]);
    let muted = color_rgb(colors.muted, GRID_COLOR);
    let accent = color_rgb(colors.accent, [41, 184, 219]);
    let warning = color_rgb(colors.warning, [229, 229, 16]);
    let panel = color_rgb(colors.panel, BADGE_FACE);
    let mut glyphs = HashMap::new();

    // Heading band.
    let band = (h * 0.09).max(f32::from(cell_pixels.height));
    fill_rect(&mut pixmap, [0.0, 0.0, w, band], accent, 255);
    let heading_size = band * 0.62;
    let heading_y = (band - heading_size) / 2.0;
    draw_small_text(
        &mut pixmap,
        font,
        &spec.heading,
        w * 0.03,
        heading_y,
        heading_size as u32,
        BADGE_FACE,
        true,
        TextAlign::Left,
        cell_pixels,
        &mut glyphs,
    )?;
    draw_small_text(
        &mut pixmap,
        font,
        &spec.subheading,
        w * 0.97,
        heading_y + heading_size * 0.15,
        (heading_size * 0.8) as u32,
        BADGE_FACE,
        false,
        TextAlign::Right,
        cell_pixels,
        &mut glyphs,
    )?;

    // Decorated frame: a chamfered panel with a thick outer and a thin inner line.
    let frame = [w * 0.04, band + h * 0.035, w * 0.96, h * 0.975];
    let cut = (h * 0.05).min(w * 0.05);
    if let Some(path) = polygon_path(chamfered(frame, cut)) {
        fill_path(&mut pixmap, &path, panel, 235);
    }
    stroke_polygon(&mut pixmap, chamfered(frame, cut), accent, 255, (h * 0.006).max(2.0));
    let inset = (h * 0.014).max(4.0);
    let inner = [frame[0] + inset, frame[1] + inset, frame[2] - inset, frame[3] - inset];
    stroke_polygon(&mut pixmap, chamfered(inner, (cut - inset * 0.6).max(2.0)), muted, 160, 1.2);
    for (x, y) in [
        (frame[0] + cut / 2.0, frame[1] + cut / 2.0),
        (frame[2] - cut / 2.0, frame[1] + cut / 2.0),
        (frame[0] + cut / 2.0, frame[3] - cut / 2.0),
        (frame[2] - cut / 2.0, frame[3] - cut / 2.0),
    ] {
        let r = cut * 0.22;
        if let Some(path) = polygon_path([(x, y - r), (x + r, y), (x, y + r), (x - r, y)]) {
            fill_path(&mut pixmap, &path, accent, 255);
        }
    }

    // Main values.
    let frame_height = frame[3] - frame[1];
    let label_size = (h * 0.045).max(8.0);
    let value_size = today_summary_value_height(spec, cell_pixels);
    let unit_size = (value_size * 0.3).max(8.0);
    let label_y = frame[1] + frame_height * 0.06;
    let value_y = label_y + label_size * 1.4;
    let delta_size = (h * 0.038).max(8.0);
    let delta_y = value_y + value_size + h * 0.015;
    let columns = spec.mains.len().max(1);
    let column_width = (frame[2] - frame[0]) / columns as f32;
    for (index, main) in spec.mains.iter().enumerate() {
        let center = frame[0] + column_width * (index as f32 + 0.5);
        draw_small_text(
            &mut pixmap,
            font,
            &main.label,
            center,
            label_y,
            label_size as u32,
            muted,
            true,
            TextAlign::Center,
            cell_pixels,
            &mut glyphs,
        )?;
        let value_width = text_cells_width(&main.value, value_size, cell_pixels);
        let unit_width = text_cells_width(&spec.unit, unit_size, cell_pixels);
        let left = center - (value_width + unit_width + value_size * 0.15) / 2.0;
        draw_small_text(
            &mut pixmap,
            font,
            &main.value,
            left,
            value_y,
            value_size as u32,
            accent,
            true,
            TextAlign::Left,
            cell_pixels,
            &mut glyphs,
        )?;
        draw_small_text(
            &mut pixmap,
            font,
            &spec.unit,
            left + value_width + value_size * 0.15,
            value_y + value_size - unit_size * 1.1,
            unit_size as u32,
            foreground,
            true,
            TextAlign::Left,
            cell_pixels,
            &mut glyphs,
        )?;
        draw_delta_centered(&mut pixmap, font, main, center, delta_y, delta_size, colors, cell_pixels, &mut glyphs)?;
        if !main.average.is_empty() {
            draw_small_text(
                &mut pixmap,
                font,
                &main.average,
                center,
                delta_y + delta_size * 1.25,
                (delta_size * 0.85) as u32,
                muted,
                false,
                TextAlign::Center,
                cell_pixels,
                &mut glyphs,
            )?;
        }
        if index > 0 {
            let x = frame[0] + column_width * index as f32;
            stroke_line(&mut pixmap, (x, label_y), (x, delta_y + delta_size), GRID_COLOR, 90, 1.0, None);
        }
    }

    // Supporting figures.
    let detail_label = (h * 0.035).max(8.0);
    let detail_value = (h * 0.06).max(9.0);
    let averages = if spec.mains.iter().any(|main| !main.average.is_empty()) {
        delta_size * 1.1
    } else {
        0.0
    };
    let detail_y = delta_y + delta_size + averages + h * 0.045;
    let count = spec.details.len().max(1);
    let detail_width = (frame[2] - frame[0]) / count as f32;
    for (index, detail) in spec.details.iter().enumerate() {
        let center = frame[0] + detail_width * (index as f32 + 0.5);
        draw_small_text(
            &mut pixmap,
            font,
            &detail.label,
            center,
            detail_y,
            detail_label as u32,
            muted,
            false,
            TextAlign::Center,
            cell_pixels,
            &mut glyphs,
        )?;
        draw_small_text(
            &mut pixmap,
            font,
            &detail.value,
            center,
            detail_y + detail_label * 1.3,
            detail_value as u32,
            foreground,
            true,
            TextAlign::Center,
            cell_pixels,
            &mut glyphs,
        )?;
        draw_delta_centered(
            &mut pixmap,
            font,
            detail,
            center,
            detail_y + detail_label * 1.3 + detail_value * 1.15,
            detail_label,
            colors,
            cell_pixels,
            &mut glyphs,
        )?;
    }

    // Dark band with the day's events at the bottom of the frame.
    let lines = if spec.events.is_empty() {
        1.0
    } else {
        spec.events.len().clamp(1, 2) as f32
    };
    let event_size = (h * 0.05).max(9.0);
    let band_height = event_size * (1.5 * lines + 0.6);
    let band_bottom = frame[3] - inset * 1.8;
    let band_top = band_bottom - band_height;
    let band_left = frame[0] + cut * 0.8;
    let band_right = frame[2] - cut * 0.8;
    fill_round_rect(
        &mut pixmap,
        [band_left, band_top, band_right, band_bottom],
        event_size * 0.3,
        [6, 8, 12],
        245,
    );
    stroke_line(&mut pixmap, (band_left, band_top), (band_right, band_top), accent, 200, 1.5, None);
    if spec.events.is_empty() && !spec.chain.is_empty() {
        draw_step_chain(
            &mut pixmap,
            font,
            spec,
            [band_left, band_top, band_right, band_bottom],
            event_size,
            cell_pixels,
            &mut glyphs,
        )?;
    }
    for (index, event) in spec.events.iter().take(2).enumerate() {
        let y = band_top + event_size * (0.45 + 1.5 * index as f32);
        let highlight = spec.highlight && index == 0;
        let text_width = text_cells_width(event, event_size, cell_pixels);
        let icon = event_size * 0.7;
        let mut left = w / 2.0 - (text_width + if highlight { icon + 6.0 } else { 0.0 }) / 2.0;
        if highlight {
            fill_triangle(&mut pixmap, left, y + (event_size - icon) / 2.0, icon, true, warning);
            left += icon + 6.0;
        }
        draw_small_text(
            &mut pixmap,
            font,
            event,
            left,
            y,
            event_size as u32,
            if highlight { warning } else { foreground },
            true,
            TextAlign::Left,
            cell_pixels,
            &mut glyphs,
        )?;
    }
    rgba_image(pixmap)
}

/// The menu's steps in the bottom band: a drawn check and the accent color on
/// done steps, dimmed names on the rest, and drawn arrows between them (the
/// image font has no «→» or «✓»). The size shrinks to fit the band; below a
/// readable size the band shows `chain_short` («段 2/5») instead.
fn draw_step_chain(
    pixmap: &mut Pixmap,
    font: &ResolvedBigFont,
    spec: &TodaySummarySpec,
    band: [f32; 4],
    size: f32,
    cell_pixels: CellPixels,
    glyphs: &mut HashMap<SmallGlyphKey, Pixmap>,
) -> Result<(), String> {
    let [left, top, right, bottom] = band;
    let room = (right - left) * 0.94;
    let correct = color_rgb(spec.colors.correct, [13, 188, 121]);
    let dim = [150, 155, 165];
    let measure = |size: f32| {
        let arrow = size * 1.6;
        let check = size * 0.9;
        spec.chain
            .iter()
            .map(|(label, done)| text_cells_width(label, size, cell_pixels) + if *done { check } else { 0.0 })
            .sum::<f32>()
            + arrow * spec.chain.len().saturating_sub(1) as f32
    };
    let mut size = size;
    while measure(size) > room && size > 8.0 {
        size *= 0.92;
    }
    let minimum = (f32::from(cell_pixels.height) * 0.7).max(9.0);
    if measure(size) > room || size < minimum {
        let size = (bottom - top) * 0.45;
        return draw_small_text(
            pixmap,
            font,
            &spec.chain_short,
            (left + right) / 2.0,
            top + ((bottom - top) - size) / 2.0,
            size as u32,
            [235, 235, 235],
            true,
            TextAlign::Center,
            cell_pixels,
            glyphs,
        );
    }
    let y = top + ((bottom - top) - size) / 2.0;
    let center_y = y + size / 2.0;
    let mut x = (left + right) / 2.0 - measure(size) / 2.0;
    for (index, (label, done)) in spec.chain.iter().enumerate() {
        if index > 0 {
            // Arrow: a shaft and a head.
            let start = x + size * 0.3;
            let end = x + size * 1.3;
            stroke_line(
                pixmap,
                (start, center_y),
                (end - size * 0.25, center_y),
                dim,
                255,
                (size * 0.1).max(1.5),
                None,
            );
            if let Some(path) = polygon_path([
                (end, center_y),
                (end - size * 0.38, center_y - size * 0.24),
                (end - size * 0.38, center_y + size * 0.24),
            ]) {
                fill_path(pixmap, &path, dim, 255);
            }
            x += size * 1.6;
        }
        if *done {
            let mut check = PathBuilder::new();
            check.move_to(x + size * 0.08, center_y);
            check.line_to(x + size * 0.3, center_y + size * 0.25);
            check.line_to(x + size * 0.7, center_y - size * 0.3);
            if let Some(path) = check.finish() {
                stroke_path(pixmap, &path, correct, 255, (size * 0.13).max(2.0));
            }
            x += size * 0.9;
        }
        draw_small_text(
            pixmap,
            font,
            label,
            x,
            y,
            size as u32,
            if *done { correct } else { dim },
            *done,
            TextAlign::Left,
            cell_pixels,
            glyphs,
        )?;
        x += text_cells_width(label, size, cell_pixels);
    }
    Ok(())
}
