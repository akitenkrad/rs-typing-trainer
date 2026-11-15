//! Tiny timer helpers used by the game loop. Keeping these as pure helpers
//! makes them trivial to unit-test against synthetic `Instant` differences.

use std::time::{Duration, Instant};

#[inline]
pub fn elapsed_since(start: Instant, now: Instant) -> Duration {
    now.saturating_duration_since(start)
}

#[inline]
pub fn is_expired(start: Instant, limit: Duration, now: Instant) -> bool {
    elapsed_since(start, now) >= limit
}

#[inline]
pub fn remaining(start: Instant, limit: Duration, now: Instant) -> Duration {
    limit.saturating_sub(elapsed_since(start, now))
}
