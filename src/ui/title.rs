//! Title screen.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::i18n::Msg;
use crate::ui::{left_inset, theme::Theme};

pub fn render(frame: &mut Frame, app: &App) {
    let theme = Theme::from_name(&app.config.ui.theme);
    let lang = app.config.general.lang;
    let area = frame.area();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", Msg::AppTitle.text(lang)))
        .style(Style::default().fg(theme.fg));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(6),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(7),
        ])
        .split(inner);

    let logo = Paragraph::new(Msg::TitleLogo.text(lang)).style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD));
    frame.render_widget(logo, left_inset(chunks[1]));

    let tagline = Paragraph::new(Msg::TitleTagline.text(lang)).style(Style::default().fg(theme.fg));
    frame.render_widget(tagline, left_inset(chunks[2]));

    let labels = match lang {
        crate::i18n::Lang::En => [
            "Start  [Space]",
            "Benchmark  [B]",
            "Analysis  [A]",
            "History  [H]",
            "Settings  [S]",
            "Quit  [Q/Esc]",
        ],
        crate::i18n::Lang::Ja => [
            "開始  [Space]",
            "ベンチマーク  [B]",
            "分析  [A]",
            "履歴  [H]",
            "設定  [S]",
            "終了  [Q/Esc]",
        ],
    };
    let actions = labels
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let selected = index == app.title_selected;
            Line::from(vec![
                Span::styled(if selected { "▶  " } else { "   " }, Style::default().fg(theme.accent)),
                Span::styled(
                    *label,
                    Style::default()
                        .fg(if selected { theme.warning } else { theme.fg })
                        .add_modifier(if selected { Modifier::BOLD } else { Modifier::empty() }),
                ),
            ])
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(actions), left_inset(chunks[4]));

    let separator = Paragraph::new("─────────────────────────────────────────");
    frame.render_widget(separator, left_inset(chunks[6]));

    let cfg = &app.config;
    let mut graphemes = cfg.characters.charset.graphemes(true);
    let mut charset_short: String = graphemes.by_ref().take(40).collect();
    if graphemes.next().is_some() {
        charset_short.push('…');
    }
    let db_path = app.db_path();
    let primary_labels = [
        Msg::Practice.text(lang),
        Msg::Charset.text(lang),
        Msg::Duration.text(lang),
        Msg::WordLength.text(lang),
        Msg::Database.text(lang),
    ];
    let primary_width = primary_labels.iter().map(|label| UnicodeWidthStr::width(*label)).max().unwrap_or(0);
    let secondary_width = [Msg::WordTimeout.text(lang), Msg::Animation.text(lang)]
        .iter()
        .map(|label| UnicodeWidthStr::width(*label))
        .max()
        .unwrap_or(0);
    let paired = |primary_label: &str, primary_value: String, secondary_label: &str, secondary_value: String| {
        format!(
            "{}  {}",
            padded_field(primary_label, &primary_value, primary_width, 8),
            labeled_value(secondary_label, &secondary_value, secondary_width)
        )
    };
    let mut info_lines = vec![
        Line::from(labeled_value(Msg::Practice.text(lang), app.practice_theme.label(lang), primary_width)),
        Line::from(labeled_value(Msg::Charset.text(lang), &charset_short, primary_width)),
        Line::from(paired(
            Msg::Duration.text(lang),
            format!("{}s", cfg.game.duration_seconds),
            Msg::WordTimeout.text(lang),
            format!("{}s", cfg.game.word_timeout_seconds),
        )),
        Line::from(paired(
            Msg::WordLength.text(lang),
            cfg.game.word_length.to_string(),
            Msg::Animation.text(lang),
            (if cfg.ui.animation_enabled {
                Msg::On.text(lang)
            } else {
                Msg::Off.text(lang)
            })
            .to_owned(),
        )),
        Line::from(labeled_value(Msg::Database.text(lang), &db_path.display().to_string(), primary_width)),
    ];
    if app.storage_notice {
        info_lines.push(Line::from(Span::styled(
            Msg::LegacyDatabaseCopied.text(lang),
            Style::default().fg(theme.warning),
        )));
    }
    if app.recommend_preview {
        info_lines.push(Line::from(Span::styled(
            match lang {
                crate::i18n::Lang::En => "Practice with more preview items is recommended.",
                crate::i18n::Lang::Ja => "先読みを増やした練習がおすすめです",
            },
            Style::default().fg(theme.warning),
        )));
    }
    if app.recommend_rhythm {
        info_lines.push(Line::from(Span::styled(
            match lang {
                crate::i18n::Lang::En => "Rhythm practice is recommended.",
                crate::i18n::Lang::Ja => "リズム練習がおすすめです",
            },
            Style::default().fg(theme.warning),
        )));
    }
    let info = Paragraph::new(info_lines)
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(theme.muted));
    frame.render_widget(info, left_inset(chunks[7]));
}

fn labeled_value(label: &str, value: &str, label_width: usize) -> String {
    format!(
        "{}{}: {value}",
        label,
        " ".repeat(label_width.saturating_sub(UnicodeWidthStr::width(label)))
    )
}

fn padded_field(label: &str, value: &str, label_width: usize, value_width: usize) -> String {
    let field = labeled_value(label, value, label_width);
    format!("{field}{}", " ".repeat(value_width.saturating_sub(UnicodeWidthStr::width(value))))
}
