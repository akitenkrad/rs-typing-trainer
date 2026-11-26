//! «Summary»: the screenshot-friendly screen of the benchmark tab
//! (MYTASK-3644), toggled with `S` like the «today» tab's (MYTASK-3632).
//!
//! It shows the latest benchmark run — the parts saved on one day with the
//! same series and form, as history and the result screen group them — with
//! the values the benchmark tab and the result screen already show: each
//! part's net WPM in large figures, its accuracy, the difference from the
//! previous result and from the median of the last three, the eight skill
//! axes, and the result screen's personal-best verdicts in the bottom band.
//! The heading band, decorated frame and bottom band are shared with the
//! «today» Summary (`ui::today_summary`, `ui::analysis_image`).

use chrono::{DateTime, Local, NaiveDate};
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui_image::Image;
use unicode_width::UnicodeWidthStr;

use crate::analysis::bench_v3::{
    BenchV3Change, BenchV3Group, bench_v3_record_order, bench_v3_run_key, classify_bench_v3_change, compute_bench_v3_group_metrics,
    compute_bench_v3_group_reliability, compute_bench_v3_group_runs, compute_bench_v3_metrics, rolling_median_3,
};
use crate::analysis::grades::Direction;
use crate::analysis::summary::SkillAxisValue;
use crate::app::App;
use crate::bench::{BenchPart, BenchResultSummary, BenchV3Part, parse_bench_id, parse_v3_bench_id};
use crate::graphics::GraphicsMode;
use crate::i18n::{Lang, Msg};
use crate::record::SessionRecord;
use crate::ui::analysis_image::{
    BenchSummaryPart, BenchSummarySpec, RadarAxis, RadarChartSpec, RadarRing, SERIES_COLORS, SummaryFigure, TrialChartSpec,
};
use crate::ui::bigtext::BigTextSize;
use crate::ui::menu_summary::{difference_span, truncate};
use crate::ui::theme::Theme;
use crate::ui::today::image_colors;
use crate::ui::today_summary::{SummaryText, centered, event_band_lines, render_band_and_frame, render_footer, value_lines};

/// The latest benchmark run and what its Summary compares it with.
#[derive(Clone, Debug, PartialEq)]
pub struct BenchRun {
    /// Local date the run was played.
    pub date: NaiveDate,
    /// `bench-v1` or `bench-v2`.
    pub series: String,
    pub form: u8,
    /// The run's parts in A–D order, with the result screen's comparisons.
    pub results: Vec<BenchResultSummary>,
    /// A/C parts with earlier results only in the other series: their
    /// differences are not compared across versions.
    pub version_changed: Vec<BenchPart>,
    /// Date of the latest benchmark record outside this run.
    pub previous_date: Option<NaiveDate>,
}

fn local_date(record: &SessionRecord) -> Option<NaiveDate> {
    DateTime::parse_from_rfc3339(&record.header.finished_at)
        .ok()
        .map(|finished| finished.with_timezone(&Local).date_naive())
}

fn identity(record: &SessionRecord) -> Option<(&str, BenchPart, u8)> {
    record.header.bench_id.as_deref().and_then(parse_bench_id)
}

/// The latest run in `records` (in time order): the parts saved on the
/// latest benchmark record's day with its series and form.
pub fn latest_bench_run(records: &[SessionRecord]) -> Option<BenchRun> {
    let (latest, record) = records.iter().enumerate().rev().find(|(_, record)| identity(record).is_some())?;
    let (series, _, form) = identity(record)?;
    let date = local_date(record)?;
    let run = records
        .iter()
        .enumerate()
        .filter_map(|(position, record)| {
            let (candidate_series, part, candidate_form) = identity(record)?;
            (candidate_series == series && candidate_form == form && local_date(record) == Some(date)).then_some((position, record, part, form))
        })
        .collect::<Vec<_>>();
    let results = crate::record::db::bench_results_for_records(records, &run, series, form);
    // The record the result screen compares: the last one of each part.
    let position_of = |part: BenchPart| run.iter().rev().find(|row| row.2 == part).map(|row| row.0);
    let version_changed = results
        .iter()
        .filter(|result| matches!(result.part, BenchPart::A | BenchPart::C) && result.comparison.previous_wpm_delta.is_none())
        .filter(|result| {
            position_of(result.part).is_some_and(|position| {
                records[..position]
                    .iter()
                    .any(|candidate| identity(candidate).is_some_and(|(candidate_series, part, _)| part == result.part && candidate_series != series))
            })
        })
        .map(|result| result.part)
        .collect();
    let in_run = |position: usize| run.iter().any(|row| row.0 == position);
    let previous_date = records[..latest]
        .iter()
        .enumerate()
        .rev()
        .find(|(position, record)| !in_run(*position) && identity(record).is_some())
        .and_then(|(_, record)| local_date(record));
    Some(BenchRun {
        date,
        series: series.to_owned(),
        form,
        results,
        version_changed,
        previous_date,
    })
}

/// Everything the Summary shows, in display strings.
#[derive(Clone, Debug, PartialEq)]
pub struct BenchSummaryView {
    pub heading: String,
    pub subheading: String,
    pub parts: Vec<BenchSummaryPart>,
    pub axes_caption: String,
    pub axes: Vec<SummaryFigure>,
    pub events: Vec<String>,
    /// The first event is a personal best.
    pub highlight: bool,
    /// `true` when the cards and supporting figures use bench-v3 semantics.
    pub is_v3: bool,
    /// Effective-WPM attempts and their rolling-three median, one per v3 part.
    pub trends: Vec<BenchSummaryTrend>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BenchSummaryTrend {
    pub label: String,
    pub raw_values: Vec<f64>,
    pub values: Vec<f64>,
}

/// The part's name as the benchmark tab writes it.
fn part_name(part: BenchPart, lang: Lang) -> &'static str {
    crate::ui::bench_summary::part_label(part, lang)
}

fn version_changed_text(lang: Lang) -> &'static str {
    match lang {
        Lang::Ja => "版が違うため比較なし",
        Lang::En => "Different version: no comparison",
    }
}

/// A difference as the benchmark tab writes it: «前回比 +2.1 WPM» or «前回比 初回».
fn difference(label: &str, delta: Option<f64>, lang: Lang) -> SummaryFigure {
    let rounded = delta.map(|delta| (delta * 10.0).round() / 10.0);
    SummaryFigure {
        label: label.to_owned(),
        value: String::new(),
        delta_text: match rounded {
            Some(delta) => format!("{label} {delta:+.1} WPM"),
            None => format!("{label} {}", Msg::InitialResult.text(lang)),
        },
        delta: rounded,
        average: String::new(),
    }
}

fn joined_parts(parts: &[BenchPart], lang: Lang) -> String {
    let names = parts.iter().map(|part| part.as_str()).collect::<Vec<_>>();
    match lang {
        Lang::Ja => format!("パート {}", names.join("・")),
        Lang::En => format!("Part {}", names.join(", ")),
    }
}

/// The bottom band: the result screen's personal-best verdicts, else the
/// previous benchmark's date.
fn events(run: &BenchRun, lang: Lang) -> (Vec<String>, bool) {
    let best = |delta: fn(&BenchResultSummary) -> Option<f64>| {
        run.results
            .iter()
            .filter(|result| delta(result).is_some_and(|delta| delta > 0.0))
            .map(|result| result.part)
            .collect::<Vec<_>>()
    };
    let wpm = best(|result| result.comparison.best_wpm_delta);
    let accuracy = best(|result| result.comparison.best_accuracy_delta);
    let mut events = Vec::new();
    if !wpm.is_empty() {
        events.push(match lang {
            Lang::Ja => format!("自己ベスト更新! {}", joined_parts(&wpm, lang)),
            Lang::En => format!("New best! {}", joined_parts(&wpm, lang)),
        });
    }
    if !accuracy.is_empty() {
        events.push(match lang {
            Lang::Ja => format!("正確さの自己ベスト更新! {}", joined_parts(&accuracy, lang)),
            Lang::En => format!("New best accuracy! {}", joined_parts(&accuracy, lang)),
        });
    }
    if !events.is_empty() {
        return (events, true);
    }
    let line = match (run.previous_date, lang) {
        (Some(date), Lang::Ja) => format!("前回のベンチ: {date}"),
        (Some(date), Lang::En) => format!("Previous benchmark: {date}"),
        (None, Lang::Ja) => "初めてのベンチです".to_owned(),
        (None, Lang::En) => "First benchmark".to_owned(),
    };
    (vec![line], false)
}

pub fn summary_view(records: &[SessionRecord], axes: &[SkillAxisValue], lang: Lang) -> BenchSummaryView {
    let Some(run) = latest_bench_run(records) else {
        return BenchSummaryView {
            heading: match lang {
                Lang::Ja => "ベンチ".to_owned(),
                Lang::En => "Benchmark".to_owned(),
            },
            subheading: String::new(),
            parts: Vec::new(),
            axes_caption: String::new(),
            axes: Vec::new(),
            events: vec![no_records_text(lang).to_owned()],
            highlight: false,
            is_v3: false,
            trends: Vec::new(),
        };
    };
    let heading = match lang {
        Lang::Ja => format!("ベンチ {}", run.date),
        Lang::En => format!("Benchmark {}", run.date),
    };
    let parts = run.results.iter().map(|result| result.part).collect::<Vec<_>>();
    let subheading = match lang {
        Lang::Ja => format!("{}・フォーム {:02}・{}", run.series, run.form, joined_parts(&parts, lang)),
        Lang::En => format!("{} / form {:02} / {}", run.series, run.form, joined_parts(&parts, lang)),
    };
    let figures = run
        .results
        .iter()
        .map(|result| {
            let (previous, recent) = if run.version_changed.contains(&result.part) {
                (
                    text_figure(version_changed_text(lang)),
                    // No second line: the median of other versions is not compared either.
                    text_figure(""),
                )
            } else {
                (
                    difference(Msg::PreviousDelta.text(lang), result.comparison.previous_wpm_delta, lang),
                    difference(Msg::RecentThreeDelta.text(lang), result.comparison.recent_three_wpm_delta, lang),
                )
            };
            BenchSummaryPart {
                label: part_name(result.part, lang).to_owned(),
                value: format!("{:.1}", result.wpm),
                accuracy: match lang {
                    Lang::Ja => format!("正確さ {:.1}%", result.accuracy * 100.0),
                    Lang::En => format!("Accuracy {:.1}%", result.accuracy * 100.0),
                },
                previous,
                recent,
            }
        })
        .collect();
    let raw = axes.iter().any(|value| value.raw.is_some() && value.percentile.is_none());
    let axes_caption = match (raw, lang) {
        (false, Lang::Ja) => "技能の 8 軸（パーセンタイル）".to_owned(),
        (false, Lang::En) => "8 skill axes (percentile)".to_owned(),
        (true, Lang::Ja) => "技能の 8 軸（パーセンタイル．* は生値: 結果が 3 回未満）".to_owned(),
        (true, Lang::En) => "8 skill axes (percentile; * raw value, fewer than 3 results)".to_owned(),
    };
    let axes = axes
        .iter()
        .map(|value| SummaryFigure {
            label: value.axis.label(lang).to_owned(),
            value: match (value.percentile, value.raw) {
                (Some(position), _) => format!("{position:.1}"),
                (None, Some(raw)) => format!("{raw:.2}*"),
                (None, None) => "—".to_owned(),
            },
            delta: None,
            delta_text: String::new(),
            average: String::new(),
        })
        .collect();
    let (events, highlight) = events(&run, lang);
    BenchSummaryView {
        heading,
        subheading,
        parts: figures,
        axes_caption,
        axes,
        events,
        highlight,
        is_v3: false,
        trends: Vec::new(),
    }
}

/// A line of text without a difference (empty: no line at all).
fn text_figure(text: &str) -> SummaryFigure {
    SummaryFigure {
        label: String::new(),
        value: String::new(),
        delta: None,
        delta_text: text.to_owned(),
        average: String::new(),
    }
}

pub fn no_records_text(lang: Lang) -> &'static str {
    match lang {
        Lang::Ja => "ベンチの記録はまだありません",
        Lang::En => "No benchmark recorded yet",
    }
}

/// The view of the app's benchmark tab.
pub fn app_view(app: &App) -> BenchSummaryView {
    let axes = crate::analysis::summary::skill_axis_values(&app.analysis_records, app.practice_layout(), &app.config);
    if app
        .analysis_records
        .iter()
        .any(|record| record.header.bench_id.as_deref().and_then(parse_v3_bench_id).is_some())
    {
        return v3_summary_view(app, &axes);
    }
    summary_view(&app.analysis_records, &axes, app.config.general.lang)
}

fn v3_summary_view(app: &App, axes: &[SkillAxisValue]) -> BenchSummaryView {
    let lang = app.config.general.lang;
    let pause_ms = app.config.analysis.pause_ms;
    let Some(latest) = app
        .analysis_records
        .iter()
        .filter(|record| record.header.bench_id.as_deref().and_then(parse_v3_bench_id).is_some())
        .filter_map(|record| Some((bench_v3_record_order(record)?, record)))
        .max_by_key(|(order, _)| *order)
        .map(|(_, record)| record)
    else {
        return summary_view(&app.analysis_records, axes, lang);
    };
    let (date, form) = bench_v3_run_key(latest).expect("latest v3 record has a run key");
    let current = app
        .analysis_records
        .iter()
        .filter(|record| bench_v3_run_key(record) == Some((date, form)))
        .cloned()
        .collect::<Vec<_>>();

    let parts = [BenchV3Group::English, BenchV3Group::Japanese, BenchV3Group::Transition]
        .into_iter()
        .filter_map(|group| {
            let summary = compute_bench_v3_group_metrics(&current, group, pause_ms)?;
            let runs = compute_bench_v3_group_runs(&app.analysis_records, group, pause_ms);
            let current_index = runs.iter().position(|run| run.date == date && run.form == form);
            let current_value = summary.metrics.effective_wpm?;
            let previous_delta = current_index
                .and_then(|index| index.checked_sub(1).and_then(|previous| runs.get(previous)))
                .and_then(|previous| previous.metrics.effective_wpm)
                .map(|previous| current_value - previous);
            let reliability = compute_bench_v3_group_reliability(&app.analysis_records, group, pause_ms);
            let previous = v3_change_figure(previous_delta, reliability.map(|value| value.mdc95_wpm), lang);
            let best = runs.iter().filter_map(|run| run.metrics.effective_wpm).fold(current_value, f64::max);
            let recent = text_figure(&match lang {
                Lang::En => format!("Personal best {best:.1} WPM"),
                Lang::Ja => format!("自己ベスト {best:.1} WPM"),
            });
            let label = match (group, lang) {
                (BenchV3Group::English, Lang::En) => "English",
                (BenchV3Group::Japanese, Lang::En) => "Japanese",
                (BenchV3Group::Transition, Lang::En) => "Transitions",
                (BenchV3Group::English, Lang::Ja) => "英語",
                (BenchV3Group::Japanese, Lang::Ja) => "日本語",
                (BenchV3Group::Transition, Lang::Ja) => "移り変わり",
            };
            Some(BenchSummaryPart {
                label: label.to_owned(),
                value: format!("{current_value:.1}"),
                accuracy: match lang {
                    Lang::En => format!(
                        "tempo {} · accuracy {:.1}%",
                        summary.metrics.tempo_wpm.map_or_else(|| "—".to_owned(), |value| format!("{value:.1}")),
                        summary.metrics.accuracy * 100.0
                    ),
                    Lang::Ja => format!(
                        "テンポ {}・正確さ {:.1}%",
                        summary.metrics.tempo_wpm.map_or_else(|| "—".to_owned(), |value| format!("{value:.1}")),
                        summary.metrics.accuracy * 100.0
                    ),
                },
                previous,
                recent,
            })
        })
        .collect::<Vec<_>>();

    let raw_axes = axes.iter().any(|value| value.raw.is_some() && value.percentile.is_none());
    let axes_caption = match (raw_axes, lang) {
        (false, Lang::En) => "8 skill axes (bench-v3 percentile)".to_owned(),
        (false, Lang::Ja) => "技能の 8 軸（bench-v3 のパーセンタイル）".to_owned(),
        (true, Lang::En) => "8 skill axes (* raw value, fewer than 3 bench-v3 runs)".to_owned(),
        (true, Lang::Ja) => "技能の 8 軸（* は生値: bench-v3 が 3 回未満）".to_owned(),
    };
    let axes = axes
        .iter()
        .map(|value| SummaryFigure {
            label: value.axis.label(lang).to_owned(),
            value: match (value.percentile, value.raw) {
                (Some(value), _) => format!("{value:.1}"),
                (None, Some(value)) => format!("{value:.2}*"),
                (None, None) => "—".to_owned(),
            },
            delta: value.percentile,
            delta_text: String::new(),
            average: String::new(),
        })
        .collect::<Vec<_>>();

    let grade_line = crate::ui::analysis::bench_v3_grade_line(app, lang);
    let reliability_lines = [BenchV3Group::English, BenchV3Group::Japanese, BenchV3Group::Transition]
        .into_iter()
        .map(|group| {
            let label = match (group, lang) {
                (BenchV3Group::English, Lang::En) => "English",
                (BenchV3Group::Japanese, Lang::En) => "Japanese",
                (BenchV3Group::Transition, Lang::En) => "Transitions",
                (BenchV3Group::English, Lang::Ja) => "英語",
                (BenchV3Group::Japanese, Lang::Ja) => "日本語",
                (BenchV3Group::Transition, Lang::Ja) => "移り変わり",
            };
            match (compute_bench_v3_group_reliability(&app.analysis_records, group, pause_ms), lang) {
                (Some(value), Lang::En) => format!(
                    "{label} error{}: SEM {:.1} WPM · MDC95 {:.1} WPM",
                    if value.reference_only { " (reference)" } else { "" },
                    value.sem_wpm,
                    value.mdc95_wpm
                ),
                (Some(value), Lang::Ja) => format!(
                    "{label}の物差しの誤差{}: SEM {:.1} WPM・MDC95 {:.1} WPM",
                    if value.reference_only { "（参考）" } else { "" },
                    value.sem_wpm,
                    value.mdc95_wpm
                ),
                (None, Lang::En) => format!("{label} error (reference): SEM / MDC95 after 2 runs"),
                (None, Lang::Ja) => format!("{label}の物差しの誤差（参考）: SEM／MDC95 は 2 回分から表示"),
            }
        })
        .collect::<Vec<_>>();
    let trend_line = match lang {
        Lang::En => "Part trends: EW · ES · JW · JS · PW (dots: attempts; line: recent-3 median)".to_owned(),
        Lang::Ja => "パートの推移: EW・ES・JW・JS・PW（点: 各回，本線: 直近 3 回の中央値）".to_owned(),
    };
    let trends = [BenchV3Part::EW, BenchV3Part::ES, BenchV3Part::JW, BenchV3Part::JS, BenchV3Part::PW]
        .into_iter()
        .filter_map(|part| {
            let mut rows = app
                .analysis_records
                .iter()
                .filter(|record| {
                    record
                        .header
                        .bench_id
                        .as_deref()
                        .and_then(parse_v3_bench_id)
                        .is_some_and(|(candidate, _)| candidate == part)
                })
                .filter_map(|record| {
                    Some((
                        bench_v3_record_order(record)?,
                        compute_bench_v3_metrics(std::slice::from_ref(record), pause_ms).effective_wpm?,
                    ))
                })
                .collect::<Vec<_>>();
            rows.sort_by_key(|(order, _)| *order);
            let raw_values = rows.into_iter().map(|(_, value)| value).collect::<Vec<_>>();
            (!raw_values.is_empty()).then(|| BenchSummaryTrend {
                label: part.as_str().to_owned(),
                values: rolling_median_3(&raw_values),
                raw_values,
            })
        })
        .collect();

    BenchSummaryView {
        heading: match lang {
            Lang::En => format!("Benchmark {date}"),
            Lang::Ja => format!("ベンチ {date}"),
        },
        subheading: match lang {
            Lang::En => format!("bench-v3 / form {form:02}"),
            Lang::Ja => format!("bench-v3・フォーム {form:02}"),
        },
        parts,
        axes_caption,
        axes,
        events: std::iter::once(grade_line)
            .chain(reliability_lines)
            .chain(std::iter::once(trend_line))
            .collect(),
        highlight: false,
        is_v3: true,
        trends,
    }
}

fn v3_change_figure(delta: Option<f64>, mdc95_wpm: Option<f64>, lang: Lang) -> SummaryFigure {
    let Some(delta) = delta else {
        return text_figure(Msg::InitialResult.text(lang));
    };
    let text = match classify_bench_v3_change(delta, mdc95_wpm) {
        BenchV3Change::WithinError => match lang {
            Lang::En => "no change (within measurement error)".to_owned(),
            Lang::Ja => "変化なし（誤差の範囲）".to_owned(),
        },
        BenchV3Change::Increase => format!("▲ {delta:+.1} WPM"),
        BenchV3Change::Decrease => format!("▼ {delta:+.1} WPM"),
        BenchV3Change::DifferenceOnly => format!("{delta:+.1} WPM"),
    };
    SummaryFigure {
        label: String::new(),
        value: String::new(),
        delta: Some(delta),
        delta_text: text,
        average: String::new(),
    }
}

/// Image when graphics are available, there is a run, and the area is at
/// least 120×29 cells (as for the «today» Summary).
pub fn uses_image(app: &App, view: &BenchSummaryView, area: Rect) -> bool {
    app.graphics.mode != GraphicsMode::Off && app.graphics.cell_pixels.is_some() && !view.parts.is_empty() && area.width >= 120 && area.height >= 29
}

pub fn render(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let lang = app.config.general.lang;
    let Some(body) = render_footer(frame, lang, theme, area) else {
        return;
    };
    let view = app_view(app);
    if uses_image(app, &view, body) && render_image(frame, app, theme, &view, body) {
        return;
    }
    render_text(frame, lang, theme, &view, body);
}

fn render_image(frame: &mut Frame, app: &App, theme: &Theme, view: &BenchSummaryView, area: Rect) -> bool {
    let spec = image_spec(view, theme, area.as_size());
    let font = app.bigtext_images.borrow_mut().resolved_font(&app.config.display.big_font, &app.home_dir);
    let mut images = app.analysis_images.borrow_mut();
    if images.prepare_bench_summary(&app.graphics, &font, &spec).is_err() {
        return false;
    }
    let Some(protocol) = images.bench_summary_protocol(&app.graphics, &font, &spec) else {
        return false;
    };
    frame.render_widget(Image::new(protocol), area);
    true
}

/// The image spec of `view` for `cells`.
pub fn image_spec(view: &BenchSummaryView, theme: &Theme, cells: ratatui::layout::Size) -> BenchSummarySpec {
    let radar = view.is_v3.then(|| RadarChartSpec {
        axes: view
            .axes
            .iter()
            .map(|axis| RadarAxis {
                label: if axis.delta.is_none() && axis.value != "—" {
                    format!("{} {}", axis.label, axis.value)
                } else {
                    axis.label.clone()
                },
                value: axis.delta,
            })
            .collect(),
        max_value: 100.0,
        rings: [25.0, 50.0, 75.0, 100.0]
            .into_iter()
            .map(|value| RadarRing {
                value,
                label: matches!(value as u8, 50 | 100).then(|| format!("{value:.0}")),
            })
            .collect(),
        color: theme.accent,
        cells: ratatui::layout::Size::new((cells.width * 2 / 5).max(1), (cells.height / 3).max(1)),
        missing_label: "-".to_owned(),
    });
    let trends = view
        .trends
        .iter()
        .enumerate()
        .map(|(index, trend)| {
            (
                trend.label.clone(),
                TrialChartSpec {
                    raw_values: trend.raw_values.clone(),
                    values: trend.values.clone(),
                    color: SERIES_COLORS[index % SERIES_COLORS.len()],
                    cells: ratatui::layout::Size::new((cells.width / 2).max(1), 2),
                },
            )
        })
        .collect();
    BenchSummarySpec {
        heading: view.heading.clone(),
        subheading: view.subheading.clone(),
        parts: view.parts.clone(),
        unit: "WPM".to_owned(),
        axes_caption: view.axes_caption.clone(),
        // The image font has no «—».
        axes: view
            .axes
            .iter()
            .map(|axis| SummaryFigure {
                value: axis.value.replace('—', "-"),
                ..axis.clone()
            })
            .collect(),
        events: view.events.clone(),
        highlight: view.highlight,
        radar,
        trends,
        cells,
        colors: image_colors(theme),
    }
}

/// One difference line that fits `width`: the label in muted text and the
/// colored «▲+2.1 WPM». When too wide it drops the unit, then uses the short
/// label, then (for a first result) only «初回».
fn difference_line(figure: &SummaryFigure, short: &str, lang: Lang, theme: &Theme, width: usize) -> Line<'static> {
    if figure.delta_text.is_empty() {
        return Line::default();
    }
    let muted = Style::default().fg(theme.muted);
    let candidates: Vec<Vec<Span<'static>>> = if figure.label.is_empty() {
        // A note without a difference («版が違うため比較なし»).
        let short = if figure.delta_text == version_changed_text(lang) {
            match lang {
                Lang::Ja => "版違いで比較なし",
                Lang::En => "new version",
            }
        } else {
            figure.delta_text.as_str()
        };
        vec![
            vec![Span::styled(figure.delta_text.clone(), muted)],
            vec![Span::styled(short.to_owned(), muted)],
        ]
    } else if let Some(delta) = figure.delta {
        [(figure.label.as_str(), " WPM"), (figure.label.as_str(), ""), (short, "")]
            .into_iter()
            .map(|(label, suffix)| {
                vec![
                    Span::styled(format!("{label} "), muted),
                    difference_span(Some(delta), Direction::Higher, move |value| format!("{value:+.1}{suffix}"), theme),
                ]
            })
            .collect()
    } else {
        let initial = Msg::InitialResult.text(lang);
        [format!("{} {initial}", figure.label), format!("{short} {initial}"), initial.to_owned()]
            .into_iter()
            .map(|text| vec![Span::styled(text, muted)])
            .collect()
    };
    let fits = |spans: &&Vec<Span<'static>>| spans.iter().map(|span| span.content.width()).sum::<usize>() <= width;
    let spans = candidates.iter().find(fits).or(candidates.last()).cloned().unwrap_or_default();
    centered(spans, width)
}

/// The axes as «持続速度 72.5» items wrapped over lines of `width`.
fn axis_lines(view: &BenchSummaryView, theme: &Theme, width: usize) -> Vec<Vec<Span<'static>>> {
    let mut lines: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    for axis in &view.axes {
        let spans = vec![
            Span::styled(format!("{} ", axis.label), Style::default().fg(theme.muted)),
            Span::styled(axis.value.clone(), Style::default().add_modifier(Modifier::BOLD)),
        ];
        let item_width = spans.iter().map(|span| span.content.width()).sum::<usize>();
        let current = lines.last_mut().expect("one line");
        let used = current.iter().map(|span| span.content.width()).sum::<usize>();
        if !current.is_empty() && used + 3 + item_width + 2 > width {
            lines.push(spans);
        } else {
            if !current.is_empty() {
                current.push(Span::raw("   "));
            }
            current.extend(spans);
        }
    }
    lines.retain(|line| !line.is_empty());
    lines
}

/// The biggest bitmap font whose rows and widths fit the frame's inside.
pub fn text_size(view: &BenchSummaryView, inner_width: u16, inner_height: u16, fixed: u16) -> SummaryText {
    let columns = view.parts.len().max(1) as u16;
    let column_width = inner_width / columns;
    for size in [BigTextSize::Dot16, BigTextSize::Dot12] {
        let Ok(font) = crate::ui::bigtext::font(size) else { continue };
        let widest = view.parts.iter().map(|part| font.text_width(&part.value)).max().unwrap_or(0);
        if !view.parts.is_empty() && widest + 2 <= column_width && fixed + size.cell_height() <= inner_height {
            return SummaryText::Big(size);
        }
    }
    SummaryText::Plain
}

/// The text version, also used when an image cannot be drawn.
pub fn render_text(frame: &mut Frame, lang: Lang, theme: &Theme, view: &BenchSummaryView, area: Rect) {
    let Some(inner) = render_band_and_frame(frame, theme, area, &view.heading, &view.subheading) else {
        return;
    };
    let inner_width = usize::from(inner.width);
    if view.parts.is_empty() {
        let y = inner.y + inner.height.saturating_sub(1) / 2;
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                truncate(no_records_text(lang), inner_width),
                Style::default().fg(theme.muted).add_modifier(Modifier::BOLD),
            )))
            .alignment(Alignment::Center),
            Rect::new(inner.x, y, inner.width, 1),
        );
        return;
    }

    // Dark band at the bottom of the frame. v3 adds the actual five part
    // series; short terminals use plain card values so all rows still fit.
    let support_events = if view.is_v3 {
        let mut events = view.events.iter().take(4).cloned().collect::<Vec<_>>();
        events.extend(view.trends.iter().map(|trend| format!("{}  {}", trend.label, trend_text_figure(trend))));
        events
    } else {
        view.events.clone()
    };
    let event_rows = if view.is_v3 {
        support_events.len().clamp(1, 9) as u16
    } else {
        support_events.len().clamp(1, 2) as u16
    }
    .min(inner.height);
    let event_area = Rect::new(inner.x, inner.y + inner.height - event_rows, inner.width, event_rows);
    frame.render_widget(
        Paragraph::new(event_band_lines(&support_events, view.highlight, theme, inner_width, event_rows)),
        event_area,
    );

    // The axes: a caption and the items wrapped over at most three lines.
    let mut axes = axis_lines(view, theme, inner_width);
    axes.truncate(3);
    let axis_rows = if axes.is_empty() { 0 } else { 1 + axes.len() as u16 };
    // Per part: name, big value, «WPM 正確さ», two differences.
    let fixed = 1 + 1 + 2 + axis_rows + event_rows;
    let size = text_size(view, inner.width, inner.height, fixed);
    let big_rows = match size {
        SummaryText::Big(size) => size.cell_height(),
        SummaryText::Plain => 1,
    };
    let main_rows = 1 + big_rows + 1 + 2;
    let free = inner.height.saturating_sub(event_rows + main_rows + axis_rows);
    let top_gap = free / 3;
    let middle_gap = (free - top_gap) / 2;
    let main_y = inner.y + top_gap;
    let columns = view.parts.len().max(1) as u16;
    let column_width = inner.width / columns;
    let (previous_short, recent_short) = match lang {
        Lang::Ja => ("前回比", "直近 3 回比"),
        Lang::En => ("prev", "recent-3"),
    };
    // Every part's full name with a blank column on each side, else «パート A» for all.
    let full_labels = view.parts.iter().all(|part| part.label.width() + 2 <= usize::from(column_width));
    for (index, part) in view.parts.iter().enumerate() {
        let x = inner.x + column_width * index as u16;
        let width = if index + 1 == view.parts.len() {
            inner.width - column_width * index as u16
        } else {
            column_width
        };
        let column_cells = usize::from(width);
        let mut row = main_y;
        let mut put = |lines: Vec<Line<'static>>, height: u16, alignment: Alignment| {
            if row + height <= event_area.y {
                frame.render_widget(Paragraph::new(lines).alignment(alignment), Rect::new(x, row, width, height));
            }
            row += height;
        };
        let label = if full_labels {
            part.label.clone()
        } else {
            let short = part.label.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
            truncate(&short, column_cells)
        };
        put(
            vec![Line::from(Span::styled(
                label,
                Style::default().fg(theme.muted).add_modifier(Modifier::BOLD),
            ))],
            1,
            Alignment::Center,
        );
        let style = Style::default().fg(theme.accent).add_modifier(Modifier::BOLD);
        put(value_lines(&part.value, size, style), big_rows, Alignment::Center);
        let unit = if view.is_v3 {
            let compact = match lang {
                Lang::En => part.accuracy.replace("tempo ", "T ").replace("accuracy ", "acc "),
                Lang::Ja => part.accuracy.replace("テンポ ", "T ").replace("正確さ ", "正 "),
            };
            [format!("WPM · {}", part.accuracy), format!("WPM · {compact}"), compact]
                .into_iter()
                .find(|unit| unit.width() + 2 <= column_cells)
                .unwrap_or_else(|| truncate(&part.accuracy, column_cells.saturating_sub(2)))
        } else {
            let accuracy = part.accuracy.rsplit_once(' ').map_or(part.accuracy.as_str(), |(_, value)| value);
            [
                format!("WPM  {}", part.accuracy),
                match lang {
                    Lang::Ja => format!("WPM  {accuracy}"),
                    Lang::En => format!("WPM  acc {accuracy}"),
                },
                format!("WPM  {accuracy}"),
            ]
            .into_iter()
            .find(|unit| unit.width() + 2 <= column_cells)
            .unwrap_or_else(|| format!("WPM  {accuracy}"))
        };
        put(
            vec![centered(
                vec![Span::styled(unit, Style::default().add_modifier(Modifier::BOLD))],
                column_cells,
            )],
            1,
            Alignment::Left,
        );
        put(
            vec![difference_line(&part.previous, previous_short, lang, theme, column_cells)],
            1,
            Alignment::Left,
        );
        put(
            vec![difference_line(&part.recent, recent_short, lang, theme, column_cells)],
            1,
            Alignment::Left,
        );
    }
    let mut y = main_y + main_rows + middle_gap;
    if !axes.is_empty() && y < event_area.y {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                truncate(&view.axes_caption, inner_width),
                Style::default().fg(theme.muted),
            )))
            .alignment(Alignment::Center),
            Rect::new(inner.x, y, inner.width, 1),
        );
        y += 1;
    }
    for spans in axes {
        if y >= event_area.y {
            break;
        }
        frame.render_widget(Paragraph::new(centered(spans, inner_width)), Rect::new(inner.x, y, inner.width, 1));
        y += 1;
    }
}

fn trend_text_figure(trend: &BenchSummaryTrend) -> String {
    const LEVELS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let finite = trend
        .raw_values
        .iter()
        .chain(&trend.values)
        .copied()
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    if finite.is_empty() {
        return "—".to_owned();
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
    format!("·{}  ━{}", spark(&trend.raw_values), spark(&trend.values))
}
