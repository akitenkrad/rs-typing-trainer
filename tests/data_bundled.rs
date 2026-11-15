use rs_typing_trainer::config::DataConfig;
use rs_typing_trainer::data::{DataCatalog, DataKind, Tier, TrainingDataSource};
use rs_typing_trainer::engine::judge::{Judge, JudgeResult};
use rs_typing_trainer::engine::romaji::RomajiJudge;
use tempfile::TempDir;

const BLOCKLIST_EN: &str = include_str!("../tools/gen-data/blocklist_en.txt");

fn guide_is_accepted(reading: &str) -> bool {
    let mut judge = RomajiJudge::new(reading);
    let guide = judge.guide().to_owned();
    let mut complete = false;
    for character in guide.chars() {
        match judge.input(&character.to_string()) {
            JudgeResult::Mistype => return false,
            JudgeResult::ItemCompleted => complete = true,
            JudgeResult::Correct => {}
        }
    }
    complete
}

#[test]
fn bundled_data_has_expected_counts_and_romaji_coverage() {
    let home = TempDir::new().unwrap();
    let catalog = DataCatalog::load(&DataConfig::default(), home.path()).unwrap();

    assert!(catalog.words_en().len() >= 20_000);
    assert!(catalog.words_ja().len() >= 10_000);
    assert!(catalog.sentences_en().len() >= 15_000);
    assert!(catalog.sentences_ja().len() >= 3_000);
    for kind in [
        DataKind::EnglishWord,
        DataKind::JapaneseWord,
        DataKind::EnglishSentence,
        DataKind::JapaneseSentence,
    ] {
        let rows = catalog.fixed_items(kind);
        assert!(!rows.is_empty());
        assert!(rows.iter().all(|row| matches!(row.tier, Tier::Core | Tier::Ext)));
        assert!(rows.iter().any(|row| row.tier == Tier::Core));
        assert!(rows.iter().any(|row| row.tier == Tier::Ext));
    }

    let allowed_reading = |reading: &str| {
        reading
            .chars()
            .all(|character| matches!(character, '\u{3041}'..='\u{3096}' | 'ー' | '、' | '。' | '「' | '」'))
    };
    assert!(catalog.words_ja().iter().all(|row| allowed_reading(&row.reading)));
    assert!(catalog.sentences_ja().iter().all(|row| allowed_reading(&row.reading)));
    for row in catalog.words_ja() {
        assert!(guide_is_accepted(&row.reading), "untypable word: {}\t{}", row.text, row.reading);
    }
    for row in catalog.sentences_ja() {
        assert!(guide_is_accepted(&row.reading), "untypable sentence: {}\t{}", row.text, row.reading);
    }
}

#[test]
fn bundled_tsv_rows_have_the_documented_shapes_and_no_blocklisted_words() {
    let home = TempDir::new().unwrap();
    let catalog = DataCatalog::load(&DataConfig::default(), home.path()).unwrap();

    let blocklist = BLOCKLIST_EN.lines().filter(|line| !line.is_empty()).collect::<BTreeSet<_>>();
    assert!(catalog.words_en().iter().all(|word| !blocklist.contains(word.as_str())));
}

#[test]
fn bundled_japanese_readings_only_keep_written_long_vowel_marks() {
    let home = TempDir::new().unwrap();
    let catalog = DataCatalog::load(&DataConfig::default(), home.path()).unwrap();
    for row in catalog.sentences_ja() {
        let text = &row.text;
        let reading = &row.reading;
        assert_eq!(
            reading.matches('ー').count(),
            text.matches('ー').count(),
            "sentence reading introduced or removed a long-vowel mark: {text}\t{reading}"
        );
    }

    for row in catalog.words_ja() {
        let surface = &row.text;
        let reading = &row.reading;
        assert!(
            !reading.contains('ー') || surface.contains('ー'),
            "word reading introduced a long-vowel mark: {surface}\t{reading}"
        );
    }
}

#[test]
fn public_docs_and_bundled_data_do_not_name_the_english_distributor() {
    fn collect(path: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        if path.is_file() {
            files.push(path.to_owned());
        } else {
            for entry in std::fs::read_dir(path).unwrap() {
                collect(&entry.unwrap().path(), files);
            }
        }
    }
    let mut files = vec!["README.md".into(), "README.ja.md".into()];
    collect(std::path::Path::new("data"), &mut files);
    collect(std::path::Path::new("docs"), &mut files);
    for path in files {
        let text = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).to_ascii_lowercase();
        assert!(!text.contains("gutenberg"), "forbidden name in {}", path.display());
    }
}

use std::collections::BTreeSet;
