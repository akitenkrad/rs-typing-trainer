use std::path::PathBuf;

use anyhow::Result;
use chrono::NaiveDate;
use clap::Parser;
use gen_data::{
    BizUdGenerationOptions, FontGenerationOptions, GenerationOptions, generate, generate_bizud_font, generate_fonts, update_bizud_manifest,
    update_bizud_notice, update_shinonome_manifest, update_shinonome_notice,
};

#[derive(Debug, Parser)]
#[command(about = "Generate deterministic bundled prompt data")]
struct Cli {
    #[arg(long)]
    input: Option<PathBuf>,
    /// Shinonome 0.9.11 source tree. When set, only bundled fonts are generated.
    #[arg(long, conflicts_with_all = ["input", "extra_aozora", "date"])]
    font_source: Option<PathBuf>,
    /// Extracted morisawa-biz-ud-gothic 1.051 source tree.
    #[arg(long, conflicts_with_all = ["input", "extra_aozora", "date"])]
    bizud_source: Option<PathBuf>,
    /// Existing bundled-data NOTICE to update with the Shinonome license and authors.
    #[arg(long)]
    font_notice: Option<PathBuf>,
    /// Existing bundled-data manifest to update with font inputs and outputs.
    #[arg(long)]
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
    if cli.font_source.is_some() || cli.bizud_source.is_some() {
        let shinonome = if let Some(source) = cli.font_source.as_ref() {
            let report = generate_fonts(&FontGenerationOptions {
                source: source.clone(),
                out: cli.out.clone(),
            })?;
            println!("12-dot glyphs: {}", report.dot12_glyphs);
            println!("16-dot glyphs: {}", report.dot16_glyphs);
            println!("Fullwidth glyphs: {}", report.fullwidth_glyphs);
            println!("Halfwidth glyphs: {}", report.halfwidth_glyphs);
            Some((source, report))
        } else {
            None
        };
        let bizud = if let Some(source) = cli.bizud_source.as_ref() {
            let report = generate_bizud_font(&BizUdGenerationOptions {
                source: source.clone(),
                out: cli.out.clone(),
            })?;
            println!("BIZ UD requested glyphs: {}", report.requested_glyphs);
            println!("BIZ UD glyphs: {}", report.glyphs);
            println!("BIZ UD missing glyphs: {}", report.missing_glyphs);
            println!("BIZ UD outline bytes: {}", report.uncompressed_bytes);
            println!("BIZ UD gzip bytes: {}", report.compressed_bytes);
            Some((source, report))
        } else {
            None
        };
        if let Some(notice) = cli.font_notice.as_ref() {
            if let Some((source, _)) = &shinonome {
                update_shinonome_notice(source, notice)?;
            }
            if let Some((source, _)) = &bizud {
                update_bizud_notice(source, notice)?;
            }
        }
        if let Some(manifest) = cli.font_manifest.as_ref() {
            if let Some((_, report)) = &shinonome {
                update_shinonome_manifest(manifest, &cli.out, report)?;
            }
            if let Some((_, report)) = &bizud {
                update_bizud_manifest(manifest, &cli.out, report)?;
            }
        }
        return Ok(());
    }
    if cli.font_notice.is_some() || cli.font_manifest.is_some() {
        anyhow::bail!("--font-notice and --font-manifest require --font-source or --bizud-source");
    }
    let input = cli
        .input
        .ok_or_else(|| anyhow::anyhow!("--input, --font-source, or --bizud-source is required"))?;
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

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::Parser;

    #[test]
    fn font_sources_can_be_combined_with_notice_and_manifest() {
        let cli = Cli::try_parse_from([
            "gen-data",
            "--font-source",
            "/tmp/shinonome",
            "--bizud-source",
            "/tmp/bizud",
            "--out",
            "/tmp/out",
            "--font-notice",
            "/tmp/NOTICE",
            "--font-manifest",
            "/tmp/manifest.toml",
        ])
        .unwrap();

        assert!(cli.font_source.is_some());
        assert!(cli.bizud_source.is_some());
        assert!(cli.font_notice.is_some());
        assert!(cli.font_manifest.is_some());
    }
}
