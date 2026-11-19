//! Training-mode selection screen.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, MODE_ENTRIES, ModeEntry};
use crate::config::ModeKind;
use crate::i18n::Lang;
use crate::ui::{left_inset, theme::Theme};

const MODES: [(ModeEntry, &str, &str); 12] = [
    // The warm-up's length comes from the menu catalog; see `warmup_label`.
    (ModeEntry::Warmup, "", ""),
    (
        ModeEntry::Mode(ModeKind::Random),
        "random — random character groups",
        "random — ランダム文字列",
    ),
    (ModeEntry::Mode(ModeKind::Words), "words — English words", "words — 英単語"),
    (ModeEntry::Mode(ModeKind::Romaji), "romaji — Japanese words", "romaji — 日本語の単語"),
    (ModeEntry::Mode(ModeKind::Sentence), "sentence — complete sentences", "sentence — 短文"),
    (ModeEntry::Mode(ModeKind::Phrase), "phrase — Phrases", "phrase — 句"),
    (
        ModeEntry::Mode(ModeKind::Ngram),
        "ngram — repeated transitions",
        "ngram — 文字の組み合わせ",
    ),
    (
        ModeEntry::Mode(ModeKind::Adaptive),
        "adaptive — focus weak transitions",
        "adaptive — 弱い遷移を重点練習",
    ),
    (
        ModeEntry::Mode(ModeKind::Unlock),
        "unlock — gradually add keys",
        "unlock — キーを段階的に解放",
    ),
    (ModeEntry::Mode(ModeKind::Rhythm), "rhythm — metronome timing", "rhythm — メトロノーム"),
    (
        ModeEntry::Mode(ModeKind::Burst),
        "burst — three all-out short sprints",
        "burst — 短距離を全力で 3 本",
    ),
    (
        ModeEntry::Mode(ModeKind::Endurance),
        "endurance — sustained sentences",
        "endurance — 短文の持久練習",
    ),
];

/// The warm-up row, with the length of the catalog's warm-up (MYTASK-3629).
fn warmup_label(lang: Lang) -> String {
    let minutes = crate::practice::menu::bundled_catalog()
        .and_then(|catalog| crate::practice::menu::warmup_step(catalog, lang, lang))
        .map_or(1, |step| step.minutes);
    match lang {
        Lang::En => format!("warmup — {minutes}-minute warm-up"),
        Lang::Ja => format!("warmup — {minutes} 分のウォームアップ"),
    }
}

pub fn render(frame: &mut Frame, app: &App) {
    let theme = Theme::from_name(&app.config.ui.theme);
    let area = frame.area();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(match app.config.general.lang {
            Lang::En => " MODE SELECT ",
            Lang::Ja => " モード選択 ",
        })
        .style(Style::default().fg(theme.fg));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Fill(1), Constraint::Length(13), Constraint::Length(4), Constraint::Fill(1)])
        .split(inner);
    if let Some(grade) = &app.mode_grade_line {
        frame.render_widget(
            Paragraph::new(grade.clone())
                .alignment(ratatui::layout::Alignment::Center)
                .style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
            chunks[0],
        );
    }
    debug_assert!(MODES.iter().map(|(entry, _, _)| *entry).eq(MODE_ENTRIES));
    let lines = MODES
        .iter()
        .enumerate()
        .map(|(index, (entry, en, ja))| {
            let selected = index == app.mode_selected;
            let label = if *entry == ModeEntry::Warmup {
                warmup_label(app.config.general.lang)
            } else {
                match app.config.general.lang {
                    Lang::En => (*en).to_owned(),
                    Lang::Ja => (*ja).to_owned(),
                }
            };
            Line::from(vec![
                Span::styled(if selected { "▶ " } else { "  " }, Style::default().fg(theme.accent)),
                Span::styled(
                    label,
                    Style::default()
                        .fg(if selected { theme.warning } else { theme.fg })
                        .add_modifier(if selected { Modifier::BOLD } else { Modifier::empty() }),
                ),
            ])
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), left_inset(chunks[1]));
    let selected_entry = app.selected_mode_entry();
    let selected_mode = selected_entry.mode();
    let pacer_available = matches!(
        selected_mode,
        Some(ModeKind::Words | ModeKind::Sentence | ModeKind::Phrase | ModeKind::Adaptive)
    );
    let pacer = if pacer_available {
        match (app.config.general.lang, app.config.pacer.enabled) {
            (Lang::En, true) => "[P] Pacer: ON",
            (Lang::En, false) => "[P] Pacer: OFF",
            (Lang::Ja, true) => "[P] ペーサー: ON",
            (Lang::Ja, false) => "[P] ペーサー: OFF",
        }
    } else {
        ""
    };
    let practice_language = match selected_entry {
        ModeEntry::Mode(ModeKind::Adaptive) => Some(app.config.adaptive.lang.to_uppercase()),
        ModeEntry::Warmup => Some(
            match app.warmup_language {
                Lang::En => "EN",
                Lang::Ja => "JA",
            }
            .to_owned(),
        ),
        ModeEntry::Mode(_) => None,
    };
    let adaptive_language = practice_language
        .map(|language| match app.config.general.lang {
            Lang::En => format!("[L] Practice language: {language}"),
            Lang::Ja => format!("[L] 練習言語: {language}"),
        })
        .unwrap_or_default();
    let phrase_options = if selected_mode == Some(ModeKind::Phrase) {
        match app.config.general.lang {
            Lang::En => format!(
                "[L] Language: {}  [C] Course: {}",
                app.config.mode.phrase.lang.to_uppercase(),
                app.config.mode.phrase.course
            ),
            Lang::Ja => format!(
                "[L] 言語: {}  [C] コース: {}",
                app.config.mode.phrase.lang.to_uppercase(),
                app.config.mode.phrase.course
            ),
        }
    } else {
        String::new()
    };
    let preview = match app.config.general.lang {
        Lang::En => format!("[[]/[]] Preview: {}", app.config.session.preview_items),
        Lang::Ja => format!("[[]/[]] 先読み: {}", app.config.session.preview_items),
    };
    let prompt_style = crate::ui::settings::prompt_style_label(&app.config.display.prompt_style, app.config.general.lang);
    let options = [pacer, adaptive_language.as_str(), phrase_options.as_str(), preview.as_str()]
        .into_iter()
        .filter(|option| !option.is_empty())
        .collect::<Vec<_>>()
        .join("  ");
    let hint = match app.config.general.lang {
        Lang::En => format!(
            "{}\n{options}\n[Z] Prompt text: {prompt_style}\n[↑/↓/k/j] Select  [ENTER] Continue  [ESC] Back",
            app.practice_theme.label(app.config.general.lang),
        ),
        Lang::Ja => format!(
            "{}\n{options}\n[Z] 問題文の表示: {prompt_style}\n[↑/↓/k/j] 選択  [ENTER] 次へ  [ESC] 戻る",
            app.practice_theme.label(app.config.general.lang),
        ),
    };
    frame.render_widget(Paragraph::new(hint).style(Style::default().fg(theme.accent)), left_inset(chunks[2]));
}
