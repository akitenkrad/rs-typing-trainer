//! Deterministic playback of portable typing-session records.

use std::fmt::Write as _;
use std::time::Duration;

use crate::analysis::Analysis;
use crate::app::App;
use crate::engine::session::{ItemOutcome, SessionLog};
use crate::i18n::Lang;
use crate::record::SessionRecord;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlaybackTiming {
    Recorded { speed: f64 },
    Fixed { interval: Duration },
}

pub struct Replay {
    record: SessionRecord,
    app: App,
    timing: PlaybackTiming,
    cursor: usize,
    current_item_idx: Option<u32>,
    paused: bool,
    exit_on_end: bool,
    should_exit: bool,
    last_clock: Duration,
    playback_elapsed: Duration,
}

impl Replay {
    pub fn new(record: SessionRecord, timing: PlaybackTiming, paused: bool, exit_on_end: bool) -> Self {
        Self::new_with_lang(record, timing, paused, exit_on_end, Lang::En)
    }

    pub fn new_with_lang(record: SessionRecord, timing: PlaybackTiming, paused: bool, exit_on_end: bool, lang: Lang) -> Self {
        let finished = record.log.keystrokes.is_empty();
        let current_item_idx = record.log.items.first().map(|item| item.idx);
        let mut app = App::for_replay(&record, lang);
        if finished {
            app.replay_finish(Duration::from_micros(record.log.duration_us));
        }
        Self {
            record,
            app,
            timing,
            cursor: 0,
            current_item_idx,
            paused,
            exit_on_end,
            should_exit: finished && exit_on_end,
            last_clock: Duration::ZERO,
            playback_elapsed: Duration::ZERO,
        }
    }

    pub fn record(&self) -> &SessionRecord {
        &self.record
    }

    pub fn app(&self) -> &App {
        &self.app
    }

    pub fn timing(&self) -> PlaybackTiming {
        self.timing
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn is_finished(&self) -> bool {
        self.cursor >= self.record.log.keystrokes.len()
    }

    pub fn should_exit(&self) -> bool {
        self.should_exit
    }

    pub fn applied_keystrokes(&self) -> usize {
        self.cursor
    }

    pub fn advance_to(&mut self, now: Duration) {
        let delta = now.saturating_sub(self.last_clock);
        self.last_clock = self.last_clock.max(now);
        if self.paused || self.is_finished() {
            return;
        }
        self.playback_elapsed = self.playback_elapsed.saturating_add(delta);
        while !self.is_finished() && self.due_at(self.cursor) <= self.playback_elapsed {
            self.apply_one();
        }
    }

    pub fn toggle_pause(&mut self, now: Duration) {
        self.advance_to(now);
        self.paused = !self.paused;
    }

    pub fn step(&mut self) {
        if !self.is_finished() {
            self.apply_one();
        }
    }

    pub fn current_log(&self) -> SessionLog {
        if self.is_finished() {
            return self.record.log.clone();
        }
        let progress_us = self.record.log.keystrokes.get(self.cursor.saturating_sub(1)).map_or(0, |key| key.t_us);
        let mut items = self
            .record
            .log
            .items
            .iter()
            .filter(|item| item.shown_us <= progress_us || item.idx == 0)
            .cloned()
            .collect::<Vec<_>>();
        for item in &mut items {
            if item.done_us.is_none_or(|done| done > progress_us) {
                item.done_us = None;
                item.outcome = ItemOutcome::Aborted;
            }
        }
        SessionLog {
            duration_us: progress_us,
            error_mode: self.record.log.error_mode,
            items,
            keystrokes: self.record.log.keystrokes[..self.cursor].to_vec(),
        }
    }

    pub fn metrics(&self) -> Analysis {
        crate::analysis::analyze(&self.current_log())
    }

    fn apply_one(&mut self) {
        let key = self.record.log.keystrokes[self.cursor].clone();
        if self.current_item_idx != Some(key.item_idx)
            && let Some((item_index, item)) = self.record.log.items.iter().enumerate().find(|(_, item)| item.idx == key.item_idx)
        {
            self.app.replay_show_item(item, &self.record.log.items[item_index + 1..]);
            self.current_item_idx = Some(key.item_idx);
        }
        self.app.replay_apply_key(&key);
        self.cursor += 1;
        if self.is_finished() {
            self.app.replay_finish(Duration::from_micros(self.record.log.duration_us));
            if self.exit_on_end {
                self.should_exit = true;
            }
        }
    }

    fn due_at(&self, index: usize) -> Duration {
        match self.timing {
            PlaybackTiming::Recorded { speed } => {
                let recorded = self.record.log.keystrokes[index].t_us as f64 / 1_000_000.0;
                if speed.is_finite() && speed > 0.0 {
                    Duration::from_secs_f64(recorded / speed)
                } else {
                    Duration::MAX
                }
            }
            PlaybackTiming::Fixed { interval } => interval.saturating_mul((index as u32).saturating_add(1)),
        }
    }
}

pub fn format_summary(record: &SessionRecord) -> String {
    let metrics = crate::analysis::analyze(&record.log);
    let prompts = record.log.items.iter().map(|item| item.text.as_str()).collect::<Vec<_>>().join(" · ");
    let mut output = String::new();
    let _ = writeln!(output, "Mode: {}", record.header.mode);
    let _ = writeln!(output, "Layout: {}", record.header.layout);
    let _ = writeln!(output, "Prompt: {prompts}");
    let _ = writeln!(output, "Items: {}", record.log.items.len());
    let _ = writeln!(output, "Keystrokes: {}", record.log.keystrokes.len());
    let _ = writeln!(output, "Duration: {:.3} s", record.log.duration_us as f64 / 1_000_000.0);
    let _ = writeln!(output, "Net WPM: {:.1}", metrics.net_wpm);
    let _ = writeln!(output, "Raw WPM: {:.1}", metrics.raw_wpm);
    let _ = writeln!(output, "Accuracy: {:.1}%", metrics.accuracy * 100.0);
    let _ = writeln!(output, "Median IKI: {:.1} ms", metrics.iki_median.as_secs_f64() * 1_000.0);
    let _ = writeln!(output, "Consistency: {:.1}%", metrics.consistency * 100.0);
    output
}
