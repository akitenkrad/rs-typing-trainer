//! History screen — list of past sessions backed by SQLite.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::app::{App, HistorySort};
use crate::i18n::Msg;
use crate::record::db::SessionSummary;
use crate::ui::theme::Theme;

pub fn render(frame: &mut Frame, app: &App) {
    let theme = Theme::from_name(&app.config.ui.theme);
    let lang = app.config.general.lang;
    let area = frame.area();

    let outer = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", Msg::HistoryTitle.text(lang)))
        .style(Style::default().fg(theme.fg));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0), Constraint::Length(2)])
        .split(inner);

    render_header(frame, app, &theme, chunks[0]);
    render_list(frame, app, &theme, chunks[1]);
    render_hint(frame, app, &theme, chunks[2]);
}

fn render_header(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let lang = app.config.general.lang;
    let lines = vec![
        Line::from(vec![Span::styled(
            format!("{} : {}", Msg::Database.text(lang), app.db_path().display()),
            Style::default().fg(theme.muted),
        )]),
        Line::from(vec![
            Span::styled(
                format!("{} {}   ", app.history.len(), Msg::Sessions.text(lang)),
                Style::default().fg(theme.muted),
            ),
            Span::styled(format!("{}: ", Msg::Sort.text(lang)), Style::default().fg(theme.muted)),
            Span::styled(
                app.history_sort.label(lang),
                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
            ),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_list(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let lang = app.config.general.lang;
    if let Some(err) = &app.history_load_error {
        let p = Paragraph::new(vec![
            Line::from(Span::styled(Msg::FailedToLoadHistory.text(lang), Style::default().fg(theme.mistype))),
            Line::from(Span::styled(err.clone(), Style::default().fg(theme.fg))),
        ])
        .alignment(Alignment::Left);
        frame.render_widget(p, area);
        return;
    }

    if app.history.is_empty() {
        let p = Paragraph::new(Msg::NoPastSessions.text(lang))
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.muted));
        frame.render_widget(p, area);
        return;
    }

    let ranking = app.history_sort.is_ranking();
    let header_text = if ranking {
        format!(
            "  {:>4}  {:<19}  {:>6}  {:>6}  {:>6}  {:>5}  {:>5}",
            Msg::Rank.text(lang),
            Msg::FinishedAt.text(lang),
            Msg::Wpm.text(lang),
            Msg::AccuracyPercent.text(lang),
            Msg::Words.text(lang),
            Msg::Combo.text(lang),
            Msg::Time.text(lang)
        )
    } else {
        format!(
            "  {:<19}  {:>6}  {:>6}  {:>6}  {:>5}  {:>5}",
            Msg::FinishedAt.text(lang),
            Msg::Wpm.text(lang),
            Msg::AccuracyPercent.text(lang),
            Msg::Words.text(lang),
            Msg::Combo.text(lang),
            Msg::Time.text(lang)
        )
    };
    let header_line = Line::from(Span::styled(header_text, Style::default().fg(theme.muted).add_modifier(Modifier::BOLD)));

    let items: Vec<ListItem> = app
        .history
        .iter()
        .enumerate()
        .map(|(i, s)| ListItem::new(format_summary_line(s, theme, if ranking { Some(i + 1) } else { None }, app.history_sort)))
        .collect();

    let list_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);

    frame.render_widget(Paragraph::new(header_line), list_area[0]);

    let list = List::new(items)
        .highlight_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD | Modifier::REVERSED))
        .highlight_symbol("▶ ");

    let mut state = ListState::default();
    state.select(Some(app.history_selected));
    frame.render_stateful_widget(list, list_area[1], &mut state);
}

fn format_summary_line<'a>(s: &SessionSummary, theme: &Theme, rank: Option<usize>, sort: HistorySort) -> Line<'a> {
    let mut spans: Vec<Span<'a>> = Vec::with_capacity(8);
    if let Some(r) = rank {
        spans.push(Span::styled(
            format!("  {:>4}", format!("#{r}")),
            Style::default().fg(theme.warning).add_modifier(Modifier::BOLD),
        ));
    }
    spans.push(Span::styled(
        format!("  {}", s.finished_at.format("%Y-%m-%d %H:%M:%S")),
        Style::default().fg(theme.fg),
    ));
    spans.push(Span::styled(
        format!("  {:>6.1}", s.wpm),
        column_style(theme, sort, HistorySort::WpmDesc, theme.accent),
    ));
    spans.push(Span::styled(
        format!("  {:>5.1}%", s.accuracy * 100.0),
        column_style(theme, sort, HistorySort::AccuracyDesc, theme.correct),
    ));
    spans.push(Span::styled(
        format!("  {:>6}", s.completed_words),
        column_style(theme, sort, HistorySort::WordsDesc, theme.fg),
    ));
    spans.push(Span::styled(
        format!("  {:>5}", s.max_combo),
        column_style(theme, sort, HistorySort::MaxComboDesc, theme.fg),
    ));
    spans.push(Span::styled(format!("  {:>4}s", s.duration.as_secs()), Style::default().fg(theme.muted)));
    Line::from(spans)
}

fn column_style(_theme: &Theme, current: HistorySort, column_sort: HistorySort, base: ratatui::style::Color) -> Style {
    if current == column_sort {
        Style::default().fg(base).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(base)
    }
}

fn render_hint(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let message = if app.history.is_empty() {
        Msg::HistoryEmptyHint
    } else {
        Msg::HistoryHint
    };
    let p = Paragraph::new(message.text(app.config.general.lang))
        .style(Style::default().fg(theme.accent))
        .alignment(Alignment::Center);
    frame.render_widget(p, area);
}
