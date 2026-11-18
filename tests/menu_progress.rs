use chrono::{NaiveDate, TimeZone, Utc};
use rs_typing_trainer::analysis::model::Prescription;
use rs_typing_trainer::practice::menu::{
    AdvanceOutcome, PrescriptionJudgment, PrescriptionProgress, ReferenceJudgment, apply_advance_outcome, apply_maintenance_recheck,
    apply_reference_judgment, build_from_storage, bundled_catalog, evaluate_advance_from_sources, reference_judgment,
};
use rs_typing_trainer::record::db;
use tempfile::TempDir;

fn table_count(connection: &rusqlite::Connection, table: &str) -> i64 {
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0))
        .unwrap()
}

#[test]
fn v7_database_migrates_once_to_menu_schema_without_losing_sessions() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    db::list(&path).unwrap();
    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "DROP TABLE IF EXISTS menu_steps;
             DROP TABLE IF EXISTS menu_runs;
             DROP TABLE IF EXISTS skill_snapshots;
             PRAGMA user_version = 7;",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO sessions (
                id, started_at, finished_at, duration_ms, mode, mode_params,
                layout, input_layout, geometry, error_mode, release_events,
                app_version, metrics_version, net_wpm, accuracy, score_eligible,
                iki_median_ms, iki_cv, cps, consistency, max_combo,
                completed_words, timeout_words, mistypes_total
             ) VALUES (
                41, '2026-11-17T08:00:00Z', '2026-11-17T08:01:00Z', 60000,
                'words', '{}', 'qwerty', 'same', 'ansi', 'stop', 0,
                '0.1.0', 1, 50.0, 0.98, 1, 180.0, 0.2, 4.0, 0.8, 10, 20, 0, 1
             )",
                [],
            )
            .unwrap();
    }

    assert_eq!(db::list(&path).unwrap().iter().map(|row| row.id).collect::<Vec<_>>(), vec![41]);
    let connection = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        connection.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0)).unwrap(),
        8
    );
    for table in ["menu_runs", "menu_steps", "skill_snapshots"] {
        let exists: i64 = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1, "missing {table}");
    }
    drop(connection);

    db::list(&path).unwrap();
    let connection = rusqlite::Connection::open(path).unwrap();
    assert_eq!(table_count(&connection, "sessions"), 1);
    assert_eq!(table_count(&connection, "menu_runs"), 0);
    assert_eq!(table_count(&connection, "menu_steps"), 0);
    assert_eq!(table_count(&connection, "skill_snapshots"), 0);
}

#[test]
fn same_menu_identity_reuses_run_and_completed_steps_roundtrip() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    let date = NaiveDate::from_ymd_opt(2026, 11, 18).unwrap();
    let plan = r#"{"date":"2026-11-18","steps":[]}"#;

    let first = db::ensure_menu_run(&path, date, "qwerty", "en,ja", "menu-v1", 15, plan).unwrap();
    let second = db::ensure_menu_run(&path, date, "qwerty", "en,ja", "menu-v1", 15, plan).unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(
        db::find_menu_run_for_day(&path, date, "qwerty", "en,ja", "menu-v1", 15)
            .unwrap()
            .unwrap()
            .id,
        first.id
    );
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO sessions (
                id, started_at, finished_at, duration_ms, mode, mode_params,
                layout, input_layout, geometry, error_mode, release_events,
                app_version, metrics_version, net_wpm, accuracy, score_eligible,
                iki_median_ms, iki_cv, cps, consistency, max_combo,
                completed_words, timeout_words, mistypes_total
             ) VALUES (
                9, '2026-11-18T08:00:00Z', '2026-11-18T08:01:00Z', 60000,
                'sentence', '{\"lang\":\"en\"}', 'qwerty', 'same', 'ansi', 'stop', 0,
                '0.1.0', 1, 50.0, 0.98, 1, 180.0, 0.2, 4.0, 0.8, 10, 20, 0, 1
             )",
            [],
        )
        .unwrap();

    db::record_menu_step(
        &path,
        &db::MenuStepRecord {
            run_id: first.id,
            idx: 1,
            role: "primary-a".into(),
            prescription_id: Some("preview.wide".into()),
            level: Some(1),
            session_id: 9,
            metric: Some("word-onset-ratio".into()),
            value: Some(1.2),
            advance_met: Some(true),
            level_after: Some(1),
        },
    )
    .unwrap();
    db::record_menu_step(
        &path,
        &db::MenuStepRecord {
            run_id: first.id,
            idx: 1,
            role: "primary-a".into(),
            prescription_id: Some("preview.wide".into()),
            level: Some(1),
            session_id: 9,
            metric: Some("word-onset-ratio".into()),
            value: Some(1.2),
            advance_met: Some(true),
            level_after: Some(1),
        },
    )
    .unwrap();

    assert_eq!(db::completed_menu_steps(&path, first.id).unwrap(), vec![1]);
    assert_eq!(db::load_menu_steps(&path, first.id).unwrap().len(), 1);
    let connection = rusqlite::Connection::open(path).unwrap();
    assert_eq!(table_count(&connection, "menu_runs"), 1);
    assert_eq!(table_count(&connection, "menu_steps"), 1);
    assert_eq!(
        connection
            .query_row("SELECT completed_steps FROM menu_runs WHERE id=?1", [first.id], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn prescription_progress_meta_update_reads_calculates_and_writes_transactionally() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    let date = NaiveDate::from_ymd_opt(2026, 11, 18).unwrap();

    let first = db::update_menu_progress(&path, "qwerty", "en", "preview.wide", |state| {
        apply_advance_outcome(state, 3, 3, AdvanceOutcome::Met, date)
    })
    .unwrap();
    let second = db::update_menu_progress(&path, "qwerty", "en", "preview.wide", |state| {
        apply_advance_outcome(state, 3, 3, AdvanceOutcome::Met, date)
    })
    .unwrap();
    assert_eq!((first.consecutive, second.consecutive), (1, 2));
    assert_eq!(
        db::load_menu_progress(&path, "qwerty", "en", "preview.wide")
            .unwrap()
            .unwrap()
            .consecutive,
        2
    );
}

#[test]
fn three_met_results_promote_and_a_miss_resets_the_streak() {
    let date = NaiveDate::from_ymd_opt(2026, 11, 18).unwrap();
    let mut state = PrescriptionProgress::default();

    let first = apply_advance_outcome(&mut state, 3, 3, AdvanceOutcome::Met, date);
    assert_eq!(first.consecutive, 1);
    let missed = apply_advance_outcome(&mut state, 3, 3, AdvanceOutcome::NotMet, date);
    assert_eq!(missed.consecutive, 0);
    for expected in [1, 2] {
        let update = apply_advance_outcome(&mut state, 3, 3, AdvanceOutcome::Met, date);
        assert_eq!(update.consecutive, expected);
        assert_eq!(update.level_after, 1);
    }
    let promoted = apply_advance_outcome(&mut state, 3, 3, AdvanceOutcome::Met, date);
    assert_eq!(promoted.consecutive, 0);
    assert_eq!(promoted.level_after, 2);
    assert_eq!(state.judgment, PrescriptionJudgment::None);
}

#[test]
fn graduating_last_level_enters_maintenance_and_regression_reopens_last_level_after_14_days() {
    let date = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
    let mut state = PrescriptionProgress {
        level: 3,
        consecutive: 2,
        ..PrescriptionProgress::default()
    };
    let graduated = apply_advance_outcome(&mut state, 3, 3, AdvanceOutcome::Met, date);
    assert_eq!(graduated.level_after, 3);
    assert_eq!(state.judgment, PrescriptionJudgment::Maintenance);
    assert_eq!(state.maintenance_due, date.checked_add_days(chrono::Days::new(14)));

    assert_eq!(
        apply_maintenance_recheck(&mut state, date + chrono::Duration::days(13), Some(true)),
        AdvanceOutcome::CannotDetermine
    );
    assert_eq!(state.judgment, PrescriptionJudgment::Maintenance);
    assert_eq!(
        apply_maintenance_recheck(&mut state, date + chrono::Duration::days(14), Some(true)),
        AdvanceOutcome::NotMet
    );
    assert_eq!(state.judgment, PrescriptionJudgment::None);
    assert_eq!(state.level, 3);
    assert_eq!(state.maintenance_due, None);
}

#[test]
fn reference_judgment_uses_improvement_and_five_no_change_boundaries() {
    assert_eq!(reference_judgment(&[2.0, 2.0], &[2.4, 2.4, 2.4]), ReferenceJudgment::InsufficientData);
    assert_eq!(reference_judgment(&[2.0, 2.0, 2.0], &[2.29, 2.30, 2.31]), ReferenceJudgment::Improved);
    assert_eq!(
        reference_judgment(&[2.0, 2.0, 2.0], &[2.09, 1.91, 2.00, 2.05]),
        ReferenceJudgment::Pending
    );
    assert_eq!(
        reference_judgment(&[2.0, 2.0, 2.0], &[2.09, 1.91, 2.00, 2.05, 1.95]),
        ReferenceJudgment::NoChange
    );
}

#[test]
fn no_change_switches_to_alternate_then_pauses_the_alternate_for_14_days() {
    let date = NaiveDate::from_ymd_opt(2026, 11, 18).unwrap();
    let mut primary = PrescriptionProgress::default();
    apply_reference_judgment(&mut primary, ReferenceJudgment::NoChange, false, date);
    assert_eq!(primary.judgment, PrescriptionJudgment::NoChange);
    assert_eq!(primary.paused_until, None);

    let mut alternate = PrescriptionProgress::default();
    apply_reference_judgment(&mut alternate, ReferenceJudgment::NoChange, true, date);
    assert_eq!(alternate.judgment, PrescriptionJudgment::NoChange);
    assert_eq!(alternate.paused_until, date.checked_add_days(chrono::Days::new(14)));
    assert!(alternate.build_state(date + chrono::Duration::days(13)).paused);
    assert!(!alternate.build_state(date + chrono::Duration::days(14)).paused);
}

#[test]
fn advance_source_chooses_the_step_or_same_menu_check_measurement() {
    let catalog = bundled_catalog().unwrap();
    let step_condition = &catalog.prescription("accuracy.gate").unwrap().levels[0].advance;
    assert_eq!(
        evaluate_advance_from_sources(step_condition, Some(1.0), Some(0.0), None),
        AdvanceOutcome::Met
    );

    let check_condition = &catalog.prescription("preview.wide").unwrap().levels[0].advance;
    assert_eq!(
        evaluate_advance_from_sources(check_condition, Some(1.0), Some(1.5), Some(1.4)),
        AdvanceOutcome::NotMet
    );
    assert_eq!(
        evaluate_advance_from_sources(check_condition, Some(2.0), None, Some(1.4)),
        AdvanceOutcome::CannotDetermine
    );
}

#[test]
fn build_from_storage_reads_the_persisted_prescription_level() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    let date = NaiveDate::from_ymd_opt(2026, 11, 18).unwrap();
    db::update_menu_progress(&path, "qwerty", "en", "transition.adaptive", |state| {
        state.level = 2;
    })
    .unwrap();
    let mut config = rs_typing_trainer::config::Config::default();
    config.storage.db_path = Some(path.clone());

    let menu = build_from_storage(&path, directory.path(), &config, rs_typing_trainer::i18n::Lang::En, date).unwrap();
    let primary = menu
        .steps
        .iter()
        .find(|step| step.role == rs_typing_trainer::practice::menu::MenuRole::PrimaryA)
        .unwrap();
    assert_eq!(primary.prescription_id, "transition.adaptive");
    assert_eq!(primary.level, Some(2));
}

#[test]
fn r3_insufficient_data_keeps_adaptive_and_ineffective_switches_to_alternate() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    let mut prescription = Prescription {
        id: None,
        created_at: Utc.with_ymd_and_hms(2026, 11, 1, 8, 0, 0).unwrap(),
        layout: "qwerty".into(),
        kind: "adaptive".into(),
        target: serde_json::json!({}),
        control: serde_json::json!([]),
        hypothesis: serde_json::json!({}),
        baseline: serde_json::json!({}),
        status: "insufficient-data".into(),
        evaluated_at: None,
        result: None,
    };
    let id = db::save_prescription(&path, &prescription).unwrap();
    let config = rs_typing_trainer::config::Config::default();
    let build = |date| build_from_storage(&path, directory.path(), &config, rs_typing_trainer::i18n::Lang::En, date).unwrap();
    let primary_id = |menu: &rs_typing_trainer::practice::menu::DailyMenu| {
        menu.steps
            .iter()
            .find(|step| step.role == rs_typing_trainer::practice::menu::MenuRole::PrimaryA)
            .unwrap()
            .prescription_id
            .clone()
    };
    assert_eq!(primary_id(&build(NaiveDate::from_ymd_opt(2026, 11, 18).unwrap())), "transition.adaptive");

    prescription.id = Some(id);
    prescription.status = "ineffective".into();
    db::save_prescription(&path, &prescription).unwrap();
    assert_eq!(primary_id(&build(NaiveDate::from_ymd_opt(2026, 11, 19).unwrap())), "transition.ngram");
}

// ---------------------------------------------------------------------------
// Storage-backed judgments (record_step_completion) and build_from_storage
// reading menu_runs / menu_steps / meta.
// ---------------------------------------------------------------------------

use rs_typing_trainer::config::{Config, ErrorMode};
use rs_typing_trainer::engine::session::{ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::practice::menu::progress::{JudgeContext, JudgmentDeferral, StepCompletionReport, record_step_completion};
use rs_typing_trainer::practice::menu::{DailyMenu, Graduation, MenuRole, MenuStep};
use rs_typing_trainer::record::{SessionHeader, SessionRecord};

const TEXT_EN: &str = "the quick brown fox jumps over the lazy dog ";

fn day(offset: i64) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 1).unwrap() + chrono::Duration::days(offset)
}

/// A practice session typed on `date` with `chars` correct keystrokes.
fn session(date: NaiveDate, hour: u32, language: &str, net_wpm: f64, chars: usize) -> SessionRecord {
    let text = TEXT_EN.repeat(chars / TEXT_EN.len() + 1).chars().take(chars).collect::<String>();
    let start = Utc.from_utc_datetime(&date.and_hms_opt(hour, 0, 0).unwrap());
    let mut time_us = 100_000_u64;
    let mut keystrokes = Vec::new();
    for (index, character) in text.chars().enumerate() {
        if index > 0 {
            time_us += 150_000 + (index as u64 % 7) * 10_000;
        }
        keystrokes.push(KeystrokeLog {
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
    SessionRecord {
        header: SessionHeader {
            id: None,
            started_at: start.to_rfc3339(),
            finished_at: (start + chrono::Duration::milliseconds((time_us / 1_000) as i64)).to_rfc3339(),
            duration_ms: (time_us / 1_000) as i64,
            mode: "sentence".to_owned(),
            mode_params: serde_json::json!({ "lang": language }).to_string(),
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
            accuracy: 0.99,
            iki_median_ms: 160.0,
            iki_cv: 0.2,
            cps: 5.0,
            consistency: 0.8,
            max_combo: 10,
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
                text,
                reading: None,
                kind: None,
                shown_us: 0,
                done_us: Some(time_us),
                outcome: ItemOutcome::Completed,
            }],
            keystrokes,
        },
    }
}

fn saved(path: &std::path::Path, mut record: SessionRecord) -> SessionRecord {
    record.header.id = Some(db::save(path, &record).unwrap());
    record
}

fn language(code: &str) -> Lang {
    match code {
        "ja" => Lang::Ja,
        _ => Lang::En,
    }
}

fn fixed_step(role: MenuRole, practice: Lang) -> MenuStep {
    let id = match role {
        MenuRole::Warmup => "fixed.warmup",
        MenuRole::Check => "fixed.check",
        _ => "fixed.finish",
    };
    MenuStep {
        role,
        language: practice,
        prescription_id: id.to_owned(),
        title: id.to_owned(),
        level: None,
        mode: "sentence".to_owned(),
        minutes: 3,
        reason: "-".to_owned(),
        focus: String::new(),
        advance: String::new(),
        advance_condition: None,
        evidence: None,
        evidence_note: None,
        settings: Default::default(),
    }
}

fn prescription_step(id: &str, role: MenuRole, practice: Lang, level: u8) -> MenuStep {
    let prescription = bundled_catalog().unwrap().prescription(id).unwrap();
    let row = &prescription.levels[usize::from(level - 1)];
    MenuStep {
        role,
        language: practice,
        prescription_id: id.to_owned(),
        title: prescription.title.text(Lang::Ja).to_owned(),
        level: Some(level),
        mode: row.mode.clone(),
        minutes: row.minutes,
        reason: "-".to_owned(),
        focus: row.focus.text(Lang::Ja).to_owned(),
        advance: row.advance.note.text(Lang::Ja).to_owned(),
        advance_condition: Some(row.advance.clone()),
        evidence: None,
        evidence_note: None,
        settings: row.settings.clone(),
    }
}

fn plan(date: NaiveDate, steps: Vec<MenuStep>) -> DailyMenu {
    DailyMenu {
        date,
        language: "en".to_owned(),
        catalog_version: "menu-v1".to_owned(),
        requested_minutes: 15,
        introductory: false,
        bench_recommended: false,
        steps,
    }
}

fn run_for(path: &std::path::Path, menu: &DailyMenu, key: &str) -> i64 {
    db::ensure_menu_run(
        path,
        menu.date,
        "qwerty",
        key,
        "menu-v1",
        menu.requested_minutes,
        &serde_json::to_string(menu).unwrap(),
    )
    .unwrap()
    .id
}

struct Store {
    directory: TempDir,
    path: std::path::PathBuf,
    config: Config,
}

impl Store {
    fn new() -> Self {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("sessions.db");
        let mut config = Config::default();
        config.storage.db_path = Some(path.clone());
        Self { directory, path, config }
    }

    fn complete(&self, date: NaiveDate, run_id: i64, index: usize, record: &SessionRecord) -> StepCompletionReport {
        let layout = rs_typing_trainer::layout::builtin_layout("qwerty").unwrap();
        let context = JudgeContext {
            db_path: &self.path,
            home: self.directory.path(),
            config: &self.config,
            layout: &layout,
            as_of: date,
        };
        record_step_completion(&context, run_id, index, record).unwrap()
    }

    fn state(&self, lang: &str, id: &str) -> PrescriptionProgress {
        db::load_menu_progress(&self.path, "qwerty", lang, id).unwrap().unwrap_or_default()
    }

    /// A day with only a check step (baseline before a prescription starts).
    fn check_only_day(&self, date: NaiveDate, lang: &str, net_wpm: f64) {
        let menu = plan(date, vec![fixed_step(MenuRole::Check, language(lang))]);
        let run = run_for(&self.path, &menu, lang);
        let record = saved(&self.path, session(date, 9, lang, net_wpm, 60));
        self.complete(date, run, 0, &record);
    }

    /// A day with one prescription step followed by a same-language check step.
    fn prescription_day(&self, date: NaiveDate, id: &str, lang: &str, step_wpm: f64, check_wpm: f64) -> (StepCompletionReport, StepCompletionReport) {
        let level = self.state(lang, id).level;
        let menu = plan(
            date,
            vec![
                prescription_step(id, MenuRole::PrimaryA, language(lang), level),
                fixed_step(MenuRole::Check, language(lang)),
            ],
        );
        let run = run_for(&self.path, &menu, lang);
        let step = saved(&self.path, session(date, 9, lang, step_wpm, 60));
        let check = saved(&self.path, session(date, 10, lang, check_wpm, 60));
        (self.complete(date, run, 0, &step), self.complete(date, run, 1, &check))
    }
}

fn r3_row(id: Option<i64>, status: &str, evaluated_at: chrono::DateTime<Utc>) -> Prescription {
    Prescription {
        id,
        created_at: Utc.with_ymd_and_hms(2026, 10, 1, 8, 0, 0).unwrap(),
        layout: "qwerty".into(),
        kind: "adaptive-transition".into(),
        target: serde_json::json!({}),
        control: serde_json::json!([]),
        hypothesis: serde_json::json!({}),
        baseline: serde_json::json!({}),
        status: status.into(),
        evaluated_at: Some(evaluated_at),
        result: None,
    }
}

#[test]
fn an_r3_verdict_is_counted_once_and_only_a_new_verdict_counts_again() {
    let store = Store::new();
    let id = db::save_prescription(
        &store.path,
        &r3_row(None, "effective", Utc.with_ymd_and_hms(2026, 11, 1, 3, 0, 0).unwrap()),
    )
    .unwrap();
    let transition_day = |date: NaiveDate| {
        let level = store.state("en", "transition.adaptive").level;
        let menu = plan(date, vec![prescription_step("transition.adaptive", MenuRole::PrimaryA, Lang::En, level)]);
        let run = run_for(&store.path, &menu, "en");
        let record = saved(&store.path, session(date, 9, "en", 50.0, 60));
        store.complete(date, run, 0, &record).own.unwrap().progress.unwrap()
    };

    let first = transition_day(day(1));
    assert_eq!(first.outcome, AdvanceOutcome::Met);
    assert_eq!(first.graduation, Some(Graduation::Level(2)));
    for offset in [2, 3, 4] {
        let repeated = transition_day(day(offset));
        assert_eq!(
            repeated.outcome,
            AdvanceOutcome::CannotDetermine,
            "day {offset} recounted the old verdict"
        );
        assert_eq!(repeated.level_after, 2);
    }
    assert_eq!(store.state("en", "transition.adaptive").judgment, PrescriptionJudgment::None);

    db::save_prescription(
        &store.path,
        &r3_row(Some(id), "effective", Utc.with_ymd_and_hms(2026, 11, 5, 3, 0, 0).unwrap()),
    )
    .unwrap();
    let renewed = transition_day(day(5));
    assert_eq!(renewed.outcome, AdvanceOutcome::Met);
    assert_eq!(renewed.level_after, 3);
}

#[test]
fn a_check_with_no_errors_leaves_the_anticipation_share_undefined() {
    let store = Store::new();
    let layout = rs_typing_trainer::layout::builtin_layout("qwerty").unwrap();
    let context = JudgeContext {
        db_path: &store.path,
        home: store.directory.path(),
        config: &store.config,
        layout: &layout,
        as_of: day(1),
    };
    let clean = session(day(1), 9, "en", 50.0, 60);
    assert_eq!(
        rs_typing_trainer::practice::menu::progress::session_metric_value(
            &context,
            rs_typing_trainer::practice::menu::MenuMetric::AnticipationRate,
            &clean,
            "en"
        )
        .unwrap(),
        None
    );

    // The judgment on it is "cannot determine" and keeps the streak.
    db::update_menu_progress(&store.path, "qwerty", "en", "accuracy.anticipation", |state| state.consecutive = 2).unwrap();
    let menu = plan(
        day(1),
        vec![
            prescription_step("accuracy.anticipation", MenuRole::PrimaryA, Lang::En, 1),
            fixed_step(MenuRole::Check, Lang::En),
        ],
    );
    let run = run_for(&store.path, &menu, "en");
    let step = saved(&store.path, session(day(1), 9, "en", 50.0, 60));
    store.complete(day(1), run, 0, &step);
    let check = saved(&store.path, session(day(1), 10, "en", 50.0, 60));
    let report = store.complete(day(1), run, 1, &check);
    let progress = report.others[0].progress.unwrap();
    assert_eq!(progress.outcome, AdvanceOutcome::CannotDetermine);
    assert_eq!(progress.consecutive, 2);
    let rows = db::load_menu_steps(&store.path, run).unwrap();
    assert_eq!(rows[0].value, None, "missing data must not be stored as 0");
    assert_eq!(rows[0].advance_met, None);
}

#[test]
fn anticipation_share_is_measured_zero_when_other_errors_exist() {
    use rs_typing_trainer::analysis::errors::{ErrorDiagnosis, ErrorKind, ErrorKindMetric};
    use rs_typing_trainer::practice::menu::progress::anticipation_rate;

    let mut diagnosis = ErrorDiagnosis {
        kinds: Vec::new(),
        confusion: Vec::new(),
        post_error_slowdown: None,
        pre_error_rhythm: None,
        correction: Default::default(),
        attack_errors: None,
    };
    assert_eq!(anticipation_rate(&diagnosis), None);
    diagnosis.kinds.push(ErrorKindMetric {
        kind: ErrorKind::Consecutive,
        n: 3,
        proportion: 0.0,
    });
    assert_eq!(anticipation_rate(&diagnosis), None, "consecutive errors are not first errors");
    diagnosis.kinds.push(ErrorKindMetric {
        kind: ErrorKind::Omission,
        n: 4,
        proportion: 1.0,
    });
    assert_eq!(anticipation_rate(&diagnosis), Some(0.0));
    diagnosis.kinds.push(ErrorKindMetric {
        kind: ErrorKind::Anticipation,
        n: 4,
        proportion: 0.5,
    });
    assert_eq!(anticipation_rate(&diagnosis), Some(0.5));
}

#[test]
fn ensure_menu_run_keeps_one_run_per_day_when_the_plan_changes() {
    let store = Store::new();
    let intro = plan(
        day(1),
        vec![fixed_step(MenuRole::Warmup, Lang::En), fixed_step(MenuRole::Check, Lang::En)],
    );
    let regular = plan(
        day(1),
        vec![
            fixed_step(MenuRole::Warmup, Lang::Ja),
            prescription_step("speed.pacer", MenuRole::PrimaryA, Lang::Ja, 1),
            fixed_step(MenuRole::Check, Lang::En),
        ],
    );
    let first = run_for(&store.path, &intro, "en,ja");
    let second = run_for(&store.path, &regular, "en,ja");
    assert_eq!(first, second);
    assert_eq!(
        db::load_menu_run(&store.path, first).unwrap().unwrap().plan_json,
        serde_json::to_string(&regular).unwrap(),
        "an unstarted plan is replaced"
    );
    let record = saved(&store.path, session(day(1), 9, "ja", 50.0, 60));
    store.complete(day(1), first, 0, &record);
    assert_eq!(run_for(&store.path, &intro, "en,ja"), first);
    assert_eq!(
        db::load_menu_run(&store.path, first).unwrap().unwrap().plan_json,
        serde_json::to_string(&regular).unwrap(),
        "a plan with recorded steps is kept"
    );
    let connection = rusqlite::Connection::open(&store.path).unwrap();
    assert_eq!(table_count(&connection, "menu_runs"), 1);
}

#[test]
fn build_from_storage_uses_the_configured_language_set_as_the_run_key() {
    let store = Store::new();
    let mut config = store.config.clone();
    config.menu.languages = Some(vec![Lang::En, Lang::Ja]);
    config.menu.minutes = 10;
    let menu = build_from_storage(&store.path, store.directory.path(), &config, Lang::En, day(1)).unwrap();
    assert!(menu.introductory);
    assert!(
        menu.steps.iter().all(|step| step.language == menu.steps[0].language),
        "the introductory menu uses one language"
    );
    assert_eq!(rs_typing_trainer::practice::menu::menu_run_language_key(&config, Lang::En), "en,ja");

    // A stored run of the day is found by the configured key and wins over
    // a fresh assembly.
    let mut stored = menu.clone();
    stored.steps[0].title = "stored plan".to_owned();
    run_for(&store.path, &stored, "en,ja");
    let rebuilt = build_from_storage(&store.path, store.directory.path(), &config, Lang::En, day(1)).unwrap();
    assert_eq!(rebuilt.steps[0].title, "stored plan");
}

#[test]
fn check_first_then_prescription_step_still_judges_once() {
    let store = Store::new();
    let menu = plan(
        day(1),
        vec![
            prescription_step("preview.wide", MenuRole::PrimaryA, Lang::En, 1),
            fixed_step(MenuRole::Check, Lang::En),
        ],
    );
    let run = run_for(&store.path, &menu, "en");
    let check = saved(&store.path, session(day(1), 9, "en", 50.0, 60));
    let report = store.complete(day(1), run, 1, &check);
    assert!(report.others.is_empty(), "the prescription step is not done yet");
    assert_eq!(store.state("en", "preview.wide").pair_judged_run, None);

    let step = saved(&store.path, session(day(1), 10, "en", 50.0, 60));
    let report = store.complete(day(1), run, 0, &step);
    let own = report.own.expect("judged when the second half of the pair arrives");
    assert!(own.progress.is_some(), "check-sourced graduation is judged");
    assert_eq!(own.reference, Some(ReferenceJudgment::InsufficientData));
    assert_eq!(report.deferred, None);
    assert_eq!(store.state("en", "preview.wide").pair_judged_run, Some(run));
    assert_eq!(
        db::load_menu_steps(&store.path, run).unwrap()[0].metric.as_deref(),
        Some("word-onset-ratio")
    );

    // Replaying either step does not judge again.
    let replay = saved(&store.path, session(day(1), 11, "en", 50.0, 60));
    assert!(store.complete(day(1), run, 1, &replay).already_recorded);
    assert!(store.complete(day(1), run, 0, &replay).already_recorded);
}

#[test]
fn a_prescription_in_a_language_other_than_todays_check_waits_for_another_day() {
    let store = Store::new();
    let menu = plan(
        day(1),
        vec![
            prescription_step("preview.wide", MenuRole::PrimaryA, Lang::Ja, 1),
            fixed_step(MenuRole::Check, Lang::En),
        ],
    );
    let run = run_for(&store.path, &menu, "en,ja");
    let step = saved(&store.path, session(day(1), 9, "ja", 50.0, 60));
    let report = store.complete(day(1), run, 0, &step);
    assert_eq!(report.deferred, Some(JudgmentDeferral::CheckLanguage(Lang::En)));
    assert!(report.own.is_none());

    let check = saved(&store.path, session(day(1), 10, "en", 50.0, 60));
    let report = store.complete(day(1), run, 1, &check);
    assert!(report.others.is_empty());
    assert_eq!(store.state("ja", "preview.wide").pair_judged_run, None);
}

#[test]
fn step_sourced_prescriptions_get_the_reference_judgment_from_check_steps() {
    let store = Store::new();
    for offset in 1..=3 {
        store.check_only_day(day(offset), "en", 40.0);
    }
    let mut reference = None;
    for offset in 4..=6 {
        let (step, check) = store.prescription_day(day(offset), "speed.pacer", "en", 60.0, 80.0);
        // speed.pacer is judged by its own step (advance.source = "step").
        assert!(step.own.as_ref().unwrap().progress.is_some());
        reference = check.others[0].reference;
        if offset < 6 {
            assert_eq!(reference, Some(ReferenceJudgment::InsufficientData));
        }
    }
    assert_eq!(reference, Some(ReferenceJudgment::Improved));
    assert_eq!(store.state("en", "speed.pacer").judgment, PrescriptionJudgment::Improved);
}

#[test]
fn no_change_pauses_and_resuming_retakes_the_baseline() {
    let store = Store::new();
    for offset in 1..=3 {
        store.check_only_day(day(offset), "en", 50.0);
    }
    for offset in 4..=8 {
        store.prescription_day(day(offset), "speed.pacer", "en", 50.0, 50.0);
    }
    let paused = store.state("en", "speed.pacer");
    assert_eq!(paused.judgment, PrescriptionJudgment::NoChange);
    // speed.pacer is its own alternate, so the component rests 14 days.
    assert_eq!(paused.paused_until, Some(day(8) + chrono::Duration::days(14)));
    assert!(paused.build_state(day(21)).paused);

    // After the pause the old checks are the baseline; the new series must
    // reach three checks before any judgment, five before "no change".
    let resumed_on = day(22);
    for (index, offset) in (22..=26).enumerate() {
        let (_, check) = store.prescription_day(day(offset), "speed.pacer", "en", 50.0, 50.0);
        let state = store.state("en", "speed.pacer");
        assert_eq!(state.started_on, Some(resumed_on));
        let reference = check.others[0].reference;
        match index {
            0 | 1 => assert_eq!(reference, Some(ReferenceJudgment::InsufficientData)),
            2 | 3 => assert_eq!(reference, Some(ReferenceJudgment::Pending)),
            _ => assert_eq!(reference, Some(ReferenceJudgment::NoChange)),
        }
        if index < 4 {
            assert_eq!(state.paused_until, None, "re-paused on day {offset}");
        }
    }
}

#[test]
fn maintenance_recheck_uses_the_recorded_check_median() {
    for (check_wpm, regressed) in [(20.0, true), (70.0, false)] {
        let store = Store::new();
        for offset in 1..=3 {
            store.check_only_day(day(offset), "en", 50.0);
        }
        db::update_menu_progress(&store.path, "qwerty", "en", "speed.pacer", |state| {
            state.level = 2;
            state.judgment = PrescriptionJudgment::Maintenance;
            state.maintenance_due = Some(day(17));
        })
        .unwrap();
        // Before the due date nothing changes.
        store.check_only_day(day(16), "en", check_wpm);
        assert_eq!(store.state("en", "speed.pacer").judgment, PrescriptionJudgment::Maintenance);

        store.check_only_day(day(17), "en", check_wpm);
        let state = store.state("en", "speed.pacer");
        assert_eq!(state.maintenance_due, None);
        assert_eq!(state.level, 2, "resumes from the last level");
        if regressed {
            assert_eq!(state.judgment, PrescriptionJudgment::None);
            assert_eq!(state.build_state(day(18)).level, 2);
        } else {
            assert_eq!(state.judgment, PrescriptionJudgment::Maintenance);
        }
    }
}

#[test]
fn graduation_is_reported_with_the_next_level_or_maintenance() {
    let store = Store::new();
    db::update_menu_progress(&store.path, "qwerty", "en", "accuracy.gate", |state| state.consecutive = 2).unwrap();
    let pass = |date: NaiveDate, level: u8| {
        let menu = plan(date, vec![prescription_step("accuracy.gate", MenuRole::PrimaryA, Lang::En, level)]);
        let run = run_for(&store.path, &menu, "en");
        let mut record = session(date, 9, "en", 50.0, 60);
        record.header.mode_params = serde_json::json!({"lang": "en", "score_eligible": true}).to_string();
        let record = saved(&store.path, record);
        store.complete(date, run, 0, &record).own.unwrap().progress.unwrap()
    };
    let graduated = pass(day(1), 1);
    assert_eq!(graduated.graduation, Some(Graduation::Level(2)));
    assert_eq!(graduated.consecutive, 0);
    db::update_menu_progress(&store.path, "qwerty", "en", "accuracy.gate", |state| {
        state.level = 3;
        state.consecutive = 2;
    })
    .unwrap();
    assert_eq!(pass(day(2), 3).graduation, Some(Graduation::Maintenance));
}

#[test]
fn step_judgment_and_its_menu_step_row_are_written_together() {
    let store = Store::new();
    let menu = plan(day(1), vec![prescription_step("accuracy.gate", MenuRole::PrimaryA, Lang::En, 1)]);
    let run = run_for(&store.path, &menu, "en");
    // A session id that does not exist makes the menu_steps insert fail
    // (foreign key); the state must not advance alone.
    let mut record = session(day(1), 9, "en", 50.0, 60);
    record.header.mode_params = serde_json::json!({"lang": "en", "score_eligible": true}).to_string();
    record.header.id = Some(9_999);
    let layout = rs_typing_trainer::layout::builtin_layout("qwerty").unwrap();
    let context = JudgeContext {
        db_path: &store.path,
        home: store.directory.path(),
        config: &store.config,
        layout: &layout,
        as_of: day(1),
    };
    assert!(record_step_completion(&context, run, 0, &record).is_err());
    assert_eq!(db::load_menu_progress(&store.path, "qwerty", "en", "accuracy.gate").unwrap(), None);
    assert!(db::load_menu_steps(&store.path, run).unwrap().is_empty());
}

#[test]
fn a_paused_prescription_is_left_out_of_the_stored_menu() {
    let store = Store::new();
    db::update_menu_progress(&store.path, "qwerty", "en", "transition.adaptive", |state| {
        state.paused_until = Some(day(15));
    })
    .unwrap();
    let primary = |date| {
        build_from_storage(&store.path, store.directory.path(), &store.config, Lang::En, date)
            .unwrap()
            .steps
            .into_iter()
            .find(|step| step.role == MenuRole::PrimaryA)
            .unwrap()
            .prescription_id
    };
    assert_eq!(primary(day(14)), "transition.ngram");
    assert_eq!(primary(day(15)), "transition.adaptive");
}

/// Enough logged keystrokes and practice sessions for a regular menu (§4.1).
fn seed_practice(store: &Store, today: NaiveDate, languages: &[&str]) {
    for offset in 1..=5 {
        for (hour, lang) in languages.iter().enumerate() {
            let wpm = if *lang == "ja" { 40.0 } else { 50.0 };
            let mut record = session(today - chrono::Duration::days(offset), 9 + hour as u32, lang, wpm, 500);
            // Accuracy C-ish (above the 94.5% priority line) so that speed
            // and accuracy both rank as candidates.
            record.header.accuracy = 0.96;
            saved(&store.path, record);
        }
    }
}

fn store_past_run(store: &Store, menu: &DailyMenu, date: NaiveDate, key: &str, edit: impl FnOnce(&mut DailyMenu)) {
    let mut past = menu.clone();
    past.date = date;
    edit(&mut past);
    let run = run_for(&store.path, &past, key);
    // A recorded step makes it a real past run, as written by the app.
    let mut record = session(date, 20, "en", 50.0, 60);
    record.header.accuracy = 0.96; // same as the seed so grades do not move
    let record = saved(&store.path, record);
    db::record_menu_step(
        &store.path,
        &db::MenuStepRecord {
            run_id: run,
            idx: 1,
            role: "warmup".into(),
            prescription_id: Some("fixed.warmup".into()),
            level: None,
            session_id: record.header.id.unwrap(),
            metric: None,
            value: None,
            advance_met: None,
            level_after: None,
        },
    )
    .unwrap();
}

fn step_of(menu: &DailyMenu, role: MenuRole) -> &MenuStep {
    menu.steps.iter().find(|step| step.role == role).unwrap()
}

#[test]
fn primary_used_on_the_two_previous_days_drops_one_place_from_stored_runs() {
    let today = day(10);
    let fresh = Store::new();
    seed_practice(&fresh, today, &["en"]);
    let baseline = build_from_storage(&fresh.path, fresh.directory.path(), &fresh.config, Lang::En, today).unwrap();
    assert!(!baseline.introductory);
    let first = step_of(&baseline, MenuRole::PrimaryA).prescription_id.clone();
    let second = step_of(&baseline, MenuRole::PrimaryB).prescription_id.clone();
    assert_ne!(first, second);

    // Only yesterday: no demotion.
    let one_day = Store::new();
    seed_practice(&one_day, today, &["en"]);
    store_past_run(&one_day, &baseline, today - chrono::Duration::days(1), "en", |_| {});
    let menu = build_from_storage(&one_day.path, one_day.directory.path(), &one_day.config, Lang::En, today).unwrap();
    assert_eq!(step_of(&menu, MenuRole::PrimaryA).prescription_id, first);

    // Yesterday and the day before: the component drops one place.
    let two_days = Store::new();
    seed_practice(&two_days, today, &["en"]);
    for offset in [1, 2] {
        // Only the A component repeats (if B repeated too, both would drop
        // one place and cancel out).
        store_past_run(&two_days, &baseline, today - chrono::Duration::days(offset), "en", |past| {
            past.steps.retain(|step| step.role != MenuRole::PrimaryB);
        });
    }
    let menu = build_from_storage(&two_days.path, two_days.directory.path(), &two_days.config, Lang::En, today).unwrap();
    assert_eq!(step_of(&menu, MenuRole::PrimaryA).prescription_id, second);
    assert_eq!(step_of(&menu, MenuRole::PrimaryB).prescription_id, first);
}

#[test]
fn two_language_ten_minute_menu_rotates_from_yesterdays_stored_run() {
    let today = day(10);
    let configure = |store: &Store| {
        let mut config = store.config.clone();
        config.menu.languages = Some(vec![Lang::En, Lang::Ja]);
        config.menu.minutes = 10;
        config
    };
    for yesterday in [Lang::En, Lang::Ja] {
        let store = Store::new();
        seed_practice(&store, today, &["en", "ja"]);
        let config = configure(&store);
        // Build a template on another day so no run of `today` exists yet.
        let template = build_from_storage(&store.path, store.directory.path(), &config, Lang::En, today - chrono::Duration::days(3)).unwrap();
        assert!(!template.introductory);
        store_past_run(&store, &template, today - chrono::Duration::days(1), "en,ja", |past| {
            for step in &mut past.steps {
                step.language = yesterday;
            }
        });
        let menu = build_from_storage(&store.path, store.directory.path(), &config, Lang::En, today).unwrap();
        assert!(menu.steps.iter().all(|step| step.role != MenuRole::PrimaryB));
        assert_ne!(step_of(&menu, MenuRole::PrimaryA).language, yesterday, "A language after {yesterday:?}");
        assert_ne!(step_of(&menu, MenuRole::Check).language, yesterday, "check language after {yesterday:?}");
    }
}

#[test]
fn blind_fade_reads_the_h5_keyboard_removable_mark() {
    use rs_typing_trainer::analysis::habits::{AssistanceEffect, AssistanceKind};
    use rs_typing_trainer::practice::menu::progress::assistance_removable_value;

    let effect = |with_n, without_n, removable| AssistanceEffect {
        kind: AssistanceKind::Keyboard,
        with_n,
        without_n,
        with_form_median: Some(1.0),
        without_form_median: Some(if removable { 1.0 } else { 0.9 }),
        removable,
    };
    assert_eq!(assistance_removable_value(&effect(5, 5, true)), Some(1.0));
    assert_eq!(assistance_removable_value(&effect(5, 5, false)), Some(0.0));
    assert_eq!(assistance_removable_value(&effect(4, 5, false)), None, "H5 has not compared yet");
    let condition = &bundled_catalog().unwrap().prescription("blind.fade").unwrap().levels[0].advance;
    assert_eq!(evaluate_advance_from_sources(condition, None, Some(1.0), None), AdvanceOutcome::Met);
    assert_eq!(
        evaluate_advance_from_sources(condition, None, None, None),
        AdvanceOutcome::CannotDetermine
    );

    // Through the check step: with too few sessions H5 cannot compare, so
    // the judgment is "cannot determine" rather than "not met".
    let store = Store::new();
    let menu = plan(
        day(1),
        vec![
            prescription_step("blind.fade", MenuRole::PrimaryA, Lang::En, 1),
            fixed_step(MenuRole::Check, Lang::En),
        ],
    );
    let run = run_for(&store.path, &menu, "en");
    let step = saved(&store.path, session(day(1), 9, "en", 50.0, 60));
    store.complete(day(1), run, 0, &step);
    let check = saved(&store.path, session(day(1), 10, "en", 50.0, 60));
    let report = store.complete(day(1), run, 1, &check);
    assert_eq!(report.others[0].progress.unwrap().outcome, AdvanceOutcome::CannotDetermine);
}

// ---------------------------------------------------------------------------
// MYTASK-3622 (design §5, 2026-11-18): the reference judgment of the
// prescriptions outside the seven components.
// ---------------------------------------------------------------------------

use rs_typing_trainer::analysis::grades::Direction;
use rs_typing_trainer::practice::menu::progress::{RunChange, run_steps_summary};
use rs_typing_trainer::practice::menu::{RAW_REFERENCE_IMPROVEMENT_RATIO, RAW_REFERENCE_NO_CHANGE_RATIO, raw_reference_judgment};

#[test]
fn raw_reference_judgment_uses_relative_change_in_the_metric_direction() {
    assert_eq!(RAW_REFERENCE_IMPROVEMENT_RATIO, 0.05);
    assert_eq!(RAW_REFERENCE_NO_CHANGE_RATIO, 0.02);
    let lower = Direction::Lower;
    assert_eq!(
        raw_reference_judgment(&[1.5, 1.5], &[1.2, 1.2, 1.2], lower),
        ReferenceJudgment::InsufficientData
    );
    // 1.50 → 1.425 is exactly 5% smaller: improved for "smaller is better".
    assert_eq!(
        raw_reference_judgment(&[1.5, 1.5, 1.5], &[1.425, 1.425, 1.425], lower),
        ReferenceJudgment::Improved
    );
    // The same change is a deterioration for "larger is better".
    assert_eq!(
        raw_reference_judgment(&[1.5, 1.5, 1.5], &[1.425, 1.425, 1.425], Direction::Higher),
        ReferenceJudgment::Pending
    );
    assert_eq!(
        raw_reference_judgment(&[1.5, 1.5, 1.5], &[1.44, 1.44, 1.44], lower),
        ReferenceJudgment::Pending,
        "4% is not enough"
    );
    // Five values within ±2% of the baseline median: no change.
    assert_eq!(
        raw_reference_judgment(&[1.5, 1.5, 1.5], &[1.52, 1.48, 1.51, 1.49, 1.5], lower),
        ReferenceJudgment::NoChange
    );
    assert_eq!(
        raw_reference_judgment(&[1.5, 1.5, 1.5], &[1.52, 1.48, 1.51, 1.5], lower),
        ReferenceJudgment::Pending,
        "four values are not yet «no change»"
    );
    assert_eq!(
        raw_reference_judgment(&[1.5, 1.5, 1.5], &[1.54, 1.48, 1.51, 1.49, 1.5], lower),
        ReferenceJudgment::Pending,
        "one value outside ±2%"
    );
    // Missing values are skipped, not read as 0.
    assert_eq!(
        raw_reference_judgment(&[1.5, f64::NAN, 1.5], &[1.2, 1.2, 1.2], lower),
        ReferenceJudgment::InsufficientData
    );
}

const TEXT_REACH: &str = "a quick brown fox jumps over sad lazy dogs ";

/// A session whose intervals come from `interval_ms(index, character)`.
fn timed_session(
    date: NaiveDate,
    hour: u32,
    mode: &str,
    params: serde_json::Value,
    chars: usize,
    interval_ms: impl Fn(usize, char) -> u64,
) -> SessionRecord {
    let mut record = session(date, hour, "en", 40.0, chars);
    let text = TEXT_REACH.repeat(chars / TEXT_REACH.len() + 1).chars().take(chars).collect::<String>();
    let mut time_us = 100_000_u64;
    for (index, (key, character)) in record.log.keystrokes.iter_mut().zip(text.chars()).enumerate() {
        if index > 0 {
            time_us += interval_ms(index, character) * 1_000;
        }
        key.t_us = time_us;
        key.typed = Some(character.to_string());
        key.expected = Some(character.to_string());
    }
    record.log.items[0].text = text;
    record.log.items[0].done_us = Some(time_us);
    record.log.duration_us = time_us;
    record.header.mode = mode.to_owned();
    record.header.mode_params = params.to_string();
    record
}

/// Remote rows typed at `far_ms`, the home row at 200 ms.
fn reach_session(date: NaiveDate, hour: u32, far_ms: u64) -> SessionRecord {
    timed_session(date, hour, "sentence", serde_json::json!({ "lang": "en" }), 120, |_, character| {
        if "asdfghjkl".contains(character) {
            200
        } else if character.is_alphabetic() {
            far_ms
        } else {
            180
        }
    })
}

#[test]
fn reach_is_judged_on_the_raw_ratio_of_the_check_steps() {
    let store = Store::new();
    for offset in 1..=3 {
        let menu = plan(day(offset), vec![fixed_step(MenuRole::Check, Lang::En)]);
        let run = run_for(&store.path, &menu, "en");
        let check = saved(&store.path, reach_session(day(offset), 9, 300));
        store.complete(day(offset), run, 0, &check);
    }
    let mut last = None;
    for offset in 4..=6 {
        let menu = plan(
            day(offset),
            vec![
                prescription_step("reach.unlock", MenuRole::PrimaryA, Lang::En, 1),
                fixed_step(MenuRole::Check, Lang::En),
            ],
        );
        let run = run_for(&store.path, &menu, "en");
        let step = saved(&store.path, reach_session(day(offset), 9, 260));
        let check = saved(&store.path, reach_session(day(offset), 10, 255));
        store.complete(day(offset), run, 0, &step);
        let report = store.complete(day(offset), run, 1, &check);
        let reference = report.others[0].reference;
        if offset < 6 {
            assert_eq!(reference, Some(ReferenceJudgment::InsufficientData), "day {offset}");
        }
        last = Some((run, reference));
    }
    let (run, reference) = last.unwrap();
    // 300/200 → 255/200 is a 15% smaller ratio: improved.
    assert_eq!(reference, Some(ReferenceJudgment::Improved));
    assert_eq!(store.state("en", "reach.unlock").judgment, PrescriptionJudgment::Improved);
    let summary = run_steps_summary(&store.path, "qwerty", run, Lang::Ja).unwrap().unwrap();
    assert!(summary.changes.iter().any(|change| matches!(
        change,
        RunChange::Improved {
            controlled: false,
            assisted: false,
            ..
        }
    )));
}

/// An endurance session whose second half is `slowdown` slower.
fn endurance_session(date: NaiveDate, hour: u32, own_step: bool, slowdown: f64) -> SessionRecord {
    let params = if own_step {
        serde_json::json!({ "lang": "en", "menu_prescription_id": "endurance.split" })
    } else {
        serde_json::json!({ "lang": "en" })
    };
    let second = (150.0 * (1.0 + slowdown)).round() as u64;
    timed_session(date, hour, "endurance", params, 200, |index, _| if index < 100 { 150 } else { second })
}

#[test]
fn endurance_is_judged_on_its_own_assisted_steps_against_earlier_endurance_sessions() {
    let store = Store::new();
    for offset in 1..=3 {
        saved(&store.path, endurance_session(day(offset), 9, false, 0.20));
    }
    let mut last = None;
    for offset in 4..=6 {
        let menu = plan(
            day(offset),
            vec![
                prescription_step("endurance.split", MenuRole::PrimaryA, Lang::En, 1),
                fixed_step(MenuRole::Check, Lang::En),
            ],
        );
        let run = run_for(&store.path, &menu, "en");
        let step = saved(&store.path, endurance_session(day(offset), 9, true, 0.10));
        let check = saved(&store.path, session(day(offset), 10, "en", 40.0, 60));
        store.complete(day(offset), run, 0, &step);
        let report = store.complete(day(offset), run, 1, &check);
        let reference = report.others[0].reference;
        if offset < 6 {
            assert_eq!(reference, Some(ReferenceJudgment::InsufficientData), "day {offset}");
        }
        last = Some((run, reference));
    }
    let (run, reference) = last.unwrap();
    assert_eq!(reference, Some(ReferenceJudgment::Improved));
    let summary = run_steps_summary(&store.path, "qwerty", run, Lang::Ja).unwrap().unwrap();
    assert!(
        summary
            .changes
            .iter()
            .any(|change| matches!(change, RunChange::Improved { assisted: true, .. })),
        "{:?}",
        summary.changes
    );
}

#[test]
fn japanese_spelling_and_blind_fade_have_no_reference_judgment() {
    for id in ["japanese.spelling", "blind.fade"] {
        let store = Store::new();
        let menu = plan(
            day(1),
            vec![
                prescription_step(id, MenuRole::PrimaryA, Lang::En, 1),
                fixed_step(MenuRole::Check, Lang::En),
            ],
        );
        let run = run_for(&store.path, &menu, "en");
        let step = saved(&store.path, session(day(1), 9, "en", 40.0, 60));
        let check = saved(&store.path, session(day(1), 10, "en", 40.0, 60));
        store.complete(day(1), run, 0, &step);
        let report = store.complete(day(1), run, 1, &check);
        assert_eq!(report.others[0].reference, Some(ReferenceJudgment::NotJudged), "{id}");
        assert_eq!(store.state("en", id).judgment, PrescriptionJudgment::None, "{id}");
        let summary = run_steps_summary(&store.path, "qwerty", run, Lang::Ja).unwrap().unwrap();
        assert!(
            summary.changes.iter().any(|change| matches!(change, RunChange::NoReference { .. })),
            "{id}: {:?}",
            summary.changes
        );
    }
}
