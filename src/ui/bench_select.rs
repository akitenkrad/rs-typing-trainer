use ratatui::Frame;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::i18n::Msg;
use crate::ui::{left_inset, theme::Theme};

pub fn render(frame: &mut Frame, app: &App) {
    let lang = app.config.general.lang;
    let theme = Theme::from_name(&app.config.ui.theme);
    let optional = if app.bench_include_d {
        Msg::BenchOptionalOn.text(lang)
    } else {
        Msg::BenchOptionalOff.text(lang)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", Msg::BenchSelectTitle.text(lang)));
    let inner = block.inner(frame.area());
    frame.render_widget(block, frame.area());
    let start = match lang {
        crate::i18n::Lang::En => "Start benchmark  [Enter]",
        crate::i18n::Lang::Ja => "ベンチマーク開始  [Enter]",
    };
    let items = [start, optional];
    let mut lines = vec![Line::from(""), Line::from(Msg::BenchRequiredParts.text(lang)), Line::from("")];
    lines.extend(items.iter().enumerate().map(|(index, label)| {
        let selected = index == app.bench_selected;
        Line::from(vec![
            Span::styled(if selected { "▶  " } else { "   " }, Style::default().fg(theme.accent)),
            Span::styled(
                *label,
                Style::default()
                    .fg(if selected { theme.warning } else { theme.fg })
                    .add_modifier(if selected { Modifier::BOLD } else { Modifier::empty() }),
            ),
        ])
    }));
    lines.push(Line::from(""));
    lines.push(Line::from(Msg::BenchSelectHint.text(lang)).style(Style::default().fg(theme.accent)));
    frame.render_widget(Paragraph::new(lines), left_inset(inner));
}
