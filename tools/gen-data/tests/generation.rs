use std::collections::BTreeSet;

use gen_data::{extract_aozora, extract_english_body, load_scowl_words, stable_sample};
use tempfile::TempDir;

#[test]
fn english_body_is_bounded_by_distribution_markers() {
    let source = "header\n*** START OF THE EBOOK SAMPLE ***\nAlpha beta.\n*** END OF THE EBOOK SAMPLE ***\nfooter\n";

    assert_eq!(extract_english_body(source).unwrap(), "Alpha beta.\n");
}

#[test]
fn aozora_extraction_removes_header_legend_notes_and_colophon() {
    let source = "作品名\r\n著者名\r\n\r\n--------------------------------\r\n凡例《はんれい》\r\n--------------------------------\r\n\r\n　親譲《おやゆず》り［＃注記］の本文。\r\n底本：「本」\r\n入力：入力者\r\n";

    let document = extract_aozora(source);

    assert_eq!(document.body, "親譲りの本文。");
    assert_eq!(document.rubies.get("親譲"), Some(&BTreeSet::from(["おやゆず".to_owned()])));
}

#[test]
fn aozora_explicit_ruby_marker_limits_the_parent_text() {
    let source = "題\n著者\n\n本文は夕方｜折戸《おりど》の蔭《かげ》にある。\n底本：本\n";

    let document = extract_aozora(source);

    assert_eq!(document.body, "本文は夕方折戸の蔭にある。");
    assert_eq!(document.rubies.get("折戸"), Some(&BTreeSet::from(["おりど".to_owned()])));
    assert_eq!(document.rubies.get("蔭"), Some(&BTreeSet::from(["かげ".to_owned()])));
}

#[test]
fn aozora_ruby_offsets_follow_trimmed_body_text() {
    let source = "題\n著者\n\n　家《いえ》の外《そと》に出る。\n底本：本\n";

    let document = extract_aozora(source);

    assert_eq!(document.body, "家の外に出る。");
    assert_eq!((document.ruby_spans[0].start, document.ruby_spans[0].end), (0, "家".len()));
    assert_eq!((document.ruby_spans[1].start, document.ruby_spans[1].end), ("家の".len(), "家の外".len()));
}

#[test]
fn scowl_loader_uses_only_requested_word_lists() {
    let directory = TempDir::new().unwrap();
    let final_dir = directory.path().join("final");
    std::fs::create_dir(&final_dir).unwrap();
    std::fs::write(final_dir.join("english-words.10"), "alpha\ncan't\n").unwrap();
    std::fs::write(final_dir.join("american-words.20"), "color\n").unwrap();
    std::fs::write(final_dir.join("english-words.60"), "expanded\n").unwrap();
    std::fs::write(final_dir.join("english-words.70"), "excluded\n").unwrap();
    std::fs::write(final_dir.join("english-proper-names.10"), "Alice\n").unwrap();

    let words = load_scowl_words(directory.path()).unwrap();

    assert_eq!(words, BTreeSet::from(["alpha".to_owned(), "color".to_owned(), "expanded".to_owned()]));
}

#[test]
fn stable_sampling_is_repeatable_and_order_independent() {
    let values_a = vec!["delta", "alpha", "charlie", "bravo"];
    let values_b = vec!["bravo", "charlie", "alpha", "delta"];

    assert_eq!(stable_sample(&values_a, 3, 0x5eed), stable_sample(&values_a, 3, 0x5eed));
    assert_eq!(stable_sample(&values_a, 3, 0x5eed), stable_sample(&values_b, 3, 0x5eed));
}
