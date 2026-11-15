use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::bench::BenchPart;
use crate::i18n::Msg;
use crate::ui::theme::Theme;

pub fn render(frame: &mut Frame, app: &App) {
    let lang = app.config.general.lang;
    let theme = Theme::from_name(&app.config.ui.theme);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", Msg::BenchSummaryTitle.text(lang)));
    let inner = block.inner(frame.area());
    frame.render_widget(block, frame.area());
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(2)])
        .split(inner);

    let mut lines = vec![Line::from("")];
    for result in &app.bench_results {
        let previous = result
            .comparison
            .previous_wpm_delta
            .map_or_else(|| Msg::InitialResult.text(lang).to_owned(), |value| format!("{value:+.1} WPM"));
        lines.push(Line::from(vec![
            Span::styled(
                format!("{}  ", part_label(result.part, lang)),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("{:>6.1} WPM  {:>5.1}%  ", result.wpm, result.accuracy * 100.0)),
            Span::raw(format!("{} {previous}", Msg::PreviousDelta.text(lang))),
        ]));
        lines.push(Line::from(format!("    {}: {}", Msg::BenchVersion.text(lang), result.id)));
    }
    frame.render_widget(Paragraph::new(lines), chunks[0]);
    frame.render_widget(
        Paragraph::new(Msg::BenchSummaryHint.text(lang))
            .style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center),
        chunks[1],
    );
}

pub(crate) fn part_label(part: BenchPart, lang: crate::i18n::Lang) -> &'static str {
    match part {
        BenchPart::A => Msg::BenchPartA.text(lang),
        BenchPart::B => Msg::BenchPartB.text(lang),
        BenchPart::C => Msg::BenchPartC.text(lang),
        BenchPart::D => Msg::BenchPartD.text(lang),
    }
}
