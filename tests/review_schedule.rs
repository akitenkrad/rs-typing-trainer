use chrono::{Duration, TimeZone, Utc};
use rs_typing_trainer::analysis::prescription::{DifficultyState, GoalSuggestion};
use rs_typing_trainer::practice::review::{ReviewSchedule, update_review};
use rs_typing_trainer::record::db;
use tempfile::TempDir;

#[test]
fn successful_reviews_expand_by_two_point_five_and_regression_resets_to_one_day() {
    let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let first = update_review(None, "qwerty", "a", "s", now, true, false);
    assert_eq!(first.interval_days, 1.0);
    let second = update_review(Some(first), "qwerty", "a", "s", now + Duration::days(1), true, false);
    assert_eq!(second.interval_days, 2.5);
    let third = update_review(Some(second), "qwerty", "a", "s", now + Duration::days(4), true, false);
    assert_eq!(third.interval_days, 6.25);
    let reset = update_review(Some(third), "qwerty", "a", "s", now + Duration::days(11), false, true);
    assert_eq!(reset.interval_days, 1.0);
}

#[test]
fn selected_goal_and_difficulty_state_roundtrip_in_meta() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    let state = DifficultyState {
        pacer_multiplier: 1.03,
        drill_ratio: 0.35,
    };
    db::save_adaptive_difficulty(&path, &state).unwrap();
    assert_eq!(db::load_adaptive_difficulty(&path).unwrap(), Some(state));
    let goal = GoalSuggestion {
        kind: "next-grade".into(),
        value: 60.0,
    };
    db::save_selected_goal(&path, &goal).unwrap();
    assert_eq!(db::load_selected_goal(&path).unwrap(), Some(goal));
}

#[test]
fn review_schedule_roundtrips_and_v6_migrates_without_losing_prescriptions() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    db::list(&path).unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute(
        "INSERT INTO prescriptions (created_at,layout,kind,target_json,control_json,hypothesis_json,baseline_json,status) VALUES (?1,'qwerty','adaptive','{}','[]','{}','{}','active')",
        ["2026-01-01T00:00:00Z"],
    ).unwrap();
    connection.execute("DROP TABLE review_schedule", []).unwrap();
    connection.pragma_update(None, "user_version", 6).unwrap();
    drop(connection);

    let now = Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap();
    let schedule = ReviewSchedule {
        layout: "qwerty".into(),
        from: "a".into(),
        to: "s".into(),
        next_due: now,
        interval_days: 1.0,
        last_reviewed_at: now - Duration::days(1),
    };
    db::save_review_schedule(&path, &schedule).unwrap();
    assert_eq!(db::load_review_schedules(&path, "qwerty").unwrap(), vec![schedule]);
    assert_eq!(db::load_prescriptions(&path).unwrap().len(), 1);
    let connection = rusqlite::Connection::open(path).unwrap();
    assert_eq!(
        connection.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0)).unwrap(),
        8
    );
    let has_kind = connection
        .prepare("PRAGMA table_info(items)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .any(|name| name.unwrap() == "kind");
    assert!(has_kind);
}

#[test]
fn opening_v7_database_repairs_missing_item_kind_column() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    db::list(&path).unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute("ALTER TABLE items DROP COLUMN kind", []).unwrap();
    assert_eq!(
        connection.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0)).unwrap(),
        8
    );
    drop(connection);

    db::list(&path).unwrap();
    let connection = rusqlite::Connection::open(path).unwrap();
    let has_kind = connection
        .prepare("PRAGMA table_info(items)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .any(|name| name.unwrap() == "kind");
    assert!(has_kind);
}
