//! Practice-menu summary screen (design §7.1).
//!
//! Shown when the last step (finish) of a menu run ends, and as «summary so
//! far» when the player leaves a run early with `Esc` after at least one
//! finished step. The figures are read from the M4 records (`menu_steps` and
//! the prescription states, [`run_steps_summary`]) and the M5 snapshots
//! ([`run_skill_summary`]); nothing is recomputed here.
//!
//! - Upper section: one row per planned step.
//! - Middle section: the check step's language series, then the practice
//!   overall and the official bench grade on two separate rows.
//! - Lower section: up to four changes of the day.

use chrono::NaiveDate;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui_image::Image;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::analysis::grades::{Component, Direction, GradeStatus, Level};
use crate::app::App;
use crate::config::Config;
use crate::data::TrainingDataSource;
use crate::error::AppResult;
use crate::graphics::GraphicsMode;
use crate::i18n::{Lang, Msg};
use crate::layout::KeyboardLayout;
use crate::practice::menu::progress::{RunChange, RunStepsSummary, StepSummary, run_steps_summary};
use crate::practice::menu::skills::{ComponentRunSkill, RunSkillSummary, TrendDirection, format_value, run_skill_summary, trend_label};
use crate::practice::menu::{BENCH_REMINDER_DAYS, Graduation, MenuMetric, MenuRole};
use crate::record::db;
use crate::ui::analysis_image::{GradeBadgeSpec, SparklineSpec};
use crate::ui::theme::Theme;

/// §7.1: the lower section lists at most four changes.
pub const MAX_CHANGE_LINES: usize = 4;

/// The official grade from the benchmarks (never mixed with practice values).
#[derive(Clone, Debug, PartialEq)]
pub struct BenchGradeInfo {
    pub language: String,
    pub grade: Level,
    pub status: GradeStatus,
    pub date: NaiveDate,
}

/// Everything the summary screen shows, built once when it opens.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuSummaryData {
    /// «Summary so far»: the run is not finished.
    pub partial: bool,
    pub as_of: NaiveDate,
    pub steps: RunStepsSummary,
    /// The check step's language series; `None` until the check step is done.
    pub skills: Option<RunSkillSummary>,
    pub bench: Option<BenchGradeInfo>,
    /// Date of the latest benchmark in the bench language.
    pub last_bench_date: Option<NaiveDate>,
}

/// Build the summary of `run_id`. `None` when the run does not exist or no
/// step of it has been played (nothing to summarize).
pub fn build_summary(
    db_path: &std::path::Path,
    home: &std::path::Path,
    config: &Config,
    layout: &KeyboardLayout,
    run_id: i64,
    display: Lang,
    as_of: NaiveDate,
) -> AppResult<Option<MenuSummaryData>> {
    let Some(steps) = run_steps_summary(db_path, &config.keyboard.layout, run_id, display)? else {
        return Ok(None);
    };
    if steps.completed() == 0 {
        return Ok(None);
    }
    let partial = steps.completed() < steps.steps.len();
    // The series language is the one the snapshots were stored under (the
    // check session's own language, §8.1); the plan's language only stands in
    // before any snapshot exists.
    let skills = match steps.check_language() {
        Some(language) => {
            let snapshots = db::load_skill_snapshots(db_path)?;
            let series = snapshots
                .iter()
                .find(|row| row.run_id == run_id)
                .map_or_else(|| language_code(language).to_owned(), |row| row.lang.clone());
            Some(run_skill_summary(&snapshots, run_id, steps.date, &series))
        }
        None => None,
    };
    let bench_language = steps
        .check_language()
        .or_else(|| steps.steps.iter().find(|step| step.done).map(|step| step.language))
        .map_or("en", language_code);
    let bench_records = db::load_records_filtered(db_path, None, Some("bench"))?;
    let bench_sessions = crate::analysis::grades::language_bench_records(&bench_records, bench_language);
    let last_bench_date = bench_sessions
        .last()
        .and_then(|record| chrono::DateTime::parse_from_rfc3339(&record.header.started_at).ok())
        .map(|time| time.date_naive());
    let bench = if bench_sessions.is_empty() {
        None
    } else {
        let corpus = crate::data::DataCatalog::load(&crate::config::DataConfig::default(), home)?;
        let geometry = crate::layout::geometry::geometry(&config.keyboard.geometry)
            .or_else(|| crate::layout::geometry::geometry("ansi"))
            .expect("bundled ansi geometry");
        crate::analysis::grades::bench_language_grade(
            bench_language,
            &bench_records,
            layout,
            &geometry,
            crate::analysis::grades::GradeOptions {
                model: crate::analysis::metrics::AnalysisSettings {
                    pause_ms: config.analysis.pause_ms,
                    shrinkage_k: config.analysis.shrinkage_k,
                    half_life_days: config.analysis.half_life_days,
                },
            },
            as_of.and_hms_opt(23, 59, 59).expect("valid end of day").and_utc(),
            corpus.words_en(),
            corpus.words_ja(),
        )
        .and_then(|(grade, date)| {
            Some(BenchGradeInfo {
                language: bench_language.to_owned(),
                grade: grade.overall.as_ref()?.grade,
                status: grade.status,
                date,
            })
        })
    };
    Ok(Some(MenuSummaryData {
        partial,
        as_of,
        steps,
        skills,
        bench,
        last_bench_date,
    }))
}

fn language_code(language: Lang) -> &'static str {
    match language {
        Lang::En => "en",
        Lang::Ja => "ja",
    }
}

// ---------------------------------------------------------------------------
// Text helpers
// ---------------------------------------------------------------------------

/// Whether a difference moves in the better direction (lower is better for
/// rhythm, transitions, preview and recovery).
pub fn is_improvement(difference: f64, direction: Direction) -> Option<bool> {
    if difference == 0.0 || !difference.is_finite() {
        return None;
    }
    Some(match direction {
        Direction::Higher => difference > 0.0,
        Direction::Lower => difference < 0.0,
    })
}

/// The colored difference: green ▲ when it is an improvement, red ▼ when it is
/// a decline, muted ± when unchanged — the result screen's rule, with the
/// direction taken from the component.
fn difference_span(difference: Option<f64>, direction: Direction, text: impl Fn(f64) -> String, theme: &Theme) -> Span<'static> {
    let Some(difference) = difference.filter(|value| value.is_finite()) else {
        return Span::styled("—", Style::default().fg(theme.muted));
    };
    match is_improvement(difference, direction) {
        Some(true) => Span::styled(format!("▲{}", text(difference)), Style::default().fg(theme.correct)),
        Some(false) => Span::styled(format!("▼{}", text(difference)), Style::default().fg(theme.mistype)),
        None => Span::styled(format!("±{}", text(0.0).trim_start_matches(['+', '-'])), Style::default().fg(theme.muted)),
    }
}

fn pad(value: &str, width: usize) -> String {
    let value = truncate(value, width);
    let padding = width.saturating_sub(value.width());
    format!("{value}{}", " ".repeat(padding))
}

fn truncate(value: &str, width: usize) -> String {
    if value.width() <= width {
        return value.to_owned();
    }
    let mut output = String::new();
    let mut used = 0;
    for character in value.chars() {
        let character_width = character.width().unwrap_or(0);
        if used + character_width + 1 > width {
            break;
        }
        output.push(character);
        used += character_width;
    }
    if width > 0 {
        output.push('…');
    }
    output
}

/// Pad a span's text to a display width (truncating when needed).
fn padded(span: Span<'static>, width: usize) -> Span<'static> {
    Span::styled(pad(&span.content, width), span.style)
}

fn circled(level: u8) -> String {
    match level {
        1..=20 => char::from_u32(0x2460 + u32::from(level) - 1).map_or_else(|| level.to_string(), String::from),
        _ => level.to_string(),
    }
}

fn role_label(role: MenuRole, lang: Lang) -> &'static str {
    match (role, lang) {
        (MenuRole::Warmup, Lang::En) => "Warm-up",
        (MenuRole::Warmup, Lang::Ja) => "ウォームアップ",
        (MenuRole::PrimaryA, Lang::En) => "Primary A",
        (MenuRole::PrimaryA, Lang::Ja) => "主処方 A",
        (MenuRole::PrimaryB, Lang::En) => "Primary B",
        (MenuRole::PrimaryB, Lang::Ja) => "主処方 B",
        (MenuRole::Check, Lang::En) => "Check",
        (MenuRole::Check, Lang::Ja) => "確認",
        (MenuRole::Finish, Lang::En) => "Finish",
        (MenuRole::Finish, Lang::Ja) => "仕上げ",
    }
}

fn practice_language_label(language: Lang, lang: Lang) -> &'static str {
    match (language, lang) {
        (Lang::En, Lang::En) => "EN",
        (Lang::Ja, Lang::En) => "JA",
        (Lang::En, Lang::Ja) => "英",
        (Lang::Ja, Lang::Ja) => "日",
    }
}

fn series_language_label(language: &str, lang: Lang) -> &'static str {
    match (language, lang) {
        ("ja", Lang::En) => "Japanese",
        ("ja", Lang::Ja) => "日本語",
        (_, Lang::En) => "English",
        (_, Lang::Ja) => "英語",
    }
}

fn is_pass_fail(metric: MenuMetric) -> bool {
    matches!(
        metric,
        MenuMetric::AccuracyGatePassed | MenuMetric::PacerTargetReached | MenuMetric::BlindAssistanceRemovable | MenuMetric::PrescriptionR3
    )
}

/// The step's main value (§7.1: the catalog's `advance.metric`; net WPM for
/// warm-up, check and finish).
fn step_value_text(step: &StepSummary, lang: Lang) -> String {
    let Some(value) = step.value else {
        return "—".to_owned();
    };
    match step.metric {
        None | Some(MenuMetric::NetWpm) => format!("{value:.0} WPM"),
        Some(metric) if is_pass_fail(metric) => match (value >= 1.0, lang) {
            (true, Lang::En) => "met".to_owned(),
            (false, Lang::En) => "not met".to_owned(),
            (true, Lang::Ja) => "満たした".to_owned(),
            (false, Lang::Ja) => "満たさない".to_owned(),
        },
        Some(metric) => metric_text(metric, value, false),
    }
}

fn metric_text(metric: MenuMetric, value: f64, signed: bool) -> String {
    let (scaled, digits, suffix) = match metric {
        MenuMetric::WordOnsetRatio | MenuMetric::FarHomeIkiRatio => (value, 2, ""),
        MenuMetric::WithinWordIkiCv => (value, 3, ""),
        MenuMetric::PostErrorExcessMs => (value, 0, " ms"),
        MenuMetric::RolloverRate => (value, 1, "%"),
        MenuMetric::AnticipationRate | MenuMetric::EnduranceSecondHalfSlowdown | MenuMetric::SpellingUsageRate => (value * 100.0, 0, "%"),
        _ => (value, 1, ""),
    };
    if signed {
        format!("{scaled:+.digits$}{suffix}")
    } else {
        format!("{scaled:.digits$}{suffix}")
    }
}

fn step_difference_span(step: &StepSummary, theme: &Theme) -> Span<'static> {
    if step.metric.is_some_and(is_pass_fail) {
        return Span::styled("—", Style::default().fg(theme.muted));
    }
    let metric = step.metric;
    difference_span(
        step.difference(),
        step.direction,
        |difference| match metric {
            None | Some(MenuMetric::NetWpm) => format!("{difference:+.0}"),
            Some(metric) => metric_text(metric, difference, true),
        },
        theme,
    )
}

/// «●●○ 2/3», «●●● → ② へ», «●●● → 維持».
fn graduation_text(step: &StepSummary, lang: Lang) -> String {
    let Some(required) = step.required else {
        return String::new();
    };
    let filled = step.consecutive.unwrap_or(0).min(required);
    let dots = format!(
        "{}{}",
        "●".repeat(usize::from(filled)),
        "○".repeat(usize::from(required.saturating_sub(filled)))
    );
    match (step.graduation, lang) {
        (Some(Graduation::Level(level)), Lang::Ja) => format!("{dots} → {} へ", circled(level)),
        (Some(Graduation::Level(level)), Lang::En) => format!("{dots} → Lv {level}"),
        (Some(Graduation::Maintenance), Lang::Ja) => format!("{dots} → 維持"),
        (Some(Graduation::Maintenance), Lang::En) => format!("{dots} → maint."),
        (None, _) => format!("{dots} {filled}/{required}"),
    }
}

fn change_text(change: &RunChange, lang: Lang) -> String {
    let text = change_text_without_note(change, lang);
    // §5 (MYTASK-3622): an endurance judgment is taken on the assisted step
    // itself, so it is an even weaker reference.
    let assisted = matches!(
        change,
        RunChange::Swapped { assisted: true, .. } | RunChange::Paused { assisted: true, .. } | RunChange::Improved { assisted: true, .. }
    );
    match (assisted, lang) {
        (false, _) => text,
        (true, Lang::Ja) => format!("{text}（補助つきの段の値）"),
        (true, Lang::En) => format!("{text} (from the assisted step)"),
    }
}

fn change_text_without_note(change: &RunChange, lang: Lang) -> String {
    let language = |language: Lang| practice_language_label(language, lang);
    match (change, lang) {
        (RunChange::LevelUp { title, language: l, level }, Lang::Ja) => format!("{} {title}がレベル {} へ", language(*l), circled(*level)),
        (RunChange::LevelUp { title, language: l, level }, Lang::En) => format!("{} {title} moves up to level {level}", language(*l)),
        (RunChange::LevelDown { title, language: l, level }, Lang::Ja) => format!("{} {title}がレベル {} へ戻る", language(*l), circled(*level)),
        (RunChange::LevelDown { title, language: l, level }, Lang::En) => format!("{} {title} moves back to level {level}", language(*l)),
        (RunChange::Maintenance { title, language: l }, Lang::Ja) => format!("{} {title}を卒業．維持へ", language(*l)),
        (RunChange::Maintenance { title, language: l }, Lang::En) => format!("{} {title} graduated; now in maintenance", language(*l)),
        (
            RunChange::Swapped {
                title,
                language: l,
                alternate,
                controlled,
                ..
            },
            Lang::Ja,
        ) => {
            let verdict = if *controlled {
                "効いていない（R3）"
            } else {
                "変化なし（参考．対照なし）"
            };
            format!("{} {title}: {verdict}．次回から{alternate}へ入れ替え", language(*l))
        }
        (
            RunChange::Swapped {
                title,
                language: l,
                alternate,
                controlled,
                ..
            },
            Lang::En,
        ) => {
            let verdict = if *controlled {
                "not effective (R3)"
            } else {
                "no change (reference only; no control)"
            };
            format!("{} {title}: {verdict}; switching to {alternate} next time", language(*l))
        }
        (
            RunChange::Paused {
                title, language: l, until, ..
            },
            Lang::Ja,
        ) => {
            format!("{} {title}: 変化なし（参考．対照なし）．{} まで休止", language(*l), until.format("%m/%d"))
        }
        (
            RunChange::Paused {
                title, language: l, until, ..
            },
            Lang::En,
        ) => format!(
            "{} {title}: no change (reference only; no control); resting until {}",
            language(*l),
            until.format("%m/%d")
        ),
        (
            RunChange::Improved {
                title,
                language: l,
                controlled: true,
                ..
            },
            Lang::Ja,
        ) => format!("{} {title}: 効いた（R3．対照つき）", language(*l)),
        (
            RunChange::Improved {
                title,
                language: l,
                controlled: true,
                ..
            },
            Lang::En,
        ) => format!("{} {title}: effective (R3, controlled)", language(*l)),
        (RunChange::Improved { title, language: l, .. }, Lang::Ja) => format!("{} {title}: 改善（参考．対照なし）", language(*l)),
        (RunChange::Improved { title, language: l, .. }, Lang::En) => {
            format!("{} {title}: improved (reference only; no control)", language(*l))
        }
        (RunChange::NoReference { title, language: l }, Lang::Ja) => format!("{} {title}: 参考の判定なし", language(*l)),
        (RunChange::NoReference { title, language: l }, Lang::En) => format!("{} {title}: no reference judgment", language(*l)),
    }
}

/// The lower section (§7.1): level changes, swaps and pauses, judgments,
/// changes beyond the typical change, then the bench reminder; at most
/// [`MAX_CHANGE_LINES`].
pub fn change_lines(data: &MenuSummaryData, lang: Lang) -> Vec<String> {
    let mut lines = data.steps.changes.iter().map(|change| change_text(change, lang)).collect::<Vec<_>>();
    if let Some(skills) = &data.skills {
        for row in &skills.components {
            let Some(trend) = row.trend.filter(|trend| trend.beyond_typical == Some(true)) else {
                continue;
            };
            let (Some(difference), Some(typical)) = (trend.previous_difference, trend.typical_change) else {
                continue;
            };
            let Some(improved) = is_improvement(difference, row.direction) else {
                continue;
            };
            let name = crate::analysis::stats::component_name(row.component, lang);
            let difference = format_value(row.component, difference, true);
            let typical = format_value(row.component, typical, false);
            lines.push(match (improved, lang) {
                (true, Lang::Ja) => format!("{name}: ふだんの変動を超えて改善（{difference}，ふだんの変動 ±{typical}）"),
                (false, Lang::Ja) => format!("{name}: ふだんの変動を超えて悪化（{difference}，ふだんの変動 ±{typical}）"),
                (true, Lang::En) => format!("{name}: improved beyond the typical change ({difference}; typical ±{typical})"),
                (false, Lang::En) => format!("{name}: declined beyond the typical change ({difference}; typical ±{typical})"),
            });
        }
    }
    lines.push(bench_reminder(data, lang));
    lines.truncate(MAX_CHANGE_LINES);
    lines
}

/// Uses the menu's existing `bench_recommended` decision (§4.2) and the
/// 7-day reminder interval.
fn bench_reminder(data: &MenuSummaryData, lang: Lang) -> String {
    let days = data.last_bench_date.map(|date| (data.as_of - date).num_days().max(0));
    match (days, data.steps.bench_recommended, lang) {
        (None, _, Lang::Ja) => "ベンチの記録がありません．ベンチ（約 4 分）を勧めます".to_owned(),
        (None, _, Lang::En) => "No benchmark yet. A benchmark (about 4 min) is recommended".to_owned(),
        (Some(days), true, Lang::Ja) => format!("ベンチから {days} 日．再測定（約 4 分）を勧めます"),
        (Some(days), true, Lang::En) => format!("{days} days since the benchmark. A re-measurement (about 4 min) is recommended"),
        (Some(days), false, Lang::Ja) => format!("ベンチから {days} 日．あと {} 日で再測定を勧めます", (BENCH_REMINDER_DAYS - days).max(1)),
        (Some(days), false, Lang::En) => format!(
            "{days} days since the benchmark. Re-measure in {} days",
            (BENCH_REMINDER_DAYS - days).max(1)
        ),
    }
}

const SPARK_CHARACTERS: [char; 7] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇'];

/// The 28-day line drawn with `▁▂▃▄▅▆▇` (the latest `width` values).
pub fn spark_text(values: &[f64], width: usize) -> String {
    let values = values.iter().copied().filter(|value| value.is_finite()).collect::<Vec<_>>();
    let values = &values[values.len().saturating_sub(width)..];
    if values.is_empty() {
        return String::new();
    }
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    values
        .iter()
        .map(|value| {
            if (max - min).abs() < f64::EPSILON {
                SPARK_CHARACTERS[3]
            } else {
                let index = ((value - min) / (max - min) * 6.0).round().clamp(0.0, 6.0) as usize;
                SPARK_CHARACTERS[index]
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// The image fallback follows the result screen: images need graphics, cell
/// pixels and at least 120×29 cells; the grade badge additionally needs 37
/// rows. Otherwise everything is drawn with characters.
fn image_level(app: &App, area: Rect) -> (bool, bool) {
    let available = app.graphics.mode != GraphicsMode::Off && app.graphics.cell_pixels.is_some();
    let sparklines = available && area.width >= 120 && area.height >= 29;
    (sparklines, sparklines && area.height >= 37)
}

pub fn render(frame: &mut Frame, app: &App) {
    let lang = app.config.general.lang;
    let theme = Theme::from_name(&app.config.ui.theme);
    let area = frame.area();
    let hint = match lang {
        Lang::En => " [Enter/Esc] Back to the menu ",
        Lang::Ja => " [Enter/Esc] メニューへ戻る ",
    };
    let Some(data) = app.menu_summary.as_ref() else {
        let block = Block::default()
            .borders(Borders::ALL)
            .title_bottom(Line::from(hint).style(Style::default().fg(theme.accent)));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(Paragraph::new(Msg::NoData.text(lang)), crate::ui::left_inset(inner));
        return;
    };
    let total = data.steps.steps.len();
    let done = data.steps.completed();
    let title = match (data.partial, lang) {
        (false, Lang::En) => format!(
            " TODAY'S PRACTICE {} ({} min / {done} of {total} steps) ",
            data.steps.date, data.steps.minutes
        ),
        (true, Lang::En) => format!(
            " SUMMARY SO FAR {} ({} min / {done} of {total} steps) ",
            data.steps.date, data.steps.minutes
        ),
        (false, Lang::Ja) => format!(" 今日の練習 {}（{} 分 / {total} 段中 {done} 段） ", data.steps.date, data.steps.minutes),
        (true, Lang::Ja) => format!(
            " ここまでのまとめ {}（{} 分 / {total} 段中 {done} 段） ",
            data.steps.date, data.steps.minutes
        ),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_bottom(Line::from(hint).style(Style::default().fg(theme.accent)))
        .style(Style::default().fg(theme.fg));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let body = crate::ui::left_inset(inner);
    if body.width == 0 || body.height == 0 {
        return;
    }
    let (sparkline_images, badge_image) = image_level(app, inner);
    let width = usize::from(body.width);

    let mut lines = upper_lines(data, lang, &theme, width);
    let middle_start = lines.len() + 1;
    lines.push(section_rule(
        match (lang, data.skills.as_ref()) {
            (Lang::En, Some(skills)) => format!(
                "Skills now — {} (check step; vs 7-run moving median)",
                series_language_label(&skills.language, lang)
            ),
            (Lang::Ja, Some(skills)) => format!(
                "今のスキル — {}（確認段から．7 回移動中央値と比べる）",
                series_language_label(&skills.language, lang)
            ),
            (Lang::En, None) => "Skills now".to_owned(),
            (Lang::Ja, None) => "今のスキル".to_owned(),
        },
        width,
        &theme,
    ));
    let spark_width = if sparkline_images { 16 } else { 8 };
    let (middle, component_rows) = middle_lines(data, lang, &theme, width, spark_width, sparkline_images);
    lines.extend(middle);
    lines.push(section_rule(
        match lang {
            Lang::En => "Today's changes".to_owned(),
            Lang::Ja => "今日の変化".to_owned(),
        },
        width,
        &theme,
    ));
    let room = usize::from(body.height).saturating_sub(lines.len());
    for change in change_lines(data, lang).into_iter().take(room.min(MAX_CHANGE_LINES)) {
        lines.push(Line::from(format!("・{}", truncate(&change, width.saturating_sub(2)))));
    }
    frame.render_widget(Paragraph::new(lines), body);

    // Images on top of the reserved character cells (the result screen's
    // order: images only when the whole layout fits).
    if let Some((skills, column)) = data.skills.as_ref().zip(component_rows) {
        if sparkline_images {
            let mut images = app.analysis_images.borrow_mut();
            for (offset, row) in skills.components.iter().enumerate() {
                if row.recent.len() < 2 {
                    continue;
                }
                let y = body.y + (middle_start + 1 + offset) as u16;
                if y >= body.y + body.height {
                    break;
                }
                let image_area = Rect::new(body.x + column as u16, y, spark_width as u16, 1);
                let spec = SparklineSpec {
                    values: row.recent.clone(),
                    color: theme.accent,
                    cells: image_area.as_size(),
                };
                if images.prepare_sparkline(&app.graphics, &spec).is_ok()
                    && let Some(protocol) = images.sparkline_protocol(&app.graphics, &spec)
                {
                    frame.render_widget(Image::new(protocol), image_area);
                }
            }
        }
        if badge_image && let Some(overall) = &skills.overall {
            let badge_area = Rect::new(
                body.x + body.width.saturating_sub(12),
                body.y + (middle_start + 1) as u16,
                10.min(body.width),
                6,
            );
            let spec = GradeBadgeSpec {
                level: overall.grade,
                color: theme.accent,
                cells: badge_area.as_size(),
            };
            let font = app.bigtext_images.borrow_mut().resolved_font(&app.config.display.big_font, &app.home_dir);
            let mut images = app.analysis_images.borrow_mut();
            if images.prepare_badge(&app.graphics, &font, &spec).is_ok()
                && let Some(protocol) = images.badge_protocol(&app.graphics, &font, &spec)
            {
                frame.render_widget(Image::new(protocol), badge_area);
            }
        }
    }
}

fn section_rule(label: String, width: usize, theme: &Theme) -> Line<'static> {
    let label = truncate(&format!("── {label} "), width);
    let rest = width.saturating_sub(label.width());
    Line::from(Span::styled(format!("{label}{}", "─".repeat(rest)), Style::default().fg(theme.muted)))
}

struct UpperWidths {
    role: usize,
    title: usize,
    value: usize,
    previous: usize,
    graduation: usize,
}

fn upper_widths(lang: Lang, width: usize) -> UpperWidths {
    let (role, value, previous, graduation) = match lang {
        Lang::En => (11, 9, 8, 12),
        Lang::Ja => (16, 10, 8, 12),
    };
    // 2-cell language column and five one-cell separators.
    let fixed = role + 2 + value + previous + graduation + 5;
    UpperWidths {
        role,
        title: width.saturating_sub(fixed).clamp(4, 32),
        value,
        previous,
        graduation,
    }
}

fn upper_lines(data: &MenuSummaryData, lang: Lang, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let widths = upper_widths(lang, width);
    let header = match lang {
        Lang::En => ["Step", "", "Prescription · level", "This time", "Prev", "Graduation"],
        Lang::Ja => ["段", "", "処方・レベル", "今回", "前回差", "卒業の条件"],
    };
    let mut lines = vec![Line::from(Span::styled(
        format!(
            "{} {} {} {} {} {}",
            pad(header[0], widths.role),
            pad(header[1], 2),
            pad(header[2], widths.title),
            pad(header[3], widths.value),
            pad(header[4], widths.previous),
            header[5]
        ),
        Style::default().fg(theme.muted),
    ))];
    for step in &data.steps.steps {
        let role = format!("{} {}", step.index + 1, role_label(step.role, lang));
        let title = match step.level {
            Some(level) => format!("{} {}", step.title, circled(level)),
            None => step.title.clone(),
        };
        let mut spans = vec![
            Span::raw(format!("{} ", pad(&role, widths.role))),
            Span::styled(
                format!("{} ", pad(practice_language_label(step.language, lang), 2)),
                Style::default().fg(theme.muted),
            ),
            Span::raw(format!("{} ", pad(&title, widths.title))),
        ];
        if !step.done {
            spans.push(Span::styled(
                match lang {
                    Lang::En => "not played",
                    Lang::Ja => "未実施",
                },
                Style::default().fg(theme.muted),
            ));
            lines.push(Line::from(spans));
            continue;
        }
        spans.push(Span::raw(format!("{} ", pad(&step_value_text(step, lang), widths.value))));
        spans.push(padded(step_difference_span(step, theme), widths.previous));
        spans.push(Span::raw(" "));
        let graduation = graduation_text(step, lang);
        let style = if step.graduation.is_some() {
            Style::default().fg(theme.correct).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        spans.push(Span::styled(truncate(&graduation, widths.graduation), style));
        lines.push(Line::from(spans));
    }
    lines
}

/// The middle section and, when the series is shown, the column (offset in
/// the body) where the 28-day line starts.
fn middle_lines(
    data: &MenuSummaryData,
    lang: Lang,
    theme: &Theme,
    width: usize,
    spark_width: usize,
    sparkline_images: bool,
) -> (Vec<Line<'static>>, Option<usize>) {
    let Some(skills) = &data.skills else {
        return (
            vec![Line::from(Span::styled(
                match lang {
                    Lang::En => "Shown after the check step",
                    Lang::Ja => "確認段を終えると表示",
                },
                Style::default().fg(theme.muted),
            ))],
            None,
        );
    };
    let no_data = Msg::NoData.text(lang);
    let name_width = Component::all()
        .iter()
        .map(|component| crate::analysis::stats::component_name(*component, lang).width())
        .max()
        .unwrap_or(8);
    let (value_width, difference_width, median_width, trend_width) = (12, 9, 10, 9);
    let header = match lang {
        Lang::En => ["Component", "Gr", "This time", "Prev diff", "Median (7)", "28 days", "Trend"],
        Lang::Ja => ["構成", "段", "今回", "前回差", "移動中央値", "28 日", "傾向"],
    };
    let mut lines = vec![Line::from(Span::styled(
        format!(
            "{} {} {} {} {} {} {}",
            pad(header[0], name_width),
            pad(header[1], 2),
            pad(header[2], value_width),
            pad(header[3], difference_width),
            pad(header[4], median_width),
            pad(header[5], spark_width),
            header[6]
        ),
        Style::default().fg(theme.muted),
    ))];
    let spark_column = name_width + 2 + value_width + difference_width + median_width + 5;
    for row in &skills.components {
        lines.push(component_line(
            row,
            lang,
            theme,
            [name_width, value_width, difference_width, median_width, spark_width, trend_width],
            sparkline_images,
            no_data,
        ));
    }
    let overall = match (&skills.overall, lang) {
        (Some(overall), Lang::Ja) => format!(
            "総合（練習から）: {} {:.2}（前回 {}）",
            overall.grade.as_str(),
            overall.score,
            skills
                .previous_overall
                .as_ref()
                .map_or_else(|| "—".to_owned(), |previous| format!("{} {:.2}", previous.grade.as_str(), previous.score))
        ),
        (Some(overall), Lang::En) => format!(
            "Overall (from practice): {} {:.2} (prev {})",
            overall.grade.as_str(),
            overall.score,
            skills
                .previous_overall
                .as_ref()
                .map_or_else(|| "—".to_owned(), |previous| format!("{} {:.2}", previous.grade.as_str(), previous.score))
        ),
        (None, Lang::Ja) => format!("総合（練習から）: {no_data}"),
        (None, Lang::En) => format!("Overall (from practice): {no_data}"),
    };
    lines.push(Line::from(Span::styled(truncate(&overall, width), Style::default().fg(theme.accent))));
    let official = match (&data.bench, lang) {
        (Some(bench), Lang::Ja) => format!(
            "正式の段（ベンチ {}，{}）: {}{}",
            series_language_label(&bench.language, lang),
            bench.date.format("%m/%d"),
            bench.grade.as_str(),
            if bench.status == GradeStatus::Official { "" } else { "（暫定）" }
        ),
        (Some(bench), Lang::En) => format!(
            "Official grade (bench {}, {}): {}{}",
            series_language_label(&bench.language, lang),
            bench.date.format("%m/%d"),
            bench.grade.as_str(),
            if bench.status == GradeStatus::Official { "" } else { " (interim)" }
        ),
        (None, Lang::Ja) => format!("正式の段（ベンチ）: {no_data}"),
        (None, Lang::En) => format!("Official grade (bench): {no_data}"),
    };
    lines.push(Line::from(Span::styled(
        truncate(&official, width),
        Style::default().fg(theme.warning).add_modifier(Modifier::BOLD),
    )));
    (lines, Some(spark_column))
}

fn component_line(
    row: &ComponentRunSkill,
    lang: Lang,
    theme: &Theme,
    [name_width, value_width, difference_width, median_width, spark_width, trend_width]: [usize; 6],
    sparkline_images: bool,
    no_data: &str,
) -> Line<'static> {
    let name = pad(crate::analysis::stats::component_name(row.component, lang), name_width);
    let Some(value) = row.value else {
        return Line::from(vec![
            Span::raw(format!("{name} ")),
            Span::styled(no_data.to_owned(), Style::default().fg(theme.muted)),
        ]);
    };
    let mut value_text = format_value(row.component, value, false);
    if !row.unit.is_empty() && row.unit != "ratio" {
        value_text.push(' ');
        value_text.push_str(&row.unit);
    }
    let trend = row.trend;
    let component = row.component;
    let difference = difference_span(
        trend.and_then(|trend| trend.previous_difference),
        row.direction,
        |difference| format_value(component, difference, true),
        theme,
    );
    let median = trend
        .and_then(|trend| trend.moving_median)
        .map_or_else(|| "—".to_owned(), |median| format_value(row.component, median, false));
    let spark = if sparkline_images && row.recent.len() >= 2 {
        String::new()
    } else {
        spark_text(&row.recent, spark_width)
    };
    let (trend_text, trend_color) = match trend.and_then(|trend| trend.trend) {
        Some(direction) => (
            trend_label(direction, lang).to_owned(),
            match direction {
                TrendDirection::Improving => theme.correct,
                TrendDirection::Flat => theme.muted,
                TrendDirection::Declining => theme.mistype,
            },
        ),
        None => ("—".to_owned(), theme.muted),
    };
    Line::from(vec![
        Span::raw(format!("{name} ")),
        Span::styled(
            format!("{} ", pad(row.grade.map_or("—", Level::as_str), 2)),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("{} ", pad(&value_text, value_width))),
        padded(difference, difference_width),
        Span::raw(format!(" {} ", pad(&median, median_width))),
        Span::styled(format!("{} ", pad(&spark, spark_width)), Style::default().fg(theme.accent)),
        Span::styled(pad(&trend_text, trend_width), Style::default().fg(trend_color)),
    ])
}
