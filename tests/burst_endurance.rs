use rs_typing_trainer::config::{Config, ModeKind};
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::{KeystrokeKind, KeystrokeLog, Session, SessionLog};
use rs_typing_trainer::practice::session_variants::{BurstSeries, compare_endurance, compare_endurance_log};
use rs_typing_trainer::record::SessionRecord;
use std::time::Duration;

fn logged_key(seq: u32, item_idx: u32, t_us: u64) -> KeystrokeLog {
    KeystrokeLog {
        seq,
        t_us,
        release_us: None,
        item_idx,
        pos: 0,
        kind: KeystrokeKind::Char,
        typed: Some("a".into()),
        expected: Some("a".into()),
        correct: true,
        code: Some("KeyA".into()),
    }
}

#[test]
fn burst_keeps_the_best_of_three_runs() {
    let mut series = BurstSeries::default();
    assert_eq!(series.push(72.0), (1, 72.0));
    assert_eq!(series.push(81.0), (2, 81.0));
    assert_eq!(series.push(76.0), (3, 81.0));
    assert!(series.complete());
}

#[test]
fn endurance_warns_at_iki_or_error_thresholds() {
    let iki = compare_endurance(&[100.0, 100.0], &[115.0, 116.0], 0.02, 0.02);
    assert!(iki.warn);
    let errors = compare_endurance(&[100.0, 100.0], &[110.0, 110.0], 0.02, 0.03);
    assert!(errors.warn);
    let steady = compare_endurance(&[100.0, 100.0], &[114.0, 114.0], 0.02, 0.029);
    assert!(!steady.warn);
    let errors_from_zero = compare_endurance(&[100.0], &[100.0], 0.0, 0.01);
    assert!(errors_from_zero.warn);
}

#[test]
fn endurance_comparison_excludes_item_boundaries() {
    let log = SessionLog {
        duration_us: 4_000_000,
        error_mode: rs_typing_trainer::config::ErrorMode::Stop,
        items: vec![],
        keystrokes: vec![
            logged_key(0, 0, 100_000),
            logged_key(1, 0, 200_000),
            logged_key(2, 1, 1_000_000),
            logged_key(3, 2, 1_800_000),
            logged_key(4, 3, 2_100_000),
            logged_key(5, 3, 2_200_000),
            logged_key(6, 4, 3_000_000),
            logged_key(7, 5, 3_800_000),
        ],
    };

    let comparison = compare_endurance_log(&log);

    assert_eq!(comparison.first_iki_ms, 100.0);
    assert_eq!(comparison.second_iki_ms, 100.0);
    assert!(!comparison.warn);
}

#[test]
fn new_training_modes_are_recorded_by_distinct_session_names() {
    for (mode, expected) in [
        (ModeKind::Rhythm, "rhythm"),
        (ModeKind::Burst, "burst"),
        (ModeKind::Endurance, "endurance"),
    ] {
        let mut config = Config::default();
        config.mode.kind = mode;
        let mut session = Session::new(Duration::from_secs(1), Duration::ZERO, Item::exact("a"));
        session.activate(Duration::ZERO);
        session.input_char(Duration::from_millis(100), "a");
        session.tick(Duration::from_secs(1));
        let record = SessionRecord::from_log(&config, session.into_log(), chrono::Utc::now());
        assert_eq!(record.header.mode, expected);
    }
}
