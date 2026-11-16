//! History screen — filtered and grouped past sessions backed by SQLite.

use std::collections::BTreeSet;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, HighlightSpacing, List, ListItem, ListState, Paragraph};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{AnalysisPeriod, App, HistoryEntry, HistoryKindFilter, HistorySort, bench_part, history_language};
use crate::i18n::{Lang, Msg};
use crate::ui::{left_inset, theme::Theme};

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
        .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(2)])
        .split(inner);
    render_header(frame, app, &theme, left_inset(chunks[0]));
    render_list(frame, app, &theme, left_inset(chunks[1]));
    render_hint(frame, app, &theme, chunks[2]);
}

fn render_header(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let lang = app.config.general.lang;
    let filter = match lang {
        Lang::En => format!(
            "Type: {}  Mode: {}  Language: {}  Period: {}",
            kind_label(app.history_kind_filter, lang),
            app.history_mode_filter.as_deref().map_or("all", |mode| mode_label(mode, lang)),
            app.history_language_filter.as_deref().unwrap_or("all"),
            period_label(app.history_period, lang),
        ),
        Lang::Ja => format!(
            "種類: {}  モード: {}  言語: {}  期間: {}",
            kind_label(app.history_kind_filter, lang),
            app.history_mode_filter.as_deref().map_or("すべて", |mode| mode_label(mode, lang)),
            app.history_language_filter.as_deref().unwrap_or("すべて"),
            period_label(app.history_period, lang),
        ),
    };
    let rank_note =
        if app.history_sort == HistorySort::WpmDesc && (app.history_mode_filter.is_none() || app.history_kind_filter == HistoryKindFilter::All) {
            match lang {
                Lang::En => "  Rank appears after selecting a mode",
                Lang::Ja => "  順位はモードを選ぶと出ます",
            }
        } else {
            ""
        };
    let lines = vec![
        Line::from(Span::styled(
            format!("{} : {}", Msg::Database.text(lang), app.db_path().display()),
            Style::default().fg(theme.muted),
        )),
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
            Span::styled(rank_note, Style::default().fg(theme.warning)),
        ]),
        Line::from(Span::styled(filter, Style::default().fg(theme.fg))),
    ];
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_list(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let lang = app.config.general.lang;
    if let Some(err) = &app.history_load_error {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(Msg::FailedToLoadHistory.text(lang), Style::default().fg(theme.mistype))),
                Line::from(Span::styled(err.clone(), Style::default().fg(theme.fg))),
            ])
            .alignment(Alignment::Left),
            area,
        );
        return;
    }
    if app.history.is_empty() {
        let message = if app.history_sessions_empty() {
            Msg::NoPastSessions
        } else {
            Msg::NoMatchingSessions
        };
        frame.render_widget(Paragraph::new(message.text(lang)).style(Style::default().fg(theme.muted)), area);
        return;
    }
    let columns = Columns::for_width(area.width, app.history_ranks.iter().any(Option::is_some));
    let header_line = Line::from(Span::styled(
        columns.header(lang),
        Style::default().fg(theme.muted).add_modifier(Modifier::BOLD),
    ));
    let items = app
        .history
        .iter()
        .zip(app.history_ranks.iter().copied())
        .map(|(entry, rank)| ListItem::new(format_entry(entry, rank, app.history_sort, columns, lang, theme)))
        .collect::<Vec<_>>();
    let list_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);
    frame.render_widget(Paragraph::new(header_line), list_area[0]);
    let list = List::new(items)
        .highlight_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD | Modifier::REVERSED))
        .highlight_symbol("▶ ")
        .highlight_spacing(HighlightSpacing::Always);
    let mut state = ListState::default();
    state.select(Some(app.history_selected));
    frame.render_stateful_widget(list, list_area[1], &mut state);
}

#[derive(Clone, Copy)]
struct Columns {
    width: usize,
    rank: bool,
    time: bool,
    language: bool,
    layout: bool,
    content: usize,
}

impl Columns {
    fn for_width(width: u16, rank: bool) -> Self {
        let width = usize::from(width.saturating_sub(2));
        let layout = width >= 106;
        let language = width >= 92;
        let time = width >= 82;
        let fixed = 45 + usize::from(rank) * 7 + usize::from(time) * 8 + usize::from(language) * 7 + usize::from(layout) * 14;
        let content = width.saturating_sub(fixed).max(8);
        Self {
            width,
            rank,
            time,
            language,
            layout,
            content,
        }
    }

    fn header(self, lang: Lang) -> String {
        let (finished, kind, content, language, layout, accuracy, time) = match lang {
            Lang::En => ("Finished", "Type", "Content", "Lang", "Layout", "Acc%", "Time"),
            Lang::Ja => ("日時", "種類", "内容", "言語", "配列", "正確さ", "時間"),
        };
        let mut fields = Vec::new();
        if self.rank {
            fields.push(pad("#", 5, true));
        }
        fields.push(pad(finished, 16, false));
        fields.push(pad(kind, 9, false));
        fields.push(pad(content, self.content, false));
        if self.language {
            fields.push(pad(language, 5, false));
        }
        if self.layout {
            fields.push(pad(layout, 12, false));
        }
        fields.push(pad("WPM", 6, true));
        fields.push(pad(accuracy, 6, true));
        if self.time {
            fields.push(pad(time, 6, true));
        }
        truncate(&fields.join("  "), self.width)
    }
}

fn format_entry(entry: &HistoryEntry, rank: Option<usize>, sort: HistorySort, columns: Columns, lang: Lang, theme: &Theme) -> Line<'static> {
    let summary = entry.primary_summary();
    let normal = Style::default().fg(theme.fg);
    let mut fields = Vec::new();
    if columns.rank {
        fields.push((pad(&rank.map_or_else(String::new, |rank| format!("#{rank}")), 5, true), normal));
    }
    fields.push((pad(&entry.finished_at().format("%Y-%m-%d %H:%M").to_string(), 16, false), normal));
    fields.push((pad(&kind_for_entry(entry, lang), 9, false), normal));
    fields.push((pad(&content_for_entry(entry, lang), columns.content, false), normal));
    if columns.language {
        fields.push((pad(&language_for_entry(entry), 5, false), normal));
    }
    if columns.layout {
        fields.push((pad(&summary.layout, 12, false), normal));
    }
    let wpm_style = if sort == HistorySort::WpmDesc {
        Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
    } else {
        normal
    };
    fields.push((pad(&format!("{:.1}", entry.wpm()), 6, true), wpm_style));
    let accuracy_style = if sort == HistorySort::AccuracyDesc {
        Style::default().fg(theme.correct).add_modifier(Modifier::BOLD)
    } else {
        normal
    };
    fields.push((pad(&format!("{:.1}%", entry.accuracy() * 100.0), 6, true), accuracy_style));
    if columns.time {
        fields.push((pad(&format!("{}s", entry.duration().as_secs()), 6, true), normal));
    }
    let mut spans = Vec::with_capacity(fields.len() * 2 - 1);
    for (index, (value, style)) in fields.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled("  ", normal));
        }
        spans.push(Span::styled(value, style));
    }
    Line::from(spans)
}

fn kind_for_entry(entry: &HistoryEntry, lang: Lang) -> String {
    if entry.primary_summary().metrics_version == 0 {
        return match lang {
            Lang::En => "v1 import",
            Lang::Ja => "v1 取込",
        }
        .to_owned();
    }
    match entry {
        HistoryEntry::Session(_) => kind_label(HistoryKindFilter::Practice, lang).to_owned(),
        HistoryEntry::BenchRun { .. } | HistoryEntry::BenchPart { .. } => kind_label(HistoryKindFilter::Bench, lang).to_owned(),
    }
}

fn content_for_entry(entry: &HistoryEntry, lang: Lang) -> String {
    match entry {
        HistoryEntry::Session(summary) => mode_label(&summary.mode, lang).to_owned(),
        HistoryEntry::BenchRun { form, parts, .. } if parts.len() == 1 => {
            let part = bench_part(&parts[0]).unwrap_or_else(|| "—".to_owned());
            match lang {
                Lang::En => format!("{part} · form {form}"),
                Lang::Ja => format!("{part}・版 {form}"),
            }
        }
        HistoryEntry::BenchRun { form, parts, .. } => {
            let scores = parts
                .iter()
                .map(|part| format!("{} {:.1}", bench_part(part).unwrap_or_else(|| "—".to_owned()), part.wpm))
                .collect::<Vec<_>>()
                .join(" / ");
            match lang {
                Lang::En => format!("form {form} · {scores}"),
                Lang::Ja => format!("版 {form}  {scores}"),
            }
        }
        HistoryEntry::BenchPart { form, summary, .. } => {
            let part = bench_part(summary).unwrap_or_else(|| "—".to_owned());
            match lang {
                Lang::En => format!("  ↳ {part} · form {form}"),
                Lang::Ja => format!("  ↳ {part}・版 {form}"),
            }
        }
    }
}

fn language_for_entry(entry: &HistoryEntry) -> String {
    let values = match entry {
        HistoryEntry::BenchRun { parts, .. } => parts.iter().filter_map(history_language).collect::<BTreeSet<_>>(),
        _ => [history_language(entry.primary_summary())].into_iter().flatten().collect(),
    };
    if values.is_empty() {
        "—".to_owned()
    } else {
        values.into_iter().collect::<Vec<_>>().join("/")
    }
}

fn kind_label(kind: HistoryKindFilter, lang: Lang) -> &'static str {
    match (kind, lang) {
        (HistoryKindFilter::All, Lang::En) => "All",
        (HistoryKindFilter::All, Lang::Ja) => "すべて",
        (HistoryKindFilter::Practice, Lang::En) => "Practice",
        (HistoryKindFilter::Practice, Lang::Ja) => "練習",
        (HistoryKindFilter::Bench, Lang::En) => "Bench",
        (HistoryKindFilter::Bench, Lang::Ja) => "ベンチ",
    }
}

fn period_label(period: AnalysisPeriod, lang: Lang) -> &'static str {
    match period {
        AnalysisPeriod::All => Msg::PeriodAll.text(lang),
        AnalysisPeriod::Days30 => Msg::PeriodDays30.text(lang),
        AnalysisPeriod::Days7 => Msg::PeriodDays7.text(lang),
    }
}

pub(crate) fn mode_label(mode: &str, lang: Lang) -> &str {
    match (mode, lang) {
        ("random", Lang::En) => "Random",
        ("random", Lang::Ja) => "ランダム文字列",
        ("words", Lang::En) => "Words",
        ("words", Lang::Ja) => "英単語",
        ("romaji", Lang::En) => "Japanese Words",
        ("romaji", Lang::Ja) => "日本語の単語",
        ("sentence", Lang::En) => "Sentences",
        ("sentence", Lang::Ja) => "短文",
        ("phrase", Lang::En) => "Phrases",
        ("phrase", Lang::Ja) => "句",
        ("ngram", Lang::En) => "N-grams",
        ("ngram", Lang::Ja) => "文字の組み合わせ",
        ("adaptive", Lang::En) => "Adaptive",
        ("adaptive", Lang::Ja) => "弱い遷移を重点練習",
        ("unlock", Lang::En) => "Unlock",
        ("unlock", Lang::Ja) => "キーを段階的に解放",
        ("rhythm", Lang::En) => "Rhythm",
        ("rhythm", Lang::Ja) => "メトロノーム",
        ("burst", Lang::En) => "Burst",
        ("burst", Lang::Ja) => "短距離を 3 本",
        ("endurance", Lang::En) => "Endurance",
        ("endurance", Lang::Ja) => "短文の持久練習",
        _ => mode,
    }
}

fn pad(value: &str, width: usize, right: bool) -> String {
    let value = truncate(value, width);
    let padding = " ".repeat(width.saturating_sub(UnicodeWidthStr::width(value.as_str())));
    if right {
        format!("{padding}{value}")
    } else {
        format!("{value}{padding}")
    }
}

fn truncate(value: &str, width: usize) -> String {
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

fn render_hint(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let hint = match app.config.general.lang {
        Lang::En => "[↑/↓/k/j] Move  [Enter] Open  [Tab] Type  [M] Mode  [L] Language  [P] Period  [S] Sort  [Esc/Q] Back",
        Lang::Ja => "[↑/↓/k/j] 移動  [Enter] 開閉・詳細  [Tab] 種類  [M] モード  [L] 言語  [P] 期間  [S] 並べ替え  [Esc/Q] 戻る",
    };
    frame.render_widget(Paragraph::new(hint).style(Style::default().fg(theme.accent)), left_inset(area));
}
