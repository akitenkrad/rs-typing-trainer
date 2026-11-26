use std::collections::{BTreeMap, BTreeSet, HashSet};

use rs_typing_trainer::bench::{
    BENCH_SERIES, BENCH_SERIES_V2, BENCH_V3_SERIES, BenchPrompt, BenchV3Part, BenchV3StageKind, BenchV3Version, PARALLEL_VERSION_COUNT,
    V2_SELECTION_RULE, generate_v3_parallel_versions, generate_v3_parallel_versions_from_items,
};
use rs_typing_trainer::config::{DataConfig, ErrorMode};
use rs_typing_trainer::data::{DataCatalog, DataKind, Tier};
use rs_typing_trainer::engine::romaji::RomajiJudge;
use rs_typing_trainer::layout::builtin_layout;
use rs_typing_trainer::layout::geometry::geometry;
use tempfile::TempDir;

type PromptKey = (String, Option<String>, String);
type PromptMultiset = BTreeMap<PromptKey, usize>;

fn catalog() -> (TempDir, DataCatalog) {
    let home = TempDir::new().unwrap();
    let data = DataCatalog::load(&DataConfig::default(), home.path()).unwrap();
    (home, data)
}

fn prompt_key(prompt: &BenchPrompt) -> PromptKey {
    (prompt.text.clone(), prompt.reading.clone(), prompt.target.clone())
}

fn prompt_multiset(prompts: &[BenchPrompt]) -> PromptMultiset {
    prompts.iter().fold(BTreeMap::new(), |mut counts, prompt| {
        *counts.entry(prompt_key(prompt)).or_default() += 1;
        counts
    })
}

fn stage_multisets(version: &BenchV3Version) -> Vec<(BenchV3StageKind, PromptMultiset)> {
    version
        .stages
        .iter()
        .map(|stage| (stage.kind, prompt_multiset(&version.prompts[stage.start..stage.end])))
        .collect()
}

fn is_periodic(word: &str) -> bool {
    let bytes = word.as_bytes();
    (1..=3).any(|period| {
        bytes.len() > period && bytes.len().is_multiple_of(period) && bytes.iter().enumerate().all(|(index, byte)| *byte == bytes[index % period])
    })
}

fn has_repeated_bigram(word: &str) -> bool {
    let bigrams = word.as_bytes().windows(2).map(|pair| (pair[0], pair[1])).collect::<Vec<_>>();
    bigrams.len() != bigrams.iter().copied().collect::<HashSet<_>>().len()
}

fn minimum_keystrokes(prompt: &BenchPrompt) -> usize {
    prompt.reading.as_deref().map_or_else(
        || prompt.target.chars().count(),
        |reading| {
            RomajiJudge::new(reading)
                .spelling_variants()
                .into_iter()
                .map(|(_, variants)| variants.into_iter().map(|variant| variant.chars().count()).min().unwrap())
                .sum()
        },
    )
}

#[test]
fn bench_v3_is_generated_without_changing_the_active_series() {
    assert_eq!(BENCH_SERIES, BENCH_SERIES_V2);
    assert_eq!(BENCH_V3_SERIES, "bench-v3");
    assert_eq!(V2_SELECTION_RULE, "sha256-v1");

    let (_home, data) = catalog();
    for part in BenchV3Part::ALL {
        let versions = generate_v3_parallel_versions(part, &data, &builtin_layout("qwerty").unwrap(), &geometry("ansi").unwrap());
        assert_eq!(versions.len(), usize::from(PARALLEL_VERSION_COUNT));
        for (index, version) in versions.iter().enumerate() {
            assert_eq!(version.id, format!("bench-v3/{}/{:02}", part.as_str(), index + 1));
            assert_eq!(version.part, part);
            assert_eq!(version.parallel_index, u8::try_from(index + 1).unwrap());
            assert_eq!(version.duration_seconds, part.duration_seconds());
            assert_eq!(version.error_mode, ErrorMode::Stop);
            assert_eq!(version.preview_items, 1);
        }
    }
}

#[test]
fn bench_v3_forms_keep_the_same_multisets_and_only_shuffle_within_stages() {
    let (_home, data) = catalog();
    let layout = builtin_layout("qwerty").unwrap();
    let keyboard = geometry("ansi").unwrap();

    for part in BenchV3Part::ALL {
        let versions = generate_v3_parallel_versions(part, &data, &layout, &keyboard);
        let base_multiset = prompt_multiset(&versions[0].prompts);
        let base_stages = stage_multisets(&versions[0]);
        for version in &versions[1..] {
            assert_eq!(
                prompt_multiset(&version.prompts),
                base_multiset,
                "{} changed its prompt multiset",
                version.id
            );
            assert_eq!(
                stage_multisets(version),
                base_stages,
                "{} changed a stage multiset or stage order",
                version.id
            );
        }

        let distinct_orders = versions
            .iter()
            .map(|version| version.prompts.iter().map(prompt_key).collect::<Vec<_>>())
            .collect::<BTreeSet<_>>()
            .len();
        if part == BenchV3Part::W {
            assert_eq!(distinct_orders, 1, "warm-up order may be shared but must be deterministic");
        } else {
            assert!(distinct_orders >= 7, "{part:?} has only {distinct_orders} distinct orders");
        }
    }
}

#[test]
fn bench_v3_staged_parts_keep_short_to_long_stage_boundaries() {
    let (_home, data) = catalog();
    let layout = builtin_layout("qwerty").unwrap();
    let keyboard = geometry("ansi").unwrap();

    for version in generate_v3_parallel_versions(BenchV3Part::EW, &data, &layout, &keyboard) {
        assert_eq!(
            version.stages.iter().map(|stage| stage.kind).collect::<Vec<_>>(),
            [BenchV3StageKind::Ew3To5, BenchV3StageKind::Ew6To8, BenchV3StageKind::Ew9OrMore]
        );
        assert_eq!(
            version.stages.iter().map(|stage| stage.end - stage.start).collect::<Vec<_>>(),
            [88, 56, 38]
        );
        for stage in &version.stages {
            assert!(version.prompts[stage.start..stage.end].iter().all(|prompt| match stage.kind {
                BenchV3StageKind::Ew3To5 => (3..=5).contains(&prompt.text.chars().count()),
                BenchV3StageKind::Ew6To8 => (6..=8).contains(&prompt.text.chars().count()),
                BenchV3StageKind::Ew9OrMore => prompt.text.chars().count() >= 9,
                _ => false,
            }));
        }
    }

    for version in generate_v3_parallel_versions(BenchV3Part::JW, &data, &layout, &keyboard) {
        assert_eq!(
            version.stages.iter().map(|stage| stage.kind).collect::<Vec<_>>(),
            [
                BenchV3StageKind::JwWord2To4,
                BenchV3StageKind::JwWord5To8,
                BenchV3StageKind::JwPhrase6To10,
                BenchV3StageKind::JwPhrase11To15,
            ]
        );
        assert_eq!(
            version.stages.iter().map(|stage| stage.end - stage.start).collect::<Vec<_>>(),
            [61, 24, 20, 14]
        );
        for stage in &version.stages {
            assert!(version.prompts[stage.start..stage.end].iter().all(|prompt| {
                let length = prompt.reading.as_deref().unwrap().chars().count();
                match stage.kind {
                    BenchV3StageKind::JwWord2To4 => (2..=4).contains(&length),
                    BenchV3StageKind::JwWord5To8 => (5..=8).contains(&length),
                    BenchV3StageKind::JwPhrase6To10 => (6..=10).contains(&length),
                    BenchV3StageKind::JwPhrase11To15 => (11..=15).contains(&length),
                    _ => false,
                }
            }));
        }
    }
}

#[test]
fn bench_v3_each_part_has_a_fifty_percent_keystroke_margin() {
    let (_home, data) = catalog();
    let layout = builtin_layout("qwerty").unwrap();
    let keyboard = geometry("ansi").unwrap();
    for part in BenchV3Part::ALL {
        let version = &generate_v3_parallel_versions(part, &data, &layout, &keyboard)[0];
        let keystrokes = version.prompts.iter().map(minimum_keystrokes).sum::<usize>();
        assert!(
            keystrokes >= part.minimum_keystrokes(),
            "{part:?} has {keystrokes} keystrokes, below {}",
            part.minimum_keystrokes()
        );
    }
}

#[test]
fn bench_v3_stage_counts_and_minimum_keystrokes_are_fixed() {
    let (_home, data) = catalog();
    let layout = builtin_layout("qwerty").unwrap();
    let keyboard = geometry("ansi").unwrap();
    let expected = [
        (BenchV3Part::W, vec![(45, 283)]),
        (BenchV3Part::EW, vec![(88, 379), (56, 380), (38, 376)]),
        (BenchV3Part::ES, vec![(28, 1_137)]),
        (BenchV3Part::JW, vec![(61, 313), (24, 199), (20, 309), (14, 306)]),
        (BenchV3Part::JS, vec![(20, 1_164)]),
        (BenchV3Part::PW, vec![(240, 1_080)]),
    ];

    for (part, expected_stages) in expected {
        let version = &generate_v3_parallel_versions(part, &data, &layout, &keyboard)[0];
        let actual = version
            .stages
            .iter()
            .map(|stage| {
                let prompts = &version.prompts[stage.start..stage.end];
                (prompts.len(), prompts.iter().map(minimum_keystrokes).sum::<usize>())
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected_stages, "{part:?}");
    }
}

#[test]
fn bench_v3_uses_only_matching_core_rows_and_keeps_sentence_text() {
    let (_home, data) = catalog();
    let layout = builtin_layout("qwerty").unwrap();
    let keyboard = geometry("ansi").unwrap();
    let core = data
        .all_fixed_items()
        .iter()
        .filter(|item| item.tier == Tier::Core)
        .map(|item| (item.kind, item.text.clone(), item.reading.clone()))
        .collect::<HashSet<_>>();
    let real_english_words = data
        .fixed_items(DataKind::EnglishWord)
        .into_iter()
        .map(|item| item.text.to_ascii_lowercase())
        .collect::<HashSet<_>>();

    for part in BenchV3Part::ALL {
        let version = &generate_v3_parallel_versions(part, &data, &layout, &keyboard)[0];
        for (stage_index, stage) in version.stages.iter().enumerate() {
            for prompt in &version.prompts[stage.start..stage.end] {
                let expected_kind = match (part, stage.kind) {
                    (BenchV3Part::W | BenchV3Part::EW, _) => Some(DataKind::EnglishWord),
                    (BenchV3Part::ES, _) => Some(DataKind::EnglishSentence),
                    (BenchV3Part::JW, BenchV3StageKind::JwWord2To4 | BenchV3StageKind::JwWord5To8) => Some(DataKind::JapaneseWord),
                    (BenchV3Part::JW, BenchV3StageKind::JwPhrase6To10 | BenchV3StageKind::JwPhrase11To15) => Some(DataKind::JapanesePhrase),
                    (BenchV3Part::JS, _) => Some(DataKind::JapaneseSentence),
                    (BenchV3Part::PW, _) => None,
                    _ => panic!("unexpected stage {stage_index} for {part:?}"),
                };
                if let Some(kind) = expected_kind {
                    assert!(
                        core.contains(&(kind, prompt.text.clone(), prompt.reading.clone())),
                        "missing core row: {part:?} {prompt:?}"
                    );
                } else {
                    let word = prompt.text.to_ascii_lowercase();
                    assert!(!real_english_words.contains(&word), "pseudoword matches bundled word {word:?}");
                    assert!(!is_periodic(&word), "periodic pseudoword {word:?}");
                    assert!(!has_repeated_bigram(&word), "repeated bigram in {word:?}");
                }
            }
        }
        if part == BenchV3Part::ES {
            assert!(version.prompts.iter().all(|prompt| (30..=90).contains(&prompt.text.chars().count())));
            assert!(version.prompts.iter().any(|prompt| prompt.text.chars().any(char::is_uppercase)));
            assert!(
                version
                    .prompts
                    .iter()
                    .any(|prompt| prompt.text.chars().any(|character| character.is_ascii_punctuation()))
            );
        }
        if part == BenchV3Part::JS {
            assert!(
                version
                    .prompts
                    .iter()
                    .any(|prompt| prompt.text.contains('。') || prompt.text.contains('、'))
            );
        }
    }
}

#[test]
fn bench_v3_selection_is_independent_of_input_row_order() {
    let (_home, data) = catalog();
    let layout = builtin_layout("qwerty").unwrap();
    let keyboard = geometry("ansi").unwrap();
    let items = data.all_fixed_items().to_vec();
    let mut reversed = items.clone();
    reversed.reverse();

    for part in BenchV3Part::ALL {
        let forward = generate_v3_parallel_versions_from_items(part, &items, &layout, &keyboard);
        let backward = generate_v3_parallel_versions_from_items(part, &reversed, &layout, &keyboard);
        assert_eq!(forward, backward, "{part:?} depends on bundled row order");
    }
}

fn bench_v3_fingerprint(versions: &[BenchV3Version]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for version in versions {
        for value in std::iter::once(version.id.as_str()).chain(
            version
                .prompts
                .iter()
                .flat_map(|prompt| [prompt.text.as_str(), prompt.reading.as_deref().unwrap_or(""), prompt.target.as_str()]),
        ) {
            for byte in value.bytes().chain(std::iter::once(0xff)) {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    hash
}

#[test]
fn every_bench_v3_prompt_stays_unchanged() {
    let (_home, data) = catalog();
    let layout = builtin_layout("qwerty").unwrap();
    let keyboard = geometry("ansi").unwrap();
    let actual = BenchV3Part::ALL.map(|part| bench_v3_fingerprint(&generate_v3_parallel_versions(part, &data, &layout, &keyboard)));

    assert_eq!(
        actual,
        [
            0x5886_108d_001f_9c1d,
            0x2e7f_252c_dd1c_26f5,
            0x4385_da16_84da_25cd,
            0xa88e_55b0_fa2a_97b7,
            0xb74b_c310_d295_67af,
            0x8f76_a5be_3166_fe5d,
        ],
        "bench-v3 fingerprints: {actual:#018x?}"
    );
}
