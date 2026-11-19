//! TUI rendering. The top-level `render` function dispatches to per-screen
//! renderers based on the current `Screen` value.

pub mod analysis;
pub mod analysis_image;
pub mod animation;
pub mod bench_select;
pub mod bench_summary;
pub mod bigtext;
pub mod bigtext_image;
pub mod countdown;
pub mod history;
mod image_cache;
pub mod key_picker;
pub mod keyboard;
pub mod keyboard_image;
pub mod menu;
pub mod menu_summary;
pub mod mode_select;
pub mod playing;
pub mod prompt;
pub mod replay;
pub mod result;
pub mod result_image;
pub mod settings;
pub mod theme;
pub mod title;
pub mod title_image;
pub mod today;

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::app::{App, Screen};

pub fn render(frame: &mut Frame, app: &App) {
    match app.screen {
        Screen::Title => title::render(frame, app),
        Screen::Menu => menu::render(frame, app),
        Screen::MenuFocus { remaining } => menu::render_focus(frame, app, remaining),
        Screen::MenuSummary => menu_summary::render(frame, app),
        Screen::ModeSelect => mode_select::render(frame, app),
        Screen::KeyPicker => key_picker::render(frame, app),
        Screen::Countdown { remaining } => countdown::render(frame, app, remaining),
        Screen::Playing => playing::render(frame, app),
        Screen::Result | Screen::HistoryDetail => result::render(frame, app),
        Screen::Replay => {
            if let Some(replay) = app.history_replay.as_deref() {
                replay::render(frame, replay, app.config.general.lang);
            }
        }
        Screen::History => history::render(frame, app),
        Screen::Settings => settings::render(frame, app),
        Screen::BenchSelect => bench_select::render(frame, app),
        Screen::BenchSummary => bench_summary::render(frame, app),
        Screen::Analysis => analysis::render(frame, app),
    }
}

pub(crate) fn left_inset(area: Rect) -> Rect {
    Rect::new(area.x.saturating_add(2), area.y, area.width.saturating_sub(4), area.height)
}
