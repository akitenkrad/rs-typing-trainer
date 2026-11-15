use std::collections::{BTreeMap, BTreeSet};

use chrono::DateTime;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{AnalysisPeriod, App};
use crate::i18n::{Lang, Msg};
use crate::layout::geometry::geometry;
use crate::ui::keyboard::{KeyboardState, KeyboardView};
use crate::ui::theme::Theme;

pub fn render(frame: &mut Frame, app: &App) {
    let theme = Theme::from_name(&app.config.ui.theme);
    let area = frame.area();
    let lang = app.config.general.lang;
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", Msg::AnalysisTitle.text(lang)));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(3), Constraint::Length(2)])
        .split(inner);
    render_tabs(frame, app, &theme, chunks[0]);
    match app.analysis_tab {
        0 => render_summary(frame, app, &theme, chunks[1]),
        1 => render_trend(frame, app, chunks[1]),
        2 => render_keyboard(frame, app, chunks[1]),
        3 => render_transitions(frame, app, &theme, chunks[1]),
        4 => render_fingers(frame, app, chunks[1]),
        _ => render_bench(frame, app, chunks[1]),
    }
    let period = match app.analysis_period {
        AnalysisPeriod::All => Msg::PeriodAll.text(lang),
        AnalysisPeriod::Days30 => Msg::PeriodDays30.text(lang),
        AnalysisPeriod::Days7 => Msg::PeriodDays7.text(lang),
    };
    let mode = app
        .analysis_mode_filter
        .as_deref()
        .map_or_else(|| Msg::ModeAll.text(lang).to_owned(), |mode| mode_label(mode, lang));
    let hint = format!(
        "{}   {}: {period}   {}: {mode}   {}",
        Msg::AnalysisHintTab.text(lang),
        Msg::AnalysisHintPeriod.text(lang),
        Msg::AnalysisHintMode.text(lang),
        Msg::AnalysisHintBack.text(lang)
    );
    frame.render_widget(Paragraph::new(hint).alignment(Alignment::Center), chunks[2]);
}

fn render_tabs(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let lang = app.config.general.lang;
    let labels = [
        match lang {
            Lang::En => "Coach",
            Lang::Ja => "コーチ",
        },
        Msg::AnalysisTrend.text(lang),
        Msg::AnalysisKeyboard.text(lang),
        Msg::AnalysisTransitions.text(lang),
        Msg::AnalysisFingersHands.text(lang),
        Msg::AnalysisBench.text(lang),
    ];
    let spans = labels
        .into_iter()
        .enumerate()
        .flat_map(|(index, label)| {
            [
                Span::raw(" "),
                Span::styled(
                    label,
                    if index == app.analysis_tab {
                        Style::default().fg(theme.accent).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                    } else {
                        Style::default().fg(theme.muted)
                    },
                ),
                Span::raw(" "),
            ]
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(Line::from(spans)).alignment(Alignment::Center), area);
}

fn render_summary(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let base = app.analysis_coach.as_ref().map_or(&app.analysis_summary.lines, |report| &report.lines);
    let mut lines = base.clone();
    if app.analysis_goal_picker
        && let Some(report) = &app.analysis_coach
    {
        let mut picker = vec![match app.config.general.lang {
            Lang::En => "▼ GOAL CANDIDATES — choose with ↑/↓ and Enter".to_owned(),
            Lang::Ja => "▼ 目標の候補 — ↑/↓ で選び Enter で保存".to_owned(),
        }];
        picker.extend(report.goals.iter().enumerate().map(|(index, goal)| {
            format!(
                "{} {}",
                if index == app.analysis_goal_selected { "▶" } else { " " },
                crate::analysis::coach::goal_label(goal, app.config.general.lang)
            )
        }));
        picker.push(String::new());
        picker.append(&mut lines);
        lines = picker;
    }
    let body = if !app.analysis_goal_picker && area.height >= 20 {
        render_grade_badges(frame, app, theme, area).unwrap_or(area)
    } else {
        area
    };
    let lines = wrap_coach_lines(&lines, usize::from(body.width));
    frame.render_widget(
        Paragraph::new(lines.into_iter().map(Line::from).collect::<Vec<_>>()).scroll((app.analysis_summary_scroll, 0)),
        body,
    );
}

fn wrap_coach_lines(lines: &[String], width: usize) -> Vec<String> {
    lines.iter().flat_map(|line| wrap_coach_line(line, width)).collect()
}

fn wrap_coach_line(line: &str, width: usize) -> Vec<String> {
    if width == 0 || UnicodeWidthStr::width(line) <= width {
        return vec![line.to_owned()];
    }
    let leading = line.chars().take_while(|character| character.is_whitespace()).collect::<String>();
    let leading_width = UnicodeWidthStr::width(leading.as_str());
    let rest = &line[leading.len()..];
    let continuation_width = if rest.starts_with("• ") { leading_width + 2 } else { leading_width };
    let continuation = " ".repeat(continuation_width.min(width.saturating_sub(1)));
    let mut remaining = line;
    let mut result = Vec::new();
    let mut first = true;
    while !remaining.is_empty() {
        let prefix = if first { "" } else { continuation.as_str() };
        let available = width.saturating_sub(UnicodeWidthStr::width(prefix)).max(1);
        if UnicodeWidthStr::width(remaining) <= available {
            result.push(format!("{prefix}{remaining}"));
            break;
        }

        let mut used = 0;
        let mut split = remaining.len();
        let mut whitespace_split = None;
        for (byte, character) in remaining.char_indices() {
            let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
            if used + character_width > available {
                split = byte;
                break;
            }
            used += character_width;
            if character.is_whitespace() {
                whitespace_split = Some(byte + character.len_utf8());
            }
        }
        if let Some(space) = whitespace_split
            && space > split / 2
        {
            split = space;
        }
        if split == 0 {
            split = remaining.chars().next().map_or(remaining.len(), char::len_utf8);
        }
        result.push(format!("{prefix}{}", remaining[..split].trim_end()));
        remaining = remaining[split..].trim_start();
        first = false;
    }
    result
}

fn render_grade_badges(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) -> Option<Rect> {
    let report = app.analysis_coach.as_ref()?;
    let grades = report
        .grades
        .languages
        .iter()
        .filter_map(|grade| Some((grade, grade.overall.as_ref()?)))
        .collect::<Vec<_>>();
    if grades.is_empty() {
        return None;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Min(1)])
        .split(area);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints((0..grades.len()).map(|_| Constraint::Ratio(1, grades.len() as u32)).collect::<Vec<_>>())
        .split(chunks[0]);
    for ((grade, overall), column) in grades.into_iter().zip(columns.iter().copied()) {
        let color = grade_color(overall.grade, theme);
        let label = match (grade.language.as_str(), app.config.general.lang) {
            ("ja", Lang::En) => "Japanese",
            ("ja", Lang::Ja) => "日本語",
            (_, Lang::En) => "English",
            (_, Lang::Ja) => "英語",
        };
        let mut lines = vec![Line::from(Span::styled(
            format!("{label}  {:.2}", overall.score),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))];
        if let Ok(font) = crate::ui::bigtext::font(crate::ui::bigtext::BigTextSize::Dot12) {
            lines.extend(
                font.render(&[crate::ui::bigtext::StyledCharacter::new(
                    overall.grade.as_str().chars().next().unwrap_or('?'),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                )])
                .lines,
            );
        } else {
            lines.push(Line::from(Span::styled(
                overall.grade.as_str(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )));
        }
        frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), column);
    }
    Some(chunks[1])
}

fn grade_color(level: crate::analysis::grades::Level, theme: &Theme) -> Color {
    match level {
        crate::analysis::grades::Level::S => theme.warning,
        crate::analysis::grades::Level::A => theme.correct,
        crate::analysis::grades::Level::B => theme.accent,
        crate::analysis::grades::Level::C => Color::LightBlue,
        crate::analysis::grades::Level::D => Color::LightMagenta,
        crate::analysis::grades::Level::E => Color::Yellow,
        crate::analysis::grades::Level::F => Color::Gray,
        crate::analysis::grades::Level::G => theme.muted,
    }
}

fn render_trend(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Ratio(1, 3), Constraint::Ratio(1, 3), Constraint::Ratio(1, 3)])
        .split(area);
    let lang = app.config.general.lang;
    render_metric_chart(frame, &app.analysis_records, chunks[0], MetricKind::NetWpm, lang);
    render_metric_chart(frame, &app.analysis_records, chunks[1], MetricKind::Accuracy, lang);
    render_metric_chart(frame, &app.analysis_records, chunks[2], MetricKind::Iki, lang);
}

#[derive(Clone, Copy)]
enum MetricKind {
    NetWpm,
    Accuracy,
    Iki,
}

fn render_metric_chart(frame: &mut Frame, records: &[crate::record::SessionRecord], area: Rect, kind: MetricKind, lang: Lang) {
    let title = match kind {
        MetricKind::NetWpm => Msg::TrendNetWpm.text(lang),
        MetricKind::Accuracy => Msg::TrendAccuracy.text(lang),
        MetricKind::Iki => Msg::TrendIki.text(lang),
    };
    let mut groups = BTreeMap::<String, BTreeMap<String, Vec<f64>>>::new();
    for record in records
        .iter()
        .filter(|record| !matches!(kind, MetricKind::Iki) || record.header.metrics_version != 0)
    {
        let date = DateTime::parse_from_rfc3339(&record.header.finished_at)
            .map(|value| value.date_naive().to_string())
            .unwrap_or_else(|_| "unknown".to_owned());
        let series = record
            .header
            .bench_id
            .as_deref()
            .and_then(|id| id.split('/').nth(1))
            .map(|part| match lang {
                Lang::En => format!("bench-{part}"),
                Lang::Ja => format!("ベンチ-{part}"),
            })
            .unwrap_or_else(|| mode_label(&record.header.mode, lang));
        groups
            .entry(series)
            .or_default()
            .entry(date)
            .or_default()
            .push(metric_value(kind, record));
    }
    let dates = groups
        .values()
        .flat_map(|days| days.keys().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if dates.len() < 2 {
        let current = records
            .iter()
            .rev()
            .find(|record| !matches!(kind, MetricKind::Iki) || record.header.metrics_version != 0)
            .map(|record| format!("{}: {:.1}", Msg::CurrentValue.text(lang), metric_value(kind, record)))
            .unwrap_or_else(|| Msg::NoData.text(lang).to_owned());
        let text = format!("{}    {current}", Msg::TrendNeedsTwoDays.text(lang));
        frame.render_widget(
            Paragraph::new(text)
                .block(Block::default().title(title.to_owned()))
                .alignment(Alignment::Center),
            area,
        );
        return;
    }
    let date_indices = dates
        .iter()
        .enumerate()
        .map(|(index, date)| (date, index as f64))
        .collect::<BTreeMap<_, _>>();
    let names = groups.keys().cloned().collect::<Vec<_>>();
    let points = groups
        .values()
        .map(|days| {
            days.iter()
                .map(|(date, values)| (*date_indices.get(date).expect("date index"), median(values)))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let colors = [Color::Cyan, Color::Green, Color::Yellow, Color::Magenta, Color::Blue, Color::Red];
    let datasets = points
        .iter()
        .enumerate()
        .map(|(index, values)| {
            Dataset::default()
                .name(names[index].clone())
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(colors[index % colors.len()]))
                .data(values)
        })
        .collect::<Vec<_>>();
    let values = points.iter().flatten().map(|(_, value)| *value).collect::<Vec<_>>();
    let min_y = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max_y = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let padding = if (max_y - min_y).abs() < f64::EPSILON {
        (max_y.abs() * 0.05).max(1.0)
    } else {
        0.0
    };
    let first_date = dates.first().cloned().unwrap_or_default();
    let last_date = dates.last().cloned().unwrap_or_default();
    let chart = Chart::new(datasets)
        .block(Block::default().title(title.to_owned()))
        .x_axis(
            Axis::default()
                .title(Msg::TrendDay.text(lang))
                .bounds([0.0, dates.len().saturating_sub(1) as f64])
                .labels(vec![Span::raw(first_date), Span::raw(last_date)]),
        )
        .y_axis(
            Axis::default()
                .bounds([min_y - padding, max_y + padding])
                .labels(vec![Span::raw(format!("{min_y:.1}")), Span::raw(format!("{max_y:.1}"))]),
        );
    frame.render_widget(chart, area);
}

fn metric_value(kind: MetricKind, record: &crate::record::SessionRecord) -> f64 {
    match kind {
        MetricKind::NetWpm => record.header.net_wpm,
        MetricKind::Accuracy => record.header.accuracy * 100.0,
        MetricKind::Iki => record.header.iki_median_ms,
    }
}

fn mode_label(mode: &str, lang: Lang) -> String {
    if lang == Lang::En {
        return mode.to_owned();
    }
    match mode {
        "random" => "ランダム",
        "words" => "単語",
        "romaji" => "ローマ字",
        "sentence" => "短文",
        "ngram" => "n-gram",
        "bench" => "ベンチ",
        "adaptive" => "適応",
        "unlock" => "解放",
        "rhythm" => "リズム",
        "burst" => "バースト",
        "endurance" => "持久",
        other => other,
    }
    .to_owned()
}

fn render_keyboard(frame: &mut Frame, app: &App, area: Rect) {
    let Some(mut physical) = geometry(&app.config.keyboard.geometry) else {
        return;
    };
    if app.config.keyboard.digit6_finger == "right-index" {
        physical = physical.with_digit6_finger(crate::layout::geometry::Finger::RightIndex);
    }
    let layout = app.practice_layout();
    let mut selected_stroke = None;
    let heatmap = match app.analysis_keyboard_view {
        0 | 1 => {
            let value = |row: &crate::analysis::metrics::KeyMetric| {
                if app.analysis_keyboard_view == 1 {
                    row.iki_median_ms
                } else {
                    1.0 - row.accuracy
                }
            };
            let maximum = app.analysis_details.keys.iter().map(value).fold(0.0_f64, f64::max).max(0.01);
            app.analysis_details
                .keys
                .iter()
                .map(|row| (row.code.clone(), value(row) / maximum))
                .collect()
        }
        2 => app
            .analysis_coach
            .as_ref()
            .into_iter()
            .flat_map(|report| &report.map.keys)
            .filter_map(|row| {
                let character = row.key.chars().next()?;
                let code = layout.stroke_for(character)?.code;
                Some((code, (row.relative_iki / 2.0).clamp(0.0, 1.0)))
            })
            .collect(),
        _ => {
            let expected = app
                .analysis_coach
                .as_ref()
                .into_iter()
                .flat_map(|report| &report.diagnosis.errors.confusion)
                .map(|row| row.expected.as_str())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .nth(app.analysis_confusion_selected);
            selected_stroke = expected
                .and_then(|value| value.chars().next())
                .and_then(|character| layout.stroke_for(character));
            let rows = app
                .analysis_coach
                .as_ref()
                .into_iter()
                .flat_map(|report| &report.diagnosis.errors.confusion)
                .filter(|row| Some(row.expected.as_str()) == expected)
                .collect::<Vec<_>>();
            let maximum = rows.iter().map(|row| row.n).max().unwrap_or(1) as f64;
            rows.into_iter()
                .filter_map(|row| {
                    let code = layout.stroke_for(row.typed.chars().next()?)?.code;
                    Some((code, row.n as f64 / maximum))
                })
                .collect()
        }
    };
    let state = KeyboardState {
        next: selected_stroke,
        last: None,
        heatmap,
        fade: Default::default(),
        unlock: None,
    };
    crate::ui::keyboard::render_heatmap(
        frame,
        area,
        &physical,
        app.practice_layout(),
        &state,
        KeyboardView {
            show: "full",
            style: &app.config.keyboard.style,
            finger_guide: true,
        },
    );
    let label = match (app.analysis_keyboard_view, app.config.general.lang) {
        (0, Lang::En) => "Error-rate heatmap  [Enter] next",
        (0, Lang::Ja) => "誤り率の地図  [Enter] 次へ",
        (1, Lang::En) => "Median IKI heatmap  [Enter] next",
        (1, Lang::Ja) => "IKI 中央値の地図  [Enter] 次へ",
        (2, Lang::En) => "Relative speed (personal median = 1.0)  [Enter] next",
        (2, Lang::Ja) => "相対速度（本人の中央値 = 1.0）  [Enter] 次へ",
        (3, Lang::En) => "Confusion by expected key  [←/→] key  [Enter] next",
        (3, Lang::Ja) => "取り違え（期待したキー別）  [←/→] キー  [Enter] 次へ",
        _ => unreachable!(),
    };
    frame.render_widget(Paragraph::new(label), Rect::new(area.x, area.y, area.width, 1));
}

fn render_transitions(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let lang = app.config.general.lang;
    let minimum = app.config.analysis.min_transition_n;
    let mut slow = app.analysis_details.bigrams.clone();
    slow.sort_by(|left, right| right.estimated_iki_ms.total_cmp(&left.estimated_iki_ms));
    let mut slow_lines = slow
        .iter()
        .filter(|row| row.n >= minimum)
        .take(20)
        .enumerate()
        .map(|(index, row)| {
            let marker = if index == app.analysis_transition_selected { "▶" } else { " " };
            Line::from(format!(
                "{marker} {:>2}. {}→{}  {:>7.1} ms  {} {:>5.1}%  n={}",
                index + 1,
                row.from,
                row.to,
                row.estimated_iki_ms,
                Msg::ErrorLabel.text(lang),
                row.estimated_error_rate * 100.0,
                row.n
            ))
            .style(if index == app.analysis_transition_selected {
                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            })
        })
        .collect::<Vec<_>>();
    let insufficient = slow.iter().filter(|row| row.n < minimum).take(5).collect::<Vec<_>>();
    if !insufficient.is_empty() {
        slow_lines.push(Line::from(""));
        slow_lines.push(Line::from(match lang {
            crate::i18n::Lang::En => format!("insufficient data (n < {minimum})"),
            crate::i18n::Lang::Ja => format!("データ不足（n < {minimum}）"),
        }));
        slow_lines.extend(
            insufficient
                .into_iter()
                .map(|row| Line::from(format!("    {}→{}  n={}", row.from, row.to, row.n))),
        );
    }
    let mut error_prone = app.analysis_details.bigrams.clone();
    error_prone.sort_by(|left, right| right.estimated_error_rate.total_cmp(&left.estimated_error_rate));
    let error_lines = error_prone
        .iter()
        .filter(|row| row.n >= minimum)
        .take(20)
        .enumerate()
        .map(|(index, row)| {
            Line::from(format!(
                "{:>2}. {}→{}  {:>6.1}%  n={}",
                index + 1,
                row.from,
                row.to,
                row.estimated_error_rate * 100.0,
                row.n
            ))
        })
        .collect::<Vec<_>>();
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
        .split(area);
    frame.render_widget(
        Paragraph::new(slow_lines).block(Block::default().title(format!(
            "{}   {}",
            Msg::SlowTransitionsTop20.text(lang),
            Msg::PracticeTransition.text(lang)
        ))),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(error_lines).block(Block::default().title(Msg::ErrorProneTransitionsTop20.text(lang))),
        chunks[1],
    );
}

fn render_fingers(frame: &mut Frame, app: &App, area: Rect) {
    let lang = app.config.general.lang;
    let mut lines = app
        .analysis_details
        .fingers
        .iter()
        .map(|row| {
            Line::from(format!(
                "{:<14} {} {:>6.1}%   IKI {:>7.1} ms   n={}",
                group_label(&row.name, lang),
                Msg::AccuracyLabel.text(lang),
                row.accuracy * 100.0,
                row.iki_median_ms,
                row.n
            ))
        })
        .collect::<Vec<_>>();
    lines.push(Line::from(""));
    for row in &app.analysis_details.hands {
        lines.push(Line::from(format!(
            "{} {:<8} {} {:>6.1}%   IKI {:>7.1} ms   n={}",
            Msg::HandLabel.text(lang),
            group_label(&row.name, lang),
            Msg::AccuracyLabel.text(lang),
            row.accuracy * 100.0,
            row.iki_median_ms,
            row.n
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(format!(
        "{}: {:.1} ms    {}: {:.1} ms",
        Msg::SameFingerDifferentKeys.text(lang),
        app.analysis_details.same_finger_median_ms,
        Msg::AlternatingHands.text(lang),
        app.analysis_details.alternating_hands_median_ms
    )));
    lines.push(Line::from(
        match (
            app.analysis_details.recovery_keys_median,
            app.analysis_details.post_error_three_error_rate,
        ) {
            (Some(keys), Some(rate)) => match lang {
                Lang::En => format!("Recovery after error: {keys:.1} keys    next 3 error rate: {:.1}%", rate * 100.0),
                Lang::Ja => format!("誤打後の立ち直り: {keys:.1} 打鍵    直後 3 打鍵の誤打率: {:.1}%", rate * 100.0),
            },
            _ => Msg::NoData.text(lang).to_owned(),
        },
    ));
    frame.render_widget(Paragraph::new(lines), area);
}

fn group_label(name: &str, lang: Lang) -> String {
    if lang == Lang::En {
        return name.to_owned();
    }
    match name {
        "left-pinky" => "左小指",
        "left-ring" => "左薬指",
        "left-middle" => "左中指",
        "left-index" => "左人差指",
        "right-index" => "右人差指",
        "right-middle" => "右中指",
        "right-ring" => "右薬指",
        "right-pinky" => "右小指",
        "thumb" => "親指",
        "left" => "左",
        "right" => "右",
        other => other,
    }
    .to_owned()
}

fn render_bench(frame: &mut Frame, app: &App, area: Rect) {
    let lang = app.config.general.lang;
    let mut lines = Vec::new();
    for part in ["A", "B", "C", "D"] {
        let mut values = bench_values(app, part, |record| record.header.net_wpm);
        if values.is_empty() {
            continue;
        }
        let latest = *values.last().unwrap_or(&0.0);
        let previous_delta = values.iter().rev().nth(1).map(|previous| latest - previous);
        values.pop();
        let start = values.len().saturating_sub(3);
        let recent_delta = (!values.is_empty()).then(|| latest - median(&values.split_off(start)));
        let part_name = match part {
            "A" => Msg::BenchPartA.text(lang),
            "B" => Msg::BenchPartB.text(lang),
            "C" => Msg::BenchPartC.text(lang),
            _ => Msg::BenchPartD.text(lang),
        };
        lines.push(Line::from(format!(
            "{part_name}: {latest:>6.1} WPM   {} {}   {} {}",
            Msg::PreviousDelta.text(lang),
            delta_label(previous_delta, lang),
            Msg::RecentThreeDelta.text(lang),
            delta_label(recent_delta, lang)
        )));
    }
    lines.push(Line::from(""));
    for value in crate::analysis::summary::skill_axis_values(&app.analysis_records, app.practice_layout(), &app.config) {
        let name = pad_display(value.axis.label(lang), 16);
        let Some(raw) = value.raw else {
            lines.push(Line::from(format!("{name}{}", Msg::NoData.text(lang))));
            continue;
        };
        let (bar, suffix) = if let Some(position) = value.percentile {
            (
                "█".repeat((position / 5.0).round() as usize),
                format!("{position:>5.1} {}", Msg::Percentile.text(lang)),
            )
        } else {
            (String::new(), Msg::RawFewerThree.text(lang).to_owned())
        };
        lines.push(Line::from(format!("{name}{raw:>10.2}  {}  {suffix}", pad_display(&bar, 20))));
    }
    frame.render_widget(Paragraph::new(lines).block(Block::default().title(Msg::BenchEightAxes.text(lang))), area);
}

fn bench_values<T>(app: &App, part: &str, value: impl Fn(&crate::record::SessionRecord) -> T) -> Vec<T> {
    app.analysis_records
        .iter()
        .filter(|record| {
            record
                .header
                .bench_id
                .as_deref()
                .is_some_and(|id| id.starts_with(&format!("bench-v1/{part}/")))
        })
        .map(value)
        .collect()
}

fn delta_label(value: Option<f64>, lang: Lang) -> String {
    value.map_or_else(|| Msg::InitialResult.text(lang).to_owned(), |value| format!("{value:+.1} WPM"))
}

fn pad_display(value: &str, width: usize) -> String {
    let padding = width.saturating_sub(value.width());
    format!("{value}{}", " ".repeat(padding))
}

fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}
