use std::collections::HashMap;

use chrono::NaiveDate;
use rs_typing_trainer::config::KeyFilter;
use rs_typing_trainer::data::{DataItem, DataKind, Tier};
use rs_typing_trainer::practice::deck::{DeckOptions, Exposure, KeySelection, Mistake, build_daily_deck};
use rs_typing_trainer::record::db::{load_item_exposures, practice_salt, record_item_exposures};

fn item(index: usize, core: bool) -> DataItem {
    DataItem {
        kind: DataKind::EnglishWord,
        key: format!("word-{index}"),
        text: if index.is_multiple_of(2) {
            format!("a{index}")
        } else {
            format!("b{index}")
        },
        reading: None,
        rank: Some(index + 1),
        tier: if core { Tier::Core } else { Tier::Ext },
        source_id: None,
        author: None,
    }
}

fn options(date: &str) -> DeckOptions {
    DeckOptions {
        date: NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap(),
        user_salt: b"same-user".to_vec(),
        size: 100,
        core_ratio: 0.65,
        weekly_theme: false,
        daily_rotation: true,
        key_selection: None,
    }
}

#[test]
fn same_date_and_salt_are_stable_but_dates_rotate() {
    let rows = (0..4_000).map(|i| item(i, i < 2_000)).collect::<Vec<_>>();
    let a = build_daily_deck(&rows, &options("2026-11-14"), &HashMap::new(), &[]).unwrap();
    let b = build_daily_deck(&rows, &options("2026-11-14"), &HashMap::new(), &[]).unwrap();
    let c = build_daily_deck(&rows, &options("2026-11-15"), &HashMap::new(), &[]).unwrap();
    assert_eq!(a.items, b.items);
    let overlap = a.items.iter().filter(|row| c.items.contains(row)).count();
    assert!(overlap < 25, "overlap was {overlap}%");
}

#[test]
fn core_ratio_and_due_mistakes_are_applied() {
    let rows = (0..4_000).map(|i| item(i, i < 2_000)).collect::<Vec<_>>();
    let date = NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
    let deck = build_daily_deck(&rows, &options("2026-11-14"), &HashMap::new(), &[]).unwrap();
    let core = deck.items.iter().filter(|row| row.rank.is_some_and(|rank| rank <= 2_000)).count();
    assert!((core as isize - 65).abs() <= 1);

    for interval in [1, 3, 7, 14] {
        let mistake = Mistake {
            item_key: "word-3999".into(),
            last_mistake: date - chrono::Duration::days(interval),
        };
        let deck = build_daily_deck(&rows, &options("2026-11-14"), &HashMap::new(), &[mistake]).unwrap();
        assert!(deck.items.iter().any(|row| row.key == "word-3999"));
    }
    let not_due = Mistake {
        item_key: "word-3999".into(),
        last_mistake: date - chrono::Duration::days(2),
    };
    let deck = build_daily_deck(&rows, &options("2026-11-14"), &HashMap::new(), &[not_due]).unwrap();
    assert!(!deck.items.iter().any(|row| row.key == "word-3999"));
}

#[test]
fn key_filter_is_applied_before_core_and_tail_split() {
    let rows = (0..4_000).map(|i| item(i, i < 2_000)).collect::<Vec<_>>();
    let mut include = options("2026-11-14");
    include.key_selection = Some(KeySelection {
        filter: KeyFilter::Include,
        selected: "a".into(),
        allowed: "a0123456789".into(),
    });
    let deck = build_daily_deck(&rows, &include, &HashMap::new(), &[]).unwrap();
    assert!(deck.items.iter().all(|row| row.text.contains('a')));
}

#[test]
fn disabled_rotation_uses_the_legacy_path() {
    let rows = vec![item(0, true)];
    let mut disabled = options("2026-11-14");
    disabled.daily_rotation = false;
    assert!(build_daily_deck(&rows, &disabled, &HashMap::<String, Exposure>::new(), &[]).is_none());
}

#[test]
fn salt_is_stable_and_exposure_counts_persist() {
    let directory = tempfile::tempdir().unwrap();
    let db = directory.path().join("sessions.db");
    let first = practice_salt(&db).unwrap();
    let second = practice_salt(&db).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.len(), 32);

    let date = NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
    record_item_exposures(&db, "sentence-en", &["a", "b"], date).unwrap();
    record_item_exposures(&db, "sentence-en", &["a"], date).unwrap();
    let exposures = load_item_exposures(&db, "sentence-en").unwrap();
    assert_eq!(exposures["a"].count, 2);
    assert_eq!(exposures["b"].count, 1);
    assert_eq!(exposures["a"].last_shown, Some(date));
}
