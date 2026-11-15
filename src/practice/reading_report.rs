use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use chrono::{DateTime, Utc};

use crate::error::AppResult;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReadingReportKind {
    Romaji,
    Sentence,
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
    if !path.exists() {
        return Ok(HashSet::new());
    }
    let source = std::fs::read_to_string(path)?;
    Ok(source
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let _at = fields.next()?;
            let kind = ReadingReportKind::parse(fields.next()?)?;
            let text = fields.next()?.to_owned();
            let reading = fields.next()?.to_owned();
            Some((kind, text, reading))
        })
        .collect())
}

fn tsv_field(value: &str) -> String {
    value.replace(['\t', '\r', '\n'], " ")
}
