use rand::SeedableRng;
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

use super::{
    BenchPrompt, PARALLEL_VERSION_COUNT, generate_candidates, japanese_prompt, markov_model_from_core_items, select_english_by_sha,
    select_japanese_by_sha, select_pseudowords, source_class_shares,
};
use crate::config::ErrorMode;
use crate::data::{DataCatalog, DataItem, DataKind, Tier};
use crate::layout::KeyboardLayout;
use crate::layout::geometry::KeyboardGeometry;

pub const BENCH_V3_SERIES: &str = "bench-v3";

const W_COUNT: usize = 45;
const EW_COUNTS: [usize; 3] = [88, 56, 38];
const ES_COUNT: usize = 28;
const JW_COUNTS: [usize; 4] = [61, 24, 20, 14];
const JS_COUNT: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum BenchV3Part {
    W,
    EW,
    ES,
    JW,
    JS,
    PW,
}

impl BenchV3Part {
    pub const ALL: [Self; 6] = [Self::W, Self::EW, Self::ES, Self::JW, Self::JS, Self::PW];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::W => "W",
            Self::EW => "EW",
            Self::ES => "ES",
            Self::JW => "JW",
            Self::JS => "JS",
            Self::PW => "PW",
        }
    }

    pub const fn next(self) -> Option<Self> {
        match self {
            Self::W => Some(Self::EW),
            Self::EW => Some(Self::ES),
            Self::ES => Some(Self::JW),
            Self::JW => Some(Self::JS),
            Self::JS => Some(Self::PW),
            Self::PW => None,
        }
    }

    pub const fn language(self) -> &'static str {
        match self {
            Self::JW | Self::JS => "ja",
            Self::W | Self::EW | Self::ES | Self::PW => "en",
        }
    }

    pub const fn duration_seconds(self) -> u64 {
        match self {
            Self::W => 15,
            Self::PW => 30,
            Self::EW | Self::ES | Self::JW | Self::JS => 60,
        }
    }

    pub const fn minimum_keystrokes(self) -> usize {
        match self {
            Self::W => 282,
            Self::PW => 563,
            Self::EW | Self::ES | Self::JW | Self::JS => 1_125,
        }
    }

    const fn seed_tag(self) -> u64 {
        match self {
            Self::W => 0,
            Self::EW => 1,
            Self::ES => 2,
            Self::JW => 3,
            Self::JS => 4,
            Self::PW => 5,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BenchV3ResultSummary {
    pub id: String,
    pub part: BenchV3Part,
    pub wpm: f64,
    pub accuracy: f64,
    pub comparison: super::BenchComparison,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BenchV3Resume {
    pub part: BenchV3Part,
    pub parallel_index: u8,
    pub completed: Vec<BenchV3ResultSummary>,
}

pub fn parse_v3_bench_id(id: &str) -> Option<(BenchV3Part, u8)> {
    let mut fields = id.split('/');
    if fields.next()? != BENCH_V3_SERIES {
        return None;
    }
    let part = match fields.next()? {
        "W" => BenchV3Part::W,
        "EW" => BenchV3Part::EW,
        "ES" => BenchV3Part::ES,
        "JW" => BenchV3Part::JW,
        "JS" => BenchV3Part::JS,
        "PW" => BenchV3Part::PW,
        _ => return None,
    };
    let parallel_field = fields.next()?;
    if parallel_field.len() != 2 || !parallel_field.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let parallel_index = parallel_field.parse().ok()?;
    (fields.next().is_none() && (1..=PARALLEL_VERSION_COUNT).contains(&parallel_index)).then_some((part, parallel_index))
}

pub fn is_v3_bench_id(id: &str) -> bool {
    parse_v3_bench_id(id).is_some()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum BenchV3StageKind {
    Warmup,
    Ew3To5,
    Ew6To8,
    Ew9OrMore,
    EnglishSentence,
    JwWord2To4,
    JwWord5To8,
    JwPhrase6To10,
    JwPhrase11To15,
    JapaneseSentence,
    Pseudoword,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchV3Stage {
    pub kind: BenchV3StageKind,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchV3Version {
    pub id: String,
    pub part: BenchV3Part,
    pub parallel_index: u8,
    pub duration_seconds: u64,
    pub item_timeout_seconds: u64,
    pub error_mode: ErrorMode,
    pub preview_items: usize,
    pub prompts: Vec<BenchPrompt>,
    pub stages: Vec<BenchV3Stage>,
}

pub fn generate_v3_parallel_versions(
    part: BenchV3Part,
    data: &DataCatalog,
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
) -> Vec<BenchV3Version> {
    generate_v3_parallel_versions_from_items(part, data.all_fixed_items(), layout, geometry)
}

pub fn generate_v3_parallel_versions_from_items(
    part: BenchV3Part,
    items: &[DataItem],
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
) -> Vec<BenchV3Version> {
    let base_stages = base_stages(part, items, layout, geometry);
    (1..=PARALLEL_VERSION_COUNT)
        .map(|parallel_index| build_version(part, parallel_index, &base_stages))
        .collect()
}

fn base_stages(
    part: BenchV3Part,
    items: &[DataItem],
    layout: &KeyboardLayout,
    geometry: &KeyboardGeometry,
) -> Vec<(BenchV3StageKind, Vec<BenchPrompt>)> {
    match part {
        BenchV3Part::W => vec![(BenchV3StageKind::Warmup, warmup(items))],
        BenchV3Part::EW => vec![
            (BenchV3StageKind::Ew3To5, english_words(items, 3, Some(5), EW_COUNTS[0])),
            (BenchV3StageKind::Ew6To8, english_words(items, 6, Some(8), EW_COUNTS[1])),
            (BenchV3StageKind::Ew9OrMore, english_words(items, 9, None, EW_COUNTS[2])),
        ],
        BenchV3Part::ES => vec![(BenchV3StageKind::EnglishSentence, english_sentences(items))],
        BenchV3Part::JW => vec![
            (
                BenchV3StageKind::JwWord2To4,
                japanese_rows(items, DataKind::JapaneseWord, 2, 4, JW_COUNTS[0]),
            ),
            (
                BenchV3StageKind::JwWord5To8,
                japanese_rows(items, DataKind::JapaneseWord, 5, 8, JW_COUNTS[1]),
            ),
            (
                BenchV3StageKind::JwPhrase6To10,
                japanese_rows(items, DataKind::JapanesePhrase, 6, 10, JW_COUNTS[2]),
            ),
            (
                BenchV3StageKind::JwPhrase11To15,
                japanese_rows(items, DataKind::JapanesePhrase, 11, 15, JW_COUNTS[3]),
            ),
        ],
        BenchV3Part::JS => vec![(BenchV3StageKind::JapaneseSentence, japanese_sentences(items))],
        BenchV3Part::PW => vec![(BenchV3StageKind::Pseudoword, pseudowords(items, layout, geometry))],
    }
}

fn warmup(items: &[DataItem]) -> Vec<BenchPrompt> {
    select_english_by_sha(core_rows(items, DataKind::EnglishWord), W_COUNT)
        .into_iter()
        .map(english_prompt)
        .collect()
}

fn english_words(items: &[DataItem], minimum: usize, maximum: Option<usize>, count: usize) -> Vec<BenchPrompt> {
    let rows = core_rows(items, DataKind::EnglishWord).filter(|row| {
        let length = row.text.chars().count();
        length >= minimum && maximum.is_none_or(|maximum| length <= maximum)
    });
    select_english_by_sha(rows, count).into_iter().map(english_prompt).collect()
}

fn english_sentences(items: &[DataItem]) -> Vec<BenchPrompt> {
    let rows = core_rows(items, DataKind::EnglishSentence).filter(|row| (30..=90).contains(&row.text.chars().count()));
    select_english_by_sha(rows, ES_COUNT).into_iter().map(english_prompt).collect()
}

fn japanese_rows(items: &[DataItem], kind: DataKind, minimum: usize, maximum: usize, count: usize) -> Vec<BenchPrompt> {
    let rows = core_rows(items, kind).filter(|row| {
        row.reading
            .as_deref()
            .is_some_and(|reading| (minimum..=maximum).contains(&reading.chars().count()))
    });
    select_japanese_by_sha(rows, count).into_iter().filter_map(japanese_prompt).collect()
}

fn japanese_sentences(items: &[DataItem]) -> Vec<BenchPrompt> {
    select_japanese_by_sha(core_rows(items, DataKind::JapaneseSentence), JS_COUNT)
        .into_iter()
        .filter_map(japanese_prompt)
        .collect()
}

fn pseudowords(items: &[DataItem], layout: &KeyboardLayout, geometry: &KeyboardGeometry) -> Vec<BenchPrompt> {
    let model = markov_model_from_core_items(items);
    let class_shares = source_class_shares(&model.source_words, layout, geometry);
    let mut rng = StdRng::seed_from_u64(0xB3EC_3000_D15C_A11E);
    let candidates = generate_candidates(&model, layout, geometry, &Default::default(), &mut rng);
    select_pseudowords(candidates, &model.character_shares, &class_shares)
}

fn core_rows(items: &[DataItem], kind: DataKind) -> impl Iterator<Item = &DataItem> {
    items.iter().filter(move |row| row.kind == kind && row.tier == Tier::Core)
}

fn english_prompt(row: &DataItem) -> BenchPrompt {
    BenchPrompt {
        text: row.text.clone(),
        reading: None,
        target: row.text.clone(),
    }
}

fn build_version(part: BenchV3Part, parallel_index: u8, base_stages: &[(BenchV3StageKind, Vec<BenchPrompt>)]) -> BenchV3Version {
    let mut prompts = Vec::new();
    let mut stages = Vec::with_capacity(base_stages.len());
    for (stage_index, (kind, base)) in base_stages.iter().enumerate() {
        let mut stage_prompts = base.clone();
        if part != BenchV3Part::W {
            let seed = 0xB3EC_3000_D15C_A11Eu64 ^ (part.seed_tag() << 32) ^ ((stage_index as u64) << 16) ^ u64::from(parallel_index);
            stage_prompts.shuffle(&mut StdRng::seed_from_u64(seed));
        }
        let start = prompts.len();
        prompts.extend(stage_prompts);
        stages.push(BenchV3Stage {
            kind: *kind,
            start,
            end: prompts.len(),
        });
    }
    BenchV3Version {
        id: format!("{BENCH_V3_SERIES}/{}/{parallel_index:02}", part.as_str()),
        part,
        parallel_index,
        duration_seconds: part.duration_seconds(),
        item_timeout_seconds: 0,
        error_mode: ErrorMode::Stop,
        preview_items: 1,
        prompts,
        stages,
    }
}
