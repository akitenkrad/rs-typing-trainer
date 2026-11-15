//! Word and word-slot model. Words are stored as a vector of grapheme
//! clusters so that symbols, combining characters and emoji can be supported
//! without surprises.

use rand::Rng;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Word {
    /// Concatenated string used for display and word-level keys.
    pub display: String,
    /// One element per grapheme cluster.
    pub graphemes: Vec<String>,
}

impl Word {
    pub fn from_graphemes(gs: &[String]) -> Self {
        Self {
            display: gs.concat(),
            graphemes: gs.to_vec(),
        }
    }

    pub fn len(&self) -> usize {
        self.graphemes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.graphemes.is_empty()
    }

    pub fn nth(&self, i: usize) -> Option<&str> {
        self.graphemes.get(i).map(|s| s.as_str())
    }
}

/// A word currently shown on screen, together with its animation state.
#[derive(Clone, Debug)]
pub struct WordSlot {
    pub word: Word,
    pub state: WordState,
}

impl WordSlot {
    pub fn new_entering(word: Word) -> Self {
        Self {
            word,
            state: WordState::Entering { progress: 0.0 },
        }
    }
}

#[derive(Clone, Debug)]
pub enum WordState {
    /// Slide-in animation in progress; key input is dropped here.
    Entering { progress: f32 },
    /// Word is fully visible and accepting input.
    Active,
    /// Slide-out / fade animation; input is also dropped here.
    Exiting { kind: ExitKind, progress: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitKind {
    Correct,
    Mistyped,
    Timeout,
}

/// Generate the next word from the given charset.
///
/// `charset` must contain grapheme clusters as separate `String` items.
/// `previous` is the display string of the most recent word, used to avoid
/// repeating the same word back-to-back when configured.
pub fn generate_word(
    charset: &[String],
    length: usize,
    allow_repeat: bool,
    allow_consecutive_same_word: bool,
    previous: Option<&str>,
    rng: &mut impl Rng,
) -> Word {
    loop {
        let graphemes: Vec<String> = if allow_repeat {
            (0..length).map(|_| charset.choose(rng).expect("non-empty charset").clone()).collect()
        } else {
            charset.choose_multiple(rng, length).cloned().collect()
        };
        let word = Word::from_graphemes(&graphemes);
        if allow_consecutive_same_word {
            return word;
        }
        match previous {
            Some(p) if p == word.display => continue,
            _ => return word,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::thread_rng;

    #[test]
    fn word_length_matches_request() {
        let charset: Vec<String> = "abcde".chars().map(|c| c.to_string()).collect();
        let mut rng = thread_rng();
        for _ in 0..100 {
            let w = generate_word(&charset, 3, true, true, None, &mut rng);
            assert_eq!(w.len(), 3);
        }
    }

    #[test]
    fn no_repeat_yields_unique_graphemes() {
        let charset: Vec<String> = "abcde".chars().map(|c| c.to_string()).collect();
        let mut rng = thread_rng();
        for _ in 0..100 {
            let w = generate_word(&charset, 3, false, true, None, &mut rng);
            let mut sorted = w.graphemes.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(sorted.len(), 3);
        }
    }

    #[test]
    fn previous_word_excluded_when_consecutive_disallowed() {
        let charset: Vec<String> = vec!["a".into(), "b".into()];
        let mut rng = thread_rng();
        for _ in 0..50 {
            let w = generate_word(&charset, 2, true, false, Some("aa"), &mut rng);
            assert_ne!(w.display, "aa");
        }
    }
}
