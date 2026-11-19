use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::analysis::model::Prescription;
use rs_typing_trainer::analysis::prescription::{PrescriptionEvaluation, PrescriptionVerdict};
use rs_typing_trainer::config::Config;
use rs_typing_trainer::config::ModeKind;
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::record::{SessionRecord, db};
use rs_typing_trainer::ui;
use std::time::Duration;

use crate::common;
use common::test_app;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn render(app: &rs_typing_trainer::app::App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer.area.width, width);
    assert_eq!(buffer.area.height, height);
    buffer.content().iter().map(|cell| cell.symbol()).collect::<Vec<_>>().join("")
}

fn render_rows(app: &rs_typing_trainer::app::App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<Vec<_>>()
                .join("")
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_cells(app: &rs_typing_trainer::app::App, width: u16, height: u16) -> Vec<Vec<String>> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| (0..width).map(|x| buffer[(x, y)].symbol().to_owned()).collect())
        .collect()
}

fn row_contains(row: &[String], needle: &str) -> bool {
    row.concat().contains(needle)
}

fn numeric_end_before(row: &[String], unit: &str) -> usize {
    let unit_symbols = unit.chars().map(|character| character.to_string()).collect::<Vec<_>>();
    let unit_x = row.windows(unit_symbols.len()).position(|window| window == unit_symbols).unwrap();
    (0..unit_x)
        .rev()
        .find(|&x| row[x].chars().all(|character| character.is_ascii_digit() || character == '.'))
        .unwrap()
        + 1
}

fn compact(value: &str) -> String {
    value.chars().filter(|character| !character.is_whitespace()).collect()
}

#[test]
fn coach_tab_has_four_sections_and_clear_empty_state_at_both_sizes() {
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    let mut app = test_app(config);
    app.handle_key(key(KeyCode::Char('a')));

    let large = compact(&render(&app, 120, 40));
    for heading in ["今の位置", "前回の処方の結果", "いちばんの伸びしろ", "習慣"] {
        assert!(large.contains(heading), "missing {heading}: {large}");
    }
    assert!(large.contains("記録が溜まると出ます"), "{large}");
    assert!(large.contains("adaptiveで練習すると"), "{large}");

    let small = render(&app, 80, 24);
    assert!(compact(&small).contains("今の位置"), "{small}");
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Down));
    assert!(app.analysis_summary_scroll > 0);
}

#[test]
fn keyboard_tab_cycles_four_views_and_confusion_selects_a_key() {
    let mut config = Config::default();
    config.general.lang = Lang::En;
    let mut app = test_app(config);
    app.handle_key(key(KeyCode::Char('a')));
    app.handle_key(key(KeyCode::Tab));
    app.handle_key(key(KeyCode::Tab));

    let mut labels = vec![render(&app, 120, 40)];
    for _ in 0..3 {
        app.handle_key(key(KeyCode::Enter));
        labels.push(render(&app, 120, 40));
    }
    assert!(labels[0].contains("Error-rate"), "{}", labels[0]);
    assert!(labels[1].contains("Median IKI"), "{}", labels[1]);
    assert!(labels[2].contains("Relative speed"), "{}", labels[2]);
    assert!(labels[3].contains("Confusion"), "{}", labels[3]);

    app.handle_key(key(KeyCode::Left));
    assert_eq!(
        app.analysis_tab,
        rs_typing_trainer::app::ANALYSIS_KEYBOARD_TAB,
        "arrow keys select confusion keys instead of changing tabs"
    );
}

#[test]
fn coach_goal_picker_saves_the_selected_goal() {
    let directory = tempfile::TempDir::new().unwrap();
    let db_path = directory.path().join("sessions.db");
    let mut record_config = Config::default();
    record_config.mode.kind = ModeKind::Words;
    let mut session = Session::new(Duration::from_secs(1), Duration::ZERO, Item::exact("as"));
    session.activate(Duration::ZERO);
    session.input_char(Duration::from_millis(100), "a");
    session.input_char(Duration::from_millis(200), "s");
    session.tick(Duration::from_secs(1));
    let mut record = SessionRecord::from_log(&record_config, session.into_log(), chrono::Utc::now());
    let mut later_error = record.log.keystrokes[0].clone();
    later_error.seq = 3;
    later_error.t_us = 360_000_000;
    later_error.typed = Some("z".into());
    later_error.expected = Some("a".into());
    later_error.correct = false;
    record.log.keystrokes.push(later_error);
    record.log.duration_us = 361_000_000;
    db::save(&db_path, &record).unwrap();
    db::save_prescription(
        &db_path,
        &Prescription {
            id: None,
            created_at: chrono::Utc::now(),
            layout: "qwerty".into(),
            kind: "transition".into(),
            target: serde_json::json!({"from": "a", "to": "s"}),
            control: serde_json::json!([]),
            hypothesis: serde_json::json!({"sessions": 3}),
            baseline: serde_json::json!({}),
            status: "effective".into(),
            evaluated_at: Some(chrono::Utc::now()),
            result: Some(
                serde_json::to_value(PrescriptionEvaluation {
                    target_delta_ms: -24.0,
                    control_delta_ms: -2.0,
                    effect: -0.08,
                    z: -1.9,
                    verdict: PrescriptionVerdict::Effective,
                    closed: true,
                })
                .unwrap(),
            ),
        },
    )
    .unwrap();

    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    config.storage.db_path = Some(db_path.clone());
    let mut app = test_app(config);
    app.handle_key(key(KeyCode::Char('a')));
    let coach = compact(&render(&app, 120, 40));
    assert!(coach.contains("効いた"), "{coach}");
    assert!(coach.contains("誤り率が上がり始める区間"), "{coach}");

    let cells = render_cells(&app, 80, 24);
    let speed = cells.iter().find(|row| row_contains(row, "速") && row_contains(row, "WPM")).unwrap();
    let accuracy = cells.iter().find(|row| row_contains(row, "正") && row_contains(row, "%")).unwrap();
    let transition = cells.iter().find(|row| row_contains(row, "遷") && row_contains(row, "%")).unwrap();
    let grade_columns = [speed, accuracy, transition].map(|row| {
        row.iter()
            .enumerate()
            .find(|(x, symbol)| *x > 2 && matches!(symbol.as_str(), "S" | "A" | "B" | "C" | "D" | "E" | "F" | "G"))
            .map(|(x, _)| x)
            .unwrap()
    });
    assert_eq!(grade_columns[0], grade_columns[1], "{}", render_rows(&app, 80, 24));
    assert_eq!(grade_columns[1], grade_columns[2], "{}", render_rows(&app, 80, 24));
    let value_ends = [
        numeric_end_before(speed, "WPM"),
        numeric_end_before(accuracy, "%"),
        numeric_end_before(transition, "%"),
    ];
    assert_eq!(value_ends[0], value_ends[1], "{}", render_rows(&app, 80, 24));
    assert_eq!(value_ends[1], value_ends[2], "{}", render_rows(&app, 80, 24));

    let bullet_y = cells
        .iter()
        .enumerate()
        .find(|(y, row)| row_contains(row, "•") && *y + 1 < cells.len() && !row_contains(&cells[*y + 1], "•"))
        .map(|(y, _)| y)
        .unwrap();
    let bullet_x = cells[bullet_y].iter().position(|symbol| symbol == "•").unwrap();
    let continuation_x = cells[bullet_y + 1]
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, symbol)| !symbol.is_empty() && symbol.as_str() != " ")
        .map(|(x, _)| x)
        .unwrap();
    assert_eq!(continuation_x, bullet_x + 2, "{}", render_rows(&app, 80, 24));
    if std::env::var_os("RS_TYPING_TRAINER_DUMP_UI").is_some() {
        println!("\n--- COACH 80x24 ---\n{}", render_rows(&app, 80, 24));
        println!("\n--- COACH 120x40 ---\n{}", render_rows(&app, 120, 40));
    }
    app.handle_key(key(KeyCode::Char('T')));
    let picker = compact(&render(&app, 80, 24));
    assert!(picker.contains("目標の候補"), "{picker}");
    app.handle_key(key(KeyCode::Enter));
    assert!(db::load_selected_goal(&db_path).unwrap().is_some());
}

#[test]
fn result_feedback_obeys_summary_and_detailed_limits() {
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    let mut app = test_app(config.clone());
    app.last_result = Some(rs_typing_trainer::metrics::stats::GameResult::from_metrics(
        &config,
        Default::default(),
        Duration::from_secs(60),
    ));
    app.result_insights = vec!["気づき一".into(), "気づき二".into(), "気づき三".into()];
    app.screen = rs_typing_trainer::app::Screen::Result;
    let summary = compact(&render(&app, 120, 40));
    assert!(summary.contains("今回の気づき"), "{summary}");
    assert!(summary.contains("気づき一"), "{summary}");
    assert!(!summary.contains("気づき二"), "{summary}");
    let compact_result = compact(&render(&app, 80, 24));
    assert!(compact_result.contains("今回の気づき"), "{compact_result}");
    assert!(compact_result.contains("気づき一"), "{compact_result}");

    app.config.display.feedback = "detailed".into();
    let detailed = compact(&render(&app, 120, 40));
    for line in ["気づき一", "気づき二", "気づき三"] {
        assert!(detailed.contains(line), "missing {line}: {detailed}");
    }
}

#[test]
fn benchmark_result_and_mode_select_show_cached_grade_badges() {
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    let mut app = test_app(config.clone());
    app.last_result = Some(rs_typing_trainer::metrics::stats::GameResult::from_metrics(
        &config,
        Default::default(),
        Duration::from_secs(60),
    ));
    app.active_bench_id = Some("bench-v1/A/01".into());
    app.result_grade_line = Some("英語 B（暫定）  C → B に昇格".into());
    app.screen = rs_typing_trainer::app::Screen::Result;
    let result = compact(&render(&app, 120, 40));
    assert!(result.contains("英語B（暫定）"), "{result}");
    assert!(result.contains("C→Bに昇格"), "{result}");

    app.mode_grade_line = Some("英 B・日 E（仮）".into());
    app.screen = rs_typing_trainer::app::Screen::ModeSelect;
    assert!(compact(&render(&app, 80, 24)).contains("英B・日E（仮）"));
    if std::env::var_os("RS_TYPING_TRAINER_DUMP_UI").is_some() {
        app.screen = rs_typing_trainer::app::Screen::Result;
        println!("\n--- BENCH RESULT 120x40 ---\n{}", render_rows(&app, 120, 40));
        app.screen = rs_typing_trainer::app::Screen::ModeSelect;
        println!("\n--- MODE SELECT 80x24 ---\n{}", render_rows(&app, 80, 24));
    }
}
