use std::path::PathBuf;

use anyhow::Result;
use chrono::NaiveDate;
use clap::Parser;
use gen_data::{FontGenerationOptions, GenerationOptions, generate, generate_fonts, update_shinonome_manifest, update_shinonome_notice};

#[derive(Debug, Parser)]
#[command(about = "Generate deterministic bundled prompt data")]
struct Cli {
    #[arg(long)]
    input: Option<PathBuf>,
    /// Shinonome 0.9.11 source tree. When set, only bundled fonts are generated.
    #[arg(long, conflicts_with_all = ["input", "extra_aozora", "date"])]
    font_source: Option<PathBuf>,
    /// Existing bundled-data NOTICE to update with the Shinonome license and authors.
    #[arg(long, requires = "font_source")]
    font_notice: Option<PathBuf>,
    /// Existing bundled-data manifest to update with font inputs and outputs.
    #[arg(long, requires = "font_source")]
    font_manifest: Option<PathBuf>,
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
    if let Some(source) = cli.font_source {
        let report = generate_fonts(&FontGenerationOptions {
            source: source.clone(),
            out: cli.out.clone(),
        })?;
        println!("12-dot glyphs: {}", report.dot12_glyphs);
        println!("16-dot glyphs: {}", report.dot16_glyphs);
        println!("Fullwidth glyphs: {}", report.fullwidth_glyphs);
        println!("Halfwidth glyphs: {}", report.halfwidth_glyphs);
        if let Some(notice) = cli.font_notice {
            update_shinonome_notice(&source, &notice)?;
        }
        if let Some(manifest) = cli.font_manifest {
            update_shinonome_manifest(&manifest, &cli.out, &report)?;
        }
        return Ok(());
    }
    let input = cli.input.ok_or_else(|| anyhow::anyhow!("--input or --font-source is required"))?;
    let report = generate(&GenerationOptions {
        input,
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
