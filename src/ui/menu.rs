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

    let summary = match (lang, menu.introductory) {
        (Lang::En, true) => format!("{} min · Introductory menu (more data needed)", menu.requested_minutes),
        (Lang::En, false) => format!("{} min · Personalized menu", menu.requested_minutes),
        (Lang::Ja, true) => format!("{} 分・データ不足のため入門メニュー", menu.requested_minutes),
        (Lang::Ja, false) => format!("{} 分・個別メニュー", menu.requested_minutes),
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
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(focus, Style::default().fg(theme.warning).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(instruction),
    ];
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
}

fn detail_lines(step: &MenuStep, lang: Lang, theme: Theme) -> Vec<Line<'static>> {
    let (reason, focus, advance, evidence) = match lang {
        Lang::En => ("Selected because", "Focus", "Graduation condition", "Evidence strength"),
        Lang::Ja => ("選んだ理由", "意識すること", "卒業の条件", "根拠の強さ"),
    };
    vec![
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
        (Lang::En, Lang::Ja) => "英",
        (Lang::Ja, Lang::Ja) => "日",
    };
    let prefix = format!("[{completion}] {} {language} {} — {}", index + 1, role_label(step.role, lang), step.title);
    match (lang, step.level) {
        (Lang::En, Some(level)) => format!("{prefix} · Lv {level} · {} min", step.minutes),
        (Lang::En, None) => format!("{prefix} · {} min", step.minutes),
        (Lang::Ja, Some(level)) => format!("{prefix}・レベル {level}・{} 分", step.minutes),
        (Lang::Ja, None) => format!("{prefix}・{} 分", step.minutes),
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
