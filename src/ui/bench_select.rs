use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::i18n::Msg;
use crate::ui::theme::Theme;

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
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(""),
            Line::from(Msg::BenchRequiredParts.text(lang)),
            Line::from(""),
            Line::from(optional).style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
            Line::from(""),
            Line::from(Msg::BenchSelectHint.text(lang)),
        ])
        .alignment(Alignment::Center),
        inner,
    );
}
