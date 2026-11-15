//! Bundled and user-provided prompt data.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use flate2::read::GzDecoder;

use crate::config::DataConfig;
use crate::error::{AppError, AppResult};

const WORDS_EN_GZ: &[u8] = include_bytes!("../../data/en_words.tsv.gz");
const WORDS_JA_GZ: &[u8] = include_bytes!("../../data/ja_words.tsv.gz");
const SENTENCES_EN_GZ: &[u8] = include_bytes!("../../data/en_sentences.tsv.gz");
const SENTENCES_JA_GZ: &[u8] = include_bytes!("../../data/ja_sentences.tsv.gz");

static BUNDLED: OnceLock<Result<BundledText, String>> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DataKind {
    EnglishWord,
    JapaneseWord,
    EnglishSentence,
    JapaneseSentence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Core,
    Ext,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataItem {
    pub kind: DataKind,
    pub key: String,
    pub text: String,
    pub reading: Option<String>,
    pub rank: Option<usize>,
    pub tier: Tier,
    pub source_id: Option<String>,
    pub author: Option<String>,
}

#[derive(Clone, Debug)]
struct BundledText {
    words_en: String,
    words_ja: String,
    sentences_en: String,
    sentences_ja: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JapaneseText {
    pub text: String,
    pub reading: String,
}

pub trait TrainingDataSource {
    fn words_en(&self) -> &[String];
    fn words_ja(&self) -> &[JapaneseText];
    fn sentences_en(&self) -> &[String];
    fn sentences_ja(&self) -> &[JapaneseText];
}

#[derive(Clone, Debug)]
pub struct DataCatalog {
    words_en: Vec<String>,
    words_ja: Vec<JapaneseText>,
    sentences_en: Vec<String>,
    sentences_ja: Vec<JapaneseText>,
    fixed: Vec<DataItem>,
}

impl DataCatalog {
    pub fn load(config: &DataConfig, home: &Path) -> AppResult<Self> {
        let bundled = bundled_text()?;
        let words_en_source = read_or_bundled(config.words_en.as_deref(), &bundled.words_en, home)?;
        let words_ja_source = read_or_bundled(config.words_ja.as_deref(), &bundled.words_ja, home)?;
        let sentences_en_source = read_or_bundled(config.sentences_en.as_deref(), &bundled.sentences_en, home)?;
        let sentences_ja_source = read_or_bundled(config.sentences_ja.as_deref(), &bundled.sentences_ja, home)?;
        let mut fixed = Vec::new();
        fixed.extend(parse_english_items(&words_en_source, DataKind::EnglishWord, config.words_en.is_none())?);
        fixed.extend(parse_japanese_items(&words_ja_source, DataKind::JapaneseWord, config.words_ja.is_none())?);
        fixed.extend(parse_english_items(
            &sentences_en_source,
            DataKind::EnglishSentence,
            config.sentences_en.is_none(),
        )?);
        fixed.extend(parse_japanese_items(
            &sentences_ja_source,
            DataKind::JapaneseSentence,
            config.sentences_ja.is_none(),
        )?);
        Ok(Self {
            words_en: load_one_column_source(&words_en_source, config.words_en.is_none())?,
            words_ja: load_japanese_source(&words_ja_source)?,
            sentences_en: load_one_column_source(&sentences_en_source, config.sentences_en.is_none())?,
            sentences_ja: load_japanese_source(&sentences_ja_source)?,
            fixed,
        })
    }

    /// Immutable complete data used by benchmarks and daily-deck construction.
    pub fn fixed_items(&self, kind: DataKind) -> Vec<&DataItem> {
        self.fixed.iter().filter(|item| item.kind == kind).collect()
    }

    pub fn all_fixed_items(&self) -> &[DataItem] {
        &self.fixed
    }

    /// Keep only the selected daily-deck keys in one category while leaving
    /// the other fixed categories intact.
    pub fn retain_kind_keys(&mut self, kind: DataKind, keys: &HashSet<String>) {
        self.fixed.retain(|item| item.kind != kind || keys.contains(&item.key));
        match kind {
            DataKind::EnglishWord => self.words_en.retain(|text| keys.contains(&format!("en-word:{text}"))),
            DataKind::EnglishSentence => self.sentences_en.retain(|text| keys.contains(&format!("en-sentence:{text}"))),
            DataKind::JapaneseWord => self
                .words_ja
                .retain(|row| keys.contains(&format!("ja-word:{}\t{}", row.text, row.reading))),
            DataKind::JapaneseSentence => self
                .sentences_ja
                .retain(|row| keys.contains(&format!("ja-sentence:{}\t{}", row.text, row.reading))),
        }
    }

    pub fn exclude_readings(&mut self, excluded: &HashSet<(String, String)>) {
        self.words_ja.retain(|row| !excluded.contains(&(row.text.clone(), row.reading.clone())));
        self.sentences_ja
            .retain(|row| !excluded.contains(&(row.text.clone(), row.reading.clone())));
        self.fixed.retain(|item| {
            item.reading
                .as_ref()
                .is_none_or(|reading| !excluded.contains(&(item.text.clone(), reading.clone())))
        });
    }
}

impl TrainingDataSource for DataCatalog {
    fn words_en(&self) -> &[String] {
        &self.words_en
    }

    fn words_ja(&self) -> &[JapaneseText] {
        &self.words_ja
    }

    fn sentences_en(&self) -> &[String] {
        &self.sentences_en
    }

    fn sentences_ja(&self) -> &[JapaneseText] {
        &self.sentences_ja
    }
}

fn load_one_column_source(source: &str, bundled_source: bool) -> AppResult<Vec<String>> {
    let rows = source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            if bundled_source {
                line.split('\t').next().unwrap_or(line).to_owned()
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return Err(AppError::Record("prompt data contains no usable rows".to_owned()));
    }
    Ok(rows)
}

fn load_japanese_source(source: &str) -> AppResult<Vec<JapaneseText>> {
    let mut rows = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split('\t');
        let Some(text) = fields.next() else {
            continue;
        };
        let Some(reading) = fields.next() else {
            return Err(AppError::Record(format!("Japanese TSV line {} must contain text and reading", index + 1)));
        };
        if text.is_empty() || reading.is_empty() {
            return Err(AppError::Record(format!("Japanese TSV line {} has an empty field", index + 1)));
        }
        rows.push(JapaneseText {
            text: text.to_owned(),
            reading: reading.to_owned(),
        });
    }
    if rows.is_empty() {
        return Err(AppError::Record("prompt data contains no usable rows".to_owned()));
    }
    Ok(rows)
}

fn parse_english_items(source: &str, kind: DataKind, bundled: bool) -> AppResult<Vec<DataItem>> {
    let mut items = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields = line.split('\t').collect::<Vec<_>>();
        let text = fields[0];
        let (rank, tier) = if bundled {
            let tier_index = match kind {
                DataKind::EnglishWord => 2,
                DataKind::EnglishSentence => 2,
                _ => unreachable!(),
            };
            if fields.len() <= tier_index {
                return Err(AppError::Record(format!("bundled TSV line {} has too few fields", index + 1)));
            }
            let rank = (kind == DataKind::EnglishWord)
                .then(|| fields[1].parse::<usize>())
                .transpose()
                .map_err(|_| AppError::Record(format!("bundled TSV line {} has an invalid rank", index + 1)))?;
            (rank, parse_tier(fields[tier_index], index + 1)?)
        } else {
            (None, Tier::Ext)
        };
        items.push(DataItem {
            kind,
            key: format!("{}:{text}", kind_key(kind)),
            text: text.to_owned(),
            reading: None,
            rank,
            tier,
            source_id: None,
            author: None,
        });
    }
    Ok(items)
}

fn parse_japanese_items(source: &str, kind: DataKind, bundled: bool) -> AppResult<Vec<DataItem>> {
    let mut items = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() < 2 {
            return Err(AppError::Record(format!("Japanese TSV line {} must contain text and reading", index + 1)));
        }
        let (rank, source_id, author, tier) = if bundled {
            match kind {
                DataKind::JapaneseWord => {
                    if fields.len() != 4 {
                        return Err(AppError::Record(format!("bundled Japanese word line {} needs four fields", index + 1)));
                    }
                    let rank = fields[2]
                        .parse::<usize>()
                        .map_err(|_| AppError::Record(format!("bundled Japanese word line {} has an invalid rank", index + 1)))?;
                    (Some(rank), None, None, parse_tier(fields[3], index + 1)?)
                }
                DataKind::JapaneseSentence => {
                    if fields.len() != 5 {
                        return Err(AppError::Record(format!(
                            "bundled Japanese sentence line {} needs five fields",
                            index + 1
                        )));
                    }
                    (
                        None,
                        Some(fields[2].to_owned()),
                        Some(fields[3].to_owned()),
                        parse_tier(fields[4], index + 1)?,
                    )
                }
                _ => unreachable!(),
            }
        } else {
            (None, None, None, Tier::Ext)
        };
        items.push(DataItem {
            kind,
            key: format!("{}:{}\t{}", kind_key(kind), fields[0], fields[1]),
            text: fields[0].to_owned(),
            reading: Some(fields[1].to_owned()),
            rank,
            tier,
            source_id,
            author,
        });
    }
    Ok(items)
}

fn parse_tier(value: &str, line: usize) -> AppResult<Tier> {
    match value {
        "core" => Ok(Tier::Core),
        "ext" => Ok(Tier::Ext),
        _ => Err(AppError::Record(format!("bundled TSV line {line} has invalid tier {value}"))),
    }
}

fn kind_key(kind: DataKind) -> &'static str {
    match kind {
        DataKind::EnglishWord => "en-word",
        DataKind::JapaneseWord => "ja-word",
        DataKind::EnglishSentence => "en-sentence",
        DataKind::JapaneseSentence => "ja-sentence",
    }
}

fn bundled_text() -> AppResult<&'static BundledText> {
    BUNDLED
        .get_or_init(|| {
            Ok(BundledText {
                words_en: decompress(WORDS_EN_GZ)?,
                words_ja: decompress(WORDS_JA_GZ)?,
                sentences_en: decompress(SENTENCES_EN_GZ)?,
                sentences_ja: decompress(SENTENCES_JA_GZ)?,
            })
        })
        .as_ref()
        .map_err(|error| AppError::Record(error.clone()))
}

fn decompress(bytes: &[u8]) -> Result<String, String> {
    let mut text = String::new();
    GzDecoder::new(bytes)
        .read_to_string(&mut text)
        .map_err(|error| format!("failed to decompress bundled prompt data: {error}"))?;
    Ok(text)
}

fn read_or_bundled(path: Option<&Path>, bundled: &str, home: &Path) -> AppResult<String> {
    match path {
        Some(path) => Ok(std::fs::read_to_string(expand_tilde(path, home))?),
        None => Ok(bundled.to_owned()),
    }
}

fn expand_tilde(path: &Path, home: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    if raw == "~" {
        return home.to_path_buf();
    }
    if let Some(suffix) = raw.strip_prefix("~/") {
        return home.join(suffix);
    }
    path.to_path_buf()
}
