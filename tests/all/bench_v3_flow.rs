use std::time::{Duration, Instant};

use chrono::{TimeZone, Utc};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::analysis::growth::build_growth;
use rs_typing_trainer::analysis::progress::Progress;
use rs_typing_trainer::app::Screen;
use rs_typing_trainer::bench::{BENCH_V3_SERIES, BenchV3Part, parse_bench_id, parse_v3_bench_id};
use rs_typing_trainer::config::{BenchSeries, Config, ErrorMode, KeyFilter, ModeKind};
use rs_typing_trainer::engine::judge::JudgeResult;
use rs_typing_trainer::engine::session::SessionLog;
use rs_typing_trainer::record::{SessionRecord, db};
use rs_typing_trainer::ui;

use crate::common::{test_app, test_app_at};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn render(app: &rs_typing_trainer::app::App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<Vec<_>>()
        .join("")
}

fn v3_config() -> Config {
    let mut config = Config::default();
    config.bench.series = BenchSeries::V3;
    config
}

fn start_benchmark(app: &mut rs_typing_trainer::app::App, now: Instant) {
    app.handle_key_at(key(KeyCode::Char('b')), now);
    assert_eq!(app.screen, Screen::BenchSelect);
    app.handle_key_at(key(KeyCode::Enter), now);
    assert!(matches!(app.screen, Screen::Countdown { .. }));
}

fn start_current_part(app: &mut rs_typing_trainer::app::App, now: Instant, date: chrono::NaiveDate) {
    app.on_tick_at_date(Duration::from_secs(3), now, date).unwrap();
    assert_eq!(app.screen, Screen::Playing);
}

fn finish_current_part(app: &mut rs_typing_trainer::app::App, started: Instant, duration_seconds: u64, date: chrono::NaiveDate) {
    app.on_tick_at_date(
        Duration::from_secs(duration_seconds),
        started + Duration::from_secs(duration_seconds),
        date,
    )
    .unwrap();
}

fn record(day: u32, bench_id: &str, wpm: f64) -> SessionRecord {
    let mut record = SessionRecord::from_log(
        &Config::default(),
        SessionLog {
            duration_us: 60_000_000,
            ..SessionLog::default()
        },
        Utc.with_ymd_and_hms(2026, 11, day, 12, 0, 0).unwrap(),
    );
    record.header.mode = "bench".to_owned();
    record.header.bench_id = Some(bench_id.to_owned());
    record.header.net_wpm = wpm;
    record.header.accuracy = 0.97;
    record
}

#[test]
fn v3_ids_have_their_own_parser_and_never_become_v2_parts() {
    assert_eq!(parse_bench_id("bench-v3/EW/03"), None);
    assert_eq!(parse_v3_bench_id("bench-v3/EW/03"), Some((BenchV3Part::EW, 3)));
    for invalid in [
        "bench-v2/EW/03",
        "bench-v3/A/03",
        "bench-v3/EW/00",
        "bench-v3/EW/09",
        "bench-v3/EW/3",
        "bench-v3/EW/03/extra",
    ] {
        assert_eq!(parse_v3_bench_id(invalid), None, "{invalid}");
    }
}

#[test]
fn v3_selection_lists_the_fixed_parts_without_optional_part_d() {
    let mut app = test_app(v3_config());
    app.handle_key(key(KeyCode::Char('b')));
    let output = render(&app);
    assert!(output.contains("BENCHMARK bench-v3"), "{output}");
    assert!(output.contains("W Warm-up"), "{output}");
    assert!(output.contains("PW Pseudo-words"), "{output}");
    assert!(!output.contains("Numbers/symbols"), "{output}");
    assert!(!output.contains("Toggle Part D"), "{output}");
}

#[test]
fn v3_runs_warmup_then_five_scored_parts_with_fixed_conditions_and_restores_configuration() {
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 26).unwrap();
    let mut config = v3_config();
    config.session.duration_seconds = 9;
    config.session.item_timeout_seconds = 4;
    config.session.error_mode = ErrorMode::Advance;
    config.session.preview_items = 3;
    config.session.blind = true;
    config.keyboard.show = "full".to_owned();
    config.pacer.enabled = true;
    config.pacer.target_wpm = Some(88.0);
    config.mode.kind = ModeKind::Romaji;
    config.mode.keys = "asdf".to_owned();
    config.mode.key_filter = KeyFilter::Only;
    config.mode.sentence.lang = "ja".to_owned();
    let original = config.clone();
    let mut app = test_app_at(config, date);
    let origin = Instant::now();

    start_benchmark(&mut app, origin);
    assert_eq!(app.active_bench_id.as_deref(), Some("bench-v3/W/01"));
    assert_eq!(app.config.session.duration_seconds, 15);
    assert_eq!(app.config.session.item_timeout_seconds, 0);
    assert_eq!(app.config.session.error_mode, ErrorMode::Stop);
    assert_eq!(app.config.session.preview_items, 1);
    assert!(!app.config.session.blind);
    assert_eq!(app.config.keyboard.show, "hidden");
    assert!(!app.config.pacer.enabled);
    assert_eq!(app.config.pacer.target_wpm, None);
    assert!(app.config.mode.keys.is_empty());
    assert_eq!(app.config.mode.key_filter, KeyFilter::Include);
    assert!(render(&app).contains("Warm-up (not scored)"));

    start_current_part(&mut app, origin, date);
    assert!(app.game.as_ref().is_some_and(|game| game.ghost.is_none()));
    assert!(render(&app).contains("Warm-up (not scored)"));
    finish_current_part(&mut app, origin, 15, date);
    assert_eq!(app.screen, Screen::BenchWarmupComplete);
    assert!(render(&app).contains("Warm-up complete. English words are next"));
    assert!(db::load_records_filtered(&app.db_path(), None, Some("bench")).unwrap().is_empty());

    app.handle_key_at(key(KeyCode::Enter), origin + Duration::from_secs(16));
    let expected = [
        (BenchV3Part::EW, 60),
        (BenchV3Part::ES, 60),
        (BenchV3Part::JW, 60),
        (BenchV3Part::JS, 60),
        (BenchV3Part::PW, 30),
    ];
    let mut base = origin + Duration::from_secs(20);
    for (index, (part, duration)) in expected.into_iter().enumerate() {
        assert_eq!(app.active_bench_id.as_deref(), Some(format!("bench-v3/{}/01", part.as_str()).as_str()));
        assert_eq!(app.config.session.duration_seconds, duration);
        start_current_part(&mut app, base, date);
        finish_current_part(&mut app, base, duration, date);
        assert_eq!(app.screen, Screen::Result);
        app.handle_key_at(key(KeyCode::Enter), base + Duration::from_secs(duration + 2));
        if index + 1 < expected.len() {
            assert!(matches!(app.screen, Screen::Countdown { .. }));
        }
        base += Duration::from_secs(duration + 5);
    }
    assert_eq!(app.screen, Screen::BenchSummary);
    let records = db::load_records_filtered(&app.db_path(), None, Some("bench")).unwrap();
    assert_eq!(records.len(), 5);
    let ids = records
        .iter()
        .filter_map(|record| record.header.bench_id.as_deref())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        ids,
        ["bench-v3/EW/01", "bench-v3/ES/01", "bench-v3/JW/01", "bench-v3/JS/01", "bench-v3/PW/01"]
            .into_iter()
            .collect()
    );
    for record in &records {
        let (part, _) = parse_v3_bench_id(record.header.bench_id.as_deref().unwrap()).unwrap();
        let params: serde_json::Value = serde_json::from_str(&record.header.mode_params).unwrap();
        assert_eq!(
            params["lang"],
            if matches!(part, BenchV3Part::JW | BenchV3Part::JS) { "ja" } else { "en" }
        );
        assert_eq!(params["preview_items"], 1);
        assert_eq!(params["blind"], false);
        assert_eq!(params["pacer_enabled"], false);
        assert_eq!(params["keyboard_show"], "hidden");
        assert_eq!(params["ghost"]["enabled"], false);
        assert_eq!(params["keys"], "");
        assert_eq!(params["key_filter"], "include");
        assert_eq!(rs_typing_trainer::analysis::model::session_language(record), params["lang"]);
    }
    let summary = render(&app);
    assert!(summary.contains("v3 analysis is coming soon"), "{summary}");
    for part in [
        "English words",
        "English sentences",
        "Japanese words and phrases",
        "Japanese sentences",
        "Pseudo-words",
    ] {
        assert!(summary.contains(part), "missing {part}: {summary}");
    }
    assert_eq!(app.config.session.duration_seconds, original.session.duration_seconds);
    assert_eq!(app.config.session.error_mode, original.session.error_mode);
    assert_eq!(app.config.session.preview_items, original.session.preview_items);
    assert_eq!(app.config.session.blind, original.session.blind);
    assert_eq!(app.config.keyboard.show, original.keyboard.show);
    assert_eq!(app.config.pacer.enabled, original.pacer.enabled);
    assert_eq!(app.config.pacer.target_wpm, original.pacer.target_wpm);
    assert_eq!(app.config.mode.kind, original.mode.kind);
    assert_eq!(app.config.mode.keys, original.mode.keys);
    assert_eq!(app.config.mode.key_filter, original.mode.key_filter);
}

#[test]
fn aborting_v3_restores_all_fixed_user_settings() {
    let mut config = v3_config();
    config.session.blind = true;
    config.keyboard.show = "full".to_owned();
    config.pacer.enabled = true;
    config.mode.keys = "qwer".to_owned();
    let original = config.clone();
    let mut app = test_app(config);
    let now = Instant::now();
    start_benchmark(&mut app, now);
    app.handle_key_at(key(KeyCode::Esc), now + Duration::from_millis(100));
    app.handle_key_at(key(KeyCode::Esc), now + Duration::from_millis(200));
    assert_eq!(app.screen, Screen::Title);
    assert_eq!(app.config.session.blind, original.session.blind);
    assert_eq!(app.config.keyboard.show, original.keyboard.show);
    assert_eq!(app.config.pacer.enabled, original.pacer.enabled);
    assert_eq!(app.config.mode.keys, original.mode.keys);
}

#[test]
fn v3_forms_rotate_only_within_v3_and_resume_replays_warmup_before_first_missing_part() {
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 26).unwrap();
    let mut app = test_app_at(v3_config(), date);
    for part in ["EW", "ES"] {
        let mut saved = record(26, &format!("{BENCH_V3_SERIES}/{part}/08"), 50.0);
        let at = date
            .and_hms_opt(8, 0, 0)
            .unwrap()
            .and_local_timezone(chrono::Local)
            .earliest()
            .unwrap()
            .with_timezone(&Utc);
        saved.header.started_at = at.to_rfc3339();
        saved.header.finished_at = (at + chrono::Duration::minutes(1)).to_rfc3339();
        db::save(&app.db_path(), &saved).unwrap();
    }
    let now = Instant::now();
    app.handle_key_at(key(KeyCode::Char('b')), now);
    let selection = render(&app);
    assert!(selection.contains("Resume from part JW (form 08)"), "{selection}");
    app.handle_key_at(key(KeyCode::Enter), now);
    assert_eq!(app.active_bench_id.as_deref(), Some("bench-v3/W/08"));
    start_current_part(&mut app, now, date);
    finish_current_part(&mut app, now, 15, date);
    app.handle_key_at(key(KeyCode::Enter), now + Duration::from_secs(17));
    assert_eq!(app.active_bench_id.as_deref(), Some("bench-v3/JW/08"));

    app.handle_key_at(key(KeyCode::Esc), now + Duration::from_secs(18));
    app.handle_key_at(key(KeyCode::Esc), now + Duration::from_secs(19));
    app.handle_key_at(key(KeyCode::Char('b')), now + Duration::from_secs(20));
    app.handle_key_at(key(KeyCode::Char(' ')), now + Duration::from_secs(20));
    assert_eq!(app.active_bench_id.as_deref(), Some("bench-v3/W/01"));
}

#[test]
fn v3_records_are_not_benchmarks_or_practice_inputs_for_v2_analysis_but_remain_in_history() {
    let v2_earlier = record(23, "bench-v2/A/08", 35.0);
    let v2 = record(24, "bench-v2/A/01", 40.0);
    let v3 = record(25, "bench-v3/EW/01", 120.0);
    let trend = rs_typing_trainer::ui::analysis_image::build_trend_data(
        &[v2_earlier.clone(), v2.clone(), v3.clone()],
        rs_typing_trainer::ui::analysis_image::MetricKind::NetWpm,
        rs_typing_trainer::i18n::Lang::En,
    )
    .unwrap();
    let bench_points = trend
        .series
        .iter()
        .find(|series| series.name == "bench")
        .map(|series| series.points.iter().map(|point| point.value).collect::<Vec<_>>())
        .unwrap();
    assert_eq!(bench_points, [35.0, 40.0]);
    let growth = build_growth(
        &[v2_earlier, v2, v3],
        &Progress::default(),
        chrono::NaiveDate::from_ymd_opt(2026, 11, 26).unwrap(),
    );
    assert_eq!(growth.personal_bests.iter().map(|best| best.wpm).collect::<Vec<_>>(), [40.0, 35.0]);

    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 26).unwrap();
    let mut app = test_app_at(v3_config(), date);
    db::save(&app.db_path(), &record(26, "bench-v3/EW/04", 55.0)).unwrap();
    db::save(&app.db_path(), &record(26, "bench-v3/ES/04", 56.0)).unwrap();
    app.handle_key(key(KeyCode::Char('h')));
    let history = render(&app);
    assert!(history.contains("bench-v3"), "{history}");
    assert!(history.contains("EW English words"), "{history}");
    let parts = match &app.history[0] {
        rs_typing_trainer::app::HistoryEntry::BenchRun { parts, .. } => parts,
        entry => panic!("expected a v3 benchmark run, got {entry:?}"),
    };
    assert_eq!(
        parts.iter().filter_map(rs_typing_trainer::app::bench_part).collect::<Vec<_>>(),
        ["EW", "ES"]
    );
}

#[test]
fn v3_japanese_prompts_accept_common_romaji_spelling_variants() {
    let prompt = rs_typing_trainer::bench::BenchPrompt {
        text: "し".to_owned(),
        reading: Some("し".to_owned()),
        target: "shi".to_owned(),
    };
    for spelling in ["si", "shi"] {
        let mut item = prompt.clone().into_item();
        let mut result = JudgeResult::Mistype;
        for character in spelling.chars() {
            result = item.judge.input(&character.to_string());
        }
        assert_eq!(result, JudgeResult::ItemCompleted, "{spelling}");
    }
}
