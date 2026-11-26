//! The «today» tab of the analysis screen (MYTASK-3631).
//!
//! Three sections, read straight from the database — independent of the
//! analysis period (`[P]`) and mode (`[M]`) filters:
//!
//! - **Today's training**: the steps of today's menu runs, with the same
//!   figures as the upper section of the menu summary screen
//!   ([`crate::ui::menu_summary::upper_lines`]), plus the steps not played yet.
//! - **Individual practice**: today's sessions that are not menu steps.
//! - **Today's totals**: time, sessions, keystrokes, and per language the
//!   median net WPM and accuracy, against the last earlier practice day.
//!
//! With terminal images the sections are drawn as figures (steps with badges
//! and bars, a timeline of the sessions, big-number cards); otherwise, or when
//! the window is small, as text — in the result screen's order of fallback.

use chrono::{DateTime, Local, NaiveDate, Timelike};
use ratatui::Frame;
use ratatui::layout::{Rect, Size};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui_image::Image;
use unicode_width::UnicodeWidthStr;

use crate::analysis::grades::Direction;
use crate::app::App;
use crate::error::AppResult;
use crate::graphics::GraphicsMode;
use crate::i18n::Lang;
use crate::practice::menu::Graduation;
use crate::practice::menu::progress::{RunStepsSummary, run_steps_summary};
use crate::record::SessionRecord;
use crate::record::db;
use crate::ui::analysis_image::{
    FORECAST_EN_COLOR, FORECAST_JA_COLOR, TodayCardSpec, TodayPointShape, TodayStepRow, TodayStepsLine, TodayStepsSpec, TodayTimelinePoint,
    TodayTimelineSpec, mode_label,
};
use crate::ui::menu_summary::{
    difference_span, graduation_progress, pad, padded, practice_language_label, role_label, section_rule, spark_text, step_difference,
    step_value_text, truncate, upper_lines,
};
use crate::ui::result_image::ResultImageColors;
use crate::ui::theme::Theme;

/// How a session of today belongs to the day.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TodaySessionKind {
    /// A step of a menu run (`step` is 1-based).
    Menu {
        run_id: i64,
        step: usize,
    },
    /// A benchmark part («A»…«D»).
    Bench(String),
    Individual,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TodaySession {
    pub id: i64,
    pub started_at: DateTime<Local>,
    pub mode: String,
    pub language: Lang,
    pub duration_ms: i64,
    pub net_wpm: f64,
    /// 0..1.
    pub accuracy: f64,
    pub keystrokes: usize,
    pub kind: TodaySessionKind,
}

/// Per-language figures of a day.
#[derive(Clone, Debug, PartialEq)]
pub struct LanguageTotals {
    pub language: Lang,
    pub sessions: usize,
    pub median_wpm: f64,
    /// Median of the sessions' accuracies (0..1).
    pub median_accuracy: f64,
    /// Net WPM of each session in time order (the small bars).
    pub wpms: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DayTotals {
    pub date: NaiveDate,
    pub duration_ms: i64,
    pub sessions: usize,
    pub keystrokes: usize,
    pub languages: Vec<LanguageTotals>,
}

impl DayTotals {
    pub fn language(&self, language: Lang) -> Option<&LanguageTotals> {
        self.languages.iter().find(|row| row.language == language)
    }
}

/// One menu run of today.
#[derive(Clone, Debug, PartialEq)]
pub struct TodayRun {
    /// `menu_runs.lang` (the practice languages of the settings, e.g. «en,ja»).
    pub languages: String,
    pub steps: RunStepsSummary,
    /// The short name of each planned step (as in the progress band,
    /// [`crate::practice::menu::step_short_label`]) and whether it is done.
    pub chain: Vec<(String, bool)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TodayData {
    pub date: NaiveDate,
    /// Runs with at least one step played, newest first; or, when none has a
    /// played step, the latest run alone (shown as not played).
    pub runs: Vec<TodayRun>,
    /// Today's sessions in time order (menu steps, benches and the rest).
    pub sessions: Vec<TodaySession>,
    /// `None` when nothing was recorded today.
    pub totals: Option<DayTotals>,
    /// The 7-day average the totals are compared with: the 7 calendar days
    /// before today (today − 7 … yesterday), over the days with records.
    pub baseline: Option<Baseline>,
}

/// Per-language part of [`Baseline`]: the average over the days the language
/// was practised of that day's median net WPM and median accuracy.
#[derive(Clone, Debug, PartialEq)]
pub struct BaselineLanguage {
    pub language: Lang,
    pub days: usize,
    pub median_wpm: f64,
    pub median_accuracy: f64,
}

/// The average day of the 7 calendar days before today, over the days with
/// records (days without practice are not counted as zero).
#[derive(Clone, Debug, PartialEq)]
pub struct Baseline {
    pub from: NaiveDate,
    pub to: NaiveDate,
    /// Days with records in the window.
    pub days: usize,
    pub duration_ms: f64,
    pub sessions: f64,
    pub keystrokes: f64,
    pub languages: Vec<BaselineLanguage>,
}

impl Baseline {
    /// Days in the window: today − 7 … today − 1.
    pub const WINDOW_DAYS: i64 = 7;

    pub fn language(&self, language: Lang) -> Option<&BaselineLanguage> {
        self.languages.iter().find(|row| row.language == language)
    }

    /// The average of `daily` (one entry per day with records); `None` when empty.
    pub fn from_days(from: NaiveDate, to: NaiveDate, daily: &[DayTotals]) -> Option<Self> {
        if daily.is_empty() {
            return None;
        }
        let count = daily.len() as f64;
        let average = |values: Vec<f64>| values.iter().sum::<f64>() / values.len().max(1) as f64;
        let languages = [Lang::En, Lang::Ja]
            .into_iter()
            .filter_map(|language| {
                let rows = daily.iter().filter_map(|day| day.language(language)).collect::<Vec<_>>();
                (!rows.is_empty()).then(|| BaselineLanguage {
                    language,
                    days: rows.len(),
                    median_wpm: average(rows.iter().map(|row| row.median_wpm).collect()),
                    median_accuracy: average(rows.iter().map(|row| row.median_accuracy).collect()),
                })
            })
            .collect();
        Some(Self {
            from,
            to,
            days: daily.len(),
            duration_ms: daily.iter().map(|day| day.duration_ms as f64).sum::<f64>() / count,
            sessions: daily.iter().map(|day| day.sessions as f64).sum::<f64>() / count,
            keystrokes: daily.iter().map(|day| day.keystrokes as f64).sum::<f64>() / count,
            languages,
        })
    }
}

impl TodayData {
    pub fn empty(date: NaiveDate) -> Self {
        Self {
            date,
            runs: Vec::new(),
            sessions: Vec::new(),
            totals: None,
            baseline: None,
        }
    }

    /// Today's sessions that are not menu steps.
    pub fn individual(&self) -> impl Iterator<Item = &TodaySession> {
        self.sessions
            .iter()
            .filter(|session| !matches!(session.kind, TodaySessionKind::Menu { .. }))
    }
}

/// Read today's runs and sessions and the 7-day average before today.
pub fn build_today(db_path: &std::path::Path, today: NaiveDate, display: Lang) -> AppResult<TodayData> {
    let days = db::session_local_days(db_path)?;
    let today_ids = days.iter().filter(|(_, day)| *day == today).map(|(id, _)| *id).collect::<Vec<_>>();
    let step_sessions = db::menu_step_sessions(db_path)?;
    let load = |ids: &[i64]| -> AppResult<Vec<TodaySession>> {
        let mut sessions = Vec::new();
        for id in ids {
            let Some(record) = db::load_record(db_path, *id)? else { continue };
            let kind = match step_sessions.iter().find(|(session, _, _)| session == id) {
                Some((_, run_id, step)) => TodaySessionKind::Menu {
                    run_id: *run_id,
                    step: *step,
                },
                None => match record.header.bench_id.as_deref().and_then(crate::bench::parse_bench_id) {
                    Some((_, part, _)) => TodaySessionKind::Bench(part.as_str().to_owned()),
                    None => TodaySessionKind::Individual,
                },
            };
            sessions.push(today_session(*id, &record, kind));
        }
        sessions.sort_by(|left, right| left.started_at.cmp(&right.started_at).then(left.id.cmp(&right.id)));
        Ok(sessions)
    };
    let sessions = load(&today_ids)?;
    let from = today - chrono::Duration::days(Baseline::WINDOW_DAYS);
    let to = today - chrono::Duration::days(1);
    let window = days
        .iter()
        .map(|(_, day)| *day)
        .filter(|day| (from..=to).contains(day))
        .collect::<std::collections::BTreeSet<_>>();
    let mut daily = Vec::new();
    for day in window {
        let ids = days.iter().filter(|(_, date)| *date == day).map(|(id, _)| *id).collect::<Vec<_>>();
        daily.extend(day_totals(day, &load(&ids)?));
    }
    let baseline = Baseline::from_days(from, to, &daily);
    let mut runs = Vec::new();
    for run in db::load_menu_runs_on(db_path, today)? {
        if let Some(steps) = run_steps_summary(db_path, &run.layout, run.id, display)? {
            let chain = serde_json::from_str::<crate::practice::menu::DailyMenu>(&run.plan_json)
                .map(|plan| {
                    plan.steps
                        .iter()
                        .enumerate()
                        .map(|(index, step)| {
                            let done = steps.steps.get(index).is_some_and(|step| step.done);
                            (crate::practice::menu::step_short_label(step, display), done)
                        })
                        .collect()
                })
                .unwrap_or_default();
            runs.push(TodayRun {
                languages: run.lang.clone(),
                steps,
                chain,
            });
        }
    }
    let played = runs.iter().filter(|run| run.steps.completed() > 0).cloned().collect::<Vec<_>>();
    let runs = if played.is_empty() { runs.into_iter().take(1).collect() } else { played };
    Ok(TodayData {
        date: today,
        runs,
        totals: day_totals(today, &sessions),
        sessions,
        baseline,
    })
}

fn today_session(id: i64, record: &SessionRecord, kind: TodaySessionKind) -> TodaySession {
    let started_at = DateTime::parse_from_rfc3339(&record.header.started_at)
        .map(|time| time.with_timezone(&Local))
        .unwrap_or_else(|_| Local::now());
    TodaySession {
        id,
        started_at,
        mode: record.header.mode.clone(),
        language: match crate::analysis::model::session_language(record) {
            "ja" => Lang::Ja,
            _ => Lang::En,
        },
        duration_ms: record.header.duration_ms.max(0),
        net_wpm: record.header.net_wpm,
        accuracy: record.header.accuracy,
        keystrokes: record.log.keystrokes.len(),
        kind,
    }
}

fn median(values: &[f64]) -> f64 {
    let mut values = values.iter().copied().filter(|value| value.is_finite()).collect::<Vec<_>>();
    if values.is_empty() {
        return f64::NAN;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}

/// The figures of a day; `None` when the day has no session.
pub fn day_totals(date: NaiveDate, sessions: &[TodaySession]) -> Option<DayTotals> {
    if sessions.is_empty() {
        return None;
    }
    let languages = [Lang::En, Lang::Ja]
        .into_iter()
        .filter_map(|language| {
            let rows = sessions.iter().filter(|session| session.language == language).collect::<Vec<_>>();
            (!rows.is_empty()).then(|| LanguageTotals {
                language,
                sessions: rows.len(),
                median_wpm: median(&rows.iter().map(|row| row.net_wpm).collect::<Vec<_>>()),
                median_accuracy: median(&rows.iter().map(|row| row.accuracy).collect::<Vec<_>>()),
                wpms: rows.iter().map(|row| row.net_wpm).collect(),
            })
        })
        .collect();
    Some(DayTotals {
        date,
        duration_ms: sessions.iter().map(|session| session.duration_ms).sum(),
        sessions: sessions.len(),
        keystrokes: sessions.iter().map(|session| session.keystrokes).sum(),
        languages,
    })
}

// ---------------------------------------------------------------------------
// Text helpers
// ---------------------------------------------------------------------------

pub(crate) fn minutes(duration_ms: i64) -> f64 {
    duration_ms as f64 / 60_000.0
}

pub(crate) fn grouped(value: impl std::fmt::Display) -> String {
    let digits = value.to_string();
    let mut output = String::new();
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            output.push(',');
        }
        output.push(character);
    }
    output
}

fn duration_text(duration_ms: i64) -> String {
    let seconds = (duration_ms.max(0) + 500) / 1000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

pub(crate) fn language_name(language: Lang, lang: Lang) -> &'static str {
    match (language, lang) {
        (Lang::En, Lang::En) => "English",
        (Lang::Ja, Lang::En) => "Japanese",
        (Lang::En, Lang::Ja) => "英語",
        (Lang::Ja, Lang::Ja) => "日本語",
    }
}

fn run_languages_label(languages: &str, lang: Lang) -> String {
    languages
        .split(',')
        .map(|code| match code {
            "ja" => language_name(Lang::Ja, lang),
            _ => language_name(Lang::En, lang),
        })
        .collect::<Vec<_>>()
        .join(match lang {
            Lang::En => " + ",
            Lang::Ja => "＋",
        })
}

fn run_heading(run: &TodayRun, lang: Lang) -> String {
    let total = run.steps.steps.len();
    let done = run.steps.completed();
    let languages = run_languages_label(&run.languages, lang);
    match (done, lang) {
        (0, Lang::En) => format!("Menu ({languages}, {} min, {total} steps) — not played", run.steps.minutes),
        (0, Lang::Ja) => format!("メニュー（{languages}，{} 分，{total} 段）— 未実施", run.steps.minutes),
        (_, Lang::En) => format!("Menu ({languages}, {} min) — {done} of {total} steps", run.steps.minutes),
        (_, Lang::Ja) => format!("メニュー（{languages}，{} 分）— {total} 段中 {done} 段", run.steps.minutes),
    }
}

fn not_yet(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "not yet",
        Lang::Ja => "未",
    }
}

fn no_record_text(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "No practice recorded today yet",
        Lang::Ja => "今日の記録はまだありません",
    }
}

fn session_mode_text(session: &TodaySession, lang: Lang) -> String {
    match (&session.kind, lang) {
        (TodaySessionKind::Bench(part), Lang::En) => format!("[bench {part}]"),
        (TodaySessionKind::Bench(part), Lang::Ja) => format!("[ベンチ {part}]"),
        _ => mode_label(&session.mode, lang),
    }
}

fn baseline_label(data: &TodayData, lang: Lang) -> String {
    match (&data.baseline, lang) {
        (Some(baseline), Lang::En) => format!(
            "vs the 7-day average ({}–{}, {} days)",
            baseline.from.format("%m/%d"),
            baseline.to.format("%m/%d"),
            baseline.days
        ),
        (Some(baseline), Lang::Ja) => format!(
            "過去 7 日平均（{}〜{}，記録 {} 日）と比べる",
            baseline.from.format("%m/%d"),
            baseline.to.format("%m/%d"),
            baseline.days
        ),
        (None, Lang::En) => "no records in the past 7 days".to_owned(),
        (None, Lang::Ja) => "過去 7 日の記録なし".to_owned(),
    }
}

fn delta_span(delta: Option<f64>, digits: usize, suffix: &'static str, theme: &Theme) -> Span<'static> {
    difference_span(delta, Direction::Higher, move |value| format!("{value:+.digits$}{suffix}"), theme)
}

// ---------------------------------------------------------------------------
// Text rendering
// ---------------------------------------------------------------------------

/// The whole tab as text lines for `width` cells.
pub fn text_lines(data: &TodayData, lang: Lang, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if data.totals.is_none() {
        lines.push(Line::from(Span::styled(
            no_record_text(lang).to_owned(),
            Style::default().fg(theme.warning).add_modifier(Modifier::BOLD),
        )));
    }
    lines.push(section_rule(
        match lang {
            Lang::En => format!("Today's training {}", data.date),
            Lang::Ja => format!("今日の訓練 {}", data.date),
        },
        width,
        theme,
    ));
    lines.extend(training_lines(data, lang, theme, width));
    if data.totals.is_none() {
        return clip(lines, width);
    }
    let individual = data.individual().collect::<Vec<_>>();
    lines.push(section_rule(
        match lang {
            Lang::En => format!("Individual practice ({} sessions)", individual.len()),
            Lang::Ja => format!("個別の練習（{} 回）", individual.len()),
        },
        width,
        theme,
    ));
    lines.extend(individual_lines(&individual, lang, theme, width));
    lines.push(section_rule(
        match lang {
            Lang::En => format!("Today's totals — {}", baseline_label(data, lang)),
            Lang::Ja => format!("今日の合計 — {}", baseline_label(data, lang)),
        },
        width,
        theme,
    ));
    lines.extend(total_lines(data, lang, theme));
    clip(lines, width)
}

/// Truncate every line to `width` display cells, keeping the styles.
fn clip(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|line| {
            let mut used = 0;
            let mut spans = Vec::new();
            for span in line.spans {
                let span_width = span.content.width();
                if used + span_width <= width {
                    used += span_width;
                    spans.push(span);
                } else {
                    let room = width.saturating_sub(used);
                    if room > 0 {
                        spans.push(Span::styled(truncate(&span.content, room), span.style));
                    }
                    break;
                }
            }
            Line::from(spans)
        })
        .collect()
}

fn training_lines(data: &TodayData, lang: Lang, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    if data.runs.is_empty() {
        return vec![Line::from(Span::styled(
            match lang {
                Lang::En => "No menu for today yet (Today's training [M] on the title screen)",
                Lang::Ja => "今日のメニューはまだありません（タイトルの «今日の訓練 [M]»）",
            }
            .to_owned(),
            Style::default().fg(theme.muted),
        ))];
    }
    let mut lines = Vec::new();
    for run in &data.runs {
        lines.push(Line::from(Span::styled(
            run_heading(run, lang),
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
        )));
        lines.extend(upper_lines(&run.steps, lang, theme, width, not_yet(lang)));
    }
    lines
}

fn individual_lines(sessions: &[&TodaySession], lang: Lang, theme: &Theme, _width: usize) -> Vec<Line<'static>> {
    if sessions.is_empty() {
        return vec![Line::from(Span::styled(
            match lang {
                Lang::En => "None (only menu steps today)",
                Lang::Ja => "なし（今日はメニューの段だけ）",
            }
            .to_owned(),
            Style::default().fg(theme.muted),
        ))];
    }
    let header = match lang {
        Lang::En => ["Time", "Mode", "Lang", "Length", "net WPM", "Accuracy"],
        Lang::Ja => ["時刻", "モード", "言語", "時間", "net WPM", "正確さ"],
    };
    let (mode_width, language_width) = (12, 8);
    let mut lines = vec![Line::from(Span::styled(
        format!(
            "{} {} {} {} {} {}",
            pad(header[0], 5),
            pad(header[1], mode_width),
            pad(header[2], language_width),
            pad(header[3], 6),
            pad(header[4], 8),
            header[5]
        ),
        Style::default().fg(theme.muted),
    ))];
    for session in sessions {
        let bench = matches!(session.kind, TodaySessionKind::Bench(_));
        lines.push(Line::from(vec![
            Span::raw(format!("{} ", session.started_at.format("%H:%M"))),
            Span::styled(
                format!("{} ", pad(&session_mode_text(session, lang), mode_width)),
                if bench {
                    Style::default().fg(theme.warning).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                },
            ),
            Span::styled(
                format!("{} ", pad(language_name(session.language, lang), language_width)),
                Style::default().fg(language_color(session.language)),
            ),
            Span::raw(format!("{} ", pad(&duration_text(session.duration_ms), 6))),
            Span::styled(format!("{:>7.1}  ", session.net_wpm), Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!("{:>5.1}%", session.accuracy * 100.0)),
        ]));
    }
    lines
}

fn total_lines(data: &TodayData, lang: Lang, theme: &Theme) -> Vec<Line<'static>> {
    let Some(totals) = &data.totals else {
        return Vec::new();
    };
    let previous = data.baseline.as_ref();
    let labels = match lang {
        Lang::En => ["Time", "min", "Sessions", "Keystrokes"],
        Lang::Ja => ["練習時間", "分", "回数", "打鍵数"],
    };
    let mut lines = vec![Line::from(vec![
        Span::styled(format!("{} ", labels[0]), Style::default().fg(theme.muted)),
        Span::styled(
            format!("{:.1} {} ", minutes(totals.duration_ms), labels[1]),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        delta_span(
            previous.map(|previous| minutes(totals.duration_ms) - previous.duration_ms / 60_000.0),
            1,
            "",
            theme,
        ),
        Span::styled(format!("   {} ", labels[2]), Style::default().fg(theme.muted)),
        Span::styled(format!("{} ", totals.sessions), Style::default().add_modifier(Modifier::BOLD)),
        delta_span(previous.map(|previous| totals.sessions as f64 - previous.sessions), 1, "", theme),
        Span::styled(format!("   {} ", labels[3]), Style::default().fg(theme.muted)),
        Span::styled(format!("{} ", grouped(totals.keystrokes)), Style::default().add_modifier(Modifier::BOLD)),
        delta_span(previous.map(|previous| totals.keystrokes as f64 - previous.keystrokes), 0, "", theme),
    ])];
    for row in &totals.languages {
        let before = previous.and_then(|previous| previous.language(row.language));
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", pad(language_name(row.language, lang), 9)),
                Style::default().fg(language_color(row.language)).add_modifier(Modifier::BOLD),
            ),
            Span::styled("net WPM ", Style::default().fg(theme.muted)),
            Span::styled(format!("{:.1} ", row.median_wpm), Style::default().add_modifier(Modifier::BOLD)),
            padded(delta_span(before.map(|before| row.median_wpm - before.median_wpm), 1, "", theme), 7),
            Span::styled(format!(" {} ", pad(&spark_text(&row.wpms, 8), 8)), Style::default().fg(theme.accent)),
            Span::styled(
                match lang {
                    Lang::En => " accuracy ",
                    Lang::Ja => " 正確さ ",
                },
                Style::default().fg(theme.muted),
            ),
            Span::styled(
                format!("{:.1}% ", row.median_accuracy * 100.0),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            delta_span(
                before.map(|before| (row.median_accuracy - before.median_accuracy) * 100.0),
                1,
                "pt",
                theme,
            ),
        ]));
    }
    lines
}

pub fn language_color(language: Lang) -> Color {
    match language {
        Lang::En => FORECAST_EN_COLOR,
        Lang::Ja => FORECAST_JA_COLOR,
    }
}

// ---------------------------------------------------------------------------
// Image specs
// ---------------------------------------------------------------------------

pub fn image_colors(theme: &Theme) -> ResultImageColors {
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

/// The graduation as the image draws it (after a drawn ▲; the image font has
/// neither circled digits nor arrows).
fn graduation_label(graduation: Option<Graduation>, lang: Lang) -> Option<String> {
    match (graduation?, lang) {
        (Graduation::Level(level), Lang::Ja) => Some(format!("レベル {level} へ")),
        (Graduation::Level(level), Lang::En) => Some(format!("level {level}")),
        (Graduation::Maintenance, Lang::Ja) => Some("維持へ".to_owned()),
        (Graduation::Maintenance, Lang::En) => Some("maintenance".to_owned()),
    }
}

/// The lines of the steps figure for `runs` (one heading and one row per step).
pub fn steps_lines(runs: &[TodayRun], lang: Lang) -> Vec<TodayStepsLine> {
    let mut lines = Vec::new();
    for run in runs {
        // The image font has no dash; a slash separates the heading instead.
        lines.push(TodayStepsLine::Run(run_heading(run, lang).replace(" — ", " / ").replace("）— ", "） / ")));
        for step in &run.steps.steps {
            let title = match step.level {
                Some(level) => format!("{} Lv{level}", step.title),
                None => step.title.clone(),
            };
            let (met, required) = match graduation_progress(step) {
                Some((met, required)) => (met, Some(required)),
                None => (0, None),
            };
            lines.push(TodayStepsLine::Step(TodayStepRow {
                label: format!(
                    "{} {} {}",
                    step.index + 1,
                    role_label(step.role, lang),
                    // Every step carries its language, as in the text version.
                    practice_language_label(step.language, lang)
                )
                .trim_end()
                .to_owned(),
                title,
                minutes: step.minutes.max(1),
                done: step.done,
                value: step_value_text(step, lang),
                difference: step_difference(step),
                required,
                met,
                graduation: graduation_label(step.graduation, lang),
            }));
        }
    }
    lines
}

pub fn timeline_spec(data: &TodayData, lang: Lang, theme: &Theme, cells: Size) -> TodayTimelineSpec {
    let minute_of = |time: DateTime<Local>| f64::from(time.hour()) * 60.0 + f64::from(time.minute()) + f64::from(time.second()) / 60.0;
    let points = data
        .sessions
        .iter()
        .map(|session| {
            let start = minute_of(session.started_at);
            TodayTimelinePoint {
                start_minute: start,
                end_minute: (start + minutes(session.duration_ms)).min(24.0 * 60.0),
                wpm: session.net_wpm,
                accuracy: session.accuracy,
                color: language_color(session.language),
                shape: match session.kind {
                    TodaySessionKind::Menu { .. } => TodayPointShape::Menu,
                    TodaySessionKind::Bench(_) => TodayPointShape::Bench,
                    TodaySessionKind::Individual => TodayPointShape::Individual,
                },
            }
        })
        .collect();
    let languages = data
        .totals
        .as_ref()
        .map(|totals| {
            totals
                .languages
                .iter()
                .map(|row| (language_name(row.language, lang).to_owned(), language_color(row.language)))
                .collect()
        })
        .unwrap_or_default();
    let (shape_labels, size_note) = match lang {
        Lang::En => (
            ["menu step".to_owned(), "individual".to_owned(), "bench".to_owned()],
            "size, label = accuracy".to_owned(),
        ),
        Lang::Ja => (
            ["メニューの段".to_owned(), "個別".to_owned(), "ベンチ".to_owned()],
            "大きさ・数字＝正確さ".to_owned(),
        ),
    };
    TodayTimelineSpec {
        points,
        languages,
        shape_labels,
        size_note,
        cells,
        colors: image_colors(theme),
    }
}

/// The big-number cards: time, sessions, keystrokes, then net WPM and
/// accuracy for each language practised today.
pub fn card_specs(data: &TodayData, lang: Lang, theme: &Theme, cells: &[Size]) -> Vec<TodayCardSpec> {
    let Some(totals) = &data.totals else {
        return Vec::new();
    };
    let previous = data.baseline.as_ref();
    let delta_text = |delta: Option<f64>, digits: usize, suffix: &str| previous_delta_text(data, delta, digits, suffix, lang);
    let mut rows = vec![
        (
            match lang {
                Lang::En => "Time (min)".to_owned(),
                Lang::Ja => "練習時間（分）".to_owned(),
            },
            format!("{:.1}", minutes(totals.duration_ms)),
            previous.map(|previous| minutes(totals.duration_ms) - previous.duration_ms / 60_000.0),
            1,
            "",
        ),
        (
            match lang {
                Lang::En => "Sessions".to_owned(),
                Lang::Ja => "回数".to_owned(),
            },
            totals.sessions.to_string(),
            previous.map(|previous| totals.sessions as f64 - previous.sessions),
            1,
            "",
        ),
        (
            match lang {
                Lang::En => "Keystrokes".to_owned(),
                Lang::Ja => "打鍵数".to_owned(),
            },
            grouped(totals.keystrokes),
            previous.map(|previous| totals.keystrokes as f64 - previous.keystrokes),
            0,
            "",
        ),
    ];
    for row in &totals.languages {
        let before = previous.and_then(|previous| previous.language(row.language));
        let name = language_name(row.language, lang);
        rows.push((
            format!("net WPM {name}"),
            format!("{:.1}", row.median_wpm),
            before.map(|before| row.median_wpm - before.median_wpm),
            1,
            "",
        ));
        rows.push((
            match lang {
                Lang::En => format!("Accuracy {name}"),
                Lang::Ja => format!("正確さ {name}"),
            },
            format!("{:.1}%", row.median_accuracy * 100.0),
            before.map(|before| (row.median_accuracy - before.median_accuracy) * 100.0),
            1,
            "pt",
        ));
    }
    rows.into_iter()
        .zip(cells.iter().copied())
        .map(|((label, value, delta, digits, suffix), cells)| TodayCardSpec {
            label,
            value,
            delta_text: delta_text(delta, digits, suffix),
            delta: delta.map(|delta| round_to(delta, digits)),
            cells,
            colors: image_colors(theme),
        })
        .collect()
}

/// «過去 7 日平均比 +5.0» / «vs 7-day avg +5.0» against the 7-day average,
/// «過去 7 日の記録なし» without a value to compare with.
pub(crate) fn previous_delta_text(_data: &TodayData, delta: Option<f64>, digits: usize, suffix: &str, lang: Lang) -> String {
    match (delta, lang) {
        (Some(delta), Lang::Ja) => format!("過去 7 日平均比 {}", signed(delta, digits, suffix)),
        (Some(delta), Lang::En) => format!("vs 7-day avg {}", signed(delta, digits, suffix)),
        (None, Lang::Ja) => "過去 7 日の記録なし".to_owned(),
        (None, Lang::En) => "no records in past 7 days".to_owned(),
    }
}

pub(crate) fn round_to(value: f64, digits: usize) -> f64 {
    let scale = 10_f64.powi(digits as i32);
    (value * scale).round() / scale
}

pub(crate) fn signed(delta: f64, digits: usize, suffix: &str) -> String {
    let rounded = round_to(delta, digits);
    if rounded == 0.0 {
        format!("±0{suffix}")
    } else {
        format!("{rounded:+.digits$}{suffix}")
    }
}

/// Number of big-number cards (3 + 2 per language).
pub fn card_count(data: &TodayData) -> usize {
    data.totals.as_ref().map_or(0, |totals| 3 + 2 * totals.languages.len())
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Which image layout fits `area` (the result screen's thresholds): the full
/// one needs 120×37 cells, the compact one 120×29; otherwise text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TodayLayout {
    Full,
    Compact,
    Text,
}

pub fn layout_for(app: &App, area: Rect) -> TodayLayout {
    let available = app.graphics.mode != GraphicsMode::Off && app.graphics.cell_pixels.is_some();
    if !available || app.analysis_today.totals.is_none() || area.width < 120 {
        TodayLayout::Text
    } else if area.height >= 37 {
        TodayLayout::Full
    } else if area.height >= 29 {
        TodayLayout::Compact
    } else {
        TodayLayout::Text
    }
}

pub fn render(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let layout = layout_for(app, area);
    if layout != TodayLayout::Text && render_images(frame, app, theme, area, layout == TodayLayout::Full) {
        return;
    }
    render_text(frame, app, theme, area);
}

fn render_text(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let body = crate::ui::left_inset(area);
    if body.width == 0 || body.height == 0 {
        return;
    }
    let lines = text_lines(&app.analysis_today, app.config.general.lang, theme, usize::from(body.width));
    let maximum = lines.len().saturating_sub(usize::from(body.height)).min(usize::from(u16::MAX)) as u16;
    frame.render_widget(Paragraph::new(lines).scroll((app.analysis_today_scroll.min(maximum), 0)), body);
}

/// Draw the three figures. `false` when an image could not be prepared (the
/// caller then draws the text version, as the result screen does).
fn render_images(frame: &mut Frame, app: &App, theme: &Theme, area: Rect, full: bool) -> bool {
    let data = &app.analysis_today;
    let lang = app.config.general.lang;
    let width = usize::from(area.width);
    let card_height: u16 = if full { 7 } else { 5 };
    let minimum_timeline: u16 = if full { 10 } else { 8 };
    // Rows: three section rules, the steps, the timeline, the cards.
    let room_for_steps = area.height.saturating_sub(3 + card_height + minimum_timeline);
    let mut runs = Vec::new();
    let mut step_rows: u16 = 0;
    for run in &data.runs {
        let rows = 1 + run.steps.steps.len() as u16;
        if step_rows + rows > room_for_steps {
            break;
        }
        step_rows += rows;
        runs.push(run.clone());
    }
    if !data.runs.is_empty() && runs.is_empty() {
        return false;
    }
    let omitted = data.runs.len() - runs.len();
    let steps_height = if runs.is_empty() { 1 } else { step_rows };
    let timeline_height = area.height.saturating_sub(3 + card_height + steps_height);

    let mut y = area.y;
    let rule = |label: String| Paragraph::new(section_rule(label, width, theme));
    frame.render_widget(
        rule(match (lang, omitted) {
            (Lang::En, 0) => format!("Today's training {}", data.date),
            (Lang::Ja, 0) => format!("今日の訓練 {}", data.date),
            (Lang::En, omitted) => format!("Today's training {} (+{omitted} more menu(s) in a smaller window)", data.date),
            (Lang::Ja, omitted) => format!("今日の訓練 {}（ほか {omitted} 件のメニューは文字の表示で）", data.date),
        }),
        Rect::new(area.x, y, area.width, 1),
    );
    y += 1;
    let steps_area = Rect::new(area.x, y, area.width, steps_height);
    y += steps_height;
    let individual = data.individual().count();
    frame.render_widget(
        rule(match lang {
            Lang::En => format!("Today's sessions by time — individual practice {individual}"),
            Lang::Ja => format!("今日の練習（時刻順）— 個別の練習 {individual} 回"),
        }),
        Rect::new(area.x, y, area.width, 1),
    );
    y += 1;
    let list_width = (area.width * 2 / 5).min(52);
    let timeline_area = Rect::new(area.x, y, area.width - list_width, timeline_height);
    let list_area = Rect::new(area.x + area.width - list_width + 1, y, list_width - 1, timeline_height);
    y += timeline_height;
    frame.render_widget(
        rule(match lang {
            Lang::En => format!("Today's totals — {}", baseline_label(data, lang)),
            Lang::Ja => format!("今日の合計 — {}", baseline_label(data, lang)),
        }),
        Rect::new(area.x, y, area.width, 1),
    );
    y += 1;
    let cards_area = Rect::new(area.x, y, area.width, card_height);

    let count = card_count(data).max(1) as u16;
    let card_width = cards_area.width / count;
    let card_rects = (0..count)
        .map(|index| Rect::new(cards_area.x + index * card_width, cards_area.y, card_width, cards_area.height))
        .collect::<Vec<_>>();
    let cards = card_specs(data, lang, theme, &card_rects.iter().map(|rect| rect.as_size()).collect::<Vec<_>>());
    let steps = (!runs.is_empty()).then(|| TodayStepsSpec {
        lines: steps_lines(&runs, lang),
        not_played: not_yet(lang).to_owned(),
        cells: steps_area.as_size(),
        colors: image_colors(theme),
    });
    let timeline = timeline_spec(data, lang, theme, timeline_area.as_size());

    let font = app.bigtext_images.borrow_mut().resolved_font(&app.config.display.big_font, &app.home_dir);
    {
        let mut images = app.analysis_images.borrow_mut();
        if cards.iter().any(|spec| images.prepare_today_card(&app.graphics, &font, spec).is_err())
            || steps
                .as_ref()
                .is_some_and(|spec| images.prepare_today_steps(&app.graphics, &font, spec).is_err())
            || images.prepare_today_timeline(&app.graphics, &font, &timeline).is_err()
        {
            return false;
        }
    }
    let mut images = app.analysis_images.borrow_mut();
    for (spec, rect) in cards.iter().zip(card_rects) {
        let Some(protocol) = images.today_card_protocol(&app.graphics, &font, spec) else {
            return false;
        };
        frame.render_widget(Image::new(protocol), rect);
    }
    match &steps {
        Some(spec) => {
            let Some(protocol) = images.today_steps_protocol(&app.graphics, &font, spec) else {
                return false;
            };
            frame.render_widget(Image::new(protocol), steps_area);
        }
        None => frame.render_widget(Paragraph::new(training_lines(data, lang, theme, width)), steps_area),
    }
    let Some(protocol) = images.today_timeline_protocol(&app.graphics, &font, &timeline) else {
        return false;
    };
    frame.render_widget(Image::new(protocol), timeline_area);
    drop(images);
    let list = data.individual().collect::<Vec<_>>();
    let list_lines = clip(
        individual_lines(&list, lang, theme, usize::from(list_area.width)),
        usize::from(list_area.width),
    );
    frame.render_widget(Paragraph::new(list_lines), list_area);
    true
}

/// The «today» hint shown under the tabs (the period and mode filters do not
/// apply to this tab, so they are not shown).
pub fn hint(lang: Lang) -> [&'static str; 2] {
    match lang {
        Lang::En => [
            "[Tab/←→] tab   [↑/↓] scroll   [S] Summary   [?] Terms   [Esc] back",
            "Today only: the period [P] and mode [M] filters do not apply",
        ],
        Lang::Ja => [
            "[Tab/←→] タブ   [↑/↓] スクロール   [S] Summary   [?] 用語   [Esc] 戻る",
            "今日の記録だけ: 期間 [P]・モード [M] の絞り込みは効きません",
        ],
    }
}

/// Only for tests and docs: the text of the tab for `width` cells.
pub fn plain_text(data: &TodayData, lang: Lang, width: usize) -> Vec<String> {
    text_lines(data, lang, &Theme::default(), width)
        .into_iter()
        .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect::<String>())
        .collect()
}
