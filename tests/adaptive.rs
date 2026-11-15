use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::analysis::metrics::{AnalysisSettings, analyze_records};
use rs_typing_trainer::config::ErrorMode;
use rs_typing_trainer::config::{Config, ModeKind};
use rs_typing_trainer::engine::session::{ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};
use rs_typing_trainer::layout::builtin_layout;
use rs_typing_trainer::layout::geometry::geometry;
use rs_typing_trainer::practice::adaptive::{AdaptiveItemKind, AdaptiveOptions, build_plan, generate_items, transition_share};
use rs_typing_trainer::record::{SessionHeader, SessionRecord};
use rs_typing_trainer::ui;

mod common;
use common::test_app;

fn synthetic_record() -> SessionRecord {
    let alphabet = "abcdefghijklmnopqrstuvwxyz".chars().collect::<Vec<_>>();
    let mut pairs = vec![('t', 'h'), ('h', 'e'), ('e', 'r'), ('a', 't'), ('r', 'u')];
    for from in &alphabet {
        for to in &alphabet {
            if from != to && !pairs.contains(&(*from, *to)) {
                pairs.push((*from, *to));
            }
            if pairs.len() == 50 {
                break;
            }
        }
        if pairs.len() == 50 {
            break;
        }
    }

    let mut items = Vec::new();
    let mut keystrokes = Vec::new();
    let mut seq = 0_u32;
    let mut item_idx = 0_u32;
    let mut clock = 0_u64;
    for (from, to) in pairs {
        for _ in 0..20 {
            let interval = match (from, to) {
                ('t', 'h') => 600_000,
                ('h', 'e') => 500_000,
                ('e', 'r') => 450_000,
                ('a', 't') => 400_000,
                ('r', 'u') => 350_000,
                _ => 100_000,
            };
            let text = format!("{from}{to}");
            items.push(ItemLog {
                idx: item_idx,
                text,
                reading: None,
                shown_us: clock,
                done_us: Some(clock + interval + 10_000),
                outcome: ItemOutcome::Completed,
            });
            for (offset, expected) in [(10_000, from), (10_000 + interval, to)] {
                keystrokes.push(KeystrokeLog {
                    seq,
                    t_us: clock + offset,
                    release_us: None,
                    item_idx,
                    pos: seq,
                    kind: KeystrokeKind::Char,
                    typed: Some(expected.to_string()),
                    expected: Some(expected.to_string()),
                    correct: true,
                    code: None,
                });
                seq += 1;
            }
            item_idx += 1;
            clock += interval + 20_000;
        }
    }
    SessionRecord {
        header: SessionHeader {
            id: None,
            started_at: "2026-11-13T00:00:00Z".into(),
            finished_at: "2026-11-13T00:10:00Z".into(),
            duration_ms: (clock / 1_000) as i64,
            mode: "words".into(),
            mode_params: "{}".into(),
            bench_id: None,
            layout: "qwerty".into(),
            input_layout: "same".into(),
            geometry: "ansi".into(),
            error_mode: "stop".into(),
            release_events: false,
            app_version: "test".into(),
            metrics_version: 2,
            net_wpm: 0.0,
            raw_wpm: Some(0.0),
            accuracy: 1.0,
            iki_median_ms: 100.0,
            iki_cv: 0.0,
            cps: 0.0,
            consistency: 0.0,
            max_combo: 0,
            completed_words: 0,
            timeout_words: 0,
            mistypes_total: 0,
            legacy_payload: None,
        },
        log: SessionLog {
            duration_us: clock,
            error_mode: ErrorMode::Stop,
            items,
            keystrokes,
        },
    }
}

fn plan(previous: &[(&str, &str)], minimum: usize) -> rs_typing_trainer::practice::adaptive::AdaptivePlan {
    let metrics = analyze_records(
        &[synthetic_record()],
        &builtin_layout("qwerty").unwrap(),
        &geometry("ansi").unwrap(),
        AnalysisSettings::default(),
    );
    let corpus = vec![
        "the".to_owned(),
        "other".to_owned(),
        "math".to_owned(),
        "rust".to_owned(),
        "cargo".to_owned(),
        "type".to_owned(),
        "error".to_owned(),
        "speed".to_owned(),
        "word".to_owned(),
        "quick".to_owned(),
    ];
    let previous = previous.iter().map(|(a, b)| ((*a).to_owned(), (*b).to_owned())).collect::<Vec<_>>();
    build_plan(
        &metrics,
        &corpus,
        AdaptiveOptions {
            error_weight_ms: None,
            drill_ratio: 0.3,
            min_transition_types: minimum,
            max_share: 0.4,
        },
        &previous,
    )
}

#[test]
fn top_transition_is_overrepresented_but_capped_and_top_five_are_used() {
    let plan = plan(&[], 50);
    assert!(!plan.fallback_to_words);
    assert_eq!((&plan.transitions[0].from, &plan.transitions[0].to), (&"t".to_owned(), &"h".to_owned()));

    let corpus = ["the", "other", "math", "rust", "cargo", "type", "error", "speed", "word", "quick"];
    let uniform = transition_share(corpus.iter().copied(), ("t", "h"));
    let generated = generate_items(&plan, &corpus, 1_000, 42);
    let adaptive = transition_share(generated.iter().map(|item| item.text.as_str()), ("t", "h"));
    eprintln!("uniform={uniform:.3} adaptive={adaptive:.3}");
    assert!(adaptive > uniform, "uniform={uniform}, adaptive={adaptive}");
    assert!(adaptive <= 0.4, "adaptive={adaptive}");
    for transition in plan.transitions.iter().take(5) {
        assert!(
            generated
                .iter()
                .any(|item| item.transition == (transition.from.clone(), transition.to.clone())),
            "{}→{} was never selected",
            transition.from,
            transition.to
        );
    }
}

#[test]
fn generated_mix_is_seventy_percent_words_and_thirty_percent_drills() {
    let generated = generate_items(
        &plan(&[], 50),
        &["the", "other", "math", "rust", "cargo", "type", "error", "speed", "word", "quick"],
        1_000,
        7,
    );
    let drills = generated.iter().filter(|item| item.kind == AdaptiveItemKind::Drill).count();
    assert_eq!(drills, 300);
    assert_eq!(generated.len() - drills, 700);
}

#[test]
fn insufficient_history_falls_back_to_words() {
    let plan = plan(&[], 51);
    assert!(plan.fallback_to_words);
    assert_eq!(plan.warning("en"), "Not enough records; using normal words mode");
    assert_eq!(plan.warning("ja"), "記録が足りないので通常の words で出題します");
}

#[test]
fn previous_focus_halves_the_transition_weight() {
    let normal = plan(&[], 50);
    let discounted = plan(&[("t", "h")], 50);
    let normal = normal.transitions.iter().find(|row| row.from == "t" && row.to == "h").unwrap();
    let discounted = discounted.transitions.iter().find(|row| row.from == "t" && row.to == "h").unwrap();
    assert!((discounted.weight - normal.weight * 0.5).abs() < 1e-9);
}

#[test]
fn adaptive_result_warns_that_wpm_is_not_an_improvement_measure() {
    let mut config = Config::default();
    config.mode.kind = ModeKind::Adaptive;
    config.session.duration_seconds = 1;
    config.ui.animation_enabled = false;
    let mut app = test_app(config);
    let now = std::time::Instant::now();
    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    app.handle_key_at(enter, now);
    app.handle_key_at(enter, now);
    app.on_tick_at(std::time::Duration::from_secs(3), now).unwrap();
    app.on_tick_at(std::time::Duration::from_millis(1), now).unwrap();
    app.on_tick_at(std::time::Duration::from_secs(2), now + std::time::Duration::from_secs(2))
        .unwrap();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    let output = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect::<String>();
    assert!(output.contains("not an improvement indicator"), "{output}");
}

#[test]
fn adaptive_language_is_independent_from_display_language() {
    let mut config = Config::default();
    config.general.lang = rs_typing_trainer::i18n::Lang::Ja;
    config.adaptive.lang = "en".to_owned();
    config.mode.kind = ModeKind::Adaptive;
    config.ui.animation_enabled = false;
    let mut app = test_app(config);
    let now = std::time::Instant::now();
    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    app.handle_key_at(enter, now);
    app.handle_key_at(enter, now);
    app.on_tick_at(std::time::Duration::from_secs(3), now).unwrap();
    app.on_tick_at(std::time::Duration::from_millis(1), now).unwrap();
    assert!(app.game.as_ref().unwrap().current_prompt.reading.is_none());

    let mut config = Config::default();
    config.general.lang = rs_typing_trainer::i18n::Lang::En;
    config.adaptive.lang = "ja".to_owned();
    config.mode.kind = ModeKind::Adaptive;
    config.ui.animation_enabled = false;
    let mut app = test_app(config);
    app.handle_key_at(enter, now);
    app.handle_key_at(enter, now);
    app.on_tick_at(std::time::Duration::from_secs(3), now).unwrap();
    app.on_tick_at(std::time::Duration::from_millis(1), now).unwrap();
    assert!(app.game.as_ref().unwrap().current_prompt.reading.is_some());
}
