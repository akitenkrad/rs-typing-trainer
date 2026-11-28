//! The growth tab reuses the analysis report's progress only when both are
//! built from the same records, and shows what the full report would give.

use chrono::{NaiveDate, TimeZone, Utc};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::analysis::coach::{self, ProgressInputs};
use rs_typing_trainer::analysis::growth::{GrowthReport, build_growth, menu_graduations, skill_promotions};
use rs_typing_trainer::app::Screen;
use rs_typing_trainer::config::{Config, ErrorMode};
use rs_typing_trainer::engine::session::{ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};
use rs_typing_trainer::record::db;
use rs_typing_trainer::record::{SessionHeader, SessionRecord};
use tempfile::TempDir;

fn record(at: chrono::DateTime<Utc>, text: &str, speed_scale: f64) -> SessionRecord {
    let chars = text.chars().collect::<Vec<_>>();
    let mut time_us = 100_000_u64;
    let mut keys = Vec::new();
    for (index, character) in chars.iter().enumerate() {
        if index > 0 {
            let base_ms = match (chars[index - 1], *character) {
                ('a', 'c') | ('c', 'a') => 240.0,
                _ => 120.0,
            };
            time_us += (base_ms * speed_scale * 1_000.0) as u64;
        }
        keys.push(KeystrokeLog {
            seq: index as u32,
            t_us: time_us,
            release_us: None,
            item_idx: 0,
            pos: index as u32,
            kind: KeystrokeKind::Char,
            typed: Some(character.to_string()),
            expected: Some(character.to_string()),
            correct: true,
            code: None,
        });
    }
    let transition_ms = (time_us - 100_000) as f64 / 1_000.0;
    let net_wpm = chars.len() as f64 / 5.0 / (transition_ms / 60_000.0);
    SessionRecord {
        header: SessionHeader {
            id: None,
            started_at: at.to_rfc3339(),
            finished_at: (at + chrono::Duration::milliseconds(time_us as i64 / 1_000)).to_rfc3339(),
            duration_ms: time_us as i64 / 1_000,
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
            net_wpm,
            raw_wpm: Some(net_wpm),
            accuracy: 0.97,
            iki_median_ms: 120.0,
            iki_cv: 0.0,
            cps: 0.0,
            consistency: 1.0,
            max_combo: chars.len() as u32,
            completed_words: 1,
            timeout_words: 0,
            mistypes_total: 0,
            legacy_payload: None,
        },
        log: SessionLog {
            duration_us: time_us,
            error_mode: ErrorMode::Stop,
            items: vec![ItemLog {
                idx: 0,
                text: text.to_owned(),
                reading: None,
                kind: None,
                shown_us: 0,
                done_us: Some(time_us),
                outcome: ItemOutcome::Completed,
            }],
            keystrokes: keys,
        },
    }
}

/// One session a day for `days` days ending on `today`, getting faster.
fn seeded(days: i64, today: NaiveDate) -> (TempDir, crate::common::TestApp) {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    let end = Utc.from_utc_datetime(&today.and_hms_opt(12, 0, 0).unwrap());
    for day in 0..days {
        let at = end - chrono::Duration::days(days - 1 - day);
        let text = "abacdabc".repeat(40);
        db::save(&path, &record(at, &text, 1.0 - day as f64 * 0.005)).unwrap();
    }
    let mut config = Config::default();
    config.storage.db_path = Some(path);
    (directory, crate::common::test_app_at(config, today))
}

/// What the growth tab showed before it reused the analysis report: the
/// progress of a full report over every record.
fn growth_from_the_full_report(app: &rs_typing_trainer::app::App, today: NaiveDate) -> (GrowthReport, usize) {
    let path = app.db_path();
    let records = db::load_records_filtered(&path, None, None).unwrap();
    let prescriptions = db::load_prescriptions(&path).unwrap();
    let selected_goal = db::load_selected_goal(&path).unwrap();
    let progress = coach::build(
        &records,
        &app.config,
        &app.home_dir,
        &prescriptions,
        selected_goal,
        today,
        app.config.general.lang,
    )
    .map(|report| report.progress)
    .unwrap_or_default();
    // The fixture gives a progress worth comparing.
    assert!(!progress.forms.is_empty(), "no forms");
    assert!(!progress.trends.is_empty(), "no trends");
    let mut growth = build_growth(&records, &progress, today);
    growth.promotions = skill_promotions(&db::load_skill_snapshots(&path).unwrap());
    assert!(db::load_all_menu_runs(&path).unwrap().is_empty());
    growth.graduations = menu_graduations(&[]);
    (growth, records.len())
}

fn open_analysis(app: &mut rs_typing_trainer::app::App) {
    app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    assert_eq!(app.screen, Screen::Analysis);
}

#[test]
fn growth_reuses_the_report_progress_when_the_period_holds_every_record() {
    let today = NaiveDate::from_ymd_opt(2026, 11, 26).unwrap();
    let (_directory, mut app) = seeded(20, today);
    open_analysis(&mut app);
    let (expected, total) = growth_from_the_full_report(&app, today);
    assert_eq!(app.analysis_records.len(), total, "30 days hold all 20 sessions");
    let all = db::load_records_filtered(&app.db_path(), None, None).unwrap();
    assert_eq!(
        app.analysis_coach_inputs,
        ProgressInputs::new(&all, &app.config, &app.home_dir, today),
        "the report was built from every record, so the growth tab reused it"
    );
    assert_eq!(app.analysis_growth, expected);
}

#[test]
fn growth_builds_its_own_progress_when_the_period_leaves_records_out() {
    let today = NaiveDate::from_ymd_opt(2026, 11, 26).unwrap();
    let (_directory, mut app) = seeded(50, today);
    open_analysis(&mut app);
    let (expected, total) = growth_from_the_full_report(&app, today);
    assert!(app.analysis_records.len() < total, "30 days leave the older sessions out");
    let all = db::load_records_filtered(&app.db_path(), None, None).unwrap();
    assert!(app.analysis_coach_inputs.is_some());
    assert_ne!(app.analysis_coach_inputs, ProgressInputs::new(&all, &app.config, &app.home_dir, today));
    assert_eq!(app.analysis_growth, expected);
}

#[test]
fn progress_alone_equals_the_progress_of_the_full_report() {
    let today = NaiveDate::from_ymd_opt(2026, 11, 26).unwrap();
    let (_directory, app) = seeded(50, today);
    let path = app.db_path();
    let records = db::load_records_filtered(&path, None, None).unwrap();
    for subset in [&records[..], &records[30..]] {
        let full = coach::build(subset, &app.config, &app.home_dir, &[], None, today, app.config.general.lang)
            .unwrap()
            .progress;
        assert_eq!(coach::build_progress(subset, &app.config, &app.home_dir, today).unwrap(), full);
    }
}

#[test]
fn progress_inputs_compare_the_record_ids_not_the_count() {
    let today = NaiveDate::from_ymd_opt(2026, 11, 26).unwrap();
    let (_directory, app) = seeded(4, today);
    let records = db::load_records_filtered(&app.db_path(), None, None).unwrap();
    let inputs = |rows: &[SessionRecord]| ProgressInputs::new(rows, &app.config, &app.home_dir, today);
    let mut reversed = records.clone();
    reversed.reverse();
    assert_eq!(inputs(&records), inputs(&reversed), "order does not matter");
    let mut swapped = records.clone();
    swapped[0] = records[3].clone();
    assert_ne!(inputs(&records), inputs(&swapped), "same count, different records");
    let mut unsaved = records.clone();
    unsaved[0].header.id = None;
    assert_eq!(inputs(&unsaved), None, "a record without an id is never the same");
    assert_ne!(
        inputs(&records),
        ProgressInputs::new(&records, &app.config, &app.home_dir, today.succ_opt().unwrap())
    );
}
