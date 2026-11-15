use std::path::PathBuf;

use anyhow::Result;
use chrono::NaiveDate;
use clap::Parser;
use gen_data::{GenerationOptions, generate};

#[derive(Debug, Parser)]
#[command(about = "Generate deterministic bundled prompt data")]
struct Cli {
    #[arg(long)]
    input: PathBuf,
    /// Additional `aozora/` directory containing selection.tsv and zips/.
    #[arg(long)]
    extra_aozora: Option<PathBuf>,
    #[arg(long)]
    out: PathBuf,
    #[arg(long, value_parser = parse_date)]
    date: Option<NaiveDate>,
}

fn parse_date(value: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|error| error.to_string())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let report = generate(&GenerationOptions {
        input: cli.input,
        extra_aozora: cli.extra_aozora,
        out: cli.out,
        date: cli.date,
    })?;
    println!("English words: {}", report.en_words);
    println!("Japanese words: {}", report.ja_words);
    println!("English sentences: {}", report.en_sentences);
    println!("Japanese sentences: {}", report.ja_sentences);
    println!("Ruby comparisons: {}, mismatches: {}", report.ruby_compared, report.ruby_mismatches);
    for example in report.ruby_mismatch_examples {
        println!("Ruby mismatch: {example}");
    }
    for id in report.missing_aozora_ids {
        println!("Missing Aozora work: {id}");
    }
    Ok(())
}
