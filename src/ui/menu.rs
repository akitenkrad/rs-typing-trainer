use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::App;
use crate::i18n::Lang;
use crate::practice::menu::{MenuRole, MenuStep};
use crate::ui::{left_inset, theme::Theme};

pub fn render(frame: &mut Frame, app: &App) {
    let lang = app.config.general.lang;
    let theme = Theme::from_name(&app.config.ui.theme);
    let area = frame.area();
    let title = match lang {
        Lang::En => " TODAY'S MENU ",
        Lang::Ja => " 今日のメニュー ",
    };
    let block = Block::default().borders(Borders::ALL).title(title).style(Style::default().fg(theme.fg));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(menu) = app.daily_menu.as_ref() else {
        let message = app.menu_error.as_deref().unwrap_or(match lang {
            Lang::En => "Menu is unavailable.",
            Lang::Ja => "メニューを表示できません．",
        });
        frame.render_widget(Paragraph::new(message), left_inset(inner));
        return;
    };

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(menu.steps.len().min(usize::from(inner.height)) as u16),
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(1),
        ])
        .split(inner);

    // §4.2 (MYTASK-3619): the actual total; prescription steps keep their
    // catalog minutes, so it can differ from `[menu] minutes`.
    let minutes = crate::practice::menu::minutes_label(menu, lang);
    let summary = match (lang, menu.introductory) {
        (Lang::En, true) => format!("{minutes} · Introductory menu (more data needed)"),
        (Lang::En, false) => format!("{minutes} · Personalized menu"),
        (Lang::Ja, true) => format!("{minutes}・データ不足のため入門メニュー"),
        (Lang::Ja, false) => format!("{minutes}・個別メニュー"),
    };
    let bench = if menu.bench_recommended {
        match lang {
            Lang::En => "Benchmark is recommended first (about 4 min).",
            Lang::Ja => "先にベンチ（約 4 分）を勧めます．",
        }
    } else {
        ""
    };
    frame.render_widget(Paragraph::new(vec![Line::from(summary), Line::from(bench)]), left_inset(rows[0]));

    let width = usize::from(rows[1].width.saturating_sub(4));
    let step_lines = menu
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            let selected = index == app.menu_selected;
            let completion = if app.menu_completed.contains(&index) {
                match lang {
                    Lang::En => "Done",
                    Lang::Ja => "済",
                }
            } else {
                " "
            };
            let text = step_line(index, step, completion, lang);
            Line::from(vec![
                Span::styled(if selected { "▶ " } else { "  " }, Style::default().fg(theme.accent)),
                Span::styled(
                    truncate_display(&text, width.saturating_sub(2)),
                    Style::default()
                        .fg(if selected { theme.warning } else { theme.fg })
                        .add_modifier(if selected { Modifier::BOLD } else { Modifier::empty() }),
                ),
            ])
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(step_lines), left_inset(rows[1]));

    if let Some(step) = menu.steps.get(app.menu_selected) {
        frame.render_widget(
            Paragraph::new(detail_lines(step, lang, theme)).wrap(Wrap { trim: false }),
            left_inset(rows[3]),
        );
    }
    let hint = match lang {
        Lang::En => "[↑/↓] Select    [Enter] Start    [Esc] Title",
        Lang::Ja => "[↑/↓] 選ぶ    [Enter] 開始    [Esc] タイトル",
    };
    frame.render_widget(Paragraph::new(hint).style(Style::default().fg(theme.accent)), left_inset(rows[4]));
}

pub fn render_focus(frame: &mut Frame, app: &App, remaining: f32) {
    let lang = app.config.general.lang;
    let theme = Theme::from_name(&app.config.ui.theme);
    let step = app
        .daily_menu
        .as_ref()
        .and_then(|menu| app.menu_active_step.and_then(|index| menu.steps.get(index)));
    let title = match lang {
        Lang::En => " FOCUS ",
        Lang::Ja => " 意識すること ",
    };
    let block = Block::default().borders(Borders::ALL).title(title).style(Style::default().fg(theme.fg));
    let inner = block.inner(frame.area());
    frame.render_widget(block, frame.area());
    let focus = step.map_or("—", |step| step.focus.as_str());
    let instruction = match lang {
        Lang::En => format!("Starting in {:.1}s · press any key to continue · [Esc] Menu", remaining.max(0.0)),
        Lang::Ja => format!("あと {:.1} 秒で開始・任意のキーで続ける・[Esc] メニュー", remaining.max(0.0)),
    };
    // MYTASK-3640: the cell of the individual-training table this step plays.
    let cell = step.and_then(|step| step_cell_label(step, lang)).unwrap_or_default();
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(cell, Style::default().fg(theme.accent))),
        Line::from(Span::styled(focus, Style::default().fg(theme.warning).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(instruction),
    ];
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
}

/// The name of the cell a menu step plays, «正確さ × 単語» /
/// «Accuracy × Words» (MYTASK-3640).
pub fn step_cell_label(step: &MenuStep, lang: Lang) -> Option<String> {
    crate::practice::menu::step_cell(step).map(|cell| crate::ui::mode_select::cell_label(cell, lang))
}

fn detail_lines(step: &MenuStep, lang: Lang, theme: Theme) -> Vec<Line<'static>> {
    let (drill, reason, focus, advance, evidence) = match lang {
        Lang::En => ("Drill", "Selected because", "Focus", "Graduation condition", "Evidence strength"),
        Lang::Ja => ("訓練", "選んだ理由", "意識すること", "卒業の条件", "根拠の強さ"),
    };
    let cell = step_cell_label(step, lang).unwrap_or_else(|| "—".to_owned());
    vec![
        Line::from(Span::styled(format!("{drill}: {cell}"), Style::default().fg(theme.accent))),
        Line::from(Span::styled(format!("{reason}: {}", step.reason), Style::default().fg(theme.accent))),
        Line::from(Span::styled(format!("{focus}: {}", step.focus), Style::default().fg(theme.warning))),
        Line::from(format!("{advance}: {}", step.advance)),
        Line::from(format!("{evidence}: {}", step.evidence.as_deref().unwrap_or("—"))),
        Line::from(step.evidence_note.clone().unwrap_or_default()),
    ]
}

fn step_line(index: usize, step: &MenuStep, completion: &str, lang: Lang) -> String {
    let language = match (step.language, lang) {
        (Lang::En, Lang::En) => "EN",
        (Lang::Ja, Lang::En) => "JA",
        (Lang::En, Lang::Ja) => "英語",
        (Lang::Ja, Lang::Ja) => "日本語",
    };
    // The practice language goes last: right after the step number, a bare
    // "日" read as "day 1, day 2" instead of "Japanese".
    let prefix = format!("[{completion}] {} {} — {}", index + 1, role_label(step.role, lang), step.title);
    match (lang, step.level) {
        (Lang::En, Some(level)) => format!("{prefix} · Lv {level} · {} min · {language}", step.minutes),
        (Lang::En, None) => format!("{prefix} · {} min · {language}", step.minutes),
        (Lang::Ja, Some(level)) => format!("{prefix}・レベル {level}・{} 分・{language}", step.minutes),
        (Lang::Ja, None) => format!("{prefix}・{} 分・{language}", step.minutes),
    }
}

fn role_label(role: MenuRole, lang: Lang) -> &'static str {
    match (role, lang) {
        (MenuRole::Warmup, Lang::En) => "Warm-up",
        (MenuRole::Warmup, Lang::Ja) => "ウォームアップ",
        (MenuRole::PrimaryA, Lang::En) => "Primary A",
        (MenuRole::PrimaryA, Lang::Ja) => "主処方 A",
        (MenuRole::PrimaryB, Lang::En) => "Primary B",
        (MenuRole::PrimaryB, Lang::Ja) => "主処方 B",
        (MenuRole::Check, Lang::En) => "Check",
        (MenuRole::Check, Lang::Ja) => "確認",
        (MenuRole::Finish, Lang::En) => "Finish",
        (MenuRole::Finish, Lang::Ja) => "仕上げ",
    }
}

fn truncate_display(value: &str, width: usize) -> String {
    if UnicodeWidthStr::width(value) <= width {
        return value.to_owned();
    }
    let mut output = String::new();
    let target = width.saturating_sub(1);
    let mut used = 0;
    for character in value.chars() {
        let character_width = character.width().unwrap_or(0);
        if used + character_width > target {
            break;
        }
        output.push(character);
        used += character_width;
    }
    output.push('…');
    output
}

/// The whole of today's menu on one line, with where the player is
/// (MYTASK-3627): `✓ウォームアップ 2 › ▶遷移 4 › 遷移 4 › 確認 3 › 仕上げ 2  段 2/5 · 残り 13 分`.
///
/// Done steps carry `✓`; `marker` (the step about to start, or the next one
/// on the result screen) carries `▶`. The minutes left are the minutes of the
/// steps not done yet, together with the marked step. When the line does not
/// fit `width` (East Asian display width), the minutes of the done steps are
/// dropped first, then every step's minutes, and last the line shrinks to
/// `段 2/5 ▶遷移 · 残り 13 分`.
pub fn progress_band(
    menu: &crate::practice::menu::DailyMenu,
    completed: &std::collections::BTreeSet<usize>,
    marker: Option<usize>,
    position: usize,
    lang: Lang,
    width: usize,
) -> String {
    let total = menu.steps.len();
    let left: u32 = menu
        .steps
        .iter()
        .enumerate()
        .filter(|(index, _)| !completed.contains(index) || marker == Some(*index))
        .map(|(_, step)| u32::from(step.minutes))
        .sum();
    let status = match lang {
        Lang::En => format!("Step {position}/{total} · {left} min left"),
        Lang::Ja => format!("段 {position}/{total} · 残り {left} 分"),
    };
    let labels = menu
        .steps
        .iter()
        .map(|step| crate::practice::menu::step_short_label(step, lang))
        .collect::<Vec<_>>();
    // How much of the per-step minutes to keep: all, those of the steps not
    // done yet (a done step's minutes no longer matter), none.
    #[derive(Clone, Copy, PartialEq)]
    enum Minutes {
        All,
        Pending,
        None,
    }
    let steps = |minutes: Minutes| {
        menu.steps
            .iter()
            .zip(&labels)
            .enumerate()
            .map(|(index, (step, label))| {
                let marked = marker == Some(index);
                let done = completed.contains(&index) && !marked;
                let mark = if marked {
                    "▶"
                } else if done {
                    "✓"
                } else {
                    ""
                };
                if minutes == Minutes::All || (minutes == Minutes::Pending && !done) {
                    format!("{mark}{label} {}", step.minutes)
                } else {
                    format!("{mark}{label}")
                }
            })
            .collect::<Vec<_>>()
            .join(" › ")
    };
    for minutes in [Minutes::All, Minutes::Pending, Minutes::None] {
        let line = format!("{}  {status}", steps(minutes));
        if UnicodeWidthStr::width(line.as_str()) <= width {
            return line;
        }
    }
    let current = marker
        .and_then(|index| labels.get(index))
        .map(|label| format!(" ▶{label}"))
        .unwrap_or_default();
    let line = match lang {
        Lang::En => format!("Step {position}/{total}{current} · {left} min left"),
        Lang::Ja => format!("段 {position}/{total}{current} · 残り {left} 分"),
    };
    truncate_display(&line, width)
}

/// The steps of a menu run as a chain, `✓ウォームアップ → ✓遷移 → 遷移 → 確認
/// → 仕上げ` (the Summary of the analysis «today» tab, MYTASK-3632): the
/// progress band's short names, `✓` on the done steps. When it does not fit
/// `width`, the spaces around the arrows go first, and last the line shrinks
/// to `段 2/5` (done of total), as the progress band does.
pub fn step_chain(steps: &[(String, bool)], lang: Lang, width: usize) -> String {
    let joined = |separator: &str| {
        steps
            .iter()
            .map(|(label, done)| if *done { format!("✓{label}") } else { label.clone() })
            .collect::<Vec<_>>()
            .join(separator)
    };
    for separator in [" → ", "→"] {
        let line = joined(separator);
        if UnicodeWidthStr::width(line.as_str()) <= width {
            return line;
        }
    }
    truncate_display(&step_chain_short(steps, lang), width)
}

/// The last form of [`step_chain`]: `段 2/5` / `Step 2/5` (done of total).
pub fn step_chain_short(steps: &[(String, bool)], lang: Lang) -> String {
    let done = steps.iter().filter(|(_, done)| *done).count();
    match lang {
        Lang::En => format!("Step {done}/{}", steps.len()),
        Lang::Ja => format!("段 {done}/{}", steps.len()),
    }
}

/// The progress band of the step `index` about to start (countdown screen).
pub fn progress_band_for_start(app: &App, width: usize) -> Option<String> {
    let menu = app.daily_menu.as_ref()?;
    let index = app.menu_active_step?;
    Some(progress_band(
        menu,
        &app.menu_completed,
        Some(index),
        index + 1,
        app.config.general.lang,
        width,
    ))
}

/// The progress band after a step finished (result screen): `▶` moves to the
/// step that [Enter] starts next — the same step while a burst series is
/// unfinished, nothing after the last step.
pub fn progress_band_after_step(app: &App, width: usize) -> Option<String> {
    let menu = app.daily_menu.as_ref()?;
    let current = app.menu_active_step?;
    let repeat = menu
        .steps
        .get(current)
        .is_some_and(|step| step.mode == "burst" && !app.burst_series.complete());
    let next = if repeat { current } else { current + 1 };
    let marker = (next < menu.steps.len()).then_some(next);
    let position = marker.map_or(current + 1, |index| index + 1);
    Some(progress_band(menu, &app.menu_completed, marker, position, app.config.general.lang, width))
}
