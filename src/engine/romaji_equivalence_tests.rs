//! Checks that `RomajiJudge::preferred_spelling` returns exactly what the
//! enumeration over the first 4,096 accepted spellings returned.

use std::collections::{BTreeMap, BTreeSet};

use super::{RomajiJudge, base_spellings, spellings_split_uniquely};
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

/// Runs `check` over `cases` on every core and adds up what it returns.
fn on_all_cores<T: Sync>(cases: &[T], check: impl Fn(&T) -> usize + Sync) -> usize {
    let threads = std::thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get);
    let chunk = cases.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let handles = cases
            .chunks(chunk)
            .map(|part| scope.spawn(|| part.iter().map(&check).sum::<usize>()))
            .collect::<Vec<_>>();
        handles.into_iter().map(|handle| handle.join().expect("check thread")).sum()
    })
}

#[test]
fn preferred_spelling_matches_the_enumeration_on_bundled_words_and_phrases() {
    check_bundled(false, 10_000, 1_000);
}

/// Every bundled reading, sentences and passages included. The enumeration
/// is slow on long readings in a debug build, so this runs with
/// `cargo test --release -- --ignored preferred_spelling`.
#[test]
#[ignore = "slow in a debug build; run with --release"]
fn preferred_spelling_matches_the_enumeration_on_every_bundled_reading() {
    check_bundled(true, 20_000, 10_000);
}

fn check_bundled(long: bool, minimum_readings: usize, minimum_changed: usize) {
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
    let references = sets.iter().map(|(_, preferences)| preferences).collect::<Vec<_>>();
    let changed_by_preferences = on_all_cores(&interleaved, |reading| {
        let judge = RomajiJudge::new(reading);
        // The direct computation is used for every bundled reading.
        assert!(judge.preferred_spelling_is_direct(), "reading={reading:?}");
        let first = judge.accepted_spellings(1).into_iter().next().unwrap_or_default();
        let expected = judge.preferred_spellings_bruteforce_many(&references);
        let mut changed = 0;
        for ((name, preferences), expected) in sets.iter().zip(expected) {
            assert_eq!(judge.preferred_spelling(preferences), expected, "reading={reading:?}, preferences={name}");
            changed += usize::from(expected != first);
        }
        changed
    });
    // The preferences do change the spelling, so the comparison is not vacuous.
    assert!(changed_by_preferences > minimum_changed, "changed: {changed_by_preferences}");
    eprintln!(
        "compared {} (readings {} x {} preference sets), {changed_by_preferences} differ from the first spelling",
        readings.len() * sets.len(),
        readings.len(),
        sets.len()
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
    check_random(0x9E37_79B9_7F4A_7C15, 400);
}

/// The same with more cases; run with `cargo test --release -- --ignored preferred_spelling`.
#[test]
#[ignore = "slow in a debug build; run with --release"]
fn preferred_spelling_matches_the_enumeration_on_many_random_readings() {
    check_random(0xD1B5_4A32_D192_ED03, 10_000);
}

fn check_random(seed: u64, count: usize) {
    let mut random = Random(seed);
    let cases = (0..count)
        .map(|_| (random_reading(&mut random), random_preferences(&mut random)))
        .collect::<Vec<_>>();
    let mut long = 0_usize;
    let mut truncated = 0_usize;
    let mut enumerated = 0_usize;
    for (reading, _) in &cases {
        let judge = RomajiJudge::new(reading);
        long += usize::from(judge.moras().len() >= 20);
        truncated += usize::from(judge.accepted_spellings(4_097).len() > 4_096);
        enumerated += usize::from(!judge.preferred_spelling_is_direct());
    }
    let compared = on_all_cores(&cases, |(reading, preferences)| {
        let judge = RomajiJudge::new(reading);
        let expected = judge.preferred_spelling_bruteforce(preferences);
        assert_eq!(
            judge.preferred_spelling(preferences),
            expected,
            "reading={reading:?}, preferences={preferences:?}"
        );
        // The batched reference used for the bundled readings agrees too.
        assert_eq!(
            judge.preferred_spellings_bruteforce_many(&[preferences]),
            [expected],
            "reading={reading:?}"
        );
        1
    });
    assert_eq!(compared, cases.len());
    // Long readings past the 4,096-spelling cut-off are well represented.
    assert!(long > cases.len() / 3, "long: {long}");
    assert!(truncated > cases.len() / 3, "truncated: {truncated}");
    eprintln!("random cases {compared}: long {long}, truncated {truncated}, enumerated {enumerated}");
}

#[test]
fn the_4096_cut_off_is_kept() {
    // 13 し have 3^13 spellings; the first 4,096 all start with seven «shi»,
    // so a «si» preference cannot reach the first moras.
    let judge = RomajiJudge::new(&"し".repeat(13));
    let preferences = BTreeMap::from([("し".to_owned(), "si".to_owned())]);
    let spelling = judge.preferred_spelling(&preferences);
    assert_eq!(spelling, judge.preferred_spelling_bruteforce(&preferences));
    assert!(spelling.starts_with(&"shi".repeat(5)), "{spelling}");
    assert!(spelling.ends_with("si"), "{spelling}");
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
    assert!(direct.as_secs_f64() < 0.001, "{direct:?}");
}
