//! Interactive keyboard and language settings screen.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect, Size};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui_image::Image;
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::graphics::{GraphicsMode, GraphicsPreference};
use crate::i18n::{Lang, Msg};
use crate::ui::bigtext::{BigTextSize, StyledCharacter, font};
use crate::ui::bigtext_image::{BigTileSpec, image_cell_height};
use crate::ui::{left_inset, theme::Theme};

pub fn render(frame: &mut Frame, app: &App) {
    let theme = Theme::from_name(&app.config.ui.theme);
    let lang = app.config.general.lang;
    let area = frame.area();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", Msg::SettingsTitle.text(lang)))
        .style(Style::default().fg(theme.fg));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(0),
            Constraint::Length(12),
            Constraint::Length(0),
            Constraint::Length(4),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(inner);

    let language = match lang {
        Lang::En => "English",
        Lang::Ja => "日本語",
    };
    let rows = [
        (Msg::SettingLayout, app.config.keyboard.layout.as_str()),
        (Msg::SettingInputLayout, app.config.keyboard.input_layout.as_str()),
        (Msg::SettingGeometry, app.config.keyboard.geometry.as_str()),
        (Msg::SettingShow, app.config.keyboard.show.as_str()),
        (Msg::SettingKeyboardStyle, app.config.keyboard.style.as_str()),
        (Msg::SettingPromptStyle, prompt_style_label(&app.config.display.prompt_style, lang)),
        (Msg::SettingGraphics, graphics_preference_label(app.config.display.graphics)),
        (Msg::SettingLanguage, language),
        (
            Msg::SettingMenuLanguages,
            menu_languages_label(app.config.menu.languages.as_deref(), lang),
        ),
        (
            Msg::SettingErrorMode,
            match app.config.session.error_mode {
                crate::config::ErrorMode::Stop => "stop",
                crate::config::ErrorMode::Advance => "advance",
            },
        ),
        (Msg::SettingFeedback, feedback_label(&app.config.display.feedback, lang)),
        (Msg::SettingBigFont, app.config.display.big_font.as_str()),
    ];
    let label_width = rows.iter().map(|(label, _)| UnicodeWidthStr::width(label.text(lang))).max().unwrap_or(0) + 3;
    let lines = rows
        .iter()
        .enumerate()
        .map(|(index, (label, value))| {
            let selected = index == app.settings_selected;
            Line::from(vec![
                Span::styled(if selected { "▶ " } else { "  " }, Style::default().fg(theme.accent)),
                Span::styled(
                    format!(
                        "{}{}",
                        label.text(lang),
                        " ".repeat(label_width.saturating_sub(UnicodeWidthStr::width(label.text(lang))))
                    ),
                    Style::default().fg(if selected { theme.accent } else { theme.fg }),
                ),
                Span::styled(
                    if index == 11 {
                        format!("‹ {value} ›")
                    } else {
                        format!("< {value} >")
                    },
                    Style::default()
                        .fg(if selected { theme.warning } else { theme.muted })
                        .add_modifier(if selected { Modifier::BOLD } else { Modifier::empty() }),
                ),
            ])
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), left_inset(chunks[1]));
    frame.render_widget(
        Paragraph::new(setting_description(app.settings_selected, lang))
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(theme.muted)),
        left_inset(chunks[3]),
    );
    if app.settings_selected == 11 {
        render_font_sample(frame, app, &theme, left_inset(chunks[5]));
    }
    frame.render_widget(
        Paragraph::new(graphics_status(app, lang)).style(Style::default().fg(theme.muted)),
        left_inset(chunks[4]),
    );
    if let Some(error) = &app.settings_save_error {
        frame.render_widget(
            Paragraph::new(error.clone()).style(Style::default().fg(theme.mistype)),
            left_inset(chunks[5]),
        );
    }
    frame.render_widget(
        Paragraph::new(Msg::SettingsHint.text(lang)).style(Style::default().fg(theme.accent)),
        left_inset(chunks[6]),
    );
}

fn setting_description(index: usize, lang: Lang) -> &'static str {
    match (index, lang) {
        (0, Lang::En) => {
            "Practice layout. It controls the on-screen keyboard, next-key highlight, and key positions in analysis. Choose qwerty, dvorak, colemak, colemak-dh, colemak-dh-ansi, workman, jis-qwerty, or a custom layout in ~/.config/rs-typing-trainer/layouts/. Default: qwerty."
        }
        (0, Lang::Ja) => {
            "練習する配列．画面のキーボード，次のキーの強調，分析のキー位置に使います．qwerty / dvorak / colemak / colemak-dh / colemak-dh-ansi / workman / jis-qwerty と，~/.config/rs-typing-trainer/layouts/ の独自配列から選べます．既定: qwerty．"
        }
        (1, Lang::En) => {
            "OS input layout. same does no remapping. Choose qwerty when the OS remains QWERTY while practicing another layout; key positions are translated. Other choices: dvorak / colemak / colemak-dh / colemak-dh-ansi / workman / jis-qwerty / custom. Default: same."
        }
        (1, Lang::Ja) => {
            "OS が使っている配列．same は読み替えません．OS が QWERTY のまま別の配列を練習するときは qwerty を選びます．ほかに dvorak / colemak / colemak-dh / colemak-dh-ansi / workman / jis-qwerty / 独自配列．既定: same．"
        }
        (2, Lang::En) => {
            "The physical key arrangement. Choose ansi for a US keyboard or jis for a Japanese keyboard. This controls key positions and the on-screen keyboard shape. Default: ansi."
        }
        (2, Lang::Ja) => {
            "キーボードの物理的な並び．ansi（US 配列）か jis（日本語配列）を選べます．キー位置と画面のキーボード形状を決めます．既定: ansi．"
        }
        (3, Lang::En) => {
            "Keyboard visibility. full: always visible. auto: dims and hides learned keys while keeping the next-key highlight. dim: darkens the whole keyboard. hidden: hides it. Default: full."
        }
        (3, Lang::Ja) => {
            "キーボード表示．full: 常に表示．auto: 慣れたキーから薄く・非表示にし，次に打つキーの強調は残します．dim: 全体を暗く．hidden: 表示しません．既定: full．"
        }
        (4, Lang::En) => {
            "Keyboard appearance. keycap: shared outlines and physical key widths. raised: coloured faces with shadows. plain: labels only. Small terminals automatically switch to a simpler style. Default: keycap."
        }
        (4, Lang::Ja) => {
            "キーボードの見た目．keycap: 共有する罫線と実物に近いキー幅．raised: 色付きの面と影による立体表示．plain: 文字だけ．端末が小さいと自動で簡素なスタイルに切り替わります．既定: keycap．"
        }
        (5, Lang::En) => {
            "Large text (large): bitmap glyphs that get smaller automatically on a small terminal (16 → 12 dots, then normal text). Normal text (normal): terminal-size letters in a framed card with wider spacing. Also [Z] on mode select. Default: large."
        }
        (5, Lang::Ja) => {
            "大きな文字（large）は字形で大きく描き，端末が小さいと自動で小さくなる（16→12ドット→通常の文字）．通常の文字（normal）は端末の文字のまま枠で囲み，字間を広げて描く．モード選択の[Z]でも切り替えられる．既定:large．"
        }
        (6, Lang::En) => {
            "Terminal images. auto queries the terminal; kitty, sixel, and iterm2 force a protocol; off keeps text rendering only. Explicit modes still need a cell pixel size. Default: auto."
        }
        (6, Lang::Ja) => {
            "端末画像．auto は端末へ問い合わせ，kitty / sixel / iterm2 は方式を明示し，off は文字だけで描きます．明示した方式にも 1 セルの画素数が必要です．既定: auto．"
        }
        (7, Lang::En) => {
            "The interface language. Choose English or Japanese. This is separate from the practice language used by adaptive and sentence modes. Default: English."
        }
        (7, Lang::Ja) => {
            "画面の表示言語．English（英語）か Japanese（日本語）を選べます．adaptive や sentence で練習する言語とは別です．既定: English．"
        }
        (8, Lang::En) => {
            "Languages practiced by Today's Menu. Choose Same as display (default), English, Japanese, or English + Japanese. The choice applies only to the menu; the interface language stays separate. Default: Same as display."
        }
        (8, Lang::Ja) => {
            "今日のメニューで練習する言語．表示言語と同じ（Same as display，既定）/ English / 日本語（Japanese）/ English + 日本語（English + Japanese）から選べます．画面の表示言語とは別です．既定: 表示言語と同じ．"
        }
        (9, Lang::En) => {
            "How a mistyped character is handled. stop keeps the cursor in place until the correct key is typed. advance accepts the error and moves forward; Backspace can move back. Default: stop."
        }
        (9, Lang::Ja) => {
            "誤入力した文字の扱い．stop は正しいキーを打つまでその位置で止まります．advance は誤入力を記録して先へ進み，Backspace で戻れます．既定: stop．"
        }
        (10, Lang::En) => {
            "Post-session coaching. summary shows the single clearest observation. detailed shows up to three observations. Nothing is shown when the evidence is insufficient. Default: summary."
        }
        (10, Lang::Ja) => {
            "練習後の気づき．summary は最も目立つ内容を 1 行，detailed は最大 3 行を示します．根拠が足りないときは表示しません．既定: summary．"
        }
        (11, Lang::En) => {
            "Large-text font. biz-ud-gothic: bundled outline font. shinonome: crisp integer-scaled bitmap. A configured TTF / OTF / TTC path is also listed. Sample: あア亜Aa. Without terminal images, Shinonome block text is used. Default: biz-ud-gothic."
        }
        (11, Lang::Ja) => {
            "大きな文字の書体．biz-ud-gothic: 同梱の輪郭書体．shinonome: 整数倍のドット字形．設定ファイルに TTF / OTF / TTC のパスがあれば候補に加わります．見本: あア亜Aa．画像を描けない端末では東雲のブロック文字で描きます．既定: biz-ud-gothic．"
        }
        _ => "",
    }
}

fn menu_languages_label(languages: Option<&[Lang]>, lang: Lang) -> &'static str {
    match (languages, lang) {
        (None, Lang::En) => "Same as display",
        (None, Lang::Ja) => "表示言語と同じ",
        (Some([Lang::En]), _) => "English",
        (Some([Lang::Ja]), _) => "日本語",
        (Some([Lang::En, Lang::Ja]) | Some([Lang::Ja, Lang::En]), _) => "English + 日本語",
        _ => "—",
    }
}

fn graphics_preference_label(value: GraphicsPreference) -> &'static str {
    match value {
        GraphicsPreference::Auto => "auto",
        GraphicsPreference::Kitty => "kitty",
        GraphicsPreference::Sixel => "sixel",
        GraphicsPreference::Iterm2 => "iterm2",
        GraphicsPreference::Off => "off",
    }
}

fn graphics_status(app: &App, lang: Lang) -> String {
    let label = match lang {
        Lang::En => "Image",
        Lang::Ja => "画像",
    };
    let status = match (app.graphics.mode, app.graphics.cell_pixels) {
        (GraphicsMode::Off, _) => format!("{label}: off ({})", app.graphics.reason.text(lang)),
        (mode, Some(cell)) => match lang {
            Lang::En => format!("{label}: {mode} (1 cell {}x{} px)", cell.width, cell.height),
            Lang::Ja => format!("{label}: {mode}（1 セル {}×{} px）", cell.width, cell.height),
        },
        (mode, None) => format!("{label}: {mode}"),
    };
    let fallback = app
        .bigtext_images
        .borrow_mut()
        .fallback_reason(&app.config.display.big_font, &app.home_dir)
        .map(str::to_owned);
    match fallback {
        Some(reason) => format!("{status}; font: {reason}"),
        None => status,
    }
}

fn render_font_sample(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let message = match app.config.general.lang {
        Lang::En => "Sample: あア亜Aa — without terminal images, Shinonome block text is used.",
        Lang::Ja => "見本: あア亜Aa — 画像を描けない端末では東雲のブロック文字で描きます．",
    };
    let Some(cell_pixels) = app.graphics.cell_pixels else {
        frame.render_widget(Paragraph::new(message).style(Style::default().fg(theme.warning)), area);
        return;
    };
    if app.graphics.mode == GraphicsMode::Off {
        frame.render_widget(Paragraph::new(message).style(Style::default().fg(theme.warning)), area);
        return;
    }
    let size = BigTextSize::Dot16;
    let Ok(bitmap) = font(size) else {
        frame.render_widget(Paragraph::new(message).style(Style::default().fg(theme.warning)), area);
        return;
    };
    let rows = image_cell_height(size, cell_pixels).min(area.height);
    let characters = "あア亜Aa"
        .chars()
        .map(|character| {
            (
                bitmap.character_width(character),
                StyledCharacter::new(character, Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
            )
        })
        .collect::<Vec<_>>();
    let total_width = characters.iter().map(|(width, _)| width).sum::<u16>();
    let mut x = area.x + area.width.saturating_sub(total_width) / 2;
    let y = area.y + area.height.saturating_sub(rows) / 2;
    let mut images = app.bigtext_images.borrow_mut();
    for (width, character) in &characters {
        let spec = BigTileSpec {
            size,
            cells: Size::new(*width, rows),
            character: *character,
        };
        if images.prepare(&app.config.display.big_font, &app.home_dir, &app.graphics, &spec).is_err() {
            frame.render_widget(Paragraph::new(message).style(Style::default().fg(theme.warning)), area);
            return;
        }
    }
    for (width, character) in characters {
        let spec = BigTileSpec {
            size,
            cells: Size::new(width, rows),
            character,
        };
        let Some(protocol) = images.protocol(&app.config.display.big_font, &app.home_dir, &app.graphics, &spec) else {
            frame.render_widget(Paragraph::new(message).style(Style::default().fg(theme.warning)), area);
            return;
        };
        frame.render_widget(Image::new(protocol), Rect::new(x, y, width, rows));
        x += width;
    }
}

pub fn feedback_label(value: &str, lang: Lang) -> &'static str {
    match (value, lang) {
        ("detailed", Lang::En) => "Detailed (up to 3)",
        ("detailed", Lang::Ja) => "詳しく（最大 3 行）",
        (_, Lang::En) => "Summary (1 line)",
        (_, Lang::Ja) => "まとめ（1 行）",
    }
}

/// Localized label of a `display.prompt_style` value.
pub fn prompt_style_label(value: &str, lang: Lang) -> &'static str {
    match (value, lang) {
        ("normal", Lang::En) => "Normal text",
        ("normal", Lang::Ja) => "通常の文字",
        (_, Lang::En) => "Large text",
        (_, Lang::Ja) => "大きな文字",
    }
}
