use std::collections::HashSet;
use std::fs;

use rs_typing_trainer::config::{Config, KeyFilter, ModeKind};
use rs_typing_trainer::data::DataCatalog;
use rs_typing_trainer::engine::modes::ModeGenerator;
use rs_typing_trainer::engine::session::ItemKind;
use rs_typing_trainer::layout::builtin_layout;
use tempfile::TempDir;

fn generator(config: &Config, seed: u64) -> ModeGenerator {
    let home = TempDir::new().unwrap();
    ModeGenerator::new(
        config.mode.clone(),
        DataCatalog::load(&config.data, home.path()).unwrap(),
        builtin_layout(&config.keyboard.layout).unwrap(),
        seed,
    )
}

#[test]
fn each_mode_generates_its_kind_of_prompt() {
    for kind in [ModeKind::Random, ModeKind::Words, ModeKind::Romaji, ModeKind::Sentence, ModeKind::Ngram] {
        let mut config = Config::default();
        config.mode.kind = kind;
        config.mode.keys = "etaoinshrdlucm".to_owned();
        config.mode.key_filter = KeyFilter::Include;
        config.mode.sentence.lang = "ja".to_owned();
        let mut generator = generator(&config, kind as u64 + 1);
        let prompt = generator.next_prompt();
        assert!(!prompt.text.is_empty(), "{kind:?}");
        if kind == ModeKind::Romaji || kind == ModeKind::Sentence {
            assert!(prompt.reading.is_some(), "{kind:?}");
        }
        if kind == ModeKind::Ngram {
            assert_eq!(prompt.target.chars().count(), config.mode.ngram.n);
            assert_eq!(prompt.kind, ItemKind::Drill);
            assert_eq!(prompt.clone().into_item().kind, ItemKind::Drill);
        } else {
            assert_eq!(prompt.kind, ItemKind::Word);
            assert_eq!(prompt.clone().into_item().kind, ItemKind::Word);
        }
    }
}

#[test]
fn words_only_use_selected_and_base_keys() {
    let mut config = Config::default();
    config.mode.kind = ModeKind::Words;
    config.mode.keys = "etaoinshrdlu".to_owned();
    config.mode.key_filter = KeyFilter::Only;
    config.mode.base_keys = "none".to_owned();
    let allowed = config.mode.keys.chars().collect::<HashSet<_>>();
    let mut generator = generator(&config, 7);

    for _ in 0..40 {
        let prompt = generator.next_prompt();
        assert!(prompt.target.chars().all(|character| allowed.contains(&character)), "{}", prompt.target);
    }
}

#[test]
fn include_prompts_always_contain_a_selected_key() {
    let mut config = Config::default();
    config.mode.kind = ModeKind::Words;
    config.mode.keys = "e".to_owned();
    config.mode.key_filter = KeyFilter::Include;
    config.mode.base_keys = "none".to_owned();
    let mut generator = generator(&config, 11);

    for _ in 0..25 {
        assert!(generator.next_prompt().target.contains('e'));
    }
}

#[test]
fn words_include_repeats_real_candidates_without_pseudo_words() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("words_en.tsv");
    fs::write(&path, "zest\n").unwrap();
    let mut config = Config::default();
    config.mode.kind = ModeKind::Words;
    config.mode.keys = "z".to_owned();
    config.mode.key_filter = KeyFilter::Include;
    config.data.words_en = Some(path);
    let mut generator = generator(&config, 31);

    assert!(generator.warning().is_some());
    for _ in 0..20 {
        let prompt = generator.next_prompt();
        assert_eq!(prompt.text, "zest");
        assert!(!prompt.fallback);
    }
}

#[test]
fn romaji_only_can_use_chi_without_the_t_key() {
    let mut config = Config::default();
    config.mode.kind = ModeKind::Romaji;
    config.mode.keys.clear();
    assert!(!config.mode.keys.contains('t'));
    let generator = generator(&config, 13);

    assert!(generator.eligible_texts().iter().any(|text| text == "地図"));
}

#[test]
fn recent_ten_prompts_are_not_repeated() {
    let mut config = Config::default();
    config.mode.kind = ModeKind::Words;
    config.mode.keys = "abcdefghijklmnopqrstuvwxyz".to_owned();
    config.mode.key_filter = KeyFilter::Include;
    let mut generator = generator(&config, 17);
    let mut recent = Vec::<String>::new();

    for _ in 0..40 {
        let text = generator.next_prompt().text;
        assert!(!recent.contains(&text), "repeated {text:?} within {recent:?}");
        recent.push(text);
        if recent.len() > 10 {
            recent.remove(0);
        }
    }
}

#[test]
fn fewer_than_twenty_candidates_sets_warning_and_uses_fallbacks() {
    let mut config = Config::default();
    config.mode.kind = ModeKind::Words;
    config.mode.keys = "q".to_owned();
    config.mode.key_filter = KeyFilter::Only;
    config.mode.base_keys = "none".to_owned();
    let mut generator = generator(&config, 19);

    assert!(generator.warning().is_some());
    assert!(generator.original_candidate_count() < 20);
    assert!((0..20).any(|_| generator.next_prompt().fallback));
}

#[test]
fn one_romaji_candidate_repeats_without_pseudo_words() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("words_ja.tsv");
    fs::write(&path, "地図\tちず\n").unwrap();
    let mut config = Config::default();
    config.mode.kind = ModeKind::Romaji;
    config.mode.keys.clear();
    config.data.words_ja = Some(path);
    let mut generator = generator(&config, 23);

    assert!(generator.warning().is_some());
    for _ in 0..20 {
        let prompt = generator.next_prompt();
        assert_eq!(prompt.text, "地図");
        assert!(!prompt.fallback);
    }
}

#[test]
fn words_only_pseudo_words_use_only_selected_keys() {
    let mut config = Config::default();
    config.mode.kind = ModeKind::Words;
    config.mode.keys = "qz".to_owned();
    config.mode.key_filter = KeyFilter::Only;
    config.mode.base_keys = "none".to_owned();
    let selected = config.mode.keys.chars().collect::<HashSet<_>>();
    let mut generator = generator(&config, 29);

    let fallbacks = (0..64)
        .map(|_| generator.next_prompt())
        .filter(|prompt| prompt.fallback)
        .collect::<Vec<_>>();
    assert!(!fallbacks.is_empty());
    assert!(
        fallbacks
            .iter()
            .all(|prompt| prompt.target.chars().all(|character| selected.contains(&character)))
    );
}

#[test]
fn small_ngram_pool_keeps_the_candidate_warning() {
    let mut config = Config::default();
    config.mode.kind = ModeKind::Ngram;
    config.mode.keys = "a".to_owned();
    config.mode.ngram.n = 2;
    config.mode.ngram.source = "pairs".to_owned();
    let generator = generator(&config, 37);

    assert_eq!(generator.original_candidate_count(), 1);
    assert!(generator.warning().is_some());
}
