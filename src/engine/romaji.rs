//! Incremental kana-to-romaji judge.

use std::collections::BTreeMap;
use std::collections::HashSet;

use crate::engine::judge::{Judge, JudgeResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RomajiOptions {
    pub accept_apostrophe: bool,
    pub accept_xn: bool,
}

impl Default for RomajiOptions {
    fn default() -> Self {
        Self {
            accept_apostrophe: true,
            accept_xn: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Route {
    mora_index: usize,
    spelling: Vec<char>,
    offset: usize,
}

#[derive(Clone, Debug)]
pub struct RomajiJudge {
    moras: Vec<String>,
    options: RomajiOptions,
    active: Vec<Route>,
    typed: String,
    expected: Vec<String>,
    guide: String,
    history: Vec<(Vec<Route>, String)>,
}

impl RomajiJudge {
    pub fn new(reading: &str) -> Self {
        Self::with_options(reading, RomajiOptions::default())
    }

    pub fn with_options(reading: &str, options: RomajiOptions) -> Self {
        let moras = split_moras(reading);
        let mut judge = Self {
            moras,
            options,
            active: Vec::new(),
            typed: String::new(),
            expected: Vec::new(),
            guide: String::new(),
            history: Vec::new(),
        };
        judge.active = judge.routes_for(0);
        judge.refresh_display();
        judge
    }

    pub fn guide(&self) -> &str {
        &self.guide
    }

    pub fn typed(&self) -> &str {
        &self.typed
    }

    pub fn is_complete(&self) -> bool {
        !self.moras.is_empty() && self.active.iter().any(|route| route.mora_index == self.moras.len())
    }

    pub fn moras(&self) -> &[String] {
        &self.moras
    }

    /// Accepted spelling variants for each mora in this reading's context.
    pub fn spelling_variants(&self) -> Vec<(String, Vec<String>)> {
        self.moras
            .iter()
            .enumerate()
            .map(|(index, mora)| (mora.clone(), self.variants_for(index)))
            .collect()
    }

    pub fn has_spelling_using(&self, allowed: &HashSet<char>) -> bool {
        self.accepted_spellings(4096)
            .iter()
            .any(|spelling| spelling.chars().all(|character| allowed.contains(&character)))
    }

    pub fn accepted_spellings(&self, limit: usize) -> Vec<String> {
        if limit == 0 || self.moras.is_empty() {
            return Vec::new();
        }
        let mut results = vec![String::new()];
        for index in 0..self.moras.len() {
            let variants = self.variants_for(index);
            let mut next = Vec::new();
            'outer: for prefix in &results {
                for variant in &variants {
                    if next.len() == limit {
                        break 'outer;
                    }
                    let mut spelling = prefix.clone();
                    spelling.push_str(variant);
                    next.push(spelling);
                }
            }
            results = next;
            if results.is_empty() {
                break;
            }
        }
        results
    }

    /// Split one accepted spelling into the spelling chosen for each mora.
    pub fn spelling_segments(&self, spelling: &str) -> Option<Vec<(String, String)>> {
        self.spelling_segments_from(0, spelling)
    }

    /// Expand the reading using the player's most-used spelling for each mora.
    ///
    /// The result is the first of the first [`PREFERRED_SPELLING_LIMIT`]
    /// accepted spellings (in the order of [`Self::accepted_spellings`]) that
    /// uses the preferred spelling for the most moras. It is computed directly
    /// from the per-mora variants instead of enumerating the spellings: the
    /// candidates are the tuples of variant indices up to the mixed-radix
    /// number `L - 1`, so they split into «equal up to position k, smaller at
    /// k, free after k» branches plus `L - 1` itself, and each branch's best
    /// tuple is chosen mora by mora.
    ///
    /// That shortcut scores each tuple by its own variants, while the
    /// enumeration scores a spelling by re-splitting it with
    /// [`Self::spelling_segments`]. The two agree only when no two tuples
    /// spell the same string; when the variants of this reading could make
    /// that happen, the enumeration is used instead.
    pub fn preferred_spelling(&self, preferences: &BTreeMap<String, String>) -> String {
        if self.moras.is_empty() {
            return String::new();
        }
        let variants = (0..self.moras.len()).map(|index| self.variants_for(index)).collect::<Vec<_>>();
        if variants.iter().any(Vec::is_empty) {
            return String::new();
        }
        if !spellings_split_uniquely(&variants) {
            return self.preferred_spelling_by_enumeration(preferences);
        }
        self.preferred_spelling_direct(&variants, preferences, PREFERRED_SPELLING_LIMIT)
    }

    /// The enumeration that [`Self::preferred_spelling`] replaces, kept as the
    /// reference the direct computation is checked against.
    #[cfg(test)]
    pub(crate) fn preferred_spelling_bruteforce(&self, preferences: &BTreeMap<String, String>) -> String {
        self.preferred_spelling_by_enumeration(preferences)
    }

    /// [`Self::preferred_spelling_bruteforce`] for several preference sets,
    /// enumerating and splitting the spellings once.
    #[cfg(test)]
    pub(crate) fn preferred_spellings_bruteforce_many(&self, sets: &[&BTreeMap<String, String>]) -> Vec<String> {
        let candidates = self.accepted_spellings(PREFERRED_SPELLING_LIMIT);
        let segments = candidates
            .iter()
            .map(|candidate| self.spelling_segments(candidate).unwrap_or_default())
            .collect::<Vec<_>>();
        sets.iter()
            .map(|preferences| {
                let mut best = candidates.first().cloned().unwrap_or_default();
                let mut best_score = 0_usize;
                for (candidate, segments) in candidates.iter().zip(&segments) {
                    let score = segments.iter().filter(|(mora, spelling)| preferences.get(mora) == Some(spelling)).count();
                    if score > best_score {
                        best.clone_from(candidate);
                        best_score = score;
                    }
                }
                best
            })
            .collect()
    }

    /// Whether [`Self::preferred_spelling`] can use the direct computation for
    /// this reading (no two spellings of it are the same string).
    #[cfg(test)]
    pub(crate) fn preferred_spelling_is_direct(&self) -> bool {
        let variants = (0..self.moras.len()).map(|index| self.variants_for(index)).collect::<Vec<_>>();
        spellings_split_uniquely(&variants)
    }

    fn preferred_spelling_direct(&self, variants: &[Vec<String>], preferences: &BTreeMap<String, String>, limit: usize) -> String {
        let count = variants.len();
        let matches = |index: usize, variant: usize| preferences.get(&self.moras[index]) == Some(&variants[index][variant]);
        // The last candidate, `min(limit, product) - 1`, as mixed-radix digits.
        let product = variants
            .iter()
            .fold(1_u128, |product, options| product.saturating_mul(options.len() as u128));
        let candidates = (limit as u128).min(product);
        if candidates == 0 {
            return String::new();
        }
        let mut remaining = candidates - 1;
        let mut last = vec![0_usize; count];
        for index in (0..count).rev() {
            let radix = variants[index].len() as u128;
            last[index] = usize::try_from(remaining % radix).expect("digit below the variant count");
            remaining /= radix;
        }
        // A free position takes its first preferred variant, or the first one.
        let free = (0..count)
            .map(|index| {
                (0..variants[index].len())
                    .find(|&variant| matches(index, variant))
                    .map_or((0, 0), |variant| (variant, 1))
            })
            .collect::<Vec<_>>();
        let mut free_score_after = vec![0_usize; count + 1];
        for index in (0..count).rev() {
            free_score_after[index] = free_score_after[index + 1] + free[index].1;
        }
        // Branches are visited in candidate order and only a strictly higher
        // score replaces the best, like the enumeration.
        let mut best: Option<(usize, Vec<usize>)> = None;
        let mut prefix_score = 0_usize;
        for position in 0..count {
            if last[position] > 0 {
                let (variant, matched) = (0..last[position])
                    .find(|&variant| matches(position, variant))
                    .map_or((0, 0), |variant| (variant, 1));
                let score = prefix_score + matched + free_score_after[position + 1];
                if best.as_ref().is_none_or(|(best_score, _)| score > *best_score) {
                    let mut tuple = last[..position].to_vec();
                    tuple.push(variant);
                    tuple.extend(free[position + 1..].iter().map(|&(variant, _)| variant));
                    best = Some((score, tuple));
                }
            }
            prefix_score += usize::from(matches(position, last[position]));
        }
        if best.as_ref().is_none_or(|(best_score, _)| prefix_score > *best_score) {
            best = Some((prefix_score, last));
        }
        // With no preferred mora anywhere the enumeration keeps the first candidate.
        let tuple = match best {
            Some((score, tuple)) if score > 0 => tuple,
            _ => vec![0; count],
        };
        tuple
            .iter()
            .enumerate()
            .map(|(index, &variant)| variants[index][variant].as_str())
            .collect()
    }

    fn preferred_spelling_by_enumeration(&self, preferences: &BTreeMap<String, String>) -> String {
        let candidates = self.accepted_spellings(PREFERRED_SPELLING_LIMIT);
        let mut best = candidates.first().cloned().unwrap_or_default();
        let mut best_score = 0_usize;
        for candidate in candidates {
            let score = self
                .spelling_segments(&candidate)
                .unwrap_or_default()
                .iter()
                .filter(|(mora, spelling)| preferences.get(mora) == Some(spelling))
                .count();
            if score > best_score {
                best = candidate;
                best_score = score;
            }
        }
        best
    }

    fn spelling_segments_from(&self, mora_index: usize, remaining: &str) -> Option<Vec<(String, String)>> {
        if mora_index == self.moras.len() {
            return remaining.is_empty().then(Vec::new);
        }
        for variant in self.variants_for(mora_index) {
            let Some(rest) = remaining.strip_prefix(&variant) else {
                continue;
            };
            if let Some(mut tail) = self.spelling_segments_from(mora_index + 1, rest) {
                let mut segments = vec![(self.moras[mora_index].clone(), variant)];
                segments.append(&mut tail);
                return Some(segments);
            }
        }
        None
    }

    fn routes_for(&self, index: usize) -> Vec<Route> {
        if index >= self.moras.len() {
            return vec![Route {
                mora_index: self.moras.len(),
                spelling: Vec::new(),
                offset: 0,
            }];
        }
        self.variants_for(index)
            .into_iter()
            .map(|spelling| Route {
                mora_index: index,
                spelling: spelling.chars().collect(),
                offset: 0,
            })
            .collect()
    }

    fn variants_for(&self, index: usize) -> Vec<String> {
        let mora = &self.moras[index];
        if mora == "ん" {
            let explicit_only = index + 1 == self.moras.len() || self.moras.get(index + 1).is_none_or(|next| n_needs_explicit_spelling(next));
            let mut variants = if explicit_only {
                vec!["nn".to_owned()]
            } else {
                vec!["n".to_owned(), "nn".to_owned()]
            };
            if self.options.accept_apostrophe {
                variants.push("n'".to_owned());
            }
            if self.options.accept_xn {
                variants.push("xn".to_owned());
                // Keep the explicit table spelling `honxn` for terminal ん:
                // the provisional n may be followed by the xn escape.
                if index + 1 == self.moras.len() {
                    variants.push("nxn".to_owned());
                }
            }
            return variants;
        }
        if mora == "っ" {
            let mut variants = Vec::new();
            if let Some(next) = self.moras.get(index + 1) {
                for spelling in base_spellings(next) {
                    if let Some(first) = spelling.chars().next()
                        && first.is_ascii_alphabetic()
                        && !matches!(first, 'a' | 'i' | 'u' | 'e' | 'o' | 'n')
                    {
                        push_unique(&mut variants, first.to_string());
                    }
                }
            }
            for spelling in ["ltu", "xtu", "ltsu", "xtsu"] {
                push_unique(&mut variants, spelling.to_owned());
            }
            return variants;
        }
        let spellings = base_spellings(mora);
        if spellings.is_empty() && mora.chars().count() == 1 && mora.is_ascii() {
            vec![mora.clone()]
        } else {
            spellings.into_iter().map(str::to_owned).collect()
        }
    }

    fn refresh_display(&mut self) {
        let mut expected = Vec::new();
        for route in &self.active {
            if let Some(character) = route.spelling.get(route.offset) {
                push_unique(&mut expected, character.to_string());
            }
        }
        self.expected = expected;

        let mut guide = self.typed.clone();
        if let Some(route) = self.active.first() {
            for character in route.spelling.iter().skip(route.offset) {
                guide.push(*character);
            }
            let next_index = if route.mora_index < self.moras.len() {
                route.mora_index + 1
            } else {
                self.moras.len()
            };
            for index in next_index..self.moras.len() {
                if let Some(representative) = self.variants_for(index).first() {
                    guide.push_str(representative);
                }
            }
        }
        self.guide = guide;
    }
}

impl Judge for RomajiJudge {
    fn position(&self) -> usize {
        self.typed.chars().count()
    }

    fn expected_candidates(&self) -> &[String] {
        &self.expected
    }

    fn input(&mut self, typed: &str) -> JudgeResult {
        let mut characters = typed.chars();
        let Some(character) = characters.next() else {
            return JudgeResult::Mistype;
        };
        if characters.next().is_some() || self.is_complete() {
            return JudgeResult::Mistype;
        }

        let mut next_routes = Vec::new();
        for route in &self.active {
            if route.spelling.get(route.offset) != Some(&character) {
                continue;
            }
            let next_offset = route.offset + 1;
            if next_offset == route.spelling.len() {
                next_routes.extend(self.routes_for(route.mora_index + 1));
            } else {
                next_routes.push(Route {
                    mora_index: route.mora_index,
                    spelling: route.spelling.clone(),
                    offset: next_offset,
                });
            }
        }
        let mut seen = HashSet::new();
        next_routes.retain(|route| seen.insert(route.clone()));
        if next_routes.is_empty() {
            return JudgeResult::Mistype;
        }

        self.history.push((self.active.clone(), self.typed.clone()));
        self.active = next_routes;
        self.typed.push(character);
        self.refresh_display();
        if self.is_complete() {
            JudgeResult::ItemCompleted
        } else {
            JudgeResult::Correct
        }
    }

    fn backspace(&mut self) -> bool {
        let Some((active, typed)) = self.history.pop() else {
            return false;
        };
        self.active = active;
        self.typed = typed;
        self.refresh_display();
        true
    }

    fn guide(&self) -> String {
        self.guide.clone()
    }

    fn completed_reading_chars(&self) -> usize {
        let completed_moras = self.active.iter().map(|route| route.mora_index).min().unwrap_or(0);
        self.moras.iter().take(completed_moras).map(|mora| mora.chars().count()).sum()
    }
}

/// How many accepted spellings [`RomajiJudge::preferred_spelling`] considers.
const PREFERRED_SPELLING_LIMIT: usize = 4_096;

/// Whether every tuple of variants spells a different string, so that
/// [`RomajiJudge::spelling_segments`] gives each spelling back as its own
/// tuple.
///
/// Two tuples that spell the same string first differ at a mora where one
/// variant is a proper prefix of the other; the moras after it must then be
/// able to spell something that starts with the longer variant's extra
/// characters. This checks that necessary condition, so `true` is certain and
/// `false` only means «not shown» (only ん and っ have such prefix pairs).
fn spellings_split_uniquely(variants: &[Vec<String>]) -> bool {
    variants.iter().enumerate().all(|(index, options)| {
        options.iter().all(|shorter| {
            options.iter().all(|longer| {
                longer.len() <= shorter.len()
                    || !longer.starts_with(shorter.as_str())
                    || !can_spell_prefix(variants, index + 1, &longer[shorter.len()..])
            })
        })
    })
}

/// Whether the moras from `index` on can spell a string that starts with `needed`.
fn can_spell_prefix(variants: &[Vec<String>], index: usize, needed: &str) -> bool {
    if needed.is_empty() {
        return true;
    }
    let Some(options) = variants.get(index) else {
        return false;
    };
    options.iter().any(|variant| {
        variant.starts_with(needed)
            || needed
                .strip_prefix(variant.as_str())
                .is_some_and(|rest| can_spell_prefix(variants, index + 1, rest))
    })
}

fn push_unique<T: PartialEq>(values: &mut Vec<T>, value: T) {
    if !values.contains(&value) {
        values.push(value);
    }
}

fn split_moras(reading: &str) -> Vec<String> {
    let normalized = reading.chars().map(normalize_katakana).collect::<String>();
    let characters = normalized.chars().collect::<Vec<_>>();
    let mut result = Vec::new();
    let mut index = 0;
    while index < characters.len() {
        if index + 1 < characters.len() {
            let combined = format!("{}{}", characters[index], characters[index + 1]);
            if is_small_kana(characters[index + 1]) && !base_spellings(&combined).is_empty() {
                result.push(combined);
                index += 2;
                continue;
            }
        }
        result.push(characters[index].to_string());
        index += 1;
    }
    result
}

fn normalize_katakana(character: char) -> char {
    let code = character as u32;
    if (0x30A1..=0x30F6).contains(&code) {
        char::from_u32(code - 0x60).unwrap_or(character)
    } else {
        character
    }
}

fn is_small_kana(character: char) -> bool {
    matches!(character, 'ぁ' | 'ぃ' | 'ぅ' | 'ぇ' | 'ぉ' | 'ゃ' | 'ゅ' | 'ょ' | 'ゎ')
}

fn n_needs_explicit_spelling(next: &str) -> bool {
    let first = next.chars().next().unwrap_or_default();
    if matches!(
        first,
        'あ' | 'い' | 'う' | 'え' | 'お' | 'な' | 'に' | 'ぬ' | 'ね' | 'の' | 'や' | 'ゆ' | 'よ' | 'ん'
    ) {
        return true;
    }
    if matches!(
        first,
        'ぁ' | 'ぃ' | 'ぅ' | 'ぇ' | 'ぉ' | 'ゃ' | 'ゅ' | 'ょ' | '、' | '。' | '？' | '！' | 'ー'
    ) {
        return true;
    }
    if first.is_ascii() {
        return !first.is_ascii_alphabetic() || matches!(first.to_ascii_lowercase(), 'a' | 'i' | 'u' | 'e' | 'o' | 'n' | 'y');
    }
    false
}

#[allow(clippy::too_many_lines)]
fn base_spellings(mora: &str) -> Vec<&'static str> {
    match mora {
        "あ" => vec!["a"],
        "い" => vec!["i"],
        "う" => vec!["u"],
        "え" => vec!["e"],
        "お" => vec!["o"],
        "か" => vec!["ka", "ca"],
        "き" => vec!["ki"],
        "く" => vec!["ku", "cu", "qu"],
        "け" => vec!["ke"],
        "こ" => vec!["ko", "co"],
        "さ" => vec!["sa"],
        "し" => vec!["shi", "si", "ci"],
        "す" => vec!["su"],
        "せ" => vec!["se", "ce"],
        "そ" => vec!["so"],
        "た" => vec!["ta"],
        "ち" => vec!["chi", "ti"],
        "つ" => vec!["tsu", "tu"],
        "て" => vec!["te"],
        "と" => vec!["to"],
        "な" => vec!["na"],
        "に" => vec!["ni"],
        "ぬ" => vec!["nu"],
        "ね" => vec!["ne"],
        "の" => vec!["no"],
        "は" => vec!["ha"],
        "ひ" => vec!["hi"],
        "ふ" => vec!["fu", "hu"],
        "へ" => vec!["he"],
        "ほ" => vec!["ho"],
        "ま" => vec!["ma"],
        "み" => vec!["mi"],
        "む" => vec!["mu"],
        "め" => vec!["me"],
        "も" => vec!["mo"],
        "や" => vec!["ya"],
        "ゆ" => vec!["yu"],
        "よ" => vec!["yo"],
        "ら" => vec!["ra"],
        "り" => vec!["ri"],
        "る" => vec!["ru"],
        "れ" => vec!["re"],
        "ろ" => vec!["ro"],
        "わ" => vec!["wa"],
        "を" => vec!["wo"],
        "が" => vec!["ga"],
        "ぎ" => vec!["gi"],
        "ぐ" => vec!["gu"],
        "げ" => vec!["ge"],
        "ご" => vec!["go"],
        "ざ" => vec!["za"],
        "じ" => vec!["ji", "zi"],
        "ず" => vec!["zu"],
        "ぜ" => vec!["ze"],
        "ぞ" => vec!["zo"],
        "だ" => vec!["da"],
        "ぢ" => vec!["di"],
        "づ" => vec!["du"],
        "で" => vec!["de"],
        "ど" => vec!["do"],
        "ば" => vec!["ba"],
        "び" => vec!["bi"],
        "ぶ" => vec!["bu"],
        "べ" => vec!["be"],
        "ぼ" => vec!["bo"],
        "ぱ" => vec!["pa"],
        "ぴ" => vec!["pi"],
        "ぷ" => vec!["pu"],
        "ぺ" => vec!["pe"],
        "ぽ" => vec!["po"],
        "ゔ" => vec!["vu"],
        "ぁ" => vec!["la", "xa"],
        "ぃ" => vec!["li", "xi"],
        "ぅ" => vec!["lu", "xu"],
        "ぇ" => vec!["le", "xe"],
        "ぉ" => vec!["lo", "xo"],
        "ゃ" => vec!["lya", "xya"],
        "ゅ" => vec!["lyu", "xyu"],
        "ょ" => vec!["lyo", "xyo"],
        "ゎ" => vec!["lwa", "xwa"],
        "きゃ" => vec!["kya", "kixya", "kilya"],
        "きゅ" => vec!["kyu", "kixyu", "kilyu"],
        "きょ" => vec!["kyo", "kixyo", "kilyo"],
        "しゃ" => vec!["sha", "sya", "sixya", "silya", "shixya", "shilya"],
        "しゅ" => vec!["shu", "syu", "sixyu", "silyu", "shixyu", "shilyu"],
        "しょ" => vec!["sho", "syo", "sixyo", "silyo", "shixyo", "shilyo"],
        "ちゃ" => vec!["cha", "tya", "cya", "chixya", "chilya", "tixya", "tilya"],
        "ちゅ" => vec!["chu", "tyu", "cyu", "chixyu", "chilyu", "tixyu", "tilyu"],
        "ちょ" => vec!["cho", "tyo", "cyo", "chixyo", "chilyo", "tixyo", "tilyo"],
        "にゃ" => vec!["nya", "nixya", "nilya"],
        "にゅ" => vec!["nyu", "nixyu", "nilyu"],
        "にょ" => vec!["nyo", "nixyo", "nilyo"],
        "ひゃ" => vec!["hya", "hixya", "hilya"],
        "ひゅ" => vec!["hyu", "hixyu", "hilyu"],
        "ひょ" => vec!["hyo", "hixyo", "hilyo"],
        "みゃ" => vec!["mya", "mixya", "milya"],
        "みゅ" => vec!["myu", "mixyu", "milyu"],
        "みょ" => vec!["myo", "mixyo", "milyo"],
        "りゃ" => vec!["rya", "rixya", "rilya"],
        "りゅ" => vec!["ryu", "rixyu", "rilyu"],
        "りょ" => vec!["ryo", "rixyo", "rilyo"],
        "ぎゃ" => vec!["gya", "gixya", "gilya"],
        "ぎゅ" => vec!["gyu", "gixyu", "gilyu"],
        "ぎょ" => vec!["gyo", "gixyo", "gilyo"],
        "じゃ" => vec!["ja", "jya", "zya", "jixya", "jilya", "zixya", "zilya"],
        "じゅ" => vec!["ju", "jyu", "zyu", "jixyu", "jilyu", "zixyu", "zilyu"],
        "じょ" => vec!["jo", "jyo", "zyo", "jixyo", "jilyo", "zixyo", "zilyo"],
        "びゃ" => vec!["bya", "bixya", "bilya"],
        "びゅ" => vec!["byu", "bixyu", "bilyu"],
        "びょ" => vec!["byo", "bixyo", "bilyo"],
        "ぴゃ" => vec!["pya", "pixya", "pilya"],
        "ぴゅ" => vec!["pyu", "pixyu", "pilyu"],
        "ぴょ" => vec!["pyo", "pixyo", "pilyo"],
        "てぃ" => vec!["thi", "texi", "teli"],
        "でぃ" => vec!["dhi", "dexi", "deli"],
        "ふぁ" => vec!["fa", "fuxa", "fula"],
        "ふぃ" => vec!["fi", "fuxi", "fuli"],
        "ふぇ" => vec!["fe", "fuxe", "fule"],
        "ふぉ" => vec!["fo", "fuxo", "fulo"],
        "しぇ" => vec!["she", "sye", "shixe", "shile", "sixe", "sile"],
        "じぇ" => vec!["je", "jye", "zye", "jixe", "jile", "zixe", "zile"],
        "ちぇ" => vec!["che", "tye", "cye", "chixe", "chile", "tixe", "tile"],
        "ー" => vec!["-"],
        "、" => vec![","],
        "。" => vec!["."],
        "「" => vec!["["],
        "」" => vec!["]"],
        "？" => vec!["?"],
        "！" => vec!["!"],
        _ => Vec::new(),
    }
}

#[cfg(test)]
#[path = "romaji_equivalence_tests.rs"]
mod equivalence_tests;
