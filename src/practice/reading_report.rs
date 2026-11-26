use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Utc};

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
    pub count: usize,
}

#[derive(Clone, Copy)]
struct ParsedReadingReport<'source> {
    at: DateTime<Utc>,
    kind: ReadingReportKind,
    text: &'source str,
    reading: &'source str,
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

    let mut reports = reports
        .into_iter()
        .map(|((kind, text, reading), (latest_at, count))| ReadingReportEntry {
            latest_at,
            kind,
            text,
            reading,
            count,
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
