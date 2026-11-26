use ratatui::Frame;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::config::BenchSeries;
use crate::i18n::Msg;
use crate::ui::{left_inset, theme::Theme};

pub fn render(frame: &mut Frame, app: &App) {
    let lang = app.config.general.lang;
    let v3 = app.config.bench.series == BenchSeries::V3;
    let theme = Theme::from_name(&app.config.ui.theme);
    let optional = if app.bench_include_d {
        Msg::BenchOptionalOn.text(lang)
    } else {
        Msg::BenchOptionalOff.text(lang)
    };
    let title = if v3 {
        match lang {
            crate::i18n::Lang::En => "BENCHMARK bench-v3",
            crate::i18n::Lang::Ja => "ベンチマーク bench-v3",
        }
    } else {
        Msg::BenchSelectTitle.text(lang)
    };
    let block = Block::default().borders(Borders::ALL).title(format!(" {title} "));
    let inner = block.inner(frame.area());
    frame.render_widget(block, frame.area());
    let start = match lang {
        crate::i18n::Lang::En => "Start benchmark  [Enter]",
        crate::i18n::Lang::Ja => "ベンチマーク開始  [Enter]",
    };
    let resume = if v3 {
        app.bench_v3_resume.as_ref().map(|resume| match lang {
            crate::i18n::Lang::En => format!("Resume from part {} (form {:02})", resume.part.as_str(), resume.parallel_index),
            crate::i18n::Lang::Ja => format!("パート {} から続ける（版 {:02}）", resume.part.as_str(), resume.parallel_index),
        })
    } else {
        app.bench_resume.as_ref().map(|resume| match lang {
            crate::i18n::Lang::En => format!("Resume from part {} (form {:02})", resume.part.as_str(), resume.parallel_index),
            crate::i18n::Lang::Ja => format!("パート {} から続ける（版 {:02}）", resume.part.as_str(), resume.parallel_index),
        })
    };
    let mut items = Vec::new();
    if let Some(resume) = resume.as_deref() {
        items.push(resume);
    }
    if v3 {
        items.push(start);
    } else {
        items.extend([start, optional]);
    }
    let part_lines = if v3 {
        match lang {
            crate::i18n::Lang::En => vec![
                "W Warm-up (15 s, not scored) → EW English words (60 s) → ES English sentences (60 s)",
                "JW Japanese words and phrases (60 s) → JS Japanese sentences (60 s) → PW Pseudo-words (30 s)",
            ],
            crate::i18n::Lang::Ja => vec![
                "W 準備運動（15 秒，採点しません）→ EW 英単語（60 秒）→ ES 英文（60 秒）",
                "JW 日本語の単語と句（60 秒）→ JS 日本語の短文（60 秒）→ PW 擬似語（30 秒）",
            ],
        }
    } else {
        vec![Msg::BenchRequiredParts.text(lang)]
    };
    let mut lines = vec![Line::from("")];
    lines.extend(part_lines.into_iter().map(Line::from));
    lines.push(Line::from(""));
    lines.extend(items.iter().enumerate().map(|(index, label)| {
        let selected = index == app.bench_selected;
        Line::from(vec![
            Span::styled(if selected { "▶  " } else { "   " }, Style::default().fg(theme.accent)),
            Span::styled(
                *label,
                Style::default()
                    .fg(if selected { theme.warning } else { theme.fg })
                    .add_modifier(if selected { Modifier::BOLD } else { Modifier::empty() }),
            ),
        ])
    }));
    lines.push(Line::from(""));
    let hint = if v3 {
        match lang {
            crate::i18n::Lang::En => "[↑/↓/k/j] Select  [ENTER] Run  [ESC] Back",
            crate::i18n::Lang::Ja => "[↑/↓/k/j] 選択  [ENTER] 実行  [ESC] 戻る",
        }
    } else {
        Msg::BenchSelectHint.text(lang)
    };
    lines.push(Line::from(hint).style(Style::default().fg(theme.accent)));
    frame.render_widget(Paragraph::new(lines), left_inset(inner));
}
