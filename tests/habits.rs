use chrono::{TimeZone, Utc};
use rs_typing_trainer::analysis::habits::{AssistanceKind, HabitForm, analyze_assistance, analyze_habits};
use rs_typing_trainer::analysis::progress::SessionForm;
use rs_typing_trainer::config::ErrorMode;
use rs_typing_trainer::engine::session::{ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};
use rs_typing_trainer::record::{SessionHeader, SessionModeParams, SessionRecord};

#[test]
fn unknown_assistance_is_excluded_and_balanced_samples_can_be_removed() {
    let mut forms = Vec::new();
    for index in 0..5 {
        for (enabled, form) in [(true, 1.0), (false, 0.98)] {
            forms.push(HabitForm {
                session_id: Some(index * 2 + i64::from(enabled)),
                started_at: Utc.with_ymd_and_hms(2026, 1, 1 + index as u32, 12, 0, 0).unwrap(),
                finished_at: Utc.with_ymd_and_hms(2026, 1, 1 + index as u32, 12, 1, 0).unwrap(),
                form,
                mode_params: Some(SessionModeParams {
                    blind: Some(enabled),
                    ..SessionModeParams::default()
                }),
            });
        }
    }
    forms.push(HabitForm {
        session_id: Some(99),
        started_at: Utc.with_ymd_and_hms(2026, 1, 9, 12, 0, 0).unwrap(),
        finished_at: Utc.with_ymd_and_hms(2026, 1, 9, 12, 1, 0).unwrap(),
        form: 9.0,
        mode_params: None,
    });

    let result = analyze_assistance(&forms);
    let blind = result.iter().find(|row| row.kind == AssistanceKind::Blind).unwrap();
    assert_eq!((blind.with_n, blind.without_n), (5, 5));
    assert!(blind.removable);
}

#[test]
fn warmup_deterioration_time_of_day_and_short_history_are_hand_calculable() {
    let start = Utc.with_ymd_and_hms(2026, 1, 1, 6, 0, 0).unwrap();
    let records = [
        error_record(1, start, 1, 1.0),
        error_record(2, start + chrono::Duration::minutes(6), 1, 1.0),
        error_record(3, start + chrono::Duration::minutes(12), 2, 1.0),
        error_record(4, start + chrono::Duration::minutes(21), 2, 1.0),
    ];
    let forms = [
        session_form(1, start, 0.90),
        session_form(2, start + chrono::Duration::minutes(6), 1.00),
        session_form(3, start + chrono::Duration::hours(4), 1.10),
    ];
    let result = analyze_habits(&records, &forms);
    assert_eq!((result.warmup.after_break_n, result.warmup.other_n), (2, 1));
    assert_eq!(result.warmup.after_break_form_median, Some(1.0));
    assert_eq!(result.warmup.other_form_median, Some(1.0));
    assert_eq!(result.deterioration.first_deteriorated_bucket.as_deref(), Some("10-20"));
    assert!(result.time_of_day.iter().all(|row| row.exploratory));
    assert!(result.practice.plateau_minutes.is_none());
}

#[test]
fn practice_plateau_needs_fourteen_days() {
    let start = Utc.with_ymd_and_hms(2026, 2, 1, 12, 0, 0).unwrap();
    let mut records = Vec::new();
    let mut forms = Vec::new();
    for day in 0..14 {
        let at = start + chrono::Duration::days(day);
        let minutes = day as f64 + 1.0;
        records.push(error_record(day * 2 + 1, at, 0, minutes));
        forms.push(session_form(day * 2 + 1, at, 1.0));
        forms.push(session_form(
            day * 2 + 2,
            at + chrono::Duration::minutes(minutes as i64),
            1.0 + minutes.min(7.0) / 100.0,
        ));
    }
    let result = analyze_habits(&records, &forms);
    assert!(result.practice.plateau_minutes.is_some(), "{:?}", result.practice);

    let short = analyze_habits(&records[..13], &forms[..26]);
    assert!(short.practice.plateau_minutes.is_none());
}

fn session_form(id: i64, at: chrono::DateTime<Utc>, form: f64) -> SessionForm {
    SessionForm {
        session_id: Some(id),
        started_at: at,
        finished_at: at + chrono::Duration::minutes(1),
        mode: "words".to_owned(),
        language: "en".to_owned(),
        net_wpm: 50.0,
        expected_wpm: 50.0 / form,
        form,
        corrected_wpm: 50.0,
        coverage: 1.0,
    }
}

fn error_record(id: i64, at: chrono::DateTime<Utc>, errors: usize, minutes: f64) -> SessionRecord {
    let keys = (0..10)
        .map(|index| KeystrokeLog {
            seq: index,
            t_us: u64::from(index) * 100_000,
            release_us: None,
            item_idx: 0,
            pos: index,
            kind: KeystrokeKind::Char,
            typed: Some("a".to_owned()),
            expected: Some("a".to_owned()),
            correct: index as usize >= errors,
            code: None,
        })
        .collect::<Vec<_>>();
    let duration_ms = (minutes * 60_000.0) as i64;
    SessionRecord {
        header: SessionHeader {
            id: Some(id),
            started_at: at.to_rfc3339(),
            finished_at: (at + chrono::Duration::milliseconds(duration_ms)).to_rfc3339(),
            duration_ms,
            mode: "words".to_owned(),
            mode_params: serde_json::json!({"lang": "en"}).to_string(),
            bench_id: None,
            layout: "qwerty".to_owned(),
            input_layout: "same".to_owned(),
            geometry: "ansi".to_owned(),
            error_mode: "stop".to_owned(),
            release_events: false,
            app_version: "test".to_owned(),
            metrics_version: 2,
            net_wpm: 50.0,
            raw_wpm: Some(50.0),
            accuracy: 1.0 - errors as f64 / 10.0,
            iki_median_ms: 100.0,
            iki_cv: 0.0,
            cps: 0.0,
            consistency: 1.0,
            max_combo: 10,
            completed_words: 1,
            timeout_words: 0,
            mistypes_total: errors as u32,
            legacy_payload: None,
        },
        log: SessionLog {
            duration_us: duration_ms.max(0) as u64 * 1_000,
            error_mode: ErrorMode::Stop,
            items: vec![ItemLog {
                idx: 0,
                text: "aaaaaaaaaa".to_owned(),
                reading: None,
                kind: None,
                shown_us: 0,
                done_us: Some(900_000),
                outcome: ItemOutcome::Completed,
            }],
            keystrokes: keys,
        },
    }
}
