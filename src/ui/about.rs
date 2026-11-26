//! Version, runtime, storage, and bundled-data information.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::build_info;
use crate::i18n::Lang;
use crate::ui::{left_inset, theme::Theme, wrap_display, wrap_field};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AboutInfo {
    pub description: String,
    pub version: String,
    pub git_hash: String,
    pub build_date: String,
    pub target: String,
    pub profile: String,
    pub license: String,
    pub repository: String,
    pub menu_version: String,
    pub briefing_version: String,
    pub grade_version: String,
    pub bench_series: String,
    pub config_path: Option<String>,
    pub database_path: String,
    pub graphics: String,
    pub release_events_supported: bool,
}

impl AboutInfo {
    pub fn from_app(app: &App) -> Self {
        let menu_version = crate::practice::menu::bundled_catalog()
            .map(|catalog| catalog.version.clone())
            .unwrap_or_else(|_| "unknown".to_owned());
        let briefing_version = crate::practice::briefing::bundled()
            .map(|catalog| catalog.version().to_owned())
            .unwrap_or_else(|_| "unknown".to_owned());
        Self {
            description: env!("CARGO_PKG_DESCRIPTION").to_owned(),
            version: build_info::VERSION.to_owned(),
            git_hash: build_info::GIT_HASH.to_owned(),
            build_date: build_info::build_date(),
            target: build_info::TARGET.to_owned(),
            profile: build_info::PROFILE.to_owned(),
            license: env!("CARGO_PKG_LICENSE").to_owned(),
            repository: env!("CARGO_PKG_REPOSITORY").to_owned(),
            menu_version,
            briefing_version,
            grade_version: crate::analysis::grades::GRADE_VERSION.to_owned(),
            bench_series: crate::bench::BENCH_SERIES.to_owned(),
            config_path: app.config_path.as_ref().map(|path| path.display().to_string()),
            database_path: app.db_path().display().to_string(),
            graphics: app.graphics.mode.as_str().to_owned(),
            release_events_supported: app.release_events_supported,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AboutRow {
    Field { label: String, value: String },
    Text(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AboutSection {
    pub title: String,
    pub rows: Vec<AboutRow>,
}

/// Build the screen's semantic rows without depending on terminal geometry or
/// rendering state.
pub fn build_sections(info: &AboutInfo, lang: Lang) -> Vec<AboutSection> {
    let field = |label: &str, value: String| AboutRow::Field {
        label: label.to_owned(),
        value,
    };
    let build = match lang {
        Lang::En => format!("{} · {}", info.target, info.profile),
        Lang::Ja => format!("{}・{}", info.target, info.profile),
    };
    let config_path = info.config_path.clone().unwrap_or_else(|| {
        match lang {
            Lang::En => "default (none)",
            Lang::Ja => "既定（なし）",
        }
        .to_owned()
    });
    let release_events = match (lang, info.release_events_supported) {
        (Lang::En, true) => "available (rollover and key hold times are measured)",
        (Lang::En, false) => "unavailable (this terminal cannot measure rollover or key hold times)",
        (Lang::Ja, true) => "取れる（重なりと押下時間を測れます）",
        (Lang::Ja, false) => "取れない（この端末では重なりと押下時間を測りません）",
    };

    match lang {
        Lang::En => vec![
            AboutSection {
                title: "rs-typing-trainer".to_owned(),
                rows: vec![
                    field("Description", info.description.clone()),
                    field("Version", info.version.clone()),
                    field("Commit", info.git_hash.clone()),
                    field("Build date", info.build_date.clone()),
                    field("Build", build),
                    field("License", info.license.clone()),
                    field("Repository", info.repository.clone()),
                ],
            },
            AboutSection {
                title: "Internal data versions".to_owned(),
                rows: vec![
                    field("Menu", info.menu_version.clone()),
                    field("Briefings", info.briefing_version.clone()),
                    field("Grade criteria", info.grade_version.clone()),
                    field("Benchmark", info.bench_series.clone()),
                ],
            },
            AboutSection {
                title: "Storage".to_owned(),
                rows: vec![field("Config file", config_path), field("Database", info.database_path.clone())],
            },
            AboutSection {
                title: "Terminal".to_owned(),
                rows: vec![
                    field("Graphics", info.graphics.clone()),
                    field("Key-release timing", release_events.to_owned()),
                ],
            },
            AboutSection {
                title: "Bundled data sources".to_owned(),
                rows: vec![
                    AboutRow::Text("Aozora Bunko — Japanese words, sentences, and passages from 560 public-domain works".to_owned()),
                    AboutRow::Text("Tatoeba — English phrases, sentences, and word frequencies (CC0)".to_owned()),
                    AboutRow::Text("30 public-domain English books — English word frequencies".to_owned()),
                    AboutRow::Text("SCOWL — selection of English and American spellings".to_owned()),
                    AboutRow::Text("lindera / IPADIC — readings for Japanese data".to_owned()),
                    AboutRow::Text("Original Japanese phrases — MIT, like this repository".to_owned()),
                    AboutRow::Text("Shinonome 0.9.11 font — public domain".to_owned()),
                    AboutRow::Text("Full copyright and permission notices are in the repository's data/NOTICE.".to_owned()),
                ],
            },
        ],
        Lang::Ja => vec![
            AboutSection {
                title: "rs-typing-trainer".to_owned(),
                rows: vec![
                    field("説明", "端末で動くタイピング練習アプリ．打鍵を細かく記録して分析する".to_owned()),
                    field("バージョン", info.version.clone()),
                    field("コミット", info.git_hash.clone()),
                    field("ビルド日", info.build_date.clone()),
                    field("ビルド", build),
                    field("ライセンス", info.license.clone()),
                    field("リポジトリ", info.repository.clone()),
                ],
            },
            AboutSection {
                title: "内部データの版".to_owned(),
                rows: vec![
                    field("メニュー", info.menu_version.clone()),
                    field("訓練の説明", info.briefing_version.clone()),
                    field("段階の基準", info.grade_version.clone()),
                    field("ベンチ", info.bench_series.clone()),
                ],
            },
            AboutSection {
                title: "保存先".to_owned(),
                rows: vec![field("設定ファイル", config_path), field("データベース", info.database_path.clone())],
            },
            AboutSection {
                title: "端末".to_owned(),
                rows: vec![
                    field("画像の表示", info.graphics.clone()),
                    field("キーを離した時刻", release_events.to_owned()),
                ],
            },
            AboutSection {
                title: "同梱データの出典".to_owned(),
                rows: vec![
                    AboutRow::Text("青空文庫 — 著作権の切れた560作品から日本語の単語・短文・長文を収録".to_owned()),
                    AboutRow::Text("Tatoeba — 英語句・英文・英単語頻度（CC0）".to_owned()),
                    AboutRow::Text("パブリックドメインの英語書籍30冊 — 英単語頻度".to_owned()),
                    AboutRow::Text("SCOWL — 英米綴りの英単語選定".to_owned()),
                    AboutRow::Text("lindera / IPADIC — 日本語データの読み".to_owned()),
                    AboutRow::Text("日本語句 — 本プロジェクトの書き下ろし，リポジトリと同じ MIT".to_owned()),
                    AboutRow::Text("東雲フォント0.9.11 — パブリックドメイン".to_owned()),
                    AboutRow::Text("著作権表示と許諾文の全文はリポジトリの data/NOTICE にあります．".to_owned()),
                ],
            },
        ],
    }
}

pub fn render(frame: &mut Frame, app: &App) {
    let lang = app.config.general.lang;
    let theme = Theme::from_name(&app.config.ui.theme);
    let title = match lang {
        Lang::En => " ABOUT ",
        Lang::Ja => " このアプリについて ",
    };
    let block = Block::default().borders(Borders::ALL).title(title).style(Style::default().fg(theme.fg));
    let inner = left_inset(block.inner(frame.area()));
    frame.render_widget(block, frame.area());
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner);

    let sections = build_sections(&AboutInfo::from_app(app), lang);
    let lines = section_lines(&sections, theme, usize::from(areas[0].width));
    let max_scroll = lines.len().saturating_sub(usize::from(areas[0].height)).min(usize::from(u16::MAX)) as u16;
    app.set_about_scroll_max(max_scroll);
    frame.render_widget(Paragraph::new(lines).scroll((app.about_scroll.min(max_scroll), 0)), areas[0]);

    let hint = match lang {
        Lang::En => "[↑/↓] Scroll    [Esc] Back    [G] Glossary",
        Lang::Ja => "[↑/↓] スクロール    [Esc] 戻る    [G] 用語集",
    };
    frame.render_widget(Paragraph::new(hint).style(Style::default().fg(theme.accent)), areas[1]);
}

fn section_lines(sections: &[AboutSection], theme: Theme, width: usize) -> Vec<Line<'static>> {
    let label_width = sections
        .iter()
        .flat_map(|section| &section.rows)
        .filter_map(|row| match row {
            AboutRow::Field { label, .. } => Some(UnicodeWidthStr::width(label.as_str())),
            AboutRow::Text(_) => None,
        })
        .max()
        .unwrap_or(0);
    let mut lines = Vec::new();
    for (index, section) in sections.iter().enumerate() {
        if index > 0 {
            lines.push(Line::from(""));
        }
        lines.extend(
            wrap_display(&format!("■ {}", section.title), width)
                .into_iter()
                .map(|text| Line::from(Span::styled(text, Style::default().fg(theme.warning).add_modifier(Modifier::BOLD)))),
        );
        for row in &section.rows {
            match row {
                AboutRow::Field { label, value } => {
                    lines.extend(wrap_field(label, value, label_width, width.saturating_sub(2)).into_iter().map(|row| {
                        Line::from(vec![
                            Span::raw("  "),
                            Span::styled(row.label, Style::default().fg(theme.muted)),
                            Span::styled(row.value, Style::default().fg(theme.fg)),
                        ])
                    }));
                }
                AboutRow::Text(text) => {
                    lines.extend(
                        wrap_display(text, width.saturating_sub(2))
                            .into_iter()
                            .map(|text| Line::from(format!("  {text}"))),
                    );
                }
            }
        }
    }
    lines
}
