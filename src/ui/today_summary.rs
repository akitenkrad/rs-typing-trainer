//! «Summary»: the screenshot-friendly screen of the «today» tab
//! (MYTASK-3632), toggled with `S`.
//!
//! A heading band, a decorated frame with today's net WPM (median) in very
//! large figures — one per language practised — supporting figures with the
//! difference from the last practice day, and a dark band at the bottom with
//! the day's events. Drawn as one terminal image when available, otherwise
//! with the bitmap big text (`ui::bigtext`), and with plain text when even
//! that does not fit.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui_image::Image;
use unicode_width::UnicodeWidthStr;

use crate::analysis::grades::Direction;
use crate::app::App;
use crate::graphics::GraphicsMode;
use crate::i18n::Lang;
use crate::practice::menu::progress::RunChange;
use crate::ui::analysis_image::{SummaryFigure, TodaySummarySpec};
use crate::ui::bigtext::{BigTextSize, StyledCharacter};
use crate::ui::menu_summary::{difference_span, truncate};
use crate::ui::theme::Theme;
use crate::ui::today::{TodayData, grouped, image_colors, language_name, minutes, previous_delta_text, round_to};

/// Everything the Summary shows, in display strings.
#[derive(Clone, Debug, PartialEq)]
pub struct SummaryView {
    pub heading: String,
    pub subheading: String,
    pub mains: Vec<SummaryFigure>,
    pub details: Vec<SummaryFigure>,
    pub events: Vec<String>,
    /// Without events on a day with a menu: the menu's steps (short name,
    /// done), shown as `✓ウォームアップ → ✓遷移 → 遷移 → …` in the bottom band.
    pub chain: Vec<(String, bool)>,
    /// The first event is a level-up or a graduation.
    pub highlight: bool,
}

/// The day's events in priority order: level-ups, graduations, swaps.
fn events(data: &TodayData, lang: Lang) -> (Vec<String>, bool) {
    let changes = data.runs.iter().flat_map(|run| run.steps.changes.iter()).collect::<Vec<_>>();
    let mut events = Vec::new();
    for change in &changes {
        if let RunChange::LevelUp { title, level, .. } = change {
            events.push(match lang {
                Lang::Ja => format!("{title}がレベル {level} へ"),
                Lang::En => format!("{title}: up to level {level}"),
            });
        }
    }
    for change in &changes {
        if let RunChange::Maintenance { title, .. } = change {
            events.push(match lang {
                Lang::Ja => format!("{title}を卒業（維持へ）"),
                Lang::En => format!("{title} graduated (maintenance)"),
            });
        }
    }
    let highlight = !events.is_empty();
    for change in &changes {
        if let RunChange::Swapped { title, alternate, .. } = change {
            events.push(match lang {
                Lang::Ja => format!("{title}を{alternate}へ入れ替え"),
                Lang::En => format!("{title} swapped for {alternate}"),
            });
        }
    }
    events.truncate(2);
    if events.is_empty() {
        let line = match (data.totals.as_ref(), data.runs.first(), lang) {
            // A day with a menu shows its steps instead (`SummaryView::chain`).
            (Some(_), Some(_), _) => None,
            (None, _, Lang::Ja) => Some("今日の記録はまだありません".to_owned()),
            (None, _, Lang::En) => Some("No practice recorded today yet".to_owned()),
            (Some(totals), None, Lang::Ja) => Some(format!("今日も練習しました（{} 回）", totals.sessions)),
            (Some(totals), None, Lang::En) => Some(format!("Practiced today ({} sessions)", totals.sessions)),
        };
        events.extend(line);
    }
    (events, highlight)
}

pub fn summary_view(data: &TodayData, lang: Lang) -> SummaryView {
    let heading = match lang {
        Lang::Ja => format!("今日の練習 {}", data.date),
        Lang::En => format!("Today's practice {}", data.date),
    };
    let subheading = match (data.runs.first(), lang) {
        (Some(run), Lang::Ja) => format!(
            "{} 分メニュー・{} 段中 {} 段",
            run.steps.minutes,
            run.steps.steps.len(),
            run.steps.completed()
        ),
        (Some(run), Lang::En) => format!(
            "{}-min menu · {} of {} steps",
            run.steps.minutes,
            run.steps.completed(),
            run.steps.steps.len()
        ),
        (None, Lang::Ja) => "個別の練習".to_owned(),
        (None, Lang::En) => "Individual practice".to_owned(),
    };
    let previous = data.baseline.as_ref();
    let figure = |label: String, value: String, delta: Option<f64>, digits: usize, suffix: &str| SummaryFigure {
        label,
        value,
        delta_text: previous_delta_text(data, delta, digits, suffix, lang),
        delta: delta.map(|delta| round_to(delta, digits)),
        average: String::new(),
    };
    let mut mains = Vec::new();
    let mut details = Vec::new();
    if let Some(totals) = &data.totals {
        let two = totals.languages.len() > 1;
        for row in &totals.languages {
            let before = previous.and_then(|previous| previous.language(row.language));
            let mut main = figure(
                language_name(row.language, lang).to_owned(),
                format!("{:.1}", row.median_wpm),
                before.map(|before| row.median_wpm - before.median_wpm),
                1,
                "",
            );
            main.average = before
                .map(|before| match lang {
                    Lang::Ja => format!("平均 {:.1}", before.median_wpm),
                    Lang::En => format!("avg {:.1}", before.median_wpm),
                })
                .unwrap_or_default();
            mains.push(main);
        }
        for row in &totals.languages {
            let before = previous.and_then(|previous| previous.language(row.language));
            let label = match (two, lang) {
                (false, Lang::Ja) => "正確さ".to_owned(),
                (false, Lang::En) => "Accuracy".to_owned(),
                (true, Lang::Ja) => format!("正確さ {}", language_name(row.language, lang)),
                (true, Lang::En) => format!("Accuracy {}", language_name(row.language, lang)),
            };
            details.push(figure(
                label,
                format!("{:.1}%", row.median_accuracy * 100.0),
                before.map(|before| (row.median_accuracy - before.median_accuracy) * 100.0),
                1,
                "pt",
            ));
        }
        details.push(figure(
            match lang {
                Lang::Ja => "練習時間".to_owned(),
                Lang::En => "Time".to_owned(),
            },
            match lang {
                Lang::Ja => format!("{:.1} 分", minutes(totals.duration_ms)),
                Lang::En => format!("{:.1} min", minutes(totals.duration_ms)),
            },
            previous.map(|previous| minutes(totals.duration_ms) - previous.duration_ms / 60_000.0),
            1,
            "",
        ));
        details.push(figure(
            match lang {
                Lang::Ja => "打鍵数".to_owned(),
                Lang::En => "Keystrokes".to_owned(),
            },
            grouped(totals.keystrokes),
            previous.map(|previous| totals.keystrokes as f64 - previous.keystrokes),
            0,
            "",
        ));
    }
    let (events, highlight) = events(data, lang);
    let chain = match (events.is_empty(), data.runs.first()) {
        (true, Some(run)) => run.chain.clone(),
        _ => Vec::new(),
    };
    SummaryView {
        heading,
        subheading,
        mains,
        details,
        events,
        chain,
        highlight,
    }
}

/// Rows of the bottom band: the events, or the menu's steps on one row.
fn band_rows(view: &SummaryView) -> u16 {
    if view.events.is_empty() {
        1
    } else {
        view.events.len().clamp(1, 2) as u16
    }
}

/// How the main values are drawn in the text version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SummaryText {
    Big(BigTextSize),
    Plain,
}

/// Image when graphics are available, there is a record, and the area is at
/// least 120×29 cells (the result screen's compact threshold).
pub fn uses_image(app: &App, area: Rect) -> bool {
    app.graphics.mode != GraphicsMode::Off
        && app.graphics.cell_pixels.is_some()
        && app.analysis_today.totals.is_some()
        && area.width >= 120
        && area.height >= 29
}

/// The biggest bitmap font whose rows and widths fit the frame's inside.
pub fn text_size(view: &SummaryView, inner_width: u16, inner_height: u16) -> SummaryText {
    let columns = view.mains.len().max(1) as u16;
    let column_width = inner_width / columns;
    // Label, unit/delta, gap, details (up to 2), band (up to 2).
    let fixed = 1 + 1 + 1 + 2 + band_rows(view);
    for size in [BigTextSize::Dot16, BigTextSize::Dot12] {
        let Ok(font) = crate::ui::bigtext::font(size) else { continue };
        let widest = view.mains.iter().map(|main| font.text_width(&main.value)).max().unwrap_or(0);
        if !view.mains.is_empty() && widest + 2 <= column_width && fixed + size.cell_height() <= inner_height {
            return SummaryText::Big(size);
        }
    }
    SummaryText::Plain
}

pub fn render(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let lang = app.config.general.lang;
    let Some(body) = render_footer(frame, lang, theme, area) else {
        return;
    };
    let view = summary_view(&app.analysis_today, lang);
    if uses_image(app, body) && render_image(frame, app, theme, &view, body) {
        return;
    }
    render_text(frame, lang, theme, &view, body);
}

/// The one-line footer of a Summary, small and muted, out of the way of a
/// screenshot. Returns the area above it, or `None` when `area` is too small.
pub(crate) fn render_footer(frame: &mut Frame, lang: Lang, theme: &Theme, area: Rect) -> Option<Rect> {
    if area.height < 3 || area.width < 10 {
        return None;
    }
    let footer = Rect::new(area.x, area.y + area.height - 1, area.width, 1);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            match lang {
                Lang::Ja => "Summary   [S] 詳細   [Esc] 戻る   [?] 用語",
                Lang::En => "Summary   [S] details   [Esc] back   [?] Terms",
            },
            Style::default().fg(theme.muted),
        )))
        .alignment(Alignment::Right),
        footer,
    );
    Some(Rect::new(area.x, area.y, area.width, area.height - 1))
}

fn render_image(frame: &mut Frame, app: &App, theme: &Theme, view: &SummaryView, area: Rect) -> bool {
    let spec = image_spec(view, app.config.general.lang, theme, area.as_size());
    let font = app.bigtext_images.borrow_mut().resolved_font(&app.config.display.big_font, &app.home_dir);
    let mut images = app.analysis_images.borrow_mut();
    if images.prepare_today_summary(&app.graphics, &font, &spec).is_err() {
        return false;
    }
    let Some(protocol) = images.today_summary_protocol(&app.graphics, &font, &spec) else {
        return false;
    };
    frame.render_widget(Image::new(protocol), area);
    true
}

/// The image spec of `view` for `cells`.
pub fn image_spec(view: &SummaryView, lang: Lang, theme: &Theme, cells: ratatui::layout::Size) -> TodaySummarySpec {
    TodaySummarySpec {
        heading: view.heading.clone(),
        subheading: view.subheading.clone(),
        mains: view.mains.clone(),
        unit: "WPM".to_owned(),
        details: view.details.clone(),
        events: view.events.clone(),
        chain: view.chain.clone(),
        chain_short: crate::ui::menu::step_chain_short(&view.chain, lang),
        highlight: view.highlight,
        cells,
        colors: image_colors(theme),
    }
}

pub(crate) fn delta_spans(figure: &SummaryFigure, digits: usize, suffix: &'static str, theme: &Theme) -> Vec<Span<'static>> {
    let Some(delta) = figure.delta else {
        return vec![Span::styled(figure.delta_text.clone(), Style::default().fg(theme.muted))];
    };
    // «11/10 比 » then the colored «▲+5.0».
    let prefix = figure
        .delta_text
        .rsplit_once(' ')
        .map_or_else(String::new, |(prefix, _)| format!("{prefix} "));
    vec![
        Span::styled(prefix, Style::default().fg(theme.muted)),
        difference_span(Some(delta), Direction::Higher, move |value| format!("{value:+.digits$}{suffix}"), theme),
    ]
}

pub(crate) fn centered(spans: Vec<Span<'static>>, width: usize) -> Line<'static> {
    let used = spans.iter().map(|span| span.content.width()).sum::<usize>();
    let mut all = vec![Span::raw(" ".repeat(width.saturating_sub(used) / 2))];
    all.extend(spans);
    Line::from(all)
}

/// The accent heading band on the first row of `area`.
pub(crate) fn render_heading_band(frame: &mut Frame, theme: &Theme, area: Rect, heading: &str, subheading: &str) {
    let width = usize::from(area.width);
    let band_style = Style::default().fg(Color::Black).bg(theme.accent).add_modifier(Modifier::BOLD);
    let heading = format!(" {heading}");
    let subheading = format!("{subheading} ");
    let gap = width.saturating_sub(heading.width() + subheading.width());
    let band = if gap >= 2 {
        format!("{heading}{}{subheading}", " ".repeat(gap))
    } else {
        format!("{:width$}", truncate(&heading, width))
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(band, band_style))),
        Rect::new(area.x, area.y, area.width, 1),
    );
}

/// The heading band and the decorated (double-lined) frame below it.
/// Returns the frame's inside, or `None` when nothing fits in it.
pub(crate) fn render_band_and_frame(frame: &mut Frame, theme: &Theme, area: Rect, heading: &str, subheading: &str) -> Option<Rect> {
    render_heading_band(frame, theme, area, heading, subheading);
    if area.height < 4 {
        return None;
    }
    let frame_area = Rect::new(area.x, area.y + 1, area.width, area.height - 1);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(theme.accent));
    let inner = block.inner(frame_area);
    frame.render_widget(block, frame_area);
    (inner.width > 0 && inner.height > 0).then_some(inner)
}

/// Event lines on the dark bottom band, centered; the first is highlighted
/// (▲ and the warning color) when `highlight` is set.
pub(crate) fn event_band_lines(events: &[String], highlight: bool, theme: &Theme, width: usize, rows: u16) -> Vec<Line<'static>> {
    events
        .iter()
        .take(usize::from(rows))
        .enumerate()
        .map(|(index, event)| {
            let highlight = highlight && index == 0;
            let text = truncate(&if highlight { format!("▲ {event}") } else { event.clone() }, width);
            let padding = width.saturating_sub(text.width());
            let line = format!("{}{text}{}", " ".repeat(padding / 2), " ".repeat(padding - padding / 2));
            Line::from(Span::styled(
                line,
                Style::default()
                    .bg(Color::Indexed(236))
                    .fg(if highlight { theme.warning } else { Color::White })
                    .add_modifier(Modifier::BOLD),
            ))
        })
        .collect()
}

/// `value` in the bitmap big text of `size`, or as one plain line.
pub(crate) fn value_lines(value: &str, size: SummaryText, style: Style) -> Vec<Line<'static>> {
    match size {
        SummaryText::Big(size) => crate::ui::bigtext::font(size)
            .map(|font| {
                let characters = value.chars().map(|character| StyledCharacter::new(character, style)).collect::<Vec<_>>();
                font.render(&characters).lines
            })
            .unwrap_or_default(),
        SummaryText::Plain => vec![Line::from(Span::styled(value.to_owned(), style))],
    }
}

/// The text version, also used when an image cannot be drawn.
pub fn render_text(frame: &mut Frame, lang: Lang, theme: &Theme, view: &SummaryView, area: Rect) {
    let Some(inner) = render_band_and_frame(frame, theme, area, &view.heading, &view.subheading) else {
        return;
    };
    let inner_width = usize::from(inner.width);
    let size = text_size(view, inner.width, inner.height);
    let big_rows = match size {
        SummaryText::Big(size) => size.cell_height(),
        SummaryText::Plain => 1,
    };

    // Dark band of events at the bottom of the frame.
    let event_rows = band_rows(view).min(inner.height);
    let event_area = Rect::new(inner.x, inner.y + inner.height - event_rows, inner.width, event_rows);
    let event_lines = event_band_lines(&view.events, view.highlight, theme, inner_width, event_rows);
    let event_lines = if view.events.is_empty() {
        vec![chain_line(view, lang, theme, inner_width)]
    } else {
        event_lines
    };
    frame.render_widget(Paragraph::new(event_lines), event_area);

    // Details wrapped over at most two lines.
    let mut detail_lines: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    for detail in &view.details {
        let (digits, suffix) = if detail.value.ends_with('%') {
            (1, "pt")
        } else if detail.value.contains('.') {
            (1, "")
        } else {
            (0, "")
        };
        let mut spans = vec![
            Span::styled(format!("{} ", detail.label), Style::default().fg(theme.muted)),
            Span::styled(format!("{} ", detail.value), Style::default().add_modifier(Modifier::BOLD)),
        ];
        spans.extend(delta_spans(detail, digits, suffix, theme));
        let item_width = spans.iter().map(|span| span.content.width()).sum::<usize>();
        let current = detail_lines.last_mut().expect("one line");
        let used = current.iter().map(|span| span.content.width()).sum::<usize>();
        if !current.is_empty() && used + 3 + item_width + 2 > inner_width {
            detail_lines.push(spans);
        } else {
            if !current.is_empty() {
                current.push(Span::raw("   "));
            }
            current.extend(spans);
        }
    }
    detail_lines.retain(|line| !line.is_empty());
    let detail_rows = detail_lines.len() as u16;

    // Vertical layout of the main block: label, big value, unit + delta.
    let main_rows = 1 + big_rows + 1;
    let free = inner.height.saturating_sub(event_rows + main_rows + detail_rows);
    let top_gap = free / 3;
    let middle_gap = (free - top_gap) / 2;
    let main_y = inner.y + top_gap;
    let columns = view.mains.len().max(1) as u16;
    let column_width = inner.width / columns;
    if view.mains.is_empty() {
        let y = main_y + 1;
        if y < event_area.y {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled("—", Style::default().fg(theme.muted)))).alignment(Alignment::Center),
                Rect::new(inner.x, y, inner.width, 1),
            );
        }
    }
    for (index, main) in view.mains.iter().enumerate() {
        let x = inner.x + column_width * index as u16;
        let width = if index + 1 == view.mains.len() {
            inner.width - column_width * index as u16
        } else {
            column_width
        };
        let column = |row: u16, height: u16| Rect::new(x, main_y + row, width, height);
        if main_y + main_rows > event_area.y {
            break;
        }
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                main.label.clone(),
                Style::default().fg(theme.muted).add_modifier(Modifier::BOLD),
            )))
            .alignment(Alignment::Center),
            column(0, 1),
        );
        let style = Style::default().fg(theme.accent).add_modifier(Modifier::BOLD);
        let value_lines = value_lines(&main.value, size, style);
        frame.render_widget(Paragraph::new(value_lines).alignment(Alignment::Center), column(1, big_rows));
        let mut unit = vec![Span::styled("WPM  ", Style::default().add_modifier(Modifier::BOLD))];
        unit.extend(delta_spans(main, 1, "", theme));
        if !main.average.is_empty() {
            unit.push(Span::styled(format!(" ({})", main.average), Style::default().fg(theme.muted)));
        }
        frame.render_widget(Paragraph::new(centered(unit, usize::from(width))), column(1 + big_rows, 1));
    }
    let details_y = main_y + main_rows + middle_gap;
    for (row, spans) in detail_lines.into_iter().enumerate() {
        let y = details_y + row as u16;
        if y >= event_area.y {
            break;
        }
        frame.render_widget(Paragraph::new(centered(spans, inner_width)), Rect::new(inner.x, y, inner.width, 1));
    }
}

/// The menu's steps on the dark band: done steps bright with `✓`, the rest
/// dimmed, separated by `→`; shrunk as [`crate::ui::menu::step_chain`] does.
fn chain_line(view: &SummaryView, lang: Lang, theme: &Theme, width: usize) -> Line<'static> {
    let band = Style::default().bg(Color::Indexed(236));
    let text = crate::ui::menu::step_chain(&view.chain, lang, width);
    let separator = [" → ", "→"].into_iter().find(|separator| {
        let joined = view
            .chain
            .iter()
            .map(|(label, done)| if *done { format!("✓{label}") } else { label.clone() })
            .collect::<Vec<_>>()
            .join(separator);
        joined == text
    });
    let mut spans = Vec::new();
    match separator {
        Some(separator) => {
            for (index, (label, done)) in view.chain.iter().enumerate() {
                if index > 0 {
                    spans.push(Span::styled(separator.to_owned(), band.fg(Color::Gray)));
                }
                spans.push(if *done {
                    Span::styled(format!("✓{label}"), band.fg(theme.correct).add_modifier(Modifier::BOLD))
                } else {
                    Span::styled(label.clone(), band.fg(Color::Gray))
                });
            }
        }
        None => spans.push(Span::styled(text, band.fg(Color::White).add_modifier(Modifier::BOLD))),
    }
    let used = spans.iter().map(|span| span.content.width()).sum::<usize>();
    let padding = width.saturating_sub(used);
    let mut line = vec![Span::styled(" ".repeat(padding / 2), band)];
    line.extend(spans);
    line.push(Span::styled(" ".repeat(padding - padding / 2), band));
    Line::from(line)
}
