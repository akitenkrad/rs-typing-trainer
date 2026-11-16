use std::collections::{BTreeMap, BTreeSet};

use chrono::Duration as ChronoDuration;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect, Size};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph};
use ratatui_image::Image;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{AnalysisPeriod, App};
use crate::bench::{BenchPart, parse_bench_id, preferred_bench_series};
use crate::graphics::GraphicsMode;
use crate::i18n::{Lang, Msg};
use crate::layout::geometry::geometry;
use crate::ui::analysis_image::{
    ComparisonBarsSpec, FORECAST_EN_COLOR, FORECAST_JA_COLOR, FingerChartSpec, GradeLadderSpec, MetricKind, PixelPoint, RadarAxis, RadarChartSpec,
    RadarRing, SERIES_COLORS, SparklineSpec, TransitionArrowSpec, TransitionBar, TransitionBarsSpec, TransitionKeyboardSpec, TrendChartSpec,
    TrendDateRange, TrendForecast, TrendPoint, build_finger_bars, build_text_trend_data, build_trend_data, metric_value, mode_label,
    shared_trend_date_range,
};
use crate::ui::keyboard::{
    KeyboardState, KeyboardStyle, KeyboardView, finger_color, heatmap_color, keycap_center_cells, keycap_image_area, keycap_tile_placements,
    resolve_style,
};
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
        render_coach_images(frame, app, theme, area)
            .or_else(|| render_grade_badges(frame, app, theme, area))
            .unwrap_or(area)
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

fn render_coach_images(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) -> Option<Rect> {
    if app.graphics.mode == GraphicsMode::Off || app.graphics.cell_pixels.is_none() || area.width < 36 {
        return None;
    }
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
    let ladder_height = (grades.len() as u16).saturating_mul(4);
    let radar_height = if area.height >= 30 {
        area.height.saturating_sub(ladder_height + 8).clamp(12, 16)
    } else {
        0
    };
    let visual_height = ladder_height.saturating_add(radar_height).min(area.height.saturating_sub(1));
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(visual_height), Constraint::Min(1)])
        .split(area);
    let ladder_area = Rect::new(chunks[0].x, chunks[0].y, chunks[0].width, ladder_height.min(chunks[0].height));
    let ladder_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints((0..grades.len()).map(|_| Constraint::Length(4)).collect::<Vec<_>>())
        .split(ladder_area);
    let label_width = match app.config.general.lang {
        Lang::En => 10,
        Lang::Ja => 8,
    }
    .min(area.width.saturating_sub(8));
    let colors = grade_colors(theme);
    let ladders = grades
        .iter()
        .zip(ladder_rows.iter().copied())
        .map(|((grade, overall), row)| {
            let image_area = Rect::new(row.x + label_width, row.y, row.width.saturating_sub(label_width), row.height);
            (
                *grade,
                *overall,
                GradeLadderSpec {
                    score: overall.score,
                    level: overall.grade,
                    colors,
                    cells: image_area.as_size(),
                },
                row,
                image_area,
            )
        })
        .collect::<Vec<_>>();
    let radar_columns = (radar_height > 0).then(|| {
        let radar_area = Rect::new(
            chunks[0].x,
            chunks[0].y + ladder_height,
            chunks[0].width,
            radar_height.min(chunks[0].height.saturating_sub(ladder_height)),
        );
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints((0..grades.len()).map(|_| Constraint::Ratio(1, grades.len() as u32)).collect::<Vec<_>>())
            .split(radar_area)
    });
    let radars = radar_columns
        .as_deref()
        .map(|columns| {
            grades
                .iter()
                .zip(columns.iter().copied())
                .map(|((grade, overall), image_area)| {
                    (
                        radar_for_language(grade, overall, app.config.general.lang, theme, image_area.as_size()),
                        image_area,
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let font = app.bigtext_images.borrow_mut().resolved_font(&app.config.display.big_font, &app.home_dir);
    let mut images = app.analysis_images.borrow_mut();
    if ladders
        .iter()
        .any(|(_, _, spec, _, _)| images.prepare_ladder(&app.graphics, &font, spec).is_err())
        || radars.iter().any(|(spec, _)| images.prepare_radar(&app.graphics, &font, spec).is_err())
    {
        return None;
    }
    for (grade, overall, spec, row, image_area) in &ladders {
        frame.render_widget(
            Paragraph::new(language_label(&grade.language, app.config.general.lang))
                .style(Style::default().fg(grade_color(overall.grade, theme)).add_modifier(Modifier::BOLD))
                .alignment(Alignment::Right),
            Rect::new(row.x, row.y + 2, label_width.saturating_sub(1), 1),
        );
        let protocol = images.ladder_protocol(&app.graphics, &font, spec)?;
        frame.render_widget(Image::new(protocol), *image_area);
    }
    for (spec, image_area) in &radars {
        let protocol = images.radar_protocol(&app.graphics, &font, spec)?;
        frame.render_widget(Image::new(protocol), *image_area);
    }
    Some(chunks[1])
}

fn radar_for_language(
    grade: &crate::analysis::grades::LanguageGrade,
    overall: &crate::analysis::grades::OverallGrade,
    lang: Lang,
    theme: &Theme,
    cells: Size,
) -> RadarChartSpec {
    let mut axes = grade
        .components
        .iter()
        .map(|row| RadarAxis {
            label: format!("{} {}", component_label(row.component, lang), row.grade.as_str()),
            value: Some(row.score),
        })
        .collect::<Vec<_>>();
    axes.extend(grade.missing.iter().map(|component| RadarAxis {
        label: component_label(*component, lang).to_owned(),
        value: None,
    }));
    RadarChartSpec {
        axes,
        max_value: 8.0,
        rings: (1..=8)
            .map(|value| RadarRing {
                value: f64::from(value),
                label: match value {
                    4 => Some("C".into()),
                    8 => Some("S".into()),
                    _ => None,
                },
            })
            .collect(),
        color: grade_color(overall.grade, theme),
        cells,
        missing_label: match lang {
            Lang::En => "no data",
            Lang::Ja => "データなし",
        }
        .into(),
    }
}

fn grade_colors(theme: &Theme) -> [Color; 8] {
    use crate::analysis::grades::Level;
    [Level::G, Level::F, Level::E, Level::D, Level::C, Level::B, Level::A, Level::S].map(|level| grade_color(level, theme))
}

fn language_label(language: &str, lang: Lang) -> &'static str {
    match (language, lang) {
        ("ja", Lang::En) => "Japanese",
        ("ja", Lang::Ja) => "日本語",
        (_, Lang::En) => "English",
        (_, Lang::Ja) => "英語",
    }
}

fn component_label(component: crate::analysis::grades::Component, lang: Lang) -> &'static str {
    use crate::analysis::grades::Component;
    match (component, lang) {
        (Component::Speed, Lang::En) => "Speed",
        (Component::Speed, Lang::Ja) => "速さ",
        (Component::Accuracy, Lang::En) => "Accuracy",
        (Component::Accuracy, Lang::Ja) => "正確さ",
        (Component::Rhythm, Lang::En) => "Rhythm",
        (Component::Rhythm, Lang::Ja) => "リズム",
        (Component::Transition, Lang::En) => "Transitions",
        (Component::Transition, Lang::Ja) => "遷移",
        (Component::Preview, Lang::En) => "Preview",
        (Component::Preview, Lang::Ja) => "先読み",
        (Component::Recovery, Lang::En) => "Recovery",
        (Component::Recovery, Lang::Ja) => "立ち直り",
        (Component::Overlap, Lang::En) => "Overlap",
        (Component::Overlap, Lang::Ja) => "重なり",
    }
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
    let kinds = [MetricKind::NetWpm, MetricKind::Accuracy, MetricKind::Iki];
    let image_enabled = app.graphics.mode != GraphicsMode::Off && app.graphics.cell_pixels.is_some();
    let mut image_specs = kinds.map(|kind| {
        if image_enabled {
            build_trend_data(&app.analysis_records, kind, lang)
        } else {
            None
        }
    });
    if let Some(spec) = &mut image_specs[0] {
        spec.forecasts = trend_forecasts(app);
    }
    let x_range = shared_trend_date_range(image_specs.iter().flatten());
    for ((area, kind), spec) in chunks.iter().copied().zip(kinds).zip(image_specs) {
        render_metric_chart(frame, app, area, kind, lang, spec, x_range);
    }
}

fn render_metric_chart(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    kind: MetricKind,
    lang: Lang,
    spec: Option<TrendChartSpec>,
    x_range: Option<TrendDateRange>,
) {
    if let (Some(mut spec), Some(x_range)) = (spec, x_range)
        && render_metric_chart_image(frame, app, area, lang, &mut spec, x_range)
    {
        return;
    }
    render_metric_chart_text(frame, &app.analysis_records, area, kind, lang);
}

fn render_metric_chart_text(frame: &mut Frame, records: &[crate::record::SessionRecord], area: Rect, kind: MetricKind, lang: Lang) {
    let title = match kind {
        MetricKind::NetWpm => Msg::TrendNetWpm.text(lang),
        MetricKind::Accuracy => Msg::TrendAccuracy.text(lang),
        MetricKind::Iki => Msg::TrendIki.text(lang),
    };
    let Some(spec) = build_text_trend_data(records, kind, lang) else {
        let current = records
            .iter()
            .rev()
            .find(|record| kind != MetricKind::Iki || record.header.metrics_version != 0)
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
    };
    let dates = spec
        .series
        .iter()
        .flat_map(|series| series.points.iter().map(|(date, _)| date.clone()))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let date_indices = dates
        .iter()
        .enumerate()
        .map(|(index, date)| (date, index as f64))
        .collect::<BTreeMap<_, _>>();
    let points = spec
        .series
        .iter()
        .map(|series| {
            series
                .points
                .iter()
                .map(|(date, value)| (*date_indices.get(date).expect("date index"), *value))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let datasets = points
        .iter()
        .enumerate()
        .map(|(index, values)| {
            Dataset::default()
                .name(spec.series[index].name.clone())
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(spec.series[index].color))
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

fn trend_forecasts(app: &App) -> Vec<TrendForecast> {
    let Some(report) = &app.analysis_coach else {
        return Vec::new();
    };
    report
        .progress
        .curves
        .iter()
        .filter(|curve| matches!(curve.target.as_str(), "en" | "ja"))
        .filter_map(|curve| {
            let trend = report.progress.trends.iter().find(|trend| trend.target == curve.target)?;
            let last = trend.speed.last()?;
            Some(TrendForecast {
                target: curve.target.clone(),
                start: TrendPoint {
                    date: last.date,
                    value: last.value,
                },
                end_date: last.date.checked_add_signed(ChronoDuration::days(i64::from(curve.forecast_days)))?,
                forecast: curve.forecast,
                band_lo: curve.band_lo,
                band_hi: curve.band_hi,
                forecast_days: curve.forecast_days,
                color: if curve.target == "ja" { FORECAST_JA_COLOR } else { FORECAST_EN_COLOR },
            })
        })
        .collect()
}

fn render_metric_chart_image(frame: &mut Frame, app: &App, area: Rect, lang: Lang, spec: &mut TrendChartSpec, x_range: TrendDateRange) -> bool {
    if area.width <= 8 || area.height <= 3 {
        return false;
    }
    let title = match spec.kind {
        MetricKind::NetWpm => Msg::TrendNetWpm.text(lang),
        MetricKind::Accuracy => Msg::TrendAccuracy.text(lang),
        MetricKind::Iki => Msg::TrendIki.text(lang),
    };
    let y_label_width = 8_u16.min(area.width.saturating_sub(1));
    let plot_area = Rect::new(
        area.x + y_label_width,
        area.y + 2,
        area.width - y_label_width,
        area.height.saturating_sub(3),
    );
    if plot_area.width == 0 || plot_area.height == 0 {
        return false;
    }
    spec.cells = plot_area.as_size();
    let Some((min_y, max_y)) = spec.y_bounds() else {
        return false;
    };
    let mut images = app.analysis_images.borrow_mut();
    if images.prepare_chart_with_range(&app.graphics, spec, x_range).is_err() {
        return false;
    }
    frame.render_widget(
        Paragraph::new(title).style(Style::default().add_modifier(Modifier::BOLD)),
        Rect::new(area.x, area.y, area.width, 1),
    );
    let mut legend = Vec::new();
    for series in &spec.series {
        legend.push(Span::styled("■ ", Style::default().fg(series.color)));
        legend.push(Span::raw(format!("{}  ", series.name)));
    }
    for forecast in &spec.forecasts {
        legend.push(Span::styled("■ ", Style::default().fg(forecast.color)));
        legend.push(Span::raw(format!("{}  ", forecast_label(forecast, lang))));
    }
    frame.render_widget(Paragraph::new(Line::from(legend)), Rect::new(area.x, area.y + 1, area.width, 1));
    frame.render_widget(
        Paragraph::new(format!("{max_y:.1}")).alignment(Alignment::Right),
        Rect::new(area.x, plot_area.y, y_label_width.saturating_sub(1), 1),
    );
    frame.render_widget(
        Paragraph::new(format!("{min_y:.1}")).alignment(Alignment::Right),
        Rect::new(
            area.x,
            plot_area.y + plot_area.height.saturating_sub(1),
            y_label_width.saturating_sub(1),
            1,
        ),
    );
    let label_area = Rect::new(plot_area.x, area.y + area.height.saturating_sub(1), plot_area.width, 1);
    frame.render_widget(Paragraph::new(x_range.start.to_string()), label_area);
    frame.render_widget(Paragraph::new(Msg::TrendDay.text(lang)).alignment(Alignment::Center), label_area);
    frame.render_widget(Paragraph::new(x_range.end.to_string()).alignment(Alignment::Right), label_area);
    let Some(protocol) = images.chart_protocol_with_range(&app.graphics, spec, x_range) else {
        return false;
    };
    frame.render_widget(Image::new(protocol), plot_area);
    true
}

fn forecast_label(forecast: &TrendForecast, lang: Lang) -> String {
    let language = match (forecast.target.as_str(), lang) {
        ("ja", Lang::En) => "Japanese",
        ("ja", Lang::Ja) => "日本語",
        (_, Lang::En) => "English",
        (_, Lang::Ja) => "英語",
    };
    match lang {
        Lang::En => format!("Forecast ({language}, {} d)", forecast.forecast_days),
        Lang::Ja => format!("予測（{language}・{} 日後）", forecast.forecast_days),
    }
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
    let mut legend_maximum = 0.0;
    let heatmap = match app.analysis_keyboard_view {
        0 | 1 => {
            let value = |row: &crate::analysis::metrics::KeyMetric| {
                if app.analysis_keyboard_view == 1 {
                    row.iki_median_ms
                } else {
                    1.0 - row.accuracy
                }
            };
            legend_maximum = app.analysis_details.keys.iter().map(value).fold(0.0_f64, f64::max);
            let maximum = legend_maximum.max(0.01);
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
            legend_maximum = rows.iter().map(|row| row.n).max().unwrap_or(0) as f64;
            let maximum = legend_maximum.max(1.0);
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
    crate::ui::keyboard::render_heatmap_with_images(
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
        crate::ui::keyboard::KeyboardImageContext {
            detection: &app.graphics,
            cache: &app.keyboard_images,
        },
    );
    if let Some(style) = resolve_style(&app.config.keyboard.style, area, true) {
        let keyboard_height = style.height(true);
        let keyboard_y = area.y + area.height.saturating_sub(keyboard_height) / 2;
        let legend_y = keyboard_y.saturating_add(keyboard_height);
        if legend_y < area.y.saturating_add(area.height)
            && let Some(legend) = keyboard_legend(
                app.analysis_keyboard_view,
                app.config.general.lang,
                legend_maximum,
                area.width,
                keyboard_no_data_color(app, style),
            )
        {
            frame.render_widget(
                Paragraph::new(legend).alignment(Alignment::Center),
                Rect::new(area.x, legend_y, area.width, 1),
            );
        }
    }
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

fn keyboard_no_data_color(app: &App, style: KeyboardStyle) -> Color {
    if style == KeyboardStyle::Raised
        || (style == KeyboardStyle::Keycap && app.graphics.mode != GraphicsMode::Off && app.graphics.cell_pixels.is_some())
    {
        Color::Rgb(75, 81, 91)
    } else {
        Color::Reset
    }
}

fn keyboard_legend(view: usize, lang: Lang, maximum: f64, width: u16, no_data_color: Color) -> Option<Line<'static>> {
    let mut parts = KeyboardLegendParts {
        show_no_data: true,
        show_note: true,
        show_words: true,
    };
    let mut band_len = 16;
    loop {
        let line = keyboard_legend_line(view, lang, maximum, band_len, parts, no_data_color);
        if line.width() <= usize::from(width) {
            return Some(line);
        }
        if parts.show_no_data {
            parts.show_no_data = false;
        } else if parts.show_note {
            parts.show_note = false;
        } else if parts.show_words {
            parts.show_words = false;
        } else if band_len > 12 {
            band_len -= 1;
        } else {
            return None;
        }
    }
}

#[derive(Clone, Copy)]
struct KeyboardLegendParts {
    show_words: bool,
    show_note: bool,
    show_no_data: bool,
}

fn keyboard_legend_line(view: usize, lang: Lang, maximum: f64, band_len: usize, parts: KeyboardLegendParts, no_data_color: Color) -> Line<'static> {
    let (left, middle, right, trailing) = match view {
        0 => ("0% ".to_owned(), None, format!(" {:.1}%", maximum * 100.0), String::new()),
        1 => (
            if parts.show_words { "0 ms ".to_owned() } else { "0 ".to_owned() },
            None,
            if parts.show_words {
                format!(" {maximum:.0} ms")
            } else {
                format!(" {maximum:.0}")
            },
            String::new(),
        ),
        2 => {
            let (fast, slow, median) = match lang {
                Lang::En => ("fast ", " slow", "(1.0 = personal median)"),
                Lang::Ja => ("速い ", " 遅い", "(1.0 = 本人の中央値)"),
            };
            (
                format!("{}0.0 ", if parts.show_words { fast } else { "" }),
                Some("1.0"),
                format!(" 2.0+{}", if parts.show_words { slow } else { "" }),
                if parts.show_note { format!("   {median}") } else { String::new() },
            )
        }
        _ => {
            let count = maximum as u64;
            let right = match (lang, parts.show_words) {
                (Lang::En, true) => format!(" {count} times"),
                (Lang::Ja, true) => format!(" {count} 回"),
                (_, false) => format!(" {count}"),
            };
            ("0 ".to_owned(), None, right, String::new())
        }
    };

    let mut spans = vec![Span::raw(left)];
    let mut band = vec![' '; band_len];
    if let Some(middle) = middle {
        let start = band_len.saturating_sub(middle.len()) / 2;
        for (offset, character) in middle.chars().enumerate() {
            if let Some(cell) = band.get_mut(start + offset) {
                *cell = character;
            }
        }
    }
    let denominator = band_len.saturating_sub(1).max(1) as f64;
    spans.extend(band.into_iter().enumerate().map(|(index, character)| {
        Span::styled(
            character.to_string(),
            Style::default().fg(Color::White).bg(heatmap_color(index as f64 / denominator)),
        )
    }));
    spans.push(Span::raw(right));
    spans.push(Span::raw(trailing));

    if view >= 3 && parts.show_words {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            " ",
            Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(match lang {
            Lang::En => " expected key",
            Lang::Ja => " 期待したキー",
        }));
    }
    if parts.show_no_data {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(" ", Style::default().bg(no_data_color)));
        spans.push(Span::raw(match lang {
            Lang::En => " no data",
            Lang::Ja => " データなし",
        }));
    }
    Line::from(spans)
}

fn render_transitions(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let lang = app.config.general.lang;
    let minimum = app.config.analysis.min_transition_n;
    let mut slow = app.analysis_details.bigrams.clone();
    slow.sort_by(|left, right| right.estimated_iki_ms.total_cmp(&left.estimated_iki_ms));
    let mut error_prone = app.analysis_details.bigrams.clone();
    error_prone.sort_by(|left, right| right.estimated_error_rate.total_cmp(&left.estimated_error_rate));
    if render_transition_images(frame, app, theme, area, &slow, &error_prone, minimum) {
        return;
    }
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

fn render_transition_images(
    frame: &mut Frame,
    app: &App,
    theme: &Theme,
    area: Rect,
    slow: &[crate::analysis::metrics::BigramMetric],
    error_prone: &[crate::analysis::metrics::BigramMetric],
    minimum: u64,
) -> bool {
    if app.graphics.mode == GraphicsMode::Off || app.graphics.cell_pixels.is_none() || area.width < 70 {
        return false;
    }
    let slow = slow.iter().filter(|row| row.n >= minimum).take(20).collect::<Vec<_>>();
    let error_prone = error_prone.iter().filter(|row| row.n >= minimum).take(20).collect::<Vec<_>>();
    if slow.is_empty() || error_prone.is_empty() {
        return false;
    }
    let insufficient = app
        .analysis_details
        .bigrams
        .iter()
        .filter(|row| row.n < minimum)
        .take(5)
        .collect::<Vec<_>>();
    let insufficient_height = if insufficient.is_empty() { 0 } else { 2 + insufficient.len() as u16 };
    let keyboard_height = 11_u16;
    let show_keyboard =
        area.height.saturating_mul(2) / 5 >= keyboard_height && area.height >= keyboard_height.saturating_add(insufficient_height).saturating_add(7);
    let visual_height = if show_keyboard { keyboard_height } else { 0 };
    let graph_height = area.height.saturating_sub(visual_height);
    let row_count = usize::from(graph_height.saturating_sub(1 + insufficient_height)).min(20);
    if row_count < 5 {
        return false;
    }
    let slow_max_error = slow.iter().map(|row| row.estimated_error_rate).fold(0.0_f64, f64::max);
    let error_max_error = error_prone.iter().map(|row| row.estimated_error_rate).fold(0.0_f64, f64::max);
    let slow = &slow[..slow.len().min(row_count)];
    let error_prone = &error_prone[..error_prone.len().min(row_count)];
    let graph_area = Rect::new(area.x, area.y + visual_height, area.width, graph_height);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
        .split(graph_area);
    let slow_spec = TransitionBarsSpec {
        rows: slow
            .iter()
            .enumerate()
            .map(|(index, row)| TransitionBar {
                value: row.estimated_iki_ms,
                error_rate: row.estimated_error_rate,
                selected: index == app.analysis_transition_selected,
            })
            .collect(),
        max_error_rate: slow_max_error,
        cells: Size::new(columns[0].width.saturating_sub(28).max(1), slow.len() as u16),
    };
    let error_spec = TransitionBarsSpec {
        rows: error_prone
            .iter()
            .map(|row| TransitionBar {
                value: row.estimated_error_rate,
                error_rate: row.estimated_error_rate,
                selected: false,
            })
            .collect(),
        max_error_rate: error_max_error,
        cells: Size::new(columns[1].width.saturating_sub(23).max(1), error_prone.len() as u16),
    };
    let keyboard = if show_keyboard {
        let Some(mut physical) = geometry(&app.config.keyboard.geometry) else {
            return false;
        };
        if app.config.keyboard.digit6_finger == "right-index" {
            physical = physical.with_digit6_finger(crate::layout::geometry::Finger::RightIndex);
        }
        Some(physical)
    } else {
        None
    };
    let keyboard_spec = keyboard.as_ref().map(|physical| {
        let selected = slow.get(app.analysis_transition_selected).copied();
        let point = |value: &str| -> Option<PixelPoint> {
            let stroke = app.practice_layout().stroke_for(value.chars().next()?)?;
            keycap_center_cells(physical, &stroke.code)
        };
        let cells = Size::new(KeyboardStyle::Keycap.width(), KeyboardStyle::Keycap.height(false));
        TransitionKeyboardSpec {
            cells,
            tiles: keycap_tile_placements(
                physical,
                app.practice_layout(),
                &KeyboardState::default(),
                KeyboardView {
                    show: "full",
                    style: "keycap",
                    finger_guide: false,
                },
                true,
            ),
            arrow: TransitionArrowSpec {
                cells,
                from: selected.and_then(|row| point(&row.from)),
                to: selected.and_then(|row| point(&row.to)),
                same_key: selected.is_some_and(|row| row.from == row.to),
                color: theme.accent,
            },
            selection: selected.map(|row| (row.from.clone(), row.to.clone())),
        }
    });
    {
        let mut images = app.analysis_images.borrow_mut();
        if images.prepare_transition_bars(&app.graphics, &slow_spec).is_err()
            || images.prepare_transition_bars(&app.graphics, &error_spec).is_err()
            || keyboard_spec
                .as_ref()
                .is_some_and(|spec| images.prepare_transition_keyboard(&app.graphics, spec).is_err())
        {
            return false;
        }
    }
    if let Some(keyboard_spec) = keyboard_spec.as_ref() {
        let keyboard_area = Rect::new(area.x, area.y, area.width, keyboard_height);
        let image_area = keycap_image_area(keyboard_area, false);
        if let Some(protocol) = app
            .analysis_images
            .borrow_mut()
            .transition_keyboard_protocol(&app.graphics, keyboard_spec)
        {
            frame.render_widget(Image::new(protocol), image_area);
        }
    }
    render_transition_bar_column(frame, columns[0], slow, &slow_spec, app.config.general.lang, true, app, theme);
    render_transition_bar_column(frame, columns[1], error_prone, &error_spec, app.config.general.lang, false, app, theme);
    if !insufficient.is_empty() {
        let y = graph_area.y + 1 + row_count as u16;
        let mut lines = vec![Line::from(match app.config.general.lang {
            Lang::En => format!("insufficient data (n < {minimum})"),
            Lang::Ja => format!("データ不足（n < {minimum}）"),
        })];
        lines.extend(
            insufficient
                .into_iter()
                .map(|row| Line::from(format!("    {}→{}  n={}", row.from, row.to, row.n))),
        );
        frame.render_widget(
            Paragraph::new(lines),
            Rect::new(area.x, y, area.width, insufficient_height.saturating_sub(1)),
        );
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn render_transition_bar_column(
    frame: &mut Frame,
    area: Rect,
    rows: &[&crate::analysis::metrics::BigramMetric],
    spec: &TransitionBarsSpec,
    lang: Lang,
    slow: bool,
    app: &App,
    theme: &Theme,
) {
    let title = if slow {
        format!("{}   {}", Msg::SlowTransitionsTop20.text(lang), Msg::PracticeTransition.text(lang))
    } else {
        Msg::ErrorProneTransitionsTop20.text(lang).to_owned()
    };
    frame.render_widget(
        Paragraph::new(title).style(Style::default().add_modifier(Modifier::BOLD)),
        Rect::new(area.x, area.y, area.width, 1),
    );
    let label_width = 9_u16.min(area.width.saturating_sub(1));
    let value_width = if slow { 19 } else { 14 }.min(area.width.saturating_sub(label_width + 1));
    let bar_area = Rect::new(
        area.x + label_width,
        area.y + 1,
        area.width.saturating_sub(label_width + value_width),
        rows.len() as u16,
    );
    if let Some(protocol) = app.analysis_images.borrow_mut().transition_bars_protocol(&app.graphics, spec) {
        frame.render_widget(Image::new(protocol), bar_area);
    }
    for (index, row) in rows.iter().enumerate() {
        let selected = slow && index == app.analysis_transition_selected;
        let style = if selected {
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let marker = if selected { "▶" } else { " " };
        frame.render_widget(
            Paragraph::new(format!("{marker}{}→{}", row.from, row.to)).style(style),
            Rect::new(area.x, area.y + 1 + index as u16, label_width, 1),
        );
        let value = if slow {
            format!("{:>6.1} ms {:>4.1}% n={}", row.estimated_iki_ms, row.estimated_error_rate * 100.0, row.n)
        } else {
            format!("{:>5.1}% n={}", row.estimated_error_rate * 100.0, row.n)
        };
        frame.render_widget(
            Paragraph::new(value).style(style),
            Rect::new(bar_area.x + bar_area.width, area.y + 1 + index as u16, value_width, 1),
        );
    }
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
    if render_finger_images(frame, app, area, &lines) {
        return;
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_finger_images(frame: &mut Frame, app: &App, area: Rect, lines: &[Line<'_>]) -> bool {
    if app.graphics.mode == GraphicsMode::Off || area.width < 58 {
        return false;
    }
    let Some(cell_pixels) = app.graphics.cell_pixels else {
        return false;
    };
    let raw_height = lines.len().min(usize::from(u16::MAX)) as u16;
    let available = area.height.saturating_sub(raw_height);
    if available < 7 {
        return false;
    }
    let visual_height = available.min(12);
    let visual_area = Rect::new(area.x, area.y, area.width, visual_height);
    let raw_area = Rect::new(area.x, area.y + visual_height, area.width, area.height.saturating_sub(visual_height));
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(3, 5), Constraint::Ratio(2, 5)])
        .split(visual_area);
    let axis_width = 9_u16.min(columns[0].width.saturating_sub(10));
    let chart_area = Rect::new(
        columns[0].x + axis_width,
        columns[0].y + 1,
        columns[0].width.saturating_sub(axis_width),
        visual_height.saturating_sub(2),
    );
    let comparison_label_width = match app.config.general.lang {
        Lang::En => 18,
        Lang::Ja => 16,
    }
    .min(columns[1].width.saturating_sub(10));
    let comparison_value_width = 8_u16.min(columns[1].width.saturating_sub(comparison_label_width + 2));
    let comparison_area = Rect::new(
        columns[1].x + comparison_label_width,
        columns[1].y + 1,
        columns[1].width.saturating_sub(comparison_label_width + comparison_value_width),
        4,
    );
    if chart_area.width == 0 || chart_area.height == 0 || comparison_area.width == 0 {
        return false;
    }
    let bars = build_finger_bars(&app.analysis_details.fingers);
    let finger_spec = FingerChartSpec {
        bars,
        cells: chart_area.as_size(),
        low_sample_n: 30,
    };
    let hand_iki = |name: &str| {
        app.analysis_details
            .hands
            .iter()
            .find(|row| row.name == name)
            .map_or(0.0, |row| row.iki_median_ms)
    };
    let comparison_spec = ComparisonBarsSpec {
        values: [
            hand_iki("left"),
            hand_iki("right"),
            app.analysis_details.same_finger_median_ms,
            app.analysis_details.alternating_hands_median_ms,
        ],
        colors: [
            finger_color(crate::layout::geometry::Finger::LeftIndex),
            finger_color(crate::layout::geometry::Finger::RightIndex),
            Color::LightMagenta,
            Color::LightYellow,
        ],
        cells: comparison_area.as_size(),
    };
    {
        let mut images = app.analysis_images.borrow_mut();
        if images.prepare_finger_chart(&app.graphics, &finger_spec).is_err()
            || images.prepare_comparison_bars(&app.graphics, &comparison_spec).is_err()
        {
            return false;
        }
    }
    let lang = app.config.general.lang;
    frame.render_widget(
        Paragraph::new(match lang {
            Lang::En => "Finger IKI and error rate",
            Lang::Ja => "指ごとの IKI と誤り率",
        })
        .style(Style::default().add_modifier(Modifier::BOLD)),
        Rect::new(columns[0].x, columns[0].y, columns[0].width, 1),
    );
    let maximum_iki = finger_spec.bars.iter().filter_map(|bar| bar.iki_median_ms).fold(0.0_f64, f64::max);
    let maximum_error = finger_spec.bars.iter().filter_map(|bar| bar.error_rate).fold(0.0_f64, f64::max);
    let axis = match lang {
        Lang::En => format!("IKI ↑\n{maximum_iki:.0} ms\n\nError ↓\n{:.1}%", maximum_error * 100.0),
        Lang::Ja => format!("IKI ↑\n{maximum_iki:.0} ms\n\n誤り ↓\n{:.1}%", maximum_error * 100.0),
    };
    frame.render_widget(Paragraph::new(axis), Rect::new(columns[0].x, chart_area.y, axis_width, chart_area.height));
    if let Some(protocol) = app.analysis_images.borrow_mut().finger_chart_protocol(&app.graphics, &finger_spec) {
        frame.render_widget(Image::new(protocol), chart_area);
    }
    let chart_pixel_width = u32::from(chart_area.width) * u32::from(cell_pixels.width);
    let cell_pixel_width = u32::from(cell_pixels.width).max(1);
    for (index, bar) in finger_spec.bars.iter().enumerate() {
        let center_pixel = ((index as f64 + 0.5) * f64::from(chart_pixel_width) / finger_spec.bars.len() as f64) as u32;
        let center_cell = (center_pixel / cell_pixel_width) as u16;
        let x = chart_area.x + center_cell.min(chart_area.width.saturating_sub(1));
        let label = match (bar.name.as_str(), lang) {
            ("left-pinky" | "right-pinky", Lang::En) => 'P',
            ("left-ring" | "right-ring", Lang::En) => 'R',
            ("left-middle" | "right-middle", Lang::En) => 'M',
            ("left-index" | "right-index", Lang::En) => 'I',
            ("thumb" | "left-thumb" | "right-thumb", Lang::En) => 'T',
            ("left-pinky" | "right-pinky", Lang::Ja) => '小',
            ("left-ring" | "right-ring", Lang::Ja) => '薬',
            ("left-middle" | "right-middle", Lang::Ja) => '中',
            ("left-index" | "right-index", Lang::Ja) => '人',
            ("thumb" | "left-thumb" | "right-thumb", Lang::Ja) => '親',
            _ => '—',
        };
        let label = if bar.iki_median_ms.is_some() { label } else { '—' };
        let label = label.to_string();
        let label_width = UnicodeWidthStr::width(label.as_str()).min(usize::from(u16::MAX)) as u16;
        frame.render_widget(
            Paragraph::new(label),
            Rect::new(x, visual_area.y + visual_height - 1, label_width.max(1), 1),
        );
    }
    frame.render_widget(
        Paragraph::new(match lang {
            Lang::En => "Hand and fingering IKI",
            Lang::Ja => "手と運指の IKI",
        })
        .style(Style::default().add_modifier(Modifier::BOLD)),
        Rect::new(columns[1].x, columns[1].y, columns[1].width, 1),
    );
    if let Some(protocol) = app.analysis_images.borrow_mut().comparison_bars_protocol(&app.graphics, &comparison_spec) {
        frame.render_widget(Image::new(protocol), comparison_area);
    }
    let labels = match lang {
        Lang::En => ["Left hand", "Right hand", "Same finger", "Alternating"],
        Lang::Ja => ["左手", "右手", "同じ指・別キー", "左右交互"],
    };
    for (index, (label, value)) in labels.into_iter().zip(comparison_spec.values).enumerate() {
        let y = columns[1].y + 1 + index as u16;
        frame.render_widget(Paragraph::new(label), Rect::new(columns[1].x, y, comparison_label_width, 1));
        frame.render_widget(
            Paragraph::new(format!("{value:.0} ms")),
            Rect::new(comparison_area.x + comparison_area.width, y, comparison_value_width, 1),
        );
    }
    frame.render_widget(Paragraph::new(lines.to_vec()), raw_area);
    true
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
    let lines = bench_text_lines(app, lang);
    if render_bench_images(frame, app, area, &lines) {
        return;
    }
    frame.render_widget(Paragraph::new(lines).block(Block::default().title(Msg::BenchEightAxes.text(lang))), area);
}

fn bench_text_lines(app: &App, lang: Lang) -> Vec<Line<'static>> {
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
    lines
}

fn render_bench_images(frame: &mut Frame, app: &App, area: Rect, text_lines: &[Line<'static>]) -> bool {
    if app.graphics.mode == GraphicsMode::Off || app.graphics.cell_pixels.is_none() || area.width < 54 {
        return false;
    }
    let lang = app.config.general.lang;
    let parts = ["A", "B", "C", "D"]
        .into_iter()
        .filter_map(|part| {
            let values = bench_values(app, part, |record| record.header.net_wpm);
            (!values.is_empty()).then_some((part, values))
        })
        .collect::<Vec<_>>();
    let axes = crate::analysis::summary::skill_axis_values(&app.analysis_records, app.practice_layout(), &app.config);
    if parts.is_empty() && axes.iter().all(|value| value.raw.is_none()) {
        return false;
    }
    let block = Block::default().title(Msg::BenchEightAxes.text(lang));
    let inner = block.inner(area);
    let raw_height = text_lines.len().min(usize::from(inner.height)) as u16;
    let horizontal = inner.width >= 88;
    let visual_height = if horizontal { 14 } else { 20 };
    if inner.height < raw_height.saturating_add(visual_height) {
        return false;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(visual_height), Constraint::Min(raw_height)])
        .split(inner);
    let (radar_area, spark_area) = if horizontal {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
            .split(chunks[0]);
        (columns[0], columns[1])
    } else {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(12), Constraint::Min(8)])
            .split(chunks[0]);
        (rows[0], rows[1])
    };
    let theme = Theme::from_name(&app.config.ui.theme);
    let radar = RadarChartSpec {
        axes: axes
            .iter()
            .map(|value| RadarAxis {
                label: value.axis.label(lang).to_owned(),
                value: value.percentile,
            })
            .collect(),
        max_value: 100.0,
        rings: [25.0, 50.0, 75.0, 100.0]
            .into_iter()
            .map(|value| RadarRing {
                value,
                label: matches!(value as u32, 50 | 100).then(|| format!("{value:.0}")),
            })
            .collect(),
        color: theme.accent,
        cells: radar_area.as_size(),
        missing_label: match lang {
            Lang::En => "under 3",
            Lang::Ja => "3 回未満",
        }
        .into(),
    };
    let row_height = (spark_area.height / parts.len().max(1) as u16).max(1);
    let spark_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints((0..parts.len()).map(|_| Constraint::Length(row_height)).collect::<Vec<_>>())
        .split(spark_area);
    let sparklines = parts
        .iter()
        .zip(spark_rows.iter().copied())
        .enumerate()
        .map(|(index, ((part, values), row))| {
            let image_area = Rect::new(row.x, row.y + 1, row.width, row.height.saturating_sub(1));
            (
                *part,
                values,
                SparklineSpec {
                    values: values.clone(),
                    color: SERIES_COLORS[index % SERIES_COLORS.len()],
                    cells: image_area.as_size(),
                },
                row,
                image_area,
            )
        })
        .collect::<Vec<_>>();
    if radar_area.width == 0
        || radar_area.height == 0
        || sparklines
            .iter()
            .any(|(_, _, _, _, image_area)| image_area.width == 0 || image_area.height == 0)
    {
        return false;
    }
    let font = app.bigtext_images.borrow_mut().resolved_font(&app.config.display.big_font, &app.home_dir);
    let mut images = app.analysis_images.borrow_mut();
    if images.prepare_radar(&app.graphics, &font, &radar).is_err()
        || sparklines
            .iter()
            .any(|(_, _, spec, _, _)| images.prepare_sparkline(&app.graphics, spec).is_err())
    {
        return false;
    }
    frame.render_widget(block, area);
    let Some(protocol) = images.radar_protocol(&app.graphics, &font, &radar) else {
        return false;
    };
    frame.render_widget(Image::new(protocol), radar_area);
    for (part, values, spec, row, image_area) in &sparklines {
        let latest = values.last().copied().unwrap_or_default();
        let delta = values.iter().rev().nth(1).map(|previous| latest - previous);
        let part_name = bench_part_label(part, lang);
        let label_width = row.width.saturating_mul(3) / 5;
        frame.render_widget(Paragraph::new(part_name), Rect::new(row.x, row.y, label_width, 1));
        frame.render_widget(
            Paragraph::new(format!("{latest:.1} WPM  {}", delta_label(delta, lang))).alignment(Alignment::Right),
            Rect::new(row.x + label_width, row.y, row.width.saturating_sub(label_width), 1),
        );
        let Some(protocol) = images.sparkline_protocol(&app.graphics, spec) else {
            return false;
        };
        frame.render_widget(Image::new(protocol), *image_area);
    }
    frame.render_widget(Paragraph::new(text_lines.to_vec()), chunks[1]);
    true
}

fn bench_part_label(part: &str, lang: Lang) -> &'static str {
    match part {
        "A" => Msg::BenchPartA.text(lang),
        "B" => Msg::BenchPartB.text(lang),
        "C" => Msg::BenchPartC.text(lang),
        _ => Msg::BenchPartD.text(lang),
    }
}

fn bench_values<T>(app: &App, part: &str, value: impl Fn(&crate::record::SessionRecord) -> T) -> Vec<T> {
    let part = match part {
        "A" => BenchPart::A,
        "B" => BenchPart::B,
        "C" => BenchPart::C,
        _ => BenchPart::D,
    };
    let series = preferred_bench_series(app.analysis_records.iter().filter_map(|record| record.header.bench_id.as_deref()), part);
    app.analysis_records
        .iter()
        .filter(|record| {
            record
                .header
                .bench_id
                .as_deref()
                .and_then(parse_bench_id)
                .is_some_and(|(candidate_series, candidate_part, _)| Some(candidate_series) == series && candidate_part == part)
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
