//! TUI rendering. The top-level `render` function dispatches to per-screen
//! renderers based on the current `Screen` value.

pub mod about;
pub mod analysis;
pub mod analysis_image;
pub mod animation;
pub mod bench_select;
pub mod bench_summary;
pub mod bench_tab_summary;
pub mod bigtext;
pub mod bigtext_image;
pub mod briefing;
pub mod countdown;
pub mod glossary;
pub mod history;
mod image_cache;
pub mod key_picker;
pub mod keyboard;
pub mod keyboard_image;
pub mod live_goal;
pub mod menu;
pub mod menu_summary;
pub mod mode_select;
pub mod playing;
pub mod prompt;
pub mod reading_reports;
pub mod replay;
pub mod result;
pub mod result_image;
pub mod review;
pub mod settings;
pub mod theme;
pub mod title;
pub mod title_image;
pub mod today;
pub mod today_summary;

use ratatui::Frame;
use ratatui::layout::Rect;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, Screen};

pub fn render(frame: &mut Frame, app: &App) {
    match app.screen {
        Screen::Title => title::render(frame, app),
        Screen::About => about::render(frame, app),
        Screen::Menu => menu::render(frame, app),
        Screen::Briefing => briefing::render(frame, app),
        Screen::Glossary => glossary::render(frame, app),
        Screen::MenuSummary => menu_summary::render(frame, app),
        Screen::ModeSelect => mode_select::render(frame, app),
        Screen::KeyPicker => key_picker::render(frame, app),
        Screen::Countdown { remaining } => countdown::render(frame, app, remaining),
        Screen::Playing => playing::render(frame, app),
        Screen::Result | Screen::HistoryDetail => result::render(frame, app),
        Screen::Review => review::render(frame, app),
        Screen::Replay => {
            if let Some(replay) = app.history_replay.as_deref() {
                replay::render(frame, replay, app.config.general.lang);
            }
        }
        Screen::History => history::render(frame, app),
        Screen::ReadingReports => reading_reports::render(frame, app),
        Screen::Settings => settings::render(frame, app),
        Screen::BenchSelect => bench_select::render(frame, app),
        Screen::BenchSummary => bench_summary::render(frame, app),
        Screen::Analysis => analysis::render(frame, app),
    }
}

pub(crate) fn left_inset(area: Rect) -> Rect {
    Rect::new(area.x.saturating_add(2), area.y, area.width.saturating_sub(4), area.height)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WrappedFieldLine {
    pub label: String,
    pub value: String,
}

/// Wrap text by terminal-cell width. A line is filled to the last character
/// that fits; only an overflowing ASCII word backs up to its preceding space.
pub(crate) fn wrap_display(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let characters = text.chars().collect::<Vec<_>>();
    let mut output = Vec::new();
    let mut start = 0;
    while start < characters.len() {
        while start < characters.len() && characters[start].is_whitespace() {
            start += 1;
        }
        if start >= characters.len() {
            break;
        }

        let mut used = 0;
        let mut end = start;
        while end < characters.len() {
            let character = characters[end];
            if character == '\n' {
                break;
            }
            let character_width = character.width().unwrap_or(0);
            if end > start && used + character_width > width {
                break;
            }
            used += character_width;
            end += 1;
        }
        if end == start {
            end += 1;
        }

        let mut cut = end;
        if end < characters.len() && characters[end] != '\n' {
            let overflow = characters[end];
            let previous = characters[end - 1];
            // Trailing ASCII punctuation belongs to its word, so «sessions.»
            // moves to the next row whole instead of leaving a lone «.».
            if previous.is_ascii_alphanumeric()
                && (overflow.is_ascii_alphanumeric() || matches!(overflow, '.' | ',' | ';' | ':' | '!' | '?' | ')'))
                && let Some(space) = characters[start..end].iter().rposition(|character| character.is_whitespace())
                && space > 0
            {
                cut = start + space;
            } else if matches!(overflow, '，' | '．' | '）' | '」') && end.saturating_sub(start) > 1 {
                cut = end - 1;
            }
        }

        let line = characters[start..cut].iter().collect::<String>();
        output.push(line.trim_end().to_owned());
        start = cut;
        if start < characters.len() && characters[start] == '\n' {
            start += 1;
        }
    }
    if output.is_empty() {
        output.push(String::new());
    }
    output
}

pub(crate) fn wrap_field(label: &str, value: &str, label_width: usize, width: usize) -> Vec<WrappedFieldLine> {
    let indent = label_width.saturating_add(1);
    let value_width = width.saturating_sub(indent).max(1);
    wrap_display(value, value_width)
        .into_iter()
        .enumerate()
        .map(|(index, value)| WrappedFieldLine {
            label: if index == 0 {
                format!("{}{} ", label, " ".repeat(label_width.saturating_sub(UnicodeWidthStr::width(label))))
            } else {
                " ".repeat(indent)
            },
            value,
        })
        .collect()
}
