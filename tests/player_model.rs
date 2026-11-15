use chrono::{DateTime, Duration, TimeZone, Utc};
use rs_typing_trainer::analysis::metrics::AnalysisSettings;
use rs_typing_trainer::analysis::model::{
    EstimateSource, ExpectedPrompt, ModelView, Prescription, TransitionKind, build_player_model, build_sufficient_statistics, expected_performance,
    transition_kind_effects, transition_map, update_sufficient_statistics,
};
use rs_typing_trainer::config::ErrorMode;
use rs_typing_trainer::engine::session::{ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};
use rs_typing_trainer::layout::geometry::{KeyboardGeometry, geometry};
use rs_typing_trainer::layout::{KeyboardLayout, builtin_layout};
use rs_typing_trainer::record::db;
use rs_typing_trainer::record::{SessionHeader, SessionRecord};
use tempfile::TempDir;

fn settings(half_life_days: f64) -> AnalysisSettings {
    AnalysisSettings {
        pause_ms: 2_000,
        shrinkage_k: 10.0,
        half_life_days,
    }
}

fn fixtures() -> (KeyboardLayout, KeyboardGeometry) {
    (builtin_layout("qwerty").unwrap(), geometry("ansi").unwrap())
}

fn record(at: DateTime<Utc>, transitions: &[(&str, &str, f64, bool)], reading: Option<&str>) -> SessionRecord {
    let mut items = Vec::new();
    let mut keystrokes = Vec::new();
    let mut clock_us = 0_u64;
    for (index, (from, to, iki_ms, correct)) in transitions.iter().enumerate() {
        let shown_us = clock_us;
        let first_us = shown_us + 200_000;
        let second_us = first_us + (*iki_ms * 1_000.0).round() as u64;
        items.push(ItemLog {
            idx: index as u32,
            text: format!("{from}{to}"),
            reading: reading.map(str::to_owned),
            kind: None,
            shown_us,
            done_us: Some(second_us),
            outcome: ItemOutcome::Completed,
        });
        for (offset, (typed, expected, is_correct, time)) in [
            ((*from).to_owned(), (*from).to_owned(), true, first_us),
            ((if *correct { *to } else { "x" }).to_owned(), (*to).to_owned(), *correct, second_us),
        ]
        .into_iter()
        .enumerate()
        {
            keystrokes.push(KeystrokeLog {
                seq: keystrokes.len() as u32,
                t_us: time,
                release_us: None,
                item_idx: index as u32,
                pos: offset as u32,
                kind: KeystrokeKind::Char,
                typed: Some(typed),
                expected: Some(expected),
                correct: is_correct,
                code: None,
            });
        }
        clock_us = second_us + 100_000;
    }
    let mode_params = serde_json::json!({ "lang": if reading.is_some() { "ja" } else { "en" } }).to_string();
    SessionRecord {
        header: SessionHeader {
            id: None,
            started_at: (at - Duration::seconds(1)).to_rfc3339(),
            finished_at: at.to_rfc3339(),
            duration_ms: (clock_us / 1_000) as i64,
            mode: if reading.is_some() { "romaji" } else { "words" }.to_owned(),
            mode_params,
            bench_id: None,
            layout: "qwerty".to_owned(),
            input_layout: "same".to_owned(),
            geometry: "ansi".to_owned(),
            error_mode: "stop".to_owned(),
            release_events: false,
            app_version: "test".to_owned(),
            metrics_version: 2,
            net_wpm: 0.0,
            raw_wpm: Some(0.0),
            accuracy: 1.0,
            iki_median_ms: 0.0,
            iki_cv: 0.0,
            cps: 0.0,
            consistency: 0.0,
            max_combo: 0,
            completed_words: transitions.len() as u32,
            timeout_words: 0,
            mistypes_total: transitions.iter().filter(|row| !row.3).count() as u32,
            legacy_payload: None,
        },
        log: SessionLog {
            duration_us: clock_us,
            error_mode: ErrorMode::Stop,
            items,
            keystrokes,
        },
    }
}

fn spelling_record(at: DateTime<Utc>, reading: &str, typed: &str, iki_ms: f64) -> SessionRecord {
    let characters = typed.chars().collect::<Vec<_>>();
    let mut sample = record(at, &[], Some(reading));
    sample.log.items.push(ItemLog {
        idx: 0,
        text: reading.to_owned(),
        reading: Some(reading.to_owned()),
        kind: None,
        shown_us: 0,
        done_us: Some(200_000 + (characters.len().saturating_sub(1) as f64 * iki_ms * 1_000.0) as u64),
        outcome: ItemOutcome::Completed,
    });
    sample.log.keystrokes = characters
        .iter()
        .enumerate()
        .map(|(index, character)| KeystrokeLog {
            seq: index as u32,
            t_us: 200_000 + (index as f64 * iki_ms * 1_000.0) as u64,
            release_us: None,
            item_idx: 0,
            pos: index as u32,
            kind: KeystrokeKind::Char,
            typed: Some(character.to_string()),
            expected: Some(character.to_string()),
            correct: true,
            code: None,
        })
        .collect();
    sample.log.duration_us = sample.log.items[0].done_us.unwrap();
    sample.header.duration_ms = (sample.log.duration_us / 1_000) as i64;
    sample.header.completed_words = 1;
    sample
}

fn repeated(from: &'static str, to: &'static str, iki_ms: f64, count: usize) -> Vec<(&'static str, &'static str, f64, bool)> {
    (0..count)
        .map(|index| {
            let factor = match index % 4 {
                0 => 0.90,
                1 => 0.97,
                2 => 1.03,
                _ => 1.10,
            };
            (from, to, iki_ms * factor, true)
        })
        .collect()
}

#[test]
fn hierarchy_recovers_dense_transitions_and_shrinks_sparse_ones() {
    let now = Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap();
    let mut samples = Vec::new();
    for (from, to, true_ms) in [
        ("a", "s", 140.0),
        ("a", "q", 205.0),
        ("f", "j", 105.0),
        ("j", "f", 112.0),
        ("j", "k", 138.0),
    ] {
        samples.extend(repeated(from, to, true_ms, 200));
    }
    samples.extend(repeated("q", "a", 390.0, 2));
    let records = vec![record(now, &samples, None)];
    let (layout, keyboard) = fixtures();
    let model = build_player_model(&records, &layout, &keyboard, settings(14.0), ModelView::AllTime, now);

    for (from, to, truth) in [
        ("a", "s", 140.0),
        ("a", "q", 205.0),
        ("f", "j", 105.0),
        ("j", "f", 112.0),
        ("j", "k", 138.0),
    ] {
        let estimate = model.transition(from, to).unwrap();
        eprintln!("{from}->{to}: truth={truth:.3}, estimate={:.3}", estimate.estimated_iki_ms);
        assert!((estimate.estimated_iki_ms / truth - 1.0).abs() <= 0.05, "{from}->{to}: {estimate:?}");
    }
    let sparse = model.transition("q", "a").unwrap();
    eprintln!(
        "q->a sparse: sample={:.3}, parent={:.3}, estimate={:.3}, interval=[{:.3}, {:.3}]",
        sparse.sample_iki_ms, sparse.parent_iki_ms, sparse.estimated_iki_ms, sparse.estimated_iki_lo_ms, sparse.estimated_iki_hi_ms
    );
    assert!((sparse.estimated_iki_ms - sparse.parent_iki_ms).abs() < (sparse.sample_iki_ms - sparse.parent_iki_ms).abs());
}

#[test]
fn single_extreme_transition_is_pulled_at_least_halfway_to_its_parent() {
    let now = Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap();
    let mut samples = repeated("a", "q", 150.0, 200);
    samples.extend(repeated("q", "z", 155.0, 200));
    samples.extend((0..10).map(|_| ("a", "q", 150.0, false)));
    samples.extend((0..10).map(|_| ("q", "z", 155.0, false)));
    samples.push(("z", "a", 800.0, true));
    let (layout, keyboard) = fixtures();
    let model = build_player_model(
        &[record(now, &samples, None)],
        &layout,
        &keyboard,
        settings(14.0),
        ModelView::AllTime,
        now,
    );

    let sparse = model.transition("z", "a").unwrap();
    eprintln!(
        "z->a extreme: sample={:.3}, parent={:.3}, estimate={:.3}, error={:.6}, interval=[{:.3}, {:.3}]",
        sparse.sample_iki_ms,
        sparse.parent_iki_ms,
        sparse.estimated_iki_ms,
        sparse.estimated_error_rate,
        sparse.estimated_iki_lo_ms,
        sparse.estimated_iki_hi_ms
    );
    let sample_gap = (sparse.sample_iki_ms - sparse.parent_iki_ms).abs();
    let estimate_gap = (sparse.estimated_iki_ms - sparse.parent_iki_ms).abs();
    assert!(estimate_gap <= sample_gap / 2.0, "{sparse:?}");
    assert!(
        sparse.estimated_iki_ms > sparse.parent_iki_ms.min(sparse.sample_iki_ms)
            && sparse.estimated_iki_ms < sparse.parent_iki_ms.max(sparse.sample_iki_ms),
        "{sparse:?}"
    );
    let overall_error_rate = 20.0 / samples.len() as f64;
    assert!(sparse.estimated_error_rate >= overall_error_rate / 3.0, "{sparse:?}");
}

#[test]
fn transition_with_unknown_finger_uses_a_finite_higher_level_parent() {
    let now = Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap();
    let mut samples = repeated("a", "s", 145.0, 200);
    samples.extend(repeated("f", "j", 155.0, 200));
    samples.push(("-", "m", 400.0, true));
    let (layout, mut keyboard) = fixtures();
    for row in &mut keyboard.rows {
        row.retain(|key| key.code != "Minus");
    }
    let model = build_player_model(
        &[record(now, &samples, None)],
        &layout,
        &keyboard,
        settings(14.0),
        ModelView::AllTime,
        now,
    );

    let sparse = model.transition("-", "m").expect("unknown finger transitions remain observable");
    assert!(sparse.estimated_iki_ms.is_finite(), "{sparse:?}");
    assert!(sparse.estimated_iki_lo_ms.is_finite(), "{sparse:?}");
    assert!(sparse.estimated_iki_hi_ms.is_finite(), "{sparse:?}");
    assert!(
        sparse.estimated_iki_ms > sparse.parent_iki_ms.min(sparse.sample_iki_ms)
            && sparse.estimated_iki_ms < sparse.parent_iki_ms.max(sparse.sample_iki_ms),
        "{sparse:?}"
    );
}

#[test]
fn sparse_transition_has_a_wider_interval() {
    let now = Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap();
    let mut samples = repeated("a", "q", 200.0, 200);
    samples.extend(repeated("q", "a", 220.0, 2));
    samples.extend(repeated("z", "a", 210.0, 30));
    let (layout, keyboard) = fixtures();
    let model = build_player_model(
        &[record(now, &samples, None)],
        &layout,
        &keyboard,
        settings(14.0),
        ModelView::AllTime,
        now,
    );
    let dense = model.transition("a", "q").unwrap();
    let sparse = model.transition("q", "a").unwrap();
    assert!(sparse.estimated_iki_hi_ms - sparse.estimated_iki_lo_ms > dense.estimated_iki_hi_ms - dense.estimated_iki_lo_ms);
}

#[test]
fn incremental_statistics_match_a_full_rebuild() {
    let first_at = Utc.with_ymd_and_hms(2026, 11, 1, 12, 0, 0).unwrap();
    let last_at = Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap();
    let old = record(first_at, &repeated("a", "s", 220.0, 20), None);
    let new = record(last_at, &repeated("a", "s", 120.0, 30), None);
    let (layout, keyboard) = fixtures();

    for view in [ModelView::Recent, ModelView::AllTime] {
        let full = build_sufficient_statistics(&[old.clone(), new.clone()], &layout, &keyboard, settings(14.0), view, last_at);
        let mut incremental = build_sufficient_statistics(std::slice::from_ref(&old), &layout, &keyboard, settings(14.0), view, first_at);
        update_sufficient_statistics(&mut incremental, std::slice::from_ref(&new), &layout, &keyboard, settings(14.0), last_at);
        assert!(
            full.approx_eq(&incremental, 1e-10),
            "{view:?}\nfull={full:#?}\nincremental={incremental:#?}"
        );
    }
}

#[test]
fn out_of_order_increment_keeps_the_latest_decay_reference_time() {
    let old_at = Utc.with_ymd_and_hms(2026, 11, 1, 12, 0, 0).unwrap();
    let latest_at = Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap();
    let old = record(old_at, &repeated("a", "s", 300.0, 20), None);
    let latest = record(latest_at, &repeated("a", "s", 100.0, 20), None);
    let (layout, keyboard) = fixtures();
    let full = build_sufficient_statistics(
        &[old.clone(), latest.clone()],
        &layout,
        &keyboard,
        settings(7.0),
        ModelView::Recent,
        latest_at,
    );
    let mut incremental = build_sufficient_statistics(
        std::slice::from_ref(&latest),
        &layout,
        &keyboard,
        settings(7.0),
        ModelView::Recent,
        latest_at,
    );
    update_sufficient_statistics(&mut incremental, std::slice::from_ref(&old), &layout, &keyboard, settings(7.0), old_at);
    assert!(full.approx_eq(&incremental, 1e-10), "full={full:#?}\nincremental={incremental:#?}");
}

#[test]
fn recent_view_follows_the_configured_half_life() {
    let old_at = Utc.with_ymd_and_hms(2026, 11, 1, 12, 0, 0).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap();
    let records = vec![
        record(old_at, &repeated("a", "s", 300.0, 100), None),
        record(now, &repeated("a", "s", 100.0, 100), None),
    ];
    let (layout, keyboard) = fixtures();
    let recent = build_player_model(&records, &layout, &keyboard, settings(1.0), ModelView::Recent, now);
    let all_time = build_player_model(&records, &layout, &keyboard, settings(1.0), ModelView::AllTime, now);
    assert!(recent.transition("a", "s").unwrap().estimated_iki_ms < 105.0);
    assert!(all_time.transition("a", "s").unwrap().estimated_iki_ms > 160.0);
}

#[test]
fn expected_wpm_matches_constant_cost_and_preferred_japanese_spelling() {
    let now = Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap();
    let mut samples = Vec::new();
    for pair in [("a", "b"), ("b", "c"), ("c", "d"), ("s", "i"), ("i", "s"), ("s", "h"), ("h", "i")] {
        samples.extend((0..100).map(|_| (pair.0, pair.1, 100.0, true)));
    }
    let mut records = vec![record(now, &samples, None)];
    records.push(spelling_record(now, "し", "si", 100.0));
    records.push(spelling_record(now, "し", "si", 100.0));
    records.push(spelling_record(now, "し", "shi", 100.0));
    let (layout, keyboard) = fixtures();
    let model = build_player_model(&records, &layout, &keyboard, settings(14.0), ModelView::AllTime, now);

    let english = expected_performance(&model, &ExpectedPrompt::english("abcd"));
    assert!((english.total_ms - 500.0).abs() < 1e-6, "{english:?}");
    assert!((english.expected_wpm - 96.0).abs() < 1e-6, "{english:?}");

    let japanese = expected_performance(&model, &ExpectedPrompt::japanese("しし"));
    assert_eq!(japanese.keystrokes, "sisi");
    assert!((japanese.expected_wpm - 96.0).abs() < 1e-6, "{japanese:?}");
}

#[test]
fn unseen_transition_uses_its_hierarchical_parent_in_expected_performance() {
    let now = Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap();
    let (layout, keyboard) = fixtures();
    let model = build_player_model(
        &[record(now, &repeated("a", "s", 150.0, 100), None)],
        &layout,
        &keyboard,
        settings(14.0),
        ModelView::AllTime,
        now,
    );

    // q->w has the same standard finger pair as the observed a->s transition.
    let parent = model.estimate_or_parent("q", "w").expect("finger-pair estimate");
    assert_eq!(parent.source, EstimateSource::FingerPair);
    assert!((parent.estimated_iki_ms - model.transition("a", "s").unwrap().parent_iki_ms).abs() < 1e-9);
    let expected = expected_performance(&model, &ExpectedPrompt::english("qw"));
    assert!(expected.expected_wpm.is_finite());
    assert!((expected.transition_ms - parent.estimated_iki_ms).abs() < 1e-9);

    let hand = model.estimate_or_parent("q", "e").expect("hand-pair estimate");
    assert_eq!(hand.source, EstimateSource::HandPair);
    assert!(hand.estimated_iki_ms.is_finite());

    // A character absent from the selected layout still has the overall parent.
    let overall = model.estimate_or_parent("🦀", "w").expect("overall estimate");
    assert_eq!(overall.source, EstimateSource::Overall);
    assert!(overall.estimated_iki_ms.is_finite());
    let expected = expected_performance(&model, &ExpectedPrompt::english("🦀w"));
    assert!((expected.transition_ms - overall.estimated_iki_ms).abs() < 1e-9);

    let encoded = serde_json::to_string(&model).unwrap();
    let restored = serde_json::from_str::<rs_typing_trainer::analysis::model::PlayerModel>(&encoded).unwrap();
    assert_eq!(restored.estimate_or_parent("q", "w"), Some(parent));
}

#[test]
fn same_finger_slowdown_is_reported_as_a_loss() {
    let now = Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap();
    let mut samples = repeated("a", "q", 220.0, 100);
    for pair in [("a", "s"), ("f", "j"), ("j", "k"), ("j", "f")] {
        samples.extend(repeated(pair.0, pair.1, 100.0, 100));
    }
    let (layout, keyboard) = fixtures();
    let model = build_player_model(
        &[record(now, &samples, None)],
        &layout,
        &keyboard,
        settings(14.0),
        ModelView::AllTime,
        now,
    );
    let effects = transition_kind_effects(&model);
    let same_finger = effects.iter().find(|row| row.kind == TransitionKind::SameFinger).unwrap();
    assert!(same_finger.difference_ms > 50.0, "{same_finger:?}");
    assert!(same_finger.weighted_per_keystroke_ms > 50.0, "{same_finger:?}");
}

#[test]
fn map_data_contains_relative_key_and_transition_values() {
    let now = Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap();
    let mut samples = repeated("a", "s", 200.0, 40);
    samples.extend(repeated("f", "j", 100.0, 40));
    let (layout, keyboard) = fixtures();
    let model = build_player_model(
        &[record(now, &samples, None)],
        &layout,
        &keyboard,
        settings(14.0),
        ModelView::AllTime,
        now,
    );
    let map = transition_map(&model);
    assert!(map.keys.iter().any(|key| key.key == "s" && key.relative_iki > 1.0));
    assert!(
        map.transitions
            .iter()
            .any(|edge| edge.from == "a" && edge.to == "s" && edge.relative_iki > 1.0)
    );
}

#[test]
fn v5_database_migrates_and_prescription_roundtrips() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("sessions.db");
    let at = Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap();
    db::save(&path, &record(at, &[("a", "s", 150.0, true)], None)).unwrap();
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("DROP TABLE IF EXISTS transition_model; DROP TABLE IF EXISTS prescriptions; PRAGMA user_version = 5;")
            .unwrap();
    }

    assert_eq!(db::list(&path).unwrap().len(), 1);
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0)).unwrap(), 7);
    for table in ["transition_model", "prescriptions"] {
        let exists: i64 = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1, "missing {table}");
    }
    drop(conn);

    let prescription = Prescription {
        id: None,
        created_at: at,
        layout: "qwerty".to_owned(),
        kind: "transition-practice".to_owned(),
        target: serde_json::json!({"from":"a","to":"s"}),
        control: serde_json::json!({"from":"j","to":"k"}),
        hypothesis: serde_json::json!({"change_ms":-20.0}),
        baseline: serde_json::json!({"estimated_iki_ms":150.0}),
        status: "active".to_owned(),
        evaluated_at: None,
        result: None,
    };
    let id = db::save_prescription(&path, &prescription).unwrap();
    let loaded = db::load_prescriptions(&path).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id, Some(id));
    assert_eq!(loaded[0].target, prescription.target);
    assert_eq!(loaded[0].control, prescription.control);

    let mut evaluated = loaded[0].clone();
    evaluated.status = "evaluated".to_owned();
    evaluated.evaluated_at = Some(at + Duration::days(7));
    evaluated.result = Some(serde_json::json!({"change_ms":-18.0}));
    assert_eq!(db::save_prescription(&path, &evaluated).unwrap(), id);
    let loaded = db::load_prescriptions(&path).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].status, "evaluated");
    assert_eq!(loaded[0].result, evaluated.result);
}

#[test]
fn transition_cache_roundtrips_without_changing_statistics() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("sessions.db");
    let now = Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap();
    let (layout, keyboard) = fixtures();
    let stats = build_sufficient_statistics(
        &[record(now, &repeated("a", "s", 150.0, 10), None)],
        &layout,
        &keyboard,
        settings(14.0),
        ModelView::Recent,
        now,
    );
    db::save_transition_model(&path, &stats).unwrap();
    let loaded = db::load_transition_model(&path, "qwerty", "en", ModelView::Recent).unwrap().unwrap();
    assert!(stats.approx_eq(&loaded, 1e-12));

    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute("UPDATE transition_model SET model_version = 0", []).unwrap();
    drop(connection);
    assert!(db::load_transition_model(&path, "qwerty", "en", ModelView::Recent).unwrap().is_none());
}

#[test]
fn cache_update_uses_session_ids_and_is_idempotent() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("sessions.db");
    let now = Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap();
    let (layout, keyboard) = fixtures();
    let mut sample = record(now, &repeated("a", "s", 150.0, 10), None);
    let id = db::save(&path, &sample).unwrap();
    sample.header.id = Some(id);

    db::update_transition_model_cache(&path, &sample, &layout, &keyboard, settings(14.0)).unwrap();
    let first = db::load_transition_model(&path, "qwerty", "en", ModelView::Recent).unwrap().unwrap();
    db::update_transition_model_cache(&path, &sample, &layout, &keyboard, settings(14.0)).unwrap();
    let second = db::load_transition_model(&path, "qwerty", "en", ModelView::Recent).unwrap().unwrap();

    assert_eq!(first.last_session_id, Some(id));
    assert!(first.approx_eq(&second, 1e-12));
}

#[test]
fn empty_history_returns_empty_outputs_without_inventing_values() {
    let now = Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap();
    let (layout, keyboard) = fixtures();
    let model = build_player_model(&[], &layout, &keyboard, settings(14.0), ModelView::Recent, now);
    assert!(model.transitions.is_empty());
    assert_eq!(model.personal_iki_median_ms, 0.0);
    assert_eq!(model.reaction_median_ms, 0.0);
    assert!(transition_map(&model).keys.is_empty());
    assert!(transition_kind_effects(&model).iter().all(|row| row.frequency == 0.0));
    let expected = expected_performance(&model, &ExpectedPrompt::english("ab"));
    assert_eq!(expected.total_ms, 0.0);
    assert_eq!(expected.expected_wpm, 0.0);
}
