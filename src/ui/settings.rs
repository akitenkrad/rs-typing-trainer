//! Interactive keyboard and language settings screen.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::i18n::{Lang, Msg};
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
            Constraint::Length(9),
            Constraint::Length(1),
            Constraint::Length(5),
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
        (Msg::SettingLanguage, language),
        (
            Msg::SettingErrorMode,
            match app.config.session.error_mode {
                crate::config::ErrorMode::Stop => "stop",
                crate::config::ErrorMode::Advance => "advance",
            },
        ),
        (Msg::SettingFeedback, feedback_label(&app.config.display.feedback, lang)),
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
                    format!("< {value} >"),
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
    if let Some(error) = &app.settings_save_error {
        frame.render_widget(
            Paragraph::new(error.clone()).style(Style::default().fg(theme.mistype)),
            left_inset(chunks[4]),
        );
    }
    frame.render_widget(
        Paragraph::new(Msg::SettingsHint.text(lang)).style(Style::default().fg(theme.accent)),
        left_inset(chunks[5]),
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
            "The interface language. Choose English or Japanese. This is separate from the practice language used by adaptive and sentence modes. Default: English."
        }
        (6, Lang::Ja) => {
            "画面の表示言語．English（英語）か Japanese（日本語）を選べます．adaptive や sentence で練習する言語とは別です．既定: English．"
        }
        (7, Lang::En) => {
            "How a mistyped character is handled. stop keeps the cursor in place until the correct key is typed. advance accepts the error and moves forward; Backspace can move back. Default: stop."
        }
        (7, Lang::Ja) => {
            "誤入力した文字の扱い．stop は正しいキーを打つまでその位置で止まります．advance は誤入力を記録して先へ進み，Backspace で戻れます．既定: stop．"
        }
        (8, Lang::En) => {
            "Post-session coaching. summary shows the single clearest observation. detailed shows up to three observations. Nothing is shown when the evidence is insufficient. Default: summary."
        }
        (8, Lang::Ja) => {
            "練習後の気づき．summary は最も目立つ内容を 1 行，detailed は最大 3 行を示します．根拠が足りないときは表示しません．既定: summary．"
        }
        _ => "",
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
