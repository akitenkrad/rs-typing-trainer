//! Keyboard-driven practice-key selection.

use std::collections::HashMap;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::config::KeyFilter;
use crate::i18n::Lang;
use crate::layout::KeyStroke;
use crate::layout::geometry::geometry;
use crate::ui::keyboard::{KeyboardState, KeyboardView, resolve_style};
use crate::ui::{left_inset, theme::Theme};

pub fn render(frame: &mut Frame, app: &App) {
    let theme = Theme::from_name(&app.config.ui.theme);
    let area = frame.area();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(match app.config.general.lang {
            Lang::En => " KEY SELECT ",
            Lang::Ja => " キー選択 ",
        })
        .style(Style::default().fg(theme.fg));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let keyboard_room = inner.height.saturating_sub(6);
    let keyboard_rows = resolve_style(&app.config.keyboard.style, Rect::new(inner.x, inner.y, inner.width, keyboard_room), false)
        .map_or(0, |style| style.height(false));
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(keyboard_rows),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Fill(1),
        ])
        .split(inner);
    let filter = match app.config.mode.key_filter {
        KeyFilter::Include => "include",
        KeyFilter::Only => "only",
    };
    let summary = match app.config.general.lang {
        Lang::En => format!("Selected: {}    Filter: {filter}", app.config.mode.keys),
        Lang::Ja => format!("選択: {}    絞り込み: {filter}", app.config.mode.keys),
    };
    frame.render_widget(Paragraph::new(summary), left_inset(chunks[0]));

    if let Some(geometry) = geometry(&app.config.keyboard.geometry) {
        let layout = app.practice_layout();
        let heatmap = app
            .config
            .mode
            .keys
            .chars()
            .filter_map(|character| layout.stroke_for(character).map(|stroke| (stroke.code, 0.55)))
            .collect::<HashMap<_, _>>();
        let next = app.picker_code().map(|code| KeyStroke { code, shift: false });
        let state = KeyboardState {
            next,
            last: None,
            focus: None,
            heatmap,
            fade: Default::default(),
            unlock: None,
        };
        crate::ui::keyboard::render_with_images(
            frame,
            chunks[1],
            &geometry,
            layout,
            &state,
            KeyboardView {
                show: "full",
                style: &app.config.keyboard.style,
                finger_guide: false,
            },
            crate::ui::keyboard::KeyboardImageContext {
                detection: &app.graphics,
                cache: &app.keyboard_images,
            },
        );
    }
    let hint = match app.config.general.lang {
        Lang::En => "[Arrows] Move    [Space] Toggle key    [Tab] include / only",
        Lang::Ja => "[矢印] 移動    [Space] キー切替    [Tab] include / only",
    };
    frame.render_widget(Paragraph::new(hint).style(Style::default().fg(theme.accent)), left_inset(chunks[2]));
    let enter = match app.config.general.lang {
        Lang::En => "[Enter] Start    [Esc] Back",
        Lang::Ja => "[Enter] 開始    [Esc] 戻る",
    };
    frame.render_widget(Paragraph::new(enter), left_inset(chunks[3]));
    if let Some(warning) = &app.mode_warning {
        frame.render_widget(
            Paragraph::new(warning.clone()).style(Style::default().fg(theme.warning)),
            left_inset(chunks[4]),
        );
    }
}
