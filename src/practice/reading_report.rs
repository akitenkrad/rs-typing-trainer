use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Utc};

use crate::data::DataCatalog;
use crate::engine::romaji::RomajiJudge;
use crate::error::AppResult;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReadingReportKind {
    Romaji,
    Sentence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadingReportEntry {
    pub latest_at: DateTime<Utc>,
    pub kind: ReadingReportKind,
    pub text: String,
    pub reading: String,
    pub correct_reading: Option<String>,
    pub count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadingCorrectionValidationError {
    Empty,
    Unchanged,
    NonHiragana,
    Untypable(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadingCorrectionEntry {
    pub at: DateTime<Utc>,
    pub kind: ReadingReportKind,
    pub text: String,
    pub wrong_reading: String,
    pub correct_reading: String,
}

#[derive(Clone, Copy)]
struct ParsedReadingReport<'source> {
    at: DateTime<Utc>,
    kind: ReadingReportKind,
    text: &'source str,
    reading: &'source str,
}

#[derive(Clone, Copy)]
struct ParsedReadingCorrection<'source> {
    at: DateTime<Utc>,
    kind: ReadingReportKind,
    text: &'source str,
    wrong_reading: &'source str,
    correct_reading: &'source str,
}

impl ReadingReportKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Romaji => "romaji",
            Self::Sentence => "sentence",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "romaji" => Some(Self::Romaji),
            "sentence" => Some(Self::Sentence),
            _ => None,
        }
    }
}

pub fn append_reading_report(db_directory: &Path, at: DateTime<Utc>, kind: ReadingReportKind, text: &str, reading: &str) -> AppResult<()> {
    std::fs::create_dir_all(db_directory)?;
    let path = db_directory.join("reading_reports.tsv");
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(
        file,
        "{}\t{}\t{}\t{}",
        at.to_rfc3339(),
        kind.as_str(),
        tsv_field(text),
        tsv_field(reading)
    )?;
    Ok(())
}

pub fn save_reading_correction(
    db_directory: &Path,
    at: DateTime<Utc>,
    kind: ReadingReportKind,
    text: &str,
    wrong_reading: &str,
    correct_reading: &str,
) -> AppResult<()> {
    fs::create_dir_all(db_directory)?;
    let path = db_directory.join("reading_corrections.tsv");
    let text = tsv_field(text);
    let wrong_reading = tsv_field(wrong_reading);
    let correct_reading = tsv_field(correct_reading);
    let source = match fs::read(&path) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error.into()),
    };
    let mut retained = retain_nonmatching_corrections(&source, kind, &text, &wrong_reading).0;
    if !retained.is_empty() && !retained.ends_with(b"\n") {
        retained.push(b'\n');
    }
    writeln!(
        retained,
        "{}\t{}\t{}\t{}\t{}",
        at.to_rfc3339(),
        kind.as_str(),
        text,
        wrong_reading,
        correct_reading
    )?;
    replace_report_file(&path, &retained)?;
    Ok(())
}

pub fn load_reading_corrections(db_directory: &Path) -> AppResult<Vec<ReadingCorrectionEntry>> {
    let path = db_directory.join("reading_corrections.tsv");
    let source = match fs::read(&path) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut corrections = HashMap::<(ReadingReportKind, String, String), ReadingCorrectionEntry>::new();
    for line in source.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(parsed) = std::str::from_utf8(line).ok().and_then(parse_reading_correction) else {
            continue;
        };
        let entry = ReadingCorrectionEntry {
            at: parsed.at,
            kind: parsed.kind,
            text: parsed.text.to_owned(),
            wrong_reading: parsed.wrong_reading.to_owned(),
            correct_reading: parsed.correct_reading.to_owned(),
        };
        let key = (entry.kind, entry.text.clone(), entry.wrong_reading.clone());
        if corrections.get(&key).is_none_or(|existing| existing.at <= entry.at) {
            corrections.insert(key, entry);
        }
    }
    let mut corrections = corrections.into_values().collect::<Vec<_>>();
    corrections.sort_by(|left, right| {
        right
            .at
            .cmp(&left.at)
            .then_with(|| left.kind.as_str().cmp(right.kind.as_str()))
            .then_with(|| left.text.cmp(&right.text))
            .then_with(|| left.wrong_reading.cmp(&right.wrong_reading))
    });
    Ok(corrections)
}

pub fn load_reading_exclusions(db_directory: &Path) -> AppResult<HashSet<(ReadingReportKind, String, String)>> {
    let path = db_directory.join("reading_reports.tsv");
    let Some(source) = read_report_source(&path)? else {
        return Ok(HashSet::new());
    };
    Ok(source
        .lines()
        .filter_map(parse_reading_report)
        .map(|report| (report.kind, report.text.to_owned(), report.reading.to_owned()))
        .collect())
}

pub fn apply_reading_reports_to_catalog(catalog: &mut DataCatalog, db_directory: &Path, is_bench: bool) -> AppResult<()> {
    if is_bench {
        return Ok(());
    }
    let mut word_corrections = HashMap::new();
    let mut sentence_corrections = HashMap::new();
    for correction in load_reading_corrections(db_directory)? {
        let target = match correction.kind {
            ReadingReportKind::Romaji => &mut word_corrections,
            ReadingReportKind::Sentence => &mut sentence_corrections,
        };
        target.insert((correction.text, correction.wrong_reading), correction.correct_reading);
    }
    catalog.apply_reading_corrections(&word_corrections, &sentence_corrections);
    let excluded = load_reading_exclusions(db_directory)?
        .into_iter()
        .map(|(_, text, reading)| (text, reading))
        .collect::<HashSet<_>>();
    catalog.exclude_readings(&excluded);
    Ok(())
}

pub fn load_reading_reports(db_directory: &Path) -> AppResult<Vec<ReadingReportEntry>> {
    let path = db_directory.join("reading_reports.tsv");
    let Some(source) = read_report_source(&path)? else {
        return Ok(Vec::new());
    };

    let mut reports = HashMap::<(ReadingReportKind, String, String), (DateTime<Utc>, usize)>::new();
    for report in source.lines().filter_map(parse_reading_report) {
        let key = (report.kind, report.text.to_owned(), report.reading.to_owned());
        let aggregate = reports.entry(key).or_insert((report.at, 0));
        aggregate.0 = aggregate.0.max(report.at);
        aggregate.1 += 1;
    }

    let corrections = load_reading_corrections(db_directory)?
        .into_iter()
        .map(|entry| ((entry.kind, entry.text, entry.wrong_reading), entry.correct_reading))
        .collect::<HashMap<_, _>>();
    let mut reports = reports
        .into_iter()
        .map(|((kind, text, reading), (latest_at, count))| {
            let correct_reading = corrections.get(&(kind, text.clone(), reading.clone())).cloned();
            ReadingReportEntry {
                latest_at,
                kind,
                text,
                reading,
                correct_reading,
                count,
            }
        })
        .collect::<Vec<_>>();
    reports.sort_by(|left, right| {
        right
            .latest_at
            .cmp(&left.latest_at)
            .then_with(|| left.kind.as_str().cmp(right.kind.as_str()))
            .then_with(|| left.text.cmp(&right.text))
            .then_with(|| left.reading.cmp(&right.reading))
    });
    Ok(reports)
}

pub fn validate_reading_correction(original_reading: &str, current_reading: &str, candidate: &str) -> Result<(), ReadingCorrectionValidationError> {
    if candidate.is_empty() {
        return Err(ReadingCorrectionValidationError::Empty);
    }
    if candidate == current_reading {
        return Err(ReadingCorrectionValidationError::Unchanged);
    }
    let original_symbols = original_reading
        .chars()
        .filter(|character| is_reading_symbol(*character))
        .collect::<HashSet<_>>();
    if candidate
        .chars()
        .any(|character| !is_hiragana_or_long_mark(character) && !original_symbols.contains(&character))
    {
        return Err(ReadingCorrectionValidationError::NonHiragana);
    }
    if RomajiJudge::new(candidate).accepted_spellings(1).is_empty() {
        let mut untypable = String::new();
        for character in candidate.chars() {
            if RomajiJudge::new(&character.to_string()).accepted_spellings(1).is_empty() && !untypable.contains(character) {
                untypable.push(character);
            }
        }
        if untypable.is_empty() {
            untypable = candidate.to_owned();
        }
        return Err(ReadingCorrectionValidationError::Untypable(untypable));
    }
    Ok(())
}

fn is_hiragana_or_long_mark(character: char) -> bool {
    ('ぁ'..='ゖ').contains(&character) || character == 'ー'
}

fn is_reading_symbol(character: char) -> bool {
    !is_hiragana_or_long_mark(character) && !character.is_alphanumeric() && !character.is_whitespace() && !character.is_control()
}

pub fn remove_reading_report(db_directory: &Path, kind: ReadingReportKind, text: &str, reading: &str) -> AppResult<usize> {
    let path = db_directory.join("reading_reports.tsv");
    let source = match fs::read(&path) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.into()),
    };

    let mut retained = Vec::with_capacity(source.len());
    let mut removed = 0;
    for line_with_ending in source.split_inclusive(|byte| *byte == b'\n') {
        let line = if let Some(line) = line_with_ending.strip_suffix(b"\n") {
            line.strip_suffix(b"\r").unwrap_or(line)
        } else {
            line_with_ending
        };
        let is_match = std::str::from_utf8(line)
            .ok()
            .and_then(parse_reading_report)
            .is_some_and(|report| report.kind == kind && report.text == text && report.reading == reading);
        if is_match {
            removed += 1;
        } else {
            retained.extend_from_slice(line_with_ending);
        }
    }

    if removed == 0 {
        return Ok(0);
    }

    replace_report_file(&path, &retained)?;
    Ok(removed)
}

pub fn remove_reading_report_and_correction(db_directory: &Path, kind: ReadingReportKind, text: &str, reading: &str) -> AppResult<(usize, usize)> {
    let reports = remove_reading_report(db_directory, kind, text, reading)?;
    let corrections = remove_reading_correction(db_directory, kind, text, reading)?;
    Ok((reports, corrections))
}

fn remove_reading_correction(db_directory: &Path, kind: ReadingReportKind, text: &str, wrong_reading: &str) -> AppResult<usize> {
    let path = db_directory.join("reading_corrections.tsv");
    let source = match fs::read(&path) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.into()),
    };
    let (retained, removed) = retain_nonmatching_corrections(&source, kind, text, wrong_reading);
    if removed > 0 {
        replace_report_file(&path, &retained)?;
    }
    Ok(removed)
}

fn retain_nonmatching_corrections(source: &[u8], kind: ReadingReportKind, text: &str, wrong_reading: &str) -> (Vec<u8>, usize) {
    let mut retained = Vec::with_capacity(source.len());
    let mut removed = 0;
    for line_with_ending in source.split_inclusive(|byte| *byte == b'\n') {
        let line = if let Some(line) = line_with_ending.strip_suffix(b"\n") {
            line.strip_suffix(b"\r").unwrap_or(line)
        } else {
            line_with_ending
        };
        let is_match = std::str::from_utf8(line)
            .ok()
            .and_then(parse_reading_correction)
            .is_some_and(|entry| entry.kind == kind && entry.text == text && entry.wrong_reading == wrong_reading);
        if is_match {
            removed += 1;
        } else {
            retained.extend_from_slice(line_with_ending);
        }
    }
    (retained, removed)
}

fn read_report_source(path: &Path) -> AppResult<Option<String>> {
    match fs::read_to_string(path) {
        Ok(source) => Ok(Some(source)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn parse_reading_report(line: &str) -> Option<ParsedReadingReport<'_>> {
    let mut fields = line.split('\t');
    let at = DateTime::parse_from_rfc3339(fields.next()?).ok()?.with_timezone(&Utc);
    let kind = ReadingReportKind::parse(fields.next()?)?;
    let text = fields.next()?;
    let reading = fields.next()?;
    if fields.next().is_some() {
        return None;
    }
    Some(ParsedReadingReport { at, kind, text, reading })
}

fn parse_reading_correction(line: &str) -> Option<ParsedReadingCorrection<'_>> {
    let mut fields = line.split('\t');
    let at = DateTime::parse_from_rfc3339(fields.next()?).ok()?.with_timezone(&Utc);
    let kind = ReadingReportKind::parse(fields.next()?)?;
    let text = fields.next()?;
    let wrong_reading = fields.next()?;
    let correct_reading = fields.next()?;
    if fields.next().is_some() {
        return None;
    }
    Some(ParsedReadingCorrection {
        at,
        kind,
        text,
        wrong_reading,
        correct_reading,
    })
}

fn replace_report_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    let (temporary_path, mut temporary_file) = create_temporary_file(path)?;
    let replace_result = (|| {
        temporary_file.write_all(contents)?;
        temporary_file.sync_all()?;
        drop(temporary_file);
        fs::rename(&temporary_path, path)
    })();
    if replace_result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    replace_result
}

fn create_temporary_file(path: &Path) -> io::Result<(PathBuf, File)> {
    static NEXT_TEMPORARY_FILE_ID: AtomicU64 = AtomicU64::new(0);

    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("reading_reports.tsv");
    for _ in 0..100 {
        let id = NEXT_TEMPORARY_FILE_ID.fetch_add(1, Ordering::Relaxed);
        let temporary_path = directory.join(format!(".{file_name}.tmp-{}-{id}", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&temporary_path) {
            Ok(file) => return Ok((temporary_path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not create a unique reading report temporary file",
    ))
}

fn tsv_field(value: &str) -> String {
    value.replace(['\t', '\r', '\n'], " ")
}
