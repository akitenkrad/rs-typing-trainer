//! The live goal of a menu step on screen (MYTASK-3633): a meter with the
//! target line while typing, and the headline of the result screen.
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

use crate::app::{App, ResultOrigin};
use crate::i18n::Lang;
use crate::practice::menu::PACER_ACCURACY_FLOOR;
use crate::practice::menu::live_goal::{GoalKind, GoalMeasure, LiveGoal, LiveGoalState, RECENT_INTERVALS, Tone, histogram, mistype_slots};
use crate::ui::theme::Theme;

/// The live goal while a menu step with one is being played; `None` in every
/// other session (individual training, benchmarks, the stand-alone warm-up,
/// the warm-up and check steps).
pub fn playing_goal(app: &App) -> Option<&LiveGoalState> {
    if app.menu_active_step.is_none() || app.active_bench_id.is_some() || app.game.is_none() {
        return None;
    }
    app.menu_live_goal.as_ref()
}

/// The goal of the step whose result is on screen, once its measurement is
/// final.
pub fn result_goal(app: &App) -> Option<(&LiveGoalState, &GoalMeasure)> {
    if app.result_origin != ResultOrigin::JustPlayed || app.menu_active_step.is_none() || app.active_bench_id.is_some() {
        return None;
    }
    let state = app.menu_live_goal.as_ref()?;
    Some((state, state.finished.as_ref()?))
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
        goal.target,
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
    let step = app.daily_menu.as_ref()?.steps.get(app.menu_active_step?)?;
    let level = step.level.map(|level| format!(" Lv{level}")).unwrap_or_default();
    let title = format!(" {}{level} ", step.title);
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
            details.extend(streak_line(app, lang, theme));
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
                None => muted(
                    if ja {
                        "基準なし（確認段の履歴が足りません）".to_owned()
                    } else {
                        "No baseline yet (not enough check steps)".to_owned()
                    },
                    theme,
                ),
            });
            details.push(plain(
                if ja {
                    format!("先走り {} / 最初の誤打 {}", measure.anticipations, measure.first_errors)
                } else {
                    format!("Anticipation {} of {} first errors", measure.anticipations, measure.first_errors)
                },
                theme,
            ));
            details.push(muted(
                if ja {
                    "卒業の判定は確認段の値で行います".to_owned()
                } else {
                    "Graduation is judged on the check step".to_owned()
                },
                theme,
            ));
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
                None => muted(
                    if ja {
                        "基準なし（確認段の履歴が足りません）".to_owned()
                    } else {
                        "No baseline yet (not enough check steps)".to_owned()
                    },
                    theme,
                ),
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
            details.push(muted(
                if ja {
                    "卒業の判定は確認段の値で行います".to_owned()
                } else {
                    "Graduation is judged on the check step".to_owned()
                },
                theme,
            ));
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
            details.extend(streak_line(app, lang, theme));
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
            details.extend(streak_line(app, lang, theme));
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
            details.push(muted(
                if ja {
                    "最も速い 1 本を記録します".to_owned()
                } else {
                    "The fastest one is recorded".to_owned()
                },
                theme,
            ));
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
    };
    Some(GoalReport {
        title,
        label: label.to_owned(),
        value,
        tone,
        details,
        chart,
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
