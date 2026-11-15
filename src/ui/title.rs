//! Title screen.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use unicode_segmentation::UnicodeSegmentation;

use crate::app::App;
use crate::i18n::Msg;
use crate::ui::theme::Theme;

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
            Constraint::Length(2),
            Constraint::Length(4),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(6),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(7),
            Constraint::Min(0),
        ])
        .split(inner);

    let logo_lines: Vec<Line> = [
        "╔═══════════════════════════════════════╗".to_string(),
        format!("║ {:^37} ║", Msg::TitleLogo.text(lang)),
        "╚═══════════════════════════════════════╝".to_string(),
    ]
    .into_iter()
    .map(|line| Line::from(Span::styled(line, Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))))
    .collect();
    let logo = Paragraph::new(logo_lines).alignment(Alignment::Center);
    frame.render_widget(logo, chunks[1]);

    let tagline = Paragraph::new(Msg::TitleTagline.text(lang))
        .alignment(Alignment::Center)
        .style(Style::default().fg(theme.fg));
    frame.render_widget(tagline, chunks[3]);

    let actions = Paragraph::new(vec![
        Line::from(format!("  ▶  {}", Msg::TitleStartHint.text(lang))),
        Line::from(match lang {
            crate::i18n::Lang::En => "  ▶  [B] Benchmark",
            crate::i18n::Lang::Ja => "  ▶  [B] ベンチマーク",
        }),
        Line::from(match lang {
            crate::i18n::Lang::En => "  ▶  [A] Analysis",
            crate::i18n::Lang::Ja => "  ▶  [A] 分析",
        }),
        Line::from(format!("  ▶  {}", Msg::TitleHistoryHint.text(lang))),
        Line::from(format!("  ▶  {}", Msg::TitleSettingsHint.text(lang))),
        Line::from(format!("  ▶  {}", Msg::TitleQuitHint.text(lang))),
    ])
    .alignment(Alignment::Center);
    frame.render_widget(actions, chunks[5]);

    let separator = Paragraph::new("─────────────────────────────────────────").alignment(Alignment::Center);
    frame.render_widget(separator.clone(), chunks[7]);

    let cfg = &app.config;
    let mut graphemes = cfg.characters.charset.graphemes(true);
    let mut charset_short: String = graphemes.by_ref().take(40).collect();
    if graphemes.next().is_some() {
        charset_short.push('…');
    }
    let db_path = app.db_path();
    let mut info_lines = vec![
        Line::from(format!("  {:<12}: {}", "Practice", app.practice_theme.label(lang))),
        Line::from(format!("  {:<12}: {charset_short}", Msg::Charset.text(lang))),
        Line::from(format!(
            "  {:<12}: {}s     {} : {}s",
            Msg::Duration.text(lang),
            cfg.game.duration_seconds,
            Msg::WordTimeout.text(lang),
            cfg.game.word_timeout_seconds
        )),
        Line::from(format!(
            "  {:<12}: {}       {} : {}",
            Msg::WordLength.text(lang),
            cfg.game.word_length,
            Msg::Animation.text(lang),
            if cfg.ui.animation_enabled {
                Msg::On.text(lang)
            } else {
                Msg::Off.text(lang)
            }
        )),
        Line::from(format!("  {:<12}: {}", Msg::Database.text(lang), db_path.display())),
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
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(theme.muted));
    frame.render_widget(info, chunks[8]);
}
