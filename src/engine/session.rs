//! Deterministic session progression and keystroke logging.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::ErrorMode;
use crate::engine::item::Item;
use crate::engine::judge::JudgeResult;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemOutcome {
    Completed,
    Timeout,
    Aborted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeystrokeKind {
    Char,
    Backspace,
}

/// The role of a displayed item in training and later evaluation.
///
/// `None` in persisted [`ItemLog`] values means that the record predates this
/// field; callers must not invent a kind for such records.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    Drill,
    Word,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemLog {
    pub idx: u32,
    pub text: String,
    pub reading: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ItemKind>,
    pub shown_us: u64,
    pub done_us: Option<u64>,
    pub outcome: ItemOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeystrokeLog {
    pub seq: u32,
    pub t_us: u64,
    pub release_us: Option<u64>,
    pub item_idx: u32,
    pub pos: u32,
    pub kind: KeystrokeKind,
    pub typed: Option<String>,
    pub expected: Option<String>,
    pub correct: bool,
    pub code: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionLog {
    pub duration_us: u64,
    #[serde(default)]
    pub error_mode: ErrorMode,
    pub items: Vec<ItemLog>,
    pub keystrokes: Vec<KeystrokeLog>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionPhase {
    Entering,
    Active,
    Exiting,
    Finished,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputEffect {
    Dropped,
    Correct,
    Mistype,
    ItemCompleted,
    ItemCompletedMistyped,
    Backspace,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TickResult {
    pub item_timed_out: bool,
    pub session_finished: bool,
}

pub struct Session {
    duration: Duration,
    item_timeout: Duration,
    current: Item,
    current_idx: u32,
    phase: SessionPhase,
    log: SessionLog,
    last_t_us: u64,
    error_mode: ErrorMode,
    advance_slots: Vec<bool>,
}

struct PendingKey {
    kind: KeystrokeKind,
    typed: Option<String>,
    expected: Option<String>,
    correct: bool,
    code: Option<String>,
}

impl Session {
    pub fn new(duration: Duration, item_timeout: Duration, first: Item) -> Self {
        Self::with_error_mode(duration, item_timeout, first, ErrorMode::Stop)
    }

    pub fn with_error_mode(duration: Duration, item_timeout: Duration, first: Item, error_mode: ErrorMode) -> Self {
        let log = SessionLog {
            error_mode,
            ..SessionLog::default()
        };
        Self {
            duration,
            item_timeout,
            current: first,
            current_idx: 0,
            phase: SessionPhase::Entering,
            log,
            last_t_us: 0,
            error_mode,
            advance_slots: Vec::new(),
        }
    }

    pub fn phase(&self) -> SessionPhase {
        self.phase
    }

    pub fn current_text(&self) -> &str {
        &self.current.text
    }

    pub fn position(&self) -> usize {
        self.current.judge.position()
    }

    pub fn expected(&self) -> Option<String> {
        self.current.judge.expected_candidates().first().cloned()
    }

    pub fn current_guide(&self) -> String {
        self.current.judge.guide()
    }

    pub fn completed_reading_chars(&self) -> usize {
        self.current.judge.completed_reading_chars()
    }

    pub fn log(&self) -> &SessionLog {
        &self.log
    }

    pub fn activate(&mut self, now: Duration) {
        if self.phase != SessionPhase::Entering {
            return;
        }
        let shown_us = self.monotonic_us(now);
        self.log.items.push(ItemLog {
            idx: self.current_idx,
            text: self.current.text.clone(),
            reading: self.current.reading.clone(),
            kind: Some(self.current.kind),
            shown_us,
            done_us: None,
            outcome: ItemOutcome::Aborted,
        });
        self.phase = SessionPhase::Active;
    }

    pub fn input_char(&mut self, now: Duration, typed: &str) -> InputEffect {
        self.input_char_with_code(now, typed, None)
    }

    pub fn input_char_with_code(&mut self, now: Duration, typed: &str, code: Option<String>) -> InputEffect {
        let tick = self.tick(now);
        if tick.session_finished || tick.item_timed_out {
            return InputEffect::Dropped;
        }
        if self.phase != SessionPhase::Active {
            return InputEffect::Dropped;
        }
        let t_us = self.monotonic_us(now);
        let pos = self.current.judge.position() as u32;
        let expected = self.current.judge.expected_candidates().first().cloned();
        let mut judged = self.current.judge.input(typed);
        let correct = !matches!(judged, JudgeResult::Mistype);
        if !correct
            && self.error_mode == ErrorMode::Advance
            && let Some(expected) = expected.as_deref()
        {
            judged = self.current.judge.input(expected);
        }
        if self.error_mode == ErrorMode::Advance {
            self.advance_slots.push(correct);
        }
        self.push_key(
            t_us,
            pos,
            PendingKey {
                kind: KeystrokeKind::Char,
                typed: Some(typed.to_owned()),
                expected,
                correct,
                code,
            },
        );
        match judged {
            JudgeResult::Correct if !correct => InputEffect::Mistype,
            JudgeResult::Correct => InputEffect::Correct,
            JudgeResult::Mistype => InputEffect::Mistype,
            JudgeResult::ItemCompleted => {
                self.complete_item(t_us, ItemOutcome::Completed);
                if correct && self.advance_slots.iter().all(|slot| *slot) {
                    InputEffect::ItemCompleted
                } else {
                    InputEffect::ItemCompletedMistyped
                }
            }
        }
    }

    pub fn input_backspace(&mut self, now: Duration) -> InputEffect {
        self.input_backspace_with_code(now, None)
    }

    pub fn input_backspace_with_code(&mut self, now: Duration, code: Option<String>) -> InputEffect {
        let tick = self.tick(now);
        if tick.session_finished || tick.item_timed_out {
            return InputEffect::Dropped;
        }
        if self.phase != SessionPhase::Active {
            return InputEffect::Dropped;
        }
        let t_us = self.monotonic_us(now);
        let pos = self.current.judge.position() as u32;
        let expected = self.current.judge.expected_candidates().first().cloned();
        self.push_key(
            t_us,
            pos,
            PendingKey {
                kind: KeystrokeKind::Backspace,
                typed: None,
                expected,
                correct: false,
                code,
            },
        );
        if self.error_mode == ErrorMode::Advance && self.current.judge.backspace() {
            self.advance_slots.pop();
        }
        InputEffect::Backspace
    }

    /// Attach a release timestamp to the newest still-held press of `code`.
    pub fn release_key(&mut self, now: Duration, code: &str) -> bool {
        let release_us = self.monotonic_us(now);
        let Some(key) = self
            .log
            .keystrokes
            .iter_mut()
            .rev()
            .find(|key| key.release_us.is_none() && key.code.as_deref() == Some(code))
        else {
            return false;
        };
        key.release_us = Some(release_us.max(key.t_us));
        true
    }

    pub fn tick(&mut self, now: Duration) -> TickResult {
        if self.phase == SessionPhase::Finished {
            return TickResult {
                session_finished: true,
                ..TickResult::default()
            };
        }
        if now >= self.duration {
            let t_us = self.monotonic_us(self.duration);
            self.abort_open_item(t_us);
            self.log.duration_us = self.duration.as_micros().min(u64::MAX as u128) as u64;
            self.phase = SessionPhase::Finished;
            return TickResult {
                session_finished: true,
                ..TickResult::default()
            };
        }
        let t_us = self.monotonic_us(now);
        let timeout_us = self.item_timeout.as_micros().min(u64::MAX as u128) as u64;
        if timeout_us > 0
            && self.phase == SessionPhase::Active
            && let Some(item) = self.log.items.last()
            && t_us.saturating_sub(item.shown_us) >= timeout_us
        {
            self.complete_item(t_us, ItemOutcome::Timeout);
            return TickResult {
                item_timed_out: true,
                ..TickResult::default()
            };
        }
        TickResult::default()
    }

    pub fn advance(&mut self, next: Item, now: Duration) {
        if self.phase != SessionPhase::Exiting {
            return;
        }
        self.monotonic_us(now);
        self.current_idx += 1;
        self.current = next;
        self.advance_slots.clear();
        self.phase = SessionPhase::Entering;
    }

    pub fn skip_item(&mut self, now: Duration) {
        if matches!(self.phase, SessionPhase::Entering | SessionPhase::Active) {
            let t_us = self.monotonic_us(now);
            self.complete_item(t_us, ItemOutcome::Aborted);
        }
    }

    pub fn abort(&mut self, now: Duration) {
        let t_us = self.monotonic_us(now);
        self.abort_open_item(t_us);
        self.log.duration_us = t_us;
        self.phase = SessionPhase::Finished;
    }

    pub fn into_log(mut self) -> SessionLog {
        if self.log.duration_us == 0 {
            self.log.duration_us = self.last_t_us;
        }
        self.log
    }

    fn monotonic_us(&mut self, now: Duration) -> u64 {
        let raw = now.as_micros().min(u64::MAX as u128) as u64;
        let value = raw.max(self.last_t_us);
        self.last_t_us = value;
        value
    }

    fn push_key(&mut self, t_us: u64, pos: u32, key: PendingKey) {
        self.log.keystrokes.push(KeystrokeLog {
            seq: self.log.keystrokes.len() as u32,
            t_us,
            release_us: None,
            item_idx: self.current_idx,
            pos,
            kind: key.kind,
            typed: key.typed,
            expected: key.expected,
            correct: key.correct,
            code: key.code,
        });
    }

    fn complete_item(&mut self, t_us: u64, outcome: ItemOutcome) {
        if let Some(item) = self.log.items.last_mut() {
            item.done_us = Some(t_us);
            item.outcome = outcome;
        }
        self.phase = SessionPhase::Exiting;
    }

    fn abort_open_item(&mut self, t_us: u64) {
        if self.phase == SessionPhase::Active {
            self.complete_item(t_us, ItemOutcome::Aborted);
        }
    }
}
