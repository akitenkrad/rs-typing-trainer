use std::collections::HashMap;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect, Size};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui_image::Image;
use unicode_width::UnicodeWidthStr;

use crate::analysis::bench_summary::{BenchChange, BenchMetric, BenchmarkRunAnalysis, PracticeSuggestion, WeakFingerReason};
use crate::analysis::bench_v3::{
    BenchV3Group, compute_bench_v3_group_metrics, compute_bench_v3_group_reliability, compute_bench_v3_group_runs, compute_bench_v3_metrics,
    compute_bench_v3_part_reliability, previous_effective_wpm_delta,
};
use crate::analysis::grades::{Component, Level};
use crate::app::App;
use crate::bench::{BenchPart, parse_bench_id};
use crate::graphics::GraphicsMode;
use crate::i18n::{Lang, Msg};
use crate::record::SessionRecord;
use crate::ui::analysis_image::GradeBadgeSpec;
use crate::ui::keyboard::{KeyboardState, KeyboardStyle, KeyboardView, keycap_tile_placements};
use crate::ui::result_image::{
    BenchPartCardSpec, BenchTrendPoint, BenchTrendSpec, MetricDeltaKind, MistypeKeyboardSpec, ResultImageColors, format_metric_delta,
};
use crate::ui::{left_inset, theme::Theme};

pub fn render(frame: &mut Frame, app: &App) {
    let lang = app.config.general.lang;
    let theme = Theme::from_name(&app.config.ui.theme);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", Msg::BenchSummaryTitle.text(lang)));
    let inner = block.inner(frame.area());
    frame.render_widget(block, frame.area());
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(2)])
        .split(inner);

    if app.benchmark_series_v3() {
        render_v3_text(frame, app, &theme, rows[0]);
        render_hint(frame, app, &theme, rows[1]);
        return;
    }

    let graphics = app.graphics.mode != GraphicsMode::Off && app.graphics.cell_pixels.is_some();
    let cards = graphics && rows[0].width >= 90 && rows[0].height >= 25;
    let badge = cards && rows[0].width >= 100 && rows[0].height >= 27;
    let keyboard = badge && rows[0].width >= 110 && rows[0].height >= 30;
    let trends = keyboard && rows[0].width >= 120 && rows[0].height >= 34;
    if cards {
        render_rich(frame, app, &theme, rows[0], badge, keyboard, trends);
    } else {
        render_text(frame, app, &theme, rows[0]);
    }
    render_hint(frame, app, &theme, rows[1]);
}

fn render_v3_text(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let lang = app.config.general.lang;
    let label_width = 26;
    // Right-aligned by display width: `{:>n}` counts a full-width character
    // as one column, which shifted the Japanese headings off their values.
    let row = |label: &str, cells: [&str; 5]| {
        let mut line = pad_display(label, label_width);
        for (cell, width) in cells.into_iter().zip(V3_COLUMN_WIDTHS) {
            line.push(' ');
            line.push_str(&pad_left_display(cell, width));
        }
        line
    };
    let mut lines = vec![Line::from(match lang {
        Lang::En => row("Part", ["Eff.WPM", "Tempo", "Acc.%", "±run", "Previous"]),
        Lang::Ja => row("パート", ["主 WPM", "テンポ", "正確さ", "誤差", "前回"]),
    })];
    for record in &app.bench_summary_current {
        let Some((part, _)) = record.header.bench_id.as_deref().and_then(crate::bench::parse_v3_bench_id) else {
            continue;
        };
        let metrics = compute_bench_v3_metrics(std::slice::from_ref(record), app.config.analysis.pause_ms);
        let previous = previous_effective_wpm_delta(&app.bench_summary_records, record, app.config.analysis.pause_ms);
        let reliability = compute_bench_v3_part_reliability(&app.bench_summary_records, part, app.config.analysis.pause_ms);
        let previous = previous.map(|value| crate::ui::result::format_v3_change(value, reliability.map(|estimate| estimate.mdc95_wpm), lang));
        lines.push(Line::from(row(
            v3_part_label(part, lang),
            [
                &number(metrics.effective_wpm),
                &number(metrics.tempo_wpm),
                &percent(metrics.accuracy),
                &signed_margin(metrics.error_margin_wpm),
                previous.as_deref().unwrap_or("—"),
            ],
        )));
    }
    lines.push(Line::from(""));
    for group in [BenchV3Group::English, BenchV3Group::Japanese, BenchV3Group::Transition] {
        if let Some(summary) = compute_bench_v3_group_metrics(&app.bench_summary_current, group, app.config.analysis.pause_ms) {
            let reliability = compute_bench_v3_group_reliability(&app.bench_summary_records, group, app.config.analysis.pause_ms);
            let previous = group_previous_delta(
                &app.bench_summary_records,
                &app.bench_summary_current,
                group,
                app.config.analysis.pause_ms,
            );
            lines.push(Line::from(group_line(
                &summary,
                previous,
                reliability.map(|estimate| estimate.mdc95_wpm),
                lang,
            )));
        }
    }
    lines.push(Line::from(""));
    for group in [BenchV3Group::English, BenchV3Group::Japanese] {
        lines.push(Line::from(yardstick_error_line(
            group,
            compute_bench_v3_group_reliability(&app.bench_summary_records, group, app.config.analysis.pause_ms),
            lang,
        )));
    }
    lines.push(Line::from(Span::styled(
        match lang {
            Lang::En => "Grades will appear in the next update",
            Lang::Ja => "段階は次の更新で表示",
        },
        Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
    )));
    frame.render_widget(Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: true }), left_inset(area));
}

fn group_line(summary: &crate::analysis::bench_v3::BenchV3GroupMetrics, previous: Option<f64>, mdc95_wpm: Option<f64>, lang: Lang) -> String {
    let label = match (summary.group, lang) {
        (BenchV3Group::English, Lang::En) => "English total",
        (BenchV3Group::Japanese, Lang::En) => "Japanese total",
        (BenchV3Group::Transition, Lang::En) => "Transitions",
        (BenchV3Group::English, Lang::Ja) => "英語",
        (BenchV3Group::Japanese, Lang::Ja) => "日本語",
        (BenchV3Group::Transition, Lang::Ja) => "移り変わり",
    };
    let metrics = &summary.metrics;
    let partial = if summary.partial {
        match lang {
            Lang::En => " (partial parts)",
            Lang::Ja => "（一部のパートのみ）",
        }
    } else {
        ""
    };
    let previous = previous.map(|value| crate::ui::result::format_v3_change(value, mdc95_wpm, lang));
    let previous = previous.as_deref().unwrap_or_else(|| Msg::InitialResult.text(lang));
    match lang {
        Lang::En => format!(
            "{label} {} (tempo {} · accuracy {}) {} · previous {previous}{partial}",
            metric_wpm(metrics.effective_wpm),
            number(metrics.tempo_wpm),
            percent(metrics.accuracy),
            signed_margin_with_unit(metrics.error_margin_wpm),
        ),
        Lang::Ja => format!(
            "{label} {}（テンポ {}・正確さ {}） {}・前回 {previous}{partial}",
            metric_wpm(metrics.effective_wpm),
            number(metrics.tempo_wpm),
            percent(metrics.accuracy),
            signed_margin_with_unit(metrics.error_margin_wpm),
        ),
    }
}

fn group_previous_delta(records: &[SessionRecord], current: &[SessionRecord], group: BenchV3Group, pause_ms: u64) -> Option<f64> {
    let current_record = current.iter().find(|record| {
        record
            .header
            .bench_id
            .as_deref()
            .and_then(crate::bench::parse_v3_bench_id)
            .is_some_and(|(part, _)| group.parts().contains(&part))
    })?;
    let (_, form) = current_record.header.bench_id.as_deref().and_then(crate::bench::parse_v3_bench_id)?;
    let date = chrono::DateTime::parse_from_rfc3339(&current_record.header.finished_at)
        .ok()?
        .date_naive();
    let runs = compute_bench_v3_group_runs(records, group, pause_ms);
    let index = runs.iter().position(|run| run.date == date && run.form == form)?;
    let current = runs.get(index)?.metrics.effective_wpm?;
    let previous = runs.get(index.checked_sub(1)?)?.metrics.effective_wpm?;
    Some(current - previous)
}

fn yardstick_error_line(group: BenchV3Group, reliability: Option<crate::analysis::bench_v3::BenchV3Reliability>, lang: Lang) -> String {
    let label = match (group, lang) {
        (BenchV3Group::English, Lang::En) => "English",
        (BenchV3Group::Japanese, Lang::En) => "Japanese",
        (BenchV3Group::English, Lang::Ja) => "英語",
        (BenchV3Group::Japanese, Lang::Ja) => "日本語",
        (BenchV3Group::Transition, _) => unreachable!("the summary shows language reliability only"),
    };
    match (reliability, lang) {
        (Some(value), Lang::En) => format!(
            "{label} measurement error: SEM {:.1} WPM · meaningful change {:.1} WPM{}",
            value.sem_wpm,
            value.mdc95_wpm,
            if value.reference_only {
                format!(" (reference: {} runs)", value.sample_count)
            } else {
                String::new()
            }
        ),
        (Some(value), Lang::Ja) => format!(
            "{label}のこの物差しの誤差: SEM {:.1} WPM・意味のある変化 {:.1} WPM{}",
            value.sem_wpm,
            value.mdc95_wpm,
            if value.reference_only {
                format!("（参考: {} 回分）", value.sample_count)
            } else {
                String::new()
            }
        ),
        (None, Lang::En) => format!("{label} measurement error: available after 2 runs"),
        (None, Lang::Ja) => format!("{label}のこの物差しの誤差: 2 回分で表示"),
    }
}

fn pad_display(value: &str, width: usize) -> String {
    format!("{value}{}", " ".repeat(width.saturating_sub(UnicodeWidthStr::width(value))))
}

/// Display widths of the v3 summary columns after the part name.
const V3_COLUMN_WIDTHS: [usize; 5] = [8, 7, 7, 6, 8];

fn pad_left_display(value: &str, width: usize) -> String {
    format!("{}{value}", " ".repeat(width.saturating_sub(UnicodeWidthStr::width(value))))
}

fn number(value: Option<f64>) -> String {
    value.map_or_else(|| "—".to_owned(), |value| format!("{value:.1}"))
}

fn metric_wpm(value: Option<f64>) -> String {
    value.map_or_else(|| "—".to_owned(), |value| format!("{value:.1} WPM"))
}

fn percent(value: f64) -> String {
    format!("{:.1}%", value * 100.0)
}

fn signed_margin(value: Option<f64>) -> String {
    value.map_or_else(|| "—".to_owned(), |value| format!("±{value:.1}"))
}

fn signed_margin_with_unit(value: Option<f64>) -> String {
    value.map_or_else(|| "—".to_owned(), |value| format!("±{value:.1} WPM"))
}

fn render_rich(frame: &mut Frame, app: &App, theme: &Theme, area: Rect, badge: bool, keyboard: bool, trends: bool) {
    let trend_height = if trends { if area.height >= 44 { 16 } else { 14 } } else { 0 };
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(10), Constraint::Length(trend_height), Constraint::Min(1)])
        .split(area);
    render_top(frame, app, theme, rows[0], badge);
    if trends {
        render_trends(frame, app, theme, rows[1]);
    }
    render_analysis(frame, app, theme, rows[2], keyboard);
}

fn render_top(frame: &mut Frame, app: &App, theme: &Theme, area: Rect, badge: bool) {
    let grade_width = if badge { (area.width / 4).clamp(27, 38) } else { 22.min(area.width / 3) };
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(grade_width), Constraint::Min(1)])
        .split(area);
    render_grade(frame, app, theme, columns[0], badge);
    if app.bench_results.is_empty() {
        return;
    }
    let card_areas = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            app.bench_results
                .iter()
                .map(|_| Constraint::Ratio(1, app.bench_results.len() as u32))
                .collect::<Vec<_>>(),
        )
        .split(columns[1]);
    let colors = image_colors(theme);
    let font = app.bigtext_images.borrow_mut().resolved_font(&app.config.display.big_font, &app.home_dir);
    let specs = app
        .bench_results
        .iter()
        .zip(card_areas.iter().copied())
        .map(|(result, area)| {
            let area = Rect::new(area.x, area.y, area.width, area.height.min(9));
            let spec = BenchPartCardSpec {
                label: part_label(result.part, app.config.general.lang).into(),
                wpm: result.wpm,
                accuracy: result.accuracy,
                previous_wpm_delta: result.comparison.previous_wpm_delta,
                best_wpm_delta: result.comparison.best_wpm_delta,
                previous_accuracy_delta: result.comparison.previous_accuracy_delta,
                best_accuracy_delta: result.comparison.best_accuracy_delta,
                version: short_version(&result.id),
                lang: app.config.general.lang,
                cells: area.as_size(),
                colors,
            };
            (spec, area)
        })
        .collect::<Vec<_>>();
    if specs.iter().any(|(spec, _)| {
        app.result_images
            .borrow_mut()
            .prepare_bench_part_card(&app.graphics, &font, spec)
            .is_err()
    }) {
        render_part_text(frame, app, columns[1]);
        return;
    }
    for (spec, area) in &specs {
        if let Some(protocol) = app.result_images.borrow_mut().bench_part_card_protocol(&app.graphics, &font, spec) {
            frame.render_widget(Image::new(protocol), *area);
        }
    }
}

fn render_grade(frame: &mut Frame, app: &App, theme: &Theme, area: Rect, image: bool) {
    let lang = app.config.general.lang;
    let selected = app
        .bench_summary_grades
        .languages
        .iter()
        .filter_map(|language| Some((language, language.overall.as_ref()?)))
        .min_by(|left, right| left.1.score.total_cmp(&right.1.score));
    let lines = language_grade_lines(app, theme);
    if image
        && area.width >= 12
        && area.height >= 6
        && let Some((_, overall)) = selected
    {
        let color = grade_color(overall.grade, theme);
        let badge_area = Rect::new(area.x, area.y, 10.min(area.width), 6.min(area.height));
        let spec = GradeBadgeSpec {
            level: overall.grade,
            color,
            cells: badge_area.as_size(),
        };
        let font = app.bigtext_images.borrow_mut().resolved_font(&app.config.display.big_font, &app.home_dir);
        if app.analysis_images.borrow_mut().prepare_badge(&app.graphics, &font, &spec).is_ok()
            && let Some(protocol) = app.analysis_images.borrow_mut().badge_protocol(&app.graphics, &font, &spec)
        {
            frame.render_widget(Image::new(protocol), badge_area);
        }
        let component_area = Rect::new(
            badge_area.x + badge_area.width,
            area.y,
            area.width.saturating_sub(badge_area.width),
            area.height,
        );
        frame.render_widget(Paragraph::new(lines), component_area);
    } else {
        let mut text = selected.map_or_else(Vec::new, |(_, overall)| {
            vec![Line::from(Span::styled(
                format!("{}: {}", grade_heading(lang), overall.grade.as_str()),
                Style::default().fg(grade_color(overall.grade, theme)).add_modifier(Modifier::BOLD),
            ))]
        });
        text.extend(lines);
        frame.render_widget(Paragraph::new(text).wrap(ratatui::widgets::Wrap { trim: true }), area);
    }
}

fn render_trends(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let lang = app.config.general.lang;
    let block = Block::default().borders(Borders::TOP).title(match lang {
        Lang::En => " Past trends: WPM / accuracy ",
        Lang::Ja => " 過去の傾向: WPM／正確さ ",
    });
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if app.bench_results.is_empty() {
        return;
    }
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            app.bench_results
                .iter()
                .map(|_| Constraint::Ratio(1, app.bench_results.len() as u32))
                .collect::<Vec<_>>(),
        )
        .split(inner);
    let colors = image_colors(theme);
    let font = app.bigtext_images.borrow_mut().resolved_font(&app.config.display.big_font, &app.home_dir);
    for (result, column) in app.bench_results.iter().zip(columns.iter().copied()) {
        let title_area = Rect::new(column.x, column.y, column.width, 1.min(column.height));
        frame.render_widget(
            Paragraph::new(format!("{}{}", result.part.as_str(), initial_suffix(app, result.part, lang)))
                .style(Style::default().fg(theme.muted).add_modifier(Modifier::BOLD))
                .alignment(Alignment::Center),
            title_area,
        );
        let chart_area = Rect::new(column.x, column.y.saturating_add(1), column.width, column.height.saturating_sub(1));
        let spec = BenchTrendSpec {
            points: trend_points(app, result.part),
            cells: chart_area.as_size(),
            colors,
        };
        if spec.points.is_empty() || app.result_images.borrow_mut().prepare_bench_trend(&app.graphics, &font, &spec).is_err() {
            continue;
        }
        if let Some(protocol) = app.result_images.borrow_mut().bench_trend_protocol(&app.graphics, &font, &spec) {
            frame.render_widget(Image::new(protocol), chart_area);
        }
    }
}

fn render_analysis(frame: &mut Frame, app: &App, theme: &Theme, area: Rect, keyboard: bool) {
    if keyboard && area.width >= 90 && area.height >= 10 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(3, 5), Constraint::Ratio(2, 5)])
            .split(area);
        render_mistype_keyboard(frame, app, theme, columns[0]);
        render_analysis_text(frame, app, theme, columns[1]);
    } else {
        render_analysis_text(frame, app, theme, area);
    }
}

fn render_mistype_keyboard(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let lang = app.config.general.lang;
    let maximum = app.bench_summary_analysis.mistyped_keys.values().copied().max().unwrap_or(0);
    let title = match lang {
        Lang::En => format!(" Missed keys (expected)  0 ░▒▓ {maximum} "),
        Lang::Ja => format!(" 誤打したキー（打つべきキー）  0 ░▒▓ {maximum} "),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .style(Style::default().fg(theme.muted));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if maximum == 0 {
        frame.render_widget(
            Paragraph::new(match lang {
                Lang::En => "No missed keys",
                Lang::Ja => "誤打はありません",
            })
            .alignment(Alignment::Center),
            inner,
        );
        return;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(10.min(inner.height.saturating_sub(1))), Constraint::Min(1)])
        .split(inner);
    let Some(spec) = mistype_keyboard_spec(app, theme, rows[0].as_size()) else {
        return;
    };
    if app.result_images.borrow_mut().prepare_mistype_keyboard(&app.graphics, &spec).is_err() {
        return;
    }
    if let Some(protocol) = app.result_images.borrow_mut().mistype_keyboard_protocol(&app.graphics, &spec) {
        frame.render_widget(Image::new(protocol), rows[0]);
    }
    let hidden = mistype_key_count(app).saturating_sub(3);
    if hidden > 0 {
        frame.render_widget(
            Paragraph::new(match lang {
                Lang::En => format!("and {hidden} more"),
                Lang::Ja => format!("ほか {hidden} 件"),
            })
            .style(Style::default().fg(theme.fg))
            .alignment(Alignment::Center),
            rows[1],
        );
    }
}

fn render_analysis_text(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let lang = app.config.general.lang;
    let title = match lang {
        Lang::En => " This run's analysis ",
        Lang::Ja => " 今回の分析 ",
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(title, Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)))
        .style(Style::default().fg(theme.muted));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(analysis_lines(&app.bench_summary_analysis, lang, theme)).wrap(ratatui::widgets::Wrap { trim: true }),
        inner,
    );
}

fn render_text(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let lang = app.config.general.lang;
    let mut lines = grade_text_lines(app, theme);
    lines.extend(part_text_lines(app));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        match lang {
            Lang::En => "This run's analysis",
            Lang::Ja => "今回の分析",
        },
        Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
    )));
    lines.extend(analysis_lines(&app.bench_summary_analysis, lang, theme));
    frame.render_widget(Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: true }), left_inset(area));
}

fn render_part_text(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(Paragraph::new(part_text_lines(app)), area);
}

fn part_text_lines(app: &App) -> Vec<Line<'static>> {
    let lang = app.config.general.lang;
    app.bench_results
        .iter()
        .map(|result| {
            let previous = delta_pair(
                lang,
                result.comparison.previous_wpm_delta,
                result.comparison.previous_accuracy_delta,
                MetricDeltaKind::Previous,
            );
            let best = delta_pair(
                lang,
                result.comparison.best_wpm_delta,
                result.comparison.best_accuracy_delta,
                MetricDeltaKind::Best,
            );
            Line::from(format!(
                "{}  {:.1} WPM  {:.1}%  {previous}  {best}  {}",
                part_label(result.part, lang),
                result.wpm,
                result.accuracy * 100.0,
                short_version(&result.id)
            ))
        })
        .collect()
}

fn analysis_lines(analysis: &BenchmarkRunAnalysis, lang: Lang, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if !analysis.slow_transitions.is_empty() {
        lines.push(section_line(
            match lang {
                Lang::En => "Slow transitions",
                Lang::Ja => "遅いキーのつなぎ",
            },
            theme,
        ));
        lines.extend(
            analysis
                .slow_transitions
                .iter()
                .take(5)
                .map(|row| body_line(format!("• {}→{} {:.0} ms (n={})", row.from, row.to, row.milliseconds, row.n), theme)),
        );
    } else if analysis.transition_counts_insufficient {
        lines.push(body_line(
            match lang {
                Lang::En => "Transitions: not enough observations",
                Lang::Ja => "つなぎ：回数が足りません",
            },
            theme,
        ));
    }
    if !analysis.weak_fingers.is_empty() {
        lines.push(section_line(
            match lang {
                Lang::En => "Weak fingers",
                Lang::Ja => "弱い指",
            },
            theme,
        ));
        lines.extend(analysis.weak_fingers.iter().take(2).map(|finger| {
            let name = finger_label(&finger.name, lang);
            let value = match (finger.reason, lang) {
                (WeakFingerReason::ErrorRate, Lang::En) => format!("error rate {:.1}%", finger.error_rate * 100.0),
                (WeakFingerReason::ErrorRate, Lang::Ja) => format!("誤打率 {:.1}%", finger.error_rate * 100.0),
                (WeakFingerReason::Iki, Lang::En) => format!("interval {:.0} ms", finger.iki_median_ms),
                (WeakFingerReason::Iki, Lang::Ja) => format!("打鍵間隔 {:.0} ms", finger.iki_median_ms),
            };
            body_line(format!("• {name} {value}"), theme)
        }));
    }
    if !analysis.changes.is_empty() {
        lines.push(section_line(
            match lang {
                Lang::En => "Changes since previous",
                Lang::Ja => "前回からの変化",
            },
            theme,
        ));
        let mut better = analysis.changes.iter().filter(|change| change.delta > 0.0).collect::<Vec<_>>();
        let mut worse = analysis.changes.iter().filter(|change| change.delta < 0.0).collect::<Vec<_>>();
        better.sort_by(|left, right| right.delta.total_cmp(&left.delta));
        worse.sort_by(|left, right| left.delta.total_cmp(&right.delta));
        lines.extend(
            better
                .into_iter()
                .take(2)
                .chain(worse.into_iter().take(2))
                .map(|change| change_line(change, lang, theme)),
        );
    }
    if !analysis.practice.is_empty() {
        lines.push(section_line(
            match lang {
                Lang::En => "Practice next",
                Lang::Ja => "次に練習すること",
            },
            theme,
        ));
        lines.extend(
            analysis
                .practice
                .iter()
                .take(3)
                .map(|suggestion| body_line(format!("• {}", practice_line(suggestion, lang)), theme)),
        );
    }
    if lines.is_empty() {
        lines.push(body_line(
            match lang {
                Lang::En => "Not enough keystrokes for analysis",
                Lang::Ja => "分析に足る打鍵がありません",
            },
            theme,
        ));
    }
    lines
}

fn grade_text_lines(app: &App, theme: &Theme) -> Vec<Line<'static>> {
    let lang = app.config.general.lang;
    let selected = app
        .bench_summary_grades
        .languages
        .iter()
        .filter_map(|language| Some((language, language.overall.as_ref()?)))
        .min_by(|left, right| left.1.score.total_cmp(&right.1.score));
    let mut lines = selected.map_or_else(Vec::new, |(_, overall)| {
        vec![Line::from(Span::styled(
            format!("{}: {}", grade_heading(lang), overall.grade.as_str()),
            Style::default().fg(grade_color(overall.grade, theme)).add_modifier(Modifier::BOLD),
        ))]
    });
    lines.extend(language_grade_lines(app, theme));
    lines
}

fn language_grade_lines(app: &App, theme: &Theme) -> Vec<Line<'static>> {
    let lang = app.config.general.lang;
    let mut lines = Vec::new();
    for (code, part) in [("en", "A"), ("ja", "C")] {
        let label = match (code, lang) {
            ("ja", Lang::En) => "Japanese",
            ("ja", Lang::Ja) => "日本語",
            (_, Lang::En) => "English",
            (_, Lang::Ja) => "英語",
        };
        match app.bench_summary_grades.languages.iter().find(|language| language.language == code) {
            Some(language) if language.overall.is_some() => {
                let overall = language.overall.as_ref().expect("checked overall");
                lines.push(Line::from(Span::styled(
                    format!("{label} {}", overall.grade.as_str()),
                    Style::default().fg(grade_color(overall.grade, theme)).add_modifier(Modifier::BOLD),
                )));
                for components in language.components.chunks(2) {
                    let mut spans = Vec::new();
                    for component in components {
                        spans.push(Span::styled(
                            format!("  {} {}", component_label(component.component, lang), component.grade.as_str()),
                            Style::default().fg(theme.fg),
                        ));
                    }
                    lines.push(Line::from(spans));
                }
            }
            Some(_) => lines.push(body_line(
                match lang {
                    Lang::En => format!("{label}: required components could not be measured"),
                    Lang::Ja => format!("{label}：必要な構成を求められません"),
                },
                theme,
            )),
            None => lines.push(body_line(
                match lang {
                    Lang::En => format!("{label}: not enough benchmark part {part} records"),
                    Lang::Ja => format!("{label}: ベンチ {part} の記録が足りません"),
                },
                theme,
            )),
        }
    }
    lines
}

fn trend_points(app: &App, part: BenchPart) -> Vec<BenchTrendPoint> {
    let mut rows = app
        .bench_summary_records
        .iter()
        .filter_map(|record| {
            let (series, candidate, _) = record.header.bench_id.as_deref().and_then(parse_bench_id)?;
            (candidate == part).then_some((record, series))
        })
        .collect::<Vec<_>>();
    let start = rows.len().saturating_sub(20);
    rows.drain(..start);
    rows.iter()
        .enumerate()
        .map(|(index, (record, series))| {
            let starts_new_series =
                matches!(part, BenchPart::A | BenchPart::C) && index > 0 && rows[index - 1].1 != *series && *series == crate::bench::BENCH_SERIES_V2;
            BenchTrendPoint {
                date: chrono::DateTime::parse_from_rfc3339(&record.header.finished_at)
                    .map_or(chrono::NaiveDate::MIN, |finished| finished.with_timezone(&chrono::Local).date_naive()),
                wpm: record.header.net_wpm,
                accuracy: record.header.accuracy,
                starts_new_series,
                current: app.bench_summary_current.iter().any(|current| same_record(current, record)),
            }
        })
        .collect()
}

fn same_record(left: &SessionRecord, right: &SessionRecord) -> bool {
    match (left.header.id, right.header.id) {
        (Some(left), Some(right)) => left == right,
        _ => left.header.started_at == right.header.started_at && left.header.bench_id == right.header.bench_id,
    }
}

fn mistype_key_count(app: &App) -> usize {
    let layout = app.practice_layout();
    app.bench_summary_analysis
        .mistyped_keys
        .keys()
        .filter_map(|expected| {
            let mut characters = expected.chars();
            let character = characters.next().filter(|_| characters.next().is_none())?;
            layout.stroke_for(character).map(|stroke| stroke.code)
        })
        .collect::<std::collections::HashSet<_>>()
        .len()
}

fn mistype_keyboard_spec(app: &App, theme: &Theme, cells: Size) -> Option<MistypeKeyboardSpec> {
    let layout = app.practice_layout();
    let mut counts = HashMap::<String, u32>::new();
    for (expected, count) in &app.bench_summary_analysis.mistyped_keys {
        let mut characters = expected.chars();
        let Some(character) = characters.next().filter(|_| characters.next().is_none()) else {
            continue;
        };
        if let Some(stroke) = layout.stroke_for(character) {
            *counts.entry(stroke.code).or_default() += *count;
        }
    }
    let maximum = counts.values().copied().max().unwrap_or(0);
    let geometry = crate::layout::geometry::geometry(&app.config.keyboard.geometry).or_else(|| crate::layout::geometry::geometry("ansi"))?;
    let state = KeyboardState {
        heatmap: counts
            .iter()
            .map(|(code, count)| (code.clone(), f64::from(*count) / f64::from(maximum.max(1))))
            .collect(),
        ..KeyboardState::default()
    };
    let mut placements = keycap_tile_placements(
        &geometry,
        layout,
        &state,
        KeyboardView {
            show: "full",
            style: "keycap",
            finger_guide: false,
        },
        true,
    );
    let board_width = KeyboardStyle::Keycap.width();
    let board_height = 10_u16;
    let offset_x = cells.width.saturating_sub(board_width) / 2;
    let offset_y = cells.height.saturating_sub(board_height) / 2;
    let mut ranked = counts.iter().collect::<Vec<_>>();
    ranked.sort_by(|(left_code, left), (right_code, right)| right.cmp(left).then(left_code.cmp(right_code)));
    let top = ranked
        .into_iter()
        .take(3)
        .map(|(code, count)| (code.as_str(), *count))
        .collect::<HashMap<_, _>>();
    let mistype = crate::ui::image_cache::color_rgb(theme.mistype, [241, 76, 76]);
    for (placement, key) in placements.iter_mut().zip(geometry.rows.iter().flatten()) {
        placement.x = placement.x.saturating_add(offset_x);
        placement.y = placement.y.saturating_add(offset_y);
        if let Some(count) = counts.get(key.code) {
            let ratio = f64::from(*count) / f64::from(maximum.max(1));
            placement.spec.face_color = Color::Rgb(
                (30.0 + f64::from(mistype[0]) * ratio * 0.8) as u8,
                (12.0 + f64::from(mistype[1]) * ratio * 0.35) as u8,
                (12.0 + f64::from(mistype[2]) * ratio * 0.35) as u8,
            );
            if let Some(count) = top.get(key.code) {
                placement.spec.label = format!("{} {count}", placement.spec.label);
            }
        }
    }
    Some(MistypeKeyboardSpec { placements, cells })
}

fn render_hint(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let remaining = app.result_input_delay_remaining();
    let hint = remaining.map_or_else(
        || Msg::BenchSummaryHint.text(app.config.general.lang).to_owned(),
        |remaining| {
            format!(
                "{}  … {:.1}s",
                Msg::BenchSummaryHint.text(app.config.general.lang),
                remaining.as_secs_f32()
            )
        },
    );
    let style = if remaining.is_some() {
        Style::default().fg(theme.muted)
    } else {
        Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
    };
    frame.render_widget(Paragraph::new(hint).style(style), left_inset(area));
}

fn delta_pair(lang: Lang, wpm: Option<f64>, accuracy: Option<f64>, kind: MetricDeltaKind) -> String {
    match (wpm, accuracy) {
        (Some(wpm), Some(accuracy)) => format!(
            "{} / {}",
            format_metric_delta(lang, kind, wpm, " WPM"),
            format_metric_delta(lang, kind, accuracy * 100.0, "pt")
        ),
        _ => {
            let label = match (lang, kind) {
                (_, MetricDeltaKind::Previous) => Msg::PreviousDelta.text(lang),
                (Lang::En, MetricDeltaKind::Best) => "best",
                (Lang::Ja, MetricDeltaKind::Best) => "自己ベスト",
            };
            format!("{label} {}", Msg::InitialResult.text(lang))
        }
    }
}

fn change_line(change: &BenchChange, lang: Lang, theme: &Theme) -> Line<'static> {
    let metric = match (change.metric, lang) {
        (BenchMetric::Wpm, _) => "WPM",
        (BenchMetric::Accuracy, Lang::En) => "accuracy",
        (BenchMetric::Accuracy, Lang::Ja) => "正確さ",
    };
    let suffix = if change.metric == BenchMetric::Accuracy { "pt" } else { "" };
    body_line(format!("• {} {metric} {:+.1}{suffix}", change.part.as_str(), change.delta), theme)
}

fn practice_line(suggestion: &PracticeSuggestion, lang: Lang) -> String {
    match (suggestion, lang) {
        (PracticeSuggestion::Transition { from, to }, Lang::En) => format!("Focused transition practice: {from}→{to}"),
        (PracticeSuggestion::Transition { from, to }, Lang::Ja) => format!("つなぎの絞り込み練習で {from}→{to} を"),
        (PracticeSuggestion::Finger { name }, Lang::En) => format!("Finger-focused practice: {}", finger_label(name, lang)),
        (PracticeSuggestion::Finger { name }, Lang::Ja) => format!("{}を意識した練習", finger_label(name, lang)),
        (PracticeSuggestion::Keys { keys }, Lang::En) => format!("Focused-key practice: {}", keys.join(" · ")),
        (PracticeSuggestion::Keys { keys }, Lang::Ja) => format!("キーの絞り込み練習で {} を", keys.join("・")),
    }
}

fn body_line(text: impl Into<String>, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(text.into(), Style::default().fg(theme.fg)))
}

fn section_line(label: &'static str, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(label, Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)))
}

fn image_colors(theme: &Theme) -> ResultImageColors {
    ResultImageColors {
        foreground: theme.fg,
        muted: theme.muted,
        correct: theme.correct,
        mistype: theme.mistype,
        accent: theme.accent,
        warning: theme.warning,
        panel: theme.bg_panel,
    }
}

fn grade_color(level: Level, theme: &Theme) -> Color {
    match level {
        Level::S | Level::A => theme.correct,
        Level::B | Level::C => theme.accent,
        Level::D | Level::E => theme.warning,
        Level::F | Level::G => theme.mistype,
    }
}

fn component_label(component: Component, lang: Lang) -> &'static str {
    match (component, lang) {
        (Component::Speed, Lang::En) => "Speed",
        (Component::Speed, Lang::Ja) => "速さ",
        (Component::Accuracy, Lang::En) => "Accuracy",
        (Component::Accuracy, Lang::Ja) => "正確さ",
        (Component::Rhythm, Lang::En) => "Rhythm",
        (Component::Rhythm, Lang::Ja) => "リズム",
        (Component::Transition, Lang::En) => "Transitions",
        (Component::Transition, Lang::Ja) => "つなぎ",
        (Component::Preview, Lang::En) => "Preview",
        (Component::Preview, Lang::Ja) => "先読み",
        (Component::Recovery, Lang::En) => "Recovery",
        (Component::Recovery, Lang::Ja) => "立ち直り",
        (Component::Overlap, Lang::En) => "Overlap",
        (Component::Overlap, Lang::Ja) => "重なり",
    }
}

fn finger_label(name: &str, lang: Lang) -> String {
    if lang == Lang::En {
        return name.replace('-', " ");
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
        "thumb" | "left-thumb" | "right-thumb" => "親指",
        other => other,
    }
    .into()
}

fn initial_suffix(app: &App, part: BenchPart, lang: Lang) -> &'static str {
    if trend_points(app, part).len() == 1 {
        match lang {
            Lang::En => "  Initial",
            Lang::Ja => "  初回",
        }
    } else {
        ""
    }
}

fn grade_heading(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Grade",
        Lang::Ja => "段階",
    }
}

fn short_version(id: &str) -> String {
    id.strip_prefix("bench-").unwrap_or(id).to_owned()
}

pub(crate) fn part_label(part: BenchPart, lang: Lang) -> &'static str {
    match part {
        BenchPart::A => Msg::BenchPartA.text(lang),
        BenchPart::B => Msg::BenchPartB.text(lang),
        BenchPart::C => Msg::BenchPartC.text(lang),
        BenchPart::D => Msg::BenchPartD.text(lang),
    }
}

pub(crate) fn v3_part_label(part: crate::bench::BenchV3Part, lang: Lang) -> &'static str {
    match (part, lang) {
        (crate::bench::BenchV3Part::W, Lang::En) => "Warm-up",
        (crate::bench::BenchV3Part::W, Lang::Ja) => "準備運動",
        (crate::bench::BenchV3Part::EW, Lang::En) => "English words",
        (crate::bench::BenchV3Part::EW, Lang::Ja) => "英単語",
        (crate::bench::BenchV3Part::ES, Lang::En) => "English sentences",
        (crate::bench::BenchV3Part::ES, Lang::Ja) => "英文",
        (crate::bench::BenchV3Part::JW, Lang::En) => "Japanese words and phrases",
        (crate::bench::BenchV3Part::JW, Lang::Ja) => "日本語の単語と句",
        (crate::bench::BenchV3Part::JS, Lang::En) => "Japanese sentences",
        (crate::bench::BenchV3Part::JS, Lang::Ja) => "日本語の短文",
        (crate::bench::BenchV3Part::PW, Lang::En) => "Pseudo-words",
        (crate::bench::BenchV3Part::PW, Lang::Ja) => "擬似語",
    }
}
