//! Japanese passages: three to five consecutive sentences of one Aozora Bunko
//! paragraph, joined without a separator.

use std::collections::{BTreeMap, BTreeSet};

use crate::generate::{valid_sentence_reading, valid_sentence_text_shape};
use crate::{fnv1a, stable_sample};

/// Fewest sentences in one passage.
pub const JA_PASSAGE_MIN_SENTENCES: usize = 3;
/// Most sentences in one passage.
pub const JA_PASSAGE_MAX_SENTENCES: usize = 5;
/// Longest passage text, in characters.
pub const JA_PASSAGE_MAX_CHARS: usize = 160;
/// Most passages taken from one work.
pub const JA_PASSAGES_PER_WORK: usize = 8;
/// Sampling seed for passages, distinct from the sentence seed.
pub const JA_PASSAGE_SEED: u64 = 0x5253_5459_5041_5353;

/// One sentence of a work in body order.
///
/// `reading` is `None` when the reading could not be produced: the sentence
/// touches gaiji or contains a token whose IPADIC reading is unknown. The
/// text conditions (length, characters, balanced quotes) and the overrides are
/// applied by [`select_work_passages`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PassageSentence {
    pub text: String,
    pub reading: Option<String>,
    /// Body line (Aozora paragraph) the sentence belongs to.
    pub line: usize,
    /// Byte offset of the sentence in the body; orders the output.
    pub start: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct JapanesePassage {
    pub text: String,
    pub reading: String,
    pub source_id: String,
    pub author: String,
    pub start: usize,
    pub sentences: usize,
}

impl AsRef<str> for JapanesePassage {
    fn as_ref(&self) -> &str {
        &self.text
    }
}

/// Applies the sentence conditions and overrides, finds runs of usable
/// sentences that are adjacent in one paragraph, cuts each run into chunks of
/// three to five sentences, and samples at most [`JA_PASSAGES_PER_WORK`]
/// chunks with a fixed seed. The result is ordered by body position.
pub fn select_work_passages(
    work_id: &str,
    author: &str,
    sentences: &[PassageSentence],
    overrides: &BTreeMap<String, Option<String>>,
) -> Vec<JapanesePassage> {
    let usable = sentences.iter().map(|sentence| usable_reading(sentence, overrides)).collect::<Vec<_>>();

    let mut candidates = Vec::new();
    let mut run_start = 0;
    while run_start < sentences.len() {
        if usable[run_start].is_none() {
            run_start += 1;
            continue;
        }
        let mut run_end = run_start + 1;
        while run_end < sentences.len() && usable[run_end].is_some() && sentences[run_end].line == sentences[run_start].line {
            run_end += 1;
        }
        for (chunk_start, chunk_len) in chunk_run(run_end - run_start) {
            let indices = (run_start + chunk_start..run_start + chunk_start + chunk_len).collect::<Vec<_>>();
            if let Some(passage) = build_passage(work_id, author, sentences, &usable, &indices) {
                candidates.push(passage);
            }
        }
        run_start = run_end;
    }

    candidates.sort();
    let mut seen = BTreeSet::new();
    candidates.retain(|passage| seen.insert(passage.text.clone()));
    let mut selected = stable_sample(&candidates, JA_PASSAGES_PER_WORK, JA_PASSAGE_SEED ^ fnv1a(work_id.as_bytes(), 0));
    selected.sort_by_key(|passage| passage.start);
    selected
}

/// Joins the per-work selections, keeping the first occurrence of a text in
/// work order, and orders the rows by work id and body position.
pub fn merge_passages(per_work: impl IntoIterator<Item = Vec<JapanesePassage>>) -> Vec<JapanesePassage> {
    let mut seen = BTreeSet::new();
    let mut merged = per_work
        .into_iter()
        .flatten()
        .filter(|passage| seen.insert(passage.text.clone()))
        .collect::<Vec<_>>();
    merged.sort_by(|left, right| left.source_id.cmp(&right.source_id).then(left.start.cmp(&right.start)));
    merged
}

/// `text<TAB>reading<TAB>work id<TAB>author<TAB>ext`, one passage per line.
pub fn render_ja_passages(passages: &[JapanesePassage]) -> String {
    passages
        .iter()
        .map(|passage| format!("{}\t{}\t{}\t{}\text\n", passage.text, passage.reading, passage.source_id, passage.author))
        .collect()
}

fn usable_reading(sentence: &PassageSentence, overrides: &BTreeMap<String, Option<String>>) -> Option<String> {
    if !valid_sentence_text_shape(&sentence.text) {
        return None;
    }
    let generated = sentence.reading.as_ref()?;
    let reading = match overrides.get(&sentence.text) {
        Some(None) => return None,
        Some(Some(reading)) => reading.clone(),
        None => generated.clone(),
    };
    valid_sentence_reading(&sentence.text, &reading).then_some(reading)
}

/// Splits a run of `len` sentences into chunks of three to five sentences of
/// nearly equal size. Returns `(offset, length)` pairs; a run shorter than
/// three yields nothing.
pub fn chunk_run(len: usize) -> Vec<(usize, usize)> {
    if len < JA_PASSAGE_MIN_SENTENCES {
        return Vec::new();
    }
    let count = len.div_ceil(JA_PASSAGE_MAX_SENTENCES);
    let base = len / count;
    let extra = len % count;
    let mut chunks = Vec::with_capacity(count);
    let mut offset = 0;
    for index in 0..count {
        let size = base + usize::from(index < extra);
        chunks.push((offset, size));
        offset += size;
    }
    chunks
}

fn build_passage(
    work_id: &str,
    author: &str,
    sentences: &[PassageSentence],
    usable: &[Option<String>],
    indices: &[usize],
) -> Option<JapanesePassage> {
    let mut count = indices.len();
    loop {
        if count < JA_PASSAGE_MIN_SENTENCES {
            return None;
        }
        let text = indices[..count].iter().map(|index| sentences[*index].text.as_str()).collect::<String>();
        if text.chars().count() <= JA_PASSAGE_MAX_CHARS {
            let reading = indices[..count]
                .iter()
                .map(|index| usable[*index].as_deref().expect("chunk holds usable sentences"))
                .collect::<String>();
            return Some(JapanesePassage {
                text,
                reading,
                source_id: work_id.to_owned(),
                author: author.to_owned(),
                start: sentences[indices[0]].start,
                sentences: count,
            });
        }
        count -= 1;
    }
}
