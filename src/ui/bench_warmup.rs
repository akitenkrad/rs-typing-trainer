use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::i18n::Lang;
use crate::ui::theme::Theme;

pub fn render(frame: &mut Frame, app: &App) {
    let lang = app.config.general.lang;
    let theme = Theme::from_name(&app.config.ui.theme);
    let block = Block::default().borders(Borders::ALL).title(match lang {
        Lang::En => " WARM-UP ",
        Lang::Ja => " 準備運動 ",
    });
    let inner = block.inner(frame.area());
    frame.render_widget(block, frame.area());
    let lines = match lang {
        Lang::En => vec![
            Line::from(""),
            Line::from("Warm-up complete. English words are next"),
            Line::from(""),
            Line::from("[ENTER] Continue"),
        ],
        Lang::Ja => vec![
            Line::from(""),
            Line::from("準備運動はここまで．次は英単語です"),
            Line::from(""),
            Line::from("[ENTER] 続ける"),
        ],
    };
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        inner,
    );
}
