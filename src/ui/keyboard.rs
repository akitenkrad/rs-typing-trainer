//! On-screen physical keyboard, key feedback, finger guide, and heatmap.

use std::collections::{HashMap, HashSet};

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::layout::geometry::{Finger, KeyboardGeometry, PhysicalKey};
use crate::layout::{KeyStroke, KeyboardLayout};
use crate::practice::keyboard_fade::KeyFade;

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

#[derive(Clone, Copy, Debug)]
pub struct KeyboardView<'a> {
    pub show: &'a str,
    pub finger_guide: bool,
}

pub fn render(frame: &mut Frame, area: Rect, geometry: &KeyboardGeometry, layout: &KeyboardLayout, state: &KeyboardState, view: KeyboardView<'_>) {
    render_inner(frame, area, geometry, layout, state, view, false);
}

pub fn render_heatmap(
    frame: &mut Frame,
    area: Rect,
    geometry: &KeyboardGeometry,
    layout: &KeyboardLayout,
    state: &KeyboardState,
    view: KeyboardView<'_>,
) {
    render_inner(frame, area, geometry, layout, state, view, true);
}

fn render_inner(
    frame: &mut Frame,
    area: Rect,
    geometry: &KeyboardGeometry,
    layout: &KeyboardLayout,
    state: &KeyboardState,
    view: KeyboardView<'_>,
    heatmap_only: bool,
) {
    if view.show == "hidden" || area.width == 0 || area.height == 0 {
        return;
    }
    let full_width = geometry
        .rows
        .iter()
        .enumerate()
        .map(|(row_index, row)| row.iter().map(|key| key.width * 5).sum::<u16>() + row_offset(row_index, 5))
        .max()
        .unwrap_or(0);
    let unit = if full_width <= area.width { 5 } else { 3 };
    let guide_rows = u16::from(view.finger_guide);
    let available_rows = area.height.saturating_sub(guide_rows) as usize;

    let keyboard_width = geometry
        .rows
        .iter()
        .enumerate()
        .map(|(row_index, row)| row.iter().map(|key| key.width * unit).sum::<u16>() + row_offset(row_index, unit))
        .max()
        .unwrap_or(0);
    let keyboard_x = area.x + area.width.saturating_sub(keyboard_width) / 2;

    for (row_index, row) in geometry.rows.iter().take(available_rows).enumerate() {
        let row_width = row.iter().map(|key| key.width * unit).sum::<u16>();
        let offset = if row_index + 1 == geometry.rows.len() {
            keyboard_width.saturating_sub(row_width) / 2
        } else {
            row_offset(row_index, unit)
        };
        let mut x = keyboard_x + offset;
        for key in row {
            let width = key.width * unit;
            let rect = Rect::new(x, area.y + row_index as u16, width.min(area.right().saturating_sub(x)), 1);
            let mut label = key_label(key, layout);
            let mut style = base_style(view.show);
            if view.show == "auto" && !is_next_key(key.code, state) {
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
            if rect.width > 0 {
                let label_width = UnicodeWidthStr::width(label.as_str()) as u16;
                let rendered = if label_width > rect.width { "·".to_owned() } else { label };
                frame.render_widget(Paragraph::new(rendered).alignment(Alignment::Center).style(style), rect);
            }
            x = x.saturating_add(width);
        }
    }

    if view.finger_guide && area.height > geometry.rows.len() as u16 {
        render_finger_guide(frame, area, geometry, unit);
    }
}

/// Left-edge stagger in quarter-key units: 0, 1.5, 1.75 and 2.25 keys.
/// ANSI and JIS boards share these left-hand row offsets.
fn row_offset(row_index: usize, unit: u16) -> u16 {
    let quarters = match row_index {
        0 => 0,
        1 => 6,
        2 => 7,
        3 => 9,
        _ => 0,
    };
    (quarters * unit + 2) / 4
}

fn key_label(key: &PhysicalKey, layout: &KeyboardLayout) -> String {
    match key.code {
        "ShiftLeft" | "ShiftRight" => "⇧".to_owned(),
        "Space" => "Space".to_owned(),
        code => layout.key(code).map(|pair| pair.normal.to_string()).unwrap_or_else(|| "·".to_owned()),
    }
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

fn heatmap_color(value: f64) -> Color {
    let value = value.clamp(0.0, 1.0);
    Color::Rgb((40.0 + value * 180.0) as u8, (20.0 * (1.0 - value)) as u8, 20)
}

fn render_finger_guide(frame: &mut Frame, area: Rect, geometry: &KeyboardGeometry, unit: u16) {
    let Some(row) = geometry.rows.get(2) else { return };
    let keyboard_width = geometry
        .rows
        .iter()
        .enumerate()
        .map(|(row_index, row)| row.iter().map(|key| key.width * unit).sum::<u16>() + row_offset(row_index, unit))
        .max()
        .unwrap_or(0);
    let mut x = area.x + area.width.saturating_sub(keyboard_width) / 2 + row_offset(2, unit);
    let y = area.y + area.height - 1;
    for key in row {
        let width = (key.width * unit).min(area.right().saturating_sub(x));
        if width > 0 {
            let rect = Rect::new(x, y, width, 1);
            frame.render_widget(
                Paragraph::new("━".repeat(width as usize)).style(Style::default().fg(finger_color(key.finger))),
                rect,
            );
        }
        x = x.saturating_add(key.width * unit);
    }
}

fn finger_color(finger: Finger) -> Color {
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
