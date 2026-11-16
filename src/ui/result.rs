//! Result screen — big WPM/CPS/Accuracy summary, mistype heatmap, reaction histogram.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, ResultOrigin};
use crate::bench::BenchPart;
use crate::i18n::{Lang, Msg};
use crate::metrics::stats::GameResult;
use crate::ui::{left_inset, theme::Theme};

pub fn render(frame: &mut Frame, app: &App) {
    let theme = Theme::from_name(&app.config.ui.theme);
    let lang = app.config.general.lang;
    let area = frame.area();

    if app.result_origin == ResultOrigin::JustPlayed && app.active_bench_id.is_some() {
        render_benchmark_result(frame, app, &theme, area, lang);
        return;
    }

    let title = match app.result_origin {
        ResultOrigin::JustPlayed => Msg::GameResultTitle,
        ResultOrigin::History => Msg::HistoryDetailTitle,
    };
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", title.text(lang)))
        .style(Style::default().fg(theme.fg));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let content = if app.result_origin == ResultOrigin::History {
        let condition_height = if app.history_detail.as_ref().and_then(|detail| detail.export_notice.as_ref()).is_some() {
            9
        } else {
            8
        };
        let areas = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(condition_height), Constraint::Min(0)])
            .split(inner);
        render_history_conditions(frame, app, &theme, areas[0], lang);
        areas[1]
    } else {
        inner
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(9), // big WPM / CPS / Accuracy summary
            Constraint::Length(1),
            Constraint::Length(6), // detailed secondary stats
            Constraint::Length(1),
            Constraint::Length(11), // heatmap
            Constraint::Length(1),
            Constraint::Min(0),    // reaction histogram + save info
            Constraint::Length(2), // hint
        ])
        .split(content);

    let Some(result) = app.last_result.as_ref() else {
        let none = Paragraph::new(Msg::NoResultYet.text(lang));
        frame.render_widget(none, left_inset(chunks[0]));
        return;
    };

    render_big_summary(frame, result, app.legacy_wpm, &theme, chunks[0], lang);
    render_secondary_box(frame, app, result, &theme, chunks[2], lang);
    if app.result_origin == ResultOrigin::JustPlayed && !app.result_insights.is_empty() && area.height < 34 {
        render_result_insights(frame, app, &theme, left_inset(chunks[4]));
    } else if app.config.mode.kind == crate::config::ModeKind::Adaptive && app.result_origin == ResultOrigin::JustPlayed {
        render_adaptive_result(frame, app, &theme, left_inset(chunks[4]), lang);
    } else {
        render_mistype_heatmap(frame, result, &theme, left_inset(chunks[4]), lang);
    }
    render_reaction_and_save(frame, app, result, &theme, left_inset(chunks[6]));
    match app.result_origin {
        ResultOrigin::JustPlayed => render_hint(frame, app, &theme, chunks[7], lang),
        ResultOrigin::History => render_hint_history(frame, app, &theme, chunks[7], lang),
    }
}

fn render_history_conditions(frame: &mut Frame, app: &App, theme: &Theme, area: Rect, lang: Lang) {
    let Some(detail) = app.history_detail.as_ref() else {
        return;
    };
    let summary = &detail.summary;
    let header = &detail.record.header;
    let params = serde_json::from_str::<serde_json::Value>(&header.mode_params).unwrap_or_default();
    let (mode, language, practice_layout, input_layout, error_mode, aids, key_filter, bench_id, words, combo, best, previous) = match lang {
        Lang::En => (
            "Mode",
            "Language",
            "Practice layout",
            "Input layout",
            "Error mode",
            "Assists",
            "Key filter",
            "Bench ID",
            "Words",
            "Max combo",
            "Personal best",
            "Previous",
        ),
        Lang::Ja => (
            "モード",
            "言語",
            "練習の配列",
            "入力の配列",
            "誤打の扱い",
            "補助",
            "キーの絞り込み",
            "ベンチ ID",
            "語数",
            "最大コンボ",
            "自己ベスト",
            "前回差",
        ),
    };
    let mut lines = vec![Line::from(format!(
        "{mode}: {}    {language}: {}",
        crate::ui::history::mode_label(&header.mode, lang),
        crate::app::history_language(summary).unwrap_or("—")
    ))];
    let mut layout_line = format!("{practice_layout}: {}", header.layout);
    if header.input_layout != "same" && header.input_layout != header.layout {
        layout_line.push_str(&format!("    {input_layout}: {}", header.input_layout));
    }
    layout_line.push_str(&format!("    {error_mode}: {}", header.error_mode));
    lines.push(Line::from(layout_line));

    let mut assistance = Vec::new();
    if let Some(enabled) = params.get("pacer_enabled").and_then(serde_json::Value::as_bool) {
        assistance.push(format!("Pacer: {}", on_off(enabled)));
    }
    if let Some(ghost) = params.get("ghost") {
        let enabled = ghost.get("enabled").and_then(serde_json::Value::as_bool).unwrap_or(false);
        let opponent = ghost.get("opponent").and_then(serde_json::Value::as_str);
        assistance.push(opponent.map_or_else(
            || format!("Ghost: {}", on_off(enabled)),
            |opponent| format!("Ghost: {} ({opponent})", on_off(enabled)),
        ));
    }
    if let Some(enabled) = params.get("blind").and_then(serde_json::Value::as_bool) {
        assistance.push(format!("Blind: {}", on_off(enabled)));
    }
    if let Some(show) = params.get("keyboard_show").and_then(serde_json::Value::as_str) {
        assistance.push(format!("Keyboard: {show}"));
    }
    if !assistance.is_empty() {
        lines.push(Line::from(format!("{aids}: {}", assistance.join("  "))));
    }

    let mut conditions = Vec::new();
    if let Some(filter) = params.get("key_filter").and_then(serde_json::Value::as_str) {
        conditions.push(format!("{key_filter}: {filter}"));
    }
    if let Some(keys) = params.get("keys").and_then(serde_json::Value::as_str).filter(|keys| !keys.is_empty()) {
        conditions.push(format!("Keys: {keys}"));
    }
    if let Some(id) = header.bench_id.as_deref() {
        conditions.push(format!("{bench_id}: {id}"));
    }
    if !conditions.is_empty() {
        lines.push(Line::from(conditions.join("    ")));
    }
    lines.push(Line::from(format!(
        "{words}: {}    {combo}: {}",
        header.completed_words, header.max_combo
    )));
    let best_value = detail.personal_best_wpm.map_or_else(|| "—".to_owned(), |value| format!("{value:.1} WPM"));
    let previous_value = detail
        .previous_wpm_delta
        .map_or_else(|| "—".to_owned(), |value| format!("{value:+.1} WPM"));
    lines.push(Line::from(Span::styled(
        format!("{best}: {best_value}    {previous}: {previous_value}"),
        Style::default().fg(theme.warning).add_modifier(Modifier::BOLD),
    )));
    if let Some(notice) = detail.export_notice.as_ref() {
        lines.push(Line::from(Span::styled(notice.clone(), Style::default().fg(theme.correct))));
    }
    let block = Block::default().borders(Borders::ALL).title(match lang {
        Lang::En => " Conditions ",
        Lang::Ja => " 条件 ",
    });
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(lines), left_inset(inner));
}

fn on_off(enabled: bool) -> &'static str {
    if enabled { "ON" } else { "OFF" }
}

fn render_benchmark_result(frame: &mut Frame, app: &App, theme: &Theme, area: Rect, lang: Lang) {
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", Msg::BenchResultTitle.text(lang)))
        .style(Style::default().fg(theme.fg));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    let Some(result) = app.last_result.as_ref() else {
        frame.render_widget(Paragraph::new(Msg::NoResultYet.text(lang)), left_inset(inner));
        return;
    };
    let part = app.active_bench_part().or_else(|| app.active_bench_id.as_deref().and_then(part_from_id));
    let part_name = part.map_or("", |part| crate::ui::bench_summary::part_label(part, lang));
    let bench_id = app.active_bench_id.as_deref().unwrap_or_default();
    let comparison = app.bench_comparison.unwrap_or_default();
    let previous = comparison
        .previous_wpm_delta
        .map_or_else(|| Msg::InitialResult.text(lang).to_owned(), |value| format!("{value:+.1} WPM"));
    let recent = comparison
        .recent_three_wpm_delta
        .map_or_else(|| Msg::InitialResult.text(lang).to_owned(), |value| format!("{value:+.1} WPM"));
    let next = app.next_bench_part();
    let mut next_line = next.map_or_else(
        || Msg::BenchViewSummary.text(lang).to_owned(),
        |part| format!("{} {}", Msg::BenchNext.text(lang), crate::ui::bench_summary::part_label(part, lang)),
    );
    let remaining = app.result_input_delay_remaining();
    if let Some(remaining) = remaining {
        next_line.push_str(&format!("  … {:.1}s", remaining.as_secs_f32()));
    }
    let hint_color = if remaining.is_some() { theme.muted } else { theme.accent };
    let abort_note = if app.abort_confirmation_active() {
        Msg::AbortConfirmHint.text(lang)
    } else {
        Msg::BenchAbortNote.text(lang)
    };
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(part_name, Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))),
        Line::from(format!("{}: {bench_id}", Msg::BenchVersion.text(lang))),
        Line::from(""),
        Line::from(format!(
            "WPM {:>6.1}    {} {:>5.1}%",
            result.wpm,
            Msg::Accuracy.text(lang),
            result.accuracy * 100.0
        )),
        Line::from(format!("{}: {previous}", Msg::PreviousDelta.text(lang))),
        Line::from(format!("{}: {recent}", Msg::RecentThreeDelta.text(lang))),
        Line::from(Span::styled(
            app.result_grade_line.clone().unwrap_or_default(),
            Style::default().fg(theme.warning).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(next_line, Style::default().fg(hint_color).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled(
            abort_note,
            Style::default().fg(if app.abort_confirmation_active() { theme.warning } else { hint_color }),
        )),
    ];
    frame.render_widget(Paragraph::new(lines), left_inset(inner));
}

fn part_from_id(id: &str) -> Option<BenchPart> {
    match id.split('/').nth(1)? {
        "A" => Some(BenchPart::A),
        "B" => Some(BenchPart::B),
        "C" => Some(BenchPart::C),
        "D" => Some(BenchPart::D),
        _ => None,
    }
}

fn render_big_summary(frame: &mut Frame, r: &GameResult, legacy_wpm: Option<f64>, theme: &Theme, area: Rect, lang: Lang) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1, 3), Constraint::Ratio(1, 3), Constraint::Ratio(1, 3)])
        .split(area);

    render_big_metric(frame, Msg::Wpm.text(lang), &format!("{:.1}", r.wpm), theme, cols[0]);
    if let Some(old_wpm) = legacy_wpm {
        let note = Msg::LegacyWpmNote.text(lang).replace("{wpm}", &format!("{old_wpm:.1}"));
        let note_area = Rect::new(cols[0].x, cols[0].y + cols[0].height.saturating_sub(2), cols[0].width, 2);
        frame.render_widget(
            Paragraph::new(note).alignment(Alignment::Center).style(Style::default().fg(theme.muted)),
            note_area,
        );
    }
    render_big_metric(frame, Msg::Cps.text(lang), &format!("{:.2}", r.cps), theme, cols[1]);
    render_big_metric(frame, Msg::Accuracy.text(lang), &format!("{:.1}%", r.accuracy * 100.0), theme, cols[2]);
}

fn render_big_metric(frame: &mut Frame, label: &str, value: &str, theme: &Theme, area: Rect) {
    let big_rows = big_text(value);

    let mut lines: Vec<Line> = Vec::with_capacity(7);
    lines.push(Line::from(Span::styled(
        label.to_string(),
        Style::default().fg(theme.muted).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    for row in big_rows {
        lines.push(Line::from(Span::styled(
            row,
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
        )));
    }

    let p = Paragraph::new(lines).alignment(Alignment::Center);
    frame.render_widget(p, area);
}

fn render_secondary_box(frame: &mut Frame, app: &App, r: &GameResult, theme: &Theme, area: Rect, lang: Lang) {
    let block = Block::default().borders(Borders::ALL).style(Style::default().fg(theme.muted));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{}: {:>4}", Msg::Words.text(lang), r.metrics.completed_words),
                Style::default().fg(theme.fg),
            ),
            Span::raw("    "),
            Span::styled(
                format!("{}: {:>3}", Msg::Mistypes.text(lang), r.metrics.mistypes_total),
                Style::default().fg(theme.fg),
            ),
            Span::raw("    "),
            Span::styled(format!("{}: {:>3}", Msg::MaxCombo.text(lang), r.max_combo), Style::default().fg(theme.fg)),
            Span::raw("    "),
            Span::styled(
                format!("{}: {:>3}", Msg::Timeouts.text(lang), r.metrics.timeout_words),
                Style::default().fg(theme.fg),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!("{}: {:>5.2}", Msg::Consistency.text(lang), r.consistency),
                Style::default().fg(theme.fg),
            ),
            Span::raw("    "),
            Span::styled(
                format!("{}: {:>5.2}s", Msg::MeanReaction.text(lang), r.mean_reaction.as_secs_f64()),
                Style::default().fg(theme.fg),
            ),
            Span::raw("    "),
            Span::styled(
                format!("{}: {:>5.2}s", Msg::P90Reaction.text(lang), r.p90_reaction.as_secs_f64()),
                Style::default().fg(theme.fg),
            ),
        ]),
    ];
    if let Some(comparison) = app.bench_comparison {
        let previous = comparison
            .previous_wpm_delta
            .map_or_else(|| Msg::InitialResult.text(lang).to_owned(), |value| format!("{value:+.1} WPM"));
        let recent = comparison
            .recent_three_wpm_delta
            .map_or_else(|| Msg::InitialResult.text(lang).to_owned(), |value| format!("{value:+.1} WPM"));
        lines.push(Line::from(format!(
            "{}: {previous}    {}: {recent}",
            Msg::PreviousDelta.text(lang),
            Msg::RecentThreeDelta.text(lang)
        )));
    }
    if app.gate_failed && app.result_origin == ResultOrigin::JustPlayed {
        lines.push(Line::from(Span::styled(
            match lang {
                Lang::En => "Gate not met: this session does not count toward personal bests",
                Lang::Ja => "関門未達: 自己ベストに数えません",
            },
            Style::default().fg(theme.warning).add_modifier(Modifier::BOLD),
        )));
    }
    if let Some(target) = app.next_pacer_target_wpm.filter(|_| app.result_origin == ResultOrigin::JustPlayed) {
        lines.push(Line::from(match lang {
            Lang::En => format!("Next target: {target:.0} WPM"),
            Lang::Ja => format!("次回の目標: {target:.0} WPM"),
        }));
    }
    if let Some(key) = app.new_unlocked_key.filter(|_| app.result_origin == ResultOrigin::JustPlayed) {
        lines.push(Line::from(Span::styled(
            match lang {
                Lang::En => format!("New key: {key}"),
                Lang::Ja => format!("新しいキー: {key}"),
            },
            Style::default().fg(theme.correct).add_modifier(Modifier::BOLD),
        )));
    }
    if app.config.mode.kind == crate::config::ModeKind::Rhythm
        && app.result_origin == ResultOrigin::JustPlayed
        && let Some(deviation) = app.rhythm_deviation_ms
    {
        lines.push(Line::from(match lang {
            Lang::En => format!("Median beat deviation: {deviation:.1} ms"),
            Lang::Ja => format!("拍からのずれの中央値: {deviation:.1} ms"),
        }));
    }
    if app.config.mode.kind == crate::config::ModeKind::Burst && app.result_origin == ResultOrigin::JustPlayed {
        lines.push(Line::from(match lang {
            Lang::En => format!("Burst: {} / 3    best: {:.1} WPM", app.burst_series.run_count(), app.burst_series.best()),
            Lang::Ja => format!(
                "バースト: {} / 3 本    最高: {:.1} WPM",
                app.burst_series.run_count(),
                app.burst_series.best()
            ),
        }));
    }
    if app.config.mode.kind == crate::config::ModeKind::Endurance
        && app.result_origin == ResultOrigin::JustPlayed
        && let Some(comparison) = app.endurance_comparison
    {
        lines.push(Line::from(match lang {
            Lang::En => format!(
                "First/second half IKI: {:.0}/{:.0} ms    errors: {:.1}/{:.1}%",
                comparison.first_iki_ms,
                comparison.second_iki_ms,
                comparison.first_error_rate * 100.0,
                comparison.second_error_rate * 100.0
            ),
            Lang::Ja => format!(
                "前半/後半 IKI: {:.0}/{:.0} ms    誤打率: {:.1}/{:.1}%",
                comparison.first_iki_ms,
                comparison.second_iki_ms,
                comparison.first_error_rate * 100.0,
                comparison.second_error_rate * 100.0
            ),
        }));
        if comparison.warn {
            lines.push(Line::from(Span::styled(
                match lang {
                    Lang::En => "Performance drops in the second half. Take a break.",
                    Lang::Ja => "後半に崩れています．休憩をはさみましょう",
                },
                Style::default().fg(theme.warning).add_modifier(Modifier::BOLD),
            )));
        }
    }

    let p = Paragraph::new(lines);
    frame.render_widget(p, left_inset(inner));
}

fn render_adaptive_result(frame: &mut Frame, app: &App, theme: &Theme, area: Rect, lang: Lang) {
    let mut lines = vec![Line::from(Span::styled(
        match lang {
            Lang::En => "Today's focus — this session IKI / prior estimate",
            Lang::Ja => "今日の重点 — 今回の IKI / 前回までの推定値",
        },
        Style::default().fg(theme.muted),
    ))];
    for row in &app.adaptive_result {
        let current = row.session_iki_ms.map_or_else(|| "—".to_owned(), |value| format!("{value:.0} ms"));
        lines.push(Line::from(format!(
            "  {}→{}  {current:>8} / {:>6.0} ms",
            row.from, row.to, row.prior_estimated_iki_ms
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        match lang {
            Lang::En => "This practice WPM is not an improvement indicator. Check progress with the benchmark.",
            Lang::Ja => "この練習の WPM は上達の指標ではありません．上達はベンチで確かめてください",
        },
        Style::default().fg(theme.warning),
    )));
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_mistype_heatmap(frame: &mut Frame, r: &GameResult, theme: &Theme, area: Rect, lang: Lang) {
    let mut entries: Vec<(&String, &u32)> = r.metrics.mistypes_by_char.iter().collect();
    entries.sort_by(|a, b| b.1.cmp(a.1));
    let top = entries.into_iter().take(8);
    let max_count = r.metrics.mistypes_by_char.values().copied().max().unwrap_or(1).max(1);

    let mut lines = vec![Line::from(Span::styled(Msg::MistypeHeatmap.text(lang), Style::default().fg(theme.muted)))];

    let mut rendered = 0;
    for (g, count) in top {
        let bar_len = (12 * (*count) / max_count).max(1);
        let bar: String = "█".repeat(bar_len as usize);
        lines.push(Line::from(vec![
            Span::styled(format!("  {g:>3}  "), Style::default().fg(theme.fg).add_modifier(Modifier::BOLD)),
            Span::styled(bar, Style::default().fg(theme.mistype)),
            Span::styled(format!("  {count}"), Style::default().fg(theme.muted)),
        ]));
        rendered += 1;
    }
    if rendered == 0 {
        lines.push(Line::from(Span::styled(
            format!("  {}", Msg::NoMistypes.text(lang)),
            Style::default().fg(theme.correct),
        )));
    }
    let p = Paragraph::new(lines);
    frame.render_widget(p, area);
}

fn render_reaction_and_save(frame: &mut Frame, app: &App, r: &GameResult, theme: &Theme, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    let mut lines = if app.result_origin == ResultOrigin::JustPlayed && !app.result_insights.is_empty() {
        insight_lines(app, theme)
    } else {
        let mut buckets = [0u32; 4];
        for duration in &r.metrics.reaction_times {
            match duration.as_secs_f64() {
                value if value < 0.3 => buckets[0] += 1,
                value if value < 0.5 => buckets[1] += 1,
                value if value < 0.8 => buckets[2] += 1,
                _ => buckets[3] += 1,
            }
        }
        let maximum = buckets.iter().copied().max().unwrap_or(1).max(1);
        let labels = ["<0.3s    ", "0.3-0.5s ", "0.5-0.8s ", ">0.8s    "];
        let mut lines = vec![Line::from(Span::styled(
            Msg::ReactionTimeDistribution.text(app.config.general.lang),
            Style::default().fg(theme.muted),
        ))];
        for (index, count) in buckets.iter().enumerate() {
            let bar_len = (12 * (*count) / maximum).max(u32::from(*count > 0));
            lines.push(Line::from(vec![
                Span::styled(format!("  {} ", labels[index]), Style::default().fg(theme.fg)),
                Span::styled("█".repeat(bar_len as usize), Style::default().fg(theme.accent)),
                Span::styled(format!("  {count}"), Style::default().fg(theme.muted)),
            ]));
        }
        lines
    };
    lines.truncate(usize::from(chunks[0].height));
    frame.render_widget(Paragraph::new(lines), chunks[0]);

    let save_line = match app.result_origin {
        ResultOrigin::JustPlayed => {
            if let Some(id) = app.last_save_id {
                Line::from(vec![
                    Span::styled(
                        format!("💾 {}", Msg::SavedToDatabase.text(app.config.general.lang)),
                        Style::default().fg(theme.correct),
                    ),
                    Span::styled(id.to_string(), Style::default().fg(theme.fg)),
                ])
            } else if let Some(e) = &app.last_save_error {
                Line::from(vec![
                    Span::styled(
                        format!("💾 {} ", Msg::SaveFailed.text(app.config.general.lang)),
                        Style::default().fg(theme.mistype),
                    ),
                    Span::styled(e.clone(), Style::default().fg(theme.fg)),
                ])
            } else {
                Line::from(Span::styled(
                    Msg::NotSaved.text(app.config.general.lang),
                    Style::default().fg(theme.muted),
                ))
            }
        }
        ResultOrigin::History => {
            let stamp = r.finished_at.format("%Y-%m-%d %H:%M:%S").to_string();
            Line::from(vec![
                Span::styled(
                    format!("📜 {} ", Msg::HistoryEntry.text(app.config.general.lang)),
                    Style::default().fg(theme.muted),
                ),
                Span::styled(stamp, Style::default().fg(theme.fg)),
            ])
        }
    };
    frame.render_widget(Paragraph::new(save_line), left_inset(chunks[1]));
}

fn render_result_insights(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    frame.render_widget(Paragraph::new(insight_lines(app, theme)), area);
}

fn insight_lines(app: &App, theme: &Theme) -> Vec<Line<'static>> {
    let limit = if app.config.display.feedback == "detailed" { 3 } else { 1 };
    let mut lines = vec![Line::from(Span::styled(
        match app.config.general.lang {
            Lang::En => "This session's observation",
            Lang::Ja => "今回の気づき",
        },
        Style::default().fg(theme.warning).add_modifier(Modifier::BOLD),
    ))];
    lines.extend(app.result_insights.iter().take(limit).cloned().map(Line::from));
    lines
}

fn render_hint(frame: &mut Frame, app: &App, theme: &Theme, area: Rect, lang: Lang) {
    let remaining = app.result_input_delay_remaining();
    let hint = remaining.map_or_else(
        || Msg::ResultHint.text(lang).to_owned(),
        |remaining| format!("{}  … {:.1}s", Msg::ResultHint.text(lang), remaining.as_secs_f32()),
    );
    let color = if remaining.is_some() { theme.muted } else { theme.accent };
    let p = Paragraph::new(hint).style(Style::default().fg(color));
    frame.render_widget(p, left_inset(area));
}

fn render_hint_history(frame: &mut Frame, app: &App, theme: &Theme, area: Rect, lang: Lang) {
    let actions = if app
        .history_detail
        .as_ref()
        .is_some_and(|detail| detail.record.header.metrics_version != 0)
    {
        match lang {
            Lang::En => "[R] Replay    [E] Export    ",
            Lang::Ja => "[R] 再生    [E] 書き出し    ",
        }
    } else {
        ""
    };
    let p = Paragraph::new(format!("{actions}{}", Msg::HistoryDetailHint.text(lang))).style(Style::default().fg(theme.accent));
    frame.render_widget(p, left_inset(area));
}

/// Render `s` as a 5-row block-character "big text" suitable for the result
/// summary. Supported glyphs are `0-9`, `.`, `%`, and space; unknown
/// characters render as blank.
fn big_text(s: &str) -> [String; 5] {
    let mut rows: [String; 5] = Default::default();
    for c in s.chars() {
        let glyph = big_glyph(c);
        for (i, line) in glyph.iter().enumerate() {
            rows[i].push_str(line);
            rows[i].push(' ');
        }
    }
    rows
}

fn big_glyph(c: char) -> [&'static str; 5] {
    match c {
        '0' => [" ███ ", "█   █", "█   █", "█   █", " ███ "],
        '1' => ["  █  ", " ██  ", "  █  ", "  █  ", " ███ "],
        '2' => [" ███ ", "█   █", "   █ ", "  █  ", "█████"],
        '3' => [" ███ ", "█   █", "  ██ ", "█   █", " ███ "],
        '4' => ["█   █", "█   █", "█████", "    █", "    █"],
        '5' => ["█████", "█    ", "████ ", "    █", "████ "],
        '6' => [" ███ ", "█    ", "████ ", "█   █", " ███ "],
        '7' => ["█████", "    █", "   █ ", "  █  ", "  █  "],
        '8' => [" ███ ", "█   █", " ███ ", "█   █", " ███ "],
        '9' => [" ███ ", "█   █", " ████", "    █", " ███ "],
        '.' => ["   ", "   ", "   ", "   ", " █ "],
        '%' => ["█   █", "   █ ", "  █  ", " █   ", "█   █"],
        ' ' => ["  ", "  ", "  ", "  ", "  "],
        _ => ["  ", "  ", "  ", "  ", "  "],
    }
}
