use chrono::{TimeZone, Utc};
use rs_typing_trainer::app::build_result_comparison;
use rs_typing_trainer::config::Config;
use rs_typing_trainer::engine::session::{ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};
use rs_typing_trainer::record::SessionRecord;
use rs_typing_trainer::ui::result::item_speed_points;

fn record(id: i64, hour: u32, wpm: f64, cps: f64, accuracy: f64) -> SessionRecord {
    let mut record = SessionRecord::from_log(
        &Config::default(),
        SessionLog::default(),
        Utc.with_ymd_and_hms(2026, 11, 16, hour, 0, 0).unwrap(),
    );
    record.header.id = Some(id);
    record.header.mode = "words".into();
    record.header.mode_params = serde_json::json!({"score_eligible": true}).to_string();
    record.header.net_wpm = wpm;
    record.header.cps = cps;
    record.header.accuracy = accuracy;
    record
}

#[test]
fn item_speed_uses_correct_character_count_and_item_completion_duration() {
    let items = vec![
        ItemLog {
            idx: 0,
            text: "first".into(),
            reading: None,
            kind: None,
            shown_us: 0,
            done_us: Some(1_000_000),
            outcome: ItemOutcome::Completed,
        },
        ItemLog {
            idx: 1,
            text: "second".into(),
            reading: None,
            kind: None,
            shown_us: 1_000_000,
            done_us: Some(3_000_000),
            outcome: ItemOutcome::Completed,
        },
    ];
    let mut keystrokes = Vec::new();
    for (seq, correct) in [true, true, false, true, true, true].into_iter().enumerate() {
        keystrokes.push(KeystrokeLog {
            seq: seq as u32,
            t_us: 100_000 + seq as u64 * 100_000,
            release_us: None,
            item_idx: 0,
            pos: seq as u32,
            kind: KeystrokeKind::Char,
            typed: Some("a".into()),
            expected: Some("a".into()),
            correct,
            code: Some("KeyA".into()),
        });
    }
    for seq in 0..5 {
        keystrokes.push(KeystrokeLog {
            seq: 6 + seq,
            t_us: 1_200_000 + u64::from(seq) * 200_000,
            release_us: None,
            item_idx: 1,
            pos: seq,
            kind: KeystrokeKind::Char,
            typed: Some("s".into()),
            expected: Some("s".into()),
            correct: true,
            code: Some("KeyS".into()),
        });
    }
    let log = SessionLog {
        duration_us: 3_000_000,
        error_mode: Default::default(),
        items,
        keystrokes,
    };
    let mut result_record = SessionRecord::from_log(&Config::default(), log, Utc.with_ymd_and_hms(2026, 11, 16, 8, 0, 0).unwrap());
    result_record.header.metrics_version = 2;

    let points = item_speed_points(&result_record);
    assert_eq!(points.len(), 2);
    assert!((points[0].wpm - 60.0).abs() < 1e-9);
    assert_eq!(points[0].mistypes, Some(1));
    assert!((points[1].wpm - 30.0).abs() < 1e-9);
    assert_eq!(points[1].mistypes, Some(0));
}

#[test]
fn historical_comparison_excludes_every_record_after_the_selected_record() {
    let mut records = (0..12)
        .map(|offset| {
            record(
                i64::from(offset + 1),
                1 + offset,
                30.0 + f64::from(offset),
                3.0 + f64::from(offset) / 10.0,
                0.80,
            )
        })
        .collect::<Vec<_>>();
    let current = record(20, 13, 50.0, 5.0, 0.95);
    records.push(current.clone());
    records.push(record(21, 14, 999.0, 99.0, 1.0));

    let comparison = build_result_comparison(&records, &current);

    assert_eq!(comparison.recent_wpm.len(), 10);
    assert_eq!(comparison.recent_wpm.last(), Some(&50.0));
    assert!(!comparison.recent_wpm.contains(&999.0));
    assert_eq!(comparison.previous_wpm_delta, Some(9.0));
    assert_eq!(comparison.best_wpm, Some(50.0));
    assert_eq!(comparison.best_wpm_delta, Some(9.0));
    assert!((comparison.previous_cps_delta.unwrap() - 0.9).abs() < 1e-9);
    assert!((comparison.previous_accuracy_delta.unwrap() - 0.15).abs() < 1e-9);
}

#[test]
fn comparison_reports_zero_against_an_equal_previous_best() {
    let previous = record(1, 8, 50.0, 5.0, 0.95);
    let current = record(2, 9, 50.0, 5.0, 0.95);

    let comparison = build_result_comparison(&[previous, current.clone()], &current);

    assert_eq!(comparison.best_wpm, Some(50.0));
    assert_eq!(comparison.best_wpm_delta, Some(0.0));
    assert_eq!(comparison.best_accuracy_delta, Some(0.0));
}

#[test]
fn historical_comparison_uses_benchmark_series_and_part_as_the_condition() {
    let mut same_part = record(1, 8, 40.0, 4.0, 0.9);
    same_part.header.mode = "bench".into();
    same_part.header.bench_id = Some("bench-v2/A/01".into());
    let mut other_part = record(2, 9, 90.0, 9.0, 1.0);
    other_part.header.mode = "bench".into();
    other_part.header.bench_id = Some("bench-v2/B/01".into());
    let mut current = record(3, 10, 55.0, 5.5, 0.95);
    current.header.mode = "bench".into();
    current.header.bench_id = Some("bench-v2/A/02".into());

    let comparison = build_result_comparison(&[same_part, other_part, current.clone()], &current);
    assert_eq!(comparison.recent_wpm, vec![40.0, 55.0]);
    assert_eq!(comparison.previous_wpm_delta, Some(15.0));
}

#[test]
fn item_speed_omits_unavailable_v1_keystroke_details() {
    let mut old = record(1, 8, 40.0, 4.0, 0.9);
    old.header.metrics_version = 0;
    old.log = SessionLog::default();
    assert!(item_speed_points(&old).is_empty());
}
