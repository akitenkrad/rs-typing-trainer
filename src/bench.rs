//! Fixed, deterministic `bench-v1` parallel forms.

use std::collections::{BTreeMap, HashMap, HashSet};

use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

use crate::analysis::metrics::TransitionClass;
use crate::config::{Config, ErrorMode};
use crate::data::{DataCatalog, DataKind, Tier};
use crate::engine::item::Item;
use crate::engine::romaji::RomajiJudge;
use crate::layout::KeyboardLayout;
use crate::layout::geometry::{Finger, KeyboardGeometry};

pub const BENCH_SERIES: &str = "bench-v1";
pub const PARALLEL_VERSION_COUNT: u8 = 8;
const PART_B_SOURCE_WORDS: usize = 5_000;
const PART_B_WORDS_PER_LENGTH: usize = 60;
const PART_B_CANDIDATES_PER_LENGTH: usize = 800;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum BenchPart {
    A,
    B,
    C,
    D,
}

impl BenchPart {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
            Self::D => "D",
        }
    }

    pub fn next(self, include_d: bool) -> Option<Self> {
        match self {
            Self::A => Some(Self::B),
            Self::B => Some(Self::C),
            Self::C if include_d => Some(Self::D),
            Self::C | Self::D => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchSettings {
    pub duration_seconds: u64,
    pub item_timeout_seconds: u64,
    pub error_mode: ErrorMode,
    pub preview_items: usize,
}

impl BenchSettings {
    pub fn fixed(part: BenchPart, _user: &Config) -> Self {
        Self {
            duration_seconds: if part == BenchPart::D { 45 } else { 60 },
            item_timeout_seconds: 0,
            error_mode: ErrorMode::Stop,
            preview_items: 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchPrompt {
    pub text: String,
    pub reading: Option<String>,
    pub target: String,
}

impl BenchPrompt {
    pub fn into_item(self) -> Item {
        if let Some(reading) = self.reading {
            Item::new(self.text, Some(reading.clone()), Box::new(RomajiJudge::new(&reading)))
        } else {
            Item::exact(self.target)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchVersion {
    pub id: String,
    pub part: BenchPart,
    pub parallel_index: u8,
    pub settings: BenchSettings,
    pub prompts: Vec<BenchPrompt>,
    pub transition_class_counts: BTreeMap<TransitionClass, usize>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BenchComparison {
    pub previous_wpm_delta: Option<f64>,
    pub previous_accuracy_delta: Option<f64>,
    pub recent_three_wpm_delta: Option<f64>,
    pub recent_three_accuracy_delta: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BenchResultSummary {
    pub id: String,
    pub part: BenchPart,
    pub wpm: f64,
    pub accuracy: f64,
    pub comparison: BenchComparison,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BenchResume {
    pub part: BenchPart,
    pub parallel_index: u8,
    pub completed: Vec<BenchResultSummary>,
}

pub fn next_parallel_index(previous: Option<u8>) -> u8 {
    previous.map_or(1, |value| value % PARALLEL_VERSION_COUNT + 1)
}

pub fn generate_parallel_versions(part: BenchPart, data: &DataCatalog, layout: &KeyboardLayout, geometry: &KeyboardGeometry) -> Vec<BenchVersion> {
    if part == BenchPart::B {
        return generate_comprehensive_versions(data, layout, geometry);
    }
    let base = base_prompts(part, data);
    (1..=PARALLEL_VERSION_COUNT)
        .map(|parallel_index| {
            let mut prompts = base.clone();
            let seed = 0xB3EC_0000_D15C_A11Eu64 ^ ((part as u64) << 16) ^ u64::from(parallel_index);
            prompts.shuffle(&mut StdRng::seed_from_u64(seed));
            let transition_class_counts = count_transition_classes(&prompts, layout, geometry);
            BenchVersion {
                id: format!("{BENCH_SERIES}/{}/{parallel_index:02}", part.as_str()),
                part,
                parallel_index,
                settings: BenchSettings::fixed(part, &Config::default()),
                prompts,
                transition_class_counts,
            }
        })
        .collect()
}

pub fn generate_version(
    part: BenchPart,
    parallel_index: u8,
    data: &DataCatalog,
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
) -> BenchVersion {
    let index = parallel_index.clamp(1, PARALLEL_VERSION_COUNT);
    generate_parallel_versions(part, data, layout, geometry)[usize::from(index - 1)].clone()
}

fn base_prompts(part: BenchPart, data: &DataCatalog) -> Vec<BenchPrompt> {
    match part {
        BenchPart::A => {
            let mut rows = data.fixed_items(DataKind::EnglishWord);
            rows.sort_by_key(|row| row.rank.unwrap_or(usize::MAX));
            rows.into_iter()
                .take(200)
                .map(|row| BenchPrompt {
                    text: row.text.clone(),
                    reading: None,
                    target: row.text.clone(),
                })
                .collect()
        }
        BenchPart::B => unreachable!("part B uses independently generated parallel forms"),
        BenchPart::C => {
            let mut rows = data
                .fixed_items(DataKind::JapaneseWord)
                .into_iter()
                .filter(|row| row.tier == Tier::Core)
                .collect::<Vec<_>>();
            rows.sort_by_key(|row| row.rank.unwrap_or(usize::MAX));
            rows.into_iter()
                .take(300)
                .filter_map(|row| {
                    let reading = row.reading.clone()?;
                    Some(BenchPrompt {
                        text: row.text.clone(),
                        target: RomajiJudge::new(&reading).guide().to_owned(),
                        reading: Some(reading),
                    })
                })
                .collect()
        }
        BenchPart::D => ["1234567890", "!@#$%^&*()", "-_=+[]{}", ";:'\",.<>/?"]
            .into_iter()
            .cycle()
            .take(64)
            .map(|text| BenchPrompt {
                text: text.to_owned(),
                reading: None,
                target: text.to_owned(),
            })
            .collect(),
    }
}

#[derive(Clone)]
struct MarkovModel {
    starts: [usize; 26],
    transitions: [[usize; 26]; 26],
    character_shares: [f64; 26],
    source_words: Vec<String>,
    real_words: HashSet<String>,
}

#[derive(Clone)]
struct PseudowordCandidate {
    word: String,
    character_counts: [usize; 26],
    class_counts: [usize; 4],
    bigrams: Vec<(u8, u8)>,
}

#[derive(Default)]
struct SelectionState {
    character_counts: [usize; 26],
    class_counts: [usize; 4],
    bigrams: HashSet<(u8, u8)>,
    selected_words: HashSet<String>,
}

fn generate_comprehensive_versions(data: &DataCatalog, layout: &KeyboardLayout, geometry: &KeyboardGeometry) -> Vec<BenchVersion> {
    let model = markov_model(data);
    let class_shares = source_class_shares(&model.source_words, layout, geometry);
    let mut used_across_versions = HashSet::new();
    (1..=PARALLEL_VERSION_COUNT)
        .map(|parallel_index| {
            let seed = 0xB3EC_B000_D15C_A11Eu64 ^ u64::from(parallel_index);
            let mut rng = StdRng::seed_from_u64(seed);
            let candidates = generate_candidates(&model, layout, geometry, &used_across_versions, &mut rng);
            let mut prompts = select_pseudowords(candidates, &model.character_shares, &class_shares);
            for prompt in &prompts {
                used_across_versions.insert(prompt.target.clone());
            }
            prompts.shuffle(&mut rng);
            let transition_class_counts = count_transition_classes(&prompts, layout, geometry);
            BenchVersion {
                id: format!("{BENCH_SERIES}/B/{parallel_index:02}"),
                part: BenchPart::B,
                parallel_index,
                settings: BenchSettings::fixed(BenchPart::B, &Config::default()),
                prompts,
                transition_class_counts,
            }
        })
        .collect()
}

fn markov_model(data: &DataCatalog) -> MarkovModel {
    let mut rows = data.fixed_items(DataKind::EnglishWord);
    rows.sort_by_key(|row| row.rank.unwrap_or(usize::MAX));
    let real_words = rows.iter().filter_map(|row| normalized_word(&row.text)).collect::<HashSet<_>>();
    let source_words = rows
        .into_iter()
        .take(PART_B_SOURCE_WORDS)
        .filter_map(|row| normalized_word(&row.text))
        .collect::<Vec<_>>();
    let mut starts = [0; 26];
    let mut transitions = [[0; 26]; 26];
    let mut character_counts = [0; 26];
    for word in &source_words {
        let letters = word.bytes().map(|letter| usize::from(letter - b'a')).collect::<Vec<_>>();
        if let Some(first) = letters.first() {
            starts[*first] += 1;
        }
        for letter in &letters {
            character_counts[*letter] += 1;
        }
        for pair in letters.windows(2) {
            transitions[pair[0]][pair[1]] += 1;
        }
    }
    let total = character_counts.iter().sum::<usize>() as f64;
    let character_shares = character_counts.map(|count| count as f64 / total);
    MarkovModel {
        starts,
        transitions,
        character_shares,
        source_words,
        real_words,
    }
}

fn normalized_word(text: &str) -> Option<String> {
    let word = text.to_ascii_lowercase();
    (!word.is_empty() && word.bytes().all(|letter| letter.is_ascii_lowercase())).then_some(word)
}

fn generate_candidates(
    model: &MarkovModel,
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
    already_used: &HashSet<String>,
    rng: &mut StdRng,
) -> Vec<PseudowordCandidate> {
    let physical = physical_fingers(geometry);
    let mut candidates = Vec::with_capacity(PART_B_CANDIDATES_PER_LENGTH * 4);
    let mut seen = already_used.clone();
    for length in 3..=6 {
        let start_count = candidates.len();
        for initial in 0..26 {
            for _ in 0..24 {
                if let Some(word) = generate_markov_word(model, length, Some(initial), rng) {
                    add_candidate(word, model, layout, &physical, &mut seen, &mut candidates);
                }
            }
        }
        let mut attempts = 0;
        while candidates.len() - start_count < PART_B_CANDIDATES_PER_LENGTH && attempts < PART_B_CANDIDATES_PER_LENGTH * 100 {
            attempts += 1;
            if let Some(word) = generate_markov_word(model, length, None, rng) {
                add_candidate(word, model, layout, &physical, &mut seen, &mut candidates);
            }
        }
    }
    candidates
}

fn generate_markov_word(model: &MarkovModel, length: usize, initial: Option<usize>, rng: &mut StdRng) -> Option<String> {
    let mut current = initial.or_else(|| weighted_index(&model.starts, rng))?;
    let mut bytes = Vec::with_capacity(length);
    bytes.push(b'a' + current as u8);
    for _ in 1..length {
        current = weighted_index(&model.transitions[current], rng)?;
        bytes.push(b'a' + current as u8);
    }
    String::from_utf8(bytes).ok()
}

fn weighted_index(weights: &[usize; 26], rng: &mut StdRng) -> Option<usize> {
    let total = weights.iter().sum::<usize>();
    if total == 0 {
        return None;
    }
    let mut draw = rng.gen_range(0..total);
    for (index, weight) in weights.iter().enumerate() {
        if draw < *weight {
            return Some(index);
        }
        draw -= weight;
    }
    None
}

fn add_candidate(
    word: String,
    model: &MarkovModel,
    layout: &KeyboardLayout,
    physical: &HashMap<&str, Finger>,
    seen: &mut HashSet<String>,
    candidates: &mut Vec<PseudowordCandidate>,
) {
    if model.real_words.contains(&word) || is_periodic(&word) || !seen.insert(word.clone()) {
        return;
    }
    let bigrams = word
        .as_bytes()
        .windows(2)
        .map(|pair| (pair[0] - b'a', pair[1] - b'a'))
        .collect::<Vec<_>>();
    if bigrams.iter().copied().collect::<HashSet<_>>().len() != bigrams.len() {
        return;
    }
    let mut character_counts = [0; 26];
    for letter in word.bytes() {
        character_counts[usize::from(letter - b'a')] += 1;
    }
    let mut class_counts = [0; 4];
    for &(from, to) in &bigrams {
        let Some(from_finger) = layout
            .stroke_for(char::from(b'a' + from))
            .and_then(|stroke| physical.get(stroke.code.as_str()).copied())
        else {
            continue;
        };
        let Some(to_finger) = layout
            .stroke_for(char::from(b'a' + to))
            .and_then(|stroke| physical.get(stroke.code.as_str()).copied())
        else {
            continue;
        };
        class_counts[class_index(classify(from_finger, to_finger))] += 1;
    }
    candidates.push(PseudowordCandidate {
        word,
        character_counts,
        class_counts,
        bigrams,
    });
}

fn is_periodic(word: &str) -> bool {
    let bytes = word.as_bytes();
    (1..=3).any(|period| {
        bytes.len() > period && bytes.len().is_multiple_of(period) && bytes.iter().enumerate().all(|(index, byte)| *byte == bytes[index % period])
    })
}

fn select_pseudowords(candidates: Vec<PseudowordCandidate>, character_shares: &[f64; 26], class_shares: &[f64; 4]) -> Vec<BenchPrompt> {
    let mut state = SelectionState::default();
    let mut selected = Vec::with_capacity(PART_B_WORDS_PER_LENGTH * 4);
    for round in 0..PART_B_WORDS_PER_LENGTH {
        for length in 3..=6 {
            let best = candidates
                .iter()
                .filter(|candidate| candidate.word.len() == length && !state.selected_words.contains(&candidate.word))
                .max_by(|left, right| {
                    candidate_score(left, &state, character_shares, class_shares)
                        .total_cmp(&candidate_score(right, &state, character_shares, class_shares))
                        .then_with(|| right.word.cmp(&left.word))
                })
                .unwrap_or_else(|| panic!("not enough part B candidates for length {length} in round {round}"));
            apply_candidate(best, &mut state);
            selected.push(BenchPrompt {
                text: best.word.clone(),
                reading: None,
                target: best.word.clone(),
            });
        }
    }
    selected
}

fn candidate_score(candidate: &PseudowordCandidate, state: &SelectionState, character_shares: &[f64; 26], class_shares: &[f64; 4]) -> f64 {
    let character_coverage = (0..26)
        .map(|index| candidate.character_counts[index].min(3usize.saturating_sub(state.character_counts[index])))
        .sum::<usize>();
    let class_coverage = (0..4)
        .map(|index| candidate.class_counts[index].min(15usize.saturating_sub(state.class_counts[index])))
        .sum::<usize>();
    let new_bigrams = candidate.bigrams.iter().filter(|bigram| !state.bigrams.contains(bigram)).count();
    let character_total = state.character_counts.iter().sum::<usize>() + candidate.word.len();
    let character_error = (0..26)
        .map(|index| {
            let share = (state.character_counts[index] + candidate.character_counts[index]) as f64 / character_total as f64;
            (share - character_shares[index]).powi(2)
        })
        .sum::<f64>();
    let class_total = state.class_counts.iter().sum::<usize>() + candidate.bigrams.len();
    let class_error = (0..4)
        .map(|index| {
            let share = (state.class_counts[index] + candidate.class_counts[index]) as f64 / class_total as f64;
            (share - class_shares[index]).powi(2)
        })
        .sum::<f64>();
    let diversity_bonus = if state.bigrams.len() < 120 { 100_000_000.0 } else { 10_000.0 };
    character_coverage as f64 * 1_000_000_000_000.0 + class_coverage as f64 * 10_000_000_000.0 + new_bigrams as f64 * diversity_bonus
        - character_error * 1_000_000_000.0
        - class_error * 500_000_000.0
}

fn apply_candidate(candidate: &PseudowordCandidate, state: &mut SelectionState) {
    for index in 0..26 {
        state.character_counts[index] += candidate.character_counts[index];
    }
    for index in 0..4 {
        state.class_counts[index] += candidate.class_counts[index];
    }
    state.bigrams.extend(candidate.bigrams.iter().copied());
    state.selected_words.insert(candidate.word.clone());
}

fn source_class_shares(source_words: &[String], layout: &KeyboardLayout, geometry: &KeyboardGeometry) -> [f64; 4] {
    let physical = physical_fingers(geometry);
    let mut counts = [0; 4];
    for word in source_words {
        for pair in word.as_bytes().windows(2) {
            let Some(from) = layout
                .stroke_for(char::from(pair[0]))
                .and_then(|stroke| physical.get(stroke.code.as_str()).copied())
            else {
                continue;
            };
            let Some(to) = layout
                .stroke_for(char::from(pair[1]))
                .and_then(|stroke| physical.get(stroke.code.as_str()).copied())
            else {
                continue;
            };
            counts[class_index(classify(from, to))] += 1;
        }
    }
    let total = counts.iter().sum::<usize>() as f64;
    counts.map(|count| count as f64 / total)
}

fn physical_fingers(geometry: &KeyboardGeometry) -> HashMap<&str, Finger> {
    geometry.rows.iter().flatten().map(|key| (key.code, key.finger)).collect()
}

fn class_index(class: TransitionClass) -> usize {
    match class {
        TransitionClass::SameFinger => 0,
        TransitionClass::SameHandAdjacent => 1,
        TransitionClass::SameHandNonAdjacent => 2,
        TransitionClass::AlternatingHands => 3,
    }
}

fn count_transition_classes(prompts: &[BenchPrompt], layout: &KeyboardLayout, geometry: &KeyboardGeometry) -> BTreeMap<TransitionClass, usize> {
    let physical = geometry
        .rows
        .iter()
        .flatten()
        .map(|key| (key.code, key.finger))
        .collect::<HashMap<_, _>>();
    let mut counts = BTreeMap::new();
    for prompt in prompts {
        let characters = prompt.target.chars().collect::<Vec<_>>();
        for pair in characters.windows(2) {
            let Some(from) = layout.stroke_for(pair[0]).and_then(|stroke| physical.get(stroke.code.as_str()).copied()) else {
                continue;
            };
            let Some(to) = layout.stroke_for(pair[1]).and_then(|stroke| physical.get(stroke.code.as_str()).copied()) else {
                continue;
            };
            *counts.entry(classify(from, to)).or_default() += 1;
        }
    }
    counts
}

fn classify(from: Finger, to: Finger) -> TransitionClass {
    if from == to {
        return TransitionClass::SameFinger;
    }
    if hand(from) != hand(to) {
        return TransitionClass::AlternatingHands;
    }
    if finger_index(from).abs_diff(finger_index(to)) == 1 {
        TransitionClass::SameHandAdjacent
    } else {
        TransitionClass::SameHandNonAdjacent
    }
}

fn hand(finger: Finger) -> u8 {
    match finger {
        Finger::LeftPinky | Finger::LeftRing | Finger::LeftMiddle | Finger::LeftIndex => 0,
        Finger::RightIndex | Finger::RightMiddle | Finger::RightRing | Finger::RightPinky => 1,
        Finger::Thumb => 2,
    }
}

fn finger_index(finger: Finger) -> u8 {
    match finger {
        Finger::LeftPinky | Finger::RightPinky => 0,
        Finger::LeftRing | Finger::RightRing => 1,
        Finger::LeftMiddle | Finger::RightMiddle => 2,
        Finger::LeftIndex | Finger::RightIndex => 3,
        Finger::Thumb => 4,
    }
}
