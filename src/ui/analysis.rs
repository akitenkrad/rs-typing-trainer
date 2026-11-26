use std::collections::{BTreeMap, BTreeSet};

use chrono::{Datelike, Duration as ChronoDuration, NaiveDate};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect, Size};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Block, Borders, Chart, Clear, Dataset, GraphType, Paragraph};
use ratatui_image::Image;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::analysis::bench_v3::{BenchV3Group, BenchV3Metrics, bench_v3_record_order, bench_v3_run_key};
use crate::analysis::growth::BenchV3GrowthTarget;
use crate::app::{AnalysisPeriod, App};
use crate::bench::{BENCH_SERIES_V1, BENCH_SERIES_V2, BenchPart, BenchV3Part, is_preferred_bench_series, parse_bench_id, preferred_bench_series};
use crate::graphics::GraphicsMode;
use crate::i18n::{Lang, Msg};
use crate::layout::geometry::geometry;
use crate::ui::analysis_image::{
    ComparisonBarsSpec, FORECAST_EN_COLOR, FORECAST_JA_COLOR, FingerChartSpec, GradeLadderSpec, MetricKind, PixelPoint, RadarAxis, RadarChartSpec,
    RadarRing, SERIES_COLORS, SparklineSpec, TREND_BREAK_WIDTH_DAYS, TREND_TICK_LABEL_WIDTH, TextTrendData, TransitionArrowSpec, TransitionBar,
    TransitionBarsSpec, TransitionKeyboardSpec, TrendAxis, TrendChartSpec, TrendForecast, TrendPoint, TrialChartSpec, build_finger_bars,
    build_text_trend_data, build_trend_data, metric_value, mode_label, shared_trend_axis, trend_date_label, trend_tick_candidates,
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
    if app.analysis_tab == crate::app::ANALYSIS_TODAY_TAB && app.analysis_today_summary {
        // Summary (MYTASK-3632): no tab bar and a one-line footer, for screenshots.
        crate::ui::today_summary::render(frame, app, &theme, inner);
        return;
    }
    if app.analysis_tab == crate::app::ANALYSIS_BENCH_TAB && app.analysis_bench_summary {
        // Summary (MYTASK-3644): the same screenshot layout for the benchmark tab.
        crate::ui::bench_tab_summary::render(frame, app, &theme, inner);
        return;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(3), Constraint::Length(2)])
        .split(inner);
    render_tabs(frame, app, &theme, chunks[0]);
    match app.analysis_tab {
        crate::app::ANALYSIS_TODAY_TAB => crate::ui::today::render(frame, app, &theme, chunks[1]),
        crate::app::ANALYSIS_COACH_TAB => render_summary(frame, app, &theme, chunks[1]),
        crate::app::ANALYSIS_TREND_TAB => render_trend(frame, app, chunks[1]),
        crate::app::ANALYSIS_KEYBOARD_TAB => render_keyboard(frame, app, chunks[1]),
        crate::app::ANALYSIS_TRANSITIONS_TAB => render_transitions(frame, app, &theme, chunks[1]),
        crate::app::ANALYSIS_FINGERS_TAB => render_fingers(frame, app, chunks[1]),
        crate::app::ANALYSIS_BENCH_TAB => render_bench(frame, app, chunks[1]),
        crate::app::ANALYSIS_SKILLS_TAB => render_skills(frame, app, &theme, chunks[1]),
        _ => render_growth(frame, app, chunks[1]),
    }
    if app.analysis_tab == crate::app::ANALYSIS_TODAY_TAB {
        // The «today» tab reads today's records directly; the period and mode
        // filters do not apply, so they are not shown.
        let hint = crate::ui::today::hint(lang).map(Line::from).to_vec();
        frame.render_widget(Paragraph::new(hint).alignment(Alignment::Center), chunks[2]);
        return;
    }
    if app.analysis_tab == crate::app::ANALYSIS_GROWTH_TAB {
        let hint = match lang {
            Lang::En => "[↑/↓ PgUp/PgDn] Scroll   [?] Terms   Compares all time   [Esc] Back",
            Lang::Ja => "[↑/↓ PgUp/PgDn] スクロール   [?] 用語   全期間で比べます   [Esc] 戻る",
        };
        frame.render_widget(Paragraph::new(hint).alignment(Alignment::Center), chunks[2]);
        return;
    }
    let period = match app.analysis_period {
        AnalysisPeriod::All => Msg::PeriodAll.text(lang),
        AnalysisPeriod::Days30 => Msg::PeriodDays30.text(lang),
        AnalysisPeriod::Days182 => Msg::PeriodDays182.text(lang),
        AnalysisPeriod::Days365 => Msg::PeriodDays365.text(lang),
        AnalysisPeriod::Days7 => Msg::PeriodDays7.text(lang),
    };
    let mode = app
        .analysis_mode_filter
        .as_deref()
        .map_or_else(|| Msg::ModeAll.text(lang).to_owned(), |mode| mode_label(mode, lang));
    let hint = format!(
        "{}   {}   {}: {period}   {}: {mode}   {}",
        Msg::AnalysisHintTab.text(lang),
        if lang == Lang::En { "[?] Terms" } else { "[?] 用語" },
        Msg::AnalysisHintPeriod.text(lang),
        Msg::AnalysisHintMode.text(lang),
        Msg::AnalysisHintBack.text(lang)
    );
    let mut hint_lines = vec![Line::from(hint)];
    if app.analysis_tab == crate::app::ANALYSIS_BENCH_TAB {
        hint_lines.push(Line::from("[S] Summary"));
    }
    if app.analysis_tab == crate::app::ANALYSIS_SKILLS_TAB
        && let Some(language) = skills_language(app)
    {
        hint_lines.push(Line::from(match lang {
            Lang::En => format!("[L] language: {}", language_label(&language, lang)),
            Lang::Ja => format!("[L] 言語: {}", language_label(&language, lang)),
        }));
    }
    frame.render_widget(Paragraph::new(hint_lines).alignment(Alignment::Center), chunks[2]);
}

fn render_tabs(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let lang = app.config.general.lang;
    let labels = [
        match lang {
            Lang::En => "Today",
            Lang::Ja => "今日",
        },
        match lang {
            Lang::En => "Coach",
            Lang::Ja => "コーチ",
        },
        Msg::AnalysisTrend.text(lang),
        Msg::AnalysisKeyboard.text(lang),
        if area.width < 90 {
            match lang {
                Lang::En => "Trans.",
                Lang::Ja => "遷移",
            }
        } else {
            Msg::AnalysisTransitions.text(lang)
        },
        if area.width < 90 {
            match lang {
                Lang::En => "Fingers",
                Lang::Ja => "指・手",
            }
        } else {
            Msg::AnalysisFingersHands.text(lang)
        },
        Msg::AnalysisBench.text(lang),
        match lang {
            Lang::En => "Skills",
            Lang::Ja => "スキル",
        },
        match lang {
            Lang::En => "Growth",
            Lang::Ja => "成長",
        },
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

fn render_growth(frame: &mut Frame, app: &App, area: Rect) {
    use crate::analysis::growth::{comparison_guidance, format_duration, headline};
    use crate::analysis::progress::DailyTrendSource;

    let lang = app.config.general.lang;
    let width = usize::from(area.width).max(1);
    let image_enabled = app.graphics.mode != GraphicsMode::Off && app.graphics.cell_pixels.is_some();
    let mut lines = vec![headline(&app.analysis_growth, lang).text, String::new()];
    for (growth_index, growth) in app.analysis_growth.languages.iter().enumerate() {
        let v3_group = app.analysis_growth.bench_v3.iter().find(|row| {
            matches!(
                (growth.language.as_str(), row.target),
                ("en", BenchV3GrowthTarget::Group(BenchV3Group::English)) | ("ja", BenchV3GrowthTarget::Group(BenchV3Group::Japanese))
            )
        });
        let v3_mdc = v3_group.and_then(|row| row.reliability.map(|value| value.mdc95_wpm));
        let language = language_label(&growth.language, lang);
        let source = match (growth.source, lang) {
            (DailyTrendSource::Bench, Lang::En) => "benchmark",
            (DailyTrendSource::Bench, Lang::Ja) => "ベンチ",
            (DailyTrendSource::BenchV3, Lang::En) => "benchmark v3 (accuracy-adjusted speed)",
            (DailyTrendSource::BenchV3, Lang::Ja) => "ベンチ v3（正確さを含む速さ）",
            (DailyTrendSource::CorrectedWpm, Lang::En) => "adjusted WPM",
            (DailyTrendSource::CorrectedWpm, Lang::Ja) => "補正 WPM",
        };
        lines.push(format!("■ {language}"));
        lines.push(match lang {
            Lang::En => format!("  Since you started (daily speed: {source})"),
            Lang::Ja => format!("  はじめの頃と今（日ごとの速さ: {source}）"),
        });
        if growth.recorded_days < 7 {
            lines.push(format!("    {}", comparison_guidance(growth.source, growth.recorded_days, lang, true)));
        } else {
            lines.push(period_line(growth.first.as_ref(), true, lang));
            lines.push(period_line(growth.recent.as_ref(), false, lang));
            if let Some(change) = growth.change {
                let speed = if growth.source == DailyTrendSource::BenchV3 {
                    crate::ui::result::format_v3_change(change.speed, v3_mdc, lang)
                } else {
                    format_growth_delta(change.speed, " WPM")
                };
                let accuracy = change
                    .accuracy_points
                    .map_or(String::new(), |value| format!("   {}", format_growth_delta(value, "pt")));
                lines.push(match lang {
                    Lang::En => format!("    Change        {speed} ({:+.0}%){accuracy}", change.speed_percent.unwrap_or_default(),),
                    Lang::Ja => format!("    変化          {speed}（{:+.0}%）{accuracy}", change.speed_percent.unwrap_or_default(),),
                });
            }
        }
        lines.push(String::new());
        lines.push(match lang {
            Lang::En => "  One and three months ago".to_owned(),
            Lang::Ja => "  1 か月前・3 か月前の自分".to_owned(),
        });
        lines.push(anchor_line(growth.month_ago.as_ref(), 30, growth.recent.as_ref(), v3_mdc, lang));
        lines.push(anchor_line(growth.quarter_ago.as_ref(), 90, growth.recent.as_ref(), v3_mdc, lang));
        lines.push(String::new());
        lines.push(match lang {
            Lang::En => "  Benchmarks (first → latest, personal best)".to_owned(),
            Lang::Ja => "  ベンチマーク（初回 → 最新，自己ベスト）".to_owned(),
        });
        let v3_rows = app
            .analysis_growth
            .bench_v3
            .iter()
            .filter(|row| match (growth.language.as_str(), row.target) {
                ("en", BenchV3GrowthTarget::Group(BenchV3Group::English)) => true,
                ("en", BenchV3GrowthTarget::Part(part)) => matches!(part.as_str(), "EW" | "ES" | "PW"),
                ("ja", BenchV3GrowthTarget::Group(BenchV3Group::Japanese)) => true,
                ("ja", BenchV3GrowthTarget::Part(part)) => matches!(part.as_str(), "JW" | "JS"),
                _ => false,
            })
            .collect::<Vec<_>>();
        if growth.source == DailyTrendSource::BenchV3 && !v3_rows.is_empty() {
            for row in v3_rows {
                let label = match row.target {
                    BenchV3GrowthTarget::Group(BenchV3Group::English) => {
                        if lang == Lang::Ja {
                            "英語まとめ"
                        } else {
                            "English summary"
                        }
                    }
                    BenchV3GrowthTarget::Group(BenchV3Group::Japanese) => {
                        if lang == Lang::Ja {
                            "日本語まとめ"
                        } else {
                            "Japanese summary"
                        }
                    }
                    BenchV3GrowthTarget::Group(BenchV3Group::Transition) => {
                        if lang == Lang::Ja {
                            "移り変わりまとめ"
                        } else {
                            "Transition summary"
                        }
                    }
                    BenchV3GrowthTarget::Part(part) => part.as_str(),
                };
                let delta = row.latest.wpm - row.first.wpm;
                let change = crate::ui::result::format_v3_change(delta, row.reliability.map(|value| value.mdc95_wpm), lang);
                lines.push(format!(
                    "    {label}   {:.1} → {:.1} WPM   {change}   {} {:.1} ({})",
                    row.first.wpm,
                    row.latest.wpm,
                    if lang == Lang::Ja { "ベスト" } else { "best" },
                    row.best.wpm,
                    row.best.date.format("%m/%d")
                ));
            }
            if !growth.benches.is_empty() {
                let previous = growth
                    .benches
                    .iter()
                    .map(|bench| format!("{} {:.1} → {:.1} WPM", bench.part.as_str(), bench.first.wpm, bench.latest.wpm))
                    .collect::<Vec<_>>()
                    .join("，");
                lines.push(match lang {
                    Lang::En => format!("    Previous series: v2 {previous}"),
                    Lang::Ja => format!("    以前の系列: v2 {previous}"),
                });
            }
        } else if growth.benches.is_empty() {
            lines.push(format!("    {}", if lang == Lang::Ja { "記録なし" } else { "no records" }));
        } else {
            for bench in &growth.benches {
                let delta = bench.latest.wpm - bench.first.wpm;
                lines.push(format!(
                    "    {}   {:.1} → {:.1} WPM   {}   {} {:.1} ({})",
                    bench.part.as_str(),
                    bench.first.wpm,
                    bench.latest.wpm,
                    format_growth_delta(delta, ""),
                    if lang == Lang::Ja { "ベスト" } else { "best" },
                    bench.best.wpm,
                    bench.best.date.format("%m/%d")
                ));
            }
        }
        lines.push(String::new());
        let (chart_points, granularity) = if growth.source == DailyTrendSource::BenchV3 {
            let mut points = growth.points.clone();
            points.sort_by_key(|point| point.date);
            (points, GrowthChartGranularity::RollingThree)
        } else {
            growth_chart_points(&growth.points)
        };
        lines.push(match (lang, granularity) {
            (Lang::En, GrowthChartGranularity::RollingThree) => "  Speed over time (rolling median of the latest 3 runs)".to_owned(),
            (Lang::Ja, GrowthChartGranularity::RollingThree) => "  速さの推移（直近 3 回の中央値）".to_owned(),
            (Lang::En, GrowthChartGranularity::Weekly) => "  Speed over time (all time, weekly medians)".to_owned(),
            (Lang::Ja, GrowthChartGranularity::Weekly) => "  速さの推移（全期間，週ごとの中央値）".to_owned(),
            (Lang::En, GrowthChartGranularity::Daily) => "  Speed over time (all time, daily values)".to_owned(),
            (Lang::Ja, GrowthChartGranularity::Daily) => "  速さの推移（全期間，日ごとの値）".to_owned(),
        });
        if chart_points.len() >= 2 {
            lines.push(growth_endpoint_line(&chart_points, lang));
            lines.push(format!("__GROWTH_CHART_{growth_index}__"));
        } else {
            lines.push(match lang {
                Lang::En => format!(
                    "    The chart will be available after 2 days of records ({} day{} so far)",
                    chart_points.len(),
                    if chart_points.len() == 1 { "" } else { "s" }
                ),
                Lang::Ja => format!("    記録が 2 日分たまると図を描けます（今 {} 日分）", chart_points.len()),
            });
        }
        lines.push(String::new());
    }
    let totals = app.analysis_growth.accumulation;
    lines.push(match lang {
        Lang::En => "■ Accumulation".to_owned(),
        Lang::Ja => "■ 積み重ね".to_owned(),
    });
    lines.push(match lang {
        Lang::En => format!(
            "  Practice time   {} (this month {})",
            format_duration(totals.duration_seconds, lang),
            format_duration(totals.month_duration_seconds, lang)
        ),
        Lang::Ja => format!(
            "  練習時間   {}（今月 {}）",
            format_duration(totals.duration_seconds, lang),
            format_duration(totals.month_duration_seconds, lang)
        ),
    });
    lines.push(match lang {
        Lang::En => format!(
            "  Practice days {}   current streak {}   longest streak {}",
            crate::ui::today::grouped(totals.practice_days),
            crate::ui::today::grouped(totals.current_streak),
            crate::ui::today::grouped(totals.longest_streak)
        ),
        Lang::Ja => format!(
            "  練習した日 {} 日   いまの連続 {} 日   最長の連続 {} 日",
            crate::ui::today::grouped(totals.practice_days),
            crate::ui::today::grouped(totals.current_streak),
            crate::ui::today::grouped(totals.longest_streak)
        ),
    });
    lines.push(match lang {
        Lang::En => format!("  Keystrokes   {}", crate::ui::today::grouped(totals.keystrokes)),
        Lang::Ja => format!("  打鍵数     {}", crate::ui::today::grouped(totals.keystrokes)),
    });
    if !app.analysis_growth.personal_bests.is_empty() {
        let values = app
            .analysis_growth
            .personal_bests
            .iter()
            .map(|best| {
                format!(
                    "{} {} {} {:.1} WPM",
                    best.date.format("%m/%d"),
                    language_label(&best.language, lang),
                    best.part.as_str(),
                    best.wpm
                )
            })
            .collect::<Vec<_>>()
            .join("，");
        let label = match lang {
            Lang::En => "  Recent personal bests",
            Lang::Ja => "  最近の自己ベスト",
        };
        let label_width = UnicodeWidthStr::width(label);
        lines.extend(
            crate::ui::wrap_field(label, &values, label_width, width)
                .into_iter()
                .map(|line| format!("{}{}", line.label, line.value)),
        );
    }
    if !app.analysis_growth.promotions.is_empty() {
        let values = app
            .analysis_growth
            .promotions
            .iter()
            .map(|promotion| {
                format!(
                    "{} {} {} → {}",
                    promotion.date.format("%m/%d"),
                    language_label(&promotion.language, lang),
                    promotion.from.as_str(),
                    promotion.to.as_str()
                )
            })
            .collect::<Vec<_>>()
            .join("，");
        lines.push(match lang {
            Lang::En => format!("  Grade promotions   {values}"),
            Lang::Ja => format!("  段階の昇格         {values}"),
        });
    }
    if let Some(recent) = app.analysis_growth.graduations.first() {
        let destination = recent.level.map_or_else(
            || {
                if lang == Lang::Ja {
                    "維持".to_owned()
                } else {
                    "maintenance".to_owned()
                }
            },
            |level| {
                if lang == Lang::Ja {
                    format!("レベル {level}")
                } else {
                    format!("level {level}")
                }
            },
        );
        lines.push(match lang {
            Lang::En => format!(
                "  Graduated prescriptions   {} (latest: {} {} → {destination})",
                app.analysis_growth.graduations.len(),
                recent.date.format("%m/%d"),
                recent.title
            ),
            Lang::Ja => format!(
                "  卒業した処方       {} 件（最近: {} {} → {destination}）",
                app.analysis_growth.graduations.len(),
                recent.date.format("%m/%d"),
                recent.title
            ),
        });
    }

    let mut chart_rows = Vec::new();
    let mut wrapped = Vec::<String>::new();
    for line in &lines {
        if let Some(index) = line
            .strip_prefix("__GROWTH_CHART_")
            .and_then(|value| value.strip_suffix("__"))
            .and_then(|value| value.parse::<usize>().ok())
        {
            chart_rows.push((wrapped.len(), index));
            wrapped.extend((0..7).map(|_| String::new()));
        } else {
            wrapped.extend(wrap_coach_line(line, width));
        }
    }
    let maximum = wrapped.len().saturating_sub(usize::from(area.height)).min(u16::MAX as usize) as u16;
    app.set_analysis_growth_scroll_max(maximum);
    let scroll = app.analysis_growth_scroll.min(maximum);
    frame.render_widget(
        Paragraph::new(wrapped.into_iter().map(Line::from).collect::<Vec<_>>()).scroll((scroll, 0)),
        area,
    );
    for (row, index) in chart_rows {
        let visible = row as i32 - i32::from(scroll);
        if visible < 0 || visible + 7 > i32::from(area.height) {
            continue;
        }
        let Some(growth) = app.analysis_growth.languages.get(index) else {
            continue;
        };
        let (points, _) = if growth.source == DailyTrendSource::BenchV3 {
            let mut points = growth.points.clone();
            points.sort_by_key(|point| point.date);
            (points, GrowthChartGranularity::RollingThree)
        } else {
            growth_chart_points(&growth.points)
        };
        let raw = app
            .analysis_growth
            .bench_v3
            .iter()
            .find(|row| {
                matches!(
                    (growth.language.as_str(), row.target),
                    ("en", BenchV3GrowthTarget::Group(BenchV3Group::English)) | ("ja", BenchV3GrowthTarget::Group(BenchV3Group::Japanese))
                )
            })
            .map(|row| row.raw.as_slice())
            .unwrap_or_default();
        let Some(axis) = TrendAxis::from_dates(points.iter().map(|point| point.date).chain(raw.iter().map(|point| point.date))) else {
            continue;
        };
        let chart_area = Rect::new(area.x.saturating_add(4), area.y + visible as u16, area.width.saturating_sub(4), 7);
        let color = if growth.language == "ja" { SERIES_COLORS[1] } else { SERIES_COLORS[0] };
        let mut spec = TrendChartSpec {
            kind: MetricKind::NetWpm,
            cells: chart_area.as_size(),
            series: vec![crate::ui::analysis_image::TrendSeries {
                name: language_label(&growth.language, lang).to_owned(),
                color,
                raw_points: raw
                    .iter()
                    .map(|point| TrendPoint {
                        date: point.date,
                        value: point.wpm,
                    })
                    .collect(),
                points: points
                    .iter()
                    .map(|point| TrendPoint {
                        date: point.date,
                        value: point.value,
                    })
                    .collect(),
            }],
            forecasts: Vec::new(),
            x_ticks: trend_tick_candidates(&axis, f64::from(chart_area.width.saturating_sub(8)) / axis.span().max(1.0)),
        };
        if !image_enabled || !render_metric_chart_image(frame, app, chart_area, lang, &mut spec, &axis) {
            render_growth_chart_text(frame, chart_area, &points, raw, &axis, color);
        }
    }
}

fn growth_endpoint_line(points: &[crate::analysis::progress::DailyPoint], lang: Lang) -> String {
    let (Some(first), Some(latest)) = (points.first(), points.last()) else {
        return String::new();
    };
    match lang {
        Lang::En => format!(
            "    first {} {:.1} WPM   latest {} {:.1} WPM",
            first.date.format("%Y-%m-%d"),
            first.value,
            latest.date.format("%Y-%m-%d"),
            latest.value
        ),
        Lang::Ja => format!(
            "    最初 {} {:.1} WPM   最新 {} {:.1} WPM",
            first.date.format("%Y-%m-%d"),
            first.value,
            latest.date.format("%Y-%m-%d"),
            latest.value
        ),
    }
}

fn render_growth_chart_text(
    frame: &mut Frame,
    area: Rect,
    points: &[crate::analysis::progress::DailyPoint],
    raw: &[crate::analysis::growth::BenchPoint],
    axis: &TrendAxis,
    color: Color,
) {
    let mut runs = Vec::<Vec<(f64, f64)>>::new();
    let mut previous = None;
    for point in points {
        match (runs.last_mut(), previous) {
            (Some(run), Some(previous)) if !axis.crosses_break(previous, point.date) => {
                run.push((axis.position(point.date), point.value));
            }
            _ => runs.push(vec![(axis.position(point.date), point.value)]),
        }
        previous = Some(point.date);
    }
    let raw_values = raw.iter().map(|point| (axis.position(point.date), point.wpm)).collect::<Vec<_>>();
    let mut datasets = if raw_values.is_empty() {
        Vec::new()
    } else {
        vec![
            Dataset::default()
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Scatter)
                .style(Style::default().fg(color).add_modifier(Modifier::DIM))
                .data(&raw_values),
        ]
    };
    datasets.extend(runs.iter().map(|run| {
        Dataset::default()
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(color))
            .data(run)
    }));
    let minimum = points
        .iter()
        .map(|point| point.value)
        .chain(raw.iter().map(|point| point.wpm))
        .fold(f64::INFINITY, f64::min);
    let maximum = points
        .iter()
        .map(|point| point.value)
        .chain(raw.iter().map(|point| point.wpm))
        .fold(f64::NEG_INFINITY, f64::max);
    let padding = if (maximum - minimum).abs() < f64::EPSILON {
        (maximum.abs() * 0.05).max(1.0)
    } else {
        0.0
    };
    let chart = Chart::new(datasets)
        .x_axis(Axis::default().bounds([0.0, axis.span().max(1.0)]).labels(vec![
            Span::raw(trend_date_label(axis.start(), true)),
            Span::raw(trend_date_label(axis.end(), axis.end().year() != axis.start().year())),
        ]))
        .y_axis(
            Axis::default()
                .bounds([minimum - padding, maximum + padding])
                .labels(vec![Span::raw(format!("{minimum:.1}")), Span::raw(format!("{maximum:.1}"))]),
        );
    frame.render_widget(chart, area);
}

fn period_line(period: Option<&crate::analysis::growth::TrendPeriod>, first: bool, lang: Lang) -> String {
    let label = match (first, lang) {
        (true, Lang::En) => "First 7 days ",
        (false, Lang::En) => "Latest 7 days",
        (true, Lang::Ja) => "最初の 7 日 ",
        (false, Lang::Ja) => "直近の 7 日 ",
    };
    let Some(period) = period else {
        return format!("    {label}   {}", if lang == Lang::Ja { "記録なし" } else { "no records" });
    };
    let speed = period.speed.map_or_else(|| "—".to_owned(), |value| format!("{value:.1} WPM"));
    let accuracy = period.accuracy.map_or_else(|| "—".to_owned(), |value| format!("{:.1}%", value * 100.0));
    format!(
        "    {label}   {speed}   {} {accuracy}   ({}–{})",
        if lang == Lang::Ja { "正確さ" } else { "accuracy" },
        period.start.format("%Y-%m-%d"),
        period.end.format("%m-%d")
    )
}

fn anchor_line(
    anchor: Option<&crate::analysis::growth::TrendPeriod>,
    days: i64,
    recent: Option<&crate::analysis::growth::TrendPeriod>,
    mdc95: Option<f64>,
    lang: Lang,
) -> String {
    let label = match (days, lang) {
        (30, Lang::En) => "1 month ago",
        (30, Lang::Ja) => "1 か月前",
        (_, Lang::En) => "3 months ago",
        (_, Lang::Ja) => "3 か月前",
    };
    let Some(old) = anchor.and_then(|period| period.speed) else {
        return format!("    {label}   {}", if lang == Lang::Ja { "記録なし" } else { "no records" });
    };
    let Some(now) = recent.and_then(|period| period.speed) else {
        return format!("    {label}   {old:.1} WPM");
    };
    let delta = now - old;
    let change = if mdc95.is_some() {
        crate::ui::result::format_v3_change(delta, mdc95, lang)
    } else {
        format_growth_delta(delta, "")
    };
    format!("    {label}   {old:.1} WPM → {now:.1} WPM   {change}")
}

fn format_growth_delta(value: f64, suffix: &str) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    if rounded == 0.0 {
        format!("±0.0{suffix}")
    } else if rounded > 0.0 {
        format!("▲ {value:+.1}{suffix}")
    } else {
        format!("▼ {value:+.1}{suffix}")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GrowthChartGranularity {
    RollingThree,
    Weekly,
    Daily,
}

fn growth_chart_points(points: &[crate::analysis::progress::DailyPoint]) -> (Vec<crate::analysis::progress::DailyPoint>, GrowthChartGranularity) {
    let weekly = weekly_medians(points);
    if weekly.len() >= 2 {
        return (weekly, GrowthChartGranularity::Weekly);
    }
    let mut daily = points.to_vec();
    daily.sort_by_key(|point| point.date);
    (daily, GrowthChartGranularity::Daily)
}

fn weekly_medians(points: &[crate::analysis::progress::DailyPoint]) -> Vec<crate::analysis::progress::DailyPoint> {
    use std::collections::BTreeMap;

    let mut weeks = BTreeMap::<(i32, u32), Vec<&crate::analysis::progress::DailyPoint>>::new();
    for point in points {
        let week = point.date.iso_week();
        weeks.entry((week.year(), week.week())).or_default().push(point);
    }
    weeks
        .into_values()
        .filter_map(|mut week| {
            week.sort_by(|left, right| left.value.total_cmp(&right.value));
            let middle = week.len() / 2;
            let value = if week.len().is_multiple_of(2) {
                (week[middle - 1].value + week[middle].value) / 2.0
            } else {
                week[middle].value
            };
            Some(crate::analysis::progress::DailyPoint {
                date: week.iter().map(|point| point.date).min()?,
                value,
            })
        })
        .collect()
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
    // One axis for the three image charts (forecasts included) and one for the
    // three text charts, so that a day sits in the same column in each chart.
    let image_axis = shared_trend_axis(image_specs.iter().flatten());
    let text_data = kinds.map(|kind| build_text_trend_data(&app.analysis_records, kind, lang));
    let text_axis = TrendAxis::from_dates(text_data.iter().flatten().flat_map(|data| {
        data.series
            .iter()
            .flat_map(|series| series.points.iter().chain(&series.raw_points).filter_map(|(date, _)| *date))
    }));
    let text_unknown = text_data.iter().flatten().any(|data| {
        data.series
            .iter()
            .any(|series| series.points.iter().chain(&series.raw_points).any(|(date, _)| date.is_none()))
    });
    for (((area, kind), spec), text) in chunks.iter().copied().zip(kinds).zip(image_specs).zip(text_data) {
        if let (Some(mut spec), Some(axis)) = (spec, image_axis.as_ref())
            && render_metric_chart_image(frame, app, area, lang, &mut spec, axis)
        {
            continue;
        }
        let text_axis = text_axis.as_ref().map(|axis| TextTrendAxis { axis, unknown: text_unknown });
        render_metric_chart_text(frame, &app.analysis_records, area, kind, lang, text, text_axis);
    }
}

/// The x axis of the text trend charts: the shared [`TrendAxis`], plus a slot
/// after its end for records whose timestamp could not be read.
#[derive(Clone, Copy)]
struct TextTrendAxis<'a> {
    axis: &'a TrendAxis,
    unknown: bool,
}

impl TextTrendAxis<'_> {
    fn x(&self, date: Option<chrono::NaiveDate>) -> f64 {
        match date {
            Some(date) => self.axis.position(date),
            None => self.axis.span() + TREND_BREAK_WIDTH_DAYS as f64,
        }
    }

    fn upper_bound(&self) -> f64 {
        let end = if self.unknown {
            self.axis.span() + TREND_BREAK_WIDTH_DAYS as f64
        } else {
            self.axis.span()
        };
        end.max(1.0)
    }

    fn crosses_break(&self, a: Option<chrono::NaiveDate>, b: Option<chrono::NaiveDate>) -> bool {
        match (a, b) {
            (Some(a), Some(b)) => self.axis.crosses_break(a, b),
            _ => a != b,
        }
    }
}

/// Marks the axis breaks with `≈` on `axis_row` and writes the first date of
/// each segment after a break on `label_row` where the row is still blank
/// (a `≈` counts as blank, so the date may sit right after its mark).
/// `column_of` maps a fraction of the axis to a terminal column.
fn overlay_axis_breaks(
    frame: &mut Frame,
    axis: &TrendAxis,
    to_fraction: impl Fn(f64) -> f64,
    column_of: impl Fn(f64) -> u16,
    axis_row: u16,
    label_row: Option<u16>,
    right: u16,
) {
    let buffer = frame.buffer_mut();
    let span = axis.span().max(1.0);
    for ((fraction, segment), previous) in axis.break_fractions().into_iter().zip(&axis.segments()[1..]).zip(axis.segments()) {
        let mark = column_of(to_fraction(fraction * span));
        if let Some(cell) = buffer.cell_mut((mark, axis_row)) {
            cell.set_symbol("≈");
        }
        let Some(label_row) = label_row else {
            continue;
        };
        let label = trend_date_label(segment.start, segment.start.year() != previous.end.year());
        let width = label.width() as u16;
        let mut start = column_of(to_fraction(axis.position(segment.start)));
        if label_row == axis_row {
            start = start.max(mark + 1);
        }
        if start + width > right {
            continue;
        }
        let blank = (start.saturating_sub(1)..(start + width + 1).min(right))
            .all(|x| buffer.cell((x, label_row)).is_some_and(|cell| matches!(cell.symbol(), " " | "≈")));
        if !blank {
            continue;
        }
        for (offset, ch) in label.chars().enumerate() {
            if let Some(cell) = buffer.cell_mut((start + offset as u16, label_row)) {
                cell.set_char(ch);
            }
        }
    }
}

/// Writes the x-axis tick labels (`MM/DD`) on `row`, each centred on the
/// column of its date, left to right, where the label and one column on each
/// side are still blank and inside `left..right`. Ticks whose label does not
/// fit (it would touch the endpoint dates, a break or another tick) are
/// dropped. Returns the ticks that got a label.
fn place_tick_labels(
    buffer: &mut Buffer,
    ticks: &[NaiveDate],
    column_of: impl Fn(NaiveDate) -> u16,
    row: u16,
    left: u16,
    right: u16,
) -> Vec<NaiveDate> {
    let mut placed = Vec::new();
    for &date in ticks {
        let label = trend_date_label(date, false);
        let width = label.width() as u16;
        let Some(start) = column_of(date).checked_sub(width / 2) else {
            continue;
        };
        if start < left || start + width > right {
            continue;
        }
        let blank = (start.saturating_sub(1).max(left)..(start + width + 1).min(right))
            .all(|x| buffer.cell((x, row)).is_some_and(|cell| cell.symbol() == " "));
        if !blank {
            continue;
        }
        for (offset, ch) in label.chars().enumerate() {
            if let Some(cell) = buffer.cell_mut((start + offset as u16, row)) {
                cell.set_char(ch);
            }
        }
        placed.push(date);
    }
    placed
}

fn render_metric_chart_text(
    frame: &mut Frame,
    records: &[crate::record::SessionRecord],
    area: Rect,
    kind: MetricKind,
    lang: Lang,
    spec: Option<TextTrendData>,
    axis: Option<TextTrendAxis<'_>>,
) {
    let title = match kind {
        MetricKind::NetWpm => Msg::TrendNetWpm.text(lang),
        MetricKind::Accuracy => Msg::TrendAccuracy.text(lang),
        MetricKind::Iki => Msg::TrendIki.text(lang),
    };
    let (Some(spec), Some(axis)) = (spec, axis) else {
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
    // Each series is split into runs that do not cross a break, so that no
    // line is drawn over a cut-out gap.
    let runs = spec
        .series
        .iter()
        .enumerate()
        .flat_map(|(index, series)| {
            let mut runs = Vec::<(usize, Vec<(f64, f64)>)>::new();
            let mut previous = None;
            for (date, value) in &series.points {
                let point = (axis.x(*date), *value);
                match (runs.last_mut(), previous) {
                    (Some((_, run)), Some(previous)) if !axis.crosses_break(previous, *date) => run.push(point),
                    _ => runs.push((index, vec![point])),
                }
                previous = Some(*date);
            }
            runs
        })
        .collect::<Vec<_>>();
    let mut named = BTreeSet::new();
    let raw_runs = spec
        .series
        .iter()
        .enumerate()
        .map(|(index, series)| {
            (
                index,
                series.raw_points.iter().map(|(date, value)| (axis.x(*date), *value)).collect::<Vec<_>>(),
            )
        })
        .filter(|(_, values)| !values.is_empty())
        .collect::<Vec<_>>();
    let mut datasets = raw_runs
        .iter()
        .map(|(index, values)| {
            Dataset::default()
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Scatter)
                .style(Style::default().fg(spec.series[*index].color).add_modifier(Modifier::DIM))
                .data(values)
        })
        .collect::<Vec<_>>();
    datasets.extend(runs.iter().map(|(index, values)| {
        let dataset = Dataset::default()
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(spec.series[*index].color))
            .data(values);
        if named.insert(*index) {
            dataset.name(spec.series[*index].name.clone())
        } else {
            dataset
        }
    }));
    let values = runs
        .iter()
        .chain(&raw_runs)
        .flat_map(|(_, run)| run)
        .map(|(_, value)| *value)
        .collect::<Vec<_>>();
    let min_y = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max_y = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let padding = if (max_y - min_y).abs() < f64::EPSILON {
        (max_y.abs() * 0.05).max(1.0)
    } else {
        0.0
    };
    let first_date = trend_date_label(axis.axis.start(), true);
    let last_date = if axis.unknown {
        "unknown".to_owned()
    } else {
        trend_date_label(axis.axis.end(), axis.axis.end().year() != axis.axis.start().year())
    };
    let upper = axis.upper_bound();
    let chart = Chart::new(datasets)
        .block(Block::default().title(title.to_owned()))
        .x_axis(
            Axis::default()
                .title(Msg::TrendDay.text(lang))
                .bounds([0.0, upper])
                .labels(vec![Span::raw(first_date), Span::raw(last_date)]),
        )
        .y_axis(
            Axis::default()
                .bounds([min_y - padding, max_y + padding])
                .labels(vec![Span::raw(format!("{min_y:.1}")), Span::raw(format!("{max_y:.1}"))]),
        );
    frame.render_widget(chart, area);
    // The chart widget places its graph area itself; find it from the corner
    // of the two axes it drew — a `└` followed by `─` up to the right edge
    // (the legend box has a `└` too, but closes with `┘`).
    let buffer = frame.buffer_mut();
    let symbol_at = |x: u16, y: u16| buffer.cell((x, y)).map_or("", |cell| cell.symbol());
    let corner = (area.top()..area.bottom()).rev().find_map(|y| {
        (area.left()..area.right())
            .find(|&x| {
                symbol_at(x, y) == symbols::line::BOTTOM_LEFT && (x + 1..area.right()).all(|right| symbol_at(right, y) == symbols::line::HORIZONTAL)
            })
            .map(|x| (x, y))
    });
    let Some((corner_x, axis_row)) = corner else {
        return;
    };
    let graph_left = corner_x + 1;
    let graph_width = area.right().saturating_sub(graph_left);
    if graph_width < 2 {
        return;
    }
    // Same rounding as the braille canvas (two dots a cell).
    let column_of = |fraction: f64| {
        let dots = (fraction.clamp(0.0, 1.0) * f64::from(graph_width * 2 - 1)).round() as u16;
        graph_left + dots / 2
    };
    let label_row = (axis_row + 1 < area.bottom()).then_some(axis_row + 1);
    if axis.axis.has_breaks() {
        overlay_axis_breaks(frame, axis.axis, |units| units / upper, column_of, axis_row, label_row, area.right());
    }
    // Tick dates between the endpoints, each under its own column.
    if let Some(label_row) = label_row {
        let candidates = trend_tick_candidates(axis.axis, f64::from(graph_width) / upper);
        place_tick_labels(
            frame.buffer_mut(),
            &candidates,
            |date| column_of(axis.x(Some(date)) / upper),
            label_row,
            area.left(),
            area.right(),
        );
    }
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

fn render_metric_chart_image(frame: &mut Frame, app: &App, area: Rect, lang: Lang, spec: &mut TrendChartSpec, x_range: &TrendAxis) -> bool {
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
    let column_of = |fraction: f64| {
        let column = (fraction.clamp(0.0, 1.0) * f64::from(plot_area.width)).floor() as u16;
        plot_area.x + column.min(plot_area.width - 1)
    };
    let (start, end) = (x_range.start(), x_range.end());
    frame.render_widget(Paragraph::new(trend_date_label(start, true)), label_area);
    frame.render_widget(
        Paragraph::new(trend_date_label(end, end.year() != start.year())).alignment(Alignment::Right),
        label_area,
    );
    if x_range.has_breaks() {
        overlay_axis_breaks(
            frame,
            x_range,
            |units| units / x_range.span().max(1.0),
            column_of,
            label_area.y,
            Some(label_area.y),
            label_area.right(),
        );
    }
    // Tick dates between the endpoints; the image draws a vertical grid line
    // at each tick that got a label.
    let candidates = trend_tick_candidates(x_range, f64::from(plot_area.width) / x_range.span().max(1.0));
    spec.x_ticks = place_tick_labels(
        frame.buffer_mut(),
        &candidates,
        |date| column_of(x_range.fraction(date)),
        label_area.y,
        label_area.x,
        label_area.right(),
    );
    // Without ticks the centred «day» caption stays, where the row is blank.
    if spec.x_ticks.is_empty() {
        let day = Msg::TrendDay.text(lang);
        let day_width = day.width() as u16;
        let day_start = label_area.x + label_area.width.saturating_sub(day_width) / 2;
        let buffer = frame.buffer_mut();
        let blank = (day_start.saturating_sub(1)..(day_start + day_width + 1).min(label_area.right()))
            .all(|x| buffer.cell((x, label_area.y)).is_some_and(|cell| cell.symbol() == " "));
        if blank {
            frame.render_widget(
                Paragraph::new(day),
                Rect::new(day_start, label_area.y, day_width.min(label_area.width), 1),
            );
        }
    }
    let mut images = app.analysis_images.borrow_mut();
    if images.prepare_chart_with_range(&app.graphics, spec, x_range.clone()).is_err() {
        // The text chart is drawn over the same area instead.
        frame.render_widget(Clear, area);
        return false;
    }
    let Some(protocol) = images.chart_protocol_with_range(&app.graphics, spec, x_range.clone()) else {
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
    if app
        .analysis_records
        .iter()
        .any(|record| record.header.bench_id.as_deref().and_then(crate::bench::parse_v3_bench_id).is_some())
    {
        let title = match lang {
            Lang::En => "bench-v3 / accuracy-adjusted speed",
            Lang::Ja => "bench-v3／正確さを含む速さ",
        };
        let (lines, charts, axes_row) = bench_v3_content(app, lang, area.width >= 100);
        frame.render_widget(Paragraph::new(lines).block(Block::default().title(title)), area);
        render_bench_v3_images(frame, app, area, &charts, axes_row);
        return;
    }
    let title = legacy_bench_title(app, lang);
    let invitation = match lang {
        Lang::En => "No bench-v3 benchmark yet. Measure it with [B] on the title screen (about 5 minutes)",
        Lang::Ja => "v3 のベンチはまだありません．タイトルの [B] で測れます（約 5 分）",
    };
    let lines = bench_text_lines(app, lang);
    if render_bench_images(frame, app, area, &lines, &title, invitation) {
        return;
    }
    let block = Block::default().title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(inner);
    frame.render_widget(Paragraph::new(invitation), chunks[0]);
    frame.render_widget(Paragraph::new(lines), chunks[1]);
}

fn legacy_bench_title(app: &App, lang: Lang) -> String {
    let series = if app.analysis_records.iter().any(|record| {
        record
            .header
            .bench_id
            .as_deref()
            .and_then(parse_bench_id)
            .is_some_and(|(series, _, _)| series == BENCH_SERIES_V2)
    }) {
        BENCH_SERIES_V2
    } else {
        BENCH_SERIES_V1
    };
    match lang {
        Lang::En => format!("{series} / 8 skill axes"),
        Lang::Ja => format!("{series}／技能の 8 軸"),
    }
}

type BenchV3Chart = (usize, Vec<f64>);
type BenchV3Content = (Vec<Line<'static>>, Vec<BenchV3Chart>, usize);

fn bench_v3_content(app: &App, lang: Lang, expanded: bool) -> BenchV3Content {
    let mut lines = vec![Line::from(match lang {
        Lang::En => "Parts (effective WPM, attempts in order)",
        Lang::Ja => "各パート（正確さを含む WPM，試行順）",
    })];
    let mut charts = Vec::new();
    for part in [BenchV3Part::EW, BenchV3Part::ES, BenchV3Part::JW, BenchV3Part::JS, BenchV3Part::PW] {
        let values = bench_v3_part_values(app, part);
        if values.is_empty() {
            continue;
        }
        charts.push((lines.len(), values.clone()));
        lines.push(Line::from(format!(
            "{}: {} WPM  {}",
            crate::ui::bench_summary::v3_part_label(part, lang),
            bench_v3_first_latest(&values),
            bench_v3_text_figure(&values)
        )));
    }
    lines.push(Line::from(""));
    for group in [BenchV3Group::English, BenchV3Group::Japanese, BenchV3Group::Transition] {
        let rows = bench_v3_group_rows(app, group);
        if rows.is_empty() {
            continue;
        }
        let label = bench_v3_group_label(group, lang);
        let reliability = crate::analysis::bench_v3::compute_bench_v3_group_reliability(&app.analysis_records, group, app.config.analysis.pause_ms);
        let complete_runs = crate::analysis::bench_v3::compute_bench_v3_group_runs(&app.analysis_records, group, app.config.analysis.pause_ms).len();
        let speeds = rows.iter().filter_map(|(metrics, _)| metrics.effective_wpm).collect::<Vec<_>>();
        if speeds.is_empty() {
            continue;
        }
        let change = speeds
            .windows(2)
            .last()
            .map(|pair| bench_v3_change_label(pair[1] - pair[0], reliability.map(|value| value.mdc95_wpm), lang));
        let (latest, partial) = rows.last().expect("non-empty group rows");
        let recent = crate::analysis::bench_v3::rolling_median_3(&speeds).last().copied();
        charts.push((lines.len(), speeds.clone()));
        let partial = match (*partial, lang) {
            (true, Lang::En) => " (partial parts)",
            (true, Lang::Ja) => "（一部のパートのみ）",
            (false, _) => "",
        };
        let change = change.map_or_else(String::new, |value| format!(" · {value}"));
        let tempo = latest.tempo_wpm.map_or_else(|| "—".to_owned(), |value| format!("{value:.1} WPM"));
        if expanded {
            lines.push(Line::from(format!(
                "{label}: {} WPM{partial}{change}  {}",
                bench_v3_first_latest(&speeds),
                bench_v3_text_figure(&speeds)
            )));
            lines.push(Line::from(format!(
                "  {} {tempo} · {} · {}",
                bench_v3_tempo_label(lang),
                bench_v3_accuracy_label(latest.accuracy, lang),
                bench_v3_recent_median_label(recent, lang),
            )));
        } else {
            lines.push(Line::from(format!(
                "{label}: {} WPM{partial}{change} · {} {tempo} · {} · {}  {}",
                bench_v3_first_latest(&speeds),
                bench_v3_tempo_label(lang),
                bench_v3_accuracy_label(latest.accuracy, lang),
                bench_v3_recent_median_label(recent, lang),
                bench_v3_text_figure(&speeds)
            )));
        }
        lines.push(Line::from(bench_v3_reliability_line(label, reliability, complete_runs, lang)));
    }

    let previous = bench_v2_compact_lines(app);
    if !previous.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(match lang {
            Lang::En => "Previous series (v2)",
            Lang::Ja => "以前の系列（v2）",
        }));
        lines.extend(previous.into_iter().map(Line::from));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(bench_v3_grade_line(app, lang)));
    let axes_row = lines.len();
    lines.push(Line::from(match lang {
        Lang::En => "8 skill axes (bench-v3)",
        Lang::Ja => "技能の 8 軸（bench-v3）",
    }));
    lines.extend(bench_v3_axis_lines(app, lang).into_iter().map(Line::from));
    (lines, charts, axes_row)
}

fn render_bench_v3_images(frame: &mut Frame, app: &App, area: Rect, charts: &[(usize, Vec<f64>)], axes_row: usize) {
    if app.graphics.mode == GraphicsMode::Off || app.graphics.cell_pixels.is_none() || area.width < 54 || charts.is_empty() {
        return;
    }
    let chart_width = (area.width / 3).clamp(18, 34);
    let specs = charts
        .iter()
        .enumerate()
        .filter_map(|(index, (row, values))| {
            let y = area.y.saturating_add(1).saturating_add(*row as u16);
            (y < area.y.saturating_add(area.height)).then(|| {
                let chart_area = Rect::new(area.x + area.width - chart_width, y, chart_width, 1);
                (
                    chart_area,
                    TrialChartSpec {
                        raw_values: values.clone(),
                        values: crate::analysis::bench_v3::rolling_median_3(values),
                        color: SERIES_COLORS[index % SERIES_COLORS.len()],
                        cells: chart_area.as_size(),
                    },
                )
            })
        })
        .collect::<Vec<_>>();
    let mut images = app.analysis_images.borrow_mut();
    if specs.iter().any(|(_, spec)| images.prepare_trial_chart(&app.graphics, spec).is_err()) {
        return;
    }
    let radar = (area.width >= 100).then(|| {
        let y = area.y.saturating_add(axes_row as u16 + 2);
        let available = area.y.saturating_add(area.height).saturating_sub(y);
        let radar_area = Rect::new(area.x + area.width.saturating_sub(42), y, 42, available.min(11));
        let axes = crate::analysis::summary::skill_axis_values(&app.analysis_records, app.practice_layout(), &app.config);
        let spec = RadarChartSpec {
            axes: axes
                .into_iter()
                .map(|value| RadarAxis {
                    label: value.axis.label(app.config.general.lang).to_owned(),
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
            color: Theme::from_name(&app.config.ui.theme).accent,
            cells: radar_area.as_size(),
            missing_label: match app.config.general.lang {
                Lang::En => "under 3",
                Lang::Ja => "3 回未満",
            }
            .to_owned(),
        };
        (radar_area, spec)
    });
    let font = app.bigtext_images.borrow_mut().resolved_font(&app.config.display.big_font, &app.home_dir);
    if let Some((radar_area, spec)) = radar.as_ref()
        && (radar_area.width == 0 || radar_area.height < 8 || images.prepare_radar(&app.graphics, &font, spec).is_err())
    {
        return;
    }
    for (chart_area, spec) in &specs {
        let Some(protocol) = images.trial_chart_protocol(&app.graphics, spec) else {
            return;
        };
        frame.render_widget(Image::new(protocol), *chart_area);
    }
    if let Some((radar_area, spec)) = radar {
        let Some(protocol) = images.radar_protocol(&app.graphics, &font, &spec) else {
            return;
        };
        frame.render_widget(Image::new(protocol), radar_area);
    }
}

fn bench_v3_part_values(app: &App, part: BenchV3Part) -> Vec<f64> {
    let mut records = app
        .analysis_records
        .iter()
        .filter(|record| {
            record
                .header
                .bench_id
                .as_deref()
                .and_then(crate::bench::parse_v3_bench_id)
                .is_some_and(|(candidate, _)| candidate == part)
        })
        .filter_map(|record| {
            let finished = chrono::DateTime::parse_from_rfc3339(&record.header.finished_at)
                .ok()?
                .with_timezone(&chrono::Utc);
            Some(((finished, record.header.id.unwrap_or(i64::MIN)), record))
        })
        .collect::<Vec<_>>();
    records.sort_by_key(|(order, _)| *order);
    records
        .into_iter()
        .filter_map(|(_, record)| {
            crate::analysis::bench_v3::compute_bench_v3_metrics(std::slice::from_ref(record), app.config.analysis.pause_ms).effective_wpm
        })
        .collect()
}

fn bench_v3_group_rows(app: &App, group: BenchV3Group) -> Vec<(BenchV3Metrics, bool)> {
    let mut runs = BTreeMap::<(NaiveDate, u8), Vec<crate::record::SessionRecord>>::new();
    for record in &app.analysis_records {
        let Some((part, _)) = record.header.bench_id.as_deref().and_then(crate::bench::parse_v3_bench_id) else {
            continue;
        };
        if !group.parts().contains(&part) {
            continue;
        }
        let Some((date, form)) = bench_v3_run_key(record) else {
            continue;
        };
        runs.entry((date, form)).or_default().push(record.clone());
    }
    let mut values = runs
        .into_values()
        .filter_map(|records| {
            let order = records.iter().filter_map(bench_v3_record_order).max()?;
            let summary = crate::analysis::bench_v3::compute_bench_v3_group_metrics(&records, group, app.config.analysis.pause_ms)?;
            Some((order, summary.metrics, summary.partial))
        })
        .collect::<Vec<_>>();
    values.sort_by_key(|(order, _, _)| *order);
    values.into_iter().map(|(_, value, partial)| (value, partial)).collect()
}

fn bench_v3_tempo_label(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Tempo",
        Lang::Ja => "テンポ",
    }
}

fn bench_v3_accuracy_label(accuracy: f64, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Accuracy {:.1}%", accuracy * 100.0),
        Lang::Ja => format!("正確さ {:.1}%", accuracy * 100.0),
    }
}

fn bench_v3_recent_median_label(value: Option<f64>, lang: Lang) -> String {
    let value = value.map_or_else(|| "—".to_owned(), |value| format!("{value:.1} WPM"));
    match lang {
        Lang::En => format!("Recent-3 median {value}"),
        Lang::Ja => format!("直近 3 回の中央値 {value}"),
    }
}

pub(crate) fn bench_v3_grade_line(app: &App, lang: Lang) -> String {
    use crate::analysis::grades::{Component, ComponentValue, GRADE_VERSION_V2, GradeSource, GradeStatus};

    let labels = [("en", BenchV3Group::English), ("ja", BenchV3Group::Japanese)]
        .into_iter()
        .filter_map(|(language, group)| {
            let complete = crate::analysis::bench_v3::compute_bench_v3_group_runs(&app.analysis_records, group, app.config.analysis.pause_ms);
            let selected = complete.iter().rev().take(3).collect::<Vec<_>>();
            if selected.is_empty() {
                return None;
            }
            let fallback = || {
                let speeds = selected.iter().filter_map(|run| run.metrics.effective_wpm).collect::<Vec<_>>();
                let accuracies = selected.iter().map(|run| run.metrics.accuracy * 100.0).collect::<Vec<_>>();
                crate::analysis::grades::evaluate_language_with_version(
                    language,
                    &[
                        ComponentValue::new(Component::Speed, median(&speeds)),
                        ComponentValue::new(Component::Accuracy, median(&accuracies)),
                    ],
                    GradeSource::Bench,
                    if selected.len() >= 3 {
                        GradeStatus::Official
                    } else {
                        GradeStatus::Interim
                    },
                    selected.len(),
                    GRADE_VERSION_V2,
                )
                .ok()
            };
            let grade = app
                .analysis_coach
                .as_ref()
                .and_then(|report| {
                    report
                        .grades
                        .languages
                        .iter()
                        .find(|grade| grade.language == language && grade.version == GRADE_VERSION_V2)
                })
                .cloned()
                .or_else(fallback)?;
            let overall = grade.overall?;
            let language = match (language, lang) {
                ("en", Lang::En) => "English",
                ("ja", Lang::En) => "Japanese",
                ("en", Lang::Ja) => "英語",
                ("ja", Lang::Ja) => "日本語",
                _ => language,
            };
            Some(format!("{language} {} (n={})", overall.grade.as_str(), grade.n_sessions))
        })
        .collect::<Vec<_>>();
    let heading = match lang {
        Lang::En => "Grades (grades-v2)",
        Lang::Ja => "段階（grades-v2）",
    };
    if labels.is_empty() {
        format!("{heading}: —")
    } else {
        format!("{heading}: {}", labels.join(" · "))
    }
}

fn bench_v3_axis_lines(app: &App, lang: Lang) -> Vec<String> {
    crate::analysis::summary::skill_axis_values(&app.analysis_records, app.practice_layout(), &app.config)
        .into_iter()
        .map(|value| {
            let name = pad_display(value.axis.label(lang), 16);
            let Some(raw) = value.raw else {
                return format!("{name}{}", Msg::NoData.text(lang));
            };
            let suffix = value.percentile.map_or_else(
                || Msg::RawFewerThree.text(lang).to_owned(),
                |position| {
                    format!(
                        "{position:.1} {}",
                        match lang {
                            Lang::En => Msg::Percentile.text(lang),
                            Lang::Ja => "百分位",
                        }
                    )
                },
            );
            format!("{name}{raw:>10.2}  {suffix}")
        })
        .collect()
}

fn bench_v3_first_latest(values: &[f64]) -> String {
    match values {
        [] => String::new(),
        [value] => format!("{value:.1}"),
        [first, .., latest] => format!("{first:.1} → {latest:.1}"),
    }
}

fn bench_v3_text_figure(values: &[f64]) -> String {
    const LEVELS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let rolling = crate::analysis::bench_v3::rolling_median_3(values);
    let finite = values
        .iter()
        .chain(&rolling)
        .copied()
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    if finite.is_empty() {
        return String::new();
    }
    let min = finite.iter().copied().fold(f64::INFINITY, f64::min);
    let max = finite.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let spark = |series: &[f64]| {
        series
            .iter()
            .map(|value| {
                let fraction = if max > min { (*value - min) / (max - min) } else { 0.5 };
                LEVELS[(fraction.clamp(0.0, 1.0) * (LEVELS.len() - 1) as f64).round() as usize]
            })
            .collect::<String>()
    };
    format!("·{} ━{}", spark(values), spark(&rolling))
}

fn bench_v3_group_label(group: BenchV3Group, lang: Lang) -> &'static str {
    match (group, lang) {
        (BenchV3Group::English, Lang::En) => "English",
        (BenchV3Group::Japanese, Lang::En) => "Japanese",
        (BenchV3Group::Transition, Lang::En) => "Transitions",
        (BenchV3Group::English, Lang::Ja) => "英語",
        (BenchV3Group::Japanese, Lang::Ja) => "日本語",
        (BenchV3Group::Transition, Lang::Ja) => "移り変わり",
    }
}

fn bench_v3_change_label(value: f64, mdc95_wpm: Option<f64>, lang: Lang) -> String {
    use crate::analysis::bench_v3::{BenchV3Change, classify_bench_v3_change};

    match classify_bench_v3_change(value, mdc95_wpm) {
        BenchV3Change::DifferenceOnly => format!("{value:+.1} WPM"),
        BenchV3Change::WithinError => match lang {
            Lang::En => "no change (within measurement error)".to_owned(),
            Lang::Ja => "変化なし（誤差の範囲）".to_owned(),
        },
        BenchV3Change::Increase => format!("▲ {value:+.1} WPM"),
        BenchV3Change::Decrease => format!("▼ {value:+.1} WPM"),
    }
}

fn bench_v3_reliability_line(
    label: &str,
    reliability: Option<crate::analysis::bench_v3::BenchV3Reliability>,
    complete_runs: usize,
    lang: Lang,
) -> String {
    match (reliability, lang) {
        (Some(value), Lang::En) => format!(
            "{label}: SEM {:.1} WPM · MDC95 {:.1} WPM · n={}{}",
            value.sem_wpm,
            value.mdc95_wpm,
            value.sample_count,
            if value.reference_only { " (reference)" } else { "" }
        ),
        (Some(value), Lang::Ja) => format!(
            "{label}: SEM {:.1} WPM・MDC95 {:.1} WPM・n={}{}",
            value.sem_wpm,
            value.mdc95_wpm,
            value.sample_count,
            if value.reference_only { "（参考）" } else { "" }
        ),
        (None, Lang::En) => format!("{label}: SEM / MDC95 available after 2 complete runs · n={complete_runs}"),
        (None, Lang::Ja) => format!("{label}: SEM／MDC95 は完了した 2 回分から表示・n={complete_runs}"),
    }
}

fn bench_v2_compact_lines(app: &App) -> Vec<String> {
    [BenchPart::A, BenchPart::B, BenchPart::C, BenchPart::D]
        .into_iter()
        .filter_map(|part| {
            let mut records = app
                .analysis_records
                .iter()
                .filter(|record| {
                    record
                        .header
                        .bench_id
                        .as_deref()
                        .and_then(parse_bench_id)
                        .is_some_and(|(series, candidate, _)| series == BENCH_SERIES_V2 && candidate == part)
                })
                .filter_map(|record| {
                    let finished = chrono::DateTime::parse_from_rfc3339(&record.header.finished_at)
                        .ok()?
                        .with_timezone(&chrono::Utc);
                    Some(((finished, record.header.id.unwrap_or(i64::MIN)), record.header.net_wpm))
                })
                .collect::<Vec<_>>();
            records.sort_by_key(|(order, _)| *order);
            let values = records.into_iter().map(|(_, value)| value).collect::<Vec<_>>();
            (!values.is_empty()).then(|| format!("{}: {} WPM", part.as_str(), bench_v3_first_latest(&values)))
        })
        .collect()
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

fn render_bench_images(frame: &mut Frame, app: &App, area: Rect, text_lines: &[Line<'static>], title: &str, invitation: &str) -> bool {
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
    let block = Block::default().title(title);
    let inner = block.inner(area);
    let outer_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(inner);
    let inner = outer_chunks[1];
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
    frame.render_widget(Paragraph::new(invitation), outer_chunks[0]);
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
                .is_some_and(|(candidate_series, candidate_part, _)| {
                    candidate_part == part && is_preferred_bench_series(candidate_series, part, series)
                })
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

// ---------------------------------------------------------------------------
// «Skills» tab (design §8.4)
// ---------------------------------------------------------------------------

/// The language whose series the skills tab shows: the `L` selection, else
/// the display language, else the first language with records.
pub fn skills_language(app: &App) -> Option<String> {
    let languages = app.analysis_skills.languages();
    if let Some(selected) = app.analysis_skills_lang.as_ref().filter(|selected| languages.contains(selected)) {
        return Some(selected.clone());
    }
    let display = match app.config.general.lang {
        Lang::En => "en",
        Lang::Ja => "ja",
    };
    languages.iter().find(|language| *language == display).or(languages.first()).cloned()
}

struct SkillRow {
    component: crate::analysis::grades::Component,
    points: Vec<TrendPoint>,
    /// Median of the latest runs up to each point (up to
    /// [`MOVING_MEDIAN_WINDOW`](crate::practice::menu::skills::MOVING_MEDIAN_WINDOW)
    /// runs, so the line starts at the first run).
    medians: Vec<TrendPoint>,
    bench: Vec<TrendPoint>,
    /// Grade boundaries in score order (see [`SkillChartSpec::boundaries`]).
    boundaries: Vec<f64>,
    latest: Option<crate::practice::menu::skills::SeriesPointFigures>,
    unit: String,
    /// Whether a lower raw value is better. The chart values (points,
    /// medians, bench, boundaries) of such a component are negated so that
    /// «up» is «better» in every chart; [`SkillRow::raw`] undoes it.
    lower_is_better: bool,
}

const SKILL_LEVELS: [crate::analysis::grades::Level; 8] = {
    use crate::analysis::grades::Level;
    [Level::G, Level::F, Level::E, Level::D, Level::C, Level::B, Level::A, Level::S]
};

impl SkillRow {
    /// The raw value of a chart value.
    fn raw(&self, value: f64) -> f64 {
        if self.lower_is_better { -value } else { value }
    }

    /// The grade of the latest run (the grade shown next to the name).
    fn grade(&self, language: &str) -> Option<crate::analysis::grades::ComponentGrade> {
        let latest = self.latest.as_ref()?;
        crate::analysis::grades::grade_component(language, self.component, latest.raw_value).ok()
    }

    /// Index (G = 0 … S = 7) of the latest run's band.
    fn current_band(&self, language: &str) -> Option<usize> {
        let grade = self.grade(language)?;
        SKILL_LEVELS.iter().position(|level| *level == grade.grade)
    }

    /// The y range: every value drawn, plus the current band's edges (the
    /// upper edge is the next grade's threshold, so the target is in view).
    fn y_range(&self, language: &str) -> Option<(f64, f64)> {
        let mut values = self
            .points
            .iter()
            .chain(&self.medians)
            .chain(&self.bench)
            .map(|point| point.value)
            .filter(|value| value.is_finite())
            .collect::<Vec<_>>();
        if values.is_empty() {
            return None;
        }
        if let Some(band) = self.current_band(language) {
            values.extend(self.boundaries.get(band).copied());
            if band + 1 < SKILL_LEVELS.len() {
                values.extend(self.boundaries.get(band + 1).copied());
            }
        }
        let min = values.iter().copied().fold(f64::INFINITY, f64::min);
        let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if !min.is_finite() || !max.is_finite() {
            return None;
        }
        let padding = if (max - min).abs() < f64::EPSILON {
            (max.abs() * 0.05).max(0.01)
        } else {
            (max - min) * 0.08
        };
        Some((min - padding, max + padding))
    }
}

/// Median of the latest `MOVING_MEDIAN_WINDOW` values up to each index (the
/// same median as the stored moving median, which needs five runs; here the
/// line starts at the first run).
fn running_medians(values: &[f64]) -> Vec<f64> {
    use crate::practice::menu::skills::MOVING_MEDIAN_WINDOW;
    (0..values.len())
        .map(|index| {
            let mut window = values[(index + 1).saturating_sub(MOVING_MEDIAN_WINDOW)..=index].to_vec();
            window.sort_by(f64::total_cmp);
            let middle = window.len() / 2;
            if window.len().is_multiple_of(2) {
                (window[middle - 1] + window[middle]) / 2.0
            } else {
                window[middle]
            }
        })
        .collect()
}

fn skill_rows(app: &App, language: &str, since: Option<chrono::NaiveDate>) -> Vec<SkillRow> {
    use crate::practice::menu::skills::{component_direction_and_unit, series_figures};

    let in_period = |date: chrono::NaiveDate| since.is_none_or(|since| date >= since);
    crate::analysis::grades::Component::all()
        .iter()
        .copied()
        .map(|component| {
            let (direction, unit) = component_direction_and_unit(language, component);
            let lower_is_better = direction == crate::analysis::grades::Direction::Lower;
            let sign = if lower_is_better { -1.0 } else { 1.0 };
            // Figures over the whole series, then cut to the period, so the
            // median at the start of a period still uses earlier runs.
            let all = series_figures(&app.analysis_skills.snapshots, language, component, direction);
            let medians = running_medians(&all.iter().map(|point| point.raw_value).collect::<Vec<_>>());
            let (figures, medians): (Vec<_>, Vec<_>) = all.into_iter().zip(medians).filter(|(point, _)| in_period(point.date)).unzip();
            SkillRow {
                component,
                points: figures
                    .iter()
                    .map(|point| TrendPoint {
                        date: point.date,
                        value: sign * point.raw_value,
                    })
                    .collect(),
                medians: figures
                    .iter()
                    .zip(&medians)
                    .map(|(point, median)| TrendPoint {
                        date: point.date,
                        value: sign * median,
                    })
                    .collect(),
                bench: app
                    .analysis_skills
                    .bench
                    .get(&(language.to_owned(), component))
                    .into_iter()
                    .flatten()
                    .filter(|(date, _)| in_period(*date))
                    .map(|(date, value)| TrendPoint {
                        date: *date,
                        value: sign * value,
                    })
                    .collect(),
                boundaries: crate::analysis::grades::component_score_points(language, component)
                    .map(|(_, points)| points.iter().map(|point| sign * point).collect())
                    .unwrap_or_default(),
                latest: figures.last().cloned(),
                unit,
                lower_is_better,
            }
        })
        .collect()
}

/// Width of the band-name column right of each skill chart.
const SKILL_BAND_GUTTER: u16 = 8;

/// The skills tab's x axis and the mapping from a fraction of the axis to a
/// terminal column of the plot (the image's cells, or the text chart's
/// braille dots).
struct SkillXAxis {
    axis: TrendAxis,
    left: u16,
    width: u16,
    image: bool,
}

impl SkillXAxis {
    fn fraction(&self, date: NaiveDate) -> f64 {
        crate::ui::analysis_image::skill_axis_fraction(&self.axis, date)
    }

    fn column_of(&self, fraction: f64) -> u16 {
        let fraction = fraction.clamp(0.0, 1.0);
        if self.image {
            self.left + ((fraction * f64::from(self.width)).floor() as u16).min(self.width.saturating_sub(1))
        } else {
            let dots = (fraction * f64::from((self.width * 2).saturating_sub(1))).round() as u16;
            self.left + dots / 2
        }
    }

    /// Fraction of the axis at the centre of a plot column.
    fn column_fraction(&self, column: u16) -> f64 {
        if self.image {
            (f64::from(column) + 0.5) / f64::from(self.width.max(1))
        } else {
            f64::from(column * 2) / f64::from((self.width * 2).saturating_sub(1).max(1))
        }
    }

    fn right(&self) -> u16 {
        self.left + self.width
    }
}

fn render_skills(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let lang = app.config.general.lang;
    let Some(language) = skills_language(app) else {
        let message = match lang {
            Lang::En => "No skill records yet. Shown after finishing the check step of a practice menu.",
            Lang::Ja => "スキルの記録はまだありません．メニューの確認段を終えると表示します．",
        };
        frame.render_widget(
            Paragraph::new(message)
                .alignment(Alignment::Center)
                .style(Style::default().fg(theme.muted)),
            Rect::new(area.x, area.y + area.height / 2, area.width, 1.min(area.height)),
        );
        return;
    };
    let today = app.analysis_day();
    let since = app.analysis_period.start_date(today);
    let rows = skill_rows(app, &language, since);
    let history = app
        .analysis_skills
        .prescriptions
        .get(&language)
        .into_iter()
        .flatten()
        .filter(|day| since.is_none_or(|since| day.date >= since))
        .cloned()
        .collect::<Vec<_>>();
    let image_enabled = app.graphics.mode != GraphicsMode::Off && app.graphics.cell_pixels.is_some();
    let bench_mark = if image_enabled { "◆" } else { "█" };
    let (runs, median, bench, bands) = match lang {
        Lang::En => ("each run", "median of last 7 runs", "bench", "bands G–S"),
        Lang::Ja => ("各回", "直近 7 回の中央値", "ベンチ", "帯: 段 G〜S"),
    };
    let header = Line::from(vec![
        Span::styled(
            format!("{} — ", language_label(&language, lang)),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(if image_enabled { "● " } else { "• " }, Style::default().fg(theme.fg)),
        Span::raw(format!("{runs}  ")),
        Span::styled("━ ", Style::default().fg(theme.accent)),
        Span::raw(format!("{median}  ")),
        Span::styled(format!("{bench_mark} "), Style::default().fg(theme.warning)),
        Span::raw(format!("{bench}  ")),
        Span::raw(bands),
    ]);
    frame.render_widget(Paragraph::new(header), Rect::new(area.x, area.y, area.width, 1.min(area.height)));
    // Only the days with a record in the period: the period limits which
    // records are used, it does not stretch the axis to its own start.
    let dates = rows
        .iter()
        .flat_map(|row| row.points.iter().chain(&row.bench).map(|point| point.date))
        .chain(history.iter().map(|day| day.date))
        .collect::<BTreeSet<_>>();
    let Some(axis) = TrendAxis::from_dates(dates) else {
        if area.height > 1 {
            frame.render_widget(
                Paragraph::new(Msg::NoData.text(lang)).style(Style::default().fg(theme.fg)),
                Rect::new(area.x, area.y + 1, area.width, 1),
            );
        }
        return;
    };

    let gutter = if area.width >= 50 { SKILL_BAND_GUTTER } else { 0 };
    let label_width = 24_u16.min(area.width / 3);
    let plot_width = area.width.saturating_sub(label_width + gutter);
    let x_axis = SkillXAxis {
        axis,
        left: area.x + label_width,
        width: plot_width,
        image: image_enabled,
    };

    // Vertical budget: the header, then the prescription band (2 rows) and
    // the date row when they fit, one row for each component without a
    // record, and the rest shared by the components with records.
    let bottom = area.bottom();
    let mut y = area.y + 1;
    let available = bottom.saturating_sub(y);
    let empty = rows.iter().filter(|row| row.latest.is_none()).count() as u16;
    let filled = rows.len() as u16 - empty;
    let show_dates = available > rows.len() as u16;
    let show_band = available >= rows.len() as u16 + 2 + u16::from(show_dates) + filled;
    let chart_rows = available.saturating_sub(u16::from(show_dates) + if show_band { 2 } else { 0 } + empty);

    let mut x_ticks = Vec::new();
    if show_dates && plot_width > 2 {
        x_ticks = render_skill_dates(frame, &x_axis, lang, theme, area.x, label_width, bottom - 1);
    }
    if show_band && plot_width > 0 {
        render_prescription_band(frame, app, theme, &history, &x_axis, area, label_width, y);
        y += 2;
    }
    let rows_bottom = if show_dates { bottom - 1 } else { bottom };
    let mut filled_index = 0_u16;
    for row in &rows {
        if y >= rows_bottom {
            break;
        }
        let height = if row.latest.is_none() {
            1
        } else {
            let share = chart_rows / filled.max(1) + u16::from(filled_index < chart_rows % filled.max(1));
            filled_index += 1;
            share.max(1)
        }
        .min(rows_bottom - y);
        render_skill_label(frame, row, &language, lang, theme, Rect::new(area.x, y, label_width, height));
        let plot_area = Rect::new(x_axis.left, y, plot_width, height);
        if let Some(y_range) = row.y_range(&language)
            && plot_area.width > 2
        {
            let drawn = image_enabled && render_skill_chart_image(frame, app, row, &language, &x_axis, &x_ticks, y_range, theme, plot_area);
            if !drawn {
                render_skill_chart_text(frame, row, &language, &x_axis.axis, y_range, theme, plot_area);
            }
            if gutter > 0 {
                render_skill_band_names(frame, row, &language, y_range, theme, Rect::new(x_axis.right(), y, gutter, height));
            }
        }
        y += height;
    }
}

/// The date row under the skill charts: the first day (`YYYY/MM/DD`) and the
/// last (`MM/DD`), tick dates between them where they fit, and `≈` at each
/// cut-out gap. Returns the tick dates that got a label (the image charts draw
/// a vertical grid line at each).
fn render_skill_dates(frame: &mut Frame, x_axis: &SkillXAxis, lang: Lang, theme: &Theme, x: u16, label_width: u16, row: u16) -> Vec<NaiveDate> {
    frame.render_widget(
        Paragraph::new(Msg::TrendDay.text(lang)).style(Style::default().fg(Color::Gray)),
        Rect::new(x, row, label_width, 1),
    );
    let axis = &x_axis.axis;
    let (start, end) = (axis.start(), axis.end());
    let place = |frame: &mut Frame, label: &str, centre: u16| {
        let width = label.width() as u16;
        let left = centre
            .saturating_sub(width / 2)
            .clamp(x_axis.left, x_axis.right().saturating_sub(width).max(x_axis.left));
        frame.render_widget(
            Paragraph::new(label.to_owned()).style(Style::default().fg(theme.fg)),
            Rect::new(left, row, width.min(x_axis.width), 1),
        );
    };
    place(frame, &trend_date_label(start, true), x_axis.column_of(x_axis.fraction(start)));
    if end > start {
        let label = trend_date_label(end, end.year() != start.year());
        let first_end = x_axis.left + trend_date_label(start, true).width() as u16;
        let centre = x_axis.column_of(x_axis.fraction(end));
        if centre.saturating_sub(label.width() as u16 / 2) > first_end {
            place(frame, &label, centre);
        }
    }
    let pad = crate::ui::analysis_image::skill_axis_padding(axis);
    let full = axis.span() + 2.0 * pad;
    if axis.has_breaks() {
        overlay_axis_breaks(
            frame,
            axis,
            |units| (units + pad) / full,
            |fraction| x_axis.column_of(fraction),
            row,
            Some(row),
            x_axis.right(),
        );
    }
    let candidates = trend_tick_candidates(axis, f64::from(x_axis.width) / full)
        .into_iter()
        .filter(|date| start < *date && *date < end)
        .collect::<Vec<_>>();
    let ticks = place_tick_labels(
        frame.buffer_mut(),
        &candidates,
        |date| x_axis.column_of(x_axis.fraction(date)),
        row,
        x_axis.left,
        x_axis.right(),
    );
    for &date in &ticks {
        let column = x_axis.column_of(x_axis.fraction(date));
        for offset in 0..TREND_TICK_LABEL_WIDTH {
            if let Some(cell) = frame
                .buffer_mut()
                .cell_mut((column.saturating_sub(TREND_TICK_LABEL_WIDTH / 2) + offset, row))
            {
                cell.set_fg(theme.fg);
            }
        }
    }
    ticks
}

fn truncate_text(value: &str, width: usize) -> String {
    if value.width() <= width {
        return value.to_owned();
    }
    let mut output = String::new();
    let mut used = 0;
    for character in value.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if used + character_width + 1 > width {
            break;
        }
        output.push(character);
        used += character_width;
    }
    output.push('…');
    output
}

/// A component value with its unit (`35.1 WPM`, `96.2%`, `0.123`).
fn skill_value_with_unit(component: crate::analysis::grades::Component, value: f64, signed: bool, unit: &str) -> String {
    let value = crate::practice::menu::skills::format_value(component, value, signed);
    match unit {
        "" => value,
        "%" => format!("{value}%"),
        unit => format!("{value} {unit}"),
    }
}

fn render_skill_label(frame: &mut Frame, row: &SkillRow, language: &str, lang: Lang, theme: &Theme, area: Rect) {
    let name = component_label(row.component, lang);
    let width = usize::from(area.width);
    let Some(latest) = &row.latest else {
        let none = match lang {
            Lang::En => "no records",
            Lang::Ja => "記録なし",
        };
        let line = Line::from(vec![
            Span::styled(format!("{name}  "), Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(none, Style::default().fg(Color::Gray)),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        return;
    };
    let grade = row.grade(language);
    let with_unit = |value: f64, signed: bool| skill_value_with_unit(row.component, value, signed, &row.unit);
    let mut spans = vec![Span::styled(format!("{name} "), Style::default().add_modifier(Modifier::BOLD))];
    if let Some(grade) = &grade {
        spans.push(Span::styled(
            format!("{} ", grade.grade.as_str()),
            Style::default().fg(grade_color(grade.grade, theme)).add_modifier(Modifier::BOLD),
        ));
    }
    spans.push(Span::styled(with_unit(latest.raw_value, false), Style::default().fg(theme.fg)));
    let next = match grade.as_ref().map(|grade| (grade.next_grade, grade.gap)) {
        Some((Some(next), Some(gap))) => match lang {
            Lang::En => format!("{} to {}", with_unit(gap, true), next.as_str()),
            Lang::Ja => format!("次の段 {} まで {}", next.as_str(), with_unit(gap, true)),
        },
        Some((None, _)) => match lang {
            Lang::En => "top grade".to_owned(),
            Lang::Ja => "最上位の段".to_owned(),
        },
        _ => String::new(),
    };
    let mut lines = vec![
        Line::from(spans),
        Line::from(Span::styled(truncate_text(&next, width), Style::default().fg(theme.fg))),
    ];
    if let Some(median) = row.medians.last() {
        let text = match lang {
            Lang::En => format!("median {}", with_unit(row.raw(median.value), false)),
            Lang::Ja => format!("中央値 {}", with_unit(row.raw(median.value), false)),
        };
        lines.push(Line::from(Span::styled(truncate_text(&text, width), Style::default().fg(Color::Gray))));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

/// Names of the grade bands in view, right of a skill chart, each on the row
/// of its band's middle: the grade and the value that enters it (`D≥41.0`,
/// or `D≤1.40` when lower is better — those charts are drawn upside down, so
/// the better band is always above). The current band is drawn in its grade
/// colour and placed first; the others are placed where a row is still free.
fn render_skill_band_names(frame: &mut Frame, row: &SkillRow, language: &str, y_range: (f64, f64), theme: &Theme, area: Rect) {
    let (min_y, max_y) = y_range;
    if area.height == 0 || max_y <= min_y || row.boundaries.len() < 2 {
        return;
    }
    let current = row.current_band(language);
    let mut bands = row
        .boundaries
        .windows(2)
        .enumerate()
        .take(SKILL_LEVELS.len())
        .filter_map(|(index, pair)| {
            let (low, high) = (pair[0].min(pair[1]).max(min_y), pair[0].max(pair[1]).min(max_y));
            (high > low).then_some((index, (low + high) / 2.0, high - low))
        })
        .collect::<Vec<_>>();
    bands.sort_by(|left, right| {
        (Some(right.0) == current)
            .cmp(&(Some(left.0) == current))
            .then(right.2.total_cmp(&left.2))
    });
    let mut used = vec![false; usize::from(area.height)];
    for (index, middle, _) in bands {
        let fraction = ((max_y - middle) / (max_y - min_y)).clamp(0.0, 1.0);
        let line = ((fraction * f64::from(area.height)).floor() as usize).min(used.len() - 1);
        if used[line] {
            continue;
        }
        used[line] = true;
        let level = SKILL_LEVELS[index];
        let label = if index == 0 {
            level.as_str().to_owned()
        } else {
            let edge = crate::practice::menu::skills::format_value(row.component, row.raw(row.boundaries[index]), false);
            format!("{}{}{edge}", level.as_str(), if row.lower_is_better { "≤" } else { "≥" })
        };
        let style = if Some(index) == current {
            Style::default().fg(grade_color(level, theme)).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        frame.render_widget(
            Paragraph::new(Span::styled(truncate_text(&label, usize::from(area.width - 1)), style)),
            Rect::new(area.x + 1, area.y + line as u16, area.width - 1, 1),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn render_skill_chart_image(
    frame: &mut Frame,
    app: &App,
    row: &SkillRow,
    language: &str,
    x_axis: &SkillXAxis,
    x_ticks: &[NaiveDate],
    y_range: (f64, f64),
    theme: &Theme,
    area: Rect,
) -> bool {
    let current_band = row.current_band(language);
    let spec = crate::ui::analysis_image::SkillChartSpec {
        cells: area.as_size(),
        x_axis: x_axis.axis.clone(),
        x_ticks: x_ticks.to_vec(),
        y_range,
        points: row.points.clone(),
        medians: row.medians.clone(),
        bench: row.bench.clone(),
        boundaries: row.boundaries.clone(),
        current_band,
        current_color: current_band.map_or(theme.accent, |band| grade_color(SKILL_LEVELS[band], theme)),
        point_color: theme.fg,
        line_color: theme.accent,
        bench_color: theme.warning,
    };
    let mut images = app.analysis_images.borrow_mut();
    if images.prepare_skill_chart(&app.graphics, &spec).is_err() {
        return false;
    }
    let Some(protocol) = images.skill_chart_protocol(&app.graphics, &spec) else {
        return false;
    };
    frame.render_widget(Image::new(protocol), area);
    true
}

fn render_skill_chart_text(frame: &mut Frame, row: &SkillRow, language: &str, axis: &TrendAxis, y_range: (f64, f64), theme: &Theme, area: Rect) {
    let pad = crate::ui::analysis_image::skill_axis_padding(axis);
    let (left, right) = (-pad, axis.span() + pad);
    let current = row.current_band(language);
    let current_edges = current
        .and_then(|band| Some((row.boundaries.get(band)?, row.boundaries.get(band + 1)?)))
        .map(|(low, high)| [*low, *high]);
    let boundaries = row
        .boundaries
        .iter()
        .copied()
        .filter(|value| (y_range.0..=y_range.1).contains(value))
        .map(|value| {
            let highlighted = current_edges.is_some_and(|edges| edges.contains(&value));
            (highlighted, vec![(left, value), (right, value)])
        })
        .collect::<Vec<_>>();
    // The median line, cut at the axis breaks.
    let mut runs = Vec::<Vec<(f64, f64)>>::new();
    let mut previous: Option<NaiveDate> = None;
    for point in &row.medians {
        let vertex = (axis.position(point.date), point.value);
        match (runs.last_mut(), previous) {
            (Some(run), Some(previous)) if !axis.crosses_break(previous, point.date) => run.push(vertex),
            _ => runs.push(vec![vertex]),
        }
        previous = Some(point.date);
    }
    let points = row
        .points
        .iter()
        .map(|point| (axis.position(point.date), point.value))
        .collect::<Vec<_>>();
    let bench = row.bench.iter().map(|point| (axis.position(point.date), point.value)).collect::<Vec<_>>();
    let current_color = current.map_or(theme.accent, |band| grade_color(SKILL_LEVELS[band], theme));
    let mut datasets = boundaries
        .iter()
        .map(|(highlighted, line)| {
            Dataset::default()
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(if *highlighted { current_color } else { theme.muted }))
                .data(line)
        })
        .collect::<Vec<_>>();
    for run in &runs {
        datasets.push(
            Dataset::default()
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
                .data(run),
        );
    }
    datasets.push(
        Dataset::default()
            .marker(symbols::Marker::Dot)
            .graph_type(GraphType::Scatter)
            .style(Style::default().fg(theme.fg))
            .data(&points),
    );
    datasets.push(
        Dataset::default()
            .marker(symbols::Marker::Block)
            .graph_type(GraphType::Scatter)
            .style(Style::default().fg(theme.warning))
            .data(&bench),
    );
    let chart = Chart::new(datasets)
        .x_axis(Axis::default().bounds([left, right]))
        .y_axis(Axis::default().bounds([y_range.0, y_range.1]));
    frame.render_widget(chart, area);
}

/// «Prescription history» band (§8.4): one cell per column of the plot, on
/// the same x axis as the charts. The upper half is prescription A, the lower
/// half prescription B; a menu day covers the days until the next menu day.
/// Reference only (no control, §5).
#[allow(clippy::too_many_arguments)]
fn render_prescription_band(
    frame: &mut Frame,
    app: &App,
    theme: &Theme,
    history: &[crate::practice::menu::skills::PrescriptionDay],
    x_axis: &SkillXAxis,
    area: Rect,
    label_width: u16,
    y: u16,
) {
    use crate::practice::menu::MenuRole;

    let lang = app.config.general.lang;
    let catalog = crate::practice::menu::bundled_catalog().ok();
    let mut order = Vec::<String>::new();
    for day in history {
        if !order.contains(&day.prescription_id) {
            order.push(day.prescription_id.clone());
        }
    }
    let color = |id: &str| SERIES_COLORS[order.iter().position(|candidate| candidate == id).unwrap_or(0) % SERIES_COLORS.len()];
    let menu_days = history.iter().map(|day| day.date).collect::<BTreeSet<_>>();
    let mut cells = Vec::new();
    for column in 0..x_axis.width {
        let date = crate::ui::analysis_image::skill_axis_date_at(&x_axis.axis, x_axis.column_fraction(column));
        let menu_day = date.and_then(|date| {
            menu_days
                .range(..=date)
                .next_back()
                .copied()
                .filter(|menu_day| *menu_day == date || menu_days.range(date..).next().is_some())
        });
        let find = |role: MenuRole| menu_day.and_then(|menu_day| history.iter().find(|day| day.date == menu_day && day.role == role));
        let (a, b) = (find(MenuRole::PrimaryA), find(MenuRole::PrimaryB));
        cells.push(match (a, b) {
            (Some(a), Some(b)) => Span::styled("▀", Style::default().fg(color(&a.prescription_id)).bg(color(&b.prescription_id))),
            (Some(a), None) => Span::styled("▀", Style::default().fg(color(&a.prescription_id))),
            (None, Some(b)) => Span::styled("▄", Style::default().fg(color(&b.prescription_id))),
            (None, None) => Span::raw(" "),
        });
    }
    let label = match lang {
        Lang::En => "Prescriptions (ref.)",
        Lang::Ja => "処方の履歴（参考）",
    };
    frame.render_widget(
        Paragraph::new(truncate_text(label, usize::from(label_width))).style(Style::default().fg(theme.fg)),
        Rect::new(area.x, y, label_width, 1),
    );
    frame.render_widget(Paragraph::new(Line::from(cells)), Rect::new(x_axis.left, y, x_axis.width, 1));
    let mut legend = vec![Span::styled(
        match lang {
            Lang::En => "top half = A, bottom = B (no control): ",
            Lang::Ja => "上半分=処方 A，下半分=処方 B（対照なし）: ",
        },
        Style::default().fg(Color::Gray),
    )];
    if order.is_empty() {
        legend.push(Span::styled(Msg::NoData.text(lang), Style::default().fg(theme.fg)));
    }
    for id in &order {
        let title = catalog
            .and_then(|catalog| catalog.prescription(id))
            .map_or_else(|| id.clone(), |prescription| prescription.title.text(lang).to_owned());
        let levels = history
            .iter()
            .filter(|day| &day.prescription_id == id)
            .filter_map(|day| day.level)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|level| char::from_u32(0x2460 + u32::from(level.clamp(1, 20)) - 1).unwrap_or('?'))
            .collect::<String>();
        legend.push(Span::styled("■", Style::default().fg(color(id))));
        legend.push(Span::styled(format!("{title} {levels}  "), Style::default().fg(theme.fg)));
    }
    frame.render_widget(Paragraph::new(Line::from(legend)), Rect::new(area.x, y + 1, area.width, 1));
}
