//! On-screen physical keyboard, key feedback, finger guide, and heatmap.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect, Size};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Paragraph;
use ratatui_image::Image;
use unicode_width::UnicodeWidthStr;

use crate::graphics::{GraphicsDetection, GraphicsMode};
use crate::layout::geometry::{Finger, KeyRow, KeyboardGeometry, PhysicalKey};
use crate::layout::{KeyStroke, KeyboardLayout};
use crate::practice::keyboard_fade::KeyFade;
use crate::ui::analysis_image::PixelPoint;
use crate::ui::keyboard_image::{KeyTileSpec, KeyboardImageCache};

const QUARTERS_PER_UNIT: u16 = 4;
const BOARD_QUARTERS: u16 = 60;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyFeedback {
    pub pressed_code: String,
    pub expected_code: Option<String>,
    pub correct: bool,
}

#[derive(Clone, Debug, Default)]
pub struct KeyboardState {
    pub next: Option<KeyStroke>,
    pub last: Option<KeyFeedback>,
    pub heatmap: HashMap<String, f64>,
    pub unlock: Option<UnlockKeyboardState>,
    pub fade: HashMap<String, KeyFade>,
}

#[derive(Clone, Debug, Default)]
pub struct UnlockKeyboardState {
    pub unlocked_codes: HashSet<String>,
    pub next_code: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyboardStyle {
    Keycap,
    Raised,
    Plain,
}

impl KeyboardStyle {
    pub fn parse(value: &str) -> Self {
        match value {
            "raised" => Self::Raised,
            "plain" => Self::Plain,
            _ => Self::Keycap,
        }
    }

    pub fn height(self, finger_guide: bool) -> u16 {
        match self {
            Self::Keycap => 11,
            Self::Raised => 10,
            Self::Plain => 5 + u16::from(finger_guide),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keycap => "keycap",
            Self::Raised => "raised",
            Self::Plain => "plain",
        }
    }

    pub fn width(self) -> u16 {
        match self {
            Self::Keycap => scaled_boundary(BOARD_QUARTERS, 5) + 1,
            Self::Raised => scaled_boundary(BOARD_QUARTERS, 4),
            Self::Plain => scaled_boundary(BOARD_QUARTERS, 3),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct KeyboardView<'a> {
    pub show: &'a str,
    pub style: &'a str,
    pub finger_guide: bool,
}

#[derive(Clone, Copy)]
pub struct KeyboardImageContext<'a> {
    pub detection: &'a GraphicsDetection,
    pub cache: &'a RefCell<KeyboardImageCache>,
}

#[derive(Clone, Copy)]
struct RenderOptions<'a> {
    heatmap_only: bool,
    images: Option<KeyboardImageContext<'a>>,
}

/// Resolve the requested appearance against the actual rectangle. Styles only
/// simplify: keycap → raised → plain → hidden.
pub fn resolve_style(requested: &str, area: Rect, finger_guide: bool) -> Option<KeyboardStyle> {
    let requested = KeyboardStyle::parse(requested);
    let candidates: &[KeyboardStyle] = match requested {
        KeyboardStyle::Keycap => &[KeyboardStyle::Keycap, KeyboardStyle::Raised, KeyboardStyle::Plain],
        KeyboardStyle::Raised => &[KeyboardStyle::Raised, KeyboardStyle::Plain],
        KeyboardStyle::Plain => &[KeyboardStyle::Plain],
    };
    candidates
        .iter()
        .copied()
        .find(|style| style.width() <= area.width && style.height(finger_guide) <= area.height)
}

pub fn render(frame: &mut Frame, area: Rect, geometry: &KeyboardGeometry, layout: &KeyboardLayout, state: &KeyboardState, view: KeyboardView<'_>) {
    render_inner(
        frame,
        area,
        geometry,
        layout,
        state,
        view,
        RenderOptions {
            heatmap_only: false,
            images: None,
        },
    );
}

pub fn render_with_images(
    frame: &mut Frame,
    area: Rect,
    geometry: &KeyboardGeometry,
    layout: &KeyboardLayout,
    state: &KeyboardState,
    view: KeyboardView<'_>,
    images: KeyboardImageContext<'_>,
) {
    render_inner(
        frame,
        area,
        geometry,
        layout,
        state,
        view,
        RenderOptions {
            heatmap_only: false,
            images: Some(images),
        },
    );
}

pub fn render_heatmap(
    frame: &mut Frame,
    area: Rect,
    geometry: &KeyboardGeometry,
    layout: &KeyboardLayout,
    state: &KeyboardState,
    view: KeyboardView<'_>,
) {
    render_inner(
        frame,
        area,
        geometry,
        layout,
        state,
        view,
        RenderOptions {
            heatmap_only: true,
            images: None,
        },
    );
}

pub fn render_heatmap_with_images(
    frame: &mut Frame,
    area: Rect,
    geometry: &KeyboardGeometry,
    layout: &KeyboardLayout,
    state: &KeyboardState,
    view: KeyboardView<'_>,
    images: KeyboardImageContext<'_>,
) {
    render_inner(
        frame,
        area,
        geometry,
        layout,
        state,
        view,
        RenderOptions {
            heatmap_only: true,
            images: Some(images),
        },
    );
}

fn render_inner(
    frame: &mut Frame,
    area: Rect,
    geometry: &KeyboardGeometry,
    layout: &KeyboardLayout,
    state: &KeyboardState,
    view: KeyboardView<'_>,
    options: RenderOptions<'_>,
) {
    if view.show == "hidden" || area.width == 0 || area.height == 0 {
        return;
    }
    let Some(style) = resolve_style(view.style, area, view.finger_guide) else {
        return;
    };
    match style {
        KeyboardStyle::Keycap => {
            let rendered = options.images.is_some_and(|images| {
                images.detection.mode != GraphicsMode::Off && render_keycap_image(frame, area, geometry, layout, state, view, options)
            });
            if !rendered {
                render_keycap(frame, area, geometry, layout, state, view, options.heatmap_only);
            }
        }
        KeyboardStyle::Raised => render_raised(frame, area, geometry, layout, state, view, options.heatmap_only),
        KeyboardStyle::Plain => render_plain(frame, area, geometry, layout, state, view, options.heatmap_only),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct KeyTilePlacement {
    pub x: u16,
    pub y: u16,
    pub spec: KeyTileSpec,
}

pub fn prepare_keycap_images(
    detection: &GraphicsDetection,
    cache: &mut KeyboardImageCache,
    geometry: &KeyboardGeometry,
    layout: &KeyboardLayout,
    state: &KeyboardState,
    view: KeyboardView<'_>,
    heatmap_only: bool,
) -> Result<usize, String> {
    let before = cache.creation_count();
    let ordinary = KeyboardState::default();
    for placement in keycap_tile_placements(geometry, layout, &ordinary, view, heatmap_only) {
        cache.prepare(detection, &placement.spec)?;
    }
    for placement in keycap_tile_placements(geometry, layout, state, view, heatmap_only) {
        cache.prepare(detection, &placement.spec)?;
    }
    Ok(cache.creation_count() - before)
}

pub fn keycap_center_cells(geometry: &KeyboardGeometry, code: &str) -> Option<PixelPoint> {
    const UNIT: u16 = 5;
    for (row_index, row) in geometry.rows.iter().enumerate() {
        let mut quarter = 0;
        for key in row {
            let left = scaled_boundary(quarter, UNIT);
            quarter += key.width;
            let right = scaled_boundary(quarter, UNIT);
            if key.code == code {
                return Some(PixelPoint {
                    x: f64::from(left + right) / 2.0,
                    y: row_index as f64 * 2.0 + 1.0,
                });
            }
        }
    }
    None
}

pub(crate) fn keycap_image_area(area: Rect, finger_guide: bool) -> Rect {
    let width = KeyboardStyle::Keycap.width();
    let height = KeyboardStyle::Keycap.height(finger_guide);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width.min(area.width),
        height.min(area.height),
    )
}

fn render_keycap_image(
    frame: &mut Frame,
    area: Rect,
    geometry: &KeyboardGeometry,
    layout: &KeyboardLayout,
    state: &KeyboardState,
    view: KeyboardView<'_>,
    options: RenderOptions<'_>,
) -> bool {
    let Some(images) = options.images else {
        return false;
    };
    let placements = keycap_tile_placements(geometry, layout, state, view, options.heatmap_only);
    let mut cache = images.cache.borrow_mut();
    if prepare_keycap_images(images.detection, &mut cache, geometry, layout, state, view, options.heatmap_only).is_err() {
        return false;
    }
    let image_area = keycap_image_area(area, view.finger_guide);
    for placement in placements {
        let Some(protocol) = cache.protocol(images.detection, &placement.spec) else {
            return false;
        };
        frame.render_widget(
            Image::new(protocol),
            Rect::new(
                image_area.x + placement.x,
                image_area.y + placement.y,
                placement.spec.cells.width,
                placement.spec.cells.height,
            ),
        );
    }
    true
}

pub fn keycap_tile_placements(
    geometry: &KeyboardGeometry,
    layout: &KeyboardLayout,
    state: &KeyboardState,
    view: KeyboardView<'_>,
    heatmap_only: bool,
) -> Vec<KeyTilePlacement> {
    const UNIT: u16 = 5;
    let mut placements = Vec::with_capacity(geometry.rows.iter().map(Vec::len).sum());
    for (row_index, row) in geometry.rows.iter().enumerate() {
        let mut quarter = 0;
        for key in row {
            let left = scaled_boundary(quarter, UNIT);
            quarter += key.width;
            let right = scaled_boundary(quarter, UNIT);
            let cells_width = right.saturating_sub(left);
            if cells_width == 0 {
                continue;
            }
            let (label, style) = key_appearance(key, layout, state, view.show, heatmap_only, false, u16::MAX);
            placements.push(KeyTilePlacement {
                x: left,
                y: row_index as u16 * 2,
                spec: KeyTileSpec {
                    cells: Size::new(cells_width, 2),
                    label,
                    face_color: style.bg.unwrap_or(Color::Rgb(75, 81, 91)),
                    text_color: style.fg.unwrap_or(Color::White),
                    bold: style.add_modifier.contains(Modifier::BOLD),
                    pressed: is_pressed(key.code, state),
                    modifier: is_modifier(key),
                    finger_color: view.finger_guide.then(|| finger_color(key.finger)),
                },
            });
        }
    }
    placements
}

fn render_keycap(
    frame: &mut Frame,
    area: Rect,
    geometry: &KeyboardGeometry,
    layout: &KeyboardLayout,
    state: &KeyboardState,
    view: KeyboardView<'_>,
    heatmap_only: bool,
) {
    const UNIT: u16 = 5;
    const UP: u8 = 1;
    const DOWN: u8 = 2;
    const LEFT: u8 = 4;
    const RIGHT: u8 = 8;

    let width = KeyboardStyle::Keycap.width();
    let height = KeyboardStyle::Keycap.height(view.finger_guide);
    let origin_x = area.x + area.width.saturating_sub(width) / 2;
    let origin_y = area.y + area.height.saturating_sub(height) / 2;
    let mut connections = vec![vec![0_u8; usize::from(width)]; usize::from(height)];
    let mut border_colors = vec![vec![Color::DarkGray; usize::from(width)]; usize::from(height)];

    for (row_index, row) in geometry.rows.iter().enumerate() {
        let top = row_index * 2;
        let bottom = top + 2;
        let mut quarter = 0;
        for key in row {
            let left = scaled_boundary(quarter, UNIT);
            quarter += key.width;
            let right = scaled_boundary(quarter, UNIT);
            add_horizontal(&mut connections, top, left, right, LEFT, RIGHT);
            add_horizontal(&mut connections, bottom, left, right, LEFT, RIGHT);
            add_vertical(&mut connections, left, top, bottom, UP, DOWN);
            add_vertical(&mut connections, right, top, bottom, UP, DOWN);
            if view.finger_guide {
                for x in left.saturating_add(1)..right {
                    border_colors[bottom][usize::from(x)] = finger_color(key.finger);
                }
            }
        }
    }

    for (y, row) in connections.iter().enumerate() {
        for (x, value) in row.iter().enumerate() {
            let glyph = box_glyph(*value);
            if glyph != " " {
                render_cell(
                    frame,
                    origin_x + x as u16,
                    origin_y + y as u16,
                    glyph,
                    Style::default().fg(border_colors[y][x]),
                );
            }
        }
    }

    for (row_index, row) in geometry.rows.iter().enumerate() {
        let mut quarter = 0;
        for key in row {
            let left = scaled_boundary(quarter, UNIT);
            quarter += key.width;
            let right = scaled_boundary(quarter, UNIT);
            let face_width = right.saturating_sub(left).saturating_sub(1);
            if face_width == 0 {
                continue;
            }
            let (label, style) = key_appearance(key, layout, state, view.show, heatmap_only, false, face_width);
            frame.render_widget(
                Paragraph::new(label).alignment(Alignment::Center).style(style),
                Rect::new(origin_x + left + 1, origin_y + row_index as u16 * 2 + 1, face_width, 1),
            );
        }
    }

    for (row_index, row) in geometry.rows.iter().enumerate() {
        let mut quarter = 0;
        for key in row {
            let left = scaled_boundary(quarter, UNIT);
            quarter += key.width;
            let right = scaled_boundary(quarter, UNIT);
            if is_pressed(key.code, state) {
                let top = origin_y + row_index as u16 * 2;
                let bottom = top + 2;
                for x in left.saturating_add(1)..right {
                    render_cell(frame, origin_x + x, top, "━", Style::default().fg(Color::Black));
                    render_cell(frame, origin_x + x, bottom, "━", Style::default().fg(Color::Black));
                }
                render_cell(frame, origin_x + left, top + 1, "┃", Style::default().fg(Color::Black));
                render_cell(frame, origin_x + right, top + 1, "┃", Style::default().fg(Color::Black));
            }
        }
    }
}

fn add_horizontal(connections: &mut [Vec<u8>], y: usize, left: u16, right: u16, left_flag: u8, right_flag: u8) {
    for x in left..right {
        connections[y][usize::from(x)] |= right_flag;
        connections[y][usize::from(x + 1)] |= left_flag;
    }
}

fn add_vertical(connections: &mut [Vec<u8>], x: u16, top: usize, bottom: usize, up_flag: u8, down_flag: u8) {
    for y in top..bottom {
        connections[y][usize::from(x)] |= down_flag;
        connections[y + 1][usize::from(x)] |= up_flag;
    }
}

fn box_glyph(value: u8) -> &'static str {
    match value {
        1..=3 => "│",
        4 | 8 | 12 => "─",
        10 => "┌",
        6 => "┐",
        9 => "└",
        5 => "┘",
        14 => "┬",
        13 => "┴",
        11 => "├",
        7 => "┤",
        15 => "┼",
        _ => " ",
    }
}

fn render_raised(
    frame: &mut Frame,
    area: Rect,
    geometry: &KeyboardGeometry,
    layout: &KeyboardLayout,
    state: &KeyboardState,
    view: KeyboardView<'_>,
    heatmap_only: bool,
) {
    const UNIT: u16 = 4;
    let width = KeyboardStyle::Raised.width();
    let height = KeyboardStyle::Raised.height(view.finger_guide);
    let origin_x = area.x + area.width.saturating_sub(width) / 2;
    let origin_y = area.y + area.height.saturating_sub(height) / 2;
    for (row_index, row) in geometry.rows.iter().enumerate() {
        let mut quarter = 0;
        for key in row {
            let left = scaled_boundary(quarter, UNIT);
            quarter += key.width;
            let right = scaled_boundary(quarter, UNIT);
            let face_width = right.saturating_sub(left).saturating_sub(1).max(1);
            let pressed = is_pressed(key.code, state);
            let face_y = origin_y + row_index as u16 * 2 + u16::from(pressed);
            let (label, style) = key_appearance(key, layout, state, view.show, heatmap_only, true, face_width);
            frame.render_widget(
                Paragraph::new(label).alignment(Alignment::Center).style(style),
                Rect::new(origin_x + left, face_y, face_width, 1),
            );
            if !pressed {
                let shadow = "▀".repeat(usize::from(face_width));
                let color = if view.finger_guide { finger_color(key.finger) } else { Color::Black };
                frame.render_widget(
                    Paragraph::new(shadow).style(Style::default().fg(color)),
                    Rect::new(origin_x + left, face_y + 1, face_width, 1),
                );
            }
        }
    }
}

fn render_plain(
    frame: &mut Frame,
    area: Rect,
    geometry: &KeyboardGeometry,
    layout: &KeyboardLayout,
    state: &KeyboardState,
    view: KeyboardView<'_>,
    heatmap_only: bool,
) {
    const UNIT: u16 = 3;
    let width = KeyboardStyle::Plain.width();
    let height = KeyboardStyle::Plain.height(view.finger_guide);
    let origin_x = area.x + area.width.saturating_sub(width) / 2;
    let origin_y = area.y + area.height.saturating_sub(height) / 2;
    for (row_index, row) in geometry.rows.iter().enumerate() {
        let mut quarter = 0;
        for key in row {
            let left = scaled_boundary(quarter, UNIT);
            quarter += key.width;
            let right = scaled_boundary(quarter, UNIT);
            let key_width = right.saturating_sub(left).max(1);
            let (label, style) = key_appearance(key, layout, state, view.show, heatmap_only, false, key_width);
            frame.render_widget(
                Paragraph::new(label).alignment(Alignment::Center).style(style),
                Rect::new(origin_x + left, origin_y + row_index as u16, key_width, 1),
            );
        }
    }
    if view.finger_guide {
        render_plain_finger_guide(frame, origin_x, origin_y + 5, geometry, UNIT);
    }
}

fn key_appearance(
    key: &PhysicalKey,
    layout: &KeyboardLayout,
    state: &KeyboardState,
    show: &str,
    heatmap_only: bool,
    raised: bool,
    available_width: u16,
) -> (String, Style) {
    let mut label = key_label(key, layout);
    let mut style = base_style(show);
    if raised {
        style = style.bg(if is_modifier(key) {
            Color::Rgb(54, 58, 66)
        } else {
            Color::Rgb(75, 81, 91)
        });
    }
    if show == "auto" && !is_next_key(key.code, state) {
        match state.fade.get(key.code) {
            Some(KeyFade::Dim) => style = style.fg(Color::DarkGray),
            Some(KeyFade::Hidden) => label.clear(),
            None => {}
        }
    }
    if let Some(value) = state.heatmap.get(key.code) {
        style = style.bg(heatmap_color(*value));
    }
    if let Some(unlock) = state.unlock.as_ref() {
        style = if unlock.next_code.as_deref() == Some(key.code) {
            style.fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if unlock.unlocked_codes.contains(key.code) {
            style.fg(Color::Black).bg(Color::DarkGray)
        } else {
            style.fg(Color::DarkGray)
        };
    }
    if !heatmap_only {
        style = feedback_style(style, key.code, state);
    }
    if UnicodeWidthStr::width(label.as_str()) > usize::from(available_width) {
        label = if available_width > 0 { "·".to_owned() } else { String::new() };
    }
    (label, style)
}

fn key_label(key: &PhysicalKey, layout: &KeyboardLayout) -> String {
    let special = match key.code {
        "Backspace" => Some("BKSP"),
        "Tab" => Some("TAB"),
        "CapsLock" => Some("CAPS"),
        "Enter" => Some("ENTER"),
        "ShiftLeft" | "ShiftRight" => Some("⇧"),
        "ControlLeft" | "ControlRight" => Some("CTRL"),
        "MetaLeft" | "MetaRight" => Some("CMD"),
        "AltLeft" | "AltRight" => Some("ALT"),
        "ContextMenu" => Some("MENU"),
        "Space" => Some("SPACE"),
        "NonConvert" => Some("無変"),
        "Convert" => Some("変換"),
        "KanaMode" => Some("かな"),
        "IntlRo" => Some("ろ"),
        _ => None,
    };
    special
        .map(str::to_owned)
        .unwrap_or_else(|| layout.key(key.code).map(|pair| pair.normal.to_string()).unwrap_or_else(|| "·".to_owned()))
}

fn is_modifier(key: &PhysicalKey) -> bool {
    key.row == KeyRow::Control || matches!(key.code, "Backspace" | "Tab" | "CapsLock" | "Enter" | "ShiftLeft" | "ShiftRight")
}

fn base_style(show: &str) -> Style {
    if show == "dim" {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::White)
    }
}

fn feedback_style(mut style: Style, code: &str, state: &KeyboardState) -> Style {
    if is_next_key(code, state) {
        style = style.fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD);
    }
    if let Some(last) = &state.last {
        if last.pressed_code == code {
            style = style
                .fg(Color::Black)
                .bg(if last.correct { Color::Green } else { Color::Red })
                .add_modifier(Modifier::BOLD);
        } else if !last.correct && last.expected_code.as_deref() == Some(code) {
            style = style.fg(Color::White).bg(Color::Red).add_modifier(Modifier::BOLD);
        }
    }
    style
}

fn is_next_key(code: &str, state: &KeyboardState) -> bool {
    state.next.as_ref().is_some_and(|target| target.code == code)
        || state
            .next
            .as_ref()
            .is_some_and(|target| target.shift && matches!(code, "ShiftLeft" | "ShiftRight"))
}

fn is_pressed(code: &str, state: &KeyboardState) -> bool {
    state.last.as_ref().is_some_and(|feedback| feedback.pressed_code == code)
}

pub(crate) fn heatmap_color(value: f64) -> Color {
    let value = value.clamp(0.0, 1.0);
    Color::Rgb((40.0 + value * 180.0) as u8, (20.0 * (1.0 - value)) as u8, 20)
}

fn render_plain_finger_guide(frame: &mut Frame, origin_x: u16, y: u16, geometry: &KeyboardGeometry, unit: u16) {
    let Some(row) = geometry.rows.get(2) else {
        return;
    };
    let mut quarter = 0;
    for key in row {
        let left = scaled_boundary(quarter, unit);
        quarter += key.width;
        let right = scaled_boundary(quarter, unit);
        let width = right.saturating_sub(left);
        if width > 0 {
            frame.render_widget(
                Paragraph::new("━".repeat(usize::from(width))).style(Style::default().fg(finger_color(key.finger))),
                Rect::new(origin_x + left, y, width, 1),
            );
        }
    }
}

pub(crate) fn finger_color(finger: Finger) -> Color {
    match finger {
        Finger::LeftPinky => Color::LightRed,
        Finger::LeftRing => Color::LightMagenta,
        Finger::LeftMiddle => Color::LightBlue,
        Finger::LeftIndex => Color::LightCyan,
        Finger::RightIndex => Color::LightGreen,
        Finger::RightMiddle => Color::LightYellow,
        Finger::RightRing => Color::Blue,
        Finger::RightPinky => Color::Magenta,
        Finger::Thumb => Color::Gray,
    }
}

fn scaled_boundary(quarters: u16, unit: u16) -> u16 {
    (quarters * unit + QUARTERS_PER_UNIT / 2) / QUARTERS_PER_UNIT
}

fn render_cell(frame: &mut Frame, x: u16, y: u16, symbol: &'static str, style: Style) {
    frame.render_widget(Paragraph::new(symbol).style(style), Rect::new(x, y, 1, 1));
}
