//! Game-domain logic: word generation, timer, and input handling.

pub mod input;
pub mod timer;
pub mod word;

pub use word::{ExitKind, Word, WordSlot, WordState, generate_word};
