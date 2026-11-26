//! Bilingual glossary browser.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::glossary::{CATEGORY_ORDER, GlossaryTerm, category_name, screen_name};
use crate::i18n::Lang;
use crate::ui::{left_inset, theme::Theme, wrap_display};

/// A semantic row in the glossary index. Keeping this independent of
/// terminal geometry makes category grouping and selection easy to test.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlossaryListRow {
    Category(String),
    Term { term_index: usize, label: String },
}

/// Build the categorized glossary index for `terms`.
pub fn build_list(terms: &[&GlossaryTerm], lang: Lang) -> Vec<GlossaryListRow> {
    let mut rows = Vec::new();
    for category in CATEGORY_ORDER {
        let entries = terms.iter().enumerate().filter(|(_, term)| term.category == category).collect::<Vec<_>>();
        if entries.is_empty() {
            continue;
        }
        if let Some(name) = category_name(category, lang) {
            rows.push(GlossaryListRow::Category(name.to_owned()));
        }
        rows.extend(entries.into_iter().map(|(term_index, term)| GlossaryListRow::Term {
            term_index,
            label: term.term.text(lang).to_owned(),
        }));
    }
    rows
}

/// Build every line in the selected term's detail pane. The caller may style
/// the first line, but wrapping and content are deliberately pure.
pub fn build_detail(term: &GlossaryTerm, lang: Lang, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = wrap_display(term.term.text(lang), width);
    if !term.aliases.is_empty() {
        let label = match lang {
            Lang::En => "Aliases",
            Lang::Ja => "別名",
        };
        lines.extend(wrap_display(&format!("{label}: {}", term.aliases.join(", ")), width));
    }
    lines.push(String::new());
    lines.extend(wrap_display(term.short.text(lang), width));
    if let Some(detail) = &term.detail {
        lines.push(String::new());
        lines.extend(wrap_display(detail.text(lang), width));
    }
    lines.push(String::new());
    let screen_label = match lang {
        Lang::En => "Appears on",
        Lang::Ja => "出てくる画面",
    };
    let separator = if lang == Lang::Ja { "，" } else { ", " };
    let screens = term
        .screens
        .iter()
        .filter_map(|screen| screen_name(screen, lang))
        .collect::<Vec<_>>()
        .join(separator);
    lines.extend(wrap_display(&format!("{screen_label}: {screens}"), width));
    lines
}

pub fn render(frame: &mut Frame, app: &App) {
    let lang = app.config.general.lang;
    let theme = Theme::from_name(&app.config.ui.theme);
    let title = match lang {
        Lang::En => " GLOSSARY ",
        Lang::Ja => " 用語集 ",
    };
    let block = Block::default().borders(Borders::ALL).title(title).style(Style::default().fg(theme.fg));
    let inner = left_inset(block.inner(frame.area()));
    frame.render_widget(block, frame.area());

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0), Constraint::Length(1)])
        .split(inner);

    let terms = app.glossary_visible_terms().unwrap_or_default();
    render_filter(frame, app, terms.len(), lang, theme, rows[0]);
    render_content(frame, app, &terms, lang, theme, rows[1]);

    let footer = match lang {
        Lang::En => "[↑/↓ k/j PgUp/PgDn Home/End] Select   [/] Search   [Esc/q] Back",
        Lang::Ja => "[↑/↓ k/j PgUp/PgDn Home/End] 選ぶ   [/] 検索   [Esc/q] 戻る",
    };
    frame.render_widget(Paragraph::new(footer).style(Style::default().fg(theme.accent)), rows[2]);
}

fn render_filter(frame: &mut Frame, app: &App, count: usize, lang: Lang, theme: Theme, area: Rect) {
    let scope = scope_line(app, count, lang);
    let search = if app.glossary_searching {
        match lang {
            Lang::En => format!("Search: {}█   [Enter] Apply   [Esc] Clear", app.glossary_query),
            Lang::Ja => format!("検索: {}█   [Enter] 確定   [Esc] 消去", app.glossary_query),
        }
    } else if app.glossary_query.is_empty() {
        match lang {
            Lang::En => "Search: —   [/] Type to filter".to_owned(),
            Lang::Ja => "検索: —   [/] 入力して絞り込む".to_owned(),
        }
    } else {
        match lang {
            Lang::En => format!("Search: {}   [/] Edit", app.glossary_query),
            Lang::Ja => format!("検索: {}   [/] 編集", app.glossary_query),
        }
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(scope, Style::default().fg(theme.warning).add_modifier(Modifier::BOLD))),
            Line::from(Span::styled(search, Style::default().fg(theme.accent))),
        ]),
        area,
    );
}

fn scope_line(app: &App, count: usize, lang: Lang) -> String {
    if app.glossary_show_all || app.glossary_context.is_none() {
        let switch = if app.glossary_context.is_some() {
            match (lang, app.glossary_context.is_some_and(|context| context.starts_with("analysis."))) {
                (Lang::En, true) => "   [Tab] This tab",
                (Lang::En, false) => "   [Tab] This screen",
                (Lang::Ja, true) => "   [Tab] このタブ",
                (Lang::Ja, false) => "   [Tab] この画面",
            }
        } else {
            ""
        };
        return match lang {
            Lang::En => format!("All terms: {count}{switch}"),
            Lang::Ja => format!("すべての用語 {count} 件{switch}"),
        };
    }

    match (lang, app.glossary_context.is_some_and(|context| context.starts_with("analysis."))) {
        (Lang::En, true) => format!("Terms on this tab: {count}   [Tab] All terms"),
        (Lang::En, false) => format!("Terms on this screen: {count}   [Tab] All terms"),
        (Lang::Ja, true) => format!("このタブの用語 {count} 件   [Tab] すべて"),
        (Lang::Ja, false) => format!("この画面の用語 {count} 件   [Tab] すべて"),
    }
}

fn render_content(frame: &mut Frame, app: &App, terms: &[&GlossaryTerm], lang: Lang, theme: Theme, area: Rect) {
    let panes = if area.width >= 100 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
            .split(area)
    };
    render_index(frame, terms, app.glossary_selected, lang, theme, panes[0]);
    render_detail(frame, terms, app.glossary_selected, lang, theme, panes[1]);
}

fn render_index(frame: &mut Frame, terms: &[&GlossaryTerm], selected: usize, lang: Lang, theme: Theme, area: Rect) {
    let title = match lang {
        Lang::En => " Terms ",
        Lang::Ja => " 用語 ",
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if terms.is_empty() {
        let message = match lang {
            Lang::En => "No matching terms.",
            Lang::Ja => "当てはまる用語がありません．",
        };
        frame.render_widget(Paragraph::new(message).style(Style::default().fg(theme.muted)), inner);
        return;
    }

    let selected = selected.min(terms.len() - 1);
    let rows = build_list(terms, lang);
    let selected_row = rows
        .iter()
        .position(|row| matches!(row, GlossaryListRow::Term { term_index, .. } if *term_index == selected))
        .unwrap_or(0);
    let height = usize::from(inner.height);
    let offset = selected_row
        .saturating_sub(height.saturating_sub(1))
        .min(rows.len().saturating_sub(height));
    let lines = rows
        .into_iter()
        .skip(offset)
        .take(height)
        .map(|row| match row {
            GlossaryListRow::Category(name) => Line::from(Span::styled(name, Style::default().fg(theme.warning).add_modifier(Modifier::BOLD))),
            GlossaryListRow::Term { term_index, label } => Line::from(vec![
                Span::styled(if term_index == selected { "▶ " } else { "  " }, Style::default().fg(theme.accent)),
                Span::styled(
                    label,
                    Style::default()
                        .fg(if term_index == selected { theme.accent } else { theme.fg })
                        .add_modifier(if term_index == selected { Modifier::BOLD } else { Modifier::empty() }),
                ),
            ]),
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_detail(frame: &mut Frame, terms: &[&GlossaryTerm], selected: usize, lang: Lang, theme: Theme, area: Rect) {
    let title = match lang {
        Lang::En => " Details ",
        Lang::Ja => " 説明 ",
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let Some(term) = terms.get(selected.min(terms.len().saturating_sub(1))) else {
        return;
    };
    let mut detail = build_detail(term, lang, usize::from(inner.width));
    if detail.len() > usize::from(inner.height) {
        detail.retain(|line| !line.is_empty());
    }
    let lines = detail
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            if index == 0 {
                Line::from(Span::styled(line, Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)))
            } else {
                Line::from(line)
            }
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), inner);
}
