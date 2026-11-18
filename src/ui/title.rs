//! Title screen.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect, Size};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui_image::Image;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::i18n::Msg;
use crate::ui::title_image::TitleLogoSpec;
use crate::ui::{left_inset, theme::Theme};

const IMAGE_LOGO_HEIGHT: u16 = 7;
const IMAGE_LOGO_MAX_WIDTH: u16 = 80;
const IMAGE_LAYOUT_MIN_HEIGHT: u16 = 25;
const IMAGE_LAYOUT_MIN_WIDTH: u16 = 52;

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

    let image_logo = prepare_image_logo(app, theme, inner);
    let logo_height = if image_logo.is_some() { IMAGE_LOGO_HEIGHT } else { 1 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(logo_height),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(6),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(7),
        ])
        .split(inner);

    if let Some((font, spec)) = image_logo {
        let logo_area = centered(chunks[1], spec.cells.width);
        let mut images = app.title_images.borrow_mut();
        if let Some(protocol) = images.protocol(&app.graphics, &font, &spec) {
            frame.render_widget(Image::new(protocol), logo_area);
        }
    } else {
        let logo = Paragraph::new(Msg::TitleLogo.text(lang)).style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD));
        frame.render_widget(logo, left_inset(chunks[1]));
    }

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
    let introductory = app.daily_menu.as_ref().is_some_and(|menu| menu.introductory);
    let menu_line = match (lang, introductory) {
        (crate::i18n::Lang::En, true) => format!("Today's menu ({} min) — M to open (introductory: more data needed)", cfg.menu.minutes),
        (crate::i18n::Lang::En, false) => format!("Today's menu ({} min) — M to open", cfg.menu.minutes),
        (crate::i18n::Lang::Ja, true) => format!("今日のメニュー（{} 分）— M で開く（データ不足のため入門メニュー）", cfg.menu.minutes),
        (crate::i18n::Lang::Ja, false) => format!("今日のメニュー（{} 分）— M で開く", cfg.menu.minutes),
    };
    info_lines.push(Line::from(Span::styled(menu_line, Style::default().fg(theme.warning))));
    let info = Paragraph::new(info_lines)
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(theme.muted));
    frame.render_widget(info, left_inset(chunks[7]));
}

fn prepare_image_logo(app: &App, theme: Theme, inner: Rect) -> Option<(crate::ui::bigtext_image::ResolvedBigFont, TitleLogoSpec)> {
    if inner.width < IMAGE_LAYOUT_MIN_WIDTH || inner.height < IMAGE_LAYOUT_MIN_HEIGHT {
        return None;
    }
    let width = inner.width.saturating_sub(8).clamp(48, IMAGE_LOGO_MAX_WIDTH);
    let spec = TitleLogoSpec::from_theme(Size::new(width, IMAGE_LOGO_HEIGHT), theme);
    let font = app.bigtext_images.borrow_mut().resolved_font(&app.config.display.big_font, &app.home_dir);
    app.title_images.borrow_mut().prepare(&app.graphics, &font, &spec).ok()?;
    Some((font, spec))
}

fn centered(area: Rect, width: u16) -> Rect {
    let width = width.min(area.width);
    Rect::new(area.x + area.width.saturating_sub(width) / 2, area.y, width, area.height)
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
