use std::fmt::Write;
use std::path::Path;

use chrono::NaiveDate;

use crate::analysis::grades::{Component, GradeChangeKind, GradeOptions, GradeSource, GradeStatus, GradesAnalysis, LanguageGrade, analyze_grades};
use crate::analysis::metrics::{AnalysisSettings, analyze_records};
use crate::analysis::model::ExpectedPrompt;
use crate::analysis::progress::{ProgressOptions, ReferencePrompts, analyze_progress};
use crate::analysis::{DiagnosisOptions, diagnose};
use crate::bench::{BenchPart, generate_version};
use crate::cli::StatsCsv;
use crate::config::{Config, DataConfig};
use crate::data::{DataCatalog, TrainingDataSource};
use crate::error::AppResult;
use crate::i18n::Lang;
use crate::layout::builtin_layout;
use crate::layout::geometry::geometry;
use crate::record::SessionRecord;
use crate::record::db;

#[allow(clippy::too_many_arguments)]
pub fn render(
    path: &Path,
    home: &Path,
    config: &Config,
    csv: Option<StatsCsv>,
    summary: bool,
    grades: bool,
    coach: bool,
    since: Option<NaiveDate>,
    mode: Option<&str>,
    lang: Lang,
    as_of: NaiveDate,
) -> AppResult<String> {
    let records = db::load_records_filtered(path, since, mode)?;
    Ok(match csv {
        Some(StatsCsv::Sessions) => sessions_csv(&records),
        Some(kind) => detailed_csv(path, kind, &records, config, home, as_of)?,
        None if grades => grades_text(&grade_analysis(&records, config, home, as_of)?, lang),
        None if coach => {
            let mut prescriptions = db::load_prescriptions(path)?;
            if let Some(since) = since {
                prescriptions.retain(|row| row.created_at.date_naive() >= since);
            }
            if mode.is_some_and(|mode| mode != "adaptive") {
                prescriptions.clear();
            }
            let selected_goal = db::load_selected_goal(path)?;
            crate::analysis::coach::build(&records, config, home, &prescriptions, selected_goal, as_of, lang)?.to_text()
        }
        None if summary => crate::analysis::summary::generate(&records, config, as_of, lang).to_text(),
        None => overview(&records),
    })
}

fn overview(records: &[SessionRecord]) -> String {
    let mut output = String::from("Sessions  Net WPM  Accuracy\n");
    if records.is_empty() {
        output.push_str("       0        -         -\n");
        return output;
    }
    let wpm = records.iter().map(|record| record.header.net_wpm).sum::<f64>() / records.len() as f64;
    let accuracy = records.iter().map(|record| record.header.accuracy).sum::<f64>() / records.len() as f64;
    let _ = writeln!(output, "{:>8}  {:>7.1}  {:>7.1}%", records.len(), wpm, accuracy * 100.0);
    output
}

fn sessions_csv(records: &[SessionRecord]) -> String {
    let mut output = String::from(
        "id,started_at,finished_at,duration_ms,mode,bench_id,layout,net_wpm,raw_wpm,accuracy,iki_median_ms,iki_cv,recovery_keys_median,post_error_three_error_rate,metrics_version\n",
    );
    for record in records {
        let header = &record.header;
        let iki_median = if header.metrics_version != 0 {
            format!("{:.6}", header.iki_median_ms)
        } else {
            String::new()
        };
        let iki_cv = if header.metrics_version != 0 {
            format!("{:.6}", header.iki_cv)
        } else {
            String::new()
        };
        let recovery = (header.metrics_version != 0).then(|| crate::practice::recovery::recovery_metrics(&record.log));
        let recovery_keys = recovery
            .and_then(|metrics| metrics.median_keys_to_recover)
            .map_or_else(String::new, |value| format!("{value:.6}"));
        let post_error_rate = recovery.map_or_else(String::new, |metrics| format!("{:.6}", metrics.next_three_error_rate));
        let _ = writeln!(
            output,
            "{},{},{},{},{},{},{},{:.6},{},{:.6},{},{},{},{},{}",
            header.id.map_or_else(String::new, |value| value.to_string()),
            csv_field(&header.started_at),
            csv_field(&header.finished_at),
            header.duration_ms,
            csv_field(&header.mode),
            csv_field(header.bench_id.as_deref().unwrap_or("")),
            csv_field(&header.layout),
            header.net_wpm,
            header.raw_wpm.map_or_else(String::new, |value| format!("{value:.6}")),
            header.accuracy,
            iki_median,
            iki_cv,
            recovery_keys,
            post_error_rate,
            header.metrics_version,
        );
    }
    output
}

fn detailed_csv(path: &Path, kind: StatsCsv, records: &[SessionRecord], config: &Config, home: &Path, as_of: NaiveDate) -> AppResult<String> {
    if matches!(kind, StatsCsv::Grades | StatsCsv::GradeHistory) {
        let grades = grade_analysis(records, config, home, as_of)?;
        return Ok(match kind {
            StatsCsv::Grades => grades_csv(&grades),
            StatsCsv::GradeHistory => grade_history_csv(&grades),
            _ => unreachable!(),
        });
    }
    let layout = builtin_layout(&config.keyboard.layout).unwrap_or_else(|_| builtin_layout("qwerty").expect("bundled qwerty"));
    let keyboard_geometry = geometry(&config.keyboard.geometry).unwrap_or_else(|| geometry("ansi").expect("bundled ansi"));
    let metrics = analyze_records(
        records,
        &layout,
        &keyboard_geometry,
        AnalysisSettings {
            pause_ms: config.analysis.pause_ms,
            shrinkage_k: config.analysis.shrinkage_k,
            half_life_days: config.analysis.half_life_days,
        },
    );
    let diagnosis = if matches!(
        kind,
        StatsCsv::Errors | StatsCsv::Confusion | StatsCsv::Kana | StatsCsv::Spelling | StatsCsv::Words
    ) {
        let catalog = DataCatalog::load(&config.data, home)?;
        Some(diagnose(
            records,
            &layout,
            &keyboard_geometry,
            DiagnosisOptions {
                model: AnalysisSettings {
                    pause_ms: config.analysis.pause_ms,
                    shrinkage_k: config.analysis.shrinkage_k,
                    half_life_days: config.analysis.half_life_days,
                },
                min_transition_n: config.analysis.min_transition_n,
                spelling_suggestions: config.analysis.spelling_suggestions,
            },
            as_of.and_hms_opt(23, 59, 59).expect("valid end of day").and_utc(),
            catalog.words_ja(),
        ))
    } else {
        None
    };
    let progress = if matches!(
        kind,
        StatsCsv::Form | StatsCsv::Curve | StatsCsv::Changepoints | StatsCsv::Regressions | StatsCsv::Habits
    ) {
        let catalog = DataCatalog::load(&config.data, home)?;
        let reference_version = |part| generate_version(part, 1, &catalog, &layout, &keyboard_geometry);
        let references = ReferencePrompts {
            english: reference_version(BenchPart::A)
                .prompts
                .into_iter()
                .map(|prompt| ExpectedPrompt::english(prompt.target))
                .collect(),
            japanese: reference_version(BenchPart::C)
                .prompts
                .into_iter()
                .filter_map(|prompt| prompt.reading.map(ExpectedPrompt::japanese))
                .collect(),
        };
        Some(analyze_progress(
            records,
            &layout,
            &keyboard_geometry,
            ProgressOptions {
                model: AnalysisSettings {
                    pause_ms: config.analysis.pause_ms,
                    shrinkage_k: config.analysis.shrinkage_k,
                    half_life_days: config.analysis.half_life_days,
                },
                min_transition_n: config.analysis.min_transition_n,
            },
            as_of.and_hms_opt(23, 59, 59).expect("valid end of day").and_utc(),
            &references,
        ))
    } else {
        None
    };
    let mut output = String::new();
    match kind {
        StatsCsv::Keys => {
            output.push_str("key,code,correct,total,accuracy,iki_n,iki_median_ms,iki_p90_ms\n");
            for row in metrics.keys {
                let _ = writeln!(
                    output,
                    "{},{},{},{},{:.6},{},{:.6},{:.6}",
                    csv_field(&row.key),
                    csv_field(&row.code),
                    row.correct,
                    row.total,
                    row.accuracy,
                    row.iki_n,
                    row.iki_median_ms,
                    row.iki_p90_ms
                );
            }
        }
        StatsCsv::Bigrams => {
            output.push_str("from,to,class,n,median_ms,error_n,attempts,error_rate,estimated_iki_ms,estimated_error_rate,estimated_iki_lo_ms,estimated_iki_hi_ms,relative_iki\n");
            for row in metrics.bigrams {
                let _ = writeln!(
                    output,
                    "{},{},{},{},{:.6},{},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6}",
                    csv_field(&row.from),
                    csv_field(&row.to),
                    row.class.as_str(),
                    row.n,
                    row.median_ms,
                    row.error_n,
                    row.attempts,
                    row.error_rate,
                    row.estimated_iki_ms,
                    row.estimated_error_rate,
                    row.estimated_iki_lo_ms,
                    row.estimated_iki_hi_ms,
                    row.relative_iki
                );
            }
        }
        StatsCsv::Fingers => {
            output.push_str("finger,correct,total,accuracy,n,iki_median_ms\n");
            for row in metrics.fingers {
                let _ = writeln!(
                    output,
                    "{},{},{},{},{:.6},{:.6}",
                    csv_field(&row.name),
                    row.correct,
                    row.total,
                    row.accuracy,
                    row.n,
                    row.iki_median_ms
                );
            }
        }
        StatsCsv::Errors => {
            output.push_str("kind,n,proportion\n");
            for row in &diagnosis.as_ref().expect("diagnosis CSV initialized").errors.kinds {
                let _ = writeln!(output, "{},{},{:.6}", row.kind.as_str(), row.n, row.proportion);
            }
        }
        StatsCsv::Confusion => {
            output.push_str("expected,typed,n\n");
            for row in &diagnosis.as_ref().expect("diagnosis CSV initialized").errors.confusion {
                let _ = writeln!(output, "{},{},{}", csv_field(&row.expected), csv_field(&row.typed), row.n);
            }
        }
        StatsCsv::Kana => {
            output.push_str("kana,n,median_ms,error_rate\n");
            for row in &diagnosis.as_ref().expect("diagnosis CSV initialized").japanese.kana {
                let median_ms = row.median_ms.map_or_else(String::new, |value| format!("{value:.6}"));
                let _ = writeln!(output, "{},{},{},{:.6}", csv_field(&row.kana), row.n, median_ms, row.error_rate);
            }
        }
        StatsCsv::Spelling => {
            output.push_str("kana,current,candidate,current_expected_ms,candidate_expected_ms,savings_ms,decision\n");
            for row in &diagnosis.as_ref().expect("diagnosis CSV initialized").japanese.spelling {
                let _ = writeln!(
                    output,
                    "{},{},{},{:.6},{:.6},{:.6},{}",
                    csv_field(&row.kana),
                    csv_field(&row.current),
                    csv_field(&row.candidate),
                    row.current_expected_ms,
                    row.candidate_expected_ms,
                    row.savings_ms,
                    row.decision.as_str()
                );
            }
        }
        StatsCsv::Words => {
            output.push_str("word,n,observed_ms,predicted_ms,ratio\n");
            for row in &diagnosis.as_ref().expect("diagnosis CSV initialized").style.words {
                let _ = writeln!(
                    output,
                    "{},{},{:.6},{:.6},{:.6}",
                    csv_field(&row.word),
                    row.n,
                    row.observed_ms,
                    row.predicted_ms,
                    row.ratio
                );
            }
        }
        StatsCsv::Form => {
            output.push_str("session_id,started_at,mode,net_wpm,expected_wpm,form,corrected_wpm,coverage\n");
            for row in &progress.as_ref().expect("progress CSV initialized").forms {
                let _ = writeln!(
                    output,
                    "{},{},{},{:.6},{:.6},{:.6},{:.6},{:.6}",
                    row.session_id.map_or_else(String::new, |value| value.to_string()),
                    row.started_at.to_rfc3339(),
                    csv_field(&row.mode),
                    row.net_wpm,
                    row.expected_wpm,
                    row.form,
                    row.corrected_wpm,
                    row.coverage
                );
            }
        }
        StatsCsv::Curve => {
            output.push_str("target,model,a,b,c,aicc,n,forecast_days,forecast,band_lo,band_hi\n");
            for row in &progress.as_ref().expect("progress CSV initialized").curves {
                let _ = writeln!(
                    output,
                    "{},{},{:.6},{:.6},{:.6},{:.6},{},{},{:.6},{:.6},{:.6}",
                    csv_field(&row.target),
                    row.model.as_str(),
                    row.a,
                    row.b,
                    row.c,
                    row.aicc,
                    row.n,
                    row.forecast_days,
                    row.forecast,
                    row.band_lo,
                    row.band_hi
                );
            }
        }
        StatsCsv::Changepoints => {
            output.push_str("target,date,probability,kind\n");
            for row in &progress.as_ref().expect("progress CSV initialized").changepoints {
                let _ = writeln!(
                    output,
                    "{},{},{:.6},{}",
                    csv_field(&row.target),
                    row.date,
                    row.probability,
                    row.kind.as_str()
                );
            }
        }
        StatsCsv::Regressions => {
            output.push_str("from,to,best_ms,recent_ms,ratio\n");
            for row in &progress.as_ref().expect("progress CSV initialized").regressions {
                let _ = writeln!(
                    output,
                    "{},{},{:.6},{:.6},{:.6}",
                    csv_field(&row.from),
                    csv_field(&row.to),
                    row.best_ms,
                    row.recent_ms,
                    row.ratio
                );
            }
        }
        StatsCsv::Habits => {
            output.push_str("kind,bucket,n,value\n");
            let progress = progress.as_ref().expect("progress CSV initialized");
            let habits = crate::analysis::habits::analyze_habits(records, &progress.forms);
            for row in habits.rows {
                let value = row.value.map_or_else(String::new, |value| format!("{value:.6}"));
                let _ = writeln!(output, "{},{},{},{}", csv_field(&row.kind), csv_field(&row.bucket), row.n, value);
            }
        }
        StatsCsv::Prescriptions => {
            output.push_str("id,created_at,status,targets,controls,sessions,target_delta_ms,control_delta_ms,effect,z,verdict\n");
            for row in db::load_prescriptions(path)? {
                let result = row.result.as_ref();
                let number = |name: &str| result.and_then(|value| value.get(name)).and_then(serde_json::Value::as_f64);
                let value = |number: Option<f64>| number.map_or_else(String::new, |value| format!("{value:.6}"));
                let sessions = row.hypothesis.get("sessions").and_then(serde_json::Value::as_u64).unwrap_or_default();
                let verdict = result
                    .and_then(|result| result.get("verdict"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(&row.status);
                let _ = writeln!(
                    output,
                    "{},{},{},{},{},{},{},{},{},{},{}",
                    row.id.map_or_else(String::new, |id| id.to_string()),
                    row.created_at.to_rfc3339(),
                    csv_field(&row.status),
                    csv_field(&row.target.to_string()),
                    csv_field(&row.control.to_string()),
                    sessions,
                    value(number("target_delta_ms")),
                    value(number("control_delta_ms")),
                    value(number("effect")),
                    value(number("z")),
                    csv_field(verdict),
                );
            }
        }
        StatsCsv::Reviews => {
            output.push_str("from,to,next_due,interval_days\n");
            for row in db::load_review_schedules(path, &config.keyboard.layout)? {
                let _ = writeln!(
                    output,
                    "{},{},{},{:.6}",
                    csv_field(&row.from),
                    csv_field(&row.to),
                    row.next_due.to_rfc3339(),
                    row.interval_days,
                );
            }
        }
        StatsCsv::Sessions => unreachable!(),
        StatsCsv::Grades | StatsCsv::GradeHistory => unreachable!("grade CSV returned before detailed metrics"),
    }
    Ok(output)
}

fn grade_analysis(records: &[SessionRecord], config: &Config, home: &Path, as_of: NaiveDate) -> AppResult<GradesAnalysis> {
    let layout = builtin_layout(&config.keyboard.layout).unwrap_or_else(|_| builtin_layout("qwerty").expect("bundled qwerty"));
    let keyboard_geometry = geometry(&config.keyboard.geometry).unwrap_or_else(|| geometry("ansi").expect("bundled ansi"));
    let grade_corpus = DataCatalog::load(&DataConfig::default(), home)?;
    let reference_version = |part| generate_version(part, 1, &grade_corpus, &layout, &keyboard_geometry);
    let references = ReferencePrompts {
        english: reference_version(BenchPart::A)
            .prompts
            .into_iter()
            .map(|prompt| ExpectedPrompt::english(prompt.target))
            .collect(),
        japanese: reference_version(BenchPart::C)
            .prompts
            .into_iter()
            .filter_map(|prompt| prompt.reading.map(ExpectedPrompt::japanese))
            .collect(),
    };
    let model = AnalysisSettings {
        pause_ms: config.analysis.pause_ms,
        shrinkage_k: config.analysis.shrinkage_k,
        half_life_days: config.analysis.half_life_days,
    };
    let at = as_of.and_hms_opt(23, 59, 59).expect("valid end of day").and_utc();
    let progress = analyze_progress(
        records,
        &layout,
        &keyboard_geometry,
        ProgressOptions {
            model,
            min_transition_n: config.analysis.min_transition_n,
        },
        at,
        &references,
    );
    Ok(analyze_grades(
        records,
        &progress.forms,
        &layout,
        &keyboard_geometry,
        GradeOptions { model },
        at,
        grade_corpus.words_en(),
        grade_corpus.words_ja(),
    ))
}

fn grades_csv(grades: &GradesAnalysis) -> String {
    let mut output = String::from("language,component,value,unit,grade,score,next_grade,next_threshold,gap,source,n_sessions,version\n");
    for language in &grades.languages {
        if let Some(overall) = &language.overall {
            let _ = writeln!(
                output,
                "{},overall,{:.6},score,{},{:.6},,,,{},{},{}",
                language.language,
                overall.score,
                overall.grade.as_str(),
                overall.score,
                language.source.as_str(),
                language.n_sessions,
                language.version,
            );
        }
        for row in &language.components {
            let _ = writeln!(
                output,
                "{},{},{:.6},{},{},{:.6},{},{},{},{},{},{}",
                language.language,
                row.component.as_str(),
                row.value,
                csv_field(&row.unit),
                row.grade.as_str(),
                row.score,
                row.next_grade.map_or("", |grade| grade.as_str()),
                row.next_threshold.map_or_else(String::new, |value| format!("{value:.6}")),
                row.gap.map_or_else(String::new, |value| format!("{value:.6}")),
                language.source.as_str(),
                language.n_sessions,
                language.version,
            );
        }
    }
    output
}

fn grade_history_csv(grades: &GradesAnalysis) -> String {
    let mut output = String::from("date,language,component,grade,score,version\n");
    for row in &grades.history {
        let _ = writeln!(
            output,
            "{},{},{},{},{:.6},{}",
            row.date,
            row.language,
            row.component,
            row.grade.as_str(),
            row.score,
            row.version,
        );
    }
    output
}

fn grades_text(grades: &GradesAnalysis, lang: Lang) -> String {
    let mut output = String::new();
    for language in ["en", "ja"] {
        let label = match (language, lang) {
            ("en", Lang::Ja) => "英語",
            ("ja", Lang::Ja) => "日本語",
            ("en", Lang::En) => "English",
            _ => "Japanese",
        };
        let Some(grade) = grades.languages.iter().find(|grade| grade.language == language) else {
            if lang == Lang::Ja {
                let _ = writeln!(output, "{label}: 測定できません");
            } else {
                let _ = writeln!(output, "{label}: not measurable");
            }
            continue;
        };
        render_language_grade(&mut output, grades, grade, label, lang);
    }
    output
}

fn render_language_grade(output: &mut String, grades: &GradesAnalysis, grade: &LanguageGrade, label: &str, lang: Lang) {
    let heading = if lang == Lang::Ja {
        format!("総合（{label}）")
    } else {
        format!("Overall ({label})")
    };
    let source = match (grade.source, grade.status, lang) {
        (GradeSource::Bench, GradeStatus::Official, Lang::Ja) => "正式・直近 3 回".to_owned(),
        (GradeSource::Bench, GradeStatus::Official, Lang::En) => "official, latest 3".to_owned(),
        (GradeSource::Bench, GradeStatus::Interim, Lang::Ja) => format!("暫定・直近 {} 回", grade.n_sessions),
        (GradeSource::Bench, GradeStatus::Interim, Lang::En) => format!("interim, latest {}", grade.n_sessions),
        (GradeSource::Provisional, GradeStatus::Provisional, Lang::Ja) => "仮・ベンチ記録が無いため練習から推定".to_owned(),
        (GradeSource::Provisional, GradeStatus::Provisional, Lang::En) => {
            "provisional, estimated from practice because no benchmark is recorded".to_owned()
        }
        (GradeSource::ProvisionalUncorrected, GradeStatus::Provisional, Lang::Ja) => "仮・難しさ未補正".to_owned(),
        (GradeSource::ProvisionalUncorrected, GradeStatus::Provisional, Lang::En) => "provisional, difficulty uncorrected".to_owned(),
        _ => "invalid grade source".to_owned(),
    };
    if let Some(overall) = &grade.overall {
        let change = grades
            .changes
            .iter()
            .find(|change| change.language == grade.language && change.component == "overall")
            .map(|change| match (change.kind, lang) {
                (GradeChangeKind::Promotion, Lang::Ja) => format!("  前回 {} → {} に昇格", change.from.as_str(), change.to.as_str()),
                (GradeChangeKind::Demotion, Lang::Ja) => format!("  前回 {} → {} に降格", change.from.as_str(), change.to.as_str()),
                (GradeChangeKind::Promotion, Lang::En) => {
                    format!("  previous {} → promoted to {}", change.from.as_str(), change.to.as_str())
                }
                (GradeChangeKind::Demotion, Lang::En) => {
                    format!("  previous {} → demoted to {}", change.from.as_str(), change.to.as_str())
                }
            })
            .unwrap_or_default();
        let source = match lang {
            Lang::En => format!("({source})"),
            Lang::Ja => format!("（{source}）"),
        };
        let _ = writeln!(output, "{heading}: {}  {:.2}  {source}{change}", overall.grade.as_str(), overall.score);
    } else {
        let text = if lang == Lang::Ja {
            "速さまたは正確さを測定できません"
        } else {
            "speed or accuracy is not measurable"
        };
        let source = match lang {
            Lang::En => format!("({source})"),
            Lang::Ja => format!("（{source}）"),
        };
        let _ = writeln!(output, "{heading}: {text}  {source}");
    }
    for line in crate::analysis::coach::formatted_component_lines(grade, lang, true) {
        let _ = writeln!(output, "{line}");
    }
    if let Some(component) = grade.overall.as_ref().and_then(|overall| overall.easiest_component) {
        if lang == Lang::Ja {
            let _ = writeln!(output, "総合を上げやすい構成: {}", component_name(component, lang));
        } else {
            let _ = writeln!(output, "Best component to improve overall: {}", component_name(component, lang));
        }
    }
}

fn component_name(component: Component, lang: Lang) -> &'static str {
    match (component, lang) {
        (Component::Speed, Lang::Ja) => "速さ",
        (Component::Accuracy, Lang::Ja) => "正確さ",
        (Component::Rhythm, Lang::Ja) => "リズム",
        (Component::Transition, Lang::Ja) => "遷移",
        (Component::Preview, Lang::Ja) => "先読み",
        (Component::Recovery, Lang::Ja) => "立ち直り",
        (Component::Overlap, Lang::Ja) => "重なり",
        (Component::Speed, Lang::En) => "Speed",
        (Component::Accuracy, Lang::En) => "Accuracy",
        (Component::Rhythm, Lang::En) => "Rhythm",
        (Component::Transition, Lang::En) => "Transitions",
        (Component::Preview, Lang::En) => "Preview",
        (Component::Recovery, Lang::En) => "Recovery",
        (Component::Overlap, Lang::En) => "Overlap",
    }
}

fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}
