use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use encoding_rs::EUC_JP;
use flate2::{Compression, GzBuilder};
use sha2::{Digest, Sha256};

const MAGIC: &[u8; 8] = b"SHNFONT\0";
const FORMAT_VERSION: u8 = 1;
/// Upstream archive the source tree is extracted from.
pub const SHINONOME_ARCHIVE: &str = "shinonome-0.9.11p1.tar.bz2";
/// SHA-256 of [`SHINONOME_ARCHIVE`] as published upstream.
pub const SHINONOME_ARCHIVE_SHA256: &str = "95663c95c92ba5765f63ccbdf033eb93b707be01812a989c548db943479c838f";
/// Files read from the extracted tree, relative to its root.
const SHINONOME_INPUT_FILES: [&str; 6] = [
    "bdf/shnmk12.bdf",
    "bdf/shnm6x12r.bdf",
    "bdf/shnmk16.bdf",
    "bdf/shnm8x16r.bdf",
    "LICENSE",
    "AUTHORS",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontKind {
    JisX0201,
    JisX0208,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BdfGlyph {
    pub width: u8,
    pub rows: Vec<u16>,
}

#[derive(Clone, Debug)]
pub struct FontGenerationOptions {
    pub source: PathBuf,
    pub out: PathBuf,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FontGenerationReport {
    pub dot12_glyphs: usize,
    pub dot16_glyphs: usize,
    pub fullwidth_glyphs: usize,
    pub halfwidth_glyphs: usize,
    /// `(path relative to the extracted tree, SHA-256)` of every file read.
    pub input_hashes: Vec<(String, String)>,
}

pub fn jis_x0208_to_char(code: u32) -> Option<char> {
    let high = u8::try_from(code >> 8).ok()?;
    let low = u8::try_from(code & 0xff).ok()?;
    if !(0x21..=0x7e).contains(&high) || !(0x21..=0x7e).contains(&low) {
        return None;
    }
    let bytes = [high | 0x80, low | 0x80];
    let decoded = EUC_JP.decode_without_bom_handling_and_without_replacement(&bytes)?;
    let mut characters = decoded.chars();
    let character = characters.next()?;
    characters.next().is_none().then_some(character)
}

pub fn parse_bdf(source: &[u8], kind: FontKind, expected_width: u8, expected_height: u8) -> Result<BTreeMap<char, BdfGlyph>> {
    let source = std::str::from_utf8(source).context("BDF must be ASCII")?;
    let mut glyphs = BTreeMap::new();
    let mut encoding = None;
    let mut width = None;
    let mut height = None;
    let mut rows = Vec::new();
    let mut in_bitmap = false;
    let mut in_glyph = false;

    for (line_index, raw_line) in source.lines().enumerate() {
        let line = raw_line.trim();
        if line.starts_with("STARTCHAR ") {
            ensure!(!in_glyph, "nested STARTCHAR at line {}", line_index + 1);
            in_glyph = true;
            encoding = None;
            width = None;
            height = None;
            rows.clear();
            in_bitmap = false;
        } else if in_glyph && line == "BITMAP" {
            in_bitmap = true;
        } else if in_glyph && line == "ENDCHAR" {
            in_bitmap = false;
            let glyph_width = width.context("glyph has no BBX width")?;
            let glyph_height = height.context("glyph has no BBX height")?;
            ensure!(
                glyph_width == expected_width && glyph_height == expected_height,
                "glyph dimensions {glyph_width}x{glyph_height} do not match {expected_width}x{expected_height}"
            );
            ensure!(
                rows.len() == usize::from(expected_height),
                "glyph has {} rows, expected {expected_height}",
                rows.len()
            );
            if let Some(character) = encoding.and_then(|value| encoding_to_char(kind, value)) {
                let glyph = BdfGlyph {
                    width: expected_width,
                    rows: rows.clone(),
                };
                ensure!(glyphs.insert(character, glyph).is_none(), "duplicate Unicode mapping for {character:?}");
            }
            in_glyph = false;
        } else if in_glyph && in_bitmap {
            let byte_width = usize::from(expected_width).div_ceil(8);
            ensure!(
                line.len() == byte_width * 2,
                "bitmap row has {} hex digits, expected {}",
                line.len(),
                byte_width * 2
            );
            let value = u16::from_str_radix(line, 16).with_context(|| format!("invalid bitmap row at line {}", line_index + 1))?;
            rows.push(value << (16 - byte_width * 8));
        } else if in_glyph {
            if let Some(value) = line.strip_prefix("ENCODING ") {
                encoding = Some(
                    value
                        .parse::<u32>()
                        .with_context(|| format!("invalid ENCODING at line {}", line_index + 1))?,
                );
            } else if let Some(value) = line.strip_prefix("BBX ") {
                let mut fields = value.split_whitespace();
                width = Some(fields.next().context("BBX has no width")?.parse::<u8>()?);
                height = Some(fields.next().context("BBX has no height")?.parse::<u8>()?);
            }
        }
    }
    ensure!(!in_glyph, "unterminated BDF glyph");
    Ok(glyphs)
}

fn encoding_to_char(kind: FontKind, encoding: u32) -> Option<char> {
    match kind {
        FontKind::JisX0208 => jis_x0208_to_char(encoding),
        FontKind::JisX0201 => match encoding {
            0x20..=0x7e => char::from_u32(encoding),
            0xa1..=0xdf => char::from_u32(0xff61 + encoding - 0xa1),
            _ => None,
        },
    }
}

pub fn generate_fonts(options: &FontGenerationOptions) -> Result<FontGenerationReport> {
    fs::create_dir_all(&options.out).with_context(|| format!("failed to create {}", options.out.display()))?;
    // When the archive sits next to the extracted tree (the layout `fetch.sh`
    // creates), refuse to generate from a tree whose archive does not match.
    if let Some(archive) = options.source.parent().map(|parent| parent.join(SHINONOME_ARCHIVE))
        && archive.is_file()
    {
        let actual = sha256_file(&archive)?;
        ensure!(
            actual == SHINONOME_ARCHIVE_SHA256,
            "{} has SHA-256 {actual}, expected {SHINONOME_ARCHIVE_SHA256}",
            archive.display()
        );
    }
    let input_hashes = SHINONOME_INPUT_FILES
        .iter()
        .map(|name| Ok(((*name).to_owned(), sha256_file(&options.source.join(name))?)))
        .collect::<Result<Vec<_>>>()?;
    let dot12 = load_size(&options.source, "shnmk12.bdf", "shnm6x12r.bdf", 12, 12, 6)?;
    let dot16 = load_size(&options.source, "shnmk16.bdf", "shnm8x16r.bdf", 16, 16, 8)?;
    ensure!(dot12.0 == dot16.0, "12-dot and 16-dot fullwidth character sets differ");
    ensure!(dot12.1 == dot16.1, "12-dot and 16-dot halfwidth character sets differ");

    let data12 = gzip(&encode_font(12, 6, 12, &dot12.2)?)?;
    let data16 = gzip(&encode_font(16, 8, 16, &dot16.2)?)?;
    write_bytes_if_changed(&options.out.join("shinonome12.bin.gz"), &data12)?;
    write_bytes_if_changed(&options.out.join("shinonome16.bin.gz"), &data16)?;

    Ok(FontGenerationReport {
        dot12_glyphs: dot12.2.len(),
        dot16_glyphs: dot16.2.len(),
        fullwidth_glyphs: dot12.0,
        halfwidth_glyphs: dot12.1,
        input_hashes,
    })
}

pub fn update_shinonome_notice(source: &Path, notice_path: &Path) -> Result<()> {
    const SECTION: &str = "Shinonome bitmap fonts\n========================";
    let decode = |name: &str| -> Result<String> {
        let path = source.join(name);
        let bytes = fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
        let decoded = EUC_JP
            .decode_without_bom_handling_and_without_replacement(&bytes)
            .with_context(|| format!("{} is not valid EUC-JP", path.display()))?;
        Ok(decoded.into_owned())
    };
    let license = decode("LICENSE")?;
    let authors = decode("AUTHORS")?;
    let mut notice = fs::read_to_string(notice_path).unwrap_or_default();
    if let Some(index) = notice.find(&format!("\n\n{SECTION}")) {
        notice.truncate(index);
    }
    while notice.ends_with('\n') {
        notice.pop();
    }
    if !notice.is_empty() {
        notice.push_str("\n\n");
    }
    notice.push_str(SECTION);
    notice.push_str(
        "\n\nSource: http://openlab.ring.gr.jp/efont/shinonome/\nArchive: shinonome-0.9.11p1.tar.bz2\nSHA-256: 95663c95c92ba5765f63ccbdf033eb93b707be01812a989c548db943479c838f\n\nLICENSE (converted from EUC-JP to UTF-8)\n-----------------------------------------\n\n",
    );
    notice.push_str(license.trim_end());
    notice.push_str("\n\nAUTHORS (converted from EUC-JP to UTF-8)\n-----------------------------------------\n\n");
    notice.push_str(authors.trim_end());
    notice.push('\n');
    write_bytes_if_changed(notice_path, notice.as_bytes())
}

pub fn update_shinonome_manifest(manifest_path: &Path, font_dir: &Path, report: &FontGenerationReport) -> Result<()> {
    let source = fs::read_to_string(manifest_path).with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let notice_path = manifest_path.parent().unwrap_or_else(|| Path::new(".")).join("NOTICE");
    let notice_hash = notice_path.exists().then(|| sha256_file(&notice_path)).transpose()?;
    let mut blocks = source
        .trim_end()
        .split("\n\n")
        .filter(|block| {
            !block.contains("path = \"shinonome/")
                && !block.contains("path = \"font/shinonome12.bin.gz\"")
                && !block.contains("path = \"font/shinonome16.bin.gz\"")
        })
        .map(|block| {
            if block.contains("path = \"NOTICE\"")
                && let Some(hash) = notice_hash.as_ref()
            {
                return block
                    .lines()
                    .map(|line| {
                        if line.starts_with("sha256 = ") {
                            format!("sha256 = \"{hash}\"")
                        } else {
                            line.to_owned()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            block.to_owned()
        })
        .collect::<Vec<_>>();
    let inputs = std::iter::once(format!(
        "[[inputs]]\npath = \"shinonome/{SHINONOME_ARCHIVE}\"\nsha256 = \"{SHINONOME_ARCHIVE_SHA256}\""
    ))
    .chain(
        report
            .input_hashes
            .iter()
            .map(|(name, hash)| format!("[[inputs]]\npath = \"shinonome/shinonome-0.9.11/{name}\"\nsha256 = \"{hash}\"")),
    )
    .collect::<Vec<_>>();
    let first_output = blocks.iter().position(|block| block.starts_with("[[outputs]]")).unwrap_or(blocks.len());
    blocks.splice(first_output..first_output, inputs);
    for (name, count) in [("shinonome12.bin.gz", report.dot12_glyphs), ("shinonome16.bin.gz", report.dot16_glyphs)] {
        let path = font_dir.join(name);
        blocks.push(format!(
            "[[outputs]]\npath = \"font/{name}\"\ncount = {count}\nsha256 = \"{}\"",
            sha256_file(&path)?
        ));
    }
    let mut output = blocks.join("\n\n");
    output.push('\n');
    write_bytes_if_changed(manifest_path, output.as_bytes())
}

fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn load_size(
    source: &Path,
    full_name: &str,
    half_name: &str,
    height: u8,
    full_width: u8,
    half_width: u8,
) -> Result<(usize, usize, BTreeMap<char, BdfGlyph>)> {
    let bdf = source.join("bdf");
    let full_path = bdf.join(full_name);
    let half_path = bdf.join(half_name);
    let full = parse_bdf(
        &fs::read(&full_path).with_context(|| format!("failed to read {}", full_path.display()))?,
        FontKind::JisX0208,
        full_width,
        height,
    )?;
    let half = parse_bdf(
        &fs::read(&half_path).with_context(|| format!("failed to read {}", half_path.display()))?,
        FontKind::JisX0201,
        half_width,
        height,
    )?;
    let full_count = full.len();
    let half_count = half.len();
    let mut merged = full;
    for (character, glyph) in half {
        ensure!(
            merged.insert(character, glyph).is_none(),
            "duplicate fullwidth/halfwidth mapping for {character:?}"
        );
    }
    Ok((full_count, half_count, merged))
}

fn encode_font(height: u8, half_width: u8, full_width: u8, glyphs: &BTreeMap<char, BdfGlyph>) -> Result<Vec<u8>> {
    let mut output = Vec::with_capacity(20 + glyphs.len() * (5 + usize::from(height) * 2));
    output.extend_from_slice(MAGIC);
    output.extend_from_slice(&[FORMAT_VERSION, height, half_width, full_width]);
    output.extend_from_slice(&(glyphs.len() as u32).to_le_bytes());
    output.extend_from_slice(&[0; 4]);
    for (character, glyph) in glyphs {
        ensure!(glyph.rows.len() == usize::from(height), "glyph {character:?} has the wrong height");
        output.extend_from_slice(&(*character as u32).to_le_bytes());
        output.push(glyph.width);
        for row in &glyph.rows {
            output.extend_from_slice(&row.to_le_bytes());
        }
    }
    Ok(output)
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
