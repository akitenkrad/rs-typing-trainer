//! Full-screen review of the session currently shown on the result screen.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::config::DrillSkill;
use crate::i18n::Lang;
use crate::practice::briefing::{resolve_menu_step, resolve_skill};
use crate::practice::session_review::{ReviewGoal, ReviewInput, ReviewReport, build_review_report};
use crate::ui::{left_inset, theme::Theme, wrap_display};

pub fn build_report(app: &App) -> Option<ReviewReport> {
    let result = app.last_result.as_ref()?;
    let record = app.result_record.as_ref()?;
    let lang = app.config.general.lang;
    let (skill, name, briefing) = if let Some(step) = app
        .daily_menu
        .as_ref()
        .and_then(|menu| app.menu_active_step.and_then(|index| menu.steps.get(index)))
    {
        let skill = crate::practice::menu::step_cell(step).map_or(DrillSkill::Normal, |cell| cell.skill);
        let cell = crate::ui::menu::step_cell_label(step, lang).unwrap_or_else(|| step.title.clone());
        let separator = if lang == Lang::Ja { "・" } else { " · " };
        (skill, format!("{cell}{separator}{}", step.title), resolve_menu_step(step).ok()?)
    } else {
        let cell = result.config.mode.drill;
        let skill = cell.map_or(DrillSkill::Normal, |cell| cell.skill);
        let name = cell.map_or_else(
            || match lang {
                Lang::En => "Normal practice".to_owned(),
                Lang::Ja => "ふつうの練習".to_owned(),
            },
            |cell| crate::ui::mode_select::cell_label(cell, lang),
        );
        (skill, name, resolve_skill(skill).ok()?)
    };
    let geometry = crate::layout::geometry::geometry(&app.config.keyboard.geometry).or_else(|| crate::layout::geometry::geometry("ansi"))?;
    let goal = crate::ui::live_goal::result_goal(app).map(|(state, measure)| ReviewGoal { goal: &state.goal, measure });
    let menu_result_line = crate::ui::result::menu_result_line(app, lang, usize::MAX);
    Some(build_review_report(ReviewInput {
        lang,
        name: &name,
        skill,
        missed: briefing.missed.text(lang),
        met: briefing.met.text(lang),
        tip: briefing.tips.first().map_or("", |tip| tip.text(lang)),
        result,
        comparison: app.result_comparison.clone(),
        record,
        layout: app.practice_layout(),
        geometry: &geometry,
        goal,
        menu_progress: app.menu_step_progress,
        menu_result_line: menu_result_line.as_deref(),
    }))
}

pub fn render(frame: &mut Frame, app: &App) {
    let lang = app.config.general.lang;
    let theme = Theme::from_name(&app.config.ui.theme);
    let title = match lang {
        Lang::En => " REVIEW ",
        Lang::Ja => " 振り返り ",
    };
    let block = Block::default().borders(Borders::ALL).title(title).style(Style::default().fg(theme.fg));
    let inner = left_inset(block.inner(frame.area()));
    frame.render_widget(block, frame.area());
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner);

    let lines = build_report(app).map_or_else(
        || {
            vec![Line::from(Span::styled(
                match lang {
                    Lang::En => "Review data is unavailable.",
                    Lang::Ja => "振り返りのデータがありません．",
                },
                Style::default().fg(theme.muted),
            ))]
        },
        |report| report_lines(&report, theme, usize::from(rows[0].width)),
    );
    let body_lines = lines.len();
    let max_scroll = review_scroll_max(body_lines, usize::from(rows[0].height));
    app.set_review_scroll_max(max_scroll);
    frame.render_widget(Paragraph::new(lines).scroll((app.review_scroll.min(max_scroll), 0)), rows[0]);
    let hint = match lang {
        Lang::En => "[↑/↓ PgUp/PgDn ^U/^D] Scroll  [?] Terms  [D/Esc] Result  [Enter] Continue",
        Lang::Ja => "[↑/↓ PgUp/PgDn ^U/^D] 移動  [?] 用語  [D/Esc] 結果  [Enter] 続ける",
    };
    frame.render_widget(Paragraph::new(hint).style(Style::default().fg(theme.accent)), rows[1]);
}

fn review_scroll_max(body_lines: usize, visible_lines: usize) -> u16 {
    body_lines.saturating_sub(visible_lines).min(usize::from(u16::MAX)) as u16
}

fn report_lines(report: &ReviewReport, theme: Theme, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (index, section) in report.sections.iter().enumerate() {
        if index > 0 {
            lines.push(Line::from(""));
        }
        lines.extend(
            wrap_display(&format!("■ {}", section.title), width)
                .into_iter()
                .map(|text| Line::from(Span::styled(text, Style::default().fg(theme.warning).add_modifier(Modifier::BOLD)))),
        );
        for line in &section.lines {
            // A line's own leading spaces (the «   → advice» under a numbered
            // improvement) are kept as a hanging indent for every wrapped row.
            let body = line.trim_start_matches(' ');
            let indent = 2 + (line.len() - body.len());
            let content_width = width.saturating_sub(indent).max(1);
            let pad = " ".repeat(indent);
            lines.extend(
                wrap_display(body, content_width)
                    .into_iter()
                    .map(|text| Line::from(format!("{pad}{text}"))),
            );
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::practice::session_review::ReviewSection;

    #[test]
    fn scroll_max_is_body_minus_viewport_with_a_zero_floor() {
        assert_eq!(review_scroll_max(30, 20), 10);
        assert_eq!(review_scroll_max(20, 20), 0);
        assert_eq!(review_scroll_max(5, 20), 0);
    }

    #[test]
    fn an_advice_line_keeps_its_indent_on_every_wrapped_row() {
        let report = ReviewReport {
            sections: vec![ReviewSection {
                title: "改善点".to_owned(),
                lines: vec![
                    "1. 最も多い誤り: その他の置換 11 件".to_owned(),
                    "   → 誤打したキーの組（期待キー→押したキー）を結果画面のキーボード図で確かめましょう".to_owned(),
                ],
            }],
            improvements: Vec::new(),
            next_action: String::new(),
        };
        let rows = report_lines(&report, Theme::from_name("default"), 40)
            .into_iter()
            .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect::<String>())
            .collect::<Vec<_>>();
        assert_eq!(rows[1], "  1. 最も多い誤り: その他の置換 11 件");
        assert!(rows[2].starts_with("     → "), "{rows:?}");
        assert!(rows.len() > 3, "{rows:?}");
        for row in &rows[3..] {
            assert!(row.starts_with("     ") && !row.starts_with("      "), "{rows:?}");
        }
    }

    #[test]
    fn trailing_ascii_punctuation_wraps_with_its_word() {
        let rows = wrap_display("Keep anticipation errors below your median for three sessions.", 61);
        assert_eq!(rows, ["Keep anticipation errors below your median for three", "sessions."]);
    }
}
