//! Fixed, deterministic benchmark parallel forms.

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

pub const BENCH_SERIES_V1: &str = "bench-v1";
pub const BENCH_SERIES_V2: &str = "bench-v2";
pub const BENCH_SERIES: &str = BENCH_SERIES_V2;
pub const V2_SELECTION_RULE: &str = "sha256-v1";
pub const PARALLEL_VERSION_COUNT: u8 = 8;
const PART_B_SOURCE_WORDS: usize = 5_000;
const PART_B_WORDS_PER_LENGTH: usize = 60;
const PART_B_CANDIDATES_PER_LENGTH: usize = 800;
const SHA256_INITIAL_STATE: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];
const SHA256_ROUND_CONSTANTS: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

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
    pub best_wpm_delta: Option<f64>,
    pub best_accuracy_delta: Option<f64>,
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

pub fn parse_bench_id(id: &str) -> Option<(&str, BenchPart, u8)> {
    let mut fields = id.split('/');
    let series = fields.next()?;
    if !matches!(series, BENCH_SERIES_V1 | BENCH_SERIES_V2) {
        return None;
    }
    let part = match fields.next()? {
        "A" => BenchPart::A,
        "B" => BenchPart::B,
        "C" => BenchPart::C,
        "D" => BenchPart::D,
        _ => return None,
    };
    let parallel_index = fields.next()?.parse().ok()?;
    (fields.next().is_none() && (1..=PARALLEL_VERSION_COUNT).contains(&parallel_index)).then_some((series, part, parallel_index))
}

pub fn preferred_bench_series<'a>(ids: impl Iterator<Item = &'a str>, part: BenchPart) -> Option<&'static str> {
    let mut has_v1 = false;
    for id in ids {
        match parse_bench_id(id) {
            Some((BENCH_SERIES_V2, candidate, _)) if candidate == part => return Some(BENCH_SERIES_V2),
            Some((BENCH_SERIES_V1, candidate, _)) if candidate == part => has_v1 = true,
            _ => {}
        }
    }
    has_v1.then_some(BENCH_SERIES_V1)
}

/// Returns whether a record belongs to the comparison series selected for a part.
///
/// Parts B and D have identical prompts in bench-v1 and bench-v2, so both
/// versions form one continuous series. Parts A and C changed in v2 and keep
/// the newest available version as a separate series.
pub fn is_preferred_bench_series(candidate: &str, part: BenchPart, preferred: Option<&str>) -> bool {
    preferred.is_some()
        && if matches!(part, BenchPart::B | BenchPart::D) {
            matches!(candidate, BENCH_SERIES_V1 | BENCH_SERIES_V2)
        } else {
            Some(candidate) == preferred
        }
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
                id: format!("{BENCH_SERIES_V1}/{}/{parallel_index:02}", part.as_str()),
                part,
                parallel_index,
                settings: BenchSettings::fixed(part, &Config::default()),
                prompts,
                transition_class_counts,
            }
        })
        .collect()
}

pub fn generate_parallel_versions_for_series(
    series: &str,
    part: BenchPart,
    data: &DataCatalog,
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
) -> Vec<BenchVersion> {
    match series {
        BENCH_SERIES_V1 => generate_parallel_versions(part, data, layout, geometry),
        BENCH_SERIES_V2 if matches!(part, BenchPart::B | BenchPart::D) => generate_parallel_versions(part, data, layout, geometry)
            .into_iter()
            .map(|mut version| {
                version.id = format!("{BENCH_SERIES_V2}/{}/{:02}", part.as_str(), version.parallel_index);
                version
            })
            .collect(),
        BENCH_SERIES_V2 => generate_v2_staged_versions(part, data, layout, geometry),
        _ => Vec::new(),
    }
}

fn generate_v2_staged_versions(part: BenchPart, data: &DataCatalog, layout: &KeyboardLayout, geometry: &KeyboardGeometry) -> Vec<BenchVersion> {
    let stages = v2_stages(part, data);
    (1..=PARALLEL_VERSION_COUNT)
        .map(|parallel_index| {
            let mut prompts = Vec::new();
            for (stage_index, stage) in stages.iter().enumerate() {
                let mut stage = stage.clone();
                let seed = 0xB3EC_2000_D15C_A11Eu64 ^ ((part as u64) << 24) ^ ((stage_index as u64) << 8) ^ u64::from(parallel_index);
                stage.shuffle(&mut StdRng::seed_from_u64(seed));
                prompts.extend(stage);
            }
            let transition_class_counts = count_transition_classes(&prompts, layout, geometry);
            BenchVersion {
                id: format!("{BENCH_SERIES_V2}/{}/{parallel_index:02}", part.as_str()),
                part,
                parallel_index,
                settings: BenchSettings::fixed(part, &Config::default()),
                prompts,
                transition_class_counts,
            }
        })
        .collect()
}

fn v2_stages(part: BenchPart, data: &DataCatalog) -> Vec<Vec<BenchPrompt>> {
    match part {
        BenchPart::A => vec![
            english_word_stage(data, 3, 5, 15),
            english_word_stage(data, 6, 8, 15),
            english_phrase_stage(data, 10, 15, 12),
            english_phrase_stage(data, 16, 22, 12),
            english_phrase_stage(data, 23, 29, 12),
        ],
        BenchPart::C => vec![
            japanese_word_stage(data, 2, 4, 15),
            japanese_word_stage(data, 5, 8, 15),
            japanese_phrase_stage(data, 6, 10, 12),
            japanese_phrase_stage(data, 11, 15, 12),
            japanese_phrase_stage(data, 16, 20, 12),
        ],
        BenchPart::B | BenchPart::D => unreachable!("v2 B and D reuse v1 forms"),
    }
}

fn english_word_stage(data: &DataCatalog, minimum: usize, maximum: usize, count: usize) -> Vec<BenchPrompt> {
    let mut rows = data.fixed_items(DataKind::EnglishWord);
    rows.sort_by_key(|row| row.rank.unwrap_or(usize::MAX));
    rows.into_iter()
        .filter(|row| (minimum..=maximum).contains(&row.text.chars().count()))
        .take(count)
        .map(|row| BenchPrompt {
            text: row.text.clone(),
            reading: None,
            target: row.text.clone(),
        })
        .collect()
}

fn english_phrase_stage(data: &DataCatalog, minimum: usize, maximum: usize, count: usize) -> Vec<BenchPrompt> {
    let mut words = data.fixed_items(DataKind::EnglishWord);
    words.sort_by_key(|row| row.rank.unwrap_or(usize::MAX));
    let common = words
        .into_iter()
        .take(3_000)
        .map(|row| row.text.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let mut rows = data
        .fixed_items(DataKind::EnglishPhrase)
        .into_iter()
        .filter(|row| row.tier == Tier::Core)
        .filter(|row| (minimum..=maximum).contains(&row.text.chars().count()))
        .filter(|row| {
            row.text
                .split(|character: char| !character.is_ascii_alphabetic())
                .filter(|word| !word.is_empty())
                .all(|word| common.contains(&word.to_ascii_lowercase()))
        })
        .map(|row| (sha256(&row.text), row))
        .collect::<Vec<_>>();
    rows.sort_by(|(left_hash, left), (right_hash, right)| left_hash.cmp(right_hash).then_with(|| left.text.cmp(&right.text)));
    rows.into_iter()
        .take(count)
        .map(|(_, row)| BenchPrompt {
            text: row.text.clone(),
            reading: None,
            target: row.text.clone(),
        })
        .collect()
}

fn japanese_word_stage(data: &DataCatalog, minimum: usize, maximum: usize, count: usize) -> Vec<BenchPrompt> {
    let mut rows = data
        .fixed_items(DataKind::JapaneseWord)
        .into_iter()
        .filter(|row| row.tier == Tier::Core)
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| row.rank.unwrap_or(usize::MAX));
    rows.into_iter()
        .filter(|row| {
            row.reading
                .as_deref()
                .is_some_and(|reading| (minimum..=maximum).contains(&reading.chars().count()))
        })
        .take(count)
        .filter_map(japanese_prompt)
        .collect()
}

fn japanese_phrase_stage(data: &DataCatalog, minimum: usize, maximum: usize, count: usize) -> Vec<BenchPrompt> {
    let mut rows = data
        .fixed_items(DataKind::JapanesePhrase)
        .into_iter()
        .filter(|row| {
            row.reading
                .as_deref()
                .is_some_and(|reading| (minimum..=maximum).contains(&reading.chars().count()))
        })
        .map(|row| (sha256(row.reading.as_deref().unwrap_or_default()), row))
        .collect::<Vec<_>>();
    rows.sort_by(|(left_hash, left), (right_hash, right)| {
        let left_reading = left.reading.as_deref().unwrap_or_default();
        let right_reading = right.reading.as_deref().unwrap_or_default();
        left_hash
            .cmp(right_hash)
            .then_with(|| left.text.cmp(&right.text))
            .then_with(|| left_reading.cmp(right_reading))
    });
    rows.into_iter().take(count).filter_map(|(_, row)| japanese_prompt(row)).collect()
}

fn sha256(value: &str) -> [u8; 32] {
    sha256_digest(value.as_bytes())
}

/// SHA-256 of `bytes` (FIPS 180-4). Also used by the tests to check the
/// bundled data against `data/manifest.toml` without another dependency.
pub fn sha256_digest(bytes: &[u8]) -> [u8; 32] {
    let mut padded = bytes.to_vec();
    let bit_length = (padded.len() as u64).wrapping_mul(8);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_length.to_be_bytes());

    let mut state = SHA256_INITIAL_STATE;
    for block in padded.as_chunks::<64>().0 {
        let mut words = [0_u32; 64];
        for (index, bytes) in block.as_chunks::<4>().0.iter().enumerate() {
            words[index] = u32::from_be_bytes(*bytes);
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7) ^ words[index - 15].rotate_right(18) ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17) ^ words[index - 2].rotate_right(19) ^ (words[index - 2] >> 10);
            words[index] = words[index - 16].wrapping_add(s0).wrapping_add(words[index - 7]).wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for (constant, word) in SHA256_ROUND_CONSTANTS.into_iter().zip(words) {
            let sum1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ (!e & g);
            let temporary1 = h.wrapping_add(sum1).wrapping_add(choice).wrapping_add(constant).wrapping_add(word);
            let sum0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temporary2 = sum0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temporary1);
            d = c;
            c = b;
            b = a;
            a = temporary1.wrapping_add(temporary2);
        }
        for (target, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *target = target.wrapping_add(value);
        }
    }

    let mut digest = [0_u8; 32];
    for (chunk, word) in digest.as_chunks_mut::<4>().0.iter_mut().zip(state) {
        chunk.copy_from_slice(&word.to_be_bytes());
    }
    digest
}

fn japanese_prompt(row: &crate::data::DataItem) -> Option<BenchPrompt> {
    let reading = row.reading.clone()?;
    Some(BenchPrompt {
        text: row.text.clone(),
        target: RomajiJudge::new(&reading).guide().to_owned(),
        reading: Some(reading),
    })
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
                id: format!("{BENCH_SERIES_V1}/B/{parallel_index:02}"),
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
