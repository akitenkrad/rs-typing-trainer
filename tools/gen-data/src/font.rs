use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use encoding_rs::EUC_JP;
use flate2::{Compression, GzBuilder};
use sha2::{Digest, Sha256};

const MAGIC: &[u8; 8] = b"SHNFONT\0";
const FORMAT_VERSION: u8 = 1;
const OUTLINE_MAGIC: &[u8; 8] = b"BIZOUTL\0";
const OUTLINE_FORMAT_VERSION: u16 = 1;
/// Upstream archive the source tree is extracted from.
pub const SHINONOME_ARCHIVE: &str = "shinonome-0.9.11p1.tar.bz2";
/// SHA-256 of [`SHINONOME_ARCHIVE`] as published upstream.
pub const SHINONOME_ARCHIVE_SHA256: &str = "95663c95c92ba5765f63ccbdf033eb93b707be01812a989c548db943479c838f";
pub const BIZUD_ARCHIVE: &str = "v1.051.zip";
pub const BIZUD_ARCHIVE_SHA256: &str = "73b7fc086ed39d00219437b68593e86aa535c8590d9975936f7f45e8b3f94bde";
const BIZUD_TTF: &str = "fonts/ttf/BIZUDGothic-Regular.ttf";
const BIZUD_LICENSE: &str = "OFL.txt";
const BIZUD_OUTPUT: &str = "bizud-gothic.outl.gz";
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutlineCommand {
    MoveTo(i32, i32),
    LineTo(i32, i32),
    QuadTo(i32, i32, i32, i32),
    CurveTo(i32, i32, i32, i32, i32, i32),
    Close,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutlineGlyph {
    pub advance: u16,
    pub commands: Vec<OutlineCommand>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutlineFont {
    pub units_per_em: u16,
    pub ascender: i16,
    pub descender: i16,
    pub glyphs: BTreeMap<char, OutlineGlyph>,
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

#[derive(Clone, Debug)]
pub struct BizUdGenerationOptions {
    pub source: PathBuf,
    pub out: PathBuf,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BizUdGenerationReport {
    pub requested_glyphs: usize,
    pub glyphs: usize,
    pub missing_glyphs: usize,
    pub uncompressed_bytes: usize,
    pub compressed_bytes: usize,
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

pub fn generate_bizud_font(options: &BizUdGenerationOptions) -> Result<BizUdGenerationReport> {
    fs::create_dir_all(&options.out).with_context(|| format!("failed to create {}", options.out.display()))?;
    if let Some(archive) = options.source.parent().map(|parent| parent.join(BIZUD_ARCHIVE))
        && archive.is_file()
    {
        let actual = sha256_file(&archive)?;
        ensure!(
            actual == BIZUD_ARCHIVE_SHA256,
            "{} has SHA-256 {actual}, expected {BIZUD_ARCHIVE_SHA256}",
            archive.display()
        );
    }
    let input_hashes = [BIZUD_TTF, BIZUD_LICENSE]
        .into_iter()
        .map(|name| Ok((name.to_owned(), sha256_file(&options.source.join(name))?)))
        .collect::<Result<Vec<_>>>()?;
    let ttf_path = options.source.join(BIZUD_TTF);
    let data = fs::read(&ttf_path).with_context(|| format!("failed to read {}", ttf_path.display()))?;
    let face = ttf_parser::Face::parse(&data, 0).with_context(|| format!("failed to parse {}", ttf_path.display()))?;
    let requested = requested_bizud_characters();
    let mut glyphs = BTreeMap::new();
    for character in &requested {
        let Some(glyph_id) = face.glyph_index(*character) else {
            continue;
        };
        let Some(advance) = face.glyph_hor_advance(glyph_id) else {
            continue;
        };
        let mut collector = OutlineCollector::default();
        face.outline_glyph(glyph_id, &mut collector);
        glyphs.insert(
            *character,
            OutlineGlyph {
                advance,
                commands: collector.commands,
            },
        );
    }
    let font = OutlineFont {
        units_per_em: face.units_per_em(),
        ascender: face.ascender(),
        descender: face.descender(),
        glyphs,
    };
    let encoded = encode_outline_font(&font)?;
    let compressed = gzip_deterministic(&encoded)?;
    write_bytes_if_changed(&options.out.join(BIZUD_OUTPUT), &compressed)?;
    Ok(BizUdGenerationReport {
        requested_glyphs: requested.len(),
        glyphs: font.glyphs.len(),
        missing_glyphs: requested.len() - font.glyphs.len(),
        uncompressed_bytes: encoded.len(),
        compressed_bytes: compressed.len(),
        input_hashes,
    })
}

fn requested_bizud_characters() -> BTreeSet<char> {
    let mut characters = (0x20..=0x7e).filter_map(char::from_u32).collect::<BTreeSet<_>>();
    for high in 0x21..=0x7e {
        for low in 0x21..=0x7e {
            if let Some(character) = jis_x0208_to_char((high << 8) | low) {
                characters.insert(character);
            }
        }
    }
    characters
}

#[derive(Default)]
struct OutlineCollector {
    commands: Vec<OutlineCommand>,
}

impl ttf_parser::OutlineBuilder for OutlineCollector {
    fn move_to(&mut self, x: f32, y: f32) {
        self.commands.push(OutlineCommand::MoveTo(x.round() as i32, y.round() as i32));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.commands.push(OutlineCommand::LineTo(x.round() as i32, y.round() as i32));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.commands.push(OutlineCommand::QuadTo(
            x1.round() as i32,
            y1.round() as i32,
            x.round() as i32,
            y.round() as i32,
        ));
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.commands.push(OutlineCommand::CurveTo(
            x1.round() as i32,
            y1.round() as i32,
            x2.round() as i32,
            y2.round() as i32,
            x.round() as i32,
            y.round() as i32,
        ));
    }

    fn close(&mut self) {
        self.commands.push(OutlineCommand::Close);
    }
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

pub fn update_bizud_notice(source: &Path, notice_path: &Path) -> Result<()> {
    const SECTION: &str = "BIZ UD Gothic\n===============";
    let license_path = source.join(BIZUD_LICENSE);
    let license = fs::read_to_string(&license_path).with_context(|| format!("failed to read {}", license_path.display()))?;
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
        "\n\nSource: https://github.com/googlefonts/morisawa-biz-ud-gothic\nVersion: 1.051\nArchive: v1.051.zip\nSHA-256: 73b7fc086ed39d00219437b68593e86aa535c8590d9975936f7f45e8b3f94bde\nFont: BIZUDGothic-Regular.ttf\nCopyright: Copyright 2022 The BIZ UDGothic Project Authors (https://github.com/googlefonts/morisawa-biz-ud-mincho)\n\nOFL.txt\n-------\n\n",
    );
    notice.push_str(&license.lines().map(str::trim_end).collect::<Vec<_>>().join("\n"));
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

pub fn update_bizud_manifest(manifest_path: &Path, font_dir: &Path, report: &BizUdGenerationReport) -> Result<()> {
    let source = fs::read_to_string(manifest_path).with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let notice_path = manifest_path.parent().unwrap_or_else(|| Path::new(".")).join("NOTICE");
    let notice_hash = notice_path.exists().then(|| sha256_file(&notice_path)).transpose()?;
    let mut blocks = source
        .trim_end()
        .split("\n\n")
        .filter(|block| !block.contains("path = \"bizud/") && !block.contains("path = \"font/bizud-gothic.outl.gz\""))
        .map(|block| update_notice_hash(block, notice_hash.as_deref()))
        .collect::<Vec<_>>();
    let inputs = std::iter::once(format!(
        "[[inputs]]\npath = \"bizud/{BIZUD_ARCHIVE}\"\nsha256 = \"{BIZUD_ARCHIVE_SHA256}\""
    ))
    .chain(
        report
            .input_hashes
            .iter()
            .map(|(name, hash)| format!("[[inputs]]\npath = \"bizud/morisawa-biz-ud-gothic-1.051/{name}\"\nsha256 = \"{hash}\"")),
    )
    .collect::<Vec<_>>();
    let first_output = blocks.iter().position(|block| block.starts_with("[[outputs]]")).unwrap_or(blocks.len());
    blocks.splice(first_output..first_output, inputs);
    blocks.push(format!(
        "[[outputs]]\npath = \"font/{BIZUD_OUTPUT}\"\ncount = {}\nsha256 = \"{}\"",
        report.glyphs,
        sha256_file(&font_dir.join(BIZUD_OUTPUT))?
    ));
    let mut output = blocks.join("\n\n");
    output.push('\n');
    write_bytes_if_changed(manifest_path, output.as_bytes())
}

fn update_notice_hash(block: &str, notice_hash: Option<&str>) -> String {
    if block.contains("path = \"NOTICE\"")
        && let Some(hash) = notice_hash
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

pub fn encode_outline_font(font: &OutlineFont) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    output.extend_from_slice(OUTLINE_MAGIC);
    output.extend_from_slice(&OUTLINE_FORMAT_VERSION.to_le_bytes());
    output.extend_from_slice(&font.units_per_em.to_le_bytes());
    output.extend_from_slice(&font.ascender.to_le_bytes());
    output.extend_from_slice(&font.descender.to_le_bytes());
    output.extend_from_slice(&(u32::try_from(font.glyphs.len()).context("too many outline glyphs")?).to_le_bytes());
    for (character, glyph) in &font.glyphs {
        let mut commands = Vec::new();
        let mut previous = (0i32, 0i32);
        for command in &glyph.commands {
            match *command {
                OutlineCommand::MoveTo(x, y) => {
                    commands.push(0);
                    encode_point(&mut commands, &mut previous, x, y);
                }
                OutlineCommand::LineTo(x, y) => {
                    commands.push(1);
                    encode_point(&mut commands, &mut previous, x, y);
                }
                OutlineCommand::QuadTo(x1, y1, x, y) => {
                    commands.push(2);
                    encode_point(&mut commands, &mut previous, x1, y1);
                    encode_point(&mut commands, &mut previous, x, y);
                }
                OutlineCommand::CurveTo(x1, y1, x2, y2, x, y) => {
                    commands.push(3);
                    encode_point(&mut commands, &mut previous, x1, y1);
                    encode_point(&mut commands, &mut previous, x2, y2);
                    encode_point(&mut commands, &mut previous, x, y);
                }
                OutlineCommand::Close => commands.push(4),
            }
        }
        output.extend_from_slice(&(*character as u32).to_le_bytes());
        output.extend_from_slice(&glyph.advance.to_le_bytes());
        output.extend_from_slice(&(u32::try_from(commands.len()).context("outline command data is too large")?).to_le_bytes());
        output.extend_from_slice(&commands);
    }
    Ok(output)
}

pub fn decode_outline_font(bytes: &[u8]) -> Result<OutlineFont> {
    ensure!(bytes.len() >= 20, "outline font header is truncated");
    ensure!(&bytes[..8] == OUTLINE_MAGIC, "outline font magic is invalid");
    let version = u16::from_le_bytes(bytes[8..10].try_into().expect("two-byte version"));
    ensure!(version == OUTLINE_FORMAT_VERSION, "unsupported outline font version {version}");
    let units_per_em = u16::from_le_bytes(bytes[10..12].try_into().expect("two-byte units"));
    let ascender = i16::from_le_bytes(bytes[12..14].try_into().expect("two-byte ascender"));
    let descender = i16::from_le_bytes(bytes[14..16].try_into().expect("two-byte descender"));
    let count = u32::from_le_bytes(bytes[16..20].try_into().expect("four-byte count"));
    let mut offset = 20usize;
    let mut glyphs = BTreeMap::new();
    for _ in 0..count {
        ensure!(offset + 10 <= bytes.len(), "outline glyph header is truncated");
        let codepoint = u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four-byte codepoint"));
        let character = char::from_u32(codepoint).with_context(|| format!("invalid Unicode scalar U+{codepoint:04X}"))?;
        let advance = u16::from_le_bytes(bytes[offset + 4..offset + 6].try_into().expect("two-byte advance"));
        let length = u32::from_le_bytes(bytes[offset + 6..offset + 10].try_into().expect("four-byte length")) as usize;
        offset += 10;
        let end = offset.checked_add(length).context("outline glyph length overflow")?;
        ensure!(end <= bytes.len(), "outline glyph data is truncated");
        let commands = decode_outline_commands(&bytes[offset..end])?;
        offset = end;
        ensure!(
            glyphs.insert(character, OutlineGlyph { advance, commands }).is_none(),
            "duplicate outline glyph for {character:?}"
        );
    }
    ensure!(offset == bytes.len(), "outline font has trailing data");
    Ok(OutlineFont {
        units_per_em,
        ascender,
        descender,
        glyphs,
    })
}

fn encode_point(output: &mut Vec<u8>, previous: &mut (i32, i32), x: i32, y: i32) {
    encode_varint(output, zigzag(x.wrapping_sub(previous.0)));
    encode_varint(output, zigzag(y.wrapping_sub(previous.1)));
    *previous = (x, y);
}

fn decode_outline_commands(bytes: &[u8]) -> Result<Vec<OutlineCommand>> {
    let mut offset = 0usize;
    let mut previous = (0i32, 0i32);
    let mut commands = Vec::new();
    while offset < bytes.len() {
        let opcode = bytes[offset];
        offset += 1;
        let mut point = || -> Result<(i32, i32)> {
            let x = previous.0.wrapping_add(unzigzag(decode_varint(bytes, &mut offset)?));
            let y = previous.1.wrapping_add(unzigzag(decode_varint(bytes, &mut offset)?));
            previous = (x, y);
            Ok(previous)
        };
        commands.push(match opcode {
            0 => {
                let (x, y) = point()?;
                OutlineCommand::MoveTo(x, y)
            }
            1 => {
                let (x, y) = point()?;
                OutlineCommand::LineTo(x, y)
            }
            2 => {
                let (x1, y1) = point()?;
                let (x, y) = point()?;
                OutlineCommand::QuadTo(x1, y1, x, y)
            }
            3 => {
                let (x1, y1) = point()?;
                let (x2, y2) = point()?;
                let (x, y) = point()?;
                OutlineCommand::CurveTo(x1, y1, x2, y2, x, y)
            }
            4 => OutlineCommand::Close,
            value => bail!("invalid outline opcode {value}"),
        });
    }
    Ok(commands)
}

fn zigzag(value: i32) -> u32 {
    ((value << 1) ^ (value >> 31)) as u32
}

fn unzigzag(value: u32) -> i32 {
    ((value >> 1) as i32) ^ -((value & 1) as i32)
}

fn encode_varint(output: &mut Vec<u8>, mut value: u32) {
    while value >= 0x80 {
        output.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    output.push(value as u8);
}

fn decode_varint(bytes: &[u8], offset: &mut usize) -> Result<u32> {
    let mut value = 0u32;
    for shift in (0..35).step_by(7) {
        let byte = *bytes.get(*offset).context("truncated outline coordinate")?;
        *offset += 1;
        ensure!(shift != 28 || byte <= 0x0f, "outline coordinate varint overflow");
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    bail!("outline coordinate varint overflow")
}

fn gzip(content: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = GzBuilder::new().mtime(0).write(Vec::new(), Compression::best());
    encoder.write_all(content)?;
    Ok(encoder.finish()?)
}

pub fn gzip_deterministic(content: &[u8]) -> Result<Vec<u8>> {
    gzip(content)
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
