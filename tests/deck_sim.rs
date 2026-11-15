use chrono::{Duration, NaiveDate};
use rs_typing_trainer::data::{DataItem, DataKind, Tier};
use rs_typing_trainer::practice::deck::{DeckOptions, Exposure, build_daily_deck};
use std::collections::HashMap;

#[test]
fn thirty_day_sentence_simulation_keeps_repeats_under_five_percent() {
    let rows = (0..20_000)
        .map(|i| DataItem {
            kind: DataKind::EnglishSentence,
            key: format!("s-{i}"),
            text: format!("Sentence {i} has enough text for practice."),
            reading: None,
            rank: None,
            tier: if i < 2_000 { Tier::Core } else { Tier::Ext },
            source_id: None,
            author: None,
        })
        .collect::<Vec<_>>();
    let start = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
    let mut exposure = HashMap::<String, Exposure>::new();
    let mut total = 0usize;
    let mut repeats = 0usize;
    for day in 0..30 {
        for _ in 0..3 {
            let date = start + Duration::days(day);
            let opts = DeckOptions {
                date,
                user_salt: b"sim".to_vec(),
                size: 12,
                core_ratio: 0.65,
                weekly_theme: true,
                daily_rotation: true,
                key_selection: None,
            };
            let deck = build_daily_deck(&rows, &opts, &exposure, &[]).unwrap();
            for row in deck.items {
                let seen = exposure.entry(row.key).or_default();
                repeats += usize::from(seen.count > 0);
                seen.count += 1;
                seen.last_shown = Some(date);
                total += 1;
            }
        }
    }
    let rate = repeats as f64 / total as f64;
    eprintln!("repeat rate: {rate:.4}");
    assert!(rate < 0.05, "repeat rate was {:.2}%", rate * 100.0);
}
