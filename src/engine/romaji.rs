//! Incremental kana-to-romaji judge.

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
