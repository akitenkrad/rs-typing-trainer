use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

mod font;
mod generate;

pub use font::{
    BIZUD_ARCHIVE, BIZUD_ARCHIVE_SHA256, BdfGlyph, BizUdGenerationOptions, BizUdGenerationReport, FontGenerationOptions, FontGenerationReport,
    FontKind, OutlineCommand, OutlineFont, OutlineGlyph, SHINONOME_ARCHIVE, SHINONOME_ARCHIVE_SHA256, decode_outline_font, encode_outline_font,
    generate_bizud_font, generate_fonts, gzip_deterministic, jis_x0208_to_char, parse_bdf, update_bizud_manifest, update_bizud_notice,
    update_shinonome_manifest, update_shinonome_notice,
};
pub use generate::{GenerationOptions, GenerationReport, generate, prepare_english_phrase, prepare_english_sentence, prepare_japanese_phrases};

const SCOWL_LEVELS: [u8; 6] = [10, 20, 35, 40, 50, 60];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RubySpan {
    pub start: usize,
    pub end: usize,
    pub reading: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AozoraDocument {
    pub body: String,
    pub rubies: BTreeMap<String, BTreeSet<String>>,
    pub ruby_spans: Vec<RubySpan>,
    gaiji_ranges: Vec<(usize, usize)>,
}

impl AozoraDocument {
    pub fn has_gaiji_in(&self, start: usize, end: usize) -> bool {
        self.gaiji_ranges
            .iter()
            .any(|(gaiji_start, gaiji_end)| *gaiji_start < end && start < *gaiji_end)
    }
}

pub fn extract_english_body(source: &str) -> Result<&str> {
    let start = source.find("*** START OF").context("English book has no START marker")?;
    let content_start = source[start..]
        .find('\n')
        .map(|offset| start + offset + 1)
        .context("English START marker has no following line")?;
    let end_offset = source[content_start..].find("*** END OF").context("English book has no END marker")?;
    Ok(&source[content_start..content_start + end_offset])
}

pub fn extract_aozora(source: &str) -> AozoraDocument {
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    let lines = normalized.lines().collect::<Vec<_>>();
    let separators = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| is_separator(line).then_some(index))
        .collect::<Vec<_>>();
    let start = if separators.len() >= 2 {
        separators[1] + 1
    } else {
        lines.iter().position(|line| line.trim().is_empty()).map_or(0, |index| index + 1)
    };
    let end = lines[start..]
        .iter()
        .position(|line| line.trim_start().starts_with("底本："))
        .map_or(lines.len(), |offset| start + offset);

    let mut document = AozoraDocument::default();
    for line in &lines[start..end] {
        let has_gaiji = line.contains("※［＃");
        let first_new_span = document.ruby_spans.len();
        let stripped = strip_aozora_line(line, 0, &mut document);
        let trimmed_start = stripped.trim_start_matches([' ', '　', '\t']);
        let leading_bytes = stripped.len() - trimmed_start.len();
        let stripped = trimmed_start.trim_end_matches([' ', '　', '\t']);
        if stripped.is_empty() {
            document.ruby_spans.truncate(first_new_span);
            continue;
        }
        if !document.body.is_empty() {
            document.body.push('\n');
        }
        let actual_start = document.body.len();
        for span in &mut document.ruby_spans[first_new_span..] {
            span.start = actual_start + span.start.saturating_sub(leading_bytes);
            span.end = actual_start + span.end.saturating_sub(leading_bytes);
        }
        document.body.push_str(stripped);
        if has_gaiji {
            document.gaiji_ranges.push((actual_start, document.body.len()));
        }
    }
    document
}

fn is_separator(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.len() >= 10 && trimmed.bytes().all(|byte| byte == b'-')
}

fn strip_aozora_line(line: &str, base_offset: usize, document: &mut AozoraDocument) -> String {
    let mut output = String::new();
    let mut cursor = 0;
    while cursor < line.len() {
        let rest = &line[cursor..];
        if rest.starts_with("［＃")
            && let Some(end) = rest.find('］')
        {
            cursor += end + '］'.len_utf8();
            continue;
        }
        if rest.starts_with('｜')
            && let Some(open) = rest.find('《')
            && let Some(close_rel) = rest[open + '《'.len_utf8()..].find('》')
        {
            let parent = &rest['｜'.len_utf8()..open];
            let ruby_start = open + '《'.len_utf8();
            let ruby_end = ruby_start + close_rel;
            let reading = &rest[ruby_start..ruby_end];
            append_ruby(&mut output, parent, reading, base_offset, document);
            cursor += ruby_end + '》'.len_utf8();
            continue;
        }
        if rest.starts_with('《')
            && let Some(close) = rest.find('》')
        {
            let reading = &rest['《'.len_utf8()..close];
            let parent_start = implicit_parent_start(&output);
            let parent = output[parent_start..].to_owned();
            if !parent.is_empty() {
                document.rubies.entry(parent).or_default().insert(katakana_to_hiragana(reading));
                document.ruby_spans.push(RubySpan {
                    start: base_offset + parent_start,
                    end: base_offset + output.len(),
                    reading: katakana_to_hiragana(reading),
                });
            }
            cursor += close + '》'.len_utf8();
            continue;
        }
        let character = rest.chars().next().expect("cursor is on a char boundary");
        output.push(character);
        cursor += character.len_utf8();
    }
    output
}

fn append_ruby(output: &mut String, parent: &str, reading: &str, base_offset: usize, document: &mut AozoraDocument) {
    let start = base_offset + output.len();
    output.push_str(parent);
    let reading = katakana_to_hiragana(reading);
    document.rubies.entry(parent.to_owned()).or_default().insert(reading.clone());
    document.ruby_spans.push(RubySpan {
        start,
        end: base_offset + output.len(),
        reading,
    });
}

fn implicit_parent_start(output: &str) -> usize {
    output
        .char_indices()
        .rev()
        .take_while(|(_, character)| is_ruby_parent_character(*character))
        .map(|(index, _)| index)
        .last()
        .unwrap_or(output.len())
}

fn is_ruby_parent_character(character: char) -> bool {
    matches!(
        character,
        '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{f900}'..='\u{faff}' | '々' | 'ヶ'
    )
}

pub fn load_scowl_words(scowl_dir: &Path) -> Result<BTreeSet<String>> {
    let final_dir = scowl_dir.join("final");
    let mut words = BTreeSet::new();
    let mut found = false;
    for family in ["english-words", "american-words"] {
        for level in SCOWL_LEVELS {
            let path = final_dir.join(format!("{family}.{level}"));
            if !path.exists() {
                continue;
            }
            found = true;
            let source = fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
            for word in source.split(|byte| *byte == b'\n') {
                let word = word.trim_ascii();
                if !word.is_empty() && word.iter().all(|byte| byte.is_ascii_alphabetic()) {
                    words.insert(String::from_utf8(word.to_ascii_lowercase()).expect("ASCII word is UTF-8"));
                }
            }
        }
    }
    if !found {
        bail!("no requested SCOWL word lists found under {}", final_dir.display());
    }
    Ok(words)
}

pub fn stable_sample<T>(values: &[T], limit: usize, seed: u64) -> Vec<T>
where
    T: AsRef<str> + Clone,
{
    let mut ranked = values
        .iter()
        .cloned()
        .map(|value| (fnv1a(value.as_ref().as_bytes(), seed), value))
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.as_ref().cmp(right.1.as_ref())));
    ranked.truncate(limit);
    ranked.into_iter().map(|(_, value)| value).collect()
}

pub fn fnv1a(bytes: &[u8], seed: u64) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64 ^ seed;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

pub fn katakana_to_hiragana(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if matches!(character, '\u{30a1}'..='\u{30f6}') {
                char::from_u32(character as u32 - 0x60).unwrap_or(character)
            } else {
                character
            }
        })
        .collect()
}
