//! Entry point for the timing tests.
//!
//! These tests measure image creation latency and assert budgets in release
//! builds, so they live in a binary of their own rather than in `tests/all/`,
//! where they would share the CPU with several hundred concurrent tests. Inside
//! this binary every test takes [`perf_lock`], so only one measurement runs at
//! a time.

use std::sync::{Mutex, MutexGuard, PoisonError};

static PERF_LOCK: Mutex<()> = Mutex::new(());

/// Serializes the timing tests. A failed test does not poison the others.
pub fn perf_lock() -> MutexGuard<'static, ()> {
    PERF_LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

mod analysis_image_perf;
mod bigtext_image_perf;
mod cell_goal_perf;
mod keyboard_image_perf;
mod live_goal_perf;
mod result_image_perf;
mod title_image_perf;
