//! M5: skill snapshots from the check step (§8.1) and their trends (§8.3).

use std::process::Command as ProcessCommand;

use chrono::{NaiveDate, TimeZone, Utc};
use rs_typing_trainer::analysis::grades::{Component, Direction};
use rs_typing_trainer::config::{Config, ErrorMode};
use rs_typing_trainer::data::{DataCatalog, TrainingDataSource};
use rs_typing_trainer::engine::session::{ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::practice::menu::progress::{JudgeContext, record_step_completion};
use rs_typing_trainer::practice::menu::skills::{
    MOVING_MEDIAN_MIN_POINTS, SeriesPoint, SkillSnapshot, SnapshotInputs, SnapshotSource, TYPICAL_CHANGE_MIN_DIFFERENCES, TrendDirection,
    compute_run_snapshots, rebuild_all_snapshots, record_run_snapshots, render_text, skill_trend, skills_report,
};
use rs_typing_trainer::practice::menu::{DailyMenu, MenuRole, MenuStep, bundled_catalog};
use rs_typing_trainer::record::db::{self, SkillSnapshotRecord};
use rs_typing_trainer::record::{SessionHeader, SessionRecord};
use tempfile::TempDir;

const EPSILON: f64 = 1e-9;

fn close(actual: Option<f64>, expected: f64) {
    let actual = actual.unwrap_or_else(|| panic!("expected {expected}, got None"));
    assert!((actual - expected).abs() < EPSILON, "expected {expected}, got {actual}");
}

// ---------------------------------------------------------------------------
// §8.3 trends on synthetic series
// ---------------------------------------------------------------------------

/// 2026-11-02 is a Monday.
fn date(offset: i64) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 2).unwrap() + chrono::Duration::days(offset)
}

fn series(values: &[f64]) -> Vec<SeriesPoint> {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| SeriesPoint {
            run_id: index as i64 + 1,
            date: date(index as i64),
            value: *value,
        })
        .collect()
}

#[test]
fn trend_figures_match_hand_calculation() {
    // Mon 11-02 .. Mon 11-09: two ISO weeks.
    let points = series(&[10.0, 12.0, 11.0, 15.0, 13.0, 14.0, 16.0, 12.0]);
    let trend = skill_trend(&points, Direction::Higher).unwrap();

    assert!((trend.latest - 12.0).abs() < EPSILON);
    // 12 − 16.
    close(trend.previous_difference, -4.0);
    // Latest 7: 12 11 15 13 14 16 12 → sorted 11 12 12 13 14 15 16.
    close(trend.moving_median, 13.0);
    // |differences| 2 1 4 2 1 2 4 → sorted 1 1 2 2 2 4 4.
    close(trend.typical_change, 2.0);
    assert_eq!(trend.beyond_typical, Some(true));
    // Least squares on x = 0..7: cov 20.5, var 42 → 20.5/42 per day.
    close(trend.slope_per_week, 20.5 / 42.0 * 7.0);
    // 3.42/week > 2 × 1/4.
    assert_eq!(trend.trend, Some(TrendDirection::Improving));

    let lower = skill_trend(&points, Direction::Lower).unwrap();
    assert_eq!(lower.trend, Some(TrendDirection::Declining));
    // The raw difference keeps its sign whatever the direction.
    close(lower.previous_difference, -4.0);
}

#[test]
fn even_count_median_averages_the_middle_two() {
    let trend = skill_trend(&series(&[10.0, 12.0, 11.0, 15.0, 13.0, 14.0]), Direction::Higher).unwrap();
    // Sorted 10 11 12 13 14 15.
    close(trend.moving_median, 12.5);
    // |differences| 2 1 4 2 1 → 2; latest difference +1 is within it.
    close(trend.typical_change, 2.0);
    close(trend.previous_difference, 1.0);
    assert_eq!(trend.beyond_typical, Some(false));
}

#[test]
fn lower_is_better_component_improves_when_its_value_falls() {
    let points = series(&[10.0, 8.0, 9.0, 5.0, 7.0, 6.0, 4.0, 8.0]);
    let trend = skill_trend(&points, Direction::Lower).unwrap();
    close(trend.slope_per_week, -20.5 / 42.0 * 7.0);
    assert_eq!(trend.trend, Some(TrendDirection::Improving));
    close(trend.previous_difference, 4.0);
}

#[test]
fn moving_median_needs_five_runs() {
    assert_eq!(MOVING_MEDIAN_MIN_POINTS, 5);
    let four = skill_trend(&series(&[10.0, 11.0, 12.0, 13.0]), Direction::Higher).unwrap();
    assert_eq!(four.moving_median, None);
    let five = skill_trend(&series(&[10.0, 11.0, 12.0, 13.0, 20.0]), Direction::Higher).unwrap();
    close(five.moving_median, 12.0);
}

#[test]
fn typical_change_needs_four_differences_and_trend_direction_needs_it() {
    assert_eq!(TYPICAL_CHANGE_MIN_DIFFERENCES, 4);
    // Four points spread over two weeks: a slope but only 3 differences.
    let points = [0, 3, 7, 9]
        .iter()
        .zip([10.0, 11.0, 12.0, 14.0])
        .map(|(offset, value)| SeriesPoint {
            run_id: *offset,
            date: date(*offset),
            value,
        })
        .collect::<Vec<_>>();
    let trend = skill_trend(&points, Direction::Higher).unwrap();
    assert_eq!(trend.typical_change, None);
    assert_eq!(trend.beyond_typical, None);
    assert!(trend.slope_per_week.is_some());
    assert_eq!(trend.trend, None);

    let only_one = skill_trend(&series(&[10.0]), Direction::Higher).unwrap();
    assert_eq!(only_one.previous_difference, None);
}

#[test]
fn typical_change_uses_only_the_latest_twenty_differences() {
    // Three huge jumps first, then 20 differences of exactly 1.
    let mut values = vec![0.0, 100.0, 0.0, 100.0];
    for index in 0..20 {
        values.push(100.0 + f64::from(index + 1));
    }
    let trend = skill_trend(&series(&values), Direction::Higher).unwrap();
    close(trend.typical_change, 1.0);
}

#[test]
fn no_trend_when_the_last_28_days_fall_in_one_calendar_week() {
    // Tue 11-03 .. Sat 11-07, one ISO week.
    let points = (1..=5)
        .map(|offset| SeriesPoint {
            run_id: offset,
            date: date(offset),
            value: offset as f64,
        })
        .collect::<Vec<_>>();
    let trend = skill_trend(&points, Direction::Higher).unwrap();
    assert_eq!(trend.slope_per_week, None);
    assert_eq!(trend.trend, None);

    // A point older than 28 days does not count as a second week.
    let mut with_old = points.clone();
    with_old.insert(
        0,
        SeriesPoint {
            run_id: 0,
            date: date(5) - chrono::Duration::days(28),
            value: 0.0,
        },
    );
    assert_eq!(skill_trend(&with_old, Direction::Higher).unwrap().slope_per_week, None);
}

#[test]
fn series_is_ordered_by_date_and_run_id() {
    let mut points = series(&[10.0, 12.0, 11.0]);
    points.reverse();
    let trend = skill_trend(&points, Direction::Higher).unwrap();
    assert!((trend.latest - 11.0).abs() < EPSILON);
    close(trend.previous_difference, -1.0);
}

// ---------------------------------------------------------------------------
// §8.1 snapshots in a temporary database
// ---------------------------------------------------------------------------

const TEXT_EN: &str = "the quick brown fox jumps over the lazy dog ";

/// A session typed on `date` with `chars` correct keystrokes.
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

fn practice(code: &str) -> Lang {
    if code == "ja" { Lang::Ja } else { Lang::En }
}

fn fixed_step(role: MenuRole, language: &str) -> MenuStep {
    let id = match role {
        MenuRole::Warmup => "fixed.warmup",
        MenuRole::Check => "fixed.check",
        _ => "fixed.finish",
    };
    MenuStep {
        role,
        language: practice(language),
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

fn prescription_step(id: &str, role: MenuRole, language: &str) -> MenuStep {
    let prescription = bundled_catalog().unwrap().prescription(id).unwrap();
    let row = &prescription.levels[0];
    MenuStep {
        role,
        language: practice(language),
        prescription_id: id.to_owned(),
        title: prescription.title.text(Lang::Ja).to_owned(),
        level: Some(1),
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

    fn with_context<T>(&self, date: NaiveDate, body: impl FnOnce(&JudgeContext<'_>) -> T) -> T {
        let layout = rs_typing_trainer::layout::builtin_layout("qwerty").unwrap();
        let context = JudgeContext {
            db_path: &self.path,
            home: self.directory.path(),
            config: &self.config,
            layout: &layout,
            as_of: date,
        };
        body(&context)
    }

    fn run(&self, menu: &DailyMenu, key: &str) -> i64 {
        db::ensure_menu_run(
            &self.path,
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

    fn saved(&self, mut record: SessionRecord) -> SessionRecord {
        record.header.id = Some(db::save(&self.path, &record).unwrap());
        record
    }

    /// Finish one step the way the app does: record the step, then write the
    /// run's snapshots.
    fn finish(&self, date: NaiveDate, run_id: i64, index: usize, record: &SessionRecord) {
        self.with_context(date, |context| {
            record_step_completion(context, run_id, index, record).unwrap();
            record_run_snapshots(context, run_id).unwrap();
        });
    }

    fn snapshots(&self) -> Vec<SkillSnapshotRecord> {
        db::load_skill_snapshots(&self.path).unwrap()
    }

    fn snapshot(&self, run_id: i64, component: Component) -> Option<SkillSnapshotRecord> {
        self.snapshots()
            .into_iter()
            .find(|row| row.run_id == run_id && row.component == component.as_str())
    }
}

fn pure_snapshots(store: &Store, date: NaiveDate, check: &SessionRecord, others: &[SessionRecord], corrected: Option<f64>) -> Vec<SkillSnapshot> {
    let layout = rs_typing_trainer::layout::builtin_layout("qwerty").unwrap();
    let geometry = rs_typing_trainer::layout::geometry::geometry("ansi").unwrap();
    let corpus = DataCatalog::load(&rs_typing_trainer::config::DataConfig::default(), store.directory.path()).unwrap();
    compute_run_snapshots(&SnapshotInputs {
        run_id: 1,
        date,
        check,
        other_steps: others,
        corrected_wpm: corrected,
        layout: &layout,
        geometry: &geometry,
        settings: rs_typing_trainer::analysis::metrics::AnalysisSettings {
            pause_ms: store.config.analysis.pause_ms,
            shrinkage_k: store.config.analysis.shrinkage_k,
            half_life_days: store.config.analysis.half_life_days,
        },
        min_transition_n: store.config.analysis.min_transition_n,
        english_corpus: corpus.words_en(),
        japanese_corpus: corpus.words_ja(),
    })
}

#[test]
fn nothing_is_written_before_the_check_step_and_sources_follow_the_design() {
    let store = Store::new();
    let day = date(0);
    let menu = plan(day, vec![fixed_step(MenuRole::Warmup, "en"), fixed_step(MenuRole::Check, "en")]);
    let run = store.run(&menu, "en");

    let warmup = store.saved(session(day, 8, "en", 50.0, 120));
    store.finish(day, run, 0, &warmup);
    assert!(store.snapshots().is_empty());

    let check = store.saved(session(day, 9, "en", 55.0, 120));
    store.finish(day, run, 1, &check);
    let rows = store.snapshots();
    assert!(!rows.is_empty());
    for row in &rows {
        assert_eq!(row.lang, "en");
        assert_eq!(row.date, day);
        let expected = match row.component.as_str() {
            "transition" | "recovery" => "check+steps",
            _ => "check",
        };
        assert_eq!(row.source, expected, "{}", row.component);
        assert!(row.raw_value.is_finite());
        assert!(row.n > 0);
    }
    // All keystrokes are correct, so there is no post-error interval:
    // recovery is "no data", never 0.
    assert!(store.snapshot(run, Component::Recovery).is_none());
    // Keystrokes of the check step only.
    assert_eq!(store.snapshot(run, Component::Speed).unwrap().n, 120);
}

#[test]
fn speed_without_a_corrected_wpm_stores_the_raw_net_wpm_uncorrected() {
    let store = Store::new();
    let day = date(0);
    let menu = plan(day, vec![fixed_step(MenuRole::Check, "en")]);
    let run = store.run(&menu, "en");
    // A fresh database has no prior keystrokes, so there is no form.
    let check = store.saved(session(day, 9, "en", 57.5, 120));
    store.finish(day, run, 0, &check);

    let speed = store.snapshot(run, Component::Speed).unwrap();
    assert!(!speed.corrected);
    assert!((speed.raw_value - 57.5).abs() < EPSILON);

    let corrected = pure_snapshots(&store, day, &check, &[], Some(61.25));
    let speed = corrected.iter().find(|row| row.component == Component::Speed).unwrap();
    assert!(speed.corrected);
    assert!((speed.raw_value - 61.25).abs() < EPSILON);
    let uncorrected = pure_snapshots(&store, day, &check, &[], None);
    let speed = uncorrected.iter().find(|row| row.component == Component::Speed).unwrap();
    assert!(!speed.corrected);
    assert!((speed.raw_value - 57.5).abs() < EPSILON);
    assert!(corrected.iter().filter(|row| row.component != Component::Speed).all(|row| !row.corrected));
}

#[test]
fn speed_uses_the_corrected_wpm_once_the_progress_analysis_has_a_form() {
    let store = Store::new();
    let day = date(3);
    for offset in 0..3 {
        store.saved(session(date(offset), 9, "en", 50.0, 400));
    }
    let menu = plan(day, vec![fixed_step(MenuRole::Check, "en")]);
    let run = store.run(&menu, "en");
    let check = store.saved(session(day, 9, "en", 52.0, 200));
    store.finish(day, run, 0, &check);

    let speed = store.snapshot(run, Component::Speed).unwrap();
    assert!(speed.corrected, "{speed:?}");
    assert!(speed.raw_value.is_finite() && speed.raw_value > 0.0);
}

#[test]
fn transition_and_recovery_below_min_transition_n_have_no_row_and_show_no_data() {
    let mut store = Store::new();
    store.config.analysis.min_transition_n = 1_000_000;
    let day = date(0);
    let menu = plan(day, vec![fixed_step(MenuRole::Check, "en")]);
    let run = store.run(&menu, "en");
    let check = store.saved(session(day, 9, "en", 55.0, 120));
    store.finish(day, run, 0, &check);

    assert!(store.snapshot(run, Component::Transition).is_none());
    assert!(store.snapshot(run, Component::Recovery).is_none());
    assert!(store.snapshot(run, Component::Speed).is_some());

    let report = skills_report(&store.snapshots());
    let english = &report.languages[0];
    let transition = english.components.iter().find(|row| row.component == Component::Transition).unwrap();
    assert_eq!(transition.latest, None);
    assert_eq!(transition.runs, 0);
    let text = render_text(&report, Lang::Ja);
    let line = text.lines().find(|line| line.starts_with("遷移")).unwrap();
    assert!(line.contains("データなし"), "{text}");
    let text = render_text(&report, Lang::En);
    let line = text.lines().find(|line| line.starts_with("Transitions")).unwrap();
    assert!(line.contains("no data"), "{text}");
}

#[test]
fn a_later_step_rewrites_the_run_without_duplicates_and_writing_twice_is_idempotent() {
    let store = Store::new();
    let day = date(0);
    let menu = plan(day, vec![fixed_step(MenuRole::Check, "en"), fixed_step(MenuRole::Finish, "en")]);
    let run = store.run(&menu, "en");
    let check = store.saved(session(day, 9, "en", 55.0, 120));
    store.finish(day, run, 0, &check);
    let before = store.snapshots();
    let transition_before = store.snapshot(run, Component::Transition).unwrap();

    let finish = store.saved(session(day, 10, "en", 60.0, 120));
    store.finish(day, run, 1, &finish);
    let after = store.snapshots();
    assert_eq!(after.len(), before.len());
    let transition_after = store.snapshot(run, Component::Transition).unwrap();
    assert!(transition_after.n > transition_before.n, "{transition_before:?} → {transition_after:?}");
    // Components measured on the check step alone do not change.
    assert_eq!(
        store.snapshot(run, Component::Speed),
        before.iter().find(|row| row.component == "speed").cloned()
    );

    store.with_context(day, |context| {
        let first = record_run_snapshots(context, run).unwrap();
        let second = record_run_snapshots(context, run).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.rows, after.len());
    });
    assert_eq!(store.snapshots(), after);
}

#[test]
fn prescription_steps_are_left_out_of_transition_and_recovery() {
    let store = Store::new();
    let day = date(0);
    let menu = plan(
        day,
        vec![
            prescription_step("preview.wide", MenuRole::PrimaryA, "en"),
            fixed_step(MenuRole::Check, "en"),
        ],
    );
    let run = store.run(&menu, "en");
    let step = store.saved(session(day, 8, "en", 45.0, 200));
    store.finish(day, run, 0, &step);
    let check = store.saved(session(day, 9, "en", 55.0, 120));
    store.finish(day, run, 1, &check);

    let check_only = pure_snapshots(&store, day, &check, &[], None);
    let expected = check_only.iter().find(|row| row.component == Component::Transition).unwrap();
    assert_eq!(store.snapshot(run, Component::Transition).unwrap().n, expected.n);
}

#[test]
fn a_two_language_day_goes_only_to_the_check_language_series() {
    let store = Store::new();
    let day = date(0);
    let menu = plan(
        day,
        vec![
            fixed_step(MenuRole::Warmup, "en"),
            fixed_step(MenuRole::Check, "ja"),
            fixed_step(MenuRole::Finish, "en"),
        ],
    );
    let run = store.run(&menu, "en,ja");
    let warmup = store.saved(session(day, 8, "en", 50.0, 120));
    store.finish(day, run, 0, &warmup);
    let check = store.saved(session(day, 9, "ja", 40.0, 120));
    store.finish(day, run, 1, &check);
    let finish = store.saved(session(day, 10, "en", 60.0, 120));
    store.finish(day, run, 2, &finish);

    let rows = store.snapshots();
    assert!(!rows.is_empty());
    assert!(rows.iter().all(|row| row.lang == "ja"), "{rows:?}");
    // The English warm-up and finish are not mixed into the Japanese series.
    let check_only = pure_snapshots(&store, day, &check, &[], None);
    let expected = check_only.iter().find(|row| row.component == Component::Transition).unwrap();
    assert_eq!(store.snapshot(run, Component::Transition).unwrap().n, expected.n);
    assert_eq!(expected.source, SnapshotSource::CheckSteps);
    let report = skills_report(&rows);
    assert_eq!(report.languages.iter().map(|row| row.language.as_str()).collect::<Vec<_>>(), vec!["ja"]);
}

fn seed_runs(store: &Store, days: i64) {
    for offset in 0..days {
        let day = date(offset);
        let menu = plan(day, vec![fixed_step(MenuRole::Warmup, "en"), fixed_step(MenuRole::Check, "en")]);
        let run = store.run(&menu, "en");
        let warmup = store.saved(session(day, 8, "en", 48.0, 100));
        store.finish(day, run, 0, &warmup);
        let check = store.saved(session(day, 9, "en", 50.0 + offset as f64, 100 + offset as usize * 10));
        store.finish(day, run, 1, &check);
    }
}

#[test]
fn rebuild_replaces_every_row_and_is_idempotent() {
    let store = Store::new();
    seed_runs(&store, 3);
    let written = store.snapshots();

    // A stale row from an old calculation disappears on rebuild.
    db::replace_skill_snapshots(
        &store.path,
        1,
        &[SkillSnapshotRecord {
            run_id: 1,
            date: date(0),
            lang: "en".to_owned(),
            component: "speed".to_owned(),
            raw_value: 1.0,
            n: 1,
            source: "check".to_owned(),
            corrected: false,
        }],
    )
    .unwrap();

    let first = store.with_context(date(3), |context| rebuild_all_snapshots(context).unwrap());
    let rebuilt = store.snapshots();
    let second = store.with_context(date(3), |context| rebuild_all_snapshots(context).unwrap());
    assert_eq!(first, second);
    assert_eq!(first.runs, 3);
    assert_eq!(first.rows, rebuilt.len());
    assert_eq!(store.snapshots(), rebuilt);
    assert_eq!(rebuilt, written);
}

// ---------------------------------------------------------------------------
// `stats --skills` and `stats --rebuild-skills` through the binary
// ---------------------------------------------------------------------------

fn write_config(store: &Store) -> std::path::PathBuf {
    let config_path = store.directory.path().join("config.toml");
    std::fs::write(&config_path, format!("[storage]\ndb_path = {:?}\n", store.path.to_str().unwrap())).unwrap();
    config_path
}

fn run_cli(config: &std::path::Path, home: &std::path::Path, args: &[&str]) -> (bool, String, String) {
    let output = ProcessCommand::new(env!("CARGO_BIN_EXE_rs-typing-trainer"))
        .env("HOME", home)
        .args(["--config", config.to_str().unwrap()])
        .args(args)
        .output()
        .unwrap();
    (
        output.status.success(),
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

#[test]
fn stats_skills_prints_one_line_and_an_empty_json_report_without_records() {
    let store = Store::new();
    let config = write_config(&store);
    let home = store.directory.path();

    let (ok, stdout, stderr) = run_cli(&config, home, &["--lang", "ja", "stats", "--skills"]);
    assert!(ok, "{stderr}");
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
    assert!(stdout.contains("スキルの記録はまだありません"), "{stdout}");

    let (ok, stdout, stderr) = run_cli(&config, home, &["--lang", "en", "stats", "--skills"]);
    assert!(ok, "{stderr}");
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
    assert!(stdout.contains("No skill records yet"), "{stdout}");

    let (ok, stdout, stderr) = run_cli(&config, home, &["stats", "--skills", "--json"]);
    assert!(ok, "{stderr}");
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["languages"].as_array().unwrap().len(), 0);
    assert_eq!(value["grade_version"], "grades-v1");
}

#[test]
fn stats_skills_text_json_and_rebuild_through_the_binary() {
    let store = Store::new();
    seed_runs(&store, 6);
    let config = write_config(&store);
    let home = store.directory.path();
    let rows = store.snapshots().len();

    let (ok, stdout, stderr) = run_cli(&config, home, &["--lang", "ja", "stats", "--skills"]);
    assert!(ok, "{stderr}");
    for expected in [
        "今のスキル",
        "英語",
        "構成",
        "移動中央値",
        "ふだんの変動",
        "傾向",
        "速さ",
        "立ち直り",
        "データなし",
    ] {
        assert!(stdout.contains(expected), "missing {expected}:\n{stdout}");
    }

    let (ok, stdout, stderr) = run_cli(&config, home, &["stats", "--skills", "--json"]);
    assert!(ok, "{stderr}");
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let languages = value["languages"].as_array().unwrap();
    assert_eq!(languages.len(), 1);
    assert_eq!(languages[0]["language"], "en");
    let components = languages[0]["components"].as_array().unwrap();
    assert_eq!(components.len(), 7);
    let speed = components.iter().find(|row| row["component"] == "speed").unwrap();
    assert_eq!(speed["runs"], 6);
    // Expected figures from the stored series (whether each run's speed was
    // corrected depends on the progress analysis, not on this test).
    let mut stored = store.snapshots().into_iter().filter(|row| row.component == "speed").collect::<Vec<_>>();
    stored.sort_by_key(|row| (row.date, row.run_id));
    let values = stored.iter().map(|row| row.raw_value).collect::<Vec<_>>();
    assert!((speed["latest"]["raw_value"].as_f64().unwrap() - values[5]).abs() < EPSILON);
    assert_eq!(speed["latest"]["corrected"], stored[5].corrected);
    assert!((speed["previous_difference"].as_f64().unwrap() - (values[5] - values[4])).abs() < EPSILON);
    let mut sorted = values.clone();
    sorted.sort_by(f64::total_cmp);
    // Six runs: the mean of the middle two.
    assert!((speed["moving_median"].as_f64().unwrap() - (sorted[2] + sorted[3]) / 2.0).abs() < EPSILON);
    assert!(speed["grade"].is_string());
    assert!(speed["score"].is_number());
    let recovery = components.iter().find(|row| row["component"] == "recovery").unwrap();
    assert!(recovery["latest"].is_null());
    assert!(recovery["moving_median"].is_null());

    for _ in 0..2 {
        let (ok, stdout, stderr) = run_cli(&config, home, &["--lang", "en", "stats", "--rebuild-skills"]);
        assert!(ok, "{stderr}");
        assert_eq!(stdout.trim(), format!("Rebuilt skill snapshots: 6 runs, {rows} rows"));
        assert_eq!(store.snapshots().len(), rows);
    }
}

#[test]
fn a_check_step_without_keystrokes_writes_no_rows() {
    let store = Store::new();
    let day = date(0);
    let menu = plan(day, vec![fixed_step(MenuRole::Check, "en")]);
    let run = store.run(&menu, "en");
    // As stored by the app when nothing was typed: no keystroke rows,
    // net WPM 0 and accuracy 100%.
    let mut empty = session(day, 9, "en", 0.0, 0);
    empty.header.accuracy = 1.0;
    assert!(empty.log.keystrokes.is_empty());
    let check = store.saved(empty);
    store.finish(day, run, 0, &check);

    assert_eq!(store.snapshots(), Vec::new());
    assert!(pure_snapshots(&store, day, &check, &[], Some(50.0)).is_empty());
    store.with_context(day, |context| {
        assert_eq!(rebuild_all_snapshots(context).unwrap().rows, 0);
    });
    assert!(store.snapshots().is_empty());
}

#[test]
fn every_stored_component_has_observations() {
    let store = Store::new();
    seed_runs(&store, 2);
    assert!(store.snapshots().iter().all(|row| row.n > 0));
}

#[test]
fn no_data_rows_keep_the_component_column_aligned() {
    use unicode_width::UnicodeWidthStr;

    let mut store = Store::new();
    store.config.analysis.min_transition_n = 1_000_000;
    let day = date(0);
    let menu = plan(day, vec![fixed_step(MenuRole::Check, "en")]);
    let run = store.run(&menu, "en");
    let check = store.saved(session(day, 9, "en", 55.0, 120));
    store.finish(day, run, 0, &check);
    let report = skills_report(&store.snapshots());

    for (lang, no_data) in [(Lang::En, "— (no data)"), (Lang::Ja, "—（データなし）")] {
        let text = render_text(&report, lang);
        let lines = text.lines().skip(1).collect::<Vec<_>>();
        assert_eq!(lines.len(), 8, "{text}");
        // The second column starts at the same display column on every row:
        // the header, the measured rows and the "no data" rows.
        let starts = lines
            .iter()
            .map(|line| {
                let name_end = line.find("  ").unwrap();
                let rest = &line[name_end..];
                let second = name_end + (rest.len() - rest.trim_start().len());
                UnicodeWidthStr::width(&line[..second])
            })
            .collect::<Vec<_>>();
        assert!(starts.iter().all(|start| *start == starts[0]), "{lang:?} {starts:?}\n{text}");
        let transition = lines
            .iter()
            .find(|line| line.starts_with("Transitions") || line.starts_with("遷移"))
            .unwrap();
        assert!(transition.contains(no_data), "{text}");
    }
}
