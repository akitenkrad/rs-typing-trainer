use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::time::{Duration, Instant};

use chrono::{TimeZone, Utc};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::analysis::metrics::TransitionClass;
use rs_typing_trainer::app::Screen;
use rs_typing_trainer::bench::{BenchPart, BenchSettings, generate_parallel_versions, next_parallel_index};
use rs_typing_trainer::config::{Config, DataConfig, ErrorMode, ModeKind};
use rs_typing_trainer::data::DataCatalog;
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::layout::builtin_layout;
use rs_typing_trainer::layout::geometry::geometry;
use rs_typing_trainer::record::{SessionRecord, db};
use rs_typing_trainer::ui;
use tempfile::TempDir;

mod common;
use common::test_app;

fn catalog() -> (TempDir, DataCatalog) {
    let home = TempDir::new().unwrap();
    let data = DataCatalog::load(&DataConfig::default(), home.path()).unwrap();
    (home, data)
}

fn frequencies(version: &rs_typing_trainer::bench::BenchVersion) -> BTreeMap<char, usize> {
    let mut result = BTreeMap::new();
    for character in version.prompts.iter().flat_map(|prompt| prompt.target.chars()) {
        *result.entry(character).or_default() += 1;
    }
    result
}

fn l1(left: &BTreeMap<char, usize>, right: &BTreeMap<char, usize>) -> usize {
    left.keys()
        .chain(right.keys())
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|key| left.get(&key).copied().unwrap_or(0).abs_diff(right.get(&key).copied().unwrap_or(0)))
        .sum()
}

fn normalized_l1(left: &BTreeMap<char, usize>, right: &BTreeMap<char, usize>) -> f64 {
    let left_total = left.values().sum::<usize>() as f64;
    let right_total = right.values().sum::<usize>() as f64;
    left.keys()
        .chain(right.keys())
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|key| {
            let left_share = left.get(&key).copied().unwrap_or_default() as f64 / left_total;
            let right_share = right.get(&key).copied().unwrap_or_default() as f64 / right_total;
            (left_share - right_share).abs()
        })
        .sum()
}

fn transition_class_shares(version: &rs_typing_trainer::bench::BenchVersion) -> BTreeMap<TransitionClass, f64> {
    let total = version.transition_class_counts.values().sum::<usize>() as f64;
    version
        .transition_class_counts
        .iter()
        .map(|(class, count)| (*class, *count as f64 / total))
        .collect()
}

fn is_periodic(word: &str) -> bool {
    let characters = word.chars().collect::<Vec<_>>();
    (1..=3).any(|period| {
        characters.len() > period
            && characters.len().is_multiple_of(period)
            && characters
                .iter()
                .enumerate()
                .all(|(index, character)| *character == characters[index % period])
    })
}

fn word_bigrams(word: &str) -> Vec<(char, char)> {
    word.chars().collect::<Vec<_>>().windows(2).map(|pair| (pair[0], pair[1])).collect()
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn render(app: &rs_typing_trainer::app::App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
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

fn save_bench_record(path: &std::path::Path, bench_id: &str, wpm: f64, accuracy: f64, day: u32) {
    let mut config = Config::default();
    config.mode.kind = ModeKind::Words;
    let mut session = Session::new(Duration::from_secs(1), Duration::ZERO, Item::exact("as"));
    session.activate(Duration::ZERO);
    session.tick(Duration::from_secs(1));
    let mut record = SessionRecord::from_log(&config, session.into_log(), Utc.with_ymd_and_hms(2026, 11, day, 0, 0, 0).unwrap());
    record.header.mode = "bench".into();
    record.header.bench_id = Some(bench_id.into());
    record.header.net_wpm = wpm;
    record.header.accuracy = accuracy;
    db::save(path, &record).unwrap();
}

#[test]
fn eight_parallel_versions_are_deterministic_and_frequency_balanced() {
    let (_home, data) = catalog();
    let layout = builtin_layout("qwerty").unwrap();
    for part in [BenchPart::A, BenchPart::B, BenchPart::C, BenchPart::D] {
        let first = generate_parallel_versions(part, &data, &layout, &geometry("ansi").unwrap());
        let second = generate_parallel_versions(part, &data, &layout, &geometry("ansi").unwrap());
        assert_eq!(first, second);
        assert_eq!(first.len(), 8);
        match part {
            BenchPart::A => assert_eq!(first[0].prompts.len(), 200),
            BenchPart::C => assert_eq!(first[0].prompts.len(), 300),
            BenchPart::B | BenchPart::D => assert!(!first[0].prompts.is_empty()),
        }
        if part != BenchPart::B {
            let base = frequencies(&first[0]);
            let base_classes = &first[0].transition_class_counts;
            for version in &first[1..] {
                assert_eq!(l1(&base, &frequencies(version)), 0, "part {part:?}, {}", version.id);
                assert_eq!(base_classes, &version.transition_class_counts, "part {part:?}, {}", version.id);
            }
        }
    }
}

#[test]
fn part_b_versions_have_varied_well_covered_pseudowords() {
    let (_home, data) = catalog();
    let versions = generate_parallel_versions(BenchPart::B, &data, &builtin_layout("qwerty").unwrap(), &geometry("ansi").unwrap());
    for version in versions {
        assert!(version.prompts.len() >= 200, "{} is too short", version.id);
        let mut words = HashSet::new();
        let mut character_counts = BTreeMap::<char, usize>::new();
        let mut distinct_bigrams = HashSet::new();
        for prompt in &version.prompts {
            let word = &prompt.target;
            assert!((3..=6).contains(&word.chars().count()), "{}: {word}", version.id);
            assert!(!is_periodic(word), "{}: periodic word {word}", version.id);
            assert!(words.insert(word), "{}: duplicate word {word}", version.id);
            for character in word.chars() {
                *character_counts.entry(character).or_default() += 1;
            }
            let bigrams = word_bigrams(word);
            assert_eq!(
                bigrams.len(),
                bigrams.iter().copied().collect::<HashSet<_>>().len(),
                "{}: repeated bigram in {word}",
                version.id
            );
            distinct_bigrams.extend(bigrams);
        }
        for character in 'a'..='z' {
            assert!(
                character_counts.get(&character).copied().unwrap_or_default() >= 3,
                "{}: {character} appears fewer than three times",
                version.id
            );
        }
        for class in [
            TransitionClass::SameFinger,
            TransitionClass::SameHandAdjacent,
            TransitionClass::SameHandNonAdjacent,
            TransitionClass::AlternatingHands,
        ] {
            assert!(
                version.transition_class_counts.get(&class).copied().unwrap_or_default() >= 15,
                "{}: {class:?} appears fewer than 15 times",
                version.id
            );
        }
        assert!(
            distinct_bigrams.len() >= 120,
            "{}: only {} distinct bigrams",
            version.id,
            distinct_bigrams.len()
        );
    }
}

#[test]
fn part_b_parallel_versions_are_distinct_and_balanced() {
    let (_home, data) = catalog();
    let versions = generate_parallel_versions(BenchPart::B, &data, &builtin_layout("qwerty").unwrap(), &geometry("ansi").unwrap());
    for left_index in 0..versions.len() {
        for right_index in left_index + 1..versions.len() {
            let left = &versions[left_index];
            let right = &versions[right_index];
            let left_words = left.prompts.iter().map(|prompt| prompt.target.as_str()).collect::<HashSet<_>>();
            let right_words = right.prompts.iter().map(|prompt| prompt.target.as_str()).collect::<HashSet<_>>();
            let overlap = left_words.intersection(&right_words).count() as f64 / left_words.len().min(right_words.len()) as f64;
            assert!(overlap < 0.10, "{} and {} overlap by {overlap:.3}", left.id, right.id);

            let character_l1 = normalized_l1(&frequencies(left), &frequencies(right));
            assert!(character_l1 <= 0.05, "{} and {} character L1 = {character_l1:.4}", left.id, right.id);

            let left_classes = transition_class_shares(left);
            let right_classes = transition_class_shares(right);
            for class in [
                TransitionClass::SameFinger,
                TransitionClass::SameHandAdjacent,
                TransitionClass::SameHandNonAdjacent,
                TransitionClass::AlternatingHands,
            ] {
                let difference = (left_classes[&class] - right_classes[&class]).abs();
                assert!(difference <= 0.02, "{} and {} {class:?} difference = {difference:.4}", left.id, right.id);
            }
        }
    }
}

#[test]
fn benchmark_result_names_part_version_comparisons_and_next_part() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    save_bench_record(&path, "bench-v1/A/01", 40.0, 0.90, 1);
    save_bench_record(&path, "bench-v1/A/02", 50.0, 0.95, 2);
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    config.storage.db_path = Some(path.clone());
    let mut app = test_app(config.clone());
    app.handle_key(key(KeyCode::Char('b')));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.active_bench_id.as_deref(), Some("bench-v1/A/03"));
    let mut result = rs_typing_trainer::metrics::stats::GameResult::from_metrics(&config, Default::default(), Duration::from_secs(60));
    result.wpm = 60.0;
    result.accuracy = 0.98;
    app.bench_comparison = Some(db::bench_comparison(&path, "bench-v1/A/03", result.wpm, result.accuracy).unwrap());
    app.last_result = Some(result);
    app.screen = Screen::Result;

    let output = render(&app);
    let compact = output.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    assert!(compact.contains("パートA英単語"), "{output}");
    assert!(compact.contains("bench-v1/A/03"), "{output}");
    assert!(compact.contains("前回比"), "{output}");
    assert!(compact.contains("直近3回の中央値との差"), "{output}");
    assert!(compact.contains("[ENTER]次へ:パートB網羅"), "{output}");
    assert!(compact.contains("完了したパートは保存済み"), "{output}");
}

#[test]
fn completing_last_required_part_opens_benchmark_summary() {
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    let mut app = test_app(config);
    let started = Instant::now();
    app.handle_key_at(key(KeyCode::Char('b')), started);
    app.handle_key_at(key(KeyCode::Enter), started);

    for offset in 0..3 {
        let base = started + Duration::from_secs(offset * 70);
        app.on_tick_at_date(Duration::from_secs(3), base, chrono::NaiveDate::from_ymd_opt(2026, 11, 14).unwrap())
            .unwrap();
        app.on_tick_at(Duration::from_secs(60), base + Duration::from_secs(60)).unwrap();
        assert_eq!(app.screen, Screen::Result);
        app.handle_key_at(key(KeyCode::Enter), base + Duration::from_secs(61));
    }

    assert_ne!(app.screen, Screen::Title);
    let output = render(&app);
    let compact = output.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    assert!(compact.contains("ベンチマークまとめ"), "{output}");
    assert!(compact.contains("パートA"), "{output}");
    assert!(compact.contains("パートB"), "{output}");
    assert!(compact.contains("パートC"), "{output}");
    assert!(compact.contains("前回比"), "{output}");
    assert!(compact.contains("[ENTER]タイトルへ"), "{output}");
}

#[test]
fn escape_from_benchmark_result_returns_to_title_without_quitting_and_keeps_completed_part() {
    let mut app = test_app(Config::default());
    let started = Instant::now();
    app.handle_key_at(key(KeyCode::Char('b')), started);
    app.handle_key_at(key(KeyCode::Enter), started);
    app.on_tick_at_date(Duration::from_secs(3), started, chrono::NaiveDate::from_ymd_opt(2026, 11, 14).unwrap())
        .unwrap();
    app.on_tick_at(Duration::from_secs(60), started + Duration::from_secs(60)).unwrap();
    assert_eq!(app.screen, Screen::Result);

    app.handle_key_at(key(KeyCode::Esc), started + Duration::from_secs(61));

    assert_eq!(app.screen, Screen::Title);
    assert!(!app.should_quit);
    let records = db::load_records_filtered(&app.db_path(), None, Some("bench")).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].header.bench_id.as_deref(), Some("bench-v1/A/01"));
}

#[test]
fn versions_rotate_and_settings_ignore_user_configuration() {
    assert_eq!(next_parallel_index(None), 1);
    for current in 1..8 {
        assert_eq!(next_parallel_index(Some(current)), current + 1);
    }
    assert_eq!(next_parallel_index(Some(8)), 1);

    let mut config = Config::default();
    config.session.duration_seconds = 999;
    config.session.item_timeout_seconds = 7;
    config.session.error_mode = ErrorMode::Advance;
    config.session.preview_items = 4;
    let settings = BenchSettings::fixed(BenchPart::A, &config);
    assert_eq!(settings.duration_seconds, 60);
    assert_eq!(settings.item_timeout_seconds, 0);
    assert_eq!(settings.error_mode, ErrorMode::Stop);
    assert_eq!(settings.preview_items, 1);
}

#[test]
fn daily_rotation_inputs_cannot_change_benchmark_text() {
    let (_home, data) = catalog();
    let layout = builtin_layout("dvorak").unwrap();
    let geometry = geometry("ansi").unwrap();
    let baseline = generate_parallel_versions(BenchPart::C, &data, &layout, &geometry);

    let dates = ["2026-01-01", "2026-11-14"];
    let salts = [b"alice".as_slice(), b"bob".as_slice()];
    let exposure_counts = [0_u64, 10_000];
    let exclusions = [vec![], vec![("今日", "きょう")]];
    for (((_date, _salt), _exposure), _excluded) in dates.into_iter().zip(salts).zip(exposure_counts).zip(exclusions) {
        let regenerated = generate_parallel_versions(BenchPart::C, &data, &layout, &geometry);
        assert_eq!(regenerated, baseline);
    }
}

#[test]
fn title_starts_a_fixed_benchmark_session() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::time::{Duration, Instant};

    let mut config = Config::default();
    config.session.duration_seconds = 12;
    config.session.item_timeout_seconds = 3;
    config.session.error_mode = ErrorMode::Advance;
    config.session.preview_items = 4;
    let mut app = test_app(config);
    let now = Instant::now();
    app.handle_key_at(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE), now);
    assert_eq!(app.screen, Screen::BenchSelect);
    app.handle_key_at(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), now);
    assert!(matches!(app.screen, Screen::Countdown { .. }));
    assert_eq!(app.active_bench_id.as_deref(), Some("bench-v1/A/01"));
    assert_eq!(app.config.session.duration_seconds, 60);
    assert_eq!(app.config.session.item_timeout_seconds, 0);
    assert_eq!(app.config.session.error_mode, ErrorMode::Stop);
    assert_eq!(app.config.session.preview_items, 1);
    app.on_tick_at_date(Duration::from_secs(3), now, chrono::NaiveDate::from_ymd_opt(2026, 11, 14).unwrap())
        .unwrap();
    app.on_tick_at(Duration::from_millis(1), now).unwrap();
    assert_eq!(app.screen, Screen::Playing);
    app.on_tick_at(Duration::from_secs(60), now + Duration::from_secs(60)).unwrap();
    assert_eq!(app.screen, Screen::Result);
    let saved = rs_typing_trainer::record::db::load_record(&app.db_path(), app.last_save_id.unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(saved.header.mode, "bench");
    assert_eq!(saved.header.bench_id.as_deref(), Some("bench-v1/A/01"));
    assert_eq!(saved.header.error_mode, "stop");

    app.handle_key_at(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), now + Duration::from_secs(61));
    assert!(matches!(app.screen, Screen::Countdown { .. }));
    assert_eq!(app.active_bench_id.as_deref(), Some("bench-v1/B/01"));
}
