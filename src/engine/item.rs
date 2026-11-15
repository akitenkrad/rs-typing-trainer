//! A displayed item and its input judge.

use crate::engine::judge::{ExactJudge, Judge};
use crate::engine::session::ItemKind;

#[derive(Debug)]
pub struct Item {
    pub text: String,
    pub reading: Option<String>,
    pub kind: ItemKind,
    pub judge: Box<dyn Judge>,
}

impl Item {
    pub fn new(text: impl Into<String>, reading: Option<String>, judge: Box<dyn Judge>) -> Self {
        Self {
            text: text.into(),
            reading,
            kind: ItemKind::Word,
            judge,
        }
    }

    pub fn exact(text: impl Into<String>) -> Self {
        let text = text.into();
        let judge = Box::new(ExactJudge::new(&text));
        Self::new(text, None, judge)
    }

    pub fn with_kind(mut self, kind: ItemKind) -> Self {
        self.kind = kind;
        self
    }
}
