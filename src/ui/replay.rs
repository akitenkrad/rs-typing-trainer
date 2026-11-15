//! Replay overlay for the ordinary playing screen.

use ratatui::Frame;
use ratatui::text::Line;

use crate::i18n::Lang;
use crate::replay::{PlaybackTiming, Replay};

pub fn render(frame: &mut Frame, replay: &Replay, lang: Lang) {
    crate::ui::playing::render_with_footer(frame, replay.app(), Some(status_line(replay, lang)));
}

fn status_line(replay: &Replay, lang: Lang) -> Line<'static> {
    if replay.is_finished() {
        let metrics = replay.metrics();
        return Line::from(match lang {
            Lang::En => format!(
                "Replay finished · WPM {:.1} · Accuracy {:.1}% · [Esc/Q] quit",
                metrics.net_wpm,
                metrics.accuracy * 100.0
            ),
            Lang::Ja => format!(
                "リプレイ終了 · WPM {:.1} · 正確さ {:.1}% · [Esc/Q] 終了",
                metrics.net_wpm,
                metrics.accuracy * 100.0
            ),
        });
    }

    let timing = match replay.timing() {
        PlaybackTiming::Recorded { speed } => format!("{speed}x"),
        PlaybackTiming::Fixed { interval } => format!("{} ms", interval.as_millis()),
    };
    Line::from(match lang {
        Lang::En => format!(
            "Replay · {}/{} · {timing} · [Space] pause/play [→] step [Esc/Q] quit",
            replay.applied_keystrokes(),
            replay.record().log.keystrokes.len()
        ),
        Lang::Ja => format!(
            "リプレイ · {}/{} · {timing} · [Space] 再生・停止 [→] 1 打鍵進む [Esc/Q] 終了",
            replay.applied_keystrokes(),
            replay.record().log.keystrokes.len()
        ),
    })
}
