use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use clap::Parser;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier, Style};
use rs_typing_trainer::cli::{Cli, Command};
use rs_typing_trainer::config::Config;
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::record::SessionRecord;
use rs_typing_trainer::replay::{PlaybackTiming, Replay, format_summary};

mod common;
use common::test_app;

fn record() -> SessionRecord {
    let mut session = Session::new(Duration::from_secs(1), Duration::ZERO, Item::exact("ab"));
    session.activate(Duration::ZERO);
    session.input_char(Duration::from_millis(100), "a");
    session.input_char(Duration::from_millis(250), "b");
    session.tick(Duration::from_secs(1));
    SessionRecord::from_log(
        &Config::default(),
        session.into_log(),
        Utc.with_ymd_and_hms(2026, 11, 14, 0, 0, 0).unwrap(),
    )
}

fn two_a_record(config: &Config) -> SessionRecord {
    let mut session = Session::new(Duration::from_secs(1), Duration::ZERO, Item::exact("aa"));
    session.activate(Duration::ZERO);
    session.input_char_with_code(Duration::from_millis(100), "a", Some("KeyA".to_owned()));
    session.input_char_with_code(Duration::from_millis(250), "a", Some("KeyA".to_owned()));
    session.tick(Duration::from_secs(1));
    SessionRecord::from_log(config, session.into_log(), Utc.with_ymd_and_hms(2026, 11, 14, 0, 0, 0).unwrap())
}

fn replay_config() -> Config {
    let mut config = Config::default();
    config.mode.keys = "a".to_owned();
    config.mode.base_keys = "none".to_owned();
    config.mode.random.word_length = 2;
    config.session.item_timeout_seconds = 0;
    config.ui.animation_enabled = false;
    config
}

fn buffer_cells_except_row(buffer: &Buffer, excluded_y: u16) -> Vec<(String, Style)> {
    buffer
        .content()
        .iter()
        .enumerate()
        .filter(|(index, _)| *index as u16 / buffer.area.width != excluded_y)
        .map(|(_, cell)| (cell.symbol().to_owned(), cell.style()))
        .collect()
}

fn buffer_text(buffer: &Buffer) -> String {
    buffer.content().iter().map(|cell| cell.symbol()).collect()
}

#[test]
fn replay_arguments_match_the_public_cli_contract() {
    let cli = Cli::try_parse_from([
        "rs-typing-trainer",
        "replay",
        "game.ttrec",
        "--interval",
        "80",
        "--paused",
        "--exit-on-end",
        "--print",
    ])
    .unwrap();
    assert_eq!(
        cli.command,
        Some(Command::Replay {
            file: PathBuf::from("game.ttrec"),
            interval: Some(80),
            speed: 1.0,
            paused: true,
            exit_on_end: true,
            print: true,
        })
    );

    let cli = Cli::try_parse_from(["rs-typing-trainer", "replay", "game.ttrec", "--speed", "2.5"]).unwrap();
    let Some(Command::Replay { speed, .. }) = cli.command else {
        panic!("expected replay command");
    };
    assert_eq!(speed, 2.5);
}

#[test]
fn interval_and_speed_are_mutually_exclusive() {
    let error = Cli::try_parse_from(["rs-typing-trainer", "replay", "game.ttrec", "--interval", "80", "--speed", "2"]).unwrap_err();
    assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
}

#[test]
fn recorded_timestamps_are_applied_at_their_injected_times() {
    let mut replay = Replay::new(record(), PlaybackTiming::Recorded { speed: 1.0 }, false, false);
    replay.advance_to(Duration::from_millis(99));
    assert_eq!(replay.applied_keystrokes(), 0);
    replay.advance_to(Duration::from_millis(100));
    assert_eq!(replay.applied_keystrokes(), 1);
    replay.advance_to(Duration::from_millis(249));
    assert_eq!(replay.applied_keystrokes(), 1);
    replay.advance_to(Duration::from_millis(250));
    assert_eq!(replay.applied_keystrokes(), 2);
    assert!(replay.is_finished());
    assert!(!replay.should_exit());
}

#[test]
fn fixed_interval_pause_step_and_exit_on_end_are_deterministic() {
    let mut replay = Replay::new(
        record(),
        PlaybackTiming::Fixed {
            interval: Duration::from_millis(50),
        },
        true,
        true,
    );
    replay.advance_to(Duration::from_secs(1));
    assert_eq!(replay.applied_keystrokes(), 0);
    replay.step();
    assert_eq!(replay.applied_keystrokes(), 1);
    assert!(!replay.should_exit());
    replay.step();
    assert!(replay.is_finished());
    assert!(replay.should_exit());
}

#[test]
fn final_metrics_and_print_summary_come_from_the_record_log() {
    let record = record();
    let expected = rs_typing_trainer::analysis::analyze(&record.log);
    let mut replay = Replay::new(record.clone(), PlaybackTiming::Recorded { speed: 4.0 }, false, false);
    replay.advance_to(Duration::from_secs(1));
    assert_eq!(replay.metrics(), expected);

    let summary = format_summary(&record);
    assert!(summary.contains("Mode: random"), "{summary}");
    assert!(summary.contains("Layout: qwerty"), "{summary}");
    assert!(summary.contains("Prompt: ab"), "{summary}");
    assert!(summary.contains("Keystrokes: 2"), "{summary}");
    assert!(summary.contains("Net WPM: 24.0"), "{summary}");
    assert!(summary.contains("Accuracy: 100.0%"), "{summary}");
}

#[test]
fn print_mode_reads_only_the_record_and_writes_the_summary() {
    let directory = tempfile::TempDir::new().unwrap();
    let record_path = directory.path().join("sample.ttrec");
    let mut file = std::fs::File::create(&record_path).unwrap();
    rs_typing_trainer::record::ttrec::write(&record(), &mut file).unwrap();
    let missing_config = directory.path().join("must-not-be-read.toml");
    let output = ProcessCommand::new(env!("CARGO_BIN_EXE_rs-typing-trainer"))
        .args([
            "--config",
            missing_config.to_str().unwrap(),
            "replay",
            record_path.to_str().unwrap(),
            "--print",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8(output.stdout).unwrap().contains("Keystrokes: 2"));
}

#[test]
fn replay_renderer_matches_the_playing_screen_except_for_its_status_row() {
    let config = replay_config();
    let mut app: common::TestApp = test_app(config.clone());
    let started = std::time::Instant::now();
    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    app.handle_key_at(enter, started);
    app.handle_key_at(enter, started);
    app.on_tick_at(Duration::from_secs(3), started).unwrap();
    app.on_tick_at(Duration::ZERO, started).unwrap();
    app.handle_key_at(
        KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
        started + Duration::from_millis(100),
    );

    let mut replay = Replay::new(two_a_record(&config), PlaybackTiming::Recorded { speed: 1.0 }, false, false);
    replay.advance_to(Duration::from_millis(100));

    let mut playing_terminal = Terminal::new(TestBackend::new(100, 28)).unwrap();
    playing_terminal
        .draw(|frame| rs_typing_trainer::ui::playing::render(frame, &app))
        .unwrap();
    let mut replay_terminal = Terminal::new(TestBackend::new(100, 28)).unwrap();
    replay_terminal
        .draw(|frame| rs_typing_trainer::ui::replay::render(frame, &replay, Lang::En))
        .unwrap();

    // The normal footer and the replay controls occupy the final inner row.
    let footer_y = 26;
    assert_eq!(
        buffer_cells_except_row(replay_terminal.backend().buffer(), footer_y),
        buffer_cells_except_row(playing_terminal.backend().buffer(), footer_y)
    );

    let replay_buffer = replay_terminal.backend().buffer();
    let text = buffer_text(replay_buffer);
    assert!(text.contains("▲"), "cursor missing: {text}");
    assert!(
        replay_buffer
            .content()
            .iter()
            .any(|cell| cell.symbol() == "a" && cell.style().fg == Some(Color::LightGreen)),
        "completed prompt color missing"
    );
    assert!(
        replay_buffer
            .content()
            .iter()
            .any(|cell| cell.symbol() == "a" && cell.style().add_modifier.contains(Modifier::UNDERLINED)),
        "prompt cursor style missing"
    );
    assert!(
        replay_buffer
            .content()
            .iter()
            .any(|cell| { cell.symbol() == "a" && matches!(cell.style().bg, Some(Color::Cyan | Color::Green)) }),
        "active-key highlight missing"
    );
    assert!(text.contains("Replay · 1/2 · 1x"), "{text}");
}

#[test]
fn finished_replay_keeps_the_last_playing_frame() {
    let config = replay_config();
    let mut replay = Replay::new(two_a_record(&config), PlaybackTiming::Recorded { speed: 1.0 }, false, false);
    replay.advance_to(Duration::from_secs(1));
    let mut terminal = Terminal::new(TestBackend::new(100, 28)).unwrap();
    terminal
        .draw(|frame| rs_typing_trainer::ui::replay::render(frame, &replay, Lang::En))
        .unwrap();
    let output = buffer_text(terminal.backend().buffer());

    assert!(output.contains("Time:"), "normal playing header missing: {output}");
    assert!(output.contains("▲"), "last prompt cursor missing: {output}");
    let compact = output.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    assert!(compact.contains("asdf"), "keyboard missing: {output}");
    assert!(output.contains("Replay finished · WPM 24.0 · Accuracy 100.0% · [Esc/Q] quit"), "{output}");
    assert!(!output.contains("Replay Result"), "result screen must not be rendered: {output}");

    let mut japanese_replay = Replay::new_with_lang(two_a_record(&config), PlaybackTiming::Recorded { speed: 1.0 }, false, false, Lang::Ja);
    japanese_replay.advance_to(Duration::from_secs(1));
    let mut japanese_terminal = Terminal::new(TestBackend::new(100, 28)).unwrap();
    japanese_terminal
        .draw(|frame| rs_typing_trainer::ui::replay::render(frame, &japanese_replay, Lang::Ja))
        .unwrap();
    let japanese = buffer_text(japanese_terminal.backend().buffer());
    let japanese_compact = japanese.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    assert!(japanese_compact.contains("リプレイ終了·WPM24.0·正確さ100.0%·[Esc/Q]終了"), "{japanese}");
    assert!(!japanese.contains("Accuracy"), "{japanese}");
}

#[test]
fn replay_switches_items_and_keeps_the_last_item_after_finishing() {
    let mut config = replay_config();
    config.mode.kind = rs_typing_trainer::config::ModeKind::Words;
    let mut session = Session::new(Duration::from_secs(2), Duration::ZERO, Item::exact("ab"));
    session.activate(Duration::ZERO);
    session.input_char(Duration::from_millis(100), "a");
    session.input_char(Duration::from_millis(200), "b");
    session.advance(Item::exact("cd"), Duration::from_millis(300));
    session.activate(Duration::from_millis(300));
    session.input_char(Duration::from_millis(400), "c");
    session.input_char(Duration::from_millis(500), "d");
    session.tick(Duration::from_secs(2));
    let record = SessionRecord::from_log(&config, session.into_log(), Utc.with_ymd_and_hms(2026, 11, 14, 0, 0, 0).unwrap());
    let mut replay = Replay::new(record, PlaybackTiming::Recorded { speed: 1.0 }, false, false);
    replay.advance_to(Duration::from_millis(400));

    let mut terminal = Terminal::new(TestBackend::new(100, 28)).unwrap();
    terminal
        .draw(|frame| rs_typing_trainer::ui::replay::render(frame, &replay, Lang::En))
        .unwrap();
    let switched = buffer_text(terminal.backend().buffer());
    assert!(switched.contains(" c  d "), "second item was not shown: {switched}");
    assert!(switched.contains("Replay · 3/4"), "{switched}");

    replay.advance_to(Duration::from_secs(2));
    terminal
        .draw(|frame| rs_typing_trainer::ui::replay::render(frame, &replay, Lang::En))
        .unwrap();
    let finished = buffer_text(terminal.backend().buffer());
    assert!(finished.contains(" c  d "), "last item was not retained: {finished}");
    assert!(finished.contains("Replay finished"), "{finished}");
    assert!(!finished.contains("Replay Result"), "{finished}");
}

#[test]
fn dvorak_words_sample_finishes_on_its_last_item() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/records/dvorak-words-en.ttrec");
    let input = std::fs::File::open(path).unwrap();
    let record = rs_typing_trainer::record::ttrec::read(std::io::BufReader::new(input)).unwrap();
    let duration = Duration::from_micros(record.log.duration_us);
    let mut replay = Replay::new(record, PlaybackTiming::Recorded { speed: 1.0 }, false, false);
    replay.advance_to(duration);
    let mut terminal = Terminal::new(TestBackend::new(110, 28)).unwrap();
    terminal
        .draw(|frame| rs_typing_trainer::ui::replay::render(frame, &replay, Lang::En))
        .unwrap();
    let output = buffer_text(terminal.backend().buffer());
    let compact = output.chars().filter(|character| !character.is_whitespace()).collect::<String>();

    assert!(compact.contains("stone"), "last item missing: {output}");
    assert!(compact.contains("aoeu"), "Dvorak keyboard missing: {output}");
    assert!(output.contains("Replay finished · WPM 59.0 · Accuracy 97.8%"), "{output}");
}

#[test]
fn sentence_replay_shows_colored_progress_cursor_and_mistype_feedback() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/records/dvorak-sentence-en.ttrec");
    let input = std::fs::File::open(path).unwrap();
    let record = rs_typing_trainer::record::ttrec::read(std::io::BufReader::new(input)).unwrap();
    let mut replay = Replay::new(record, PlaybackTiming::Recorded { speed: 1.0 }, false, false);
    replay.advance_to(Duration::from_micros(1_102_370));
    let mut terminal = Terminal::new(TestBackend::new(110, 28)).unwrap();
    terminal
        .draw(|frame| rs_typing_trainer::ui::replay::render(frame, &replay, Lang::En))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let central_cells = buffer
        .content()
        .iter()
        .enumerate()
        .filter(|(index, _)| {
            let y = *index as u16 / buffer.area.width;
            (3..19).contains(&y)
        })
        .map(|(_, cell)| cell)
        .collect::<Vec<_>>();
    let output = buffer_text(buffer);

    assert!(
        central_cells.iter().any(|cell| cell.style().fg == Some(Color::LightGreen)),
        "completed text color missing: {output}"
    );
    assert!(
        central_cells.iter().any(|cell| cell.style().add_modifier.contains(Modifier::UNDERLINED)),
        "current sentence position missing: {output}"
    );
    assert!(output.contains("▲"), "sentence cursor missing: {output}");

    replay.advance_to(Duration::from_micros(5_739_037));
    terminal
        .draw(|frame| rs_typing_trainer::ui::replay::render(frame, &replay, Lang::En))
        .unwrap();
    let mistype_buffer = terminal.backend().buffer();
    let mistype = buffer_text(mistype_buffer);
    assert!(mistype.contains("Mistype"), "mistype label missing: {mistype}");
    assert!(
        mistype_buffer.content().iter().any(|cell| cell.style().fg == Some(Color::LightRed)),
        "mistype color missing"
    );
}
