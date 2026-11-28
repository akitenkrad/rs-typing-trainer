//! Checks that `RomajiJudge::preferred_spelling` returns exactly what an
//! enumeration of every accepted spelling returns, and what the enumeration
//! over the first 4,096 accepted spellings (used up to d13c5a7) returned
//! whenever a reading has no more than 4,096 spellings.

use std::collections::{BTreeMap, BTreeSet};

use super::{RomajiJudge, base_spellings, best_resplit_tuple, first_preferred_tuple, spell_tuple, spellings_split_uniquely};
use crate::config::DataConfig;
use crate::data::{DataCatalog, DataKind};

/// Every mora `base_spellings` knows, plus ん, っ and a few characters that
/// are spelled as themselves or not at all.
const MORAS: &[&str] = &[
    "あ", "い", "う", "え", "お", "か", "き", "く", "け", "こ", "さ", "し", "す", "せ", "そ", "た", "ち", "つ", "て", "と", "な", "に", "ぬ", "ね",
    "の", "は", "ひ", "ふ", "へ", "ほ", "ま", "み", "む", "め", "も", "や", "ゆ", "よ", "ら", "り", "る", "れ", "ろ", "わ", "を", "が", "ぎ", "ぐ",
    "げ", "ご", "ざ", "じ", "ず", "ぜ", "ぞ", "だ", "ぢ", "づ", "で", "ど", "ば", "び", "ぶ", "べ", "ぼ", "ぱ", "ぴ", "ぷ", "ぺ", "ぽ", "ゔ", "ぁ",
    "ぃ", "ぅ", "ぇ", "ぉ", "ゃ", "ゅ", "ょ", "ゎ", "きゃ", "きゅ", "きょ", "しゃ", "しゅ", "しょ", "ちゃ", "ちゅ", "ちょ", "にゃ", "にゅ", "にょ",
    "ひゃ", "ひゅ", "ひょ", "みゃ", "みゅ", "みょ", "りゃ", "りゅ", "りょ", "ぎゃ", "ぎゅ", "ぎょ", "じゃ", "じゅ", "じょ", "びゃ", "びゅ", "びょ",
    "ぴゃ", "ぴゅ", "ぴょ", "てぃ", "でぃ", "ふぁ", "ふぃ", "ふぇ", "ふぉ", "しぇ", "じぇ", "ちぇ", "ー", "、", "。", "「", "」", "？", "！", "ん",
    "っ",
];

/// Characters that are not kana: spelled as themselves (ASCII) or not at all.
const OTHERS: &[&str] = &["a", "n", "N", "y", "k", "'", "1", " ", "漢"];

fn all_spellings(mora: &str) -> Vec<String> {
    match mora {
        "ん" => ["n", "nn", "n'", "xn", "nxn"].map(str::to_owned).to_vec(),
        "っ" => ["k", "s", "t", "c", "h", "p", "l", "x", "ltu", "xtu", "ltsu", "xtsu"]
            .map(str::to_owned)
            .to_vec(),
        _ => base_spellings(mora).into_iter().map(str::to_owned).collect(),
    }
}

fn hepburn() -> BTreeMap<String, String> {
    MORAS
        .iter()
        .filter_map(|mora| all_spellings(mora).first().map(|spelling| ((*mora).to_owned(), spelling.clone())))
        .collect()
}

fn changed(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    let mut preferences = hepburn();
    for (mora, spelling) in pairs {
        preferences.insert((*mora).to_owned(), (*spelling).to_owned());
    }
    preferences
}

fn preference_sets() -> Vec<(&'static str, BTreeMap<String, String>)> {
    vec![
        ("empty", BTreeMap::new()),
        ("hepburn", hepburn()),
        (
            "kunrei",
            changed(&[
                ("し", "si"),
                ("ち", "ti"),
                ("つ", "tu"),
                ("ふ", "hu"),
                ("じ", "zi"),
                ("しゃ", "sya"),
                ("しゅ", "syu"),
                ("しょ", "syo"),
                ("ちゃ", "tya"),
                ("ちゅ", "tyu"),
                ("ちょ", "tyo"),
                ("じゃ", "zya"),
                ("じゅ", "zyu"),
                ("じょ", "zyo"),
            ]),
        ),
        (
            "explicit",
            changed(&[
                ("ん", "nn"),
                ("っ", "ltu"),
                ("か", "ca"),
                ("く", "cu"),
                ("こ", "co"),
                ("し", "ci"),
                ("せ", "ce"),
                ("ちゃ", "cya"),
                ("ぁ", "xa"),
                ("ゃ", "xya"),
                ("てぃ", "texi"),
                ("ふぁ", "fuxa"),
            ]),
        ),
        // Only a few moras, so most of them score nothing.
        (
            "sparse",
            [("ん", "xn"), ("っ", "xtsu"), ("し", "si"), ("ふ", "hu")]
                .map(|(mora, spelling)| (mora.to_owned(), spelling.to_owned()))
                .into(),
        ),
    ]
}

/// The bundled Japanese readings: words and phrases, and with `long` also
/// the sentences and passages.
fn corpus_readings(long: bool) -> BTreeSet<String> {
    let home = tempfile::TempDir::new().expect("temporary home");
    let catalog = DataCatalog::load(&DataConfig::default(), home.path()).expect("bundled data");
    let mut readings = BTreeSet::new();
    for item in catalog.all_fixed_items() {
        let wanted = match item.kind {
            DataKind::JapaneseWord | DataKind::JapanesePhrase => true,
            DataKind::JapaneseSentence => long,
            _ => false,
        };
        if wanted && let Some(reading) = &item.reading {
            readings.insert(reading.clone());
        }
    }
    if long {
        for passage in catalog.passages_ja() {
            readings.insert(passage.reading.clone());
        }
    }
    readings
}

/// Runs `check` over `cases` on every core and adds up the counts it returns.
fn on_all_cores<T: Sync, const N: usize>(cases: &[T], check: impl Fn(&T) -> [usize; N] + Sync) -> [usize; N] {
    let threads = std::thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get);
    let chunk = cases.len().div_ceil(threads).max(1);
    let add = |mut total: [usize; N], counts: [usize; N]| {
        for (total, count) in total.iter_mut().zip(counts) {
            *total += count;
        }
        total
    };
    std::thread::scope(|scope| {
        let handles = cases
            .chunks(chunk)
            .map(|part| scope.spawn(|| part.iter().map(&check).fold([0; N], add)))
            .collect::<Vec<_>>();
        handles.into_iter().map(|handle| handle.join().expect("check thread")).fold([0; N], add)
    })
}

/// Readings with at most this many spellings in a debug build, and in a
/// release build, are also compared with the exhaustive enumeration.
const EXHAUSTIVE_DEBUG: u128 = 20_000;
const EXHAUSTIVE_RELEASE: u128 = 500_000;

/// Compares `preferred_spelling` for every preference set with the search
/// used for ambiguous readings (always), the exhaustive enumeration (up to
/// `exhaustive` spellings) and the old 4,096 enumeration (up to 4,096
/// spellings, where the old cut-off had no effect). Returns
/// `[old compared, exhaustive compared, exhaustive past 4,096]`.
fn compare_all(judge: &RomajiJudge, sets: &[(String, &BTreeMap<String, String>)], exhaustive: u128) -> (Vec<String>, [usize; 3]) {
    let reading = judge.moras().concat();
    let references = sets.iter().map(|(_, preferences)| *preferences).collect::<Vec<_>>();
    let actual = references
        .iter()
        .map(|preferences| judge.preferred_spelling(preferences))
        .collect::<Vec<_>>();
    for ((name, preferences), actual) in sets.iter().zip(&actual) {
        assert_eq!(
            &judge.preferred_spelling_by_search(preferences),
            actual,
            "search: reading={reading:?}, preferences={name}"
        );
    }
    let count = judge.spelling_tuple_count();
    let mut counts = [0; 3];
    if count <= 4_096 {
        let old = judge.preferred_spellings_bruteforce_many(&references);
        for (((name, _), actual), old) in sets.iter().zip(&actual).zip(old) {
            assert_eq!(actual, &old, "old enumeration: reading={reading:?}, preferences={name}");
        }
        counts[0] = 1;
    }
    if count <= exhaustive {
        let expected = judge.preferred_spellings_exhaustive_many(&references);
        for (((name, _), actual), expected) in sets.iter().zip(&actual).zip(expected) {
            assert_eq!(actual, &expected, "exhaustive: reading={reading:?}, preferences={name}");
        }
        counts[1] = 1;
        counts[2] = usize::from(count > 4_096);
    }
    (actual, counts)
}

#[test]
fn preferred_spelling_matches_the_enumeration_on_bundled_words_and_phrases() {
    check_bundled(false, 10_000, 1_000, EXHAUSTIVE_DEBUG, 20);
}

/// Every bundled reading, sentences and passages included. The enumeration
/// is slow on long readings in a debug build, so this runs with
/// `cargo test --release -- --ignored preferred_spelling`.
#[test]
#[ignore = "slow in a debug build; run with --release"]
fn preferred_spelling_matches_the_enumeration_on_every_bundled_reading() {
    check_bundled(true, 20_000, 10_000, EXHAUSTIVE_RELEASE, 1_000);
}

fn check_bundled(long: bool, minimum_readings: usize, minimum_changed: usize, exhaustive: u128, minimum_past: usize) {
    // Long readings first, so the threads finish at about the same time.
    let mut readings = corpus_readings(long).into_iter().collect::<Vec<_>>();
    assert!(readings.len() > minimum_readings, "bundled readings: {}", readings.len());
    readings.sort_by_key(|reading| std::cmp::Reverse(reading.chars().count()));
    let mut interleaved = Vec::with_capacity(readings.len());
    let threads = std::thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get);
    for offset in 0..threads {
        interleaved.extend(readings.iter().skip(offset).step_by(threads).cloned());
    }
    let sets = preference_sets();
    let named = sets
        .iter()
        .map(|(name, preferences)| ((*name).to_owned(), preferences))
        .collect::<Vec<_>>();
    let [changed, old, compared, past] = on_all_cores(&interleaved, |reading| {
        let judge = RomajiJudge::new(reading);
        // The mora-by-mora shortcut is used for every bundled reading.
        assert!(judge.preferred_spelling_is_direct(), "reading={reading:?}");
        let first = judge.accepted_spellings(1).into_iter().next().unwrap_or_default();
        let (actual, [old, compared, past]) = compare_all(&judge, &named, exhaustive);
        let changed = actual.iter().filter(|spelling| **spelling != first).count();
        [changed, old, compared, past]
    });
    // The preferences do change the spelling, so the comparison is not vacuous.
    assert!(changed > minimum_changed, "changed: {changed}");
    assert!(compared > minimum_readings / 2, "compared with the exhaustive enumeration: {compared}");
    assert!(past > minimum_past, "compared past 4,096 spellings: {past}");
    eprintln!(
        "bundled readings {} x {} preference sets: search {}, exhaustive {compared} (past 4,096: {past}), old enumeration {old}; {changed} differ from the first spelling",
        readings.len(),
        sets.len(),
        readings.len(),
    );
}

/// A fixed-seed xorshift generator, so the random cases are the same every run.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        value
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % bound as u64).expect("small bound")
    }

    fn pick<'a>(&mut self, values: &[&'a str]) -> &'a str {
        values[self.below(values.len())]
    }
}

fn random_reading(random: &mut Random) -> String {
    let length = match random.below(4) {
        0 => 1 + random.below(6),
        1 => 6 + random.below(14),
        _ => 20 + random.below(25),
    };
    let mut reading = String::new();
    for _ in 0..length {
        let mora = match random.below(20) {
            0..=2 => "ん",
            3..=4 => "っ",
            5 => random.pick(OTHERS),
            6..=7 => random.pick(&["ぁ", "ぃ", "ぅ", "ぇ", "ぉ", "ゃ", "ゅ", "ょ", "ゎ"]),
            _ => random.pick(MORAS),
        };
        reading.push_str(mora);
    }
    if random.below(3) == 0 {
        reading.push('ん');
    }
    reading
}

fn random_preferences(random: &mut Random) -> BTreeMap<String, String> {
    let density = random.below(4);
    let mut preferences = BTreeMap::new();
    for mora in MORAS {
        let spellings = all_spellings(mora);
        if spellings.is_empty() || random.below(4) > density {
            continue;
        }
        let spelling = spellings[random.below(spellings.len())].clone();
        preferences.insert((*mora).to_owned(), spelling);
    }
    preferences
}

#[test]
fn preferred_spelling_matches_the_enumeration_on_random_readings() {
    check_random(0x9E37_79B9_7F4A_7C15, 400, EXHAUSTIVE_DEBUG);
}

/// The same with more cases; run with `cargo test --release -- --ignored preferred_spelling`.
#[test]
#[ignore = "slow in a debug build; run with --release"]
fn preferred_spelling_matches_the_enumeration_on_many_random_readings() {
    check_random(0xD1B5_4A32_D192_ED03, 10_000, EXHAUSTIVE_RELEASE);
}

fn check_random(seed: u64, count: usize, exhaustive: u128) {
    let mut random = Random(seed);
    let cases = (0..count)
        .map(|_| (random_reading(&mut random), random_preferences(&mut random)))
        .collect::<Vec<_>>();
    let [compared, long, truncated, ambiguous, old, exhaustive_compared, past] = on_all_cores(&cases, |(reading, preferences)| {
        let judge = RomajiJudge::new(reading);
        let (_, [old, compared, past]) = compare_all(&judge, &[(format!("{preferences:?}"), preferences)], exhaustive);
        [
            1,
            usize::from(judge.moras().len() >= 20),
            usize::from(judge.spelling_tuple_count() > 4_096),
            usize::from(!judge.preferred_spelling_is_direct()),
            old,
            compared,
            past,
        ]
    });
    assert_eq!(compared, cases.len());
    // Long readings past the old 4,096-spelling cut-off are well represented,
    // and enough of them are small enough to enumerate.
    assert!(long > cases.len() / 3, "long: {long}");
    assert!(truncated > cases.len() / 3, "truncated: {truncated}");
    assert!(past > cases.len() / 50, "compared past 4,096 spellings: {past}");
    eprintln!(
        "random cases {compared}: long {long}, past 4,096 {truncated}, ambiguous {ambiguous}; exhaustive {exhaustive_compared} (past 4,096: {past}), old enumeration {old}"
    );
}

/// Variant tables over a two-letter alphabet, where many tuples spell the
/// same string, checked against an enumeration that splits every spelling
/// the way `spelling_segments` does.
#[test]
fn the_search_matches_the_enumeration_on_ambiguous_variant_tables() {
    let mut random = Random(0x2545_F491_4F6C_DD1D);
    let mut ambiguous = 0;
    let mut duplicated = 0;
    for _ in 0..3_000 {
        let length = 1 + random.below(7);
        let variants = (0..length)
            .map(|_| {
                let mut options = Vec::new();
                for _ in 0..1 + random.below(4) {
                    let spelling = (0..1 + random.below(3))
                        .map(|_| if random.below(2) == 0 { 'a' } else { 'b' })
                        .collect::<String>();
                    if !options.contains(&spelling) {
                        options.push(spelling);
                    }
                }
                options
            })
            .collect::<Vec<_>>();
        let preferred = variants
            .iter()
            .map(|options| (random.below(3) > 0).then(|| random.below(options.len())))
            .collect::<Vec<_>>();
        let matches = |index: usize, variant: usize| preferred[index] == Some(variant);
        let (expected, has_duplicates) = enumerate_table(&variants, matches);
        assert_eq!(
            spell_tuple(&variants, &best_resplit_tuple(&variants, matches)),
            expected,
            "variants={variants:?}, preferred={preferred:?}"
        );
        if spellings_split_uniquely(&variants) {
            assert!(!has_duplicates, "variants={variants:?}");
            assert_eq!(spell_tuple(&variants, &first_preferred_tuple(&variants, matches)), expected);
        } else {
            ambiguous += 1;
        }
        duplicated += usize::from(has_duplicates);
    }
    assert!(duplicated > 500, "tables where two tuples spell the same string: {duplicated}");
    eprintln!("ambiguous tables {ambiguous}, with duplicate spellings {duplicated}");
}

/// The first spelling with the most preferred moras, enumerating every tuple
/// and splitting its spelling by trying the variants in order; also whether
/// two tuples spelled the same string.
fn enumerate_table(variants: &[Vec<String>], matches: impl Fn(usize, usize) -> bool) -> (String, bool) {
    fn split(variants: &[Vec<String>], index: usize, rest: &str) -> Option<Vec<usize>> {
        if index == variants.len() {
            return rest.is_empty().then(Vec::new);
        }
        variants[index].iter().enumerate().find_map(|(variant, spelling)| {
            let tail = split(variants, index + 1, rest.strip_prefix(spelling.as_str())?)?;
            Some(std::iter::once(variant).chain(tail).collect())
        })
    }
    let mut tuple = vec![0_usize; variants.len()];
    let mut best = (0_usize, spell_tuple(variants, &tuple));
    let mut seen = BTreeSet::new();
    let mut duplicates = false;
    loop {
        let spelling = spell_tuple(variants, &tuple);
        let parts = split(variants, 0, &spelling).expect("a spelling splits");
        let score = parts.iter().enumerate().filter(|&(index, &variant)| matches(index, variant)).count();
        if score > best.0 {
            best = (score, spelling.clone());
        }
        duplicates |= !seen.insert(spelling);
        // The next tuple in candidate order.
        let Some(position) = (0..variants.len()).rev().find(|&index| tuple[index] + 1 < variants[index].len()) else {
            return (best.1, duplicates);
        };
        tuple[position] += 1;
        for later in &mut tuple[position + 1..] {
            *later = 0;
        }
    }
}

#[test]
fn every_preferred_mora_counts_however_long_the_reading_is() {
    // 13 し have 3^13 spellings. Up to d13c5a7 only the first 4,096 were
    // looked at; they all start with seven «shi», so a «si» preference could
    // not reach the first moras.
    let judge = RomajiJudge::new(&"し".repeat(13));
    assert!(judge.spelling_tuple_count() > 4_096);
    let preferences = BTreeMap::from([("し".to_owned(), "si".to_owned())]);
    assert_eq!(judge.preferred_spelling(&preferences), "si".repeat(13));
    assert!(judge.preferred_spelling_bruteforce(&preferences).starts_with(&"shi".repeat(5)));
    assert_eq!(judge.preferred_spellings_exhaustive_many(&[&preferences]), ["si".repeat(13)]);

    // A sentence: the preferences apply from its first mora on.
    let judge = RomajiJudge::new("しんじゅくえきのちかくにあるきっさてんでしゅくだいをしよう");
    assert!(judge.spelling_tuple_count() > 4_096);
    let preferences = changed(&[("し", "si"), ("ち", "ti"), ("ん", "nn"), ("じゅ", "zyu"), ("しゅ", "syu")]);
    assert_eq!(
        judge.preferred_spelling(&preferences),
        "sinnzyukuekinotikakuniarukissatenndesyukudaiwosiyou"
    );
    assert!(judge.preferred_spelling_bruteforce(&preferences).starts_with("shin"));

    // A preference the context rules out is still not used: ん before a
    // vowel cannot be «n», so the next preferred-or-first variant is kept.
    let judge = RomajiJudge::new(&"しんあい".repeat(4));
    let preferences = BTreeMap::from([("ん".to_owned(), "n".to_owned()), ("し".to_owned(), "si".to_owned())]);
    assert_eq!(judge.preferred_spelling(&preferences), "sinnai".repeat(4));
}

#[test]
fn a_variant_that_could_spell_like_another_tuple_is_detected() {
    let owned = |rows: &[&[&str]]| {
        rows.iter()
            .map(|row| row.iter().map(|value| (*value).to_owned()).collect::<Vec<_>>())
            .collect::<Vec<_>>()
    };
    // «n» + «na» and «nn» + «a» are the same string.
    assert!(!spellings_split_uniquely(&owned(&[&["n", "nn"], &["na", "a"]])));
    // «l» + «tsu…» against «ltsu»: the rest can spell «tsu».
    assert!(!spellings_split_uniquely(&owned(&[&["l", "ltsu"], &["t"], &["su"]])));
    // The real contexts of ん and っ never allow it.
    assert!(spellings_split_uniquely(&owned(&[&["n", "nn", "n'", "xn"], &["ka", "ca"]])));
    assert!(spellings_split_uniquely(&owned(&[
        &["l", "x", "ltu", "xtu", "ltsu", "xtsu"],
        &["la", "xa"]
    ])));
    assert!(RomajiJudge::new("こんにちは").preferred_spelling_is_direct());
}

#[test]
#[ignore = "timing; run with --release"]
fn preferred_spelling_speed_on_a_long_reading() {
    let reading = "しんじゅくえきのちかくにあるきっさてんでしゅくだいをしよう";
    let judge = RomajiJudge::new(reading);
    assert!(judge.moras().len() >= 20);
    let preferences = changed(&[("し", "si"), ("ち", "ti"), ("つ", "tu"), ("ん", "nn"), ("しゅ", "syu")]);
    let rounds = 2_000_u32;
    let started = std::time::Instant::now();
    let mut length = 0;
    for _ in 0..rounds {
        length += std::hint::black_box(judge.preferred_spelling(std::hint::black_box(&preferences))).len();
    }
    let direct = started.elapsed() / rounds;
    let started = std::time::Instant::now();
    for _ in 0..20 {
        length += std::hint::black_box(judge.preferred_spelling_bruteforce(&preferences)).len();
    }
    let enumeration = started.elapsed() / 20;
    eprintln!(
        "{} moras: preferred_spelling {:.4} ms, enumeration {:.4} ms ({length})",
        judge.moras().len(),
        direct.as_secs_f64() * 1_000.0,
        enumeration.as_secs_f64() * 1_000.0
    );
    // At most 0.1 ms for one long reading.
    assert!(direct.as_secs_f64() < 0.000_1, "{direct:?}");
}
