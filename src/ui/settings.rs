//! Interactive keyboard and language settings screen.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::i18n::{Lang, Msg};
use crate::ui::theme::Theme;

pub fn render(frame: &mut Frame, app: &App) {
    let theme = Theme::from_name(&app.config.ui.theme);
    let lang = app.config.general.lang;
    let area = frame.area();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", Msg::SettingsTitle.text(lang)))
        .style(Style::default().fg(theme.fg));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Length(10), Constraint::Min(0), Constraint::Length(2)])
        .split(inner);

    let language = match lang {
        Lang::En => "English",
        Lang::Ja => "日本語",
    };
    let rows = [
        (Msg::SettingLayout, app.config.keyboard.layout.as_str()),
        (Msg::SettingInputLayout, app.config.keyboard.input_layout.as_str()),
        (Msg::SettingGeometry, app.config.keyboard.geometry.as_str()),
        (Msg::SettingShow, app.config.keyboard.show.as_str()),
        (Msg::SettingLanguage, language),
        (
            Msg::SettingErrorMode,
            match app.config.session.error_mode {
                crate::config::ErrorMode::Stop => "stop",
                crate::config::ErrorMode::Advance => "advance",
            },
        ),
    ];
    let label_width = rows.iter().map(|(label, _)| UnicodeWidthStr::width(label.text(lang))).max().unwrap_or(0) + 3;
    let lines = rows
        .iter()
        .enumerate()
        .map(|(index, (label, value))| {
            let selected = index == app.settings_selected;
            Line::from(vec![
                Span::styled(if selected { "▶ " } else { "  " }, Style::default().fg(theme.accent)),
                Span::styled(
                    format!(
                        "{}{}",
                        label.text(lang),
                        " ".repeat(label_width.saturating_sub(UnicodeWidthStr::width(label.text(lang))))
                    ),
                    Style::default().fg(if selected { theme.accent } else { theme.fg }),
                ),
                Span::styled(
                    format!("< {value} >"),
                    Style::default()
                        .fg(if selected { theme.warning } else { theme.muted })
                        .add_modifier(if selected { Modifier::BOLD } else { Modifier::empty() }),
                ),
            ])
        })
        .collect::<Vec<_>>();
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Fill(1), Constraint::Length(52), Constraint::Fill(1)])
        .split(chunks[1]);
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Left), columns[1]);
    if let Some(error) = &app.settings_save_error {
        frame.render_widget(
            Paragraph::new(error.clone())
                .alignment(Alignment::Center)
                .style(Style::default().fg(theme.mistype)),
            chunks[2],
        );
    }
    frame.render_widget(
        Paragraph::new(Msg::SettingsHint.text(lang))
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.accent)),
        chunks[3],
    );
}
