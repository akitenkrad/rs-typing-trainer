use std::time::Duration;

use chrono::{TimeZone, Utc};
use rs_typing_trainer::config::{Config, ErrorMode, KeyFilter, ModeKind};
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::{ItemKind, Session};
use rs_typing_trainer::record::SessionRecord;
use rs_typing_trainer::record::db;
use rs_typing_trainer::record::{GhostModeParams, SessionModeParams};
use tempfile::TempDir;

fn sample_record() -> SessionRecord {
    let mut session = Session::new(Duration::from_secs(2), Duration::from_secs(1), Item::exact("ab"));
    session.activate(Duration::ZERO);
    session.input_char(Duration::from_millis(100), "a");
    session.input_char(Duration::from_millis(250), "b");
    session.tick(Duration::from_secs(2));
    SessionRecord::from_log(
        &Config::default(),
        session.into_log(),
        Utc.with_ymd_and_hms(2026, 11, 14, 1, 2, 3).unwrap(),
    )
}

#[test]
fn save_list_and_load_roundtrip() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("nested/sessions.db");
    let record = sample_record();

    let id = db::save(&path, &record).unwrap();
    let listed = db::list(&path).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, id);

    let loaded = db::load_record(&path, id).unwrap().unwrap();
    let mut expected = record;
    expected.header.id = Some(id);
    assert_eq!(loaded, expected);
    assert_eq!(loaded.log.items[0].kind, Some(ItemKind::Word));

    let conn = rusqlite::Connection::open(path).unwrap();
    assert_eq!(conn.query_row("SELECT count(*) FROM items", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(
        conn.query_row("SELECT count(*) FROM keystrokes", [], |row| row.get::<_, i64>(0)).unwrap(),
        2
    );
    assert_eq!(
        conn.query_row("SELECT kind FROM items", [], |row| row.get::<_, String>(0)).unwrap(),
        "word"
    );
}

#[test]
fn item_kind_roundtrips_through_database() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("sessions.db");
    let mut record = sample_record();
    record.log.items[0].kind = Some(ItemKind::Drill);

    let id = db::save(&path, &record).unwrap();
    let loaded = db::load_record(&path, id).unwrap().unwrap();
    assert_eq!(loaded.log.items[0].kind, Some(ItemKind::Drill));
}

#[test]
fn record_header_uses_configured_keyboard_values() {
    let mut config = Config::default();
    config.keyboard.layout = "dvorak".to_owned();
    config.keyboard.input_layout = "qwerty".to_owned();
    config.keyboard.geometry = "jis".to_owned();
    config.mode.kind = ModeKind::Ngram;
    config.mode.keys = "th".to_owned();
    config.mode.key_filter = KeyFilter::Only;
    config.mode.base_keys = "home".to_owned();
    config.mode.ngram.n = 3;
    config.mode.ngram.source = "corpus".to_owned();
    config.session.error_mode = ErrorMode::Advance;
    config.session.preview_items = 3;
    config.session.blind = true;
    config.keyboard.show = "auto".to_owned();
    config.keyboard.style = "raised".to_owned();
    config.display.prompt_style = "normal".to_owned();
    config.gate.min_accuracy = 0.96;
    let mut session = Session::with_error_mode(Duration::from_secs(1), Duration::from_secs(1), Item::exact("a"), ErrorMode::Advance);
    session.activate(Duration::ZERO);
    session.input_char_with_code(Duration::from_millis(100), "a", Some("KeyA".to_owned()));
    session.tick(Duration::from_secs(1));
    let record = SessionRecord::from_log(&config, session.into_log(), Utc.with_ymd_and_hms(2026, 11, 14, 1, 2, 3).unwrap());

    assert_eq!(record.header.layout, "dvorak");
    assert_eq!(record.header.input_layout, "qwerty");
    assert_eq!(record.header.geometry, "jis");
    assert_eq!(record.header.mode, "ngram");
    assert_eq!(record.header.error_mode, "advance");
    let params: serde_json::Value = serde_json::from_str(&record.header.mode_params).unwrap();
    assert_eq!(params["kind"], "ngram");
    assert_eq!(params["keys"], "th");
    assert_eq!(params["key_filter"], "only");
    assert_eq!(params["base_keys"], "home");
    assert_eq!(params["n"], 3);
    assert_eq!(params["source"], "corpus");
    assert_eq!(params["preview_items"], 3);
    assert_eq!(params["blind"], true);
    assert_eq!(params["ghost"]["enabled"], false);
    assert_eq!(params["keyboard_show"], "auto");
    assert_eq!(params["keyboard_style"], "raised");
    assert_eq!(params["prompt_style"], "normal");
    assert_eq!(params["accuracy_gate"], 0.96);
    assert_eq!(record.log.keystrokes[0].code.as_deref(), Some("KeyA"));
}

#[test]
fn mode_params_roundtrip_assists_and_accept_old_json() {
    let params = SessionModeParams {
        pacer_enabled: Some(true),
        preview_items: Some(2),
        blind: Some(true),
        ghost: Some(GhostModeParams {
            enabled: true,
            opponent: Some("session:42".to_owned()),
        }),
        keyboard_show: Some("dim".to_owned()),
        keyboard_style: Some("plain".to_owned()),
        prompt_style: Some("large".to_owned()),
        accuracy_gate: Some(0.97),
    };
    let json = serde_json::to_string(&params).unwrap();
    assert_eq!(serde_json::from_str::<SessionModeParams>(&json).unwrap(), params);

    let old = serde_json::from_str::<SessionModeParams>(r#"{"kind":"words","lang":"en"}"#).unwrap();
    assert_eq!(old, SessionModeParams::default());

    let mut record = sample_record();
    record.set_ghost_opponent(Some("session:42".to_owned()));
    let value: serde_json::Value = serde_json::from_str(&record.header.mode_params).unwrap();
    assert_eq!(value["ghost"]["enabled"], true);
    assert_eq!(value["ghost"]["opponent"], "session:42");
}
