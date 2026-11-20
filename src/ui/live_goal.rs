//! The live goal on screen (MYTASK-3633, MYTASK-3641): a meter with the
//! target line while typing, and the headline of the result screen. A menu
//! step and a session started from the table draw it the same way; the
//! result of a table cell compares with the cell's previous session and its
//! personal best instead of the menu's «3 in a row».
//!
//! The numbers come from `practice::menu::live_goal`; this module only lays
//! them out. Every `match` on [`GoalKind`] is exhaustive, so a new goal kind
//! cannot be added without deciding how it looks.

use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::analysis::grades::Direction as Better;
use crate::app::{App, ResultOrigin};
use crate::i18n::Lang;
use crate::practice::menu::PACER_ACCURACY_FLOOR;
use crate::practice::menu::live_goal::{
    GoalKind, GoalMeasure, GoalOrigin, LiveGoal, LiveGoalState, RECENT_INTERVALS, Tone, histogram, mistype_slots,
};
use crate::ui::result_image::{MetricCardSpec, ResultImageColors};
use crate::ui::theme::Theme;

/// The live goal while a session with one is being played: a menu step or a
/// table cell whose row has a goal. `None` for benchmarks, the stand-alone
/// warm-up, the «normal» row and the warm-up and check steps.
pub fn playing_goal(app: &App) -> Option<&LiveGoalState> {
    if app.active_bench_id.is_some() || app.game.is_none() {
        return None;
    }
    app.menu_live_goal.as_ref()
}

/// The goal of the session whose result is on screen, once its measurement
/// is final.
pub fn result_goal(app: &App) -> Option<(&LiveGoalState, &GoalMeasure)> {
    if app.result_origin != ResultOrigin::JustPlayed || app.active_bench_id.is_some() {
        return None;
    }
    let state = app.menu_live_goal.as_ref()?;
    if state.goal.origin == GoalOrigin::Menu && app.menu_active_step.is_none() {
        return None;
    }
    Some((state, state.finished.as_ref()?))
}

/// The main value of a goal as the meter and the headline show it.
pub fn format_value(kind: GoalKind, value: f64) -> String {
    match kind {
        GoalKind::AccuracyGate | GoalKind::Blind => percent(value),
        GoalKind::Anticipation | GoalKind::Spelling => format!("{:.0}%", value * 100.0),
        GoalKind::Metronome => format!("{value:.2}"),
        GoalKind::Pacer | GoalKind::Sprint => format!("{value:.1}"),
        GoalKind::EnduranceSplit => format!("{:+.1}%", value * 100.0),
        GoalKind::Recovery => format!("{value:+.0}"),
        GoalKind::Preview | GoalKind::Reach => format!("{value:.2}"),
        GoalKind::Overlap => format!("{value:.0}%"),
        GoalKind::WeakTransition => format!("{value:.0}"),
    }
}

/// The size of a difference between two values of a goal.
fn format_delta(kind: GoalKind, delta: f64) -> String {
    let delta = delta.abs();
    match kind {
        GoalKind::AccuracyGate | GoalKind::Blind | GoalKind::EnduranceSplit => format!("{:.1}pt", delta * 100.0),
        GoalKind::Anticipation | GoalKind::Spelling => format!("{:.0}pt", delta * 100.0),
        GoalKind::Metronome | GoalKind::Preview | GoalKind::Reach => format!("{delta:.2}"),
        GoalKind::Pacer | GoalKind::Sprint => format!("{delta:.1} WPM"),
        GoalKind::Recovery | GoalKind::WeakTransition => format!("{delta:.0} ms"),
        GoalKind::Overlap => format!("{delta:.0}pt"),
    }
}

/// The keyboard display of the no-look row as the player sees it.
fn keyboard_state(app: &App, ja: bool) -> &'static str {
    match (app.config.session.blind, app.config.keyboard.show.as_str()) {
        (true, _) if ja => "なし（補助なし）",
        (true, _) => "off (no assistance)",
        (false, "hidden") if ja => "非表示",
        (false, "hidden") => "hidden",
        (false, "auto") if ja => "だんだん消える",
        (false, "auto") => "fading out",
        (false, _) if ja => "表示中",
        (false, _) => "shown",
    }
}

fn tone_color(tone: Tone, theme: &Theme) -> Color {
    match tone {
        Tone::Good => theme.correct,
        Tone::Warning => theme.warning,
        Tone::Bad => theme.mistype,
        Tone::Neutral => theme.accent,
    }
}

fn percent(value: f64) -> String {
    format!("{:.1}%", value * 100.0)
}

fn clock(duration: Duration) -> String {
    format!("{}:{:02}", duration.as_secs() / 60, duration.as_secs() % 60)
}

/// The label, value text and target text of the meter.
struct MeterText {
    label: String,
    value: String,
    target: String,
}

fn meter_text(goal: &LiveGoal, measure: &GoalMeasure, elapsed: Duration, lang: Lang) -> MeterText {
    let ja = lang == Lang::Ja;
    let value = measure.value;
    match goal.kind {
        GoalKind::AccuracyGate => MeterText {
            label: if ja { "正確さ" } else { "Accuracy" }.to_owned(),
            value: value.map_or_else(|| "--.-%".to_owned(), percent),
            target: goal.target.map_or_else(String::new, |gate| {
                if ja {
                    format!("関門 {:.0}%", gate * 100.0)
                } else {
                    format!("gate {:.0}%", gate * 100.0)
                }
            }),
        },
        GoalKind::Anticipation => MeterText {
            label: if ja { "先走り" } else { "Anticipation" }.to_owned(),
            value: value.map_or_else(|| "--%".to_owned(), |value| format!("{:.0}%", value * 100.0)),
            target: match goal.target {
                Some(median) if ja => format!("中央値 {:.0}%", median * 100.0),
                Some(median) => format!("median {:.0}%", median * 100.0),
                None if ja => "基準なし".to_owned(),
                None => "no baseline".to_owned(),
            },
        },
        GoalKind::Metronome => MeterText {
            label: if ja { "語中CV" } else { "In-word CV" }.to_owned(),
            value: value.map_or_else(|| "-.--".to_owned(), |value| format!("{value:.2}")),
            target: match goal.target {
                Some(median) if ja => format!("中央値 {median:.2}"),
                Some(median) => format!("median {median:.2}"),
                None if ja => "基準なし".to_owned(),
                None => "no baseline".to_owned(),
            },
        },
        GoalKind::Pacer => {
            let gap = goal.target.zip(value).map(|(target, value)| value - target);
            MeterText {
                label: if ja { "速さ" } else { "Speed" }.to_owned(),
                value: value.map_or_else(|| "--.- WPM".to_owned(), |value| format!("{value:.1} WPM")),
                target: match (goal.target, gap) {
                    (Some(_), Some(gap)) if gap >= 0.0 && ja => format!("ペーサーより {gap:.1} 前"),
                    (Some(_), Some(gap)) if ja => format!("ペーサーより {:.1} 後ろ", -gap),
                    (Some(_), Some(gap)) if gap >= 0.0 => format!("{gap:.1} ahead of pacer"),
                    (Some(_), Some(gap)) => format!("{:.1} behind pacer", -gap),
                    (Some(target), None) if ja => format!("ペーサー {target:.0}"),
                    (Some(target), None) => format!("pacer {target:.0}"),
                    (None, _) => String::new(),
                },
            }
        }
        GoalKind::EnduranceSplit => {
            let second_half = goal.in_second_half(elapsed);
            MeterText {
                label: if ja { "後半の遅れ" } else { "Slowdown" }.to_owned(),
                value: match value {
                    Some(value) if second_half => format!("{:+.1}%", value * 100.0),
                    _ if ja => "前半: 基準を測定中".to_owned(),
                    _ => "1st half: measuring".to_owned(),
                },
                target: goal.target.map_or_else(String::new, |threshold| {
                    if ja {
                        format!("{:.0}% 未満", threshold * 100.0)
                    } else {
                        format!("under {:.0}%", threshold * 100.0)
                    }
                }),
            }
        }
        GoalKind::Sprint => MeterText {
            label: if ja {
                format!("{}/3 本目", goal.run_number())
            } else {
                format!("Sprint {}/3", goal.run_number())
            },
            value: value.map_or_else(|| "--.- WPM".to_owned(), |value| format!("{value:.1} WPM")),
            target: match goal.target {
                Some(best) if ja => format!("最速 {best:.1}"),
                Some(best) => format!("best {best:.1}"),
                None if ja => "1 本目: 基準を作る".to_owned(),
                None => "1st: set the mark".to_owned(),
            },
        },
        GoalKind::Recovery => MeterText {
            label: if ja { "誤打後の余分" } else { "After-error excess" }.to_owned(),
            value: value.map_or_else(|| "--- ms".to_owned(), |value| format!("{value:+.0} ms")),
            target: median_text(goal.target.map(|median| format!("{median:+.0} ms")), ja),
        },
        GoalKind::Blind => MeterText {
            label: if ja { "正確さ" } else { "Accuracy" }.to_owned(),
            value: value.map_or_else(|| "--.-%".to_owned(), percent),
            target: median_text(goal.target.map(percent), ja),
        },
        GoalKind::Preview => MeterText {
            label: if ja { "語頭の遅れ" } else { "Word onset" }.to_owned(),
            value: value.map_or_else(|| "-.--".to_owned(), |value| format!("{value:.2}")),
            target: median_text(goal.target.map(|median| format!("{median:.2}")), ja),
        },
        GoalKind::Overlap => MeterText {
            label: if ja { "ロールオーバー" } else { "Rollover" }.to_owned(),
            value: value.map_or_else(|| "--%".to_owned(), |value| format!("{value:.0}%")),
            target: median_text(goal.target.map(|median| format!("{median:.0}%")), ja),
        },
        GoalKind::Spelling => MeterText {
            label: if ja { "綴りの使用率" } else { "Spelling use" }.to_owned(),
            value: value.map_or_else(|| "--%".to_owned(), |value| format!("{:.0}%", value * 100.0)),
            target: match (goal.spelling.as_ref(), goal.target) {
                (None, _) if ja => "勧める綴りなし".to_owned(),
                (None, _) => "no suggestion".to_owned(),
                (Some(_), Some(threshold)) if ja => format!("{:.0}% 以上", threshold * 100.0),
                (Some(_), Some(threshold)) => format!("{:.0}%+", threshold * 100.0),
                (Some(_), None) => String::new(),
            },
        },
        GoalKind::WeakTransition => MeterText {
            label: if ja { "対象遷移" } else { "Targets" }.to_owned(),
            value: value.map_or_else(|| "--- ms".to_owned(), |value| format!("{value:.0} ms")),
            target: match goal.line(measure) {
                Some(line) if ja => format!("基準 {line:.0} ms"),
                Some(line) => format!("estimate {line:.0} ms"),
                None if ja => "基準なし".to_owned(),
                None => "no estimate".to_owned(),
            },
        },
        GoalKind::Reach => MeterText {
            label: if ja { "遠い段/ホーム" } else { "Far/home IKI" }.to_owned(),
            value: value.map_or_else(|| "-.--".to_owned(), |value| format!("{value:.2}")),
            target: median_text(goal.target.map(|median| format!("{median:.2}")), ja),
        },
    }
}

/// «中央値 x» / «median x», or the missing baseline.
fn median_text(median: Option<String>, ja: bool) -> String {
    match median {
        Some(median) if ja => format!("中央値 {median}"),
        Some(median) => format!("median {median}"),
        None if ja => "基準なし".to_owned(),
        None => "no baseline".to_owned(),
    }
}

/// A meter bar of `cells` cells: filled up to `value`, with the target line.
fn bar_spans(value: Option<f64>, range: (f64, f64), target: Option<f64>, cells: usize, fill: Color, theme: &Theme) -> Vec<Span<'static>> {
    let (low, high) = range;
    let span = (high - low).max(f64::EPSILON);
    let position = |value: f64| (((value - low) / span) * cells as f64).clamp(0.0, cells as f64);
    let filled = value.map_or(0, |value| position(value).round() as usize);
    let line = target.map(|target| (position(target).floor() as usize).min(cells.saturating_sub(1)));
    (0..cells)
        .map(|index| {
            if Some(index) == line {
                Span::styled("┃", Style::default().fg(theme.fg).add_modifier(Modifier::BOLD))
            } else if index < filled {
                Span::styled("█", Style::default().fg(fill))
            } else {
                Span::styled("░", Style::default().fg(theme.muted))
            }
        })
        .collect()
}

/// One meter line `label bar value target` that fits `width` columns.
fn meter_line(
    text: &MeterText,
    value: Option<f64>,
    range: (f64, f64),
    target: Option<f64>,
    tone: Tone,
    width: usize,
    theme: &Theme,
) -> Line<'static> {
    let color = tone_color(tone, theme);
    let label = format!("{} ", text.label);
    let value_text = format!(" {}", text.value);
    let mut target_text = if text.target.is_empty() {
        String::new()
    } else {
        format!("  {}", text.target)
    };
    let fixed = |target: &str| UnicodeWidthStr::width(label.as_str()) + UnicodeWidthStr::width(value_text.as_str()) + UnicodeWidthStr::width(target);
    if width.saturating_sub(fixed(&target_text)) < 8 {
        target_text.clear();
    }
    let cells = width.saturating_sub(fixed(&target_text)).clamp(4, 40);
    let mut spans = vec![Span::styled(label, Style::default().fg(theme.muted))];
    spans.extend(bar_spans(value, range, target, cells, color, theme));
    spans.push(Span::styled(value_text, Style::default().fg(color).add_modifier(Modifier::BOLD)));
    spans.push(Span::styled(target_text, Style::default().fg(theme.muted)));
    Line::from(spans)
}

/// The meter row of the playing screen.
pub fn render_meter(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let (Some(state), Some(game)) = (playing_goal(app), app.game.as_ref()) else {
        return;
    };
    let goal = &state.goal;
    let measure = &state.measure;
    let text = meter_text(goal, measure, game.elapsed, app.config.general.lang);
    let value = match goal.kind {
        GoalKind::EnduranceSplit if !goal.in_second_half(game.elapsed) => None,
        _ => measure.value,
    };
    let tone = if value.is_none() { Tone::Neutral } else { goal.tone(measure) };
    let line = meter_line(
        &text,
        value,
        goal.meter_range(measure),
        goal.line(measure),
        tone,
        usize::from(area.width.saturating_sub(2)),
        theme,
    );
    frame.render_widget(Paragraph::new(line).alignment(Alignment::Center), area);
}

/// Recent within-word intervals as a band of block characters, scaled to
/// twice the median interval (a flat band is an even rhythm).
fn interval_band(intervals: &[f64]) -> String {
    const LEVELS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let recent = &intervals[intervals.len().saturating_sub(RECENT_INTERVALS)..];
    let mut sorted = recent.to_vec();
    sorted.sort_by(f64::total_cmp);
    let median = sorted.get(sorted.len() / 2).copied().unwrap_or(0.0).max(1.0);
    recent
        .iter()
        .map(|value| {
            let level = ((value / (median * 2.0)) * (LEVELS.len() - 1) as f64)
                .round()
                .clamp(0.0, (LEVELS.len() - 1) as f64);
            LEVELS[level as usize]
        })
        .collect()
}

/// The line under the meter; it replaces the combo/errors/accuracy row.
pub fn render_note(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let (Some(state), Some(game)) = (playing_goal(app), app.game.as_ref()) else {
        return;
    };
    let ja = app.config.general.lang == Lang::Ja;
    let goal = &state.goal;
    let measure = &state.measure;
    let muted = Style::default().fg(theme.muted);
    let flash = Style::default().fg(theme.warning).add_modifier(Modifier::BOLD);
    let mistypes = game.metrics.mistypes_total;
    let mut spans: Vec<Span<'static>> = Vec::new();
    match goal.kind {
        GoalKind::AccuracyGate => {
            spans.push(Span::styled(if ja { "連続 " } else { "Streak " }, muted));
            spans.push(Span::styled(
                format!("{}", measure.streak),
                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                if ja {
                    format!(" 打（最長 {}）    誤打 {mistypes}", measure.longest_streak)
                } else {
                    format!(" keys (longest {})    Errors {mistypes}", measure.longest_streak)
                },
                muted,
            ));
            if let (Some(gate), Some(value)) = (goal.target, measure.value) {
                let margin = (value - gate) * 100.0;
                let tone = goal.tone(measure);
                spans.push(Span::styled(
                    if ja {
                        format!("    関門まで {margin:+.1}pt")
                    } else {
                        format!("    gate margin {margin:+.1}pt")
                    },
                    Style::default().fg(tone_color(tone, theme)),
                ));
            }
            if state.record_flash(game.elapsed) {
                spans.push(Span::styled(
                    if ja {
                        "    ★ 連続記録更新!"
                    } else {
                        "    ★ New longest streak!"
                    },
                    flash,
                ));
            }
        }
        GoalKind::Anticipation => {
            spans.push(Span::styled(
                if ja {
                    format!(
                        "先走り {} / 最初の誤打 {}    誤打 {mistypes}",
                        measure.anticipations, measure.first_errors
                    )
                } else {
                    format!(
                        "Anticipation {} / first errors {}    Errors {mistypes}",
                        measure.anticipations, measure.first_errors
                    )
                },
                muted,
            ));
            if state.anticipation_flash(game.elapsed) {
                spans.push(Span::styled(
                    if ja { "    先走り!" } else { "    Anticipated!" },
                    Style::default().fg(theme.mistype).add_modifier(Modifier::BOLD),
                ));
            }
        }
        GoalKind::Metronome => {
            spans.push(Span::styled(if ja { "直近の間隔 " } else { "Recent intervals " }, muted));
            let band = interval_band(&measure.intervals);
            if band.is_empty() {
                spans.push(Span::styled(if ja { "（まだありません）" } else { "(none yet)" }, muted));
            } else {
                spans.push(Span::styled(band, Style::default().fg(tone_color(goal.tone(measure), theme))));
            }
        }
        GoalKind::Pacer => {
            let accuracy = (measure.keystrokes > 0).then_some(measure.accuracy);
            let tone = match accuracy {
                Some(value) if value >= PACER_ACCURACY_FLOOR => Tone::Good,
                Some(_) => Tone::Bad,
                None => Tone::Neutral,
            };
            let text = MeterText {
                label: if ja { "正確さ" } else { "Accuracy" }.to_owned(),
                value: accuracy.map_or_else(|| "--.-%".to_owned(), percent),
                target: if ja {
                    format!("{:.0}% 以上", PACER_ACCURACY_FLOOR * 100.0)
                } else {
                    format!("{:.0}%+", PACER_ACCURACY_FLOOR * 100.0)
                },
            };
            let line = meter_line(
                &text,
                accuracy,
                (0.85, 1.0),
                Some(PACER_ACCURACY_FLOOR),
                tone,
                usize::from(area.width.saturating_sub(2)),
                theme,
            );
            frame.render_widget(Paragraph::new(line).alignment(Alignment::Center), area);
            return;
        }
        GoalKind::EnduranceSplit => {
            let planned = Duration::from_micros(goal.planned_us);
            if goal.in_second_half(game.elapsed) {
                let comparison = measure.endurance.unwrap_or_default();
                spans.push(Span::styled(
                    if ja {
                        format!("前半 IKI {:.0} ms → 後半 IKI {:.0} ms", comparison.first_iki_ms, comparison.second_iki_ms)
                    } else {
                        format!(
                            "1st-half IKI {:.0} ms → 2nd-half IKI {:.0} ms",
                            comparison.first_iki_ms, comparison.second_iki_ms
                        )
                    },
                    muted,
                ));
            } else {
                let left = (planned / 2).saturating_sub(game.elapsed);
                spans.push(Span::styled(
                    if ja {
                        format!("後半まで {}    前半の速さが基準になります", clock(left))
                    } else {
                        format!("Second half in {}    the first half sets the baseline", clock(left))
                    },
                    muted,
                ));
            }
        }
        GoalKind::Sprint => {
            spans.push(Span::styled(
                if ja {
                    format!("語 {} / {}", game.metrics.completed_words, app.config.burst.words)
                } else {
                    format!("Words {} / {}", game.metrics.completed_words, app.config.burst.words)
                },
                muted,
            ));
            if state.record_flash(game.elapsed) {
                spans.push(Span::styled(if ja { "    ★ 最速ペース!" } else { "    ★ Fastest pace!" }, flash));
            }
        }
        GoalKind::Recovery => {
            let errors = measure.errors;
            match measure.last_recovery_ms {
                Some(extra) => {
                    let style = if state.event_flash(game.elapsed) {
                        flash
                    } else {
                        Style::default().fg(theme.accent)
                    };
                    spans.push(Span::styled(
                        if ja {
                            format!("復帰 {:+.2} s", extra / 1_000.0)
                        } else {
                            format!("Recovery {:+.2} s", extra / 1_000.0)
                        },
                        style,
                    ));
                    spans.push(Span::styled(
                        if ja {
                            format!("    誤打 {errors}（測れた {}）", measure.recoveries)
                        } else {
                            format!("    Errors {errors} ({} measured)", measure.recoveries)
                        },
                        muted,
                    ));
                }
                None => spans.push(Span::styled(
                    if ja {
                        format!("誤打 {errors}    誤打のあと元の速さへ戻るまでを測ります")
                    } else {
                        format!("Errors {errors}    timing the way back to your pace after an error")
                    },
                    muted,
                )),
            }
        }
        GoalKind::Blind => {
            spans.push(Span::styled(if ja { "キーボード表示: " } else { "Keyboard: " }, muted));
            spans.push(Span::styled(
                keyboard_state(app, ja),
                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                if ja {
                    format!("    連続 {} 打    誤打 {mistypes}", measure.streak)
                } else {
                    format!("    Streak {}    Errors {mistypes}", measure.streak)
                },
                muted,
            ));
            if state.record_flash(game.elapsed) {
                spans.push(Span::styled(
                    if ja {
                        "    ★ 連続記録更新!"
                    } else {
                        "    ★ New longest streak!"
                    },
                    flash,
                ));
            }
        }
        GoalKind::Preview => {
            spans.push(Span::styled(if ja { "止まらず " } else { "No-stop run " }, muted));
            spans.push(Span::styled(
                format!("{}", measure.onset_run),
                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                if ja {
                    format!(" 語（最長 {}）    語頭 {}", measure.longest_onset_run, measure.words_started)
                } else {
                    format!(" words (longest {})    Word starts {}", measure.longest_onset_run, measure.words_started)
                },
                muted,
            ));
            if state.record_flash(game.elapsed) {
                spans.push(Span::styled(if ja { "    ★ 最長更新!" } else { "    ★ Longest run!" }, flash));
            }
        }
        GoalKind::Overlap => {
            spans.push(Span::styled(
                if !app.release_events_supported {
                    if ja {
                        "キーを離した時刻が取れない端末です（計測できません）".to_owned()
                    } else {
                        "This terminal does not report key releases (cannot measure)".to_owned()
                    }
                } else if ja {
                    format!(
                        "重なった打鍵 {} / {} 組    誤打 {mistypes}",
                        measure.overlap_pairs, measure.comparable_pairs
                    )
                } else {
                    format!(
                        "Overlapping pairs {} / {}    Errors {mistypes}",
                        measure.overlap_pairs, measure.comparable_pairs
                    )
                },
                muted,
            ));
        }
        GoalKind::Spelling => match goal.spelling.as_ref() {
            Some(target) => {
                spans.push(Span::styled(
                    target.label(),
                    Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::styled(
                    if ja {
                        format!("    使用 {} / {} 回", measure.spelling_used, measure.spelling_total)
                    } else {
                        format!("    used {} of {}", measure.spelling_used, measure.spelling_total)
                    },
                    muted,
                ));
            }
            None => spans.push(Span::styled(
                if ja {
                    "勧める綴りがまだありません（日本語の記録が足りません）"
                } else {
                    "No suggested spelling yet (not enough Japanese sessions)"
                },
                muted,
            )),
        },
        GoalKind::WeakTransition => match measure.last_hit.as_ref() {
            Some(hit) => {
                let faster = hit.prior_ms.map(|prior| hit.ms <= prior);
                let color = match faster {
                    Some(true) => theme.correct,
                    Some(false) => theme.mistype,
                    None => theme.accent,
                };
                let mut style = Style::default().fg(color);
                if state.event_flash(game.elapsed) {
                    style = style.add_modifier(Modifier::BOLD);
                }
                let verdict = match (faster, ja) {
                    (Some(true), true) => " 速い ▲",
                    (Some(false), true) => " 遅い ▼",
                    (Some(true), false) => " faster ▲",
                    (Some(false), false) => " slower ▼",
                    (None, _) => "",
                };
                spans.push(Span::styled(format!("{} {:.0} ms{verdict}", hit.label, hit.ms), style));
                if let Some(prior) = hit.prior_ms {
                    spans.push(Span::styled(
                        if ja {
                            format!("（基準 {prior:.0}）")
                        } else {
                            format!(" (estimate {prior:.0})")
                        },
                        muted,
                    ));
                }
                spans.push(Span::styled(
                    if ja {
                        format!("    対象 {} 回", measure.transition_hits)
                    } else {
                        format!("    Targets typed {}", measure.transition_hits)
                    },
                    muted,
                ));
            }
            None => {
                let labels = goal.transitions.iter().map(|transition| transition.label()).collect::<Vec<_>>().join(" ");
                spans.push(Span::styled(
                    match (labels.is_empty(), ja) {
                        (true, true) => "対象の遷移がありません".to_owned(),
                        (true, false) => "No target transitions".to_owned(),
                        (false, true) => format!("対象: {labels}"),
                        (false, false) => format!("Targets: {labels}"),
                    },
                    muted,
                ));
            }
        },
        GoalKind::Reach => {
            spans.push(Span::styled(
                if ja {
                    format!("遠い段（数字・上・下）の間隔をホーム段と比べます    誤打 {mistypes}")
                } else {
                    format!("Remote rows (number, top, bottom) against the home row    Errors {mistypes}")
                },
                muted,
            ));
        }
    }
    frame.render_widget(Paragraph::new(Line::from(spans)).alignment(Alignment::Center), area);
}

/// The headline of the result screen.
pub struct GoalReport {
    pub title: String,
    pub label: String,
    /// The main value, drawn large.
    pub value: String,
    pub tone: Tone,
    /// Lines beside the large value (verdict first).
    pub details: Vec<Line<'static>>,
    /// Full-width lines under it (time line, histogram, sprint bars).
    pub chart: Vec<Line<'static>>,
    /// The cell's earlier values and this one, oldest first (a table cell;
    /// empty for a menu step). The image card draws them as a sparkline.
    pub recent: Vec<f64>,
}

impl GoalReport {
    /// Rows the bordered headline needs.
    pub fn height(&self) -> u16 {
        2 + 5u16.max(self.details.len() as u16 + 1) + self.chart.len() as u16
    }
}

fn verdict_line(met: Option<bool>, yes: String, no: String, unknown: String, theme: &Theme) -> Line<'static> {
    match met {
        Some(true) => Line::from(Span::styled(
            format!("✓ {yes}"),
            Style::default().fg(theme.correct).add_modifier(Modifier::BOLD),
        )),
        Some(false) => Line::from(Span::styled(
            format!("✗ {no}"),
            Style::default().fg(theme.mistype).add_modifier(Modifier::BOLD),
        )),
        None => Line::from(Span::styled(unknown, Style::default().fg(theme.muted))),
    }
}

/// What the result says about progress: «3 回連続: 2/3» for a menu step,
/// the difference from the cell's previous session and personal best for a
/// table cell.
fn progress_lines(app: &App, goal: &LiveGoal, measure: &GoalMeasure, lang: Lang, theme: &Theme) -> Vec<Line<'static>> {
    match goal.origin {
        GoalOrigin::Menu => streak_line(app, lang, theme).into_iter().collect(),
        GoalOrigin::Drill(_) => comparison_lines(goal, measure, lang, theme),
    }
}

/// The value against the cell's previous session and its best one. ▲ is
/// better and ▼ worse in the goal's own direction.
pub fn comparison_lines(goal: &LiveGoal, measure: &GoalMeasure, lang: Lang, theme: &Theme) -> Vec<Line<'static>> {
    let ja = lang == Lang::Ja;
    let Some(value) = measure.value else {
        return Vec::new();
    };
    let (Some(previous), Some(best)) = (goal.previous_value(), goal.best_value()) else {
        return vec![muted(
            if ja {
                "この升目の初回です（比べる回はまだありません）".to_owned()
            } else {
                "First session of this cell (nothing to compare yet)".to_owned()
            },
            theme,
        )];
    };
    let kind = goal.kind;
    let change = |label: &str, other: f64| -> Line<'static> {
        let (arrow, word, color) = if kind.better(value, other) {
            ("▲", if ja { "改善" } else { "better" }, theme.correct)
        } else if kind.better(other, value) {
            ("▼", if ja { "後退" } else { "worse" }, theme.mistype)
        } else {
            ("＝", if ja { "同じ" } else { "same" }, theme.muted)
        };
        Line::from(vec![
            Span::styled(format!("{label} {}  ", format_value(kind, other)), Style::default().fg(theme.fg)),
            Span::styled(
                format!("{arrow} {} {word}", format_delta(kind, value - other)),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ])
    };
    let mut lines = vec![change(if ja { "前回" } else { "Previous" }, previous)];
    if kind.better(value, best) {
        lines.push(Line::from(Span::styled(
            if ja {
                format!("★ 自己ベスト更新（これまで {}）", format_value(kind, best))
            } else {
                format!("★ New personal best (was {})", format_value(kind, best))
            },
            Style::default().fg(theme.warning).add_modifier(Modifier::BOLD),
        )));
    } else {
        lines.push(change(if ja { "自己ベスト" } else { "Best" }, best));
    }
    lines
}

/// The note that a check-step metric is judged there (menu steps only).
fn judged_on_check(goal: &LiveGoal, ja: bool, theme: &Theme) -> Option<Line<'static>> {
    (goal.origin == GoalOrigin::Menu).then(|| {
        muted(
            if ja {
                "卒業の判定は確認段の値で行います".to_owned()
            } else {
                "Graduation is judged on the check step".to_owned()
            },
            theme,
        )
    })
}

/// The missing own median: a menu step lacks check steps, a table cell
/// lacks both its own sessions and check steps.
fn no_baseline(goal: &LiveGoal, ja: bool, theme: &Theme) -> Line<'static> {
    muted(
        match (goal.origin, ja) {
            (GoalOrigin::Menu, true) => "基準なし（確認段の履歴が足りません）",
            (GoalOrigin::Menu, false) => "No baseline yet (not enough check steps)",
            (GoalOrigin::Drill(_), true) => "基準なし（この升目の記録も確認段もありません）",
            (GoalOrigin::Drill(_), false) => "No baseline yet (no sessions of this cell or check steps)",
        }
        .to_owned(),
        theme,
    )
}

/// «自分の中央値 x 以下/以上» against the own median of an own-median row.
fn median_verdict(goal: &LiveGoal, measure: &GoalMeasure, shown: String, unknown: &str, ja: bool, theme: &Theme) -> Line<'static> {
    let source = match (goal.baseline_runs, ja) {
        (Some(runs), true) => format!("（この升目の直近 {runs} 回）"),
        (Some(runs), false) => format!(" (last {runs} of this cell)"),
        (None, _) => String::new(),
    };
    let (good, bad) = match (goal.kind.direction(), ja) {
        (Better::Lower, true) => ("以下", "を上回った"),
        (Better::Lower, false) => ("At or below", "Above"),
        (Better::Higher, true) => ("以上", "を下回った"),
        (Better::Higher, false) => ("At or above", "Below"),
    };
    verdict_line(
        measure.met,
        if ja {
            format!("自分の中央値 {shown}{source} {good}")
        } else {
            format!("{good} your median {shown}{source}")
        },
        if ja {
            format!("自分の中央値 {shown}{source} {bad}")
        } else {
            format!("{bad} your median {shown}{source}")
        },
        unknown.to_owned(),
        theme,
    )
}

/// «3 回連続: 2/3» from the recorded progress of the step.
fn streak_line(app: &App, lang: Lang, theme: &Theme) -> Option<Line<'static>> {
    use crate::practice::menu::Graduation;

    let progress = app.menu_step_progress?;
    let text = match (progress.graduation, lang) {
        (Some(Graduation::Level(level)), Lang::Ja) => format!("卒業 → レベル {level}"),
        (Some(Graduation::Level(level)), Lang::En) => format!("Graduated → level {level}"),
        (Some(Graduation::Maintenance), Lang::Ja) => "卒業 → 維持".to_owned(),
        (Some(Graduation::Maintenance), Lang::En) => "Graduated → maintenance".to_owned(),
        (None, Lang::Ja) => format!("{} 回連続: {}/{}", progress.required, progress.consecutive, progress.required),
        (None, Lang::En) => format!("{} in a row: {}/{}", progress.required, progress.consecutive, progress.required),
    };
    Some(Line::from(Span::styled(text, Style::default().fg(theme.accent))))
}

fn plain(text: String, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(text, Style::default().fg(theme.fg)))
}

fn muted(text: String, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(text, Style::default().fg(theme.muted)))
}

/// The result headline of the finished step, laid out for `width` columns.
pub fn result_report(app: &App, width: u16, theme: &Theme) -> Option<GoalReport> {
    let (state, measure) = result_goal(app)?;
    let lang = app.config.general.lang;
    let ja = lang == Lang::Ja;
    let goal = &state.goal;
    let title = match goal.origin {
        GoalOrigin::Menu => {
            let step = app.daily_menu.as_ref()?.steps.get(app.menu_active_step?)?;
            let level = step.level.map(|level| format!(" Lv{level}")).unwrap_or_default();
            format!(" {}{level} ", step.title)
        }
        GoalOrigin::Drill(cell) => format!(" {} ", crate::ui::mode_select::cell_label(cell, lang)),
    };
    let tone = goal.tone(measure);
    // The chart is drawn inside the headline's border and side margins.
    let chart_width = usize::from(width.saturating_sub(6));
    let mut details = Vec::new();
    let mut chart = Vec::new();
    let (label, value) = match goal.kind {
        GoalKind::AccuracyGate => {
            let gate = goal.target.unwrap_or(0.0) * 100.0;
            details.push(verdict_line(
                measure.met,
                if ja {
                    format!("関門 {gate:.0}% を通過")
                } else {
                    format!("Passed the {gate:.0}% gate")
                },
                if ja {
                    format!("関門 {gate:.0}% に届かず")
                } else {
                    format!("Below the {gate:.0}% gate")
                },
                if ja {
                    "判定できません".to_owned()
                } else {
                    "Cannot be judged".to_owned()
                },
                theme,
            ));
            details.extend(progress_lines(app, goal, measure, lang, theme));
            details.push(plain(
                if ja {
                    format!("最長の連続 {} 打", measure.longest_streak)
                } else {
                    format!("Longest streak {} keys", measure.longest_streak)
                },
                theme,
            ));
            if let Some(record) = app.result_record.as_ref() {
                let errors = record.header.mistypes_total;
                details.push(plain(if ja { format!("誤打 {errors}") } else { format!("Errors {errors}") }, theme));
                let prefix = if ja { "誤打の位置 " } else { "Errors in time " };
                let slots = chart_width.saturating_sub(UnicodeWidthStr::width(prefix) + 2).max(8);
                let marks = mistype_slots(record, slots);
                let mut spans = vec![Span::styled(prefix, Style::default().fg(theme.muted)), Span::raw("│")];
                spans.extend(marks.iter().map(|mark| {
                    if *mark {
                        Span::styled("×", Style::default().fg(theme.mistype).add_modifier(Modifier::BOLD))
                    } else {
                        Span::styled("·", Style::default().fg(theme.muted))
                    }
                }));
                spans.push(Span::raw("│"));
                chart.push(Line::from(spans));
                let end = clock(Duration::from_micros(record.log.duration_us));
                let pad = UnicodeWidthStr::width(prefix) + 1;
                let gap = slots.saturating_sub(4 + end.len());
                chart.push(muted(format!("{}0:00{}{end}", " ".repeat(pad), " ".repeat(gap)), theme));
            }
            (
                if ja { "正確さ" } else { "Accuracy" },
                measure.value.map_or_else(|| "--".to_owned(), percent),
            )
        }
        GoalKind::Anticipation => {
            let median = goal.target;
            details.push(match median {
                Some(median) => verdict_line(
                    measure.met,
                    if ja {
                        format!("自分の中央値 {:.0}% 以下", median * 100.0)
                    } else {
                        format!("At or below your median {:.0}%", median * 100.0)
                    },
                    if ja {
                        format!("自分の中央値 {:.0}% を上回った", median * 100.0)
                    } else {
                        format!("Above your median {:.0}%", median * 100.0)
                    },
                    if ja {
                        "誤打がなく割合は出ません".to_owned()
                    } else {
                        "No errors, so no share".to_owned()
                    },
                    theme,
                ),
                None => no_baseline(goal, ja, theme),
            });
            details.push(plain(
                if ja {
                    format!("先走り {} / 最初の誤打 {}", measure.anticipations, measure.first_errors)
                } else {
                    format!("Anticipation {} of {} first errors", measure.anticipations, measure.first_errors)
                },
                theme,
            ));
            details.extend(judged_on_check(goal, ja, theme));
            details.extend(progress_lines(app, goal, measure, lang, theme));
            (
                if ja { "先走りの割合" } else { "Anticipation share" },
                measure.value.map_or_else(|| "--".to_owned(), |value| format!("{:.0}%", value * 100.0)),
            )
        }
        GoalKind::Metronome => {
            details.push(match goal.target {
                Some(median) => verdict_line(
                    measure.met,
                    if ja {
                        format!("自分の中央値 {median:.2} 以下")
                    } else {
                        format!("At or below your median {median:.2}")
                    },
                    if ja {
                        format!("自分の中央値 {median:.2} を上回った")
                    } else {
                        format!("Above your median {median:.2}")
                    },
                    if ja {
                        "間隔が足りず CV は出ません".to_owned()
                    } else {
                        "Too few intervals for a CV".to_owned()
                    },
                    theme,
                ),
                None => no_baseline(goal, ja, theme),
            });
            let mut sorted = measure.intervals.clone();
            sorted.sort_by(f64::total_cmp);
            let median_ms = sorted.get(sorted.len() / 2).copied().unwrap_or(0.0);
            details.push(plain(
                if ja {
                    format!("語中の間隔 {} 個・中央値 {median_ms:.0} ms", sorted.len())
                } else {
                    format!("{} in-word intervals, median {median_ms:.0} ms", sorted.len())
                },
                theme,
            ));
            details.extend(judged_on_check(goal, ja, theme));
            details.extend(progress_lines(app, goal, measure, lang, theme));
            if !sorted.is_empty() {
                const LEVELS: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
                let high = (median_ms * 2.0).max(1.0);
                let prefix = if ja { "間隔の分布 " } else { "Intervals " };
                let bins = chart_width.saturating_sub(UnicodeWidthStr::width(prefix) + 2).clamp(8, 24);
                let counts = histogram(&measure.intervals, 0.0, high, bins);
                let maximum = counts.iter().copied().max().unwrap_or(1).max(1);
                let color = tone_color(tone, theme);
                for row in (0..2).rev() {
                    let label = if row == 1 {
                        prefix.to_owned()
                    } else {
                        " ".repeat(UnicodeWidthStr::width(prefix))
                    };
                    let mut spans = vec![Span::styled(label, Style::default().fg(theme.muted))];
                    spans.extend(counts.iter().map(|count| {
                        let eighths = ((f64::from(*count) / f64::from(maximum)) * 16.0).round() as usize;
                        let level = eighths.saturating_sub(row * 8).min(8);
                        Span::styled(LEVELS[level], Style::default().fg(color))
                    }));
                    chart.push(Line::from(spans));
                }
                let pad = UnicodeWidthStr::width(prefix);
                let end = format!("{high:.0} ms");
                let gap = bins.saturating_sub(1 + end.len());
                chart.push(muted(format!("{}0{}{end}", " ".repeat(pad), " ".repeat(gap)), theme));
            }
            (
                if ja { "語中 IKI の CV" } else { "In-word IKI CV" },
                measure.value.map_or_else(|| "--".to_owned(), |value| format!("{value:.2}")),
            )
        }
        GoalKind::Pacer => {
            let target = goal.target;
            details.push(verdict_line(
                measure.met,
                if ja {
                    "目標に到達（速さ・正確さとも）".to_owned()
                } else {
                    "Target reached (speed and accuracy)".to_owned()
                },
                if ja {
                    "目標に届かず".to_owned()
                } else {
                    "Target not reached".to_owned()
                },
                if ja {
                    "ペーサーの目標がありません".to_owned()
                } else {
                    "No pacer target".to_owned()
                },
                theme,
            ));
            if let Some(target) = target {
                let ok = measure.net_wpm >= target;
                details.push(plain(
                    format!(
                        "{} {} {:.1} / {target:.0} WPM",
                        if ok { "✓" } else { "✗" },
                        if ja { "速さ" } else { "Speed" },
                        measure.net_wpm
                    ),
                    theme,
                ));
            }
            let ok = measure.accuracy >= PACER_ACCURACY_FLOOR;
            details.push(plain(
                format!(
                    "{} {} {} / {:.0}%+",
                    if ok { "✓" } else { "✗" },
                    if ja { "正確さ" } else { "Accuracy" },
                    percent(measure.accuracy),
                    PACER_ACCURACY_FLOOR * 100.0
                ),
                theme,
            ));
            details.extend(progress_lines(app, goal, measure, lang, theme));
            (if ja { "速さ（WPM）" } else { "Speed (WPM)" }, format!("{:.1}", measure.net_wpm))
        }
        GoalKind::EnduranceSplit => {
            let threshold = goal.target.unwrap_or(0.0) * 100.0;
            details.push(verdict_line(
                measure.met,
                if ja {
                    format!("後半の遅れ {threshold:.0}% 未満")
                } else {
                    format!("Slowdown under {threshold:.0}%")
                },
                if ja {
                    format!("後半の遅れが {threshold:.0}% 以上")
                } else {
                    format!("Slowdown {threshold:.0}% or more")
                },
                if ja {
                    "前半か後半の打鍵が足りません".to_owned()
                } else {
                    "Not enough keystrokes in a half".to_owned()
                },
                theme,
            ));
            let comparison = measure.endurance.unwrap_or_default();
            details.push(plain(
                if ja {
                    format!("IKI 前半 {:.0} ms → 後半 {:.0} ms", comparison.first_iki_ms, comparison.second_iki_ms)
                } else {
                    format!("IKI 1st {:.0} ms → 2nd {:.0} ms", comparison.first_iki_ms, comparison.second_iki_ms)
                },
                theme,
            ));
            details.push(plain(
                if ja {
                    format!(
                        "誤打率 前半 {} → 後半 {}",
                        percent(comparison.first_error_rate),
                        percent(comparison.second_error_rate)
                    )
                } else {
                    format!(
                        "Errors 1st {} → 2nd {}",
                        percent(comparison.first_error_rate),
                        percent(comparison.second_error_rate)
                    )
                },
                theme,
            ));
            details.extend(progress_lines(app, goal, measure, lang, theme));
            (
                if ja { "後半の遅れ" } else { "Second-half slowdown" },
                measure.value.map_or_else(|| "--".to_owned(), |value| format!("{:+.1}%", value * 100.0)),
            )
        }
        GoalKind::Sprint => {
            let runs = app.burst_series.runs();
            let best = app.burst_series.best();
            let latest_is_best =
                runs.len() > 1 && runs.last().is_some_and(|last| *last >= best) && goal.target.is_some_and(|previous| best > previous);
            if latest_is_best {
                details.push(Line::from(Span::styled(
                    if ja { "★ 最速更新!" } else { "★ New fastest sprint!" },
                    Style::default().fg(theme.warning).add_modifier(Modifier::BOLD),
                )));
            }
            details.push(plain(
                if ja {
                    format!("{} / 3 本", runs.len())
                } else {
                    format!("{} of 3 sprints", runs.len())
                },
                theme,
            ));
            if goal.origin == GoalOrigin::Menu {
                details.push(muted(
                    if ja {
                        "最も速い 1 本を記録します".to_owned()
                    } else {
                        "The fastest one is recorded".to_owned()
                    },
                    theme,
                ));
            } else {
                details.extend(progress_lines(app, goal, measure, lang, theme));
            }
            let best_index = runs
                .iter()
                .enumerate()
                .max_by(|left, right| left.1.total_cmp(right.1))
                .map(|(index, _)| index);
            let bar_width = chart_width.saturating_sub(24).clamp(8, 40);
            for (index, wpm) in runs.iter().enumerate() {
                let filled = ((wpm / best.max(1.0)) * bar_width as f64).round() as usize;
                let is_best = Some(index) == best_index;
                let color = if is_best { theme.correct } else { theme.accent };
                chart.push(Line::from(vec![
                    Span::styled(
                        if ja {
                            format!("{} 本目 ", index + 1)
                        } else {
                            format!("Run {} ", index + 1)
                        },
                        Style::default().fg(theme.muted),
                    ),
                    Span::styled("█".repeat(filled.min(bar_width)), Style::default().fg(color)),
                    Span::styled(
                        format!(" {wpm:.1} WPM{}", if is_best { if ja { " ★最速" } else { " ★ best" } } else { "" }),
                        Style::default().fg(if is_best { theme.correct } else { theme.fg }),
                    ),
                ]));
            }
            (if ja { "最速の 1 本（WPM）" } else { "Fastest sprint (WPM)" }, format!("{best:.1}"))
        }
        GoalKind::Recovery => {
            details.push(match goal.target {
                Some(median) => median_verdict(
                    goal,
                    measure,
                    format!("{median:+.0} ms"),
                    if ja {
                        "誤打が足りず測れません"
                    } else {
                        "Too few errors to measure"
                    },
                    ja,
                    theme,
                ),
                None => no_baseline(goal, ja, theme),
            });
            details.push(plain(
                if ja {
                    format!("誤打 {}・復帰を測れた誤打 {}", measure.errors, measure.recoveries)
                } else {
                    format!("Errors {}, recovery measured for {}", measure.errors, measure.recoveries)
                },
                theme,
            ));
            details.extend(judged_on_check(goal, ja, theme));
            details.extend(progress_lines(app, goal, measure, lang, theme));
            (
                if ja {
                    "誤打後の余分な時間（ms）"
                } else {
                    "Excess after errors (ms)"
                },
                measure.value.map_or_else(|| "--".to_owned(), |value| format_value(goal.kind, value)),
            )
        }
        GoalKind::Blind => {
            details.push(match goal.target {
                Some(median) => median_verdict(
                    goal,
                    measure,
                    percent(median),
                    if ja { "打鍵がありません" } else { "No keystrokes" },
                    ja,
                    theme,
                ),
                None => no_baseline(goal, ja, theme),
            });
            details.push(plain(
                if ja {
                    format!("キーボード表示: {}・最長の連続 {} 打", keyboard_state(app, ja), measure.longest_streak)
                } else {
                    format!("Keyboard: {}, longest streak {} keys", keyboard_state(app, ja), measure.longest_streak)
                },
                theme,
            ));
            details.extend(judged_on_check(goal, ja, theme));
            details.extend(progress_lines(app, goal, measure, lang, theme));
            (
                if ja { "正確さ（見ないで）" } else { "Accuracy (no-look)" },
                measure.value.map_or_else(|| "--".to_owned(), percent),
            )
        }
        GoalKind::Preview => {
            details.push(match goal.target {
                Some(median) => median_verdict(
                    goal,
                    measure,
                    format!("{median:.2}"),
                    if ja {
                        "語頭が足りず測れません"
                    } else {
                        "Too few word starts"
                    },
                    ja,
                    theme,
                ),
                None => no_baseline(goal, ja, theme),
            });
            details.push(plain(
                if ja {
                    format!("語頭 {}・止まらず最長 {} 語", measure.words_started, measure.longest_onset_run)
                } else {
                    format!("{} word starts, longest no-stop run {}", measure.words_started, measure.longest_onset_run)
                },
                theme,
            ));
            details.extend(judged_on_check(goal, ja, theme));
            details.extend(progress_lines(app, goal, measure, lang, theme));
            (
                if ja {
                    "語頭の遅れ（語頭/語中）"
                } else {
                    "Word onset (start/inside)"
                },
                measure.value.map_or_else(|| "--".to_owned(), |value| format_value(goal.kind, value)),
            )
        }
        GoalKind::Overlap => {
            details.push(match goal.target {
                Some(median) => median_verdict(
                    goal,
                    measure,
                    format!("{median:.0}%"),
                    if ja {
                        "キーを離した時刻がなく測れません"
                    } else {
                        "No key releases to measure"
                    },
                    ja,
                    theme,
                ),
                None => no_baseline(goal, ja, theme),
            });
            details.push(plain(
                if measure.comparable_pairs == 0 {
                    if ja {
                        "キーを離した時刻が取れない端末です".to_owned()
                    } else {
                        "This terminal does not report key releases".to_owned()
                    }
                } else if ja {
                    format!("重なった打鍵 {} / {} 組", measure.overlap_pairs, measure.comparable_pairs)
                } else {
                    format!("Overlapping pairs {} of {}", measure.overlap_pairs, measure.comparable_pairs)
                },
                theme,
            ));
            details.extend(judged_on_check(goal, ja, theme));
            details.extend(progress_lines(app, goal, measure, lang, theme));
            (
                if ja { "ロールオーバー率" } else { "Rollover rate" },
                measure.value.map_or_else(|| "--".to_owned(), |value| format_value(goal.kind, value)),
            )
        }
        GoalKind::Spelling => {
            let threshold = goal.target.unwrap_or(0.0) * 100.0;
            details.push(match goal.spelling.as_ref() {
                Some(_) => verdict_line(
                    measure.met,
                    if ja {
                        format!("使用率 {threshold:.0}% 以上")
                    } else {
                        format!("Used {threshold:.0}% of the time or more")
                    },
                    if ja {
                        format!("使用率 {threshold:.0}% に届かず")
                    } else {
                        format!("Below {threshold:.0}%")
                    },
                    if ja {
                        "その仮名を打っていません".to_owned()
                    } else {
                        "The kana was not typed".to_owned()
                    },
                    theme,
                ),
                None => muted(
                    if ja {
                        "勧める綴りがまだありません".to_owned()
                    } else {
                        "No suggested spelling yet".to_owned()
                    },
                    theme,
                ),
            });
            if let Some(target) = goal.spelling.as_ref() {
                details.push(plain(
                    if ja {
                        format!("{}・使用 {} / {} 回", target.label(), measure.spelling_used, measure.spelling_total)
                    } else {
                        format!("{}, used {} of {}", target.label(), measure.spelling_used, measure.spelling_total)
                    },
                    theme,
                ));
            }
            details.extend(judged_on_check(goal, ja, theme));
            details.extend(progress_lines(app, goal, measure, lang, theme));
            (
                if ja { "勧めた綴りの使用率" } else { "Suggested spelling use" },
                measure.value.map_or_else(|| "--".to_owned(), |value| format_value(goal.kind, value)),
            )
        }
        GoalKind::WeakTransition => {
            let line = goal.line(measure);
            details.push(match line {
                Some(line) => verdict_line(
                    measure.met,
                    if ja {
                        format!("基準 {line:.0} ms より速い")
                    } else {
                        format!("Faster than the estimate {line:.0} ms")
                    },
                    if ja {
                        format!("基準 {line:.0} ms より遅い")
                    } else {
                        format!("Slower than the estimate {line:.0} ms")
                    },
                    if ja {
                        "対象の遷移を打っていません".to_owned()
                    } else {
                        "No target transition was typed".to_owned()
                    },
                    theme,
                ),
                None => muted(
                    if ja {
                        "基準なし（遷移の推定がありません）".to_owned()
                    } else {
                        "No estimate for the targets".to_owned()
                    },
                    theme,
                ),
            });
            details.push(plain(
                if ja {
                    format!("対象 {} 種・{} 回", measure.transition_rows.len(), measure.transition_hits)
                } else {
                    format!("{} targets typed {} times", measure.transition_rows.len(), measure.transition_hits)
                },
                theme,
            ));
            details.extend(progress_lines(app, goal, measure, lang, theme));
            for (label, median, prior, n) in measure.transition_rows.iter().take(5) {
                let (mark, color) = match prior {
                    Some(prior) if median <= prior => ("▲", theme.correct),
                    Some(_) => ("▼", theme.mistype),
                    None => (" ", theme.fg),
                };
                let prior = prior.map_or_else(String::new, |prior| {
                    if ja {
                        format!("（基準 {prior:.0}）")
                    } else {
                        format!(" (estimate {prior:.0})")
                    }
                });
                chart.push(Line::from(vec![
                    Span::styled(format!("{label:<4}"), Style::default().fg(theme.muted)),
                    Span::styled(format!("{median:>5.0} ms {mark}"), Style::default().fg(color)),
                    Span::styled(format!("{prior}  ×{n}"), Style::default().fg(theme.muted)),
                ]));
            }
            (
                if ja {
                    "対象遷移の平均（ms）"
                } else {
                    "Target transitions (ms)"
                },
                measure.value.map_or_else(|| "--".to_owned(), |value| format_value(goal.kind, value)),
            )
        }
        GoalKind::Reach => {
            details.push(match goal.target {
                Some(median) => median_verdict(
                    goal,
                    measure,
                    format!("{median:.2}"),
                    if ja {
                        "遠い段かホーム段の打鍵が足りません"
                    } else {
                        "Too few keys on a remote or the home row"
                    },
                    ja,
                    theme,
                ),
                None => no_baseline(goal, ja, theme),
            });
            details.extend(judged_on_check(goal, ja, theme));
            details.extend(progress_lines(app, goal, measure, lang, theme));
            (
                if ja {
                    "遠い段/ホーム段の IKI 比"
                } else {
                    "Remote/home-row IKI ratio"
                },
                measure.value.map_or_else(|| "--".to_owned(), |value| format_value(goal.kind, value)),
            )
        }
    };
    let recent = match goal.origin {
        GoalOrigin::Drill(_) => {
            let mut recent = goal.history[goal.history.len().saturating_sub(19)..].to_vec();
            recent.extend(measure.value);
            recent
        }
        GoalOrigin::Menu => Vec::new(),
    };
    Some(GoalReport {
        title,
        label: label.to_owned(),
        value,
        tone,
        details,
        chart,
        recent,
    })
}

/// Draw the headline into `area` (bordered, titled with the step).
pub fn render_report(frame: &mut Frame, report: &GoalReport, theme: &Theme, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(report.title.clone())
        .border_style(Style::default().fg(tone_color(report.tone, theme)))
        .style(Style::default().fg(theme.fg));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let top_height = 5u16.max(report.details.len() as u16 + 1).min(inner.height);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(top_height), Constraint::Min(0)])
        .split(inner);
    let big = crate::ui::result::big_text(&report.value);
    let big_width = big.iter().map(|row| UnicodeWidthStr::width(row.as_str())).max().unwrap_or(0) as u16;
    let color = tone_color(report.tone, theme);
    let label_style = Style::default().fg(theme.muted).add_modifier(Modifier::BOLD);
    let value_style = Style::default().fg(color).add_modifier(Modifier::BOLD);
    // The large digits need five rows and room for the details beside them.
    let use_big = rows[0].height >= 5 && big_width + 30 <= rows[0].width;
    let left_width = if use_big {
        big_width + 2
    } else {
        (UnicodeWidthStr::width(report.label.as_str()).max(UnicodeWidthStr::width(report.value.as_str())) as u16 + 4).min(rows[0].width / 2)
    };
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(left_width), Constraint::Min(0)])
        .split(rows[0]);
    let mut left = Vec::new();
    if use_big {
        left.extend(big.into_iter().map(|row| Line::from(Span::styled(row, value_style))));
    } else {
        left.push(Line::from(Span::styled(report.label.clone(), label_style)));
        left.push(Line::from(Span::styled(report.value.clone(), value_style)));
    }
    frame.render_widget(Paragraph::new(left).alignment(Alignment::Center), columns[0]);
    let mut right = Vec::new();
    if use_big {
        right.push(Line::from(Span::styled(report.label.clone(), label_style)));
    }
    right.extend(report.details.iter().cloned());
    right.truncate(usize::from(columns[1].height));
    frame.render_widget(Paragraph::new(right), columns[1]);
    let mut chart = report.chart.clone();
    chart.truncate(usize::from(rows[1].height));
    frame.render_widget(Paragraph::new(chart), crate::ui::left_inset(rows[1]));
}

/// The goal's headline as an image card for the image result layouts
/// (MYTASK-3641): the label and the value drawn large in the tone colour,
/// with the cell's recent values as a sparkline. The comparisons stay in
/// the text beside it, where ▲ and ▼ follow the goal's direction.
pub fn goal_card_spec(report: &GoalReport, lang: Lang, cells: ratatui::layout::Size, colors: ResultImageColors) -> MetricCardSpec {
    let accent = match report.tone {
        Tone::Good => colors.correct,
        Tone::Warning => colors.warning,
        Tone::Bad => colors.mistype,
        Tone::Neutral => colors.accent,
    };
    MetricCardSpec {
        label: report.label.clone(),
        value: report.value.clone(),
        previous_delta: None,
        best_delta: None,
        recent: report.recent.clone(),
        lang,
        delta_suffix: String::new(),
        cells,
        colors: ResultImageColors { accent, ..colors },
    }
}
