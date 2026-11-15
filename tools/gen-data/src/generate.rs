use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result, ensure};
use bzip2::read::BzDecoder;
use chrono::{DateTime, NaiveDate, Utc};
use encoding_rs::SHIFT_JIS;
use flate2::{Compression, GzBuilder};
use lindera::dictionary::load_dictionary;
use lindera::mode::Mode;
use lindera::segmenter::Segmenter;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::{AozoraDocument, extract_aozora, extract_english_body, katakana_to_hiragana, load_scowl_words, stable_sample};

const EN_WORD_LIMIT: usize = 20_000;
const JA_WORD_LIMIT: usize = 12_000;
const JA_REVIEW_LIMIT: usize = 600;
const JA_SENTENCES_PER_WORK: usize = 15;
const SAMPLE_SEED: u64 = 0x5253_5459_5045_5034;
const LINDERA_VERSION: &str = "6.2.0";
const IPADIC_VERSION: &str = "mecab-ipadic-2.7.0-20250920";
const BLOCKLIST_EN: &str = include_str!("../blocklist_en.txt");
const IPADIC_COPYING: &str = include_str!("../licenses/IPADIC-COPYING.txt");
const REVIEWED_JA_WORDS: &str = include_str!("../reviewed/ja_words.tsv");
const REVIEWED_JA_SENTENCES: &str = include_str!("../reviewed/ja_sentences.tsv");

#[derive(Clone, Debug)]
pub struct GenerationOptions {
    pub input: PathBuf,
    pub extra_aozora: Option<PathBuf>,
    pub out: PathBuf,
    pub date: Option<NaiveDate>,
}

#[derive(Clone, Debug, Default)]
pub struct GenerationReport {
    pub en_words: usize,
    pub ja_words: usize,
    pub en_sentences: usize,
    pub ja_sentences: usize,
    pub ruby_compared: usize,
    pub ruby_mismatches: usize,
    pub ruby_mismatch_examples: Vec<String>,
    pub missing_aozora_ids: Vec<String>,
}

#[derive(Clone, Debug)]
struct Work {
    id: String,
    author: String,
    title: String,
    input_by: String,
    document: AozoraDocument,
}

#[derive(Clone, Debug)]
struct JapaneseWord {
    surface: String,
    reading: String,
    count: u64,
    ruby_result: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct JapaneseSentence {
    text: String,
    reading: String,
    source_id: String,
    author: String,
}

impl AsRef<str> for JapaneseSentence {
    fn as_ref(&self) -> &str {
        &self.text
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct EnglishSentence {
    text: String,
    id: String,
}

impl AsRef<str> for EnglishSentence {
    fn as_ref(&self) -> &str {
        &self.text
    }
}

#[derive(Clone, Copy, Debug)]
struct IpadicFields {
    pos: usize,
    pos1: usize,
    base_form: usize,
    reading: usize,
}

impl IpadicFields {
    fn from_segmenter(segmenter: &Segmenter) -> Result<Self> {
        let schema = &segmenter.dictionary.metadata.dictionary_schema;
        let field = |name: &str| schema.get_custom_field_index(name).with_context(|| format!("IPADIC schema lacks {name}"));
        Ok(Self {
            pos: field("part_of_speech")?,
            pos1: field("part_of_speech_subcategory_1")?,
            base_form: field("base_form")?,
            reading: field("reading")?,
        })
    }
}

pub fn generate(options: &GenerationOptions) -> Result<GenerationReport> {
    validate_inputs(&options.input)?;
    fs::create_dir_all(&options.out).with_context(|| format!("failed to create {}", options.out.display()))?;

    let blocklist = load_blocklist();
    let mut input_files = collect_input_files(&options.input)?;
    if let Some(extra) = &options.extra_aozora {
        input_files.extend(collect_aozora_input_files(extra)?);
        input_files.sort();
        input_files.dedup();
    }
    let generated_date = options.date.unwrap_or(max_modified_date(&input_files)?);
    let scowl_root = options.input.join("scowl/scowl-2020.12.07");
    let scowl = load_scowl_words(&scowl_root)?;
    let en_words = generate_english_words(&options.input, &scowl, &blocklist)?;

    let (works, missing_aozora_ids) = load_aozora_works(&options.input, options.extra_aozora.as_deref())?;
    let dictionary = load_dictionary("embedded://ipadic").context("failed to load embedded IPADIC")?;
    let segmenter = Segmenter::new(Mode::Normal, dictionary, None);
    let fields = IpadicFields::from_segmenter(&segmenter)?;
    let overrides_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("overrides");
    let word_overrides = load_overrides(&overrides_dir.join("ja_readings.tsv"))?;
    let mut ja_words = generate_japanese_words(&works, &segmenter, fields, &word_overrides)?;
    let (ruby_compared, ruby_mismatches, ruby_mismatch_examples) = compare_word_rubies(&mut ja_words, &works);

    let sentence_overrides = load_overrides(&overrides_dir.join("ja_sentences.tsv"))?;
    let ja_sentences = generate_japanese_sentences(&works, &segmenter, fields, &sentence_overrides)?;
    let en_sentences = generate_english_sentences(&options.input, &blocklist)?;

    let en_words_tsv = render_en_words(&en_words);
    let reviewed_words = load_reviewed_pairs(REVIEWED_JA_WORDS)?;
    let reviewed_sentences = load_reviewed_pairs(REVIEWED_JA_SENTENCES)?;
    let ja_words_tsv = render_ja_words(&ja_words, &reviewed_words);
    let en_core = en_words.iter().take(2_000).map(|(word, _)| word.as_str()).collect::<BTreeSet<_>>();
    let en_sentences_tsv = render_en_sentences(&en_sentences, &en_core);
    let ja_sentences_tsv = render_ja_sentences(&ja_sentences, &reviewed_sentences);
    let review_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("review");
    fs::create_dir_all(&review_dir).with_context(|| format!("failed to create {}", review_dir.display()))?;
    let review_words = render_ja_word_review(&ja_words);
    let review_sentences = render_ja_sentence_review(&ja_sentences);
    let notice = render_notice(&options.input, &works)?;

    let outputs = [
        ("en_words.tsv.gz", gzip(en_words_tsv.as_bytes())?, en_words.len()),
        ("ja_words.tsv.gz", gzip(ja_words_tsv.as_bytes())?, ja_words.len()),
        ("en_sentences.tsv.gz", gzip(en_sentences_tsv.as_bytes())?, en_sentences.len()),
        ("ja_sentences.tsv.gz", gzip(ja_sentences_tsv.as_bytes())?, ja_sentences.len()),
        ("NOTICE", notice.into_bytes(), 1),
    ];
    for (name, content, _) in &outputs {
        write_bytes_if_changed(&options.out.join(name), content)?;
    }
    write_if_changed(&review_dir.join("ja_words_top600.tsv"), &review_words)?;
    write_if_changed(&review_dir.join("ja_sentences_candidates.tsv"), &review_sentences)?;

    let manifest = render_manifest(&options.input, options.extra_aozora.as_deref(), generated_date, &input_files, &outputs)?;
    write_if_changed(&options.out.join("manifest.toml"), &manifest)?;

    Ok(GenerationReport {
        en_words: en_words.len(),
        ja_words: ja_words.len(),
        en_sentences: en_sentences.len(),
        ja_sentences: ja_sentences.len(),
        ruby_compared,
        ruby_mismatches,
        ruby_mismatch_examples,
        missing_aozora_ids,
    })
}

fn validate_inputs(input: &Path) -> Result<()> {
    for relative in [
        "aozora/selection.tsv",
        "gutenberg",
        "scowl/scowl-2020.12.07/Copyright",
        "tatoeba/eng_sentences_CC0.tsv.bz2",
    ] {
        ensure!(input.join(relative).exists(), "missing input: {}", input.join(relative).display());
    }
    Ok(())
}

fn load_blocklist() -> BTreeSet<String> {
    BLOCKLIST_EN
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

fn generate_english_words(input: &Path, scowl: &BTreeSet<String>, blocklist: &BTreeSet<String>) -> Result<Vec<(String, u64)>> {
    let books_dir = input.join("gutenberg");
    let mut paths = fs::read_dir(&books_dir)
        .with_context(|| format!("failed to read {}", books_dir.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.retain(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("pg") && name.ends_with(".txt"))
    });
    paths.sort();
    ensure!(paths.len() == 30, "expected 30 English books, found {}", paths.len());

    let mut counts = BTreeMap::<String, u64>::new();
    for path in paths {
        let source = fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
        let body = extract_english_body(&source)?;
        for token in english_tokens(body) {
            if token.contains(['\'', '’']) {
                continue;
            }
            let word = token.to_ascii_lowercase();
            if !word.bytes().all(|byte| byte.is_ascii_lowercase())
                || (word.len() == 1 && word != "a" && word != "i")
                || !scowl.contains(&word)
                || blocklist.contains(&word)
            {
                continue;
            }
            *counts.entry(word).or_default() += 1;
        }
    }
    let tatoeba_path = input.join("tatoeba/eng_sentences_CC0.tsv.bz2");
    let reader = BufReader::new(BzDecoder::new(File::open(&tatoeba_path)?));
    for line in reader.lines() {
        let line = line?;
        let mut fields = line.splitn(4, '\t');
        let _id = fields.next();
        let language = fields.next();
        let text = fields.next();
        if language != Some("eng") {
            continue;
        }
        for token in english_tokens(text.unwrap_or_default()) {
            if token.contains(['\'', '’']) {
                continue;
            }
            let word = token.to_ascii_lowercase();
            if word.bytes().all(|byte| byte.is_ascii_lowercase())
                && (word.len() > 1 || matches!(word.as_str(), "a" | "i"))
                && scowl.contains(&word)
                && !blocklist.contains(&word)
            {
                *counts.entry(word).or_default() += 1;
            }
        }
    }
    for word in scowl {
        if !blocklist.contains(word) {
            counts.entry(word.clone()).or_default();
        }
    }
    let mut ranked = counts.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    ranked.truncate(EN_WORD_LIMIT);
    Ok(ranked)
}

fn english_tokens(text: &str) -> impl Iterator<Item = &str> {
    text.split(|character: char| !(character.is_ascii_alphabetic() || matches!(character, '\'' | '’')))
        .filter(|token| !token.is_empty())
}

fn load_aozora_works(input: &Path, extra: Option<&Path>) -> Result<(Vec<Work>, Vec<String>)> {
    let mut selections = vec![input.join("aozora")];
    if let Some(extra) = extra {
        selections.push(extra.to_path_buf());
    }
    let mut works_by_id = BTreeMap::new();
    let mut missing = BTreeSet::new();
    for root in selections {
        let selection_path = root.join("selection.tsv");
        let selection = fs::read_to_string(&selection_path).with_context(|| format!("failed to read {}", selection_path.display()))?;
        for (line_index, line) in selection.lines().enumerate() {
            let fields = line.split('\t').collect::<Vec<_>>();
            ensure!(fields.len() == 5, "selection.tsv line {} must have five fields", line_index + 1);
            let id = fields[0].to_owned();
            if works_by_id.contains_key(&id) {
                continue;
            }
            let zip_path = root.join("zips").join(format!("{id}.zip"));
            if !zip_path.exists() {
                missing.insert(id);
                continue;
            }
            let source = read_aozora_zip(&zip_path)?;
            works_by_id.insert(
                id.clone(),
                Work {
                    id: id.clone(),
                    author: fields[1].to_owned(),
                    title: fields[2].to_owned(),
                    input_by: fields[3].to_owned(),
                    document: extract_aozora(&source),
                },
            );
            missing.remove(&id);
        }
    }
    let mut works = works_by_id.into_values().collect::<Vec<_>>();
    ensure!(works.len() >= 72, "expected at least 72 Japanese works, found {}", works.len());
    works.sort_by(|left, right| left.id.cmp(&right.id));
    Ok((works, missing.into_iter().collect()))
}

fn read_aozora_zip(path: &Path) -> Result<String> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut archive = ZipArchive::new(file).with_context(|| format!("failed to read {}", path.display()))?;
    let mut text_index = None;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        if !entry.is_dir() && entry.name().to_ascii_lowercase().ends_with(".txt") {
            text_index = Some(index);
            break;
        }
    }
    let index = text_index.with_context(|| format!("{} contains no text file", path.display()))?;
    let mut entry = archive.by_index(index)?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes)?;
    let (decoded, _, had_errors) = SHIFT_JIS.decode(&bytes);
    ensure!(!had_errors, "{} is not valid Shift_JIS", path.display());
    Ok(decoded.into_owned())
}

fn load_overrides(path: &Path) -> Result<BTreeMap<String, Option<String>>> {
    let source = fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut overrides = BTreeMap::new();
    for (index, line) in source.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (surface, reading) = line
            .split_once('\t')
            .with_context(|| format!("{} line {} needs two fields", path.display(), index + 1))?;
        overrides.insert(surface.to_owned(), (reading != "-").then(|| katakana_to_hiragana(reading)));
    }
    Ok(overrides)
}

fn generate_japanese_words(
    works: &[Work],
    segmenter: &Segmenter,
    fields: IpadicFields,
    overrides: &BTreeMap<String, Option<String>>,
) -> Result<Vec<JapaneseWord>> {
    let mut counts = BTreeMap::<(String, String), u64>::new();
    let mut reading_cache = BTreeMap::<String, Option<String>>::new();
    for work in works {
        for line in work.document.body.lines() {
            let mut tokens = segmenter.segment(Cow::Borrowed(line))?;
            for token in &mut tokens {
                let details = token.details();
                let pos = detail(&details, fields.pos);
                let pos1 = detail(&details, fields.pos1);
                if !wanted_part_of_speech(pos, pos1) {
                    continue;
                }
                let base_form = detail(&details, fields.base_form).to_owned();
                let Some(reading) = base_form_reading(&base_form, segmenter, fields, &mut reading_cache)? else {
                    continue;
                };
                if !valid_japanese_word(&base_form, &reading) {
                    continue;
                }
                *counts.entry((base_form, reading)).or_default() += 1;
            }
        }
    }

    let mut overridden = BTreeMap::<(String, String), u64>::new();
    for ((surface, reading), count) in counts {
        match overrides.get(&surface) {
            Some(None) => {}
            Some(Some(replacement)) => *overridden.entry((surface, replacement.clone())).or_default() += count,
            None => *overridden.entry((surface, reading)).or_default() += count,
        }
    }
    let mut words = overridden
        .into_iter()
        .map(|((surface, reading), count)| JapaneseWord {
            surface,
            reading,
            count,
            ruby_result: "not-compared".to_owned(),
        })
        .collect::<Vec<_>>();
    words.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.surface.cmp(&right.surface))
            .then_with(|| left.reading.cmp(&right.reading))
    });
    words.truncate(JA_WORD_LIMIT);
    Ok(words)
}

fn detail<'a>(details: &'a [&str], index: usize) -> &'a str {
    details.get(index).copied().unwrap_or("*")
}

fn wanted_part_of_speech(pos: &str, pos1: &str) -> bool {
    (pos == "名詞" && matches!(pos1, "一般" | "サ変接続" | "形容動詞語幹")) || (matches!(pos, "動詞" | "形容詞") && pos1 == "自立") || pos == "副詞"
}

fn base_form_reading(
    base_form: &str,
    segmenter: &Segmenter,
    fields: IpadicFields,
    cache: &mut BTreeMap<String, Option<String>>,
) -> Result<Option<String>> {
    if let Some(cached) = cache.get(base_form) {
        return Ok(cached.clone());
    }
    let reading = ipadic_reading(base_form, segmenter, fields, false)?;
    cache.insert(base_form.to_owned(), reading.clone());
    Ok(reading)
}

fn valid_japanese_word(surface: &str, reading: &str) -> bool {
    surface != "*"
        && reading != "*"
        && !surface.is_empty()
        && reading.chars().all(is_reading_character)
        && (!reading.contains('ー') || surface.contains('ー'))
        && mora_count(reading) > 1
        && reading.chars().count() < 9
        && !reading.contains(['ゐ', 'ゑ'])
        && !contains_old_style(surface)
}

fn is_reading_character(character: char) -> bool {
    matches!(character, '\u{3041}'..='\u{3096}' | 'ー')
}

fn mora_count(reading: &str) -> usize {
    reading
        .chars()
        .filter(|character| !matches!(character, 'ぁ' | 'ぃ' | 'ぅ' | 'ぇ' | 'ぉ' | 'ゃ' | 'ゅ' | 'ょ' | 'ゎ'))
        .count()
}

fn contains_old_style(value: &str) -> bool {
    const OLD: &str = "亞惡壓圍爲醫壹榮衞驛圓緣艷鹽奧應歐毆穩假價畫會囘壞懷樂陷氣戲據擧峽挾狹曉區驅勳徑惠溪經繼莖螢輕鷄藝缺儉劍圈檢權獻縣險顯驗嚴效廣恆鑛號國黑濟齋劑册蠶慘贊殘絲齒兒辭濕實舍寫釋壽收從澁獸縱肅處敍將燒稱證乘剩壤孃條淨疊讓釀眞盡圖粹醉隨髓數樞瀨聲齊靜竊攝專戰淺潛纖踐錢禪曾雙壯搜插巢爭瘦總聰莊裝騷增藏臟屬續墮體對帶滯臺瀧擇澤單膽團彈斷癡遲晝鑄廳鎭塚遞鐵轉點傳黨盜燈當鬪德獨讀屆貳惱腦霸廢拜賣麥發髮拔蠻祕濱拂佛竝變邊辨瓣辯舖步穗寶峯豐沒萬滿默藥譯豫餘與譽搖樣謠來覽龍兩獵綠鄰凜壘淚類禮勵靈齡戀爐勞樓灣";
    value.chars().any(|character| OLD.contains(character))
}

fn compare_word_rubies(words: &mut [JapaneseWord], works: &[Work]) -> (usize, usize, Vec<String>) {
    let mut rubies = BTreeMap::<String, BTreeSet<String>>::new();
    for work in works {
        for (surface, readings) in &work.document.rubies {
            rubies.entry(surface.clone()).or_default().extend(readings.iter().cloned());
        }
    }
    let mut compared = 0;
    let mut mismatches = 0;
    let mut examples = Vec::new();
    for word in words {
        let Some(ruby_readings) = rubies.get(&word.surface) else {
            word.ruby_result = "no-ruby".to_owned();
            continue;
        };
        compared += 1;
        if ruby_readings.contains(&word.reading) {
            word.ruby_result = "match".to_owned();
        } else {
            mismatches += 1;
            let joined = ruby_readings.iter().cloned().collect::<Vec<_>>().join(",");
            word.ruby_result = format!("mismatch:{joined}");
            if examples.len() < 10 {
                examples.push(format!("{}\t{}\t{joined}", word.surface, word.reading));
            }
        }
    }
    (compared, mismatches, examples)
}

fn generate_japanese_sentences(
    works: &[Work],
    segmenter: &Segmenter,
    fields: IpadicFields,
    overrides: &BTreeMap<String, Option<String>>,
) -> Result<Vec<JapaneseSentence>> {
    let mut all = BTreeMap::<String, JapaneseSentence>::new();
    for work in works {
        let mut candidates = Vec::new();
        for (start, text) in sentence_slices(&work.document.body) {
            let character_count = text.chars().count();
            if !(15..=45).contains(&character_count)
                || !valid_sentence_text(text)
                || !balanced_quotes(text)
                || work.document.has_gaiji_in(start, start + text.len())
            {
                continue;
            }
            let Some(generated) = sentence_reading(&work.document, start, text, segmenter, fields)? else {
                continue;
            };
            let reading = match overrides.get(text) {
                Some(None) => continue,
                Some(Some(reading)) => reading.clone(),
                None => generated,
            };
            if !reading
                .chars()
                .all(|character| is_reading_character(character) || matches!(character, '、' | '。' | '「' | '」'))
                || reading.contains(['ゐ', 'ゑ'])
                || reading.matches('ー').count() != text.matches('ー').count()
            {
                continue;
            }
            candidates.push(JapaneseSentence {
                text: text.to_owned(),
                reading,
                source_id: work.id.clone(),
                author: work.author.clone(),
            });
        }
        candidates.sort();
        candidates.dedup_by(|left, right| left.text == right.text);
        for sentence in stable_sample(&candidates, JA_SENTENCES_PER_WORK, SAMPLE_SEED ^ crate::fnv1a(work.id.as_bytes(), 0)) {
            all.entry(sentence.text.clone()).or_insert(sentence);
        }
    }
    let selected = all.into_values().collect::<Vec<_>>();
    ensure!(selected.len() >= 3_000, "only {} Japanese sentence candidates survived", selected.len());
    Ok(selected)
}

fn sentence_slices(body: &str) -> Vec<(usize, &str)> {
    let mut result = Vec::new();
    let mut start = 0;
    for (index, character) in body.char_indices() {
        if character != '。' {
            continue;
        }
        let end = index + character.len_utf8();
        let raw = &body[start..end];
        let trimmed = raw.trim_matches(|character: char| character.is_whitespace() || character == '　');
        if !trimmed.is_empty() {
            let leading = raw.find(trimmed).unwrap_or(0);
            result.push((start + leading, trimmed));
        }
        start = end;
    }
    result
}

fn valid_sentence_text(text: &str) -> bool {
    text.ends_with('。')
        && text.chars().all(|character| {
            matches!(
                character,
                '\u{3041}'..='\u{3096}'
                    | '\u{30a1}'..='\u{30fa}'
                    | '\u{3400}'..='\u{4dbf}'
                    | '\u{4e00}'..='\u{9fff}'
                    | '\u{f900}'..='\u{faff}'
                    | '々'
                    | '、'
                    | '。'
                    | '「'
                    | '」'
            )
        })
}

fn balanced_quotes(text: &str) -> bool {
    let mut depth = 0_i32;
    for character in text.chars() {
        match character {
            '「' => depth += 1,
            '」' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

fn sentence_reading(
    document: &AozoraDocument,
    sentence_start: usize,
    text: &str,
    segmenter: &Segmenter,
    fields: IpadicFields,
) -> Result<Option<String>> {
    let sentence_end = sentence_start + text.len();
    let mut ruby_spans = document
        .ruby_spans
        .iter()
        .filter(|span| span.start >= sentence_start && span.end <= sentence_end)
        .collect::<Vec<_>>();
    ruby_spans.sort_by_key(|span| span.start);
    let mut reading = String::new();
    let mut cursor = 0;
    for span in ruby_spans {
        let start = span.start - sentence_start;
        let end = span.end - sentence_start;
        if start < cursor || end > text.len() {
            continue;
        }
        let Some(segment_reading) = ipadic_reading(&text[cursor..start], segmenter, fields, true)? else {
            return Ok(None);
        };
        reading.push_str(&segment_reading);
        reading.push_str(&span.reading);
        cursor = end;
    }
    let Some(segment_reading) = ipadic_reading(&text[cursor..], segmenter, fields, true)? else {
        return Ok(None);
    };
    reading.push_str(&segment_reading);
    Ok(Some(reading))
}

fn ipadic_reading(text: &str, segmenter: &Segmenter, fields: IpadicFields, preserve_particles: bool) -> Result<Option<String>> {
    if text.is_empty() {
        return Ok(Some(String::new()));
    }
    let mut reading = String::new();
    let mut tokens = segmenter.segment(Cow::Borrowed(text))?;
    for token in &mut tokens {
        let surface = token.surface.to_string();
        if matches!(surface.as_str(), "、" | "。" | "「" | "」") {
            reading.push_str(&surface);
            continue;
        }
        let details = token.details();
        let pos = detail(&details, fields.pos);
        let token_reading = detail(&details, fields.reading);
        if token_reading == "*" {
            return Ok(None);
        }
        if preserve_particles && pos == "助詞" && matches!(surface.as_str(), "は" | "へ" | "を") {
            reading.push_str(&surface);
        } else {
            reading.push_str(&katakana_to_hiragana(token_reading));
        }
    }
    Ok(Some(reading))
}

fn generate_english_sentences(input: &Path, blocklist: &BTreeSet<String>) -> Result<Vec<EnglishSentence>> {
    let path = input.join("tatoeba/eng_sentences_CC0.tsv.bz2");
    let file = File::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
    let decoder = BzDecoder::new(file);
    let reader = BufReader::new(decoder);
    let mut unique = BTreeMap::<String, EnglishSentence>::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line.with_context(|| format!("failed to decode {} line {}", path.display(), index + 1))?;
        let fields = line.splitn(4, '\t').collect::<Vec<_>>();
        ensure!(fields.len() == 4, "{} line {} must have four fields", path.display(), index + 1);
        let text = fields[2];
        let length = text.chars().count();
        if fields[1] != "eng"
            || !(30..=90).contains(&length)
            || !text.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
            || contains_blocklisted_word(text, blocklist)
        {
            continue;
        }
        unique.entry(text.to_owned()).or_insert_with(|| EnglishSentence {
            text: text.to_owned(),
            id: fields[0].to_owned(),
        });
    }
    let selected = unique.into_values().collect::<Vec<_>>();
    ensure!(selected.len() >= 15_000, "only {} English sentence candidates survived", selected.len());
    Ok(selected)
}

fn contains_blocklisted_word(text: &str, blocklist: &BTreeSet<String>) -> bool {
    english_tokens(text).any(|token| blocklist.contains(&token.to_ascii_lowercase()))
}

fn render_en_words(words: &[(String, u64)]) -> String {
    words
        .iter()
        .enumerate()
        .map(|(index, (word, _))| {
            format!(
                "{}\t{}\t{}\n",
                if word == "i" { "I" } else { word },
                index + 1,
                if index < 2_000 { "core" } else { "ext" }
            )
        })
        .collect()
}

fn render_ja_words(words: &[JapaneseWord], reviewed: &BTreeSet<(String, String)>) -> String {
    words
        .iter()
        .enumerate()
        .map(|(index, word)| {
            let tier = if reviewed.contains(&(word.surface.clone(), word.reading.clone())) {
                "core"
            } else {
                "ext"
            };
            format!("{}\t{}\t{}\t{}\n", word.surface, word.reading, index + 1, tier)
        })
        .collect()
}

fn render_ja_word_review(words: &[JapaneseWord]) -> String {
    words
        .iter()
        .take(JA_REVIEW_LIMIT)
        .enumerate()
        .map(|(index, word)| format!("{}\t{}\t{}\t{}\n", word.surface, word.reading, index + 1, word.ruby_result))
        .collect()
}

fn render_en_sentences(sentences: &[EnglishSentence], core_words: &BTreeSet<&str>) -> String {
    sentences
        .iter()
        .map(|sentence| {
            let is_core = english_tokens(&sentence.text).all(|token| {
                let word = token.to_ascii_lowercase();
                !word.contains(['\'', '’']) && core_words.contains(word.as_str())
            });
            format!("{}\t{}\t{}\n", sentence.text, sentence.id, if is_core { "core" } else { "ext" })
        })
        .collect()
}

fn render_ja_sentences(sentences: &[JapaneseSentence], reviewed: &BTreeSet<(String, String)>) -> String {
    sentences
        .iter()
        .map(|sentence| {
            let tier = if reviewed.contains(&(sentence.text.clone(), sentence.reading.clone())) {
                "core"
            } else {
                "ext"
            };
            format!(
                "{}\t{}\t{}\t{}\t{}\n",
                sentence.text, sentence.reading, sentence.source_id, sentence.author, tier
            )
        })
        .collect()
}

fn render_ja_sentence_review(sentences: &[JapaneseSentence]) -> String {
    sentences
        .iter()
        .map(|sentence| format!("{}\t{}\t{}\n", sentence.text, sentence.reading, sentence.source_id))
        .collect()
}

fn load_reviewed_pairs(source: &str) -> Result<BTreeSet<(String, String)>> {
    let mut pairs = BTreeSet::new();
    for (index, line) in source.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (text, reading) = line
            .split_once('\t')
            .with_context(|| format!("reviewed list line {} needs two fields", index + 1))?;
        pairs.insert((text.to_owned(), reading.to_owned()));
    }
    Ok(pairs)
}

fn render_notice(input: &Path, works: &[Work]) -> Result<String> {
    let scowl = fs::read_to_string(input.join("scowl/scowl-2020.12.07/Copyright"))?;
    let books = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("sources/public_domain_books.tsv"))?;
    let mut notice = String::from("rs-typing-trainer bundled data notices\n\nSCOWL\n=====\n\n");
    notice.push_str(scowl.trim_end());
    notice.push_str("\n\n\nIPADIC\n======\n\n");
    notice.push_str(IPADIC_COPYING.trim_end());
    notice.push_str("\n\n\nJapanese public-domain works\n============================\n\nTitle\tAuthor\tInput by\n");
    for work in works {
        notice.push_str(&format!("{}\t{}\t{}\n", work.title, work.author, work.input_by));
    }
    notice.push_str("\nPublic-domain English books\n===========================\n\nTitle\tAuthor\n");
    for line in books.lines().filter(|line| !line.trim().is_empty() && !line.starts_with('#')) {
        let fields = line.split('\t').collect::<Vec<_>>();
        ensure!(fields.len() == 3, "public_domain_books.tsv needs three fields");
        notice.push_str(&format!("{}\t{}\n", fields[1], fields[2]));
    }
    notice.push_str("\nTatoeba\n=======\n\nEnglish sentences were selected from the CC0 export from tatoeba.org.\n");
    Ok(notice)
}

fn collect_input_files(input: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for directory in ["aozora", "gutenberg", "scowl/scowl-2020.12.07/final", "tatoeba"] {
        collect_files_recursive(&input.join(directory), &mut files)?;
    }
    files.push(input.join("scowl/scowl-2020.12.07/Copyright"));
    files.sort();
    files.dedup();
    files.retain(|path| {
        let relative = path.strip_prefix(input).unwrap_or(path);
        let value = relative.to_string_lossy();
        value == "aozora/selection.tsv"
            || (value.starts_with("aozora/zips/") && value.ends_with(".zip"))
            || (value.starts_with("gutenberg/pg") && value.ends_with(".txt"))
            || value == "scowl/scowl-2020.12.07/Copyright"
            || requested_scowl_path(&value)
            || value == "tatoeba/eng_sentences_CC0.tsv.bz2"
    });
    Ok(files)
}

fn collect_aozora_input_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_files_recursive(root, &mut files)?;
    files.retain(|path| {
        path.as_path() == root.join("selection.tsv")
            || path.parent().is_some_and(|parent| parent == root.join("zips")) && path.extension().is_some_and(|extension| extension == "zip")
    });
    Ok(files)
}

fn collect_files_recursive(path: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_file() {
        files.push(path.to_path_buf());
        return Ok(());
    }
    for entry in fs::read_dir(path).with_context(|| format!("failed to read {}", path.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursive(&path, files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn requested_scowl_path(value: &str) -> bool {
    ["english-words", "american-words"].iter().any(|family| {
        [10, 20, 35, 40, 50, 60]
            .iter()
            .any(|level| value.ends_with(&format!("final/{family}.{level}")))
    })
}

fn max_modified_date(files: &[PathBuf]) -> Result<NaiveDate> {
    let newest = files
        .iter()
        .map(|path| fs::metadata(path).and_then(|metadata| metadata.modified()))
        .collect::<std::io::Result<Vec<SystemTime>>>()?
        .into_iter()
        .max()
        .context("input has no files")?;
    Ok(DateTime::<Utc>::from(newest).date_naive())
}

fn render_manifest(
    input: &Path,
    extra_aozora: Option<&Path>,
    generated_date: NaiveDate,
    input_files: &[PathBuf],
    outputs: &[(&str, Vec<u8>, usize)],
) -> Result<String> {
    let mut manifest = format!(
        "generated_date = \"{generated_date}\"\nlindera_version = \"{LINDERA_VERSION}\"\ndictionary = \"{IPADIC_VERSION}\"\ndictionary_license = \"NAIST and ICOT terms\"\n\n[filters]\nen_words = \"SCOWL levels 10,20,35,40,50,60; ASCII letters; no apostrophes; top 20000 by public-domain books and Tatoeba CC0 frequency; blocklist\"\nja_words = \"selected IPADIC POS; base forms reanalyzed for readings; 2-8 mora; no old kana or old-style glyphs; top 12000 by corpus frequency\"\nen_sentences = \"CC0; 30-90 characters; printable ASCII; unique; blocklist; all matching rows\"\nja_sentences = \"15-45 characters; allowed Japanese characters and punctuation; balanced quotes; no gaiji or unknown readings; at most 15 per work\"\n"
    );
    for path in input_files {
        let relative = if let Ok(relative) = path.strip_prefix(input) {
            let value = relative.to_string_lossy();
            if let Some(number) = value.strip_prefix("gutenberg/pg").and_then(|value| value.strip_suffix(".txt")) {
                format!("pd_books/{number}.txt")
            } else {
                value.into_owned()
            }
        } else if let Some(extra) = extra_aozora {
            format!("aozora/{}", path.strip_prefix(extra).unwrap_or(path).to_string_lossy())
        } else {
            path.file_name().unwrap_or_default().to_string_lossy().into_owned()
        };
        manifest.push_str(&format!(
            "\n[[inputs]]\npath = \"{}\"\nsha256 = \"{}\"\n",
            toml_escape(&relative),
            sha256_file(path)?
        ));
    }
    for (name, content, count) in outputs {
        manifest.push_str(&format!(
            "\n[[outputs]]\npath = \"{}\"\ncount = {}\nsha256 = \"{}\"\n",
            toml_escape(name),
            count,
            sha256_bytes(content)
        ));
    }
    Ok(manifest)
}

fn toml_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn write_if_changed(path: &Path, content: &str) -> Result<()> {
    write_bytes_if_changed(path, content.as_bytes())
}

fn gzip(content: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = GzBuilder::new().mtime(0).write(Vec::new(), Compression::best());
    encoder.write_all(content)?;
    Ok(encoder.finish()?)
}

fn write_bytes_if_changed(path: &Path, content: &[u8]) -> Result<()> {
    if fs::read(path).ok().as_deref() == Some(content) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("{}.tmp", path.extension().and_then(|value| value.to_str()).unwrap_or("data")));
    fs::write(&temporary, content).with_context(|| format!("failed to write {}", temporary.display()))?;
    fs::rename(&temporary, path).with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ipadic_segmenter() -> Segmenter {
        let dictionary = load_dictionary("embedded://ipadic").unwrap();
        Segmenter::new(Mode::Normal, dictionary, None)
    }

    #[test]
    fn sentence_reading_uses_ipadic_reading_not_pronunciation() {
        let segmenter = ipadic_segmenter();
        let fields = IpadicFields::from_segmenter(&segmenter).unwrap();

        for (text, expected) in [
            ("向うへ行こうよ", "むこうへいこうよ"),
            ("尊敬する", "そんけいする"),
            ("東京", "とうきょう"),
            ("銭湯", "せんとう"),
        ] {
            assert_eq!(ipadic_reading(text, &segmenter, fields, true).unwrap(), Some(expected.to_owned()));
        }
    }

    #[test]
    fn base_form_is_reanalyzed_for_its_own_reading() {
        let segmenter = ipadic_segmenter();
        let fields = IpadicFields::from_segmenter(&segmenter).unwrap();
        let mut tokens = segmenter.segment(Cow::Borrowed("行った")).unwrap();
        let verb = tokens.iter_mut().find(|token| token.surface.as_ref() == "行っ").unwrap();
        let base_form = detail(&verb.details(), fields.base_form).to_owned();
        let mut cache = BTreeMap::new();

        assert_eq!(base_form, "行く");
        assert_eq!(
            base_form_reading(&base_form, &segmenter, fields, &mut cache).unwrap(),
            Some("いく".to_owned())
        );
    }

    #[test]
    fn unknown_ipadic_reading_is_rejected() {
        let segmenter = ipadic_segmenter();
        let fields = IpadicFields::from_segmenter(&segmenter).unwrap();

        assert_eq!(ipadic_reading("ゔゔゔ", &segmenter, fields, true).unwrap(), None);
    }
}
