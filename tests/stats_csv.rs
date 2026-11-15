use std::process::Command;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use rs_typing_trainer::config::{Config, ModeKind};
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::record::{SessionRecord, db};
use tempfile::TempDir;

fn fixture() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    let directory = TempDir::new().unwrap();
    let db_path = directory.path().join("sessions.db");
    let config_path = directory.path().join("config.toml");
    std::fs::write(&config_path, format!("[storage]\ndb_path = {:?}\n", db_path)).unwrap();
    for (kind, started, text) in [
        (ModeKind::Words, Utc.with_ymd_and_hms(2026, 11, 14, 0, 0, 0).unwrap(), "as"),
        (ModeKind::Romaji, Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(), "gh"),
    ] {
        let mut config = Config::default();
        config.mode.kind = kind;
        let mut session = Session::new(Duration::from_secs(1), Duration::ZERO, Item::exact(text));
        session.activate(Duration::ZERO);
        for (index, character) in text.chars().enumerate() {
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
            "from,to,class,n,median_ms,error_n,attempts,error_rate,estimated_iki_ms,estimated_error_rate",
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
