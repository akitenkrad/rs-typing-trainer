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
