use std::io::Cursor;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use rs_typing_trainer::config::Config;
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::record::SessionRecord;
use rs_typing_trainer::record::ttrec;

fn record() -> SessionRecord {
    let mut session = Session::new(Duration::from_secs(1), Duration::from_secs(1), Item::exact("ab"));
    session.activate(Duration::ZERO);
    session.input_char(Duration::from_millis(100), "a");
    session.input_char(Duration::from_millis(250), "b");
    session.tick(Duration::from_secs(1));
    SessionRecord::from_log(
        &Config::default(),
        session.into_log(),
        Utc.with_ymd_and_hms(2026, 11, 14, 0, 0, 0).unwrap(),
    )
}

#[test]
fn write_then_read_roundtrip() {
    let expected = record();
    let mut bytes = Vec::new();
    ttrec::write(&expected, &mut bytes).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.lines().next().unwrap().contains(r#""type":"session""#));
    assert!(text.lines().next().unwrap().contains(r#""format_version":1"#));

    let actual = ttrec::read(Cursor::new(bytes)).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn broken_json_reports_the_line_number() {
    let expected = record();
    let mut bytes = Vec::new();
    ttrec::write(&expected, &mut bytes).unwrap();
    let mut lines: Vec<_> = String::from_utf8(bytes).unwrap().lines().map(str::to_owned).collect();
    lines.insert(2, "{broken".to_owned());
    let error = ttrec::read(Cursor::new(lines.join("\n"))).unwrap_err();
    assert!(error.to_string().contains("line 3"), "{error}");
}
