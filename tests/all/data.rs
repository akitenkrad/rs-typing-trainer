use std::fs;

use rs_typing_trainer::config::DataConfig;
use rs_typing_trainer::data::{DataCatalog, TrainingDataSource};
use tempfile::TempDir;

#[test]
fn bundled_catalog_contains_the_p3_sample_sizes() {
    let home = TempDir::new().unwrap();
    let catalog = DataCatalog::load(&DataConfig::default(), home.path()).unwrap();

    assert!(catalog.words_en().len() >= 140);
    assert!(catalog.words_ja().len() >= 75);
    assert!(catalog.sentences_en().len() >= 20);
    assert!(catalog.sentences_ja().len() >= 20);
    assert!(catalog.words_ja().iter().all(|entry| !entry.text.is_empty() && !entry.reading.is_empty()));
    assert!(
        catalog
            .sentences_ja()
            .iter()
            .all(|entry| !entry.text.is_empty() && !entry.reading.is_empty())
    );
}

#[test]
fn configured_file_replaces_only_its_bundled_category() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("words-en.tsv");
    fs::write(&path, "alpha\nbeta\n").unwrap();
    let config = DataConfig {
        words_en: Some(path),
        ..DataConfig::default()
    };

    let catalog = DataCatalog::load(&config, directory.path()).unwrap();

    assert_eq!(catalog.words_en(), ["alpha", "beta"]);
    assert!(catalog.words_ja().len() >= 75);
}

#[test]
fn configured_phrase_files_extend_their_bundled_categories() {
    let directory = TempDir::new().unwrap();
    let english_path = directory.path().join("phrases-en.tsv");
    let japanese_path = directory.path().join("phrases-ja.tsv");
    fs::write(&english_path, "hello there\ngood morning\n").unwrap();
    fs::write(&japanese_path, "朝の挨拶\tあさのあいさつ\n帰りの挨拶\tかえりのあいさつ\n").unwrap();
    let config = DataConfig {
        phrases_en: Some(english_path),
        phrases_ja: Some(japanese_path),
        ..DataConfig::default()
    };

    let catalog = DataCatalog::load(&config, directory.path()).unwrap();

    assert!(catalog.phrases_en().len() > 2);
    assert!(catalog.phrases_en().iter().any(|text| text == "hello there"));
    assert!(catalog.phrases_en().iter().any(|text| text == "good morning"));
    assert!(catalog.phrases_ja().len() > 2);
    assert!(
        catalog
            .phrases_ja()
            .iter()
            .any(|row| row.text == "朝の挨拶" && row.reading == "あさのあいさつ")
    );
    assert!(
        catalog
            .phrases_ja()
            .iter()
            .any(|row| row.text == "帰りの挨拶" && row.reading == "かえりのあいさつ")
    );
    assert!(catalog.words_en().len() >= 140);
    assert!(catalog.sentences_en().len() >= 20);
}
