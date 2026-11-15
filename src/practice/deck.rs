use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use chrono::{Datelike, NaiveDate, Weekday};

use crate::config::KeyFilter;
use crate::data::{DataItem, DataKind, Tier};
use crate::engine::romaji::RomajiJudge;
use crate::i18n::Lang;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Exposure {
    pub count: u64,
    pub last_shown: Option<NaiveDate>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mistake {
    pub item_key: String,
    pub last_mistake: NaiveDate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeySelection {
    pub filter: KeyFilter,
    pub selected: String,
    pub allowed: String,
}

#[derive(Clone, Debug)]
pub struct DeckOptions {
    pub date: NaiveDate,
    pub user_salt: Vec<u8>,
    pub size: usize,
    pub core_ratio: f64,
    pub weekly_theme: bool,
    pub daily_rotation: bool,
    pub key_selection: Option<KeySelection>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeeklyTheme {
    Standard,
    Symbols,
    Author,
    BroadTail,
}

impl WeeklyTheme {
    pub fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Standard, Lang::En) => "Daily mix",
            (Self::Standard, Lang::Ja) => "日替わりミックス",
            (Self::Symbols, Lang::En) => "Symbols Wednesday",
            (Self::Symbols, Lang::Ja) => "水曜：数字・記号",
            (Self::Author, Lang::En) => "Author Friday",
            (Self::Author, Lang::Ja) => "金曜：作家特集",
            (Self::BroadTail, Lang::En) => "Broad-tail Sunday",
            (Self::BroadTail, Lang::Ja) => "日曜：裾野を広く",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DailyDeck {
    pub items: Vec<DataItem>,
    pub theme: WeeklyTheme,
}

pub fn build_daily_deck(
    candidates: &[DataItem],
    options: &DeckOptions,
    exposures: &HashMap<String, Exposure>,
    mistakes: &[Mistake],
) -> Option<DailyDeck> {
    if !options.daily_rotation {
        return None;
    }
    let filtered = candidates
        .iter()
        .filter(|item| matches_key_selection(item, options.key_selection.as_ref()))
        .cloned()
        .collect::<Vec<_>>();
    if filtered.is_empty() || options.size == 0 {
        return Some(DailyDeck {
            items: Vec::new(),
            theme: theme_for(options),
        });
    }

    let theme = theme_for(options);
    let seed = daily_seed(options);
    let author = featured_author(&filtered, theme, seed);
    let mut core = filtered.iter().filter(|item| is_core(item)).cloned().collect::<Vec<_>>();
    let mut tail = filtered.iter().filter(|item| !is_core(item)).cloned().collect::<Vec<_>>();
    sort_pool(&mut core, exposures, theme, author.as_deref(), seed);
    sort_pool(&mut tail, exposures, theme, author.as_deref(), seed ^ 0x7461_696c);

    let ratio = if theme == WeeklyTheme::BroadTail {
        0.5
    } else {
        options.core_ratio.clamp(0.0, 1.0)
    };
    let target_size = options.size.min(filtered.len());
    let core_count = ((target_size as f64) * ratio).round() as usize;
    let mut selected = core.into_iter().take(core_count).collect::<Vec<_>>();
    selected.extend(tail.into_iter().take(target_size.saturating_sub(selected.len())));

    let due_keys = mistakes
        .iter()
        .filter(|mistake| matches!((options.date - mistake.last_mistake).num_days(), 1 | 3 | 7 | 14))
        .map(|mistake| mistake.item_key.as_str())
        .collect::<HashSet<_>>();
    let due_limit = target_size / 10;
    let mut due = filtered
        .iter()
        .filter(|item| due_keys.contains(item.key.as_str()) && !selected.iter().any(|row| row.key == item.key))
        .cloned()
        .collect::<Vec<_>>();
    due.sort_by_key(|item| stable_hash(seed ^ 0x6d69_7374, item.key.as_bytes()));
    for item in due.into_iter().take(due_limit.max(usize::from(target_size > 0))) {
        if selected.len() == target_size {
            selected.pop();
        }
        selected.push(item);
    }
    selected.sort_by_key(|item| stable_hash(seed ^ 0x6f72_6465, item.key.as_bytes()));
    Some(DailyDeck { items: selected, theme })
}

fn is_core(item: &DataItem) -> bool {
    match item.kind {
        DataKind::EnglishWord => item.rank.is_some_and(|rank| rank <= 2_000),
        DataKind::JapaneseWord => item.rank.is_some_and(|rank| rank <= 1_500),
        DataKind::EnglishSentence | DataKind::JapaneseSentence => item.tier == Tier::Core,
    }
}

fn theme_for(options: &DeckOptions) -> WeeklyTheme {
    weekly_theme(options.date, options.weekly_theme)
}

pub fn weekly_theme(date: NaiveDate, enabled: bool) -> WeeklyTheme {
    if !enabled {
        return WeeklyTheme::Standard;
    }
    match date.weekday() {
        Weekday::Wed => WeeklyTheme::Symbols,
        Weekday::Fri => WeeklyTheme::Author,
        Weekday::Sun => WeeklyTheme::BroadTail,
        _ => WeeklyTheme::Standard,
    }
}

fn featured_author(items: &[DataItem], theme: WeeklyTheme, seed: u64) -> Option<String> {
    if theme != WeeklyTheme::Author {
        return None;
    }
    let mut authors = items.iter().filter_map(|item| item.author.clone()).collect::<Vec<_>>();
    authors.sort();
    authors.dedup();
    authors.sort_by_key(|author| stable_hash(seed, author.as_bytes()));
    authors.into_iter().next()
}

fn sort_pool(pool: &mut [DataItem], exposures: &HashMap<String, Exposure>, theme: WeeklyTheme, author: Option<&str>, seed: u64) {
    pool.sort_by(|left, right| {
        let exposure_cmp = if matches!(left.kind, DataKind::EnglishSentence | DataKind::JapaneseSentence) {
            exposure_count(exposures, left).cmp(&exposure_count(exposures, right))
        } else {
            Ordering::Equal
        };
        exposure_cmp
            .then_with(|| theme_priority(left, theme, author).cmp(&theme_priority(right, theme, author)))
            .then_with(|| stable_hash(seed, left.key.as_bytes()).cmp(&stable_hash(seed, right.key.as_bytes())))
    });
}

fn exposure_count(exposures: &HashMap<String, Exposure>, item: &DataItem) -> u64 {
    exposures.get(&item.key).map_or(0, |value| value.count)
}

fn theme_priority(item: &DataItem, theme: WeeklyTheme, author: Option<&str>) -> u8 {
    match theme {
        WeeklyTheme::Symbols => u8::from(
            !item
                .text
                .chars()
                .any(|character| character.is_ascii_digit() || (character.is_ascii_punctuation() && !matches!(character, '.' | ','))),
        ),
        WeeklyTheme::Author => u8::from(item.author.as_deref() != author),
        _ => 0,
    }
}

fn matches_key_selection(item: &DataItem, selection: Option<&KeySelection>) -> bool {
    let Some(selection) = selection else { return true };
    if selection.selected.is_empty() {
        return true;
    }
    let selected = selection.selected.chars().collect::<HashSet<_>>();
    let allowed = selection.allowed.chars().collect::<HashSet<_>>();
    let target = item.reading.as_deref().unwrap_or(&item.text);
    match selection.filter {
        KeyFilter::Include => {
            if let Some(reading) = item.reading.as_deref() {
                RomajiJudge::new(reading)
                    .accepted_spellings(512)
                    .iter()
                    .any(|spelling| spelling.chars().any(|character| selected.contains(&character)))
            } else {
                target.chars().any(|character| selected.contains(&character))
            }
        }
        KeyFilter::Only => {
            if let Some(reading) = item.reading.as_deref() {
                RomajiJudge::new(reading).has_spelling_using(&allowed)
            } else {
                target.chars().all(|character| allowed.contains(&character))
            }
        }
    }
}

fn daily_seed(options: &DeckOptions) -> u64 {
    let mut bytes = options.date.format("%Y-%m-%d").to_string().into_bytes();
    bytes.push(0);
    bytes.extend_from_slice(&options.user_salt);
    stable_hash(0x5253_5044_4543_4b31, &bytes)
}

fn stable_hash(seed: u64, bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64 ^ seed;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}
