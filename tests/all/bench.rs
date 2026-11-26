use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::time::{Duration, Instant};

use chrono::{TimeZone, Utc};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::analysis::bench_summary::{
    BenchChange, BenchMetric, BenchmarkRunAnalysis, PracticeSuggestion, SlowTransition, WeakFinger, WeakFingerReason,
};
use rs_typing_trainer::analysis::grades::{Component, ComponentGrade, GradeSource, GradeStatus, GradesAnalysis, LanguageGrade, Level, OverallGrade};
use rs_typing_trainer::analysis::metrics::TransitionClass;
use rs_typing_trainer::app::Screen;
use rs_typing_trainer::bench::{
    BENCH_SERIES_V2, BenchComparison, BenchPart, BenchResultSummary, BenchSettings, V2_SELECTION_RULE, generate_parallel_versions,
    generate_parallel_versions_for_series, next_parallel_index,
};
use rs_typing_trainer::config::{Config, DataConfig, ErrorMode, ModeKind};
use rs_typing_trainer::data::DataCatalog;
use rs_typing_trainer::engine::item::Item;
use rs_typing_trainer::engine::session::Session;
use rs_typing_trainer::graphics::{CellPixels, GraphicsDetection, GraphicsMode, GraphicsReason};
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::layout::builtin_layout;
use rs_typing_trainer::layout::geometry::geometry;
use rs_typing_trainer::record::{SessionRecord, db};
use rs_typing_trainer::ui;
use tempfile::TempDir;

use crate::common;
use common::{test_app, test_app_at};

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
    render_size(app, 100, 24)
}

fn render_size(app: &rs_typing_trainer::app::App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
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

fn summary_app(lang: Lang, graphics: GraphicsMode) -> common::TestApp {
    let mut config = Config::default();
    config.general.lang = lang;
    let mut app = test_app(config);
    app.screen = Screen::BenchSummary;
    app.bench_results = [BenchPart::A, BenchPart::B, BenchPart::C]
        .into_iter()
        .enumerate()
        .map(|(index, part)| BenchResultSummary {
            id: format!("bench-v2/{}/{:02}", part.as_str(), index + 1),
            part,
            wpm: 50.0 + index as f64 * 5.0,
            accuracy: 0.94 + index as f64 * 0.01,
            comparison: BenchComparison {
                previous_wpm_delta: Some(if part == BenchPart::B { -2.0 } else { 3.0 }),
                previous_accuracy_delta: Some(0.01),
                best_wpm_delta: Some(if part == BenchPart::B { -4.0 } else { 1.0 }),
                best_accuracy_delta: Some(-0.005),
                recent_three_wpm_delta: None,
                recent_three_accuracy_delta: None,
            },
        })
        .collect();
    app.bench_summary_analysis = BenchmarkRunAnalysis {
        slow_transitions: vec![SlowTransition {
            from: "t".into(),
            to: "h".into(),
            milliseconds: 312.0,
            n: 8,
        }],
        transition_counts_insufficient: false,
        weak_fingers: vec![WeakFinger {
            name: "left-index".into(),
            reason: WeakFingerReason::Iki,
            error_rate: 0.02,
            iki_median_ms: 220.0,
        }],
        changes: vec![BenchChange {
            part: BenchPart::B,
            metric: BenchMetric::Wpm,
            delta: 5.0,
        }],
        practice: vec![PracticeSuggestion::Transition {
            from: "t".into(),
            to: "h".into(),
        }],
        mistyped_keys: BTreeMap::from([("f".into(), 3)]),
    };
    app.set_graphics_detection(GraphicsDetection {
        mode: graphics,
        cell_pixels: (graphics != GraphicsMode::Off).then(|| CellPixels::new(9, 20)),
        reason: if graphics == GraphicsMode::Off {
            GraphicsReason::Disabled
        } else {
            GraphicsReason::Explicit(graphics)
        },
    });
    app
}

fn save_bench_record(path: &std::path::Path, bench_id: &str, wpm: f64, accuracy: f64, day: u32) {
    save_bench_record_with_mode(path, bench_id, "bench", wpm, accuracy, day);
}

fn save_bench_record_on_local_date(path: &std::path::Path, bench_id: &str, wpm: f64, accuracy: f64, date: chrono::NaiveDate) {
    let mut config = Config::default();
    config.mode.kind = ModeKind::Words;
    let mut session = Session::new(Duration::from_secs(1), Duration::ZERO, Item::exact("as"));
    session.activate(Duration::ZERO);
    session.tick(Duration::from_secs(1));
    let finished = date
        .and_hms_opt(0, 0, 1)
        .unwrap()
        .and_local_timezone(chrono::Local)
        .earliest()
        .unwrap()
        .with_timezone(&Utc);
    let mut record = SessionRecord::from_log(&config, session.into_log(), finished);
    record.header.mode = "bench".into();
    record.header.bench_id = Some(bench_id.into());
    record.header.net_wpm = wpm;
    record.header.accuracy = accuracy;
    db::save(path, &record).unwrap();
}

fn save_bench_record_with_mode(path: &std::path::Path, bench_id: &str, mode: &str, wpm: f64, accuracy: f64, day: u32) {
    let mut config = Config::default();
    config.mode.kind = ModeKind::Words;
    let mut session = Session::new(Duration::from_secs(1), Duration::ZERO, Item::exact("as"));
    session.activate(Duration::ZERO);
    session.tick(Duration::from_secs(1));
    let mut record = SessionRecord::from_log(&config, session.into_log(), Utc.with_ymd_and_hms(2026, 11, day, 0, 0, 0).unwrap());
    record.header.mode = mode.into();
    record.header.bench_id = Some(bench_id.into());
    record.header.net_wpm = wpm;
    record.header.accuracy = accuracy;
    db::save(path, &record).unwrap();
}

fn language_grade(language: &str, level: Level) -> LanguageGrade {
    LanguageGrade {
        language: language.into(),
        source: GradeSource::Bench,
        status: GradeStatus::Interim,
        n_sessions: 1,
        components: Vec::new(),
        overall: Some(OverallGrade {
            grade: level,
            score: 1.0,
            easiest_component: None,
        }),
        missing: Vec::new(),
        version: "test".into(),
    }
}

fn component_grade(component: Component, level: Level) -> ComponentGrade {
    ComponentGrade {
        component,
        value: 1.0,
        unit: String::new(),
        grade: level,
        score: 1.0,
        next_grade: None,
        next_threshold: None,
        gap: None,
        weight: 1.0,
    }
}

fn compact_render(app: &rs_typing_trainer::app::App) -> String {
    render(app).chars().filter(|character| !character.is_whitespace()).collect()
}

fn bench_prompt_fingerprint(versions: &[rs_typing_trainer::bench::BenchVersion]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for version in versions {
        for value in std::iter::once(version.id.as_str()).chain(
            version
                .prompts
                .iter()
                .flat_map(|prompt| [prompt.text.as_str(), prompt.reading.as_deref().unwrap_or(""), prompt.target.as_str()]),
        ) {
            for byte in value.bytes().chain(std::iter::once(0xff)) {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    hash
}

#[test]
fn every_bench_v1_prompt_stays_unchanged() {
    let (_home, data) = catalog();
    let layout = builtin_layout("qwerty").unwrap();
    let geometry = geometry("ansi").unwrap();
    let actual = [BenchPart::A, BenchPart::B, BenchPart::C, BenchPart::D]
        .map(|part| bench_prompt_fingerprint(&generate_parallel_versions(part, &data, &layout, &geometry)));

    assert_eq!(
        actual,
        [0x97eb_602d_8eb3_5b39, 0x4995_ff3b_e248_0943, 0x45ae_2abf_9076_c73f, 0xccb5_5a6d_9abc_e4b5,],
        "bench-v1 fingerprints: {actual:#018x?}"
    );
}

fn prompt_length(part: BenchPart, prompt: &rs_typing_trainer::bench::BenchPrompt) -> usize {
    match part {
        BenchPart::C => prompt.reading.as_deref().unwrap().chars().count(),
        _ => prompt.text.chars().count(),
    }
}

#[test]
fn bench_v2_a_and_c_are_staged_and_keep_each_stage_multiset_across_forms() {
    let (_home, data) = catalog();
    let layout = builtin_layout("qwerty").unwrap();
    let keyboard = geometry("ansi").unwrap();
    for (part, ranges, counts) in [
        (BenchPart::A, [(3, 5), (6, 8), (10, 15), (16, 22), (23, 29)], [15, 15, 12, 12, 12]),
        (BenchPart::C, [(2, 4), (5, 8), (6, 10), (11, 15), (16, 20)], [15, 15, 12, 12, 12]),
    ] {
        let versions = generate_parallel_versions_for_series(BENCH_SERIES_V2, part, &data, &layout, &keyboard);
        assert_eq!(versions.len(), 8);
        let mut offset = 0;
        for ((minimum, maximum), count) in ranges.into_iter().zip(counts) {
            let expected = versions[0].prompts[offset..offset + count]
                .iter()
                .map(|prompt| (prompt.text.clone(), prompt.reading.clone()))
                .collect::<BTreeSet<_>>();
            for version in &versions {
                let stage = &version.prompts[offset..offset + count];
                assert!(stage.iter().all(|prompt| (minimum..=maximum).contains(&prompt_length(part, prompt))));
                assert_eq!(
                    stage
                        .iter()
                        .map(|prompt| (prompt.text.clone(), prompt.reading.clone()))
                        .collect::<BTreeSet<_>>(),
                    expected,
                    "{part:?} stage {minimum}-{maximum} changed in {}",
                    version.id
                );
            }
            assert!(
                versions[1..]
                    .iter()
                    .any(|version| version.prompts[offset..offset + count] != versions[0].prompts[offset..offset + count]),
                "{part:?} stage {minimum}-{maximum} never changes order"
            );
            offset += count;
        }
        assert_eq!(offset, versions[0].prompts.len());
    }
}

#[test]
fn bench_v2_phrase_selection_rule_and_first_rows_are_fixed() {
    let (_home, data) = catalog();
    let versions = [BenchPart::A, BenchPart::C].map(|part| {
        generate_parallel_versions_for_series(
            BENCH_SERIES_V2,
            part,
            &data,
            &builtin_layout("qwerty").unwrap(),
            &geometry("ansi").unwrap(),
        )
    });
    assert_eq!(V2_SELECTION_RULE, "sha256-v1");
    let english = &versions[0][0].prompts;
    let japanese = &versions[1][0].prompts;
    for (stage_start, expected_rows) in [
        (30, ["Change my mind.", "Cool story.", "Oh, perfect!"]),
        (42, ["Tom is a wanted man.", "Give me my book back.", "What time did you say?"]),
        (
            54,
            ["A lot of people do do that.", "Just take a seat right here.", "Why had it taken so long?"],
        ),
    ] {
        for expected in expected_rows {
            assert!(
                english[stage_start..stage_start + 12].iter().any(|prompt| prompt.text == expected),
                "missing {expected:?}"
            );
        }
    }
    for (stage_start, expected_rows) in [
        (
            30,
            [
                ("深夜の空港", "しんやのくうこう"),
                ("なんだか眠い", "なんだかねむい"),
                ("真っ白な手帳", "まっしろなてちょう"),
            ],
        ),
        (
            42,
            [
                ("ボウリングでストライク", "ぼうりんぐですとらいく"),
                ("香ばしい焼きおにぎり", "こうばしいやきおにぎり"),
                ("満員電車に揺られる", "まんいんでんしゃにゆられる"),
            ],
        ),
        (
            54,
            [
                ("ロープウェイで山頂へ向かう", "ろーぷうぇいでさんちょうへむかう"),
                ("一緒に買い物に行ってくれる？", "いっしょにかいものにいってくれる？"),
                ("秋のキャンパスの並木道を歩く", "あきのきゃんぱすのなみきみちをあるく"),
            ],
        ),
    ] {
        for (text, reading) in expected_rows {
            assert!(
                japanese[stage_start..stage_start + 12]
                    .iter()
                    .any(|prompt| prompt.text == text && prompt.reading.as_deref() == Some(reading)),
                "missing {text:?}"
            );
        }
    }
}

#[test]
fn bench_v2_phrase_stages_have_varied_openings() {
    let (_home, data) = catalog();
    let layout = builtin_layout("qwerty").unwrap();
    let keyboard = geometry("ansi").unwrap();
    for (part, stage_starts) in [(BenchPart::A, [30, 42, 54]), (BenchPart::C, [30, 42, 54])] {
        let version = &generate_parallel_versions_for_series(BENCH_SERIES_V2, part, &data, &layout, &keyboard)[0];
        for stage_start in stage_starts {
            let stage = &version.prompts[stage_start..stage_start + 12];
            let opening_counts = stage.iter().fold(BTreeMap::<char, usize>::new(), |mut counts, prompt| {
                let source = if part == BenchPart::C {
                    prompt.reading.as_deref().unwrap()
                } else {
                    prompt.text.as_str()
                };
                *counts.entry(source.chars().next().unwrap()).or_default() += 1;
                counts
            });
            assert!(
                opening_counts.len() >= 5,
                "{} stage starting at {stage_start} has biased openings: {opening_counts:?}",
                version.id
            );
            assert!(
                opening_counts.values().all(|count| *count <= stage.len() / 2),
                "{} stage starting at {stage_start} has a majority opening: {opening_counts:?}",
                version.id
            );
            if part == BenchPart::A {
                let first_word_counts = stage.iter().fold(BTreeMap::<String, usize>::new(), |mut counts, prompt| {
                    let first_word = prompt
                        .text
                        .split(|character: char| !character.is_ascii_alphabetic())
                        .find(|word| !word.is_empty())
                        .unwrap();
                    *counts.entry(first_word.to_ascii_lowercase()).or_default() += 1;
                    counts
                });
                assert!(
                    first_word_counts.values().all(|count| *count <= stage.len() / 2),
                    "{} stage starting at {stage_start} has a majority first word: {first_word_counts:?}",
                    version.id
                );
            }
        }
    }
}

#[test]
fn bench_v2_has_more_than_one_minute_of_targets_at_150_wpm() {
    let (_home, data) = catalog();
    for part in [BenchPart::A, BenchPart::C] {
        let version = &generate_parallel_versions_for_series(
            BENCH_SERIES_V2,
            part,
            &data,
            &builtin_layout("qwerty").unwrap(),
            &geometry("ansi").unwrap(),
        )[0];
        let keystrokes = version.prompts.iter().map(|prompt| prompt.target.chars().count()).sum::<usize>();
        assert!(keystrokes > 750, "{} has only {keystrokes} target characters", version.id);
    }
}

#[test]
fn bench_v2_b_and_d_reuse_v1_prompts_but_have_v2_ids() {
    let (_home, data) = catalog();
    let layout = builtin_layout("qwerty").unwrap();
    let keyboard = geometry("ansi").unwrap();
    for part in [BenchPart::B, BenchPart::D] {
        let v1 = generate_parallel_versions(part, &data, &layout, &keyboard);
        let v2 = generate_parallel_versions_for_series(BENCH_SERIES_V2, part, &data, &layout, &keyboard);
        for (index, (left, right)) in v1.iter().zip(&v2).enumerate() {
            assert_eq!(left.prompts, right.prompts);
            assert_eq!(right.id, format!("bench-v2/{}/{:02}", part.as_str(), index + 1));
        }
    }
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
    save_bench_record(&path, "bench-v2/A/01", 40.0, 0.90, 1);
    save_bench_record(&path, "bench-v2/A/02", 50.0, 0.95, 2);
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    config.storage.db_path = Some(path.clone());
    let mut app = test_app(config.clone());
    app.handle_key(key(KeyCode::Char('b')));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.active_bench_id.as_deref(), Some("bench-v2/A/03"));
    let mut result = rs_typing_trainer::metrics::stats::GameResult::from_metrics(&config, Default::default(), Duration::from_secs(60));
    result.wpm = 60.0;
    result.accuracy = 0.98;
    app.bench_comparison = Some(db::bench_comparison(&path, "bench-v2/A/03", result.wpm, result.accuracy).unwrap());
    app.last_result = Some(result);
    app.screen = Screen::Result;

    let output = render(&app);
    let compact = output.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    assert!(compact.contains("パートA英単語と句"), "{output}");
    assert!(compact.contains("bench-v2/A/03"), "{output}");
    assert!(compact.contains("前回比"), "{output}");
    assert!(compact.contains("直近3回の中央値との差"), "{output}");
    assert!(compact.contains("[ENTER]次へ:パートB網羅"), "{output}");
    assert!(compact.contains("完了したパートは保存済み"), "{output}");
}

#[test]
fn benchmark_comparison_crosses_versions_only_for_unchanged_parts() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    for part in ['A', 'B', 'C', 'D'] {
        save_bench_record(&path, &format!("bench-v1/{part}/01"), 40.0, 0.90, 1);
    }

    for part in ['B', 'D'] {
        let comparison = db::bench_comparison(&path, &format!("bench-v2/{part}/01"), 52.0, 0.94).unwrap();
        assert_eq!(comparison.previous_wpm_delta, Some(12.0), "part {part}");
        assert!((comparison.previous_accuracy_delta.unwrap() - 0.04).abs() < 1e-12, "part {part}");
        assert_eq!(comparison.best_wpm_delta, Some(12.0), "part {part}");
        assert!((comparison.best_accuracy_delta.unwrap() - 0.04).abs() < 1e-12, "part {part}");
    }
    for part in ['A', 'C'] {
        let comparison = db::bench_comparison(&path, &format!("bench-v2/{part}/01"), 52.0, 0.94).unwrap();
        assert_eq!(comparison.previous_wpm_delta, None, "part {part}");
        assert_eq!(comparison.previous_accuracy_delta, None, "part {part}");
        assert_eq!(comparison.best_wpm_delta, None, "part {part}");
        assert_eq!(comparison.best_accuracy_delta, None, "part {part}");
    }
}

#[test]
fn unpadded_v1_b_with_sentence_mode_is_used_for_comparison_and_trend_history() {
    assert_eq!(
        rs_typing_trainer::bench::parse_bench_id("bench-v1/B/1"),
        Some((rs_typing_trainer::bench::BENCH_SERIES_V1, BenchPart::B, 1))
    );

    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    save_bench_record_with_mode(&path, "bench-v1/B/1", "sentence", 41.0, 0.91, 1);
    save_bench_record(&path, "bench-v2/B/01", 53.0, 0.95, 2);

    let rows = db::bench_results_for_day_form(&path, BENCH_SERIES_V2, chrono::NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(), 1).unwrap();
    let b = rows.iter().find(|row| row.part == BenchPart::B).unwrap();
    assert_eq!(b.comparison.previous_wpm_delta, Some(12.0));
    assert!((b.comparison.previous_accuracy_delta.unwrap() - 0.04).abs() < 1e-12);

    let records = db::load_records_filtered(&path, None, Some("bench")).unwrap();
    assert_eq!(records.len(), 2, "legacy v1 record was not classified by bench_id");
    let mut app = summary_app(Lang::En, GraphicsMode::Kitty);
    app.bench_results.retain(|result| result.part == BenchPart::B);
    app.bench_summary_current = vec![records[1].clone()];
    app.bench_summary_records = records;
    let output = render_size(&app, 160, 50);
    assert!(!output.contains("B  Initial"), "v1 point is missing from the trend:\n{output}");
}

#[test]
fn latest_benchmark_form_ignores_unparseable_bench_ids() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    save_bench_record_with_mode(&path, "bench-v2/A/1", "words", 41.0, 0.91, 1);
    save_bench_record_with_mode(&path, "bench-v2/X/8", "bench", 53.0, 0.95, 2);

    assert_eq!(db::latest_bench_parallel(&path, BENCH_SERIES_V2).unwrap(), Some(1));
}

#[test]
fn benchmark_summary_comparison_uses_v1_for_previous_b_but_not_a() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    save_bench_record(&path, "bench-v1/A/01", 40.0, 0.90, 1);
    save_bench_record(&path, "bench-v1/B/01", 41.0, 0.91, 1);
    save_bench_record(&path, "bench-v2/A/01", 52.0, 0.94, 2);
    save_bench_record(&path, "bench-v2/B/01", 53.0, 0.95, 2);

    let rows = db::bench_results_for_day_form(&path, BENCH_SERIES_V2, chrono::NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(), 1).unwrap();
    let a = rows.iter().find(|row| row.part == BenchPart::A).unwrap();
    let b = rows.iter().find(|row| row.part == BenchPart::B).unwrap();
    assert_eq!(a.comparison.previous_wpm_delta, None);
    assert_eq!(b.comparison.previous_wpm_delta, Some(12.0));
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
        app.handle_key_at(key(KeyCode::Enter), base + Duration::from_secs(62));
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
fn benchmark_summary_renders_without_overlap_in_both_languages_and_sizes() {
    for lang in [Lang::En, Lang::Ja] {
        for graphics in [GraphicsMode::Kitty, GraphicsMode::Off] {
            for (width, height) in [(150, 40), (80, 24)] {
                let output = render_size(&summary_app(lang, graphics), width, height);
                let compact = output.chars().filter(|character| !character.is_whitespace()).collect::<String>();
                let part_label = if graphics != GraphicsMode::Off && width == 150 {
                    "A"
                } else {
                    match lang {
                        Lang::En => "PartA",
                        Lang::Ja => "パートA",
                    }
                };
                let expected = match lang {
                    Lang::En => [
                        "BENCHMARKSUMMARY",
                        part_label,
                        "previous",
                        "Slowtransitions",
                        "Practicenext",
                        "[ENTER]Backtotitle",
                    ],
                    Lang::Ja => [
                        "ベンチマークまとめ",
                        part_label,
                        "前回",
                        "遅いキーのつなぎ",
                        "次に練習すること",
                        "[ENTER]タイトルへ",
                    ],
                };
                for label in expected {
                    assert!(
                        compact.contains(label),
                        "{lang:?} {graphics:?} {width}x{height}: missing {label:?}\n{output}"
                    );
                }
                let hint = match lang {
                    Lang::En => "[ENTER]Backtotitle",
                    Lang::Ja => "[ENTER]タイトルへ",
                };
                assert_eq!(compact.matches(hint).count(), 1, "{lang:?} {graphics:?} {width}x{height}:\n{output}");
            }
        }
    }
}

#[test]
fn benchmark_summary_lists_english_and_japanese_grades_and_missing_reason() {
    let mut app = summary_app(Lang::Ja, GraphicsMode::Off);
    app.bench_summary_grades = GradesAnalysis {
        languages: vec![language_grade("en", Level::G), language_grade("ja", Level::F)],
        ..GradesAnalysis::default()
    };
    let output = render_size(&app, 100, 24);
    let compact = output.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    assert!(compact.contains("英語G"), "{output}");
    assert!(compact.contains("日本語F"), "{output}");

    app.bench_summary_grades.languages.retain(|grade| grade.language == "en");
    let output = render_size(&app, 100, 24);
    let compact = output.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    assert!(compact.contains("日本語:ベンチCの記録が足りません"), "{output}");
}

#[test]
fn rich_benchmark_summary_shows_every_available_grade_component() {
    let mut app = summary_app(Lang::Ja, GraphicsMode::Kitty);
    app.bench_summary_grades = GradesAnalysis {
        languages: ["en", "ja"]
            .into_iter()
            .map(|language| {
                let mut grade = language_grade(language, Level::G);
                grade.components = Component::all()
                    .iter()
                    .copied()
                    .map(|component| component_grade(component, Level::G))
                    .collect();
                grade
            })
            .collect(),
        ..GradesAnalysis::default()
    };
    let output = render_size(&app, 160, 50);
    let compact = output.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    for expected in [
        "英語G",
        "日本語G",
        "速さG",
        "正確さG",
        "リズムG",
        "つなぎG",
        "先読みG",
        "立ち直りG",
        "重なりG",
    ] {
        assert!(compact.contains(expected), "missing {expected:?}:\n{output}");
    }
    for expected in ["速さG", "正確さG", "リズムG", "つなぎG", "先読みG", "立ち直りG", "重なりG"] {
        assert_eq!(compact.matches(expected).count(), 2, "both languages must show {expected:?}:\n{output}");
    }
}

#[test]
fn benchmark_summary_names_each_weak_finger_metric_in_both_languages() {
    for (lang, expected) in [
        (Lang::Ja, ["左中指誤打率99.8%", "右中指打鍵間隔119ms"]),
        (Lang::En, ["leftmiddleerrorrate99.8%", "rightmiddleinterval119ms"]),
    ] {
        let mut app = summary_app(lang, GraphicsMode::Off);
        app.bench_summary_analysis.weak_fingers = vec![
            WeakFinger {
                name: "left-middle".into(),
                reason: WeakFingerReason::ErrorRate,
                error_rate: 0.998,
                iki_median_ms: 100.0,
            },
            WeakFinger {
                name: "right-middle".into(),
                reason: WeakFingerReason::Iki,
                error_rate: 0.01,
                iki_median_ms: 119.0,
            },
        ];
        let output = render_size(&app, 100, 30);
        let compact = output.chars().filter(|character| !character.is_whitespace()).collect::<String>();
        for label in expected {
            assert!(compact.contains(label), "{lang:?}: missing {label:?}\n{output}");
        }
    }
}

#[test]
fn benchmark_summary_explains_sparse_transitions_and_styles_analysis_text() {
    let mut app = summary_app(Lang::En, GraphicsMode::Off);
    app.bench_summary_analysis.slow_transitions.clear();
    app.bench_summary_analysis.transition_counts_insufficient = true;
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    let rows = terminal
        .backend()
        .buffer()
        .content()
        .chunks(100)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>();
    let sparse_row = rows
        .iter()
        .position(|row| row.contains("Transitions: not enough observations"))
        .expect("sparse-transition reason");
    let heading_row = rows.iter().position(|row| row.contains("This run's analysis")).expect("analysis heading");
    let body_x = rows[sparse_row].find("Transitions: not enough observations").unwrap() as u16;
    let body_cell = terminal.backend().buffer().cell((body_x, sparse_row as u16)).unwrap();
    let heading_x = rows[heading_row].find("This run's analysis").unwrap() as u16;
    let heading_cell = terminal.backend().buffer().cell((heading_x, heading_row as u16)).unwrap();
    assert_eq!(body_cell.fg, ratatui::style::Color::White);
    assert_eq!(heading_cell.fg, ratatui::style::Color::LightCyan);
    assert!(heading_cell.modifier.contains(ratatui::style::Modifier::BOLD));
}

#[test]
fn rich_benchmark_summary_places_keyboard_at_top_and_reports_hidden_keys() {
    let mut app = summary_app(Lang::Ja, GraphicsMode::Kitty);
    app.bench_summary_analysis.mistyped_keys = BTreeMap::from([("a".into(), 5), ("s".into(), 4), ("d".into(), 3), ("f".into(), 2), ("j".into(), 1)]);
    let output = render_size(&app, 160, 50);
    let compact = output.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    assert!(compact.contains("ほか2件"), "{output}");
}

#[test]
fn rich_benchmark_summary_uses_vertical_space_for_trends_without_starving_analysis() {
    for (width, height, minimum_trend_height) in [(160, 50, 16_usize), (150, 40, 14_usize)] {
        let app = summary_app(Lang::En, GraphicsMode::Kitty);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| ui::render(frame, &app)).unwrap();
        let rows = terminal
            .backend()
            .buffer()
            .content()
            .chunks(usize::from(width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>();
        let trend_row = rows.iter().position(|row| row.contains("Past trends")).expect("trend heading");
        let analysis_row = rows.iter().position(|row| row.contains("This run's analysis")).expect("analysis heading");
        let hint_row = rows.iter().position(|row| row.contains("[ENTER] Back to title")).expect("summary hint");
        assert!(
            analysis_row - trend_row >= minimum_trend_height,
            "{width}x{height}: trend section has only {} rows",
            analysis_row - trend_row
        );
        assert!(
            hint_row - analysis_row >= 10,
            "{width}x{height}: analysis section has only {} rows",
            hint_row - analysis_row
        );
    }
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

    app.handle_key_at(key(KeyCode::Esc), started + Duration::from_millis(61_500));
    assert_eq!(app.screen, Screen::Result);
    app.handle_key_at(key(KeyCode::Esc), started + Duration::from_millis(61_600));

    assert_eq!(app.screen, Screen::Title);
    assert!(!app.should_quit);
    let records = db::load_records_filtered(&app.db_path(), None, Some("bench")).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].header.bench_id.as_deref(), Some("bench-v2/A/01"));
}

#[test]
fn benchmark_result_disables_history_and_quit_and_requires_confirmed_escape() {
    let mut config = Config::default();
    config.session.duration_seconds = 17;
    config.session.item_timeout_seconds = 4;
    config.session.error_mode = ErrorMode::Advance;
    config.session.preview_items = 3;
    config.mode.kind = ModeKind::Sentence;
    let original_mode = config.mode.kind;
    let mut app = test_app(config);
    let started = Instant::now();
    app.handle_key_at(key(KeyCode::Char('b')), started);
    app.handle_key_at(key(KeyCode::Enter), started);
    app.on_tick_at_date(Duration::from_secs(3), started, chrono::NaiveDate::from_ymd_opt(2026, 11, 14).unwrap())
        .unwrap();
    app.on_tick_at(Duration::from_secs(60), started + Duration::from_secs(60)).unwrap();
    assert_eq!(app.screen, Screen::Result);

    app.handle_key_at(key(KeyCode::Char('h')), started + Duration::from_millis(61_500));
    app.handle_key_at(key(KeyCode::Char('q')), started + Duration::from_millis(61_600));
    assert_eq!(app.screen, Screen::Result);
    assert!(!app.should_quit);

    app.handle_key_at(key(KeyCode::Esc), started + Duration::from_millis(61_700));
    assert_eq!(app.screen, Screen::Result);
    assert!(render(&app).contains("Press Esc again to quit"));
    app.handle_key_at(key(KeyCode::Esc), started + Duration::from_millis(61_800));

    assert_eq!(app.screen, Screen::Title);
    assert_eq!(app.config.session.duration_seconds, 17);
    assert_eq!(app.config.session.item_timeout_seconds, 4);
    assert_eq!(app.config.session.error_mode, ErrorMode::Advance);
    assert_eq!(app.config.session.preview_items, 3);
    assert_eq!(app.config.mode.kind, original_mode);
    assert!(app.active_bench_id.is_none());
}

#[test]
fn benchmark_summary_ignores_enter_and_escape_during_result_delay() {
    let mut config = Config::default();
    config.session.duration_seconds = 17;
    config.mode.kind = ModeKind::Sentence;
    let mut app = test_app(config);
    let started = Instant::now();
    app.handle_key_at(key(KeyCode::Char('b')), started);
    app.handle_key_at(key(KeyCode::Enter), started);

    let mut summary_entered_at = started;
    for offset in 0..3 {
        let base = started + Duration::from_secs(offset * 70);
        app.on_tick_at_date(Duration::from_secs(3), base, chrono::NaiveDate::from_ymd_opt(2026, 11, 14).unwrap())
            .unwrap();
        app.on_tick_at(Duration::from_secs(60), base + Duration::from_secs(60)).unwrap();
        summary_entered_at = base + Duration::from_secs(62);
        app.handle_key_at(key(KeyCode::Enter), summary_entered_at);
    }
    assert_eq!(app.screen, Screen::BenchSummary);
    assert_eq!(app.config.session.duration_seconds, 17);
    assert_eq!(app.config.mode.kind, ModeKind::Sentence);
    assert!(app.active_bench_id.is_none());

    app.handle_key_at(key(KeyCode::Enter), summary_entered_at);
    app.handle_key_at(key(KeyCode::Esc), summary_entered_at + Duration::from_millis(100));
    assert_eq!(app.screen, Screen::BenchSummary);
}

#[test]
fn benchmark_selection_offers_resume_from_first_missing_part_of_latest_form_today() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    save_bench_record(&path, "bench-v2/A/03", 40.0, 0.90, 14);
    let mut config = Config::default();
    config.general.lang = Lang::Ja;
    config.storage.db_path = Some(path);
    let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
    let mut app = test_app_at(config, today);

    app.handle_key(key(KeyCode::Char('b')));
    let output = compact_render(&app);
    assert!(output.contains("パートBから続ける（版03）"), "{output}");

    app.handle_key(key(KeyCode::Enter));
    assert!(matches!(app.screen, Screen::Countdown { .. }));
    assert_eq!(app.active_bench_id.as_deref(), Some("bench-v2/B/03"));
}

#[test]
fn benchmark_selection_does_not_offer_resume_for_complete_or_old_forms() {
    let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();

    let complete_directory = TempDir::new().unwrap();
    let complete_path = complete_directory.path().join("sessions.db");
    for part in ['A', 'B', 'C'] {
        save_bench_record(&complete_path, &format!("bench-v2/{part}/03"), 40.0, 0.90, 14);
    }
    let mut complete_config = Config::default();
    complete_config.storage.db_path = Some(complete_path);
    let mut complete = test_app_at(complete_config, today);
    complete.handle_key(key(KeyCode::Char('b')));
    assert!(!render(&complete).contains("Resume from part"));

    let old_directory = TempDir::new().unwrap();
    let old_path = old_directory.path().join("sessions.db");
    save_bench_record(&old_path, "bench-v1/A/03", 40.0, 0.90, 13);
    let mut old_config = Config::default();
    old_config.storage.db_path = Some(old_path);
    let mut old = test_app_at(old_config, today);
    old.handle_key(key(KeyCode::Char('b')));
    assert!(!render(&old).contains("Resume from part"));
}

#[test]
fn resumed_benchmark_summary_includes_parts_completed_earlier_today() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("sessions.db");
    // Parts B and C are saved with the wall clock, so part A must be on the same local day.
    let today = chrono::Local::now().date_naive();
    save_bench_record_on_local_date(&path, "bench-v2/A/03", 40.0, 0.90, today);
    let mut config = Config::default();
    config.storage.db_path = Some(path);
    let mut app = test_app_at(config, today);
    let started = Instant::now();
    app.handle_key_at(key(KeyCode::Char('b')), started);
    app.handle_key_at(key(KeyCode::Enter), started);
    assert_eq!(app.active_bench_id.as_deref(), Some("bench-v2/B/03"));

    for offset in 0..2 {
        let base = started + Duration::from_secs(offset * 70);
        app.on_tick_at_date(Duration::from_secs(3), base, today).unwrap();
        app.on_tick_at_date(Duration::from_secs(60), base + Duration::from_secs(60), today)
            .unwrap();
        app.handle_key_at(key(KeyCode::Enter), base + Duration::from_secs(62));
    }
    assert_eq!(app.screen, Screen::BenchSummary);
    let output = render(&app);
    assert!(output.contains("Part A English words and phrases"), "{output}");
    assert!(output.contains("Part B Coverage"), "{output}");
    assert!(output.contains("Part C Japanese words and phrases"), "{output}");
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
    assert_eq!(app.active_bench_id.as_deref(), Some("bench-v2/A/01"));
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
    assert_eq!(saved.header.bench_id.as_deref(), Some("bench-v2/A/01"));
    assert_eq!(saved.header.error_mode, "stop");

    app.handle_key_at(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), now + Duration::from_secs(62));
    assert!(matches!(app.screen, Screen::Countdown { .. }));
    assert_eq!(app.active_bench_id.as_deref(), Some("bench-v2/B/01"));
}

/// MYTASK-3665: part A is English even when the practice language is
/// Japanese; the saved record says so.
#[test]
fn a_benchmark_part_records_its_own_language_not_the_practice_language() {
    let mut config = Config::default();
    config.mode.sentence.lang = "ja".to_owned();
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 20).unwrap();
    let mut app = test_app_at(config, date);
    let started = Instant::now();
    app.handle_key_at(key(KeyCode::Char('b')), started);
    app.handle_key_at(key(KeyCode::Enter), started);
    common::accept_briefing(&mut app, started);
    assert!(matches!(app.screen, Screen::Countdown { .. }), "{:?}", app.screen);
    let mut at = started + Duration::from_secs(4);
    app.on_tick_at_date(Duration::from_secs(3), at, date).unwrap();
    assert_eq!(app.screen, Screen::Playing);
    for _ in 0..30 {
        let Some(expected) = app.game.as_ref().and_then(|game| game.session.expected()) else {
            break;
        };
        let character = expected.chars().next().unwrap();
        at += Duration::from_millis(150);
        app.handle_key_at(key(KeyCode::Char(character)), at);
    }
    app.on_tick_at_date(Duration::from_secs(1), started + Duration::from_secs(70), date)
        .unwrap();
    let id = app.last_save_id.expect("the part is saved");
    let record = db::load_record(&app.config.storage.db_path.clone().unwrap(), id).unwrap().unwrap();
    assert!(
        record.header.bench_id.as_deref().is_some_and(|bench| bench.contains("/A/")),
        "{:?}",
        record.header.bench_id
    );
    let params: serde_json::Value = serde_json::from_str(&record.header.mode_params).unwrap();
    assert_eq!(params["lang"], "en", "{params}");
    assert_eq!(rs_typing_trainer::analysis::model::session_language(&record), "en");
}
