use rs_typing_trainer::config::{Config, ModeKind};
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::practice::session_variants::{BurstSeries, compare_endurance};
use rs_typing_trainer::record::SessionRecord;
use std::time::Duration;

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
