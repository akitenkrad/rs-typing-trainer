use std::time::Duration;

use chrono::{TimeZone, Utc};
use rs_typing_trainer::config::{Config, ModeKind};
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::practice::ghost::{GhostCondition, select_ghost};
use rs_typing_trainer::record::SessionRecord;

fn record(text: &str, times_ms: &[u64], wpm: f64, bench_id: Option<&str>, second: u32) -> SessionRecord {
    let mut config = Config::default();
    config.mode.kind = ModeKind::Sentence;
    let mut session = Session::new(Duration::from_secs(5), Duration::ZERO, Item::exact(text));
    session.activate(Duration::ZERO);
    for (character, time) in text.chars().zip(times_ms) {
        session.input_char(Duration::from_millis(*time), &character.to_string());
    }
    session.tick(Duration::from_secs(5));
    let mut record = SessionRecord::from_log(&config, session.into_log(), Utc.with_ymd_and_hms(2026, 11, 14, 0, 0, second).unwrap());
    record.header.net_wpm = wpm;
    if let Some(id) = bench_id {
        record.header.mode = "bench".into();
        record.header.bench_id = Some(id.into());
    }
    record
}

#[test]
fn sentence_ghost_requires_the_same_sentence_and_uses_the_best_run() {
    let slow = record("rust", &[100, 300, 500, 700], 40.0, None, 1);
    let mut fast = record("rust", &[100, 200, 300, 400], 60.0, None, 2);
    fast.header.id = Some(42);
    let other = record("type", &[100, 200, 300, 400], 80.0, None, 3);
    let ghost = select_ghost(&[slow, fast, other], &GhostCondition::Sentence { text: "rust".into() }).unwrap();
    assert_eq!(ghost.position_at(Duration::from_millis(250)), 2);
    assert_eq!(ghost.opponent(), "session:42");
    assert!(select_ghost(&[], &GhostCondition::Sentence { text: "rust".into() }).is_none());
}

#[test]
fn benchmark_ghost_requires_the_same_version_and_uses_the_previous_run() {
    let older = record("rust", &[100, 200, 300, 400], 70.0, Some("bench-v1/A/03"), 1);
    let previous = record("rust", &[100, 300, 500, 700], 50.0, Some("bench-v1/A/03"), 2);
    let other = record("rust", &[100, 150, 200, 250], 90.0, Some("bench-v1/A/04"), 3);
    let ghost = select_ghost(
        &[older, previous, other],
        &GhostCondition::Bench {
            bench_id: "bench-v1/A/03".into(),
        },
    )
    .unwrap();
    assert_eq!(ghost.position_at(Duration::from_millis(450)), 2);
    assert_eq!(ghost.opponent(), "bench:bench-v1/A/03");
    assert!(
        select_ghost(
            &[],
            &GhostCondition::Bench {
                bench_id: "bench-v1/A/03".into()
            }
        )
        .is_none()
    );
}

#[test]
fn sentence_ghost_ignores_ineligible_bests_and_does_not_apply_to_other_text() {
    let eligible = record("rust", &[100, 200, 300, 400], 50.0, None, 1);
    let mut ineligible = record("rust", &[100, 150, 200, 250], 90.0, None, 2);
    ineligible.header.mode_params = serde_json::json!({ "score_eligible": false }).to_string();
    let ghost = select_ghost(&[eligible, ineligible], &GhostCondition::Sentence { text: "rust".into() }).unwrap();
    assert_eq!(ghost.position_at(Duration::from_millis(175)), 1);
    assert!(ghost.applies_to_sentence("rust"));
    assert!(!ghost.applies_to_sentence("type"));
}

mod common;

#[test]
fn ghost_marker_sits_above_the_large_character_it_has_reached() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::Style;
    use rs_typing_trainer::ui::bigtext::{BigTextSize, StyledCharacter, font};

    // The ghost typed "ru" almost at once and "st" seconds later, so while the
    // screen below is drawn it has reached position 2 ('s').
    let ghost = select_ghost(
        &[record("rust", &[1, 2, 4_000, 4_500], 40.0, None, 1)],
        &GhostCondition::Sentence { text: "rust".into() },
    )
    .unwrap();
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("sentences.tsv");
    std::fs::write(&path, "rust\n").unwrap();
    let mut config = Config::default();
    config.mode.kind = ModeKind::Sentence;
    config.mode.sentence.lang = "en".into();
    config.data.sentences_en = Some(path);
    config.ui.animation_enabled = false;
    let mut app = common::test_app(config);
    let start = std::time::Instant::now();
    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    app.handle_key_at(enter, start);
    app.handle_key_at(enter, start);
    app.on_tick_at(Duration::from_secs(3), start).unwrap();
    app.on_tick_at(Duration::ZERO, start + Duration::from_millis(200)).unwrap();
    app.on_tick_at(Duration::ZERO, start + Duration::from_millis(400)).unwrap();
    let game = app.game.as_mut().unwrap();
    assert_eq!(game.current_prompt.text, "rust");
    game.ghost = Some(ghost);

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| rs_typing_trainer::ui::render(frame, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let marker = buffer
        .content()
        .iter()
        .position(|cell| cell.symbol() == "◆")
        .map(|index| (index as u16 % buffer.area.width, index as u16 / buffer.area.width))
        .expect("ghost marker missing");
    let dot12 = font(BigTextSize::Dot12).unwrap();
    let left = 1 + (78 - dot12.text_width("rust")) / 2;
    assert_eq!(marker.0, left + dot12.text_width("ru"), "marker={marker:?}");

    let raster = dot12.render(&[StyledCharacter::new('s', Style::default())]);
    for (dy, line) in raster.lines.iter().enumerate() {
        let expected = line.spans.iter().map(|span| span.content.as_ref()).collect::<String>();
        let actual = (0..dot12.text_width("s"))
            .map(|dx| buffer[(marker.0 + dx, marker.1 + 1 + dy as u16)].symbol())
            .collect::<String>();
        assert_eq!(actual, expected, "row {dy} below the marker must be the 's' glyph");
    }
}
