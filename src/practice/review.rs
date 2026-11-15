//! Spaced review state for previously mastered transitions.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReviewSchedule {
    pub layout: String,
    pub from: String,
    pub to: String,
    pub next_due: DateTime<Utc>,
    pub interval_days: f64,
    pub last_reviewed_at: DateTime<Utc>,
}

pub fn update_review(
    previous: Option<ReviewSchedule>,
    layout: &str,
    from: &str,
    to: &str,
    at: DateTime<Utc>,
    retained: bool,
    regressed: bool,
) -> ReviewSchedule {
    let interval_days = if regressed || !retained {
        1.0
    } else {
        previous.as_ref().map_or(1.0, |row| row.interval_days * 2.5)
    };
    ReviewSchedule {
        layout: layout.into(),
        from: from.into(),
        to: to.into(),
        next_due: at + Duration::milliseconds((interval_days * 86_400_000.0).round() as i64),
        interval_days,
        last_reviewed_at: at,
    }
}
