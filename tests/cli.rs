use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use std::time::Duration;

use chrono::Utc;
use clap::Parser;
use rs_typing_trainer::cli::{Cli, Command};
use rs_typing_trainer::config::Config;
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::graphics::GraphicsPreference;
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::record::{SessionRecord, db};
use tempfile::TempDir;

#[test]
fn parses_japanese_language_override() {
    let cli = Cli::try_parse_from(["rs-typing-trainer", "--lang", "ja"]).unwrap();

    assert_eq!(cli.lang, Some(Lang::Ja));
    assert!(cli.command.is_none());
}

#[test]
fn parses_replay_file() {
    let cli = Cli::try_parse_from(["rs-typing-trainer", "replay", "foo.ttrec"]).unwrap();

    assert_eq!(
        cli.command,
        Some(Command::Replay {
            file: PathBuf::from("foo.ttrec"),
            interval: None,
            speed: 1.0,
            paused: false,
            exit_on_end: false,
            print: false,
        })
    );
}

#[test]
fn parses_stats_subcommand() {
    let cli = Cli::try_parse_from(["rs-typing-trainer", "stats"]).unwrap();

    assert_eq!(
        cli.command,
        Some(Command::Stats {
            csv: None,
            summary: false,
            grades: false,
            coach: false,
            since: None,
            mode: None,
        })
    );
}

#[test]
fn parses_stats_summary_and_rejects_csv_at_the_same_time() {
    let cli = Cli::try_parse_from(["rs-typing-trainer", "stats", "--summary"]).unwrap();
    assert_eq!(
        cli.command,
        Some(Command::Stats {
            csv: None,
            summary: true,
            grades: false,
            coach: false,
            since: None,
            mode: None,
        })
    );
    assert!(Cli::try_parse_from(["rs-typing-trainer", "stats", "--summary", "--csv", "sessions"]).is_err());
}

#[test]
fn parses_stats_grades_and_rejects_other_output_modes() {
    let cli = Cli::try_parse_from(["rs-typing-trainer", "stats", "--grades"]).unwrap();
    assert_eq!(
        cli.command,
        Some(Command::Stats {
            csv: None,
            summary: false,
            grades: true,
            coach: false,
            since: None,
            mode: None,
        })
    );
    assert!(Cli::try_parse_from(["rs-typing-trainer", "stats", "--grades", "--summary"]).is_err());
    assert!(Cli::try_parse_from(["rs-typing-trainer", "stats", "--grades", "--csv", "grades"]).is_err());
}

#[test]
fn parses_stats_coach_and_rejects_other_output_modes() {
    let cli = Cli::try_parse_from(["rs-typing-trainer", "stats", "--coach"]).unwrap();
    assert_eq!(
        cli.command,
        Some(Command::Stats {
            csv: None,
            summary: false,
            grades: false,
            coach: true,
            since: None,
            mode: None,
        })
    );
    assert!(Cli::try_parse_from(["rs-typing-trainer", "stats", "--coach", "--summary"]).is_err());
    assert!(Cli::try_parse_from(["rs-typing-trainer", "stats", "--coach", "--csv", "grades"]).is_err());
}

#[test]
fn parses_export_subcommand() {
    let cli = Cli::try_parse_from(["rs-typing-trainer", "export", "42", "-o", "out.ttrec"]).unwrap();
    assert_eq!(
        cli.command,
        Some(Command::Export {
            session_id: 42,
            output: Some(PathBuf::from("out.ttrec")),
        })
    );
}

#[test]
fn command_line_language_takes_precedence_over_config() {
    let cli = Cli::try_parse_from(["rs-typing-trainer", "--lang", "en"]).unwrap();

    assert_eq!(cli.effective_lang(Lang::Ja), Lang::En);
    assert_eq!(Cli::default().effective_lang(Lang::Ja), Lang::Ja);
}

#[test]
fn parses_global_graphics_override_and_graphics_demo() {
    let cli = Cli::try_parse_from(["rs-typing-trainer", "--graphics", "kitty", "graphics", "--demo", "--seconds", "5"]).unwrap();

    assert_eq!(cli.graphics, Some(GraphicsPreference::Kitty));
    assert_eq!(cli.command, Some(Command::Graphics { demo: true, seconds: 5 }));
}

#[test]
fn graphics_subcommand_reports_off_when_stdio_is_not_a_terminal() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("config.toml");
    std::fs::write(&config_path, "[display]\ngraphics = \"auto\"\n").unwrap();
    let output = ProcessCommand::new(env!("CARGO_BIN_EXE_rs-typing-trainer"))
        .args(["--lang", "ja", "--config", config_path.to_str().unwrap(), "graphics"])
        .output()
        .unwrap();

    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("方式: off"), "{stdout}");
    assert!(stdout.contains("標準入出力が端末ではありません"), "{stdout}");
    assert!(stdout.contains("TERM:"), "{stdout}");
    assert!(stdout.contains("TERM_PROGRAM:"), "{stdout}");
    assert!(stdout.contains("tmux:"), "{stdout}");
    assert!(stdout.contains("SSH:"), "{stdout}");
}

#[test]
fn stats_without_arguments_prints_an_overview() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("sessions.db");
    let config_path = dir.path().join("config.toml");
    std::fs::write(&config_path, format!("[storage]\ndb_path = {:?}\n", db_path)).unwrap();
    let output = ProcessCommand::new(env!("CARGO_BIN_EXE_rs-typing-trainer"))
        .args(["--config", config_path.to_str().unwrap(), "stats"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout).unwrap().contains("Sessions"));
}

#[test]
fn stats_summary_honors_the_japanese_language_option() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("sessions.db");
    let config_path = dir.path().join("config.toml");
    std::fs::write(&config_path, format!("[storage]\ndb_path = {:?}\n", db_path)).unwrap();
    let output = ProcessCommand::new(env!("CARGO_BIN_EXE_rs-typing-trainer"))
        .args(["--lang", "ja", "--config", config_path.to_str().unwrap(), "stats", "--summary"])
        .output()
        .unwrap();

    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8(output.stdout).unwrap().contains("まだ記録がありません"));
}

#[test]
fn export_writes_v2_record_to_stdout() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("sessions.db");
    let config_path = dir.path().join("config.toml");
    std::fs::write(&config_path, format!("[storage]\ndb_path = {:?}\n", db_path)).unwrap();

    let mut session = Session::new(Duration::from_secs(1), Duration::from_secs(1), Item::exact("a"));
    session.activate(Duration::ZERO);
    session.input_char(Duration::from_millis(100), "a");
    session.tick(Duration::from_secs(1));
    let id = db::save(&db_path, &SessionRecord::from_log(&Config::default(), session.into_log(), Utc::now())).unwrap();

    let output = ProcessCommand::new(env!("CARGO_BIN_EXE_rs-typing-trainer"))
        .args(["--config", config_path.to_str().unwrap(), "export", &id.to_string()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8(output.stdout).unwrap().starts_with(r#"{"type":"session""#));
}

#[test]
fn export_rejects_v1_record_with_exit_code_one() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("sessions.db");
    let config_path = dir.path().join("config.toml");
    std::fs::write(&config_path, format!("[storage]\ndb_path = {:?}\n", db_path)).unwrap();
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE sessions (id INTEGER PRIMARY KEY AUTOINCREMENT, finished_at TEXT NOT NULL, duration_ms INTEGER NOT NULL, wpm REAL NOT NULL, cps REAL NOT NULL, accuracy REAL NOT NULL, consistency REAL NOT NULL, max_combo INTEGER NOT NULL, completed_words INTEGER NOT NULL, timeout_words INTEGER NOT NULL, mistypes_total INTEGER NOT NULL, payload TEXT NOT NULL);",
    )
    .unwrap();
    let result = rs_typing_trainer::metrics::stats::GameResult::from_metrics(&Config::default(), Default::default(), Duration::from_secs(1));
    conn.execute(
        "INSERT INTO sessions (finished_at, duration_ms, wpm, cps, accuracy, consistency, max_combo, completed_words, timeout_words, mistypes_total, payload) VALUES (?1, 1000, 0, 0, 1, 0, 0, 0, 0, 0, ?2)",
        rusqlite::params![result.finished_at.to_rfc3339(), serde_json::to_string(&result).unwrap()],
    )
    .unwrap();
    drop(conn);

    let output = ProcessCommand::new(env!("CARGO_BIN_EXE_rs-typing-trainer"))
        .args(["--config", config_path.to_str().unwrap(), "export", "1"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8(output.stderr).unwrap().contains("no keystroke log"));
}
