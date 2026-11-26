//! Pure report data for the post-session review screen.

use crate::analysis::errors::{ErrorKind, ErrorKindMetric, error_kind_counts};
use crate::app::ResultComparison;
use crate::config::DrillSkill;
use crate::i18n::Lang;
use crate::layout::KeyboardLayout;
use crate::layout::geometry::KeyboardGeometry;
use crate::metrics::stats::GameResult;
use crate::practice::menu::ProgressUpdate;
use crate::practice::menu::live_goal::{GoalKind, GoalMeasure, GoalOrigin, LiveGoal, Tone};
use crate::practice::session_variants::compare_endurance_log;
use crate::record::SessionRecord;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewReport {
    pub sections: Vec<ReviewSection>,
    pub improvements: Vec<Improvement>,
    pub next_action: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewSection {
    pub title: String,
    pub lines: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Improvement {
    pub finding: String,
    pub advice: String,
}

#[derive(Clone, Copy)]
pub struct ReviewGoal<'a> {
    pub goal: &'a LiveGoal,
    pub measure: &'a GoalMeasure,
}

pub struct ReviewInput<'a> {
    pub lang: Lang,
    pub name: &'a str,
    pub skill: DrillSkill,
    pub missed: &'a str,
    pub met: &'a str,
    pub tip: &'a str,
    pub result: &'a GameResult,
    pub comparison: ResultComparison,
    pub record: &'a SessionRecord,
    pub layout: &'a KeyboardLayout,
    pub geometry: &'a KeyboardGeometry,
    pub goal: Option<ReviewGoal<'a>>,
    pub menu_progress: Option<ProgressUpdate>,
    pub menu_result_line: Option<&'a str>,
}

pub fn build_review_report(input: ReviewInput<'_>) -> ReviewReport {
    let ja = input.lang == Lang::Ja;
    let mut result_lines = vec![input.name.to_owned()];
    let mut good = Vec::new();
    let mut target_missed = false;
    let mut goal_personal_best = false;

    if let Some(review_goal) = input.goal {
        let goal = review_goal.goal;
        let measure = review_goal.measure;
        let tone = goal.tone(measure);
        let value = measure
            .value
            .map(|value| crate::ui::live_goal::format_value(goal.kind, value))
            .unwrap_or_else(|| "—".to_owned());
        if let Some(line) = goal.line(measure) {
            if let Some(value) = measure.value {
                result_lines.push(format!(
                    "{}: {}{}{}",
                    goal_label(goal.kind, input.lang),
                    line_difference(goal.kind, value, line, input.lang),
                    if ja { "・" } else { " · " },
                    verdict_label(tone, input.lang)
                ));
            } else {
                let line_value = crate::ui::live_goal::format_value(goal.kind, line);
                result_lines.push(if ja {
                    format!(
                        "{} —（線 {line_value}・{}）",
                        goal_label(goal.kind, input.lang),
                        verdict_label(tone, input.lang)
                    )
                } else {
                    format!(
                        "{} — (line {line_value} · {})",
                        goal_label(goal.kind, input.lang),
                        verdict_label(tone, input.lang)
                    )
                });
            }
        } else {
            result_lines.push(if ja {
                format!(
                    "{} {value}（基準なし．この回の値が次回からの基準になります）",
                    goal_label(goal.kind, input.lang)
                )
            } else {
                format!(
                    "{} {value} (no baseline; this result becomes the baseline for later sessions)",
                    goal_label(goal.kind, input.lang)
                )
            });
        }

        match tone {
            Tone::Good => good.push(if ja { "✓ 目標を達成しました" } else { "✓ You met the goal" }.to_owned()),
            Tone::Warning | Tone::Bad => target_missed = true,
            Tone::Neutral => {}
        }

        if goal.origin != GoalOrigin::Menu
            && let Some(value) = measure.value
        {
            if let Some(previous) = goal.previous_value() {
                result_lines.push(value_comparison(
                    if ja { "前回" } else { "Previous" },
                    goal.kind,
                    value,
                    previous,
                    input.lang,
                ));
            }
            if let Some(best) = goal.best_value() {
                goal_personal_best = goal.kind.better(value, best);
                result_lines.push(if goal_personal_best {
                    if ja {
                        format!("自己ベストを更新（これまで {}）", crate::ui::live_goal::format_value(goal.kind, best))
                    } else {
                        format!("New personal best (previously {})", crate::ui::live_goal::format_value(goal.kind, best))
                    }
                } else {
                    value_comparison(if ja { "自己ベスト" } else { "Personal best" }, goal.kind, value, best, input.lang)
                });
            }
        }
    }

    if let Some(line) = input.menu_result_line {
        result_lines.push(line.to_owned());
    } else if let Some(progress) = input.menu_progress {
        result_lines.push(progress_line(progress, input.lang));
    }
    result_lines.push(if ja {
        format!(
            "WPM {:.1}・CPS {:.2}・正確さ {:.1}%・誤打 {} 回",
            input.result.wpm,
            input.result.cps,
            input.result.accuracy * 100.0,
            input.result.metrics.mistypes_total
        )
    } else {
        format!(
            "WPM {:.1} · CPS {:.2} · accuracy {:.1}% · {} errors",
            input.result.wpm,
            input.result.cps,
            input.result.accuracy * 100.0,
            input.result.metrics.mistypes_total
        )
    });
    if let Some(line) = previous_metrics_line(&input.comparison, input.lang) {
        result_lines.push(line);
    }

    let general_personal_best = [
        input.comparison.best_wpm_delta,
        input.comparison.best_cps_delta,
        input.comparison.best_accuracy_delta,
    ]
    .into_iter()
    .flatten()
    .any(|delta| delta > 0.0);
    if goal_personal_best || general_personal_best {
        good.push(
            if ja {
                "✓ 自己ベストを更新しました"
            } else {
                "✓ You set a new personal best"
            }
            .to_owned(),
        );
    }
    if input.result.accuracy >= 0.98 {
        good.push(if ja {
            format!("✓ 正確さを {:.1}% に保ちました", input.result.accuracy * 100.0)
        } else {
            format!("✓ You kept accuracy at {:.1}%", input.result.accuracy * 100.0)
        });
    }
    if input.comparison.previous_wpm_delta.is_some_and(|delta| delta > 0.0)
        && input.comparison.previous_accuracy_delta.is_some_and(|delta| delta >= 0.0)
    {
        good.push(
            if ja {
                "✓ 前回より WPM が上がり，正確さも落ちませんでした"
            } else {
                "✓ WPM improved without losing accuracy"
            }
            .to_owned(),
        );
    }
    good.truncate(3);

    let mut improvements = Vec::new();
    if target_missed {
        push_improvement(
            &mut improvements,
            Improvement {
                finding: input.missed.to_owned(),
                advice: advice_from_missed(input.missed, input.lang),
            },
        );
    }

    let accuracy_missed = target_missed
        && input
            .goal
            .is_some_and(|review_goal| matches!(review_goal.goal.kind, GoalKind::AccuracyGate | GoalKind::Blind));
    if improvements.len() < 3 && input.result.accuracy < 0.96 && !accuracy_missed {
        push_improvement(
            &mut improvements,
            Improvement {
                finding: if ja {
                    format!("正確さが {:.1}% でした．", input.result.accuracy * 100.0)
                } else {
                    format!("Accuracy was {:.1}%.", input.result.accuracy * 100.0)
                },
                advice: if ja {
                    "速さを少し落とし，1 打ずつ確かめましょう".to_owned()
                } else {
                    "Slow down a little and confirm each keystroke.".to_owned()
                },
            },
        );
    }

    let error_kinds = error_kind_counts(std::slice::from_ref(input.record), input.layout, input.geometry);
    let top_error = top_error(&error_kinds);
    if improvements.len() < 3
        && let Some(row) = top_error.filter(|row| row.n >= 3)
        && !(target_missed && trains_error_kind(input.skill, row.kind))
    {
        push_improvement(
            &mut improvements,
            Improvement {
                finding: if ja {
                    format!(
                        "最も多い誤り: {} {} 件（最初の誤打の {:.0}%）",
                        crate::analysis::coach::error_label(row.kind, input.lang),
                        row.n,
                        row.proportion * 100.0
                    )
                } else {
                    format!(
                        "Most common error: {} — {} ({:.0}% of first errors)",
                        crate::analysis::coach::error_label(row.kind, input.lang),
                        row.n,
                        row.proportion * 100.0
                    )
                },
                advice: error_advice(row.kind, input.lang),
            },
        );
    }

    if improvements.len() < 3
        && let Some((expected, count)) = most_mistyped_key(input.result)
        && count >= 3
    {
        push_improvement(
            &mut improvements,
            Improvement {
                finding: if ja {
                    format!("{expected} キーで {count} 回誤打しました．")
                } else {
                    format!("You mistyped the {expected} key {count} times.")
                },
                advice: if ja {
                    format!("次回は {expected} キーを含む短い語を，ゆっくり正確に打ちましょう")
                } else {
                    format!("Next time, type short words containing {expected} slowly and accurately.")
                },
            },
        );
    }

    if improvements.len() < 3 {
        let endurance = compare_endurance_log(&input.record.log);
        if endurance.first_iki_ms > 0.0 && endurance.second_iki_ms > 0.0 {
            let slowdown = endurance.second_iki_ms / endurance.first_iki_ms - 1.0;
            if slowdown >= 0.10 {
                push_improvement(
                    &mut improvements,
                    Improvement {
                        finding: if ja {
                            format!("後半に {:.0}% 遅くなりました．", slowdown * 100.0)
                        } else {
                            format!("You slowed down {:.0}% in the second half.", slowdown * 100.0)
                        },
                        advice: if ja {
                            "次回は少し遅めに始め，長い練習の前には短く休みましょう".to_owned()
                        } else {
                            "Start a little slower next time, and take a short break before a long session.".to_owned()
                        },
                    },
                );
            }
        }
    }

    if improvements.is_empty() {
        push_improvement(
            &mut improvements,
            Improvement {
                finding: if ja {
                    "大きな崩れはありません．".to_owned()
                } else {
                    "There was no major drop-off.".to_owned()
                },
                advice: if ja {
                    "次回も同じ意識で続けましょう".to_owned()
                } else {
                    "Keep the same focus next time.".to_owned()
                },
            },
        );
    }
    improvements.truncate(3);

    let achieved = input
        .goal
        .is_some_and(|review_goal| review_goal.goal.tone(review_goal.measure) == Tone::Good);
    let action = if achieved {
        input.met.to_owned()
    } else {
        improvements
            .iter()
            .find(|item| !item.advice.is_empty())
            .map_or_else(|| improvements[0].finding.clone(), |item| item.advice.clone())
    };
    let mut next_action = next_action(input.tip, &action, input.lang);
    if let Some(progress) = input.menu_progress {
        let remaining = if progress.graduation.is_some() {
            0
        } else {
            progress.required.saturating_sub(progress.consecutive)
        };
        next_action.push_str(&match input.lang {
            Lang::Ja => format!("．卒業まであと {remaining} 回"),
            Lang::En => format!(". {remaining} more until graduation"),
        });
    }

    let mut sections = vec![ReviewSection {
        title: if ja { "今回の結果" } else { "This Session" }.to_owned(),
        lines: result_lines,
    }];
    if !good.is_empty() {
        sections.push(ReviewSection {
            title: if ja { "良かった点" } else { "What Went Well" }.to_owned(),
            lines: good,
        });
    }
    sections.push(ReviewSection {
        title: if ja { "改善点" } else { "Improvements" }.to_owned(),
        lines: improvements
            .iter()
            .enumerate()
            .flat_map(|(index, item)| {
                let mut lines = vec![format!("{}. {}", index + 1, item.finding)];
                if !item.advice.is_empty() {
                    lines.push(format!("   → {}", item.advice));
                }
                lines
            })
            .collect(),
    });
    sections.push(ReviewSection {
        title: if ja { "次回やること" } else { "Next Time" }.to_owned(),
        lines: vec![next_action.clone()],
    });

    ReviewReport {
        sections,
        improvements,
        next_action,
    }
}

fn verdict_label(tone: Tone, lang: Lang) -> &'static str {
    match (tone, lang) {
        (Tone::Good, Lang::En) => "Met",
        (Tone::Good, Lang::Ja) => "達成",
        (Tone::Warning, Lang::En) => "Almost",
        (Tone::Warning, Lang::Ja) => "あと少し",
        (Tone::Bad, Lang::En) => "Not met",
        (Tone::Bad, Lang::Ja) => "未達",
        (Tone::Neutral, Lang::En) => "No judgment",
        (Tone::Neutral, Lang::Ja) => "判定なし",
    }
}

fn goal_label(kind: GoalKind, lang: Lang) -> &'static str {
    match (kind, lang) {
        (GoalKind::AccuracyGate, Lang::En) => "Accuracy",
        (GoalKind::AccuracyGate, Lang::Ja) => "正確さ",
        (GoalKind::Anticipation, Lang::En) => "Anticipation share",
        (GoalKind::Anticipation, Lang::Ja) => "先走りの割合",
        (GoalKind::Metronome, Lang::En) => "In-word IKI CV",
        (GoalKind::Metronome, Lang::Ja) => "語中 IKI の CV",
        (GoalKind::Pacer, Lang::En) => "Speed (WPM)",
        (GoalKind::Pacer, Lang::Ja) => "速さ（WPM）",
        (GoalKind::EnduranceSplit, Lang::En) => "Second-half slowdown",
        (GoalKind::EnduranceSplit, Lang::Ja) => "後半の遅れ",
        (GoalKind::Sprint, Lang::En) => "Fastest sprint (WPM)",
        (GoalKind::Sprint, Lang::Ja) => "最速の 1 本（WPM）",
        (GoalKind::Recovery, Lang::En) => "Excess after errors (ms)",
        (GoalKind::Recovery, Lang::Ja) => "誤打後の余分な時間（ms）",
        (GoalKind::Blind, Lang::En) => "Accuracy (no-look)",
        (GoalKind::Blind, Lang::Ja) => "正確さ（見ないで）",
        (GoalKind::Preview, Lang::En) => "Word onset (start/inside)",
        (GoalKind::Preview, Lang::Ja) => "語頭の遅れ（語頭/語中）",
        (GoalKind::Overlap, Lang::En) => "Rollover rate",
        (GoalKind::Overlap, Lang::Ja) => "ロールオーバー率",
        (GoalKind::Spelling, Lang::En) => "Suggested spelling use",
        (GoalKind::Spelling, Lang::Ja) => "勧めた綴りの使用率",
        (GoalKind::WeakTransition, Lang::En) => "Target transitions (ms)",
        (GoalKind::WeakTransition, Lang::Ja) => "対象遷移の平均（ms）",
        (GoalKind::Reach, Lang::En) => "Remote/home-row IKI ratio",
        (GoalKind::Reach, Lang::Ja) => "遠い段/ホーム段の IKI 比",
    }
}

fn line_difference(kind: GoalKind, value: f64, line: f64, lang: Lang) -> String {
    let better = kind.better(value, line);
    let worse = kind.better(line, value);
    let description = match (lang, better, worse) {
        (Lang::Ja, true, _) => "良い",
        (Lang::Ja, _, true) => "悪い",
        (Lang::Ja, _, _) => "同じ",
        (Lang::En, true, _) => "better",
        (Lang::En, _, true) => "worse",
        (Lang::En, _, _) => "the same",
    };
    let value_text = crate::ui::live_goal::format_value(kind, value);
    let line_text = crate::ui::live_goal::format_value(kind, line);
    let delta = format_delta(kind, value - line);
    match lang {
        Lang::Ja => format!("線 {line_text} に対して {value_text}（{delta} {description}）"),
        Lang::En => format!("{value_text} against line {line_text} ({delta} {description})"),
    }
}

fn value_comparison(label: &str, kind: GoalKind, value: f64, other: f64, lang: Lang) -> String {
    let word = match (lang, kind.better(value, other), kind.better(other, value)) {
        (Lang::Ja, true, _) => "改善",
        (Lang::Ja, _, true) => "後退",
        (Lang::Ja, _, _) => "同じ",
        (Lang::En, true, _) => "better",
        (Lang::En, _, true) => "worse",
        (Lang::En, _, _) => "same",
    };
    format!(
        "{label} {}（{} {word}）",
        crate::ui::live_goal::format_value(kind, other),
        format_delta(kind, value - other)
    )
}

fn format_delta(kind: GoalKind, delta: f64) -> String {
    let delta = delta.abs();
    match kind {
        GoalKind::AccuracyGate | GoalKind::Blind | GoalKind::EnduranceSplit => format!("{:.1}pt", delta * 100.0),
        GoalKind::Anticipation | GoalKind::Spelling => format!("{:.1}pt", delta * 100.0),
        GoalKind::Metronome | GoalKind::Preview | GoalKind::Reach => format!("{delta:.2}"),
        GoalKind::Pacer | GoalKind::Sprint => format!("{delta:.1} WPM"),
        GoalKind::Recovery | GoalKind::WeakTransition => format!("{delta:.0} ms"),
        GoalKind::Overlap => format!("{delta:.0}pt"),
    }
}

fn progress_line(progress: ProgressUpdate, lang: Lang) -> String {
    use crate::practice::menu::Graduation;

    match (progress.graduation, lang) {
        (Some(Graduation::Level(level)), Lang::En) => format!("Graduated → level {level}"),
        (Some(Graduation::Level(level)), Lang::Ja) => format!("卒業 → レベル {level}"),
        (Some(Graduation::Maintenance), Lang::En) => "Graduated → maintenance".to_owned(),
        (Some(Graduation::Maintenance), Lang::Ja) => "卒業 → 維持".to_owned(),
        (None, Lang::En) => format!("Graduation progress: {}/{}", progress.consecutive, progress.required),
        (None, Lang::Ja) => format!("卒業の進み具合: {}/{}", progress.consecutive, progress.required),
    }
}

fn previous_metrics_line(comparison: &ResultComparison, lang: Lang) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(delta) = comparison.previous_wpm_delta {
        parts.push(format!("WPM {delta:+.1}"));
    }
    if let Some(delta) = comparison.previous_cps_delta {
        parts.push(format!("CPS {delta:+.2}"));
    }
    if let Some(delta) = comparison.previous_accuracy_delta {
        parts.push(format!(
            "{} {:+.1}pt",
            if lang == Lang::Ja { "正確さ" } else { "accuracy" },
            delta * 100.0
        ));
    }
    (!parts.is_empty()).then(|| match lang {
        Lang::Ja => format!("前回との差: {}", parts.join("・")),
        Lang::En => format!("From previous: {}", parts.join(" · ")),
    })
}

fn advice_from_missed(missed: &str, lang: Lang) -> String {
    let separator = if lang == Lang::Ja { "．" } else { ". " };
    missed
        .split_once(separator)
        .map(|(_, advice)| advice.trim().to_owned())
        .filter(|advice| !advice.is_empty())
        .unwrap_or_else(|| missed.to_owned())
}

fn top_error(rows: &[ErrorKindMetric]) -> Option<&ErrorKindMetric> {
    rows.iter()
        .filter(|row| row.kind != ErrorKind::Consecutive && row.n > 0)
        .max_by(|left, right| left.n.cmp(&right.n).then_with(|| right.kind.cmp(&left.kind)))
}

fn trains_error_kind(skill: DrillSkill, kind: ErrorKind) -> bool {
    matches!(
        (skill, kind),
        (DrillSkill::Anticipation, ErrorKind::Anticipation)
            | (DrillSkill::Accuracy, ErrorKind::AdjacentKey)
            | (DrillSkill::Overlap, ErrorKind::RepeatedPrevious)
            | (DrillSkill::Weak, ErrorKind::SameFinger | ErrorKind::OppositeHand)
    )
}

fn error_advice(kind: ErrorKind, lang: Lang) -> String {
    crate::analysis::coach::error_advice(kind, lang).map_or_else(
        || match lang {
            Lang::En => "Check the expected-key → pressed-key pairs in the result-screen keyboard diagram.".to_owned(),
            Lang::Ja => "誤打したキーの組（期待キー→押したキー）を結果画面のキーボード図で確かめましょう".to_owned(),
        },
        str::to_owned,
    )
}

fn push_improvement(improvements: &mut Vec<Improvement>, mut item: Improvement) {
    if improvements
        .iter()
        .any(|existing| advice_key(&existing.advice) == advice_key(&item.advice))
    {
        item.advice.clear();
    }
    improvements.push(item);
}

fn advice_key(advice: &str) -> String {
    let lower = advice.to_lowercase();
    if (lower.contains("速さ") && lower.contains("確かめ")) || (lower.contains("slow") && (lower.contains("confirm") || lower.contains("keystroke")))
    {
        return "slow-confirm".to_owned();
    }
    lower.chars().filter(|character| character.is_alphanumeric()).collect()
}

fn next_action(tip: &str, action: &str, lang: Lang) -> String {
    match lang {
        Lang::Ja => format!(
            "次回: {}．{}",
            tip.trim().trim_end_matches(['．', '。']),
            action.trim().trim_start_matches(['．', '。'])
        ),
        Lang::En => format!("Next: {}. {}", tip.trim().trim_end_matches('.'), action.trim().trim_start_matches('.')),
    }
}

fn most_mistyped_key(result: &GameResult) -> Option<(&str, u32)> {
    result
        .metrics
        .mistypes_by_char
        .iter()
        .max_by(|left, right| left.1.cmp(right.1).then_with(|| right.0.cmp(left.0)))
        .map(|(expected, count)| (expected.as_str(), *count))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chrono::Utc;

    use super::*;
    use crate::analysis::metrics::AnalysisSettings;
    use crate::app::ResultComparison;
    use crate::config::{Config, DrillCell, ErrorMode, PromptUnit};
    use crate::engine::session::{ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};
    use crate::layout::builtin_layout;
    use crate::layout::geometry::geometry;
    use crate::metrics::stats::{GameResult, LiveMetrics};
    use crate::practice::menu::live_goal::{GoalSetup, LiveGoal};

    fn result(accuracy: f64) -> GameResult {
        let mut result = GameResult::from_metrics(
            &Config::default(),
            LiveMetrics {
                mistypes_total: 4,
                total_chars_typed: 100,
                correct_chars: (accuracy * 100.0).round() as u32,
                ..LiveMetrics::default()
            },
            Duration::from_secs(60),
        );
        result.wpm = 52.0;
        result.cps = 4.33;
        result.accuracy = accuracy;
        result
    }

    fn empty_record() -> SessionRecord {
        let mut record = SessionRecord::from_log(&Config::default(), SessionLog::default(), Utc::now());
        record.header.metrics_version = 2;
        record.header.layout = "qwerty".to_owned();
        record.header.geometry = "ansi".to_owned();
        record
    }

    fn anticipation_record() -> SessionRecord {
        let mut items = Vec::new();
        let mut keys = Vec::new();
        for item_idx in 0..3_u32 {
            items.push(ItemLog {
                idx: item_idx,
                text: "as".to_owned(),
                reading: None,
                kind: None,
                shown_us: u64::from(item_idx) * 1_000_000,
                done_us: Some(u64::from(item_idx) * 1_000_000 + 300_000),
                outcome: ItemOutcome::Completed,
            });
            keys.push(KeystrokeLog {
                seq: item_idx * 2,
                t_us: u64::from(item_idx) * 1_000_000 + 100_000,
                release_us: None,
                item_idx,
                pos: 0,
                kind: KeystrokeKind::Char,
                typed: Some("s".to_owned()),
                expected: Some("a".to_owned()),
                correct: false,
                code: Some("KeyS".to_owned()),
            });
            keys.push(KeystrokeLog {
                seq: item_idx * 2 + 1,
                t_us: u64::from(item_idx) * 1_000_000 + 200_000,
                release_us: None,
                item_idx,
                pos: 0,
                kind: KeystrokeKind::Char,
                typed: Some("a".to_owned()),
                expected: Some("a".to_owned()),
                correct: true,
                code: Some("KeyA".to_owned()),
            });
        }
        let log = SessionLog {
            duration_us: 4_000_000,
            error_mode: ErrorMode::Stop,
            items,
            keystrokes: keys,
        };
        let mut record = SessionRecord::from_log(&Config::default(), log, Utc::now());
        record.header.metrics_version = 2;
        record
    }

    fn other_substitution_record() -> SessionRecord {
        let mut record = anticipation_record();
        for key in &mut record.log.keystrokes {
            if key.correct {
                key.typed = Some("q".to_owned());
                key.expected = Some("q".to_owned());
                key.code = Some("KeyQ".to_owned());
            } else {
                key.typed = Some("f".to_owned());
                key.expected = Some("q".to_owned());
                key.code = Some("KeyF".to_owned());
            }
        }
        record
    }

    fn goal(skill: DrillSkill, target: f64) -> LiveGoal {
        let cell = DrillCell::new(skill, PromptUnit::Word);
        let mut setup = GoalSetup {
            analysis: AnalysisSettings::default(),
            as_of: Utc::now(),
            ..GoalSetup::default()
        };
        match skill {
            DrillSkill::Accuracy => setup.gate_min_accuracy = target,
            DrillSkill::Anticipation => setup.own_median = Some(target),
            _ => unreachable!(),
        }
        LiveGoal::for_cell(cell, setup).unwrap()
    }

    fn input<'a>(
        skill: DrillSkill,
        result: &'a GameResult,
        record: &'a SessionRecord,
        layout: &'a KeyboardLayout,
        geometry: &'a KeyboardGeometry,
        goal: Option<ReviewGoal<'a>>,
    ) -> ReviewInput<'a> {
        ReviewInput {
            lang: Lang::Ja,
            name: "正確さ × 単語",
            skill,
            missed: "目標未達の文．次は落ち着いて打つ",
            met: "達成後の文",
            tip: "1 打ずつ確かめる",
            result,
            comparison: ResultComparison::default(),
            record,
            layout,
            geometry,
            goal,
            menu_progress: None,
            menu_result_line: None,
        }
    }

    #[test]
    fn missed_goal_puts_briefing_missed_first() {
        let result = result(0.90);
        let record = empty_record();
        let layout = builtin_layout("qwerty").unwrap();
        let geometry = geometry("ansi").unwrap();
        let goal = goal(DrillSkill::Accuracy, 0.97);
        let measure = GoalMeasure {
            value: Some(0.90),
            accuracy: 0.90,
            ..GoalMeasure::default()
        };

        let report = build_review_report(input(
            DrillSkill::Accuracy,
            &result,
            &record,
            &layout,
            &geometry,
            Some(ReviewGoal {
                goal: &goal,
                measure: &measure,
            }),
        ));

        assert_eq!(report.improvements[0].finding, "目標未達の文．次は落ち着いて打つ");
    }

    #[test]
    fn achieved_goal_adds_good_point_and_uses_briefing_met_next() {
        let result = result(0.99);
        let record = empty_record();
        let layout = builtin_layout("qwerty").unwrap();
        let geometry = geometry("ansi").unwrap();
        let goal = goal(DrillSkill::Accuracy, 0.97);
        let measure = GoalMeasure {
            value: Some(0.99),
            accuracy: 0.99,
            ..GoalMeasure::default()
        };

        let report = build_review_report(input(
            DrillSkill::Accuracy,
            &result,
            &record,
            &layout,
            &geometry,
            Some(ReviewGoal {
                goal: &goal,
                measure: &measure,
            }),
        ));

        assert!(
            report
                .sections
                .iter()
                .flat_map(|section| &section.lines)
                .any(|line| line.contains("目標を達成"))
        );
        assert!(report.next_action.starts_with("次回:"), "{}", report.next_action);
        assert!(report.next_action.contains("達成後の文"), "{}", report.next_action);

        let good = report.sections.iter().find(|section| section.title == "良かった点").unwrap();
        assert!(good.lines.iter().all(|line| line.starts_with("✓ ")), "{:?}", good.lines);
        let improvements = report.sections.iter().find(|section| section.title == "改善点").unwrap();
        assert!(improvements.lines[0].starts_with("1. "), "{:?}", improvements.lines);
        assert!(improvements.lines[1].starts_with("   → "), "{:?}", improvements.lines);
    }

    #[test]
    fn accuracy_below_96_percent_adds_accuracy_improvement() {
        let result = result(0.955);
        let record = empty_record();
        let layout = builtin_layout("qwerty").unwrap();
        let geometry = geometry("ansi").unwrap();

        let report = build_review_report(input(DrillSkill::Normal, &result, &record, &layout, &geometry, None));

        assert!(report.improvements.iter().any(|item| item.finding.contains("95.5%")));
    }

    #[test]
    fn top_error_is_omitted_when_the_missed_goal_trains_that_error() {
        let mut result = result(0.98);
        result.metrics.mistypes_by_char.insert("a".to_owned(), 3);
        let record = anticipation_record();
        let layout = builtin_layout("qwerty").unwrap();
        let geometry = geometry("ansi").unwrap();
        let goal = goal(DrillSkill::Anticipation, 0.20);
        let measure = GoalMeasure {
            value: Some(1.0),
            anticipations: 3,
            first_errors: 3,
            accuracy: 0.98,
            ..GoalMeasure::default()
        };

        let report = build_review_report(input(
            DrillSkill::Anticipation,
            &result,
            &record,
            &layout,
            &geometry,
            Some(ReviewGoal {
                goal: &goal,
                measure: &measure,
            }),
        ));

        assert_eq!(
            report.improvements.first().map(|item| item.finding.as_str()),
            Some("目標未達の文．次は落ち着いて打つ")
        );
        assert!(!report.improvements.iter().any(|item| item.finding.contains("最も多い誤り")));

        let normal = build_review_report(input(DrillSkill::Normal, &result, &record, &layout, &geometry, None));
        assert!(
            normal.improvements.iter().any(|item| item.finding.contains("最も多い誤り")),
            "the fixture must actually produce a top classified error"
        );
    }

    #[test]
    fn improvements_never_exceed_three() {
        let mut result = result(0.90);
        result.metrics.mistypes_by_char.insert("a".to_owned(), 4);
        let mut record = anticipation_record();
        record.log.duration_us = 4_000_000;
        let layout = builtin_layout("qwerty").unwrap();
        let geometry = geometry("ansi").unwrap();
        let goal = goal(DrillSkill::Accuracy, 0.97);
        let measure = GoalMeasure {
            value: Some(0.90),
            accuracy: 0.90,
            ..GoalMeasure::default()
        };

        let report = build_review_report(input(
            DrillSkill::Accuracy,
            &result,
            &record,
            &layout,
            &geometry,
            Some(ReviewGoal {
                goal: &goal,
                measure: &measure,
            }),
        ));

        assert_eq!(report.improvements.len(), 3);
    }

    #[test]
    fn uncoached_error_uses_key_pair_advice_without_repeating_slow_down_advice() {
        let mut result = result(0.89);
        result.metrics.mistypes_by_char.insert("q".to_owned(), 3);
        let record = other_substitution_record();
        let layout = builtin_layout("qwerty").unwrap();
        let geometry = geometry("ansi").unwrap();

        let report = build_review_report(input(DrillSkill::Normal, &result, &record, &layout, &geometry, None));
        let classified = report
            .improvements
            .iter()
            .find(|item| item.finding.contains("最も多い誤り"))
            .expect("other substitutions should be reported");
        assert!(classified.finding.contains("その他の置換"), "{}", classified.finding);
        assert!(classified.advice.contains("期待キー→押したキー"), "{}", classified.advice);
        let slow_confirm = report
            .improvements
            .iter()
            .filter(|item| item.advice.contains("速さ") && item.advice.contains("確かめ"))
            .count();
        assert_eq!(slow_confirm, 1, "{:?}", report.improvements);
    }

    #[test]
    fn later_effectively_duplicate_advice_is_omitted() {
        let mut improvements = Vec::new();
        push_improvement(
            &mut improvements,
            Improvement {
                finding: "正確さが低い".to_owned(),
                advice: "速さを少し落とし，1 打ずつ確かめましょう".to_owned(),
            },
        );
        push_improvement(
            &mut improvements,
            Improvement {
                finding: "誤りが多い".to_owned(),
                advice: "速さを落とし，打つ文字を確かめましょう".to_owned(),
            },
        );

        assert!(!improvements[0].advice.is_empty());
        assert!(improvements[1].advice.is_empty(), "{:?}", improvements);
    }

    #[test]
    fn goal_without_a_baseline_explains_that_this_result_becomes_the_baseline() {
        let result = result(0.98);
        let record = empty_record();
        let layout = builtin_layout("qwerty").unwrap();
        let geometry = geometry("ansi").unwrap();
        let cell = DrillCell::new(DrillSkill::Anticipation, PromptUnit::Word);
        let goal = LiveGoal::for_cell(
            cell,
            GoalSetup {
                analysis: AnalysisSettings::default(),
                as_of: Utc::now(),
                ..GoalSetup::default()
            },
        )
        .unwrap();
        let measure = GoalMeasure {
            value: Some(0.0),
            accuracy: 0.98,
            ..GoalMeasure::default()
        };

        let report = build_review_report(input(
            DrillSkill::Anticipation,
            &result,
            &record,
            &layout,
            &geometry,
            Some(ReviewGoal {
                goal: &goal,
                measure: &measure,
            }),
        ));
        let result_lines = &report.sections[0].lines;
        assert!(
            result_lines
                .iter()
                .any(|line| line.contains("先走りの割合 0%（基準なし．この回の値が次回からの基準になります）")),
            "{result_lines:?}"
        );
        assert!(result_lines.iter().all(|line| !line.contains("判定 判定なし")), "{result_lines:?}");
    }

    #[test]
    fn report_builds_without_a_live_goal() {
        let result = result(1.0);
        let record = empty_record();
        let layout = builtin_layout("qwerty").unwrap();
        let geometry = geometry("ansi").unwrap();

        let report = build_review_report(input(DrillSkill::Normal, &result, &record, &layout, &geometry, None));

        assert_eq!(report.sections.first().map(|section| section.title.as_str()), Some("今回の結果"));
        assert!(!report.next_action.is_empty());
    }
}
