//! Reading-report list and removal confirmation.

use std::path::PathBuf;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, HighlightSpacing, List, ListItem, ListState, Paragraph};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::App;
use crate::i18n::Lang;
use crate::practice::reading_report::{ReadingReportEntry, ReadingReportKind};
use crate::ui::{left_inset, theme::Theme, wrap_field};

pub fn render(frame: &mut Frame, app: &App) {
    let theme = Theme::from_name(&app.config.ui.theme);
    let lang = app.config.general.lang;
    let area = frame.area();
    let title = match lang {
        Lang::En => " Reading reports ",
        Lang::Ja => " 読みの報告 ",
    };
    let outer = Block::default().borders(Borders::ALL).title(title).style(Style::default().fg(theme.fg));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    if inner.height < 2 {
        return;
    }
    let body = Rect::new(inner.x, inner.y, inner.width, inner.height.saturating_sub(2));
    let path_area = Rect::new(inner.x, inner.bottom().saturating_sub(2), inner.width, 1);
    let hint_area = Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1);
    render_body(frame, app, &theme, left_inset(body));
    render_path(frame, app, &theme, left_inset(path_area));
    render_hint(frame, app, &theme, left_inset(hint_area));
}

fn render_body(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let lang = app.config.general.lang;
    let detail_lines = selected_report_lines(app, usize::from(area.width));
    let status = status_text(app, lang);
    let status_height = u16::from(status.is_some()) * 2;
    let minimum_list_height = 4;
    let detail_height = u16::try_from(detail_lines.len())
        .unwrap_or(u16::MAX)
        .min(area.height.saturating_sub(1 + status_height + minimum_list_height));
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(minimum_list_height),
            Constraint::Length(detail_height),
            Constraint::Length(status_height),
        ])
        .split(area);

    let summary = match lang {
        Lang::En => format!("{} reports. These items are left out of future prompts.", app.reading_reports.len()),
        Lang::Ja => format!("報告 {} 件．これらは次回から出題しません", app.reading_reports.len()),
    };
    frame.render_widget(
        Paragraph::new(summary).style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        chunks[0],
    );

    if let Some(error) = &app.reading_reports_error {
        frame.render_widget(Paragraph::new(error.as_str()).style(Style::default().fg(theme.mistype)), chunks[1]);
    } else if app.reading_reports.is_empty() {
        let empty = match lang {
            Lang::En => {
                "There are no reading reports yet. When a Japanese word or sentence has the wrong reading, press Ctrl+R (or F2) while typing to report it."
            }
            Lang::Ja => "まだ報告はありません．日本語の単語や短文で読みが違うときは，打っている間に Ctrl+R（または F2）で報告できます．",
        };
        frame.render_widget(
            Paragraph::new(empty)
                .style(Style::default().fg(theme.muted))
                .wrap(ratatui::widgets::Wrap { trim: true }),
            chunks[1],
        );
    } else {
        render_list(frame, app, theme, chunks[1]);
    }

    if detail_height > 0 {
        let detail_lines = fit_lines(detail_lines, usize::from(detail_height), usize::from(chunks[2].width));
        frame.render_widget(Paragraph::new(detail_lines).style(Style::default().fg(theme.fg)), chunks[2]);
    }

    if let Some(status) = status {
        let color = if app.reading_reports_confirming { theme.warning } else { theme.correct };
        frame.render_widget(
            Paragraph::new(status)
                .style(Style::default().fg(color).add_modifier(Modifier::BOLD))
                .wrap(ratatui::widgets::Wrap { trim: true }),
            chunks[3],
        );
    }
}

fn render_list(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let lang = app.config.general.lang;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);
    let columns = Columns::for_width(area.width);
    frame.render_widget(
        Paragraph::new(format!("  {}", columns.header(lang))).style(Style::default().fg(theme.muted).add_modifier(Modifier::BOLD)),
        chunks[0],
    );
    let items = app
        .reading_reports
        .iter()
        .map(|report| ListItem::new(columns.row(report, lang)))
        .collect::<Vec<_>>();
    let list = List::new(items)
        .highlight_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD | Modifier::REVERSED))
        .highlight_symbol("▶ ")
        .highlight_spacing(HighlightSpacing::Always);
    let mut state = ListState::default();
    state.select(Some(app.reading_reports_selected));
    frame.render_stateful_widget(list, chunks[1], &mut state);
}

#[derive(Clone, Copy)]
struct Columns {
    width: usize,
    text: usize,
    reading: usize,
}

impl Columns {
    fn for_width(width: u16) -> Self {
        let width = usize::from(width.saturating_sub(2));
        let variable = width.saturating_sub(34);
        let text = variable / 2;
        let reading = variable.saturating_sub(text);
        Self {
            width,
            text: text.max(1),
            reading: reading.max(1),
        }
    }

    fn header(self, lang: Lang) -> String {
        let (date, kind, text, reading) = match lang {
            Lang::En => ("Date", "Type", "Text", "Reading"),
            Lang::Ja => ("日時", "種類", "表記", "読み"),
        };
        self.fields(date, kind, text, reading)
    }

    fn row(self, report: &ReadingReportEntry, lang: Lang) -> String {
        let date = report.latest_at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string();
        let kind = match (report.kind, lang) {
            (ReadingReportKind::Romaji, Lang::En) => "Word",
            (ReadingReportKind::Romaji, Lang::Ja) => "単語",
            (ReadingReportKind::Sentence, Lang::En) => "Sentence",
            (ReadingReportKind::Sentence, Lang::Ja) => "短文",
        };
        let kind = if report.count > 1 {
            format!("{kind} ×{}", report.count)
        } else {
            kind.to_owned()
        };
        self.fields(&date, &kind, &report.text, &report.reading)
    }

    fn fields(self, date: &str, kind: &str, text: &str, reading: &str) -> String {
        truncate_display(
            &format!(
                "{}  {}  {}  {}",
                pad_display(date, 16),
                pad_display(kind, 12),
                pad_display(text, self.text),
                pad_display(reading, self.reading),
            ),
            self.width,
        )
    }
}

fn render_path(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let label = match app.config.general.lang {
        Lang::En => "File",
        Lang::Ja => "ファイル",
    };
    let path = display_report_path(app);
    frame.render_widget(
        Paragraph::new(truncate_display(&format!("{label}: {path}"), usize::from(area.width))).style(Style::default().fg(theme.muted)),
        area,
    );
}

fn render_hint(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let hint = match (app.reading_reports_confirming, app.config.general.lang) {
        (true, Lang::En) => "[Y] Remove    [N] Cancel",
        (true, Lang::Ja) => "[Y] 取り消す    [N] やめる",
        (false, Lang::En) => "[↑/↓] Select    [D] Remove    [Esc] Back",
        (false, Lang::Ja) => "[↑/↓] 選ぶ    [D] 取り消す    [Esc] 戻る",
    };
    frame.render_widget(Paragraph::new(hint).style(Style::default().fg(theme.accent)), area);
}

fn selected_report_lines(app: &App, width: usize) -> Vec<String> {
    let Some(report) = app.reading_reports.get(app.reading_reports_selected) else {
        return Vec::new();
    };
    let (text_label, reading_label) = match app.config.general.lang {
        Lang::En => ("Text:", "Reading:"),
        Lang::Ja => ("表記:", "読み:"),
    };
    let label_width = UnicodeWidthStr::width(text_label).max(UnicodeWidthStr::width(reading_label));
    [
        wrap_field(text_label, &report.text, label_width, width),
        wrap_field(reading_label, &report.reading, label_width, width),
    ]
    .into_iter()
    .flatten()
    .map(|line| format!("{}{}", line.label, line.value))
    .collect()
}

fn fit_lines(mut lines: Vec<String>, height: usize, width: usize) -> Vec<Line<'static>> {
    if lines.len() > height {
        lines.truncate(height);
        if let Some(last) = lines.last_mut() {
            *last = force_ellipsis(last, width);
        }
    }
    lines.into_iter().map(Line::from).collect()
}

fn force_ellipsis(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let target = width.saturating_sub(1);
    let mut used = 0;
    let mut output = String::new();
    for character in value.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if used + character_width > target {
            break;
        }
        output.push(character);
        used += character_width;
    }
    output.push('…');
    output
}

fn status_text(app: &App, lang: Lang) -> Option<String> {
    if app.reading_reports_confirming {
        Some(
            match lang {
                Lang::En => "Remove this report? It will appear in prompts again the next time you start practicing.",
                Lang::Ja => "この報告を取り消しますか？取り消すと，次に練習を始めたときから再び出題されます．",
            }
            .to_owned(),
        )
    } else {
        app.reading_reports_notice.clone()
    }
}

fn display_report_path(app: &App) -> String {
    let db_path = app.db_path();
    let directory = db_path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let path = directory.join("reading_reports.tsv");
    shorten_home(path, &app.home_dir)
}

fn shorten_home(path: PathBuf, home: &std::path::Path) -> String {
    match path.strip_prefix(home) {
        Ok(relative) if relative.as_os_str().is_empty() => "~".to_owned(),
        Ok(relative) => format!("~/{}", relative.display()),
        Err(_) => path.display().to_string(),
    }
}

fn pad_display(value: &str, width: usize) -> String {
    let value = truncate_display(value, width);
    let padding = " ".repeat(width.saturating_sub(UnicodeWidthStr::width(value.as_str())));
    format!("{value}{padding}")
}

fn truncate_display(value: &str, width: usize) -> String {
    if UnicodeWidthStr::width(value) <= width {
        return value.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let target = width.saturating_sub(1);
    let mut used = 0;
    let mut output = String::new();
    for character in value.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if used + character_width > target {
            break;
        }
        output.push(character);
        used += character_width;
    }
    output.push('…');
    output
}
