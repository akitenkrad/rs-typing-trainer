use rs_typing_trainer::config::DataConfig;
use rs_typing_trainer::data::{DataCatalog, DataKind, Tier, TrainingDataSource};
use rs_typing_trainer::engine::judge::{Judge, JudgeResult};
use rs_typing_trainer::engine::romaji::RomajiJudge;
use rs_typing_trainer::ui::bigtext::{BigTextSize, font};
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
fn bundled_english_sentences_have_no_complete_dialogue_quote_sequences() {
    fn is_complete_dialogue_quote_sequence(text: &str) -> bool {
        let bytes = text.as_bytes();
        let mut cursor = 0;
        if bytes.first() != Some(&b'"') {
            return false;
        }
        loop {
            cursor += 1;
            while cursor < bytes.len() && bytes[cursor] != b'"' {
                cursor += 1;
            }
            if cursor == bytes.len() {
                return false;
            }
            cursor += 1;
            if cursor == bytes.len() {
                return true;
            }
            if bytes[cursor] != b' ' {
                return false;
            }
            while cursor < bytes.len() && bytes[cursor] == b' ' {
                cursor += 1;
            }
            if cursor == bytes.len() || bytes[cursor] != b'"' {
                return false;
            }
        }
    }

    let home = TempDir::new().unwrap();
    let catalog = DataCatalog::load(&DataConfig::default(), home.path()).unwrap();

    let quoted = catalog
        .sentences_en()
        .iter()
        .filter(|text| is_complete_dialogue_quote_sequence(text))
        .collect::<Vec<_>>();
    assert!(
        quoted.is_empty(),
        "complete dialogue quote sequences remain: {:?}",
        &quoted[..quoted.len().min(3)]
    );
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

#[test]
fn bundled_fonts_have_the_expected_character_sets_and_japanese_coverage() {
    let dot12 = font(BigTextSize::Dot12).unwrap();
    let dot16 = font(BigTextSize::Dot16).unwrap();
    for bundled in [dot12, dot16] {
        assert_eq!(bundled.fullwidth_glyph_count(), 6_879);
        assert_eq!(bundled.halfwidth_glyph_count(), 95 + 63);
        assert!((0x20_u32..=0x7e).all(|codepoint| bundled.contains(char::from_u32(codepoint).unwrap())));
        assert!((0xff61_u32..=0xff9f).all(|codepoint| bundled.contains(char::from_u32(codepoint).unwrap())));
    }

    let home = TempDir::new().unwrap();
    let catalog = DataCatalog::load(&DataConfig::default(), home.path()).unwrap();
    let coverage = |texts: Vec<&str>| {
        let characters = texts.into_iter().flat_map(str::chars).collect::<BTreeSet<_>>();
        let covered = characters.iter().filter(|character| dot12.contains(**character)).count();
        (covered, characters.len(), covered as f64 * 100.0 / characters.len() as f64)
    };
    let words = coverage(catalog.words_ja().iter().map(|row| row.text.as_str()).collect());
    let sentences = coverage(catalog.sentences_ja().iter().map(|row| row.text.as_str()).collect());
    eprintln!(
        "Shinonome coverage: Japanese words {}/{} ({:.3}%), Japanese sentences {}/{} ({:.3}%)",
        words.0, words.1, words.2, sentences.0, sentences.1, sentences.2
    );
    assert!(words.2 >= 95.0, "word coverage is only {:.3}%", words.2);
    assert!(sentences.2 >= 95.0, "sentence coverage is only {:.3}%", sentences.2);
}

use std::collections::BTreeSet;
