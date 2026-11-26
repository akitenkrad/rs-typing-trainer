use std::process::Command;
use std::time::Duration;
use unicode_width::UnicodeWidthStr;

use chrono::{TimeZone, Utc};
use rs_typing_trainer::config::{Config, ModeKind};
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::romaji::RomajiJudge;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::record::{SessionRecord, db};
use tempfile::TempDir;

fn fixture() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    let directory = TempDir::new().unwrap();
    let db_path = directory.path().join("sessions.db");
    let config_path = directory.path().join("config.toml");
    std::fs::write(&config_path, format!("[storage]\ndb_path = {:?}\n", db_path)).unwrap();
    for (kind, started, text, reading, input) in [
        (ModeKind::Words, Utc::now(), "as", None, "as"),
        (
            ModeKind::Romaji,
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            "し",
            Some("し"),
            "shi",
        ),
    ] {
        let mut config = Config::default();
        config.mode.kind = kind;
        let item = reading.map_or_else(
            || Item::exact(text),
            |reading| Item::new(text, Some(reading.to_owned()), Box::new(RomajiJudge::new(reading))),
        );
        let mut session = Session::new(Duration::from_secs(1), Duration::ZERO, item);
        session.activate(Duration::ZERO);
        for (index, character) in input.chars().enumerate() {
            session.input_char_with_code(
                Duration::from_millis(100 + index as u64 * 100),
                &character.to_string(),
                rs_typing_trainer::layout::builtin_layout("qwerty")
                    .unwrap()
                    .stroke_for(character)
                    .map(|stroke| stroke.code),
            );
        }
        session.tick(Duration::from_secs(1));
        db::save(&db_path, &SessionRecord::from_log(&config, session.into_log(), started)).unwrap();
    }
    (directory, config_path, db_path)
}

fn stats(config: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_rs-typing-trainer"))
        .arg("--config")
        .arg(config)
        .arg("stats")
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}

fn add_grade_benches(db_path: &std::path::Path) {
    for (part, kind, reading, input, base_wpm) in [("A", ModeKind::Words, None, "as", 78.0), ("C", ModeKind::Romaji, Some("し"), "shi", 66.0)] {
        for day in 1..=3 {
            let mut config = Config::default();
            config.mode.kind = kind;
            let item = reading.map_or_else(
                || Item::exact("as"),
                |reading| Item::new("し", Some(reading.to_owned()), Box::new(RomajiJudge::new(reading))),
            );
            let mut session = Session::new(Duration::from_secs(1), Duration::ZERO, item);
            session.activate(Duration::ZERO);
            for (index, character) in input.chars().enumerate() {
                session.input_char(Duration::from_millis(100 + index as u64 * 100), &character.to_string());
            }
            session.tick(Duration::from_secs(1));
            let mut record = SessionRecord::from_log(&config, session.into_log(), Utc.with_ymd_and_hms(2026, 11, day, 12, 0, 0).unwrap());
            record.header.bench_id = Some(format!("bench-v1/{part}/0{day}"));
            record.header.mode = "bench".to_owned();
            record.header.net_wpm = base_wpm + f64::from(day);
            record.header.accuracy = 0.98;
            db::save(db_path, &record).unwrap();
        }
    }
}

fn grades_output(config: &std::path::Path, lang: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_rs-typing-trainer"))
        .args(["--lang", lang, "--config"])
        .arg(config)
        .args(["stats", "--grades"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn four_csv_exports_have_headers_and_rows() {
    let (_directory, config, _db) = fixture();
    let cases = [
        (
            "sessions",
            "id,started_at,finished_at,duration_ms,mode,bench_id,layout,net_wpm,raw_wpm,accuracy,iki_median_ms,iki_cv,recovery_keys_median,post_error_three_error_rate,metrics_version",
        ),
        ("keys", "key,code,correct,total,accuracy,iki_n,iki_median_ms,iki_p90_ms"),
        (
            "bigrams",
            "from,to,class,n,median_ms,error_n,attempts,error_rate,estimated_iki_ms,estimated_error_rate,estimated_iki_lo_ms,estimated_iki_hi_ms,relative_iki",
        ),
        ("fingers", "finger,correct,total,accuracy,n,iki_median_ms"),
    ];
    for (kind, header) in cases {
        let csv = stats(&config, &["--csv", kind]);
        let lines = csv.lines().collect::<Vec<_>>();
        assert_eq!(lines[0], header, "{kind}: {csv}");
        assert!(lines.len() >= 2, "{kind}: {csv}");
    }
}

#[test]
fn diagnosis_csv_exports_have_stable_headers() {
    let (_directory, config, _db) = fixture();
    let cases = [
        ("errors", "kind,n,proportion"),
        ("confusion", "expected,typed,n"),
        ("kana", "kana,n,median_ms,error_rate"),
        (
            "spelling",
            "kana,current,candidate,current_expected_ms,candidate_expected_ms,savings_ms,decision",
        ),
        ("words", "word,n,observed_ms,predicted_ms,ratio"),
    ];
    for (kind, header) in cases {
        let csv = stats(&config, &["--csv", kind]);
        assert_eq!(csv.lines().next(), Some(header), "{kind}: {csv}");
    }
    let kana = stats(&config, &["--csv", "kana"]);
    assert!(kana.lines().any(|line| line == "し,1,,0.000000"), "{kana}");
}

#[test]
fn progress_csv_exports_have_stable_headers() {
    let (_directory, config, _db) = fixture();
    let cases = [
        ("form", "session_id,started_at,mode,net_wpm,expected_wpm,form,corrected_wpm,coverage"),
        ("curve", "target,model,a,b,c,aicc,n,forecast_days,forecast,band_lo,band_hi"),
        ("changepoints", "target,date,probability,kind"),
        ("regressions", "from,to,best_ms,recent_ms,ratio"),
        ("habits", "kind,bucket,n,value"),
        (
            "prescriptions",
            "id,created_at,status,targets,controls,sessions,target_delta_ms,control_delta_ms,effect,z,verdict",
        ),
        ("reviews", "from,to,next_due,interval_days"),
    ];
    for (kind, header) in cases {
        let csv = stats(&config, &["--csv", kind]);
        assert_eq!(csv.lines().next(), Some(header), "{kind}: {csv}");
    }
}

#[test]
fn grade_csv_exports_have_stable_headers_and_history() {
    let (_directory, config, db_path) = fixture();
    add_grade_benches(&db_path);
    let grades = stats(&config, &["--csv", "grades"]);
    assert_eq!(
        grades.lines().next(),
        Some("language,component,value,unit,grade,score,next_grade,next_threshold,gap,source,n_sessions,version")
    );
    assert!(grades.lines().any(|line| line.starts_with("en,overall,")), "{grades}");
    assert!(grades.lines().any(|line| line.starts_with("ja,overall,")), "{grades}");
    assert!(grades.lines().skip(1).all(|line| line.split(',').count() == 12), "{grades}");

    let history = stats(&config, &["--csv", "grade-history"]);
    assert_eq!(history.lines().next(), Some("date,language,component,grade,score,version"));
    assert!(history.lines().any(|line| line.contains(",en,overall,")), "{history}");
    assert!(history.lines().skip(1).all(|line| line.split(',').count() == 6), "{history}");
}

#[test]
fn grades_text_is_localized_in_english_and_japanese() {
    let (_directory, config, db_path) = fixture();
    add_grade_benches(&db_path);

    let english = grades_output(&config, "en");
    assert!(english.contains("Overall (English):"), "{english}");
    assert!(english.contains("Overall (Japanese):"), "{english}");
    assert!(english.contains("Best component to improve overall:"), "{english}");

    let japanese = grades_output(&config, "ja");
    assert!(japanese.contains("総合（英語）:"), "{japanese}");
    assert!(japanese.contains("総合（日本語）:"), "{japanese}");
    assert!(japanese.contains("総合を上げやすい構成:"), "{japanese}");
}

#[test]
fn grades_text_aligns_component_columns_by_display_width() {
    let (_directory, config, db_path) = fixture();
    add_grade_benches(&db_path);

    for (lang, names) in [
        ("en", ["Speed", "Accuracy", "Rhythm", "Transitions", "Preview", "Recovery", "Overlap"]),
        ("ja", ["速さ", "正確さ", "リズム", "遷移", "先読み", "立ち直り", "重なり"]),
    ] {
        let output = grades_output(&config, lang);
        let lines = output
            .lines()
            .filter(|line| names.iter().any(|name| line.trim_start().starts_with(name)))
            .filter(|line| {
                line.split_whitespace()
                    .nth(1)
                    .is_some_and(|token| matches!(token, "S" | "A" | "B" | "C" | "D" | "E" | "F" | "G"))
            })
            .collect::<Vec<_>>();
        assert!(lines.len() >= 2, "{lang}: {output}");

        let grade_columns = lines
            .iter()
            .map(|line| {
                let grade = line.split_whitespace().nth(1).unwrap();
                let byte = line.find(&format!("  {grade}  ")).unwrap() + 2;
                UnicodeWidthStr::width(&line[..byte])
            })
            .collect::<Vec<_>>();
        assert!(grade_columns.windows(2).all(|pair| pair[0] == pair[1]), "{lang}: {output}");

        let value_ends = lines
            .iter()
            .map(|line| {
                let value = line.split_whitespace().nth(2).unwrap();
                let byte = line.find(value).unwrap();
                UnicodeWidthStr::width(&line[..byte]) + UnicodeWidthStr::width(value)
            })
            .collect::<Vec<_>>();
        assert!(value_ends.windows(2).all(|pair| pair[0] == pair[1]), "{lang}: {output}");
    }

    let japanese = grades_output(&config, "ja");
    assert!(
        !japanese.contains("  ("),
        "Japanese source labels must use fullwidth parentheses: {japanese}"
    );
}

#[test]
fn coach_text_is_localized_and_uses_the_same_four_sections() {
    let (_directory, config, _db) = fixture();
    let english = stats(&config, &["--coach"]);
    for heading in ["WHERE YOU ARE NOW", "LAST PRESCRIPTION", "BEST OPPORTUNITIES", "HABITS"] {
        assert!(english.contains(heading), "missing {heading}: {english}");
    }
    assert!(
        !english.contains('（') && !english.contains('）'),
        "English coach used Japanese parentheses: {english}"
    );
    assert!(!english.contains('．'), "English coach used a Japanese full stop: {english}");

    let output = Command::new(env!("CARGO_BIN_EXE_rs-typing-trainer"))
        .args(["--lang", "ja", "--config"])
        .arg(&config)
        .args(["stats", "--coach"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let japanese = String::from_utf8(output.stdout).unwrap();
    for heading in ["今の位置", "前回の処方の結果", "いちばんの伸びしろ", "習慣"] {
        assert!(japanese.contains(heading), "missing {heading}: {japanese}");
    }
    assert!(
        !japanese.lines().any(|line| line.contains("日本語:") && line.contains("/4 週")),
        "the old Japanese record must not produce a recent slope: {japanese}"
    );
    if std::env::var_os("RS_TYPING_TRAINER_DUMP_COACH").is_some() {
        println!("\n--- COACH EN ---\n{english}");
        println!("\n--- COACH JA ---\n{japanese}");
    }
}

#[test]
fn uncorrected_provisional_speed_is_labeled_in_csv_and_both_languages() {
    let (_directory, config, _db) = fixture();

    let csv = stats(&config, &["--csv", "grades"]);
    assert!(csv.lines().any(|line| line.contains(",provisional-uncorrected,")), "{csv}");

    let english = grades_output(&config, "en");
    assert!(english.contains("provisional, difficulty uncorrected"), "{english}");
    let japanese = grades_output(&config, "ja");
    assert!(japanese.contains("仮・難しさ未補正"), "{japanese}");
}

#[test]
fn since_and_mode_filter_the_exported_sessions() {
    let (_directory, config, _db) = fixture();
    let since = stats(&config, &["--csv", "sessions", "--since", "2026-06-01"]);
    assert_eq!(since.lines().count(), 2, "{since}");
    assert!(since.contains(",words,"), "{since}");
    assert!(!since.contains(",romaji,"), "{since}");

    let mode = stats(&config, &["--csv", "sessions", "--mode", "romaji"]);
    assert_eq!(mode.lines().count(), 2, "{mode}");
    assert!(mode.contains(",romaji,"), "{mode}");
    assert!(!mode.contains(",words,"), "{mode}");
}

#[test]
fn legacy_session_csv_leaves_iki_fields_empty() {
    let (_directory, config, db_path) = fixture();
    let connection = rusqlite::Connection::open(db_path).unwrap();
    connection
        .execute(
            "UPDATE sessions SET metrics_version = 0, iki_median_ms = 9876.0, iki_cv = 9.876 WHERE mode = 'romaji'",
            [],
        )
        .unwrap();

    let csv = stats(&config, &["--csv", "sessions"]);
    let row = csv.lines().find(|line| line.contains(",romaji,")).unwrap();
    let fields = row.split(',').collect::<Vec<_>>();
    assert_eq!(fields[10], "", "{row}");
    assert_eq!(fields[11], "", "{row}");
}

#[test]
fn summary_output_uses_the_existing_since_and_mode_filters() {
    let (_directory, config, _db) = fixture();

    let output = stats(&config, &["--summary", "--since", "2026-06-01", "--mode", "words"]);

    assert!(output.contains("Current ability"), "{output}");
    assert!(output.contains("words"), "{output}");
    assert!(!output.contains("romaji"), "{output}");
}
