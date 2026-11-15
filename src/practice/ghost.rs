//! Selection and replay of comparable historical keystroke tracks.

use std::time::Duration;

use crate::engine::session::KeystrokeKind;
use crate::record::SessionRecord;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GhostCondition {
    Sentence { text: String },
    Bench { bench_id: String },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GhostTrack {
    events: Vec<(u32, u64)>,
    sentence_text: Option<String>,
    opponent: String,
}

impl GhostTrack {
    pub fn position_at(&self, elapsed: Duration) -> usize {
        let elapsed_us = elapsed.as_micros().min(u128::from(u64::MAX)) as u64;
        self.events.iter().filter(|(_, time)| *time <= elapsed_us).count()
    }

    pub fn position_for_item(&self, elapsed: Duration, item_idx: u32) -> usize {
        let elapsed_us = elapsed.as_micros().min(u128::from(u64::MAX)) as u64;
        self.events.iter().filter(|(item, time)| *item == item_idx && *time <= elapsed_us).count()
    }

    pub fn applies_to_sentence(&self, text: &str) -> bool {
        self.sentence_text.as_deref() == Some(text)
    }

    pub fn opponent(&self) -> &str {
        &self.opponent
    }
}

pub fn select_ghost(records: &[SessionRecord], condition: &GhostCondition) -> Option<GhostTrack> {
    let (record, sentence_item) = match condition {
        GhostCondition::Sentence { text } => {
            let record = records
                .iter()
                .filter(|record| record.header.mode == "sentence" && score_eligible(record) && record.log.items.iter().any(|item| item.text == *text))
                .max_by(|left, right| left.header.net_wpm.total_cmp(&right.header.net_wpm))?;
            let item = record.log.items.iter().find(|item| item.text == *text)?;
            (record, Some((item.idx, item.shown_us)))
        }
        GhostCondition::Bench { bench_id } => {
            let record = records
                .iter()
                .rev()
                .find(|record| record.header.mode == "bench" && record.header.bench_id.as_deref() == Some(bench_id))?;
            (record, None)
        }
    };
    let opponent = record.header.id.map_or_else(
        || match condition {
            GhostCondition::Sentence { text } => format!("sentence:{text}"),
            GhostCondition::Bench { bench_id } => format!("bench:{bench_id}"),
        },
        |id| format!("session:{id}"),
    );
    Some(GhostTrack {
        events: record
            .log
            .keystrokes
            .iter()
            .filter(|key| key.kind == KeystrokeKind::Char && key.correct)
            .filter_map(|key| match sentence_item {
                Some((item_idx, shown_us)) if key.item_idx == item_idx => Some((0, key.t_us.saturating_sub(shown_us))),
                Some(_) => None,
                None => Some((key.item_idx, key.t_us)),
            })
            .collect(),
        sentence_text: match condition {
            GhostCondition::Sentence { text } => Some(text.clone()),
            GhostCondition::Bench { .. } => None,
        },
        opponent,
    })
}

fn score_eligible(record: &SessionRecord) -> bool {
    serde_json::from_str::<serde_json::Value>(&record.header.mode_params)
        .ok()
        .and_then(|value| value.get("score_eligible").and_then(serde_json::Value::as_bool))
        .unwrap_or(true)
}
