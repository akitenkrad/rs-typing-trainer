//! Gradual key unlocking and unlock-mode prompt configuration.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::config::{KeyFilter, ModeConfig, ModeKind, UnlockOrder};
use crate::layout::KeyboardLayout;
use crate::layout::geometry::KeyboardGeometry;

const HOME_CODES: [&str; 8] = ["KeyA", "KeyS", "KeyD", "KeyF", "KeyJ", "KeyK", "KeyL", "Semicolon"];
const ENGLISH_FREQUENCY: &str = "etaoinshrdlucmfwypvbgkjqxz";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnlockState {
    pub unlocked: String,
    pub streak: u8,
}

impl UnlockState {
    pub fn new(unlocked: String) -> Self {
        Self { unlocked, streak: 0 }
    }
}

pub fn initial_keys(layout: &KeyboardLayout) -> String {
    HOME_CODES.iter().filter_map(|code| layout.key(code).map(|pair| pair.normal)).collect()
}

pub fn target_iki_ms(target_wpm: f64) -> f64 {
    60_000.0 / (target_wpm.max(f64::EPSILON) * 5.0)
}

pub fn unlock_sequence(layout: &KeyboardLayout, geometry: &KeyboardGeometry, order: UnlockOrder) -> Vec<char> {
    let initial = initial_keys(layout).chars().collect::<HashSet<_>>();
    let layout_characters = geometry
        .rows
        .iter()
        .flatten()
        .filter_map(|key| layout.key(key.code).map(|pair| pair.normal))
        .filter(|character| !character.is_whitespace())
        .collect::<Vec<_>>();
    let mut sequence = match order {
        UnlockOrder::Frequency => ENGLISH_FREQUENCY
            .chars()
            .filter(|character| layout_characters.contains(character))
            .collect::<Vec<_>>(),
        UnlockOrder::Rows => ["KeyG", "KeyH"]
            .into_iter()
            .chain(geometry.rows.get(1).into_iter().flatten().map(|key| key.code))
            .chain(geometry.rows.get(3).into_iter().flatten().map(|key| key.code))
            .filter_map(|code| layout.key(code).map(|pair| pair.normal))
            .collect::<Vec<_>>(),
    };
    sequence.extend(layout_characters);
    let mut seen = initial;
    sequence.retain(|character| seen.insert(*character));
    sequence
}

pub fn apply_result(state: &mut UnlockState, accuracy: f64, iki_median_ms: f64, target_wpm: f64, sequence: &[char]) -> Option<char> {
    if accuracy >= 0.97 && iki_median_ms > 0.0 && iki_median_ms <= target_iki_ms(target_wpm) {
        state.streak = state.streak.saturating_add(1);
    } else {
        state.streak = 0;
    }
    if state.streak < 3 {
        return None;
    }
    state.streak = 0;
    let next = sequence.iter().copied().find(|character| !state.unlocked.contains(*character))?;
    state.unlocked.push(next);
    Some(next)
}

pub fn mode_config(mut config: ModeConfig, unlocked: &str) -> ModeConfig {
    config.kind = ModeKind::Words;
    config.keys = unlocked.to_owned();
    config.key_filter = KeyFilter::Only;
    config.base_keys = "none".to_owned();
    config
}
