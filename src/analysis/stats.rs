use std::fmt::Write;
use std::path::Path;

use chrono::NaiveDate;

use crate::analysis::metrics::{AnalysisSettings, analyze_records};
use crate::cli::StatsCsv;
use crate::config::Config;
use crate::error::AppResult;
use crate::i18n::Lang;
use crate::layout::builtin_layout;
use crate::layout::geometry::geometry;
use crate::record::SessionRecord;
use crate::record::db;

#[allow(clippy::too_many_arguments)]
pub fn render(
    path: &Path,
    config: &Config,
    csv: Option<StatsCsv>,
    summary: bool,
    since: Option<NaiveDate>,
    mode: Option<&str>,
    lang: Lang,
    as_of: NaiveDate,
) -> AppResult<String> {
    let records = db::load_records_filtered(path, since, mode)?;
    Ok(match csv {
        Some(StatsCsv::Sessions) => sessions_csv(&records),
        Some(kind) => detailed_csv(kind, &records, config),
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

fn detailed_csv(kind: StatsCsv, records: &[SessionRecord], config: &Config) -> String {
    let layout = builtin_layout(&config.keyboard.layout).unwrap_or_else(|_| builtin_layout("qwerty").expect("bundled qwerty"));
    let keyboard_geometry = geometry(&config.keyboard.geometry).unwrap_or_else(|| geometry("ansi").expect("bundled ansi"));
    let metrics = analyze_records(
        records,
        &layout,
        &keyboard_geometry,
        AnalysisSettings {
            pause_ms: config.analysis.pause_ms,
            shrinkage_k: config.analysis.shrinkage_k,
        },
    );
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
            output.push_str("from,to,class,n,median_ms,error_n,attempts,error_rate,estimated_iki_ms,estimated_error_rate\n");
            for row in metrics.bigrams {
                let _ = writeln!(
                    output,
                    "{},{},{},{},{:.6},{},{},{:.6},{:.6},{:.6}",
                    csv_field(&row.from),
                    csv_field(&row.to),
                    row.class.as_str(),
                    row.n,
                    row.median_ms,
                    row.error_n,
                    row.attempts,
                    row.error_rate,
                    row.estimated_iki_ms,
                    row.estimated_error_rate
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
        StatsCsv::Sessions => unreachable!(),
    }
    output
}

fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}
