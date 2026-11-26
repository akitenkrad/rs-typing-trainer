//! Full-screen explanation shown before a menu step or table-cell session.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::config::{DrillCell, ModeKind};
use crate::i18n::Lang;
use crate::practice::briefing::{ResolvedBriefing, resolve_menu_step, resolve_skill};
use crate::practice::menu::{MenuRole, MenuStep};
use crate::ui::theme::Theme;
use crate::ui::{left_inset, wrap_display, wrap_field};

#[derive(Clone, Copy)]
struct Labels {
    purpose: &'static str,
    goal: &'static str,
    content: &'static str,
    tips: &'static str,
    focus: &'static str,
    advance: &'static str,
    reason: &'static str,
}

impl Labels {
    fn new(lang: Lang) -> Self {
        match lang {
            Lang::En => Self {
                purpose: "Purpose",
                goal: "Achieve",
                content: "Training",
                tips: "Tips",
                focus: "Focus",
                advance: "Graduation",
                reason: "Selected because",
            },
            Lang::Ja => Self {
                purpose: "目的",
                goal: "達成すること",
                content: "訓練の内容",
                tips: "コツ",
                focus: "意識すること",
                advance: "卒業の条件",
                reason: "選んだ理由",
            },
        }
    }

    fn width(self) -> usize {
        [self.purpose, self.goal, self.content, self.tips, self.focus, self.advance, self.reason]
            .into_iter()
            .map(UnicodeWidthStr::width)
            .max()
            .unwrap_or(0)
    }
}

#[derive(Clone, Copy)]
enum SectionTone {
    Normal,
    Accent,
    Warning,
}

struct Section {
    label: &'static str,
    values: Vec<String>,
    tone: SectionTone,
}

/// Render the explanation for the menu step or individual-training cell that
/// the app has prepared. The footer owns the last inner row, independently of
/// how much of the body fits.
pub fn render(frame: &mut Frame, app: &App) {
    let lang = app.config.general.lang;
    let theme = Theme::from_name(&app.config.ui.theme);
    let title = match lang {
        Lang::En => " TRAINING BRIEFING ",
        Lang::Ja => " 訓練の説明 ",
    };
    let block = Block::default().borders(Borders::ALL).title(title).style(Style::default().fg(theme.fg));
    let inner = left_inset(block.inner(frame.area()));
    frame.render_widget(block, frame.area());

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner);
    let width = usize::from(rows[0].width);
    if let Some(lines) = body_lines(app, width, usize::from(rows[0].height), theme) {
        frame.render_widget(Paragraph::new(lines), rows[0]);
    }
    let hint = match lang {
        Lang::En => "[Enter] Start    [?] Terms    [Esc] Back",
        Lang::Ja => "[Enter] 開始    [?] 用語    [Esc] 戻る",
    };
    frame.render_widget(Paragraph::new(hint).style(Style::default().fg(theme.accent)), rows[1]);
}

fn body_lines(app: &App, width: usize, height: usize, theme: Theme) -> Option<Vec<Line<'static>>> {
    let lang = app.config.general.lang;
    let labels = Labels::new(lang);
    let (resolved, heading, subtitle, step) = context(app, lang)?;
    let mut sections = core_sections(app, &resolved, step, labels, lang);

    let mut lines = heading_lines(&heading, subtitle.as_deref(), width, theme);
    lines.extend(section_lines(&sections, labels.width(), width, theme));
    if lines.len() > height && step.is_some() {
        sections.retain(|section| section.label != labels.reason);
        lines = heading_lines(&heading, subtitle.as_deref(), width, theme);
        lines.extend(section_lines(&sections, labels.width(), width, theme));
    }
    if lines.len() > height && step.is_some() {
        sections.retain(|section| section.label != labels.advance);
        lines = heading_lines(&heading, subtitle.as_deref(), width, theme);
        lines.extend(section_lines(&sections, labels.width(), width, theme));
    }
    Some(lines)
}

fn context(app: &App, lang: Lang) -> Option<(ResolvedBriefing, String, Option<String>, Option<&MenuStep>)> {
    if let Some(step) = app
        .daily_menu
        .as_ref()
        .and_then(|menu| app.menu_active_step.and_then(|index| menu.steps.get(index)))
    {
        let cell = crate::practice::menu::step_cell(step)?;
        let resolved = resolve_menu_step(step).ok()?;
        let heading = menu_heading(cell, step, lang);
        return Some((resolved, heading, Some(step.title.clone()), Some(step)));
    }

    let cell = app.cell_active()?;
    let resolved = resolve_skill(cell.skill).ok()?;
    let heading = cell_heading(app, cell, lang);
    Some((resolved, heading, None, None))
}

fn menu_heading(cell: DrillCell, step: &MenuStep, lang: Lang) -> String {
    let cell = crate::ui::mode_select::cell_label(cell, lang);
    let role = role_label(step.role, lang);
    let language = language_label(step.language, lang);
    match (lang, step.level) {
        (Lang::En, Some(level)) => format!("{cell} · {role} · Level {level} · {} min · {language}", step.minutes),
        (Lang::En, None) => format!("{cell} · {role} · {} min · {language}", step.minutes),
        (Lang::Ja, Some(level)) => format!("{cell}・{role}・レベル {level}・{} 分・{language}", step.minutes),
        (Lang::Ja, None) => format!("{cell}・{role}・{} 分・{language}", step.minutes),
    }
}

fn cell_heading(app: &App, cell: DrillCell, lang: Lang) -> String {
    let cell = crate::ui::mode_select::cell_label(cell, lang);
    let language = language_label(app.practice_lang, lang);
    let minutes = if app.config.mode.kind == ModeKind::Endurance {
        app.config.endurance.minutes
    } else {
        app.config.session.duration_seconds.div_ceil(60)
    };
    match lang {
        Lang::En => format!("{cell} · {language} · {minutes} min"),
        Lang::Ja => format!("{cell}・{language}・{minutes} 分"),
    }
}

fn core_sections(app: &App, resolved: &ResolvedBriefing, step: Option<&MenuStep>, labels: Labels, lang: Lang) -> Vec<Section> {
    let mut goal = vec![resolved.goal.text(lang).to_owned()];
    if let Some(state) = app.menu_live_goal.as_ref() {
        goal.push(match state.goal.target {
            Some(value) => match lang {
                Lang::En => format!("This session's line: {}", crate::ui::live_goal::format_value(state.goal.kind, value)),
                Lang::Ja => format!("今回の線: {}", crate::ui::live_goal::format_value(state.goal.kind, value)),
            },
            None => match lang {
                Lang::En => "This session's line: no baseline (this result becomes the baseline for later sessions)".to_owned(),
                Lang::Ja => "今回の線: 基準なし（この回の値が次回からの基準になります）".to_owned(),
            },
        });
    }

    let mut sections = vec![
        Section {
            label: labels.purpose,
            values: vec![resolved.purpose.text(lang).to_owned()],
            tone: SectionTone::Normal,
        },
        Section {
            label: labels.goal,
            values: goal,
            tone: SectionTone::Normal,
        },
        Section {
            label: labels.content,
            values: vec![resolved.content.text(lang).to_owned()],
            tone: SectionTone::Normal,
        },
        Section {
            label: labels.tips,
            values: resolved.tips.iter().map(|tip| format!("• {}", tip.text(lang))).collect(),
            tone: SectionTone::Accent,
        },
    ];
    if let Some(step) = step {
        sections.push(Section {
            label: labels.focus,
            values: vec![step.focus.clone()],
            tone: SectionTone::Warning,
        });
        let progress = app.menu_step_progress.map(|progress| match lang {
            Lang::En => format!("{} (progress {}/{})", step.advance, progress.consecutive, progress.required),
            Lang::Ja => format!("{}（進み具合 {}/{}）", step.advance, progress.consecutive, progress.required),
        });
        sections.push(Section {
            label: labels.advance,
            values: vec![progress.unwrap_or_else(|| step.advance.clone())],
            tone: SectionTone::Normal,
        });
        sections.push(Section {
            label: labels.reason,
            values: vec![step.reason.clone()],
            tone: SectionTone::Normal,
        });
    }
    sections
}

fn heading_lines(heading: &str, subtitle: Option<&str>, width: usize, theme: Theme) -> Vec<Line<'static>> {
    let mut lines = wrap_display(heading, width)
        .into_iter()
        .map(|text| Line::from(Span::styled(text, Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))))
        .collect::<Vec<_>>();
    if let Some(subtitle) = subtitle {
        lines.extend(
            wrap_display(subtitle, width)
                .into_iter()
                .map(|text| Line::from(Span::styled(text, Style::default().fg(theme.warning).add_modifier(Modifier::BOLD)))),
        );
    }
    lines.push(Line::from(""));
    lines
}

fn section_lines(sections: &[Section], label_width: usize, width: usize, theme: Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for section in sections {
        let style = match section.tone {
            SectionTone::Normal => Style::default().fg(theme.fg),
            SectionTone::Accent => Style::default().fg(theme.accent),
            SectionTone::Warning => Style::default().fg(theme.warning).add_modifier(Modifier::BOLD),
        };
        let mut first = true;
        for value in &section.values {
            for field in wrap_field(section.label, value, label_width, width) {
                let label = if first { field.label } else { " ".repeat(label_width.saturating_add(1)) };
                first = false;
                lines.push(Line::from(vec![
                    Span::styled(label, Style::default().fg(theme.muted)),
                    Span::styled(field.value, style),
                ]));
            }
        }
    }
    lines
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

fn language_label(practice: Lang, display: Lang) -> &'static str {
    match (practice, display) {
        (Lang::En, Lang::En) => "English",
        (Lang::Ja, Lang::En) => "Japanese",
        (Lang::En, Lang::Ja) => "英語",
        (Lang::Ja, Lang::Ja) => "日本語",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_width_wraps_japanese_without_splitting_the_budget() {
        let lines = wrap_display("正確さを一打ずつ確かめる", 10);
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|line| UnicodeWidthStr::width(line.as_str()) <= 10));
        assert_eq!(lines.concat(), "正確さを一打ずつ確かめる");
    }

    #[test]
    fn display_width_wraps_english_on_words_when_possible() {
        assert_eq!(wrap_display("follow the pacer slowly", 10), ["follow the", "pacer", "slowly"]);
    }

    #[test]
    fn display_width_fills_cjk_lines_past_an_earlier_ascii_space() {
        let lines = wrap_display("最近の速さより 10% 遅いペーサーが走ります．", 30);

        assert!(lines[0].contains("遅い"), "CJK text should fill the available cells: {lines:?}");
        assert!(lines.iter().all(|line| UnicodeWidthStr::width(line.as_str()) <= 30));
        assert!(
            lines.iter().skip(1).all(|line| !line.starts_with(['，', '．', '）', '」'])),
            "closing punctuation must not start a wrapped line: {lines:?}"
        );
    }
}
