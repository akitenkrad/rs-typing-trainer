use chrono::NaiveDate;
use rs_typing_trainer::data::{DataItem, DataKind, Tier};
use rs_typing_trainer::practice::deck::{DeckOptions, WeeklyTheme, build_daily_deck};
use std::collections::HashMap;

fn sentence(index: usize) -> DataItem {
    DataItem {
        kind: DataKind::EnglishSentence,
        key: format!("s-{index}"),
        text: if index.is_multiple_of(4) {
            format!("Sentence number {index}!")
        } else {
            "Ordinary sentence without symbols".into()
        },
        reading: None,
        rank: None,
        tier: if index < 500 { Tier::Core } else { Tier::Ext },
        source_id: None,
        author: None,
    }
}

#[test]
fn wednesday_prefers_sentences_with_digits_or_symbols_and_sunday_has_half_tail() {
    let rows = (0..1_000).map(sentence).collect::<Vec<_>>();
    let make = |date| DeckOptions {
        date: NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap(),
        user_salt: b"u".to_vec(),
        size: 100,
        core_ratio: 0.65,
        weekly_theme: true,
        daily_rotation: true,
        key_selection: None,
    };
    let monday = build_daily_deck(&rows, &make("2026-11-16"), &HashMap::new(), &[]).unwrap();
    let wednesday = build_daily_deck(&rows, &make("2026-11-18"), &HashMap::new(), &[]).unwrap();
    let themed = |deck: &rs_typing_trainer::practice::deck::DailyDeck| {
        deck.items
            .iter()
            .filter(|row| row.text.chars().any(|c| c.is_ascii_digit() || c.is_ascii_punctuation()))
            .count()
    };
    assert!(themed(&wednesday) > themed(&monday));
    assert_eq!(wednesday.theme, WeeklyTheme::Symbols);
    let sunday = build_daily_deck(&rows, &make("2026-11-22"), &HashMap::new(), &[]).unwrap();
    assert_eq!(sunday.items.iter().filter(|row| row.tier == Tier::Ext).count(), 50);
}
