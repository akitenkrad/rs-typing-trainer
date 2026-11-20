//! Deterministic prompt generation for the P3 training modes.

use std::collections::{HashSet, VecDeque};

use rand::distributions::{Distribution, WeightedIndex};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use crate::config::{KeyFilter, ModeConfig, ModeKind, PromptUnit};
use crate::data::{DataCatalog, DataKind, PromptSource, TrainingDataSource};
use crate::engine::item::Item;
use crate::engine::romaji::{RomajiJudge, RomajiOptions};
use crate::engine::session::ItemKind;
use crate::layout::KeyboardLayout;
use crate::practice::adaptive::{AdaptivePlan, AdaptiveSource, generate_from_sources};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedPrompt {
    pub text: String,
    pub reading: Option<String>,
    pub target: String,
    pub fallback: bool,
    pub kind: ItemKind,
    /// Where the text comes from (bundled sentences only); shown under the
    /// prompt being typed.
    pub source: Option<PromptSource>,
    weight: usize,
    romaji_options: Option<RomajiOptions>,
}

impl GeneratedPrompt {
    pub fn fixed(text: String, reading: Option<String>, target: String) -> Self {
        Self {
            text,
            reading,
            target,
            fallback: false,
            kind: ItemKind::Word,
            source: None,
            weight: 1,
            romaji_options: Some(RomajiOptions::default()),
        }
    }

    pub fn into_item(self) -> Item {
        let kind = self.kind;
        if let Some(reading) = self.reading {
            Item::new(
                self.text,
                Some(reading.clone()),
                Box::new(RomajiJudge::with_options(&reading, self.romaji_options.unwrap_or_default())),
            )
            .with_kind(kind)
        } else {
            Item::exact(self.text).with_kind(kind)
        }
    }
}

pub struct ModeGenerator {
    config: ModeConfig,
    candidates: Vec<GeneratedPrompt>,
    original_texts: Vec<String>,
    original_candidate_count: usize,
    warning: Option<String>,
    recent: VecDeque<String>,
    phrase_stages: Vec<Vec<GeneratedPrompt>>,
    phrase_counter: usize,
    rng: StdRng,
}

impl ModeGenerator {
    pub fn new(config: ModeConfig, data: DataCatalog, layout: KeyboardLayout, seed: u64) -> Self {
        let selected = config.keys.chars().collect::<HashSet<_>>();
        let romaji_options = RomajiOptions {
            accept_apostrophe: config.romaji.accept_apostrophe,
            accept_xn: config.romaji.accept_xn,
        };
        let base = base_keys(&config.base_keys, &layout);
        let mut allowed = selected.clone();
        allowed.extend(base);

        let unit = config.prompt_unit();
        let japanese_words = crate::practice::drill::japanese_words(&config);
        let mut candidates = match unit {
            PromptUnit::Char => random_prompt_pool(&config),
            PromptUnit::Ngram => ngram_prompts(&config, &data, &selected),
            PromptUnit::Word if japanese_words => data
                .words_ja()
                .iter()
                .map(|entry| {
                    let judge = RomajiJudge::new(&entry.reading);
                    GeneratedPrompt {
                        text: entry.text.clone(),
                        reading: Some(entry.reading.clone()),
                        target: judge.guide().to_owned(),
                        fallback: false,
                        kind: ItemKind::Word,
                        source: None,
                        weight: romaji_weight(&judge, &selected),
                        romaji_options: Some(romaji_options),
                    }
                })
                .collect(),
            PromptUnit::Word => data
                .words_en()
                .iter()
                .map(|word| exact_prompt(word, false, occurrence_count(word, &selected)))
                .collect(),
            PromptUnit::Sentence if config.sentence.lang == "ja" => japanese_sentence_prompts(&data, romaji_options),
            PromptUnit::Sentence => english_sentence_prompts(&data),
            PromptUnit::Passage if config.sentence.lang == "ja" => japanese_passage_prompts(&data, romaji_options),
            PromptUnit::Passage => english_passage_prompts(&data, seed),
            PromptUnit::Phrase if config.phrase.lang == "ja" => data
                .phrases_ja()
                .iter()
                .map(|entry| {
                    let judge = RomajiJudge::new(&entry.reading);
                    GeneratedPrompt {
                        text: entry.text.clone(),
                        reading: Some(entry.reading.clone()),
                        target: judge.guide().to_owned(),
                        fallback: false,
                        kind: ItemKind::Word,
                        source: None,
                        weight: 1,
                        romaji_options: Some(romaji_options),
                    }
                })
                .filter(|prompt| phrase_length_matches(&config.phrase.course, prompt.reading.as_deref().unwrap_or(""), true))
                .collect(),
            PromptUnit::Phrase => data
                .phrases_en()
                .iter()
                .filter(|phrase| phrase_length_matches(&config.phrase.course, phrase, false))
                .map(|phrase| exact_prompt(phrase, false, 1))
                .collect(),
        };

        if unit == PromptUnit::Word
            && !selected.is_empty()
            && !matches!(config.kind, ModeKind::Sentence | ModeKind::Phrase | ModeKind::Endurance | ModeKind::Ngram)
        {
            candidates.retain(|prompt| match config.key_filter {
                KeyFilter::Include => prompt.weight > 0,
                KeyFilter::Only if japanese_words => prompt
                    .reading
                    .as_deref()
                    .is_some_and(|reading| RomajiJudge::with_options(reading, romaji_options).has_spelling_using(&allowed)),
                KeyFilter::Only => prompt.target.chars().all(|character| allowed.contains(&character)),
            });
        }

        let original_candidate_count = candidates.len();
        let original_texts = candidates.iter().map(|prompt| prompt.text.clone()).collect();
        let warning = (original_candidate_count < 20).then(|| {
            if matches!(config.kind, ModeKind::Words | ModeKind::Unlock) && config.key_filter == KeyFilter::Only && !selected.is_empty() {
                format!("Only {original_candidate_count} matching items; pseudo-words will fill the pool")
            } else {
                format!("Only {original_candidate_count} items; recent items may repeat")
            }
        });
        if matches!(config.kind, ModeKind::Words | ModeKind::Unlock)
            && config.key_filter == KeyFilter::Only
            && !selected.is_empty()
            && candidates.len() < 32
        {
            supplement_with_pseudo_words(&mut candidates, &config, &selected);
        }
        let phrase_stages = if unit == PromptUnit::Phrase && config.phrase.course == "mixed" {
            let japanese = config.phrase.lang == "ja";
            (0..3)
                .map(|stage| {
                    candidates
                        .iter()
                        .filter(|prompt| phrase_stage(prompt.reading.as_deref().unwrap_or(&prompt.text), japanese) == Some(stage))
                        .cloned()
                        .collect()
                })
                .collect()
        } else {
            Vec::new()
        };

        Self {
            config,
            candidates,
            original_texts,
            original_candidate_count,
            warning,
            recent: VecDeque::new(),
            phrase_stages,
            phrase_counter: 0,
            rng: StdRng::seed_from_u64(seed),
        }
    }

    pub fn new_adaptive(config: ModeConfig, data: DataCatalog, layout: KeyboardLayout, seed: u64, plan: &AdaptivePlan, japanese: bool) -> Self {
        if plan.fallback_to_words {
            let mut words = config.clone();
            words.kind = if japanese { ModeKind::Romaji } else { ModeKind::Words };
            words.keys.clear();
            words.base_keys = "none".to_owned();
            words.drill = None;
            let mut generator = Self::new(words, data, layout, seed);
            generator.config = config;
            return generator;
        }
        let sources = if japanese {
            data.words_ja()
                .iter()
                .map(|item| AdaptiveSource {
                    text: item.text.clone(),
                    reading: Some(item.reading.clone()),
                    target: RomajiJudge::new(&item.reading).guide().to_owned(),
                })
                .collect::<Vec<_>>()
        } else {
            data.words_en()
                .iter()
                .map(|word| AdaptiveSource {
                    text: word.clone(),
                    reading: None,
                    target: word.clone(),
                })
                .collect::<Vec<_>>()
        };
        let candidates = generate_from_sources(plan, &sources, 4_096, seed)
            .into_iter()
            .map(|item| GeneratedPrompt {
                text: item.text,
                reading: item.reading,
                target: item.target,
                fallback: item.kind == crate::practice::adaptive::AdaptiveItemKind::Drill,
                kind: if item.kind == crate::practice::adaptive::AdaptiveItemKind::Drill {
                    ItemKind::Drill
                } else {
                    ItemKind::Word
                },
                source: None,
                weight: 1,
                romaji_options: japanese.then_some(RomajiOptions::default()),
            })
            .collect::<Vec<_>>();
        let original_texts = candidates.iter().map(|prompt| prompt.text.clone()).collect();
        Self {
            config,
            original_candidate_count: candidates.len(),
            candidates,
            original_texts,
            warning: None,
            recent: VecDeque::new(),
            phrase_stages: Vec::new(),
            phrase_counter: 0,
            rng: StdRng::seed_from_u64(seed),
        }
    }

    /// Weak transitions in phrases and sentences (MYTASK-3638): prompts
    /// that contain more of `transitions` are drawn more often, and when at
    /// least [`crate::practice::menu::MIN_TARGET_WORDS`] prompts contain one,
    /// the others are dropped.
    pub fn prefer_transitions(&mut self, transitions: &[(String, String)]) {
        if transitions.is_empty() {
            return;
        }
        for prompt in &mut self.candidates {
            prompt.weight = 1 + 4 * transition_count(&prompt.target, transitions);
        }
        if self.candidates.iter().filter(|prompt| prompt.weight > 1).count() >= crate::practice::menu::MIN_TARGET_WORDS {
            self.candidates.retain(|prompt| prompt.weight > 1);
        }
        for stage in &mut self.phrase_stages {
            for prompt in stage.iter_mut() {
                prompt.weight = 1 + 4 * transition_count(&prompt.target, transitions);
            }
        }
        self.original_texts = self.candidates.iter().map(|prompt| prompt.text.clone()).collect();
        self.original_candidate_count = self.candidates.len();
    }

    /// Keep only the transition drills of an adaptive plan (weak transitions
    /// as combinations, MYTASK-3638). Nothing changes when the plan has none.
    pub fn retain_drills(&mut self) {
        if self.candidates.iter().any(|prompt| prompt.kind == ItemKind::Drill) {
            self.candidates.retain(|prompt| prompt.kind == ItemKind::Drill);
            self.original_texts = self.candidates.iter().map(|prompt| prompt.text.clone()).collect();
            self.original_candidate_count = self.candidates.len();
        }
    }

    /// The candidate prompts with their weights, for tests.
    pub fn weighted_candidates(&self) -> Vec<(GeneratedPrompt, usize)> {
        self.candidates.iter().map(|prompt| (prompt.clone(), prompt.weight)).collect()
    }

    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }

    pub fn original_candidate_count(&self) -> usize {
        self.original_candidate_count
    }

    pub fn eligible_texts(&self) -> &[String] {
        &self.original_texts
    }

    pub fn next_prompt(&mut self) -> GeneratedPrompt {
        if self.candidates.is_empty() {
            return exact_prompt("aaa", true, 1);
        }
        let candidates = if self.phrase_stages.is_empty() {
            &self.candidates
        } else {
            let stage = (self.phrase_counter / 5) % self.phrase_stages.len();
            self.phrase_counter += 1;
            &self.phrase_stages[stage]
        };
        if candidates.is_empty() {
            return exact_prompt("aaa", true, 1);
        }
        let available = candidates
            .iter()
            .enumerate()
            .filter(|(_, prompt)| !self.recent.contains(&prompt.text))
            .map(|(index, prompt)| (index, prompt.weight.max(1)))
            .collect::<Vec<_>>();
        let index = if available.is_empty() {
            self.rng.gen_range(0..candidates.len())
        } else {
            let weights = available.iter().map(|(_, weight)| *weight).collect::<Vec<_>>();
            let selected = WeightedIndex::new(weights)
                .map(|distribution| distribution.sample(&mut self.rng))
                .unwrap_or(0);
            available[selected].0
        };
        let prompt = candidates[index].clone();
        self.recent.push_back(prompt.text.clone());
        if self.recent.len() > 10 {
            self.recent.pop_front();
        }
        prompt
    }

    pub fn kind(&self) -> ModeKind {
        self.config.kind
    }

    pub fn exclude(&mut self, text: &str, reading: Option<&str>) {
        self.candidates
            .retain(|prompt| prompt.text != text || prompt.reading.as_deref() != reading);
        self.original_texts.retain(|candidate| candidate != text);
    }
}

fn japanese_sentence_prompts(data: &DataCatalog, romaji_options: RomajiOptions) -> Vec<GeneratedPrompt> {
    data.sentences_ja()
        .iter()
        .map(|entry| {
            let judge = RomajiJudge::new(&entry.reading);
            GeneratedPrompt {
                text: entry.text.clone(),
                reading: Some(entry.reading.clone()),
                target: judge.guide().to_owned(),
                fallback: false,
                kind: ItemKind::Word,
                source: data
                    .sentence_source(DataKind::JapaneseSentence, &entry.text, Some(&entry.reading))
                    .cloned(),
                weight: 1,
                romaji_options: Some(romaji_options),
            }
        })
        .collect()
}

fn english_sentence_prompts(data: &DataCatalog) -> Vec<GeneratedPrompt> {
    data.sentences_en()
        .iter()
        .map(|sentence| GeneratedPrompt {
            source: data.sentence_source(DataKind::EnglishSentence, sentence, None).cloned(),
            ..exact_prompt(sentence, false, 1)
        })
        .collect()
}

/// Japanese passages: bundled runs of consecutive sentences of one Aozora
/// Bunko work (MYTASK-3639), each with its work as the source.
fn japanese_passage_prompts(data: &DataCatalog, romaji_options: RomajiOptions) -> Vec<GeneratedPrompt> {
    data.passages_ja()
        .iter()
        .map(|passage| {
            let judge = RomajiJudge::new(&passage.reading);
            GeneratedPrompt {
                text: passage.text.clone(),
                reading: Some(passage.reading.clone()),
                target: judge.guide().to_owned(),
                fallback: false,
                kind: ItemKind::Word,
                source: passage.source.clone(),
                weight: 1,
                romaji_options: Some(romaji_options),
            }
        })
        .collect()
}

/// Shortest and longest English passage, in sentences.
pub const ENGLISH_PASSAGE_SENTENCES: (usize, usize) = (3, 5);

/// English passages: three to five consecutive bundled sentences joined into
/// one prompt. The split points depend on the seed; the source lists the
/// Tatoeba number of every sentence when all of them have one.
fn english_passage_prompts(data: &DataCatalog, seed: u64) -> Vec<GeneratedPrompt> {
    let sentences = data.sentences_en();
    let (shortest, longest) = ENGLISH_PASSAGE_SENTENCES;
    let mut rng = StdRng::seed_from_u64(seed ^ 0x7061_7373_6167_6573);
    let mut prompts = Vec::new();
    let mut start = 0;
    while start + shortest <= sentences.len() {
        let length = rng.gen_range(shortest..=longest).min(sentences.len() - start);
        let group = &sentences[start..start + length];
        start += length;
        let text = group.iter().map(|sentence| sentence.trim()).collect::<Vec<_>>().join(" ");
        let ids = group
            .iter()
            .map(|sentence| match data.sentence_source(DataKind::EnglishSentence, sentence, None) {
                Some(PromptSource::Tatoeba { sentence_id }) => Some(sentence_id.clone()),
                _ => None,
            })
            .collect::<Option<Vec<_>>>();
        prompts.push(GeneratedPrompt {
            source: ids.map(|sentence_ids| PromptSource::TatoebaSet { sentence_ids }),
            ..exact_prompt(&text, false, 1)
        });
    }
    prompts
}

/// Occurrences of the given transitions (`from` then `to`) in a target.
fn transition_count(target: &str, transitions: &[(String, String)]) -> usize {
    let characters = target.chars().collect::<Vec<_>>();
    characters
        .windows(2)
        .filter(|pair| {
            transitions.iter().any(|(from, to)| {
                let mut from = from.chars();
                let mut to = to.chars();
                from.next() == Some(pair[0]) && from.next().is_none() && to.next() == Some(pair[1]) && to.next().is_none()
            })
        })
        .count()
}

fn phrase_stage(text: &str, japanese: bool) -> Option<usize> {
    let length = text.chars().count();
    let ranges = if japanese {
        [(6, 10), (11, 15), (16, 20)]
    } else {
        [(10, 15), (16, 22), (23, 29)]
    };
    ranges.iter().position(|(minimum, maximum)| (*minimum..=*maximum).contains(&length))
}

fn phrase_length_matches(course: &str, text: &str, japanese: bool) -> bool {
    match course {
        "short" => phrase_stage(text, japanese) == Some(0),
        "normal" => phrase_stage(text, japanese) == Some(1),
        "long" => phrase_stage(text, japanese) == Some(2),
        "mixed" => phrase_stage(text, japanese).is_some(),
        _ => false,
    }
}

fn exact_prompt(text: &str, fallback: bool, weight: usize) -> GeneratedPrompt {
    GeneratedPrompt {
        text: text.to_owned(),
        reading: None,
        target: text.to_owned(),
        fallback,
        kind: ItemKind::Word,
        source: None,
        weight,
        romaji_options: None,
    }
}

fn occurrence_count(text: &str, selected: &HashSet<char>) -> usize {
    text.chars().filter(|character| selected.contains(character)).count()
}

fn romaji_weight(judge: &RomajiJudge, selected: &HashSet<char>) -> usize {
    judge
        .accepted_spellings(512)
        .iter()
        .map(|spelling| occurrence_count(spelling, selected))
        .max()
        .unwrap_or(0)
}

fn base_keys(value: &str, layout: &KeyboardLayout) -> HashSet<char> {
    match value {
        "none" => HashSet::new(),
        "home" => [
            "KeyA",
            "KeyS",
            "KeyD",
            "KeyF",
            "KeyG",
            "KeyH",
            "KeyJ",
            "KeyK",
            "KeyL",
            "Semicolon",
            "Quote",
        ]
        .into_iter()
        .filter_map(|code| layout.key(code).map(|pair| pair.normal))
        .collect(),
        characters => characters.chars().collect(),
    }
}

fn random_prompt_pool(config: &ModeConfig) -> Vec<GeneratedPrompt> {
    let alphabet = config.keys.chars().collect::<Vec<_>>();
    if alphabet.is_empty() {
        return Vec::new();
    }
    if !config.random.allow_repeat {
        let mut words = Vec::new();
        build_random_without_repeat(&alphabet, config.random.word_length.max(1), &mut Vec::new(), &mut words);
        return words.into_iter().take(256).map(|text| exact_prompt(&text, false, 1)).collect();
    }
    (0..64)
        .map(|index| {
            let text = encoded_word(index, &alphabet, config.random.word_length.max(1));
            exact_prompt(&text, false, 1)
        })
        .collect()
}

fn build_random_without_repeat(alphabet: &[char], remaining: usize, prefix: &mut Vec<char>, words: &mut Vec<String>) {
    if remaining == 0 {
        words.push(prefix.iter().collect());
        return;
    }
    for character in alphabet {
        if prefix.contains(character) {
            continue;
        }
        prefix.push(*character);
        build_random_without_repeat(alphabet, remaining - 1, prefix, words);
        prefix.pop();
        if words.len() >= 256 {
            return;
        }
    }
}

fn ngram_prompts(config: &ModeConfig, data: &DataCatalog, selected: &HashSet<char>) -> Vec<GeneratedPrompt> {
    let n = config.ngram.n.clamp(2, 3);
    let mut values = HashSet::new();
    if config.ngram.source == "corpus" {
        for word in data.words_en() {
            let characters = word.chars().collect::<Vec<_>>();
            for window in characters.windows(n) {
                if window.iter().any(|character| selected.contains(character)) {
                    values.insert(window.iter().collect::<String>());
                }
            }
        }
    } else {
        let alphabet = selected.iter().copied().collect::<Vec<_>>();
        build_ngrams(&alphabet, n, String::new(), &mut values);
    }
    values
        .into_iter()
        .map(|value| {
            let mut prompt = exact_prompt(&value, false, 1);
            prompt.kind = ItemKind::Drill;
            prompt
        })
        .collect()
}

fn build_ngrams(alphabet: &[char], remaining: usize, prefix: String, values: &mut HashSet<String>) {
    if remaining == 0 {
        values.insert(prefix);
        return;
    }
    for character in alphabet {
        let mut next = prefix.clone();
        next.push(*character);
        build_ngrams(alphabet, remaining - 1, next, values);
    }
}

fn supplement_with_pseudo_words(candidates: &mut Vec<GeneratedPrompt>, config: &ModeConfig, selected: &HashSet<char>) {
    let mut alphabet = selected.iter().copied().collect::<Vec<_>>();
    alphabet.sort_unstable();
    if alphabet.is_empty() {
        alphabet.push('a');
    }
    let mut seen = candidates.iter().map(|prompt| prompt.text.clone()).collect::<HashSet<_>>();
    let mut counter = 0usize;
    while candidates.len() < 32 && counter < 4096 {
        let length = if alphabet.len() == 1 {
            counter + 1
        } else {
            config.random.word_length.max(3) + counter / alphabet.len().pow(3)
        };
        let text = encoded_word(counter, &alphabet, length);
        counter += 1;
        if !seen.insert(text.clone()) {
            continue;
        }
        candidates.push(GeneratedPrompt {
            text: text.clone(),
            reading: None,
            target: text,
            fallback: true,
            kind: ItemKind::Word,
            source: None,
            weight: 1,
            romaji_options: None,
        });
    }
}

fn encoded_word(mut value: usize, alphabet: &[char], length: usize) -> String {
    if alphabet.len() == 1 {
        return std::iter::repeat_n(alphabet[0], length).collect();
    }
    let mut characters = vec![alphabet[0]; length];
    for character in characters.iter_mut().rev() {
        *character = alphabet[value % alphabet.len()];
        value /= alphabet.len();
    }
    characters.into_iter().collect()
}
