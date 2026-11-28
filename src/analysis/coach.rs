//! Player-facing coaching assembled from the analysis modules.
//!
//! The TUI and `stats --coach` both use this report so their conclusions and
//! evidence counts stay in sync. Building a report is intentionally separate
//! from rendering; callers cache it until the record filter changes.

use chrono::NaiveDate;
use unicode_width::UnicodeWidthStr;

use crate::analysis::errors::{ErrorKind, ErrorKindMetric};
use crate::analysis::grades::{Component, GradeOptions, GradeSource, GradeStatus, GradesAnalysis, LanguageGrade, analyze_grades};
use crate::analysis::habits::{Habits, analyze_habits};
use crate::analysis::japanese::SpellingDecision;
use crate::analysis::metrics::AnalysisSettings;
use crate::analysis::model::{ExpectedPrompt, Prescription, TransitionMap, transition_map};
use crate::analysis::prescription::{GoalSuggestion, GoalSuggestionInput, PrescriptionEvaluation, suggest_goals};
use crate::analysis::progress::{DailyTrend, Progress, ProgressOptions, ReferencePrompts, analyze_progress};
use crate::analysis::{Diagnosis, DiagnosisOptions, diagnose};
use crate::bench::{BenchPart, generate_version};
use crate::config::{Config, DataConfig};
use crate::data::{DataCatalog, TrainingDataSource};
use crate::error::AppResult;
use crate::i18n::Lang;
use crate::layout::builtin_layout;
use crate::layout::geometry::geometry;
use crate::record::SessionRecord;

#[derive(Clone, Debug)]
pub struct CoachReport {
    pub lines: Vec<String>,
    pub grades: GradesAnalysis,
    pub progress: Progress,
    pub diagnosis: Diagnosis,
    pub habits: Habits,
    pub goals: Vec<GoalSuggestion>,
    pub selected_goal: Option<GoalSuggestion>,
    pub map: TransitionMap,
}

impl CoachReport {
    pub fn to_text(&self) -> String {
        let mut output = self.lines.join("\n");
        output.push('\n');
        output
    }
}

#[allow(clippy::too_many_arguments)]
pub fn build(
    records: &[SessionRecord],
    config: &Config,
    home: &std::path::Path,
    prescriptions: &[Prescription],
    selected_goal: Option<GoalSuggestion>,
    as_of: NaiveDate,
    lang: Lang,
) -> AppResult<CoachReport> {
    let layout = builtin_layout(&config.keyboard.layout).unwrap_or_else(|_| builtin_layout("qwerty").expect("bundled qwerty"));
    let keyboard_geometry = geometry(&config.keyboard.geometry).unwrap_or_else(|| geometry("ansi").expect("bundled ansi"));
    let catalog = DataCatalog::load(&DataConfig::default(), home)?;
    let model = AnalysisSettings {
        pause_ms: config.analysis.pause_ms,
        shrinkage_k: config.analysis.shrinkage_k,
        half_life_days: config.analysis.half_life_days,
    };
    let at = end_of_day(as_of);
    let progress = progress_with(records, config, &layout, &keyboard_geometry, &catalog, as_of);
    let grades = analyze_grades(
        records,
        &progress.forms,
        &layout,
        &keyboard_geometry,
        GradeOptions { model },
        at,
        catalog.words_en(),
        catalog.words_ja(),
    );
    let diagnosis = diagnose(
        records,
        &layout,
        &keyboard_geometry,
        DiagnosisOptions {
            model,
            min_transition_n: config.analysis.min_transition_n,
            spelling_suggestions: config.analysis.spelling_suggestions,
        },
        at,
        catalog.words_ja(),
    );
    let habits = analyze_habits(records, &progress.forms);
    let goals = goal_suggestions(records, &grades, &progress);
    let map = transition_map(&diagnosis.player_model);
    let lines = compose_lines(
        records,
        config,
        &grades,
        &progress,
        &diagnosis,
        &habits,
        prescriptions,
        selected_goal.as_ref(),
        lang,
    );
    Ok(CoachReport {
        lines,
        grades,
        progress,
        diagnosis,
        habits,
        goals,
        selected_goal,
        map,
    })
}

/// The `progress` part of [`build`] alone, without the grades, diagnosis and
/// the rest of the report. It equals `build(records, config, home, …,
/// as_of, …)?.progress` for any prescriptions, goal and language.
pub fn build_progress(records: &[SessionRecord], config: &Config, home: &std::path::Path, as_of: NaiveDate) -> AppResult<Progress> {
    let layout = builtin_layout(&config.keyboard.layout).unwrap_or_else(|_| builtin_layout("qwerty").expect("bundled qwerty"));
    let keyboard_geometry = geometry(&config.keyboard.geometry).unwrap_or_else(|| geometry("ansi").expect("bundled ansi"));
    let catalog = DataCatalog::load(&DataConfig::default(), home)?;
    Ok(progress_with(records, config, &layout, &keyboard_geometry, &catalog, as_of))
}

/// Everything the `progress` of a report depends on, so a caller can tell
/// whether a report built earlier already holds the progress it needs.
///
/// The records are identified by their ids (with multiplicity); records
/// without an id have no inputs, so they are never treated as the same.
#[derive(Clone, Debug, PartialEq)]
pub struct ProgressInputs {
    record_ids: Vec<i64>,
    layout: String,
    geometry: String,
    pause_ms: u64,
    shrinkage_k: f64,
    half_life_days: f64,
    min_transition_n: u64,
    home: std::path::PathBuf,
    as_of: NaiveDate,
}

impl ProgressInputs {
    pub fn new(records: &[SessionRecord], config: &Config, home: &std::path::Path, as_of: NaiveDate) -> Option<Self> {
        let mut record_ids = records.iter().map(|record| record.header.id).collect::<Option<Vec<_>>>()?;
        record_ids.sort_unstable();
        Some(Self {
            record_ids,
            layout: config.keyboard.layout.clone(),
            geometry: config.keyboard.geometry.clone(),
            pause_ms: config.analysis.pause_ms,
            shrinkage_k: config.analysis.shrinkage_k,
            half_life_days: config.analysis.half_life_days,
            min_transition_n: config.analysis.min_transition_n,
            home: home.to_path_buf(),
            as_of,
        })
    }
}

fn end_of_day(as_of: NaiveDate) -> chrono::DateTime<chrono::Utc> {
    as_of.and_hms_opt(23, 59, 59).expect("valid end of day").and_utc()
}

fn progress_with(
    records: &[SessionRecord],
    config: &Config,
    layout: &crate::layout::KeyboardLayout,
    keyboard_geometry: &crate::layout::geometry::KeyboardGeometry,
    catalog: &DataCatalog,
    as_of: NaiveDate,
) -> Progress {
    let model = AnalysisSettings {
        pause_ms: config.analysis.pause_ms,
        shrinkage_k: config.analysis.shrinkage_k,
        half_life_days: config.analysis.half_life_days,
    };
    let references = reference_prompts(catalog, layout, keyboard_geometry);
    analyze_progress(
        records,
        layout,
        keyboard_geometry,
        ProgressOptions {
            model,
            min_transition_n: config.analysis.min_transition_n,
        },
        end_of_day(as_of),
        &references,
    )
}

fn reference_prompts(
    catalog: &DataCatalog,
    layout: &crate::layout::KeyboardLayout,
    keyboard_geometry: &crate::layout::geometry::KeyboardGeometry,
) -> ReferencePrompts {
    let version = |part| generate_version(part, 1, catalog, layout, keyboard_geometry);
    ReferencePrompts {
        english: version(BenchPart::A)
            .prompts
            .into_iter()
            .map(|prompt| ExpectedPrompt::english(prompt.target))
            .collect(),
        japanese: version(BenchPart::C)
            .prompts
            .into_iter()
            .filter_map(|prompt| prompt.reading.map(ExpectedPrompt::japanese))
            .collect(),
    }
}

fn goal_suggestions(records: &[SessionRecord], grades: &GradesAnalysis, progress: &Progress) -> Vec<GoalSuggestion> {
    let recent_wpm = if progress.forms.is_empty() {
        records.iter().map(|record| record.header.net_wpm).collect()
    } else {
        progress.forms.iter().map(|form| form.corrected_wpm).collect()
    };
    let preferred = grades
        .languages
        .iter()
        .find(|row| row.language == "en")
        .or_else(|| grades.languages.first());
    let next_grade_wpm = preferred.and_then(|grade| {
        grade
            .components
            .iter()
            .find(|row| row.component == Component::Speed)
            .and_then(|row| row.next_threshold)
    });
    let weak_accuracy = records
        .iter()
        .rev()
        .take(3)
        .map(|record| record.header.accuracy)
        .reduce(|left, right| left.min(right));
    suggest_goals(&GoalSuggestionInput {
        recent_wpm,
        next_grade_wpm,
        weak_accuracy,
    })
}

fn goal_remaining(goal: &GoalSuggestion, records: &[SessionRecord], progress: &Progress) -> Option<f64> {
    let current = if goal.kind == "accuracy" {
        records.last().map(|record| record.header.accuracy)
    } else {
        progress
            .forms
            .last()
            .map(|form| form.corrected_wpm)
            .or_else(|| records.last().map(|record| record.header.net_wpm))
    }?;
    Some(goal.value - current)
}

#[allow(clippy::too_many_arguments)]
fn compose_lines(
    records: &[SessionRecord],
    config: &Config,
    grades: &GradesAnalysis,
    progress: &Progress,
    diagnosis: &Diagnosis,
    habits: &Habits,
    prescriptions: &[Prescription],
    selected_goal: Option<&GoalSuggestion>,
    lang: Lang,
) -> Vec<String> {
    let mut lines = Vec::new();
    section(&mut lines, lang, "WHERE YOU ARE NOW", "今の位置");
    if grades.languages.is_empty() {
        lines.push(local(
            lang,
            "Your grade will appear after a complete practice record.",
            "記録が溜まると出ます．段階は完了した練習の記録から計算します．",
        ));
    } else {
        for grade in &grades.languages {
            lines.extend(grade_lines(grade, lang));
        }
    }
    for change in &grades.changes {
        let movement = match (lang, change.kind) {
            (Lang::En, crate::analysis::grades::GradeChangeKind::Promotion) => "promoted",
            (Lang::En, crate::analysis::grades::GradeChangeKind::Demotion) => "moved down",
            (Lang::Ja, crate::analysis::grades::GradeChangeKind::Promotion) => "昇格",
            (Lang::Ja, crate::analysis::grades::GradeChangeKind::Demotion) => "降格",
        };
        lines.push(format!(
            "  {}: {} → {} {movement}",
            language_label(&change.language, lang),
            change.from.as_str(),
            change.to.as_str()
        ));
    }
    for trend in &progress.trends {
        if let Some(line) = trend_line(trend, lang) {
            lines.push(line);
        }
    }
    if let Some(form) = progress.forms.last() {
        lines.push(match lang {
            Lang::En => format!(
                "  Difficulty-adjusted form: {:+.1}% (coverage {:.0}%)",
                (form.form - 1.0) * 100.0,
                form.coverage * 100.0
            ),
            Lang::Ja => format!(
                "  難しさを補正した調子: {:+.1}%（被覆率 {:.0}%）",
                (form.form - 1.0) * 100.0,
                form.coverage * 100.0
            ),
        });
    } else {
        lines.push(local(
            lang,
            "  Difficulty-adjusted form: more records are needed (300 prior keystrokes and 50% transition coverage).",
            "  難しさを補正した調子: 記録が溜まると出ます（事前 300 打鍵，被覆率 50% が必要）．",
        ));
    }
    if let Some(goal) = selected_goal {
        let remaining = goal_remaining(goal, records, progress).map_or_else(String::new, |gap| match (lang, goal.kind.as_str()) {
            (Lang::En, "accuracy") => format!(" (remaining {:+.1} pt)", gap * 100.0),
            (Lang::Ja, "accuracy") => format!("（あと {:+.1} pt）", gap * 100.0),
            (Lang::En, _) => format!(" (remaining {gap:+.1} WPM)"),
            (Lang::Ja, _) => format!("（あと {gap:+.1} WPM）"),
        });
        lines.push(format!(
            "  {}: {}{remaining}",
            local_ref(lang, "Selected goal", "選んだ目標"),
            goal_label(goal, lang)
        ));
    } else {
        lines.push(local(
            lang,
            "  [T] Choose a goal from your recent records.",
            "  [T] 記録から目標の候補を選ぶ．",
        ));
    }

    lines.push(String::new());
    section(&mut lines, lang, "LAST PRESCRIPTION", "前回の処方の結果");
    if let Some(row) = prescriptions.iter().max_by_key(|row| row.created_at) {
        lines.extend(prescription_lines(row, lang));
    } else {
        lines.push(local(
            lang,
            "Practice with adaptive and I will check whether it transfers to real words.",
            "adaptive で練習すると，実在語へ移った効果を確かめて報告します．",
        ));
    }

    lines.push(String::new());
    section(&mut lines, lang, "BEST OPPORTUNITIES", "いちばんの伸びしろ");
    let opportunities = opportunities(config, grades, diagnosis, lang);
    if opportunities.is_empty() {
        lines.push(local(
            lang,
            "There is not enough evidence yet. Counts and assumptions will be shown when available.",
            "まだ根拠となる件数が足りません．件数と仮定を添えて示せるまで待ちます．",
        ));
    } else {
        lines.extend(opportunities.into_iter().map(|line| format!("  • {line}")));
    }

    lines.push(String::new());
    section(&mut lines, lang, "HABITS", "習慣");
    let habit_lines = habit_lines(habits, lang);
    if habit_lines.is_empty() {
        lines.push(local(lang, "No stable habit pattern yet.", "まだ判定できる習慣の傾向はありません．"));
    } else {
        lines.extend(habit_lines.into_iter().map(|line| format!("  • {line}")));
    }

    lines.push(String::new());
    section(&mut lines, lang, "DATA STATUS", "データの状態");
    lines.push(format!(
        "{}: {}{}{}: {}",
        local_ref(lang, "sessions", "セッション"),
        records.len(),
        local_ref(lang, ", ", "，"),
        local_ref(lang, "logged keystrokes", "記録済み打鍵"),
        records.iter().map(|record| record.log.keystrokes.len()).sum::<usize>()
    ));
    if grades.languages.iter().all(|row| row.status != GradeStatus::Official) {
        lines.push(local(
            lang,
            "Run benchmark A (English) or C (Japanese) again for an official grade.",
            "正式な段階には，英語はベンチ A，日本語はベンチ C を再測定してください．",
        ));
    }
    lines.push(local(
        lang,
        "Same-finger findings assume the standard fingering for the selected geometry.",
        "同じ指の判定は，選んだ geometry の標準の運指を仮定した推定です．",
    ));
    lines
}

fn trend_line(trend: &DailyTrend, lang: Lang) -> Option<String> {
    let mut values = Vec::new();
    if let Some(slope) = trend.speed_slope_per_week {
        values.push(format!(
            "{} {:+.1} WPM{}",
            local_ref(lang, "speed", "速さ"),
            slope * 4.0,
            local_ref(lang, "/4 wk", "/4 週")
        ));
    }
    if let Some(slope) = trend.accuracy_slope_per_week {
        values.push(format!(
            "{} {:+.1} pt{}",
            local_ref(lang, "accuracy", "正確さ"),
            slope * 4.0 * 100.0,
            local_ref(lang, "/4 wk", "/4 週")
        ));
    }
    (!values.is_empty()).then(|| format!("  {}: {}", language_label(&trend.target, lang), values.join(local_ref(lang, ", ", "，"))))
}

fn grade_lines(grade: &LanguageGrade, lang: Lang) -> Vec<String> {
    let source = source_label(grade, lang);
    let mut lines = vec![match &grade.overall {
        Some(overall) => match lang {
            Lang::En => format!(
                "{}: {}  {:.2} ({}; {source}, {} session{})",
                language_label(&grade.language, lang),
                overall.grade.as_str(),
                overall.score,
                grade.version,
                grade.n_sessions,
                if grade.n_sessions == 1 { "" } else { "s" }
            ),
            Lang::Ja => format!(
                "{}: {}  {:.2}（{}・{source}・{} 回）",
                language_label(&grade.language, lang),
                overall.grade.as_str(),
                overall.score,
                grade.version,
                grade.n_sessions
            ),
        },
        None => match lang {
            Lang::En => format!(
                "{}: speed or accuracy cannot be measured ({}; {source})",
                language_label(&grade.language, lang),
                grade.version
            ),
            Lang::Ja => format!(
                "{}: 速さまたは正確さを測定できません（{}・{source}）",
                language_label(&grade.language, lang),
                grade.version
            ),
        },
    }];
    lines.extend(formatted_component_lines(grade, lang, false));
    lines
}

pub(crate) fn formatted_component_lines(grade: &LanguageGrade, lang: Lang, include_missing: bool) -> Vec<String> {
    let components = if include_missing {
        Component::all().to_vec()
    } else {
        grade.components.iter().map(|row| row.component).collect()
    };
    let name_width = components
        .iter()
        .map(|component| UnicodeWidthStr::width(component_label(*component, lang)))
        .max()
        .unwrap_or(0);
    let value_width = grade
        .components
        .iter()
        .filter(|row| components.contains(&row.component))
        .map(|row| UnicodeWidthStr::width(format_component_number(row.component, row.value, false).as_str()))
        .max()
        .unwrap_or(0);
    let unit_width = grade
        .components
        .iter()
        .filter(|row| components.contains(&row.component))
        .map(|row| UnicodeWidthStr::width(row.unit.as_str()))
        .max()
        .unwrap_or(0);

    components
        .into_iter()
        .map(|component| {
            let name = pad_display_right(component_label(component, lang), name_width);
            let Some(row) = grade.components.iter().find(|row| row.component == component) else {
                let unavailable = match (component, lang) {
                    (Component::Overlap, Lang::Ja) => "測定できない（離鍵を送る端末で計測）",
                    (Component::Overlap, Lang::En) => "not measurable (requires key-release events)",
                    (_, Lang::Ja) => "測定できない",
                    (_, Lang::En) => "not measurable",
                };
                return format!("  {name}  {unavailable}");
            };
            let value = pad_display_left(&format_component_number(row.component, row.value, false), value_width);
            let unit = pad_display_right(&row.unit, unit_width);
            let mut line = format!("  {name}  {}  {value}", row.grade.as_str());
            if unit_width > 0 {
                line.push(' ');
                line.push_str(&unit);
            }
            if let Some((next, gap)) = row.next_grade.zip(row.gap) {
                let difference = format_component_measure(row.component, gap, &row.unit, true);
                let gap = match lang {
                    Lang::En => format!("{difference} to {}", next.as_str()),
                    Lang::Ja => format!("{} まで {difference}", next.as_str()),
                };
                line.push_str("  ");
                line.push_str(&gap);
            }
            line.trim_end().to_owned()
        })
        .collect()
}

fn pad_display_left(value: &str, width: usize) -> String {
    format!("{}{value}", " ".repeat(width.saturating_sub(UnicodeWidthStr::width(value))))
}

fn pad_display_right(value: &str, width: usize) -> String {
    format!("{value}{}", " ".repeat(width.saturating_sub(UnicodeWidthStr::width(value))))
}

fn format_component_number(component: Component, value: f64, signed: bool) -> String {
    match (component, signed) {
        (Component::Rhythm | Component::Preview, true) => format!("{value:+.2}"),
        (Component::Rhythm | Component::Preview, false) => format!("{value:.2}"),
        (Component::Recovery, true) => format!("{value:+.0}"),
        (Component::Recovery, false) => format!("{value:.0}"),
        (_, true) => format!("{value:+.1}"),
        (_, false) => format!("{value:.1}"),
    }
}

fn format_component_measure(component: Component, value: f64, unit: &str, signed: bool) -> String {
    let number = format_component_number(component, value, signed);
    if unit.is_empty() { number } else { format!("{number} {unit}") }
}

fn source_label(grade: &LanguageGrade, lang: Lang) -> &'static str {
    match (grade.source, grade.status, lang) {
        (GradeSource::Bench, GradeStatus::Official, Lang::En) => "official",
        (GradeSource::Bench, GradeStatus::Official, Lang::Ja) => "正式",
        (GradeSource::Bench, GradeStatus::Interim, Lang::En) => "interim",
        (GradeSource::Bench, GradeStatus::Interim, Lang::Ja) => "暫定",
        (GradeSource::ProvisionalUncorrected, _, Lang::En) => "provisional, difficulty uncorrected",
        (GradeSource::ProvisionalUncorrected, _, Lang::Ja) => "仮・難しさ未補正",
        (_, _, Lang::En) => "provisional",
        (_, _, Lang::Ja) => "仮",
    }
}

fn prescription_lines(row: &Prescription, lang: Lang) -> Vec<String> {
    let target = match (
        row.target.get("from").and_then(serde_json::Value::as_str),
        row.target.get("to").and_then(serde_json::Value::as_str),
    ) {
        (Some(from), Some(to)) => format!("{from}→{to}"),
        _ => row.target.to_string(),
    };
    let result = row
        .result
        .clone()
        .and_then(|value| serde_json::from_value::<PrescriptionEvaluation>(value).ok());
    if let Some(result) = result {
        vec![match lang {
            Lang::En => format!(
                "{target}: {}, target {:+.1} ms, controls {:+.1} ms (z={:.2})",
                verdict_label(result.verdict.as_str(), lang),
                result.target_delta_ms,
                result.control_delta_ms,
                result.z
            ),
            Lang::Ja => format!(
                "{target}: {}，目標 {:+.1} ms，対照 {:+.1} ms（z={:.2}）",
                verdict_label(result.verdict.as_str(), lang),
                result.target_delta_ms,
                result.control_delta_ms,
                result.z
            ),
        }]
    } else {
        vec![format!("{target}: {}", verdict_label(&row.status, lang))]
    }
}

fn opportunities(config: &Config, grades: &GradesAnalysis, diagnosis: &Diagnosis, lang: Lang) -> Vec<String> {
    let mut result = Vec::new();
    if let Some(component) = grades
        .languages
        .iter()
        .filter_map(|grade| grade.overall.as_ref().and_then(|overall| overall.easiest_component))
        .next()
    {
        result.push(match lang {
            Lang::En => format!(
                "Best component to improve overall: {}. {}",
                component_label(component, lang),
                practice_for_component(component, lang)
            ),
            Lang::Ja => format!(
                "総合を上げやすい構成: {}．{}",
                component_label(component, lang),
                practice_for_component(component, lang)
            ),
        });
    }
    if let Some((row, is_reference)) = top_coachable_error(&diagnosis.errors.kinds) {
        let evidence = match (lang, is_reference) {
            (Lang::En, true) => format!("n={}, reference", row.n),
            (Lang::Ja, true) => format!("n={}，参考", row.n),
            (_, false) => format!("n={}", row.n),
        };
        result.push(match lang {
            Lang::En => format!(
                "{} {:.0}% ({}). {}",
                error_label(row.kind, lang),
                row.proportion * 100.0,
                evidence,
                error_advice(row.kind, lang).expect("coachable errors have advice")
            ),
            Lang::Ja => format!(
                "{} {:.0}%（{}）．{}",
                error_label(row.kind, lang),
                row.proportion * 100.0,
                evidence,
                error_advice(row.kind, lang).expect("coachable errors have advice")
            ),
        });
    }
    if let Some(row) = diagnosis
        .player_model
        .transitions
        .iter()
        .filter(|row| row.effective_n >= config.analysis.min_transition_n as f64)
        .max_by(|left, right| left.relative_iki.total_cmp(&right.relative_iki))
    {
        result.push(match lang {
            Lang::En => format!(
                "{}→{}: {:+.0} ms {} (n≈{:.0}, {}). {}",
                row.from,
                row.to,
                row.estimated_iki_ms - diagnosis.player_model.personal_iki_median_ms,
                local_ref(lang, "from your median", "本人の中央値より遅い"),
                row.effective_n,
                local_ref(lang, "standard fingering assumed", "標準の運指を仮定"),
                local_ref(lang, "Try adaptive practice.", "adaptive で重点練習する．")
            ),
            Lang::Ja => format!(
                "{}→{}: {:+.0} ms {}（n≈{:.0}，{}）．{}",
                row.from,
                row.to,
                row.estimated_iki_ms - diagnosis.player_model.personal_iki_median_ms,
                local_ref(lang, "from your median", "本人の中央値より遅い"),
                row.effective_n,
                local_ref(lang, "standard fingering assumed", "標準の運指を仮定"),
                local_ref(lang, "Try adaptive practice.", "adaptive で重点練習する．")
            ),
        });
    }
    if config.analysis.spelling_suggestions
        && let Some(row) = diagnosis.japanese.spelling.iter().find(|row| row.decision == SpellingDecision::Suggested)
    {
        result.push(match lang {
            Lang::En => format!(
                "{}: {} → {} ({:.0} ms faster per use, n={}). Try it in romaji mode.",
                row.kana, row.current, row.candidate, row.savings_ms, row.contexts
            ),
            Lang::Ja => format!(
                "{}: {} → {}（{:.0} ms 短縮，n={}）．romaji で試す．",
                row.kana, row.current, row.candidate, row.savings_ms, row.contexts
            ),
        });
    }
    result.truncate(3);
    result
}

const MIN_ERROR_OPPORTUNITY_N: u64 = 20;

fn top_coachable_error(rows: &[ErrorKindMetric]) -> Option<(&ErrorKindMetric, bool)> {
    let first_errors = rows.iter().filter(|row| row.kind != ErrorKind::Consecutive).map(|row| row.n).sum::<u64>();
    let row = rows
        .iter()
        .filter(|row| row.n > 0 && error_advice(row.kind, Lang::En).is_some())
        .max_by(|left, right| {
            left.n.cmp(&right.n).then_with(|| {
                let left_non_anticipation = left.kind != ErrorKind::Anticipation;
                let right_non_anticipation = right.kind != ErrorKind::Anticipation;
                left_non_anticipation.cmp(&right_non_anticipation)
            })
        })?;
    Some((row, first_errors < MIN_ERROR_OPPORTUNITY_N))
}

pub(crate) fn error_advice(kind: ErrorKind, lang: Lang) -> Option<&'static str> {
    match (kind, lang) {
        (ErrorKind::Anticipation, Lang::En) => {
            Some("Slow down until you can confirm the next character before pressing it (set the pacer to 90% of your speed).")
        }
        (ErrorKind::Anticipation, Lang::Ja) => Some("次の文字を確かめてから押せる速さに落とす（ペーサーを本人の速さの 90%）．"),
        (ErrorKind::RepeatedPrevious, Lang::En) => {
            Some("Use ngram with repeated keys and practise releasing each key immediately after pressing it.")
        }
        (ErrorKind::RepeatedPrevious, Lang::Ja) => Some("同じキーの連打を含む ngram で，押したらすぐ離す練習．"),
        (ErrorKind::AdjacentKey, Lang::En) => Some("Set the accuracy gate to 95% and keep your fingers on the home position."),
        (ErrorKind::AdjacentKey, Lang::Ja) => Some("正確さの関門 95% で，指の位置（ホームポジション）を保つ．"),
        (ErrorKind::SameFinger, Lang::En) => Some("Use adaptive to practise the affected transition."),
        (ErrorKind::SameFinger, Lang::Ja) => Some("該当の遷移を adaptive で練習する．"),
        (ErrorKind::OppositeHand, Lang::En) => Some("Use ngram while consciously separating the left and right hands."),
        (ErrorKind::OppositeHand, Lang::Ja) => Some("左右の手を意識して ngram で練習する．"),
        _ => None,
    }
}

fn top_error(rows: &[ErrorKindMetric]) -> Option<&ErrorKindMetric> {
    rows.iter()
        .filter(|row| row.kind != ErrorKind::Consecutive && row.n > 0)
        .max_by_key(|row| row.n)
}

fn habit_lines(habits: &Habits, lang: Lang) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(value) = habits.warmup.form_difference {
        lines.push(match lang {
            Lang::En => format!(
                "After a 3-hour break: {:+.1}% (n={}+{})",
                value * 100.0,
                habits.warmup.after_break_n,
                habits.warmup.other_n
            ),
            Lang::Ja => format!(
                "3 時間以上の空きの後: {:+.1}%（n={}+{}）",
                value * 100.0,
                habits.warmup.after_break_n,
                habits.warmup.other_n
            ),
        });
    }
    if let Some(bucket) = &habits.deterioration.first_deteriorated_bucket {
        lines.push(format!(
            "{}: {}",
            local_ref(lang, "Accuracy first deteriorates after", "誤り率が上がり始める区間"),
            duration_bucket_label(bucket, lang)
        ));
    }
    if let Some(minutes) = habits.practice.plateau_minutes {
        lines.push(match lang {
            Lang::En => format!("Practice gain plateaus near: {minutes:.0} min (n={} days)", habits.practice.days.len()),
            Lang::Ja => format!("練習の伸びが頭打ちになる目安: {minutes:.0} 分（n={} 日）", habits.practice.days.len()),
        });
    }
    for row in habits.assistance.iter().filter(|row| row.removable) {
        lines.push(match lang {
            Lang::En => format!(
                "{}: may be removed (n={} / {})",
                assistance_label(row.kind, lang),
                row.with_n,
                row.without_n
            ),
            Lang::Ja => format!(
                "{}: 外してよい目安（n={} / {}）",
                assistance_label(row.kind, lang),
                row.with_n,
                row.without_n
            ),
        });
    }
    if let Some(row) = habits
        .time_of_day
        .iter()
        .max_by(|left, right| left.form_median.total_cmp(&right.form_median))
    {
        lines.push(match lang {
            Lang::En => format!(
                "Time of day: {} {:+.1}% (n={}, exploratory)",
                time_bucket_label(&row.bucket, lang),
                (row.form_median - 1.0) * 100.0,
                row.n
            ),
            Lang::Ja => format!(
                "時刻: {} {:+.1}%（n={}，参考）",
                time_bucket_label(&row.bucket, lang),
                (row.form_median - 1.0) * 100.0,
                row.n
            ),
        });
    }
    lines
}

fn duration_bucket_label(bucket: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("{} min", bucket.replace('-', "–")),
        Lang::Ja => format!("{} 分", bucket.replace('-', "〜")),
    }
}

fn assistance_label(kind: crate::analysis::habits::AssistanceKind, lang: Lang) -> &'static str {
    use crate::analysis::habits::AssistanceKind;
    match (kind, lang) {
        (AssistanceKind::Pacer, Lang::En) => "Pacer",
        (AssistanceKind::Pacer, Lang::Ja) => "ペーサー",
        (AssistanceKind::Ghost, Lang::En) => "Ghost",
        (AssistanceKind::Ghost, Lang::Ja) => "ゴースト",
        (AssistanceKind::Keyboard, Lang::En) => "Keyboard display",
        (AssistanceKind::Keyboard, Lang::Ja) => "キーボード表示",
        (AssistanceKind::Preview, Lang::En) => "Preview",
        (AssistanceKind::Preview, Lang::Ja) => "先読み",
        (AssistanceKind::Blind, Lang::En) => "Blind mode",
        (AssistanceKind::Blind, Lang::Ja) => "ブラインド",
    }
}

fn time_bucket_label(bucket: &str, lang: Lang) -> &str {
    match (bucket, lang) {
        ("morning", Lang::Ja) => "朝",
        ("afternoon", Lang::Ja) => "昼",
        ("evening", Lang::Ja) => "夕方",
        ("night", Lang::Ja) => "夜",
        _ => bucket,
    }
}

fn section(lines: &mut Vec<String>, lang: Lang, en: &str, ja: &str) {
    lines.push(format!("■ {}", local_ref(lang, en, ja)));
}

fn local(lang: Lang, en: &str, ja: &str) -> String {
    local_ref(lang, en, ja).to_owned()
}

fn local_ref<'a>(lang: Lang, en: &'a str, ja: &'a str) -> &'a str {
    match lang {
        Lang::En => en,
        Lang::Ja => ja,
    }
}

fn language_label(language: &str, lang: Lang) -> &'static str {
    match (language, lang) {
        ("ja", Lang::En) => "Japanese",
        ("ja", Lang::Ja) => "日本語",
        (_, Lang::En) => "English",
        (_, Lang::Ja) => "英語",
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
        (Component::Transition, Lang::Ja) => "遷移",
        (Component::Preview, Lang::En) => "Preview",
        (Component::Preview, Lang::Ja) => "先読み",
        (Component::Recovery, Lang::En) => "Recovery",
        (Component::Recovery, Lang::Ja) => "立ち直り",
        (Component::Overlap, Lang::En) => "Overlap",
        (Component::Overlap, Lang::Ja) => "重なり",
    }
}

fn practice_for_component(component: Component, lang: Lang) -> &'static str {
    match component {
        Component::Speed | Component::Transition => local_ref(lang, "Use adaptive with the pacer.", "adaptive をペーサー付きで練習する．"),
        Component::Accuracy | Component::Recovery => local_ref(
            lang,
            "Use words in stop mode and prioritize clean input.",
            "words を stop で使い，正確な入力を優先する．",
        ),
        Component::Rhythm => local_ref(lang, "Use rhythm mode at a comfortable BPM.", "rhythm を無理のない BPM で練習する．"),
        Component::Preview => local_ref(lang, "Use sentence mode with one preview item.", "sentence で先読み 1 を使う．"),
        Component::Overlap => local_ref(lang, "Use words mode and release each key lightly.", "words でキーを軽く離す練習をする．"),
    }
}

pub(crate) fn error_label(kind: ErrorKind, lang: Lang) -> &'static str {
    match (kind, lang) {
        (ErrorKind::Anticipation, Lang::En) => "Anticipation errors",
        (ErrorKind::Anticipation, Lang::Ja) => "先走り（次の文字を先に押す）",
        (ErrorKind::RepeatedPrevious, Lang::En) => "Repeated-previous errors",
        (ErrorKind::RepeatedPrevious, Lang::Ja) => "重ね打ち",
        (ErrorKind::AdjacentKey, Lang::En) => "Adjacent-key errors",
        (ErrorKind::AdjacentKey, Lang::Ja) => "隣のキーの誤り",
        (ErrorKind::SameFinger, Lang::En) => "Same-finger substitutions",
        (ErrorKind::SameFinger, Lang::Ja) => "同じ指の取り違え",
        (ErrorKind::OppositeHand, Lang::En) => "Opposite-hand substitutions",
        (ErrorKind::OppositeHand, Lang::Ja) => "反対の手の取り違え",
        (ErrorKind::OtherSubstitution, Lang::En) => "Other substitutions",
        (ErrorKind::OtherSubstitution, Lang::Ja) => "その他の置換",
        (ErrorKind::Insertion, Lang::En) => "Insertions",
        (ErrorKind::Insertion, Lang::Ja) => "挿入",
        (ErrorKind::Omission, Lang::En) => "Omissions",
        (ErrorKind::Omission, Lang::Ja) => "脱落",
        (ErrorKind::Transposition, Lang::En) => "Transpositions",
        (ErrorKind::Transposition, Lang::Ja) => "転置",
        (ErrorKind::Consecutive, Lang::En) => "Consecutive errors",
        (ErrorKind::Consecutive, Lang::Ja) => "連続の誤り",
    }
}

fn verdict_label(value: &str, lang: Lang) -> &'static str {
    match (value, lang) {
        ("effective", Lang::En) => "effective",
        ("effective", Lang::Ja) => "効いた",
        ("ineffective", Lang::En) => "not effective",
        ("ineffective", Lang::Ja) => "効いていない",
        ("insufficient-data", Lang::En) => "insufficient data",
        ("insufficient-data", Lang::Ja) => "データ不足",
        (_, Lang::En) => "pending",
        (_, Lang::Ja) => "判定中",
    }
}

pub fn goal_label(goal: &GoalSuggestion, lang: Lang) -> String {
    match goal.kind.as_str() {
        "accuracy" => format!("{} {:.1}%", local_ref(lang, "accuracy", "正確さ"), goal.value * 100.0),
        "next-grade" => format!("{} {:.0} WPM", local_ref(lang, "next grade", "次の段階"), goal.value),
        _ => format!("{} {:.0} WPM", local_ref(lang, "recent p75", "直近 p75"), goal.value),
    }
}

/// Short grade summary for the mode-selection screen.
pub fn compact_grades(grades: &GradesAnalysis, lang: Lang) -> Option<String> {
    let labels = grades
        .languages
        .iter()
        .filter_map(|grade| {
            let overall = grade.overall.as_ref()?;
            let language = match (grade.language.as_str(), lang) {
                ("ja", Lang::En) => "JP",
                ("ja", Lang::Ja) => "日",
                (_, Lang::En) => "EN",
                (_, Lang::Ja) => "英",
            };
            let provisional = match (grade.source, grade.status, lang) {
                (GradeSource::Bench, GradeStatus::Official, _) => "",
                (GradeSource::Bench, GradeStatus::Interim, Lang::En) => " (interim)",
                (GradeSource::Bench, GradeStatus::Interim, Lang::Ja) => "（暫定）",
                (GradeSource::ProvisionalUncorrected, _, Lang::En) => " (provisional, uncorrected)",
                (GradeSource::ProvisionalUncorrected, _, Lang::Ja) => "（仮・難しさ未補正）",
                (_, _, Lang::En) => " (provisional)",
                (_, _, Lang::Ja) => "（仮）",
            };
            Some(format!("{language} {}{provisional}", overall.grade.as_str()))
        })
        .collect::<Vec<_>>();
    (!labels.is_empty()).then(|| labels.join(" · "))
}

/// Grade and latest promotion/demotion shown after a benchmark session.
pub fn benchmark_grade_line(grades: &GradesAnalysis, language: &str, lang: Lang) -> Option<String> {
    let grade = grades.languages.iter().find(|grade| grade.language == language)?;
    let overall = grade.overall.as_ref()?;
    let mut line = match lang {
        Lang::En => format!(
            "{} {} ({})",
            language_label(language, lang),
            overall.grade.as_str(),
            source_label(grade, lang)
        ),
        Lang::Ja => format!(
            "{} {}（{}）",
            language_label(language, lang),
            overall.grade.as_str(),
            source_label(grade, lang)
        ),
    };
    if let Some(change) = grades
        .changes
        .iter()
        .find(|change| change.language == language && change.component == "overall")
    {
        let movement = match (change.kind, lang) {
            (crate::analysis::grades::GradeChangeKind::Promotion, Lang::En) => "promoted",
            (crate::analysis::grades::GradeChangeKind::Promotion, Lang::Ja) => "に昇格",
            (crate::analysis::grades::GradeChangeKind::Demotion, Lang::En) => "moved down",
            (crate::analysis::grades::GradeChangeKind::Demotion, Lang::Ja) => "に降格",
        };
        line.push_str(&format!("  {} → {} {movement}", change.from.as_str(), change.to.as_str()));
    }
    Some(line)
}

/// Most salient observations attributable to one completed session.
pub fn session_insights(report: &CoachReport, record: &SessionRecord, lang: Lang) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(error) = top_error(&report.diagnosis.errors.kinds) {
        lines.push(match lang {
            Lang::En => format!("{}: {:.0}% (n={})", error_label(error.kind, lang), error.proportion * 100.0, error.n),
            Lang::Ja => format!("{}: {:.0}%（n={}）", error_label(error.kind, lang), error.proportion * 100.0, error.n),
        });
    }
    if let Some(slowdown) = &report.diagnosis.errors.post_error_slowdown {
        lines.push(match lang {
            Lang::En => format!("The next 1–3 keys after an error: {:+.0} ms (n={})", slowdown.excess_ms, slowdown.post_n),
            Lang::Ja => format!("誤った後の 1〜3 打: {:+.0} ms（n={}）", slowdown.excess_ms, slowdown.post_n),
        });
    }
    if let Some(form) = report
        .progress
        .forms
        .iter()
        .find(|form| record.header.id.is_some() && form.session_id == record.header.id)
    {
        lines.push(match lang {
            Lang::En => format!(
                "Difficulty-adjusted form: {:+.0}% (coverage {:.0}%)",
                (form.form - 1.0) * 100.0,
                form.coverage * 100.0
            ),
            Lang::Ja => format!(
                "難しさを補正した調子: {:+.0}%（被覆率 {:.0}%）",
                (form.form - 1.0) * 100.0,
                form.coverage * 100.0
            ),
        });
    }
    lines.truncate(3);
    lines
}

pub fn form_insight(report: &CoachReport, record: &SessionRecord, lang: Lang) -> Option<String> {
    let form = report
        .progress
        .forms
        .iter()
        .find(|form| record.header.id.is_some() && form.session_id == record.header.id)?;
    Some(match lang {
        Lang::En => format!(
            "Difficulty-adjusted form: {:+.0}% (coverage {:.0}%)",
            (form.form - 1.0) * 100.0,
            form.coverage * 100.0
        ),
        Lang::Ja => format!(
            "難しさを補正した調子: {:+.0}%（被覆率 {:.0}%）",
            (form.form - 1.0) * 100.0,
            form.coverage * 100.0
        ),
    })
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use unicode_width::UnicodeWidthStr;

    use super::*;
    use crate::analysis::errors::{ErrorDiagnosis, ErrorKindMetric};
    use crate::analysis::grades::{ComponentGrade, GradeSource, GradeStatus, Level};
    use crate::analysis::habits::{AssistanceEffect, AssistanceKind, DailyPractice, TimeOfDay};
    use crate::analysis::model::{ModelView, TransitionEstimate, build_player_model};
    use crate::analysis::style::TypingStyleDiagnosis;
    use crate::layout::builtin_layout;

    fn grade(component: Component, value: f64, unit: &str, grade: Level, next: Level, gap: f64) -> LanguageGrade {
        LanguageGrade {
            language: "en".to_owned(),
            source: GradeSource::Provisional,
            status: GradeStatus::Provisional,
            n_sessions: 3,
            components: vec![ComponentGrade {
                component,
                value,
                unit: unit.to_owned(),
                grade,
                score: 2.0,
                next_grade: Some(next),
                next_threshold: Some(value + gap),
                gap: Some(gap),
                weight: 1.0,
            }],
            overall: None,
            missing: Vec::new(),
            version: "grades-v1".to_owned(),
        }
    }

    fn diagnosis(kinds: Vec<ErrorKindMetric>, transition: Option<TransitionEstimate>) -> Diagnosis {
        let layout = builtin_layout("qwerty").unwrap();
        let keyboard = geometry("ansi").unwrap();
        let at = Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap();
        let mut player_model = build_player_model(&[], &layout, &keyboard, AnalysisSettings::default(), ModelView::Recent, at);
        player_model.personal_iki_median_ms = 100.0;
        player_model.transitions.extend(transition);
        Diagnosis {
            player_model,
            errors: ErrorDiagnosis {
                kinds,
                ..ErrorDiagnosis::default()
            },
            style: TypingStyleDiagnosis::default(),
            japanese: Default::default(),
        }
    }

    #[test]
    fn component_gap_uses_localized_order_precision_and_no_empty_unit_space() {
        let speed = grade(Component::Speed, 40.0, "WPM", Level::E, Level::D, 2.0);
        let ja = grade_lines(&speed, Lang::Ja).join("\n");
        let en = grade_lines(&speed, Lang::En).join("\n");
        assert!(ja.contains("40.0 WPM"), "{ja}");
        assert!(ja.contains("D まで +2.0 WPM"), "{ja}");
        assert!(en.contains("40.0 WPM"), "{en}");
        assert!(en.contains("+2.0 WPM to D"), "{en}");

        let ratio = grade(Component::Rhythm, 1.05, "", Level::G, Level::F, -0.33);
        for line in grade_lines(&ratio, Lang::Ja) {
            assert!(!line.ends_with(' '), "trailing space: {line:?}");
        }
        assert!(grade_lines(&ratio, Lang::Ja).join("\n").contains("F まで -0.33"));
    }

    #[test]
    fn component_rows_align_by_display_width_in_both_languages() {
        let mut rows = grade(Component::Speed, 9.5, "WPM", Level::E, Level::D, 2.0);
        rows.components.extend([
            ComponentGrade {
                component: Component::Accuracy,
                value: 95.7,
                unit: "%".to_owned(),
                grade: Level::D,
                score: 3.0,
                next_grade: Some(Level::C),
                next_threshold: Some(96.0),
                gap: Some(0.3),
                weight: 2.0,
            },
            ComponentGrade {
                component: Component::Recovery,
                value: 111.0,
                unit: "ms".to_owned(),
                grade: Level::E,
                score: 2.0,
                next_grade: Some(Level::D),
                next_threshold: Some(95.0),
                gap: Some(-16.0),
                weight: 1.0,
            },
        ]);

        for lang in [Lang::Ja, Lang::En] {
            let lines = grade_lines(&rows, lang);
            let component_lines = &lines[1..];
            let grade_columns = component_lines
                .iter()
                .map(|line| {
                    let marker = if line.contains("  E  ") { "  E  " } else { "  D  " };
                    UnicodeWidthStr::width(&line[..line.find(marker).unwrap()]) + 2
                })
                .collect::<Vec<_>>();
            assert!(grade_columns.windows(2).all(|pair| pair[0] == pair[1]), "{lang:?}: {component_lines:?}");

            let values = ["9.5", "95.7", "111"];
            let value_ends = component_lines
                .iter()
                .zip(values)
                .map(|(line, value)| {
                    let start = line.find(value).unwrap();
                    UnicodeWidthStr::width(&line[..start]) + UnicodeWidthStr::width(value)
                })
                .collect::<Vec<_>>();
            assert!(value_ends.windows(2).all(|pair| pair[0] == pair[1]), "{lang:?}: {component_lines:?}");

            let next_markers = if lang == Lang::Ja {
                ["D まで", "C まで", "D まで"]
            } else {
                ["+2.0 WPM to D", "+0.3 % to C", "-16 ms to D"]
            };
            let next_columns = component_lines
                .iter()
                .zip(next_markers)
                .map(|(line, marker)| UnicodeWidthStr::width(&line[..line.find(marker).unwrap()]))
                .collect::<Vec<_>>();
            assert!(next_columns.windows(2).all(|pair| pair[0] == pair[1]), "{lang:?}: {component_lines:?}");
        }
    }

    #[test]
    fn compact_japanese_grades_use_fullwidth_parentheses() {
        let mut grade = grade(Component::Speed, 40.0, "WPM", Level::E, Level::D, 2.0);
        grade.overall = Some(crate::analysis::grades::OverallGrade {
            grade: Level::E,
            score: 2.0,
            easiest_component: None,
        });
        grade.source = GradeSource::ProvisionalUncorrected;
        let analysis = GradesAnalysis {
            languages: vec![grade],
            ..GradesAnalysis::default()
        };
        let japanese = compact_grades(&analysis, Lang::Ja).unwrap();
        assert!(japanese.contains("（仮・難しさ未補正）"), "{japanese}");
        assert!(!japanese.contains(['(', ')']), "{japanese}");

        let english = compact_grades(&analysis, Lang::En).unwrap();
        assert!(english.contains("(provisional, uncorrected)"), "{english}");
        assert!(!english.contains(['（', '）']), "{english}");
    }

    #[test]
    fn error_opportunities_use_kind_specific_advice_and_mark_small_samples() {
        let cases = [
            (ErrorKind::Anticipation, "ペーサーを本人の速さの 90%"),
            (ErrorKind::RepeatedPrevious, "押したらすぐ離す練習"),
            (ErrorKind::AdjacentKey, "正確さの関門 95%"),
            (ErrorKind::SameFinger, "該当の遷移を adaptive"),
            (ErrorKind::OppositeHand, "左右の手を意識して ngram"),
        ];
        for (kind, advice) in cases {
            let result = opportunities(
                &Config::default(),
                &GradesAnalysis::default(),
                &diagnosis(
                    vec![ErrorKindMetric {
                        kind,
                        n: 20,
                        proportion: 1.0,
                    }],
                    None,
                ),
                Lang::Ja,
            )
            .join("\n");
            assert!(result.contains(advice), "kind={kind:?}: {result}");
            assert!(!result.contains("参考"), "kind={kind:?}: {result}");
        }

        let small = opportunities(
            &Config::default(),
            &GradesAnalysis::default(),
            &diagnosis(
                vec![ErrorKindMetric {
                    kind: ErrorKind::AdjacentKey,
                    n: 19,
                    proportion: 1.0,
                }],
                None,
            ),
            Lang::Ja,
        )
        .join("\n");
        assert!(small.contains("参考"), "{small}");

        let unsupported = opportunities(
            &Config::default(),
            &GradesAnalysis::default(),
            &diagnosis(
                vec![ErrorKindMetric {
                    kind: ErrorKind::OtherSubstitution,
                    n: 20,
                    proportion: 1.0,
                }],
                None,
            ),
            Lang::Ja,
        );
        assert!(unsupported.is_empty(), "{unsupported:?}");
    }

    #[test]
    fn tied_error_counts_prefer_a_coachable_non_anticipation_kind() {
        let result = opportunities(
            &Config::default(),
            &GradesAnalysis::default(),
            &diagnosis(
                vec![
                    ErrorKindMetric {
                        kind: ErrorKind::AdjacentKey,
                        n: 20,
                        proportion: 0.5,
                    },
                    ErrorKindMetric {
                        kind: ErrorKind::Anticipation,
                        n: 20,
                        proportion: 0.5,
                    },
                ],
                None,
            ),
            Lang::Ja,
        )
        .join("\n");
        assert!(result.contains("正確さの関門 95%"), "{result}");
        assert!(!result.contains("先走り"), "{result}");
    }

    #[test]
    fn transition_opportunity_marks_effective_count_as_approximate_integer() {
        let transition = TransitionEstimate {
            from: "k".to_owned(),
            to: "u".to_owned(),
            from_hand: "right".to_owned(),
            to_hand: "right".to_owned(),
            from_finger: "middle".to_owned(),
            to_finger: "index".to_owned(),
            n: 21,
            effective_n: 16.2,
            attempts: 21,
            effective_attempts: 16.2,
            sample_iki_ms: 155.0,
            parent_iki_ms: 100.0,
            estimated_iki_ms: 155.0,
            estimated_iki_lo_ms: 140.0,
            estimated_iki_hi_ms: 170.0,
            estimated_error_rate: 0.01,
            relative_iki: 1.55,
        };
        let result = opportunities(
            &Config::default(),
            &GradesAnalysis::default(),
            &diagnosis(Vec::new(), Some(transition)),
            Lang::Ja,
        )
        .join("\n");
        assert!(result.contains("n≈16"), "{result}");
        assert!(!result.contains("n=16.2"), "{result}");
    }

    #[test]
    fn habit_wording_localizes_units_buckets_and_actual_counts() {
        let mut habits = Habits::default();
        habits.deterioration.first_deteriorated_bucket = Some("5-10".to_owned());
        habits.practice.plateau_minutes = Some(30.0);
        habits.practice.days = (0..14)
            .map(|day| DailyPractice {
                date: chrono::NaiveDate::from_ymd_opt(2026, 10, 1).unwrap() + chrono::Days::new(day),
                minutes: 30.0,
                within_day_gain: None,
                next_day_retention: None,
            })
            .collect();
        habits.assistance.push(AssistanceEffect {
            kind: AssistanceKind::Pacer,
            with_n: 5,
            without_n: 6,
            with_form_median: Some(1.0),
            without_form_median: Some(1.0),
            removable: true,
        });
        habits.time_of_day.push(TimeOfDay {
            bucket: "morning".to_owned(),
            n: 7,
            form_median: 1.02,
            exploratory: true,
        });
        let ja = habit_lines(&habits, Lang::Ja).join("\n");
        assert!(ja.contains("5〜10 分"), "{ja}");
        assert!(ja.contains("30 分（n=14 日）"), "{ja}");
        assert!(ja.contains("ペーサー: 外してよい目安（n=5 / 6）"), "{ja}");
        assert!(ja.contains("時刻: 朝 +2.0%（n=7，参考）"), "{ja}");

        let en = habit_lines(&habits, Lang::En).join("\n");
        assert!(en.contains("5–10 min"), "{en}");
        assert!(en.contains("30 min (n=14 days)") || en.contains("30 min（n=14 days）"), "{en}");
        assert!(
            en.contains("Pacer: may be removed (n=5 / 6)") || en.contains("Pacer: may be removed（n=5 / 6）"),
            "{en}"
        );
    }

    #[test]
    fn four_week_trend_labels_are_localized_and_missing_trends_have_no_line() {
        let trend = DailyTrend {
            target: "ja".to_owned(),
            source: crate::analysis::progress::DailyTrendSource::Bench,
            speed: Vec::new(),
            accuracy: Vec::new(),
            speed_slope_per_week: Some(0.5),
            accuracy_slope_per_week: Some(-0.01),
            retest_correlation: None,
        };
        assert_eq!(
            trend_line(&trend, Lang::Ja).as_deref(),
            Some("  日本語: 速さ +2.0 WPM/4 週，正確さ -4.0 pt/4 週")
        );
        assert_eq!(
            trend_line(&trend, Lang::En).as_deref(),
            Some("  Japanese: speed +2.0 WPM/4 wk, accuracy -4.0 pt/4 wk")
        );
        let missing = DailyTrend {
            speed_slope_per_week: None,
            accuracy_slope_per_week: None,
            ..trend
        };
        assert_eq!(trend_line(&missing, Lang::Ja), None);
    }
}
