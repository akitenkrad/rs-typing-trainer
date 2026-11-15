//! Input judges used by typing items.

use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JudgeResult {
    Correct,
    Mistype,
    ItemCompleted,
}

/// Stateful judge. Several candidates may be exposed for future judges such
/// as `RomajiJudge`.
pub trait Judge: std::fmt::Debug {
    fn position(&self) -> usize;
    fn expected_candidates(&self) -> &[String];
    fn input(&mut self, typed: &str) -> JudgeResult;
    fn backspace(&mut self) -> bool;
    fn guide(&self) -> String;
    fn completed_reading_chars(&self) -> usize {
        self.position()
    }
}

#[derive(Clone, Debug)]
pub struct ExactJudge {
    graphemes: Vec<String>,
    position: usize,
}

impl ExactJudge {
    pub fn new(text: &str) -> Self {
        Self {
            graphemes: text.graphemes(true).map(str::to_owned).collect(),
            position: 0,
        }
    }
}

impl Judge for ExactJudge {
    fn position(&self) -> usize {
        self.position
    }

    fn expected_candidates(&self) -> &[String] {
        self.graphemes.get(self.position).map(std::slice::from_ref).unwrap_or(&[])
    }

    fn input(&mut self, typed: &str) -> JudgeResult {
        let Some(expected) = self.graphemes.get(self.position) else {
            return JudgeResult::Mistype;
        };
        if expected != typed {
            return JudgeResult::Mistype;
        }
        self.position += 1;
        if self.position == self.graphemes.len() {
            JudgeResult::ItemCompleted
        } else {
            JudgeResult::Correct
        }
    }

    fn backspace(&mut self) -> bool {
        if self.position == 0 {
            false
        } else {
            self.position -= 1;
            true
        }
    }

    fn guide(&self) -> String {
        self.graphemes.concat()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_judge_uses_grapheme_clusters() {
        let mut judge = ExactJudge::new("a👩‍💻e\u{301}");
        assert_eq!(judge.input("a"), JudgeResult::Correct);
        assert_eq!(judge.input("👩‍💻"), JudgeResult::Correct);
        assert_eq!(judge.input("x"), JudgeResult::Mistype);
        assert_eq!(judge.input("e\u{301}"), JudgeResult::ItemCompleted);
    }
}
