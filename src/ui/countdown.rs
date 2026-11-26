//! Countdown screen.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::i18n::Msg;
use crate::ui::theme::Theme;

/// `remaining` is in seconds, counting down from 3.0 to 0.0.
pub fn render(frame: &mut Frame, app: &App, remaining: f32) {
    let theme = Theme::from_name(&app.config.ui.theme);
    let lang = app.config.general.lang;
    let area = frame.area();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", Msg::CountdownTitle.text(lang)))
        .style(Style::default().fg(theme.fg));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Today's training (MYTASK-3627): the whole menu and where this step is,
    // two rows below the hint. Nothing for a session started on its own.
    let band = crate::ui::menu::progress_band_for_start(app, usize::from(inner.width));
    let band_rows = if band.is_some() { 2 } else { 0 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(7),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(band_rows),
            Constraint::Min(0),
        ])
        .split(inner);

    let label = if remaining > 2.0 {
        "3"
    } else if remaining > 1.0 {
        "2"
    } else if remaining > 0.0 {
        "1"
    } else {
        Msg::CountdownGo.text(lang)
    };

    let big = render_big_number(label);
    let big_lines: Vec<Line> = big
        .lines()
        .map(|l| Line::styled(l.to_string(), Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)))
        .collect();
    let para = Paragraph::new(big_lines).alignment(Alignment::Center);
    frame.render_widget(para, chunks[1]);

    if app.active_bench_v3_part() == Some(crate::bench::BenchV3Part::W) {
        let label = match lang {
            crate::i18n::Lang::En => "Warm-up (not scored)",
            crate::i18n::Lang::Ja => "準備運動（採点しません）",
        };
        frame.render_widget(
            Paragraph::new(label)
                .alignment(Alignment::Center)
                .style(Style::default().fg(theme.warning).add_modifier(Modifier::BOLD)),
            chunks[2],
        );
    }

    let (hint_text, hint_color) = if app.abort_confirmation_active() {
        (Msg::AbortConfirmHint.text(lang), theme.warning)
    } else {
        (Msg::CountdownHint.text(lang), theme.muted)
    };
    let hint = Paragraph::new(hint_text)
        .alignment(Alignment::Center)
        .style(Style::default().fg(hint_color));
    frame.render_widget(hint, chunks[3]);
    if let Some(band) = band {
        let row = Rect::new(chunks[4].x, chunks[4].y + 1, chunks[4].width, 1);
        frame.render_widget(
            Paragraph::new(band).alignment(Alignment::Center).style(Style::default().fg(theme.accent)),
            row,
        );
    }
}

fn render_big_number(s: &str) -> String {
    match s {
        "3" => ["  ┌───────┐  ", "  │       │  ", "  │   3   │  ", "  │       │  ", "  └───────┘  "].join("\n"),
        "2" => ["  ┌───────┐  ", "  │       │  ", "  │   2   │  ", "  │       │  ", "  └───────┘  "].join("\n"),
        "1" => ["  ┌───────┐  ", "  │       │  ", "  │   1   │  ", "  │       │  ", "  └───────┘  "].join("\n"),
        _ => format!("  ╔═══════╗  \n  ║       ║  \n  ║{:^7}║  \n  ║       ║  \n  ╚═══════╝  ", s),
    }
}
