//! Individual-training screen (MYTASK-3638): a table whose rows are the skill
//! a session trains and whose columns are what its prompts are made of.

use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::config::{DrillCell, DrillSkill, PromptUnit};
use crate::i18n::Lang;
use crate::practice::drill;
use crate::ui::{left_inset, theme::Theme};

/// Display width of the row-label column.
const LABEL_WIDTH: usize = 18;
/// Display width of one table column.
const CELL_WIDTH: usize = 9;

pub fn skill_label(skill: DrillSkill, lang: Lang) -> &'static str {
    match (skill, lang) {
        (DrillSkill::Normal, Lang::En) => "Normal",
        (DrillSkill::Normal, Lang::Ja) => "ふつう",
        (DrillSkill::Accuracy, Lang::En) => "Accuracy",
        (DrillSkill::Accuracy, Lang::Ja) => "正確さ",
        (DrillSkill::Speed, Lang::En) => "Speed",
        (DrillSkill::Speed, Lang::Ja) => "速さ",
        (DrillSkill::Recovery, Lang::En) => "Recovery",
        (DrillSkill::Recovery, Lang::Ja) => "立ち直り",
        (DrillSkill::Blind, Lang::En) => "No-look",
        (DrillSkill::Blind, Lang::Ja) => "キーボードを見ない",
        (DrillSkill::Anticipation, Lang::En) => "Anticipation",
        (DrillSkill::Anticipation, Lang::Ja) => "先走り",
        (DrillSkill::Preview, Lang::En) => "Look-ahead",
        (DrillSkill::Preview, Lang::Ja) => "先読み",
        (DrillSkill::Overlap, Lang::En) => "Rollover",
        (DrillSkill::Overlap, Lang::Ja) => "重なり",
        (DrillSkill::Spelling, Lang::En) => "Spelling",
        (DrillSkill::Spelling, Lang::Ja) => "綴り",
        (DrillSkill::Weak, Lang::En) => "Weak transitions",
        (DrillSkill::Weak, Lang::Ja) => "弱い遷移",
        (DrillSkill::Rhythm, Lang::En) => "Rhythm",
        (DrillSkill::Rhythm, Lang::Ja) => "リズム",
        (DrillSkill::Burst, Lang::En) => "Burst",
        (DrillSkill::Burst, Lang::Ja) => "バースト",
        (DrillSkill::Endurance, Lang::En) => "Endurance",
        (DrillSkill::Endurance, Lang::Ja) => "持久",
        (DrillSkill::Unlock, Lang::En) => "Unlock keys",
        (DrillSkill::Unlock, Lang::Ja) => "キー解放",
    }
}

pub fn unit_label(unit: PromptUnit, lang: Lang) -> &'static str {
    match (unit, lang) {
        (PromptUnit::Char, Lang::En) => "Letters",
        (PromptUnit::Char, Lang::Ja) => "文字",
        (PromptUnit::Ngram, Lang::En) => "Combos",
        (PromptUnit::Ngram, Lang::Ja) => "組合せ",
        (PromptUnit::Word, Lang::En) => "Words",
        (PromptUnit::Word, Lang::Ja) => "単語",
        (PromptUnit::Phrase, Lang::En) => "Phrases",
        (PromptUnit::Phrase, Lang::Ja) => "句",
        (PromptUnit::Sentence, Lang::En) => "Sentence",
        (PromptUnit::Sentence, Lang::Ja) => "短文",
        (PromptUnit::Passage, Lang::En) => "Passage",
        (PromptUnit::Passage, Lang::Ja) => "長文",
    }
}

fn language_name(language: Lang, lang: Lang) -> &'static str {
    match (language, lang) {
        (Lang::En, Lang::En) => "English",
        (Lang::Ja, Lang::En) => "Japanese",
        (Lang::En, Lang::Ja) => "英語",
        (Lang::Ja, Lang::Ja) => "日本語",
    }
}

/// The name of a cell, «正確さ × 単語» / «Accuracy × Words» (the table,
/// and the steps of today's menu, MYTASK-3640).
pub fn cell_label(cell: DrillCell, lang: Lang) -> String {
    format!("{} × {}", skill_label(cell.skill, lang), unit_label(cell.unit, lang))
}

/// Why a `—` cell cannot be started in the practice language `practice`
/// (shown under the table).
pub fn unavailable_reason(cell: DrillCell, practice: Lang, lang: Lang) -> &'static str {
    let ja = lang == Lang::Ja;
    match (cell.skill, cell.unit) {
        (DrillSkill::Spelling, PromptUnit::Word | PromptUnit::Phrase | PromptUnit::Sentence | PromptUnit::Passage) if practice == Lang::En => {
            if ja {
                "選べません: 綴りはローマ字の練習なので日本語だけです（[L] で切替）"
            } else {
                "Not available: spelling practices romaji, Japanese only ([L] to switch)"
            }
        }
        (DrillSkill::Unlock, _) => {
            if ja {
                "選べません: キー解放は解放済みのキーだけで作れる問題に限られます"
            } else {
                "Not available: unlocking needs prompts made of the unlocked keys"
            }
        }
        (DrillSkill::Endurance, PromptUnit::Char | PromptUnit::Ngram) => {
            if ja {
                "選べません: 持久は意味のある文を打ち続ける練習です"
            } else {
                "Not available: endurance keeps going through meaningful text"
            }
        }
        (_, PromptUnit::Passage) => {
            if ja {
                "選べません: 長文は 1 問が長く，この技能には向きません"
            } else {
                "Not available: a passage is too long a prompt for this skill"
            }
        }
        _ => {
            if ja {
                "選べません: ランダムな文字・組み合わせでは，この技能を測れません"
            } else {
                "Not available: random letters or combos do not train this skill"
            }
        }
    }
}

/// Shown when Japanese passages are chosen but none are bundled.
pub fn passages_missing(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Japanese passages are not bundled yet; switch the language with [L]",
        Lang::Ja => "日本語の長文はまだ同梱されていません．[L] で英語に切り替えられます",
    }
}

/// What the skill of a cell trains (the first description line).
pub fn skill_description(app: &App, cell: DrillCell, lang: Lang) -> String {
    let ja = lang == Lang::Ja;
    let text = |en: &str, japanese: &str| if ja { japanese.to_owned() } else { en.to_owned() };
    match cell.skill {
        DrillSkill::Normal => text("Normal: the plain prompts with your own settings", "ふつう: 自分の設定のまま打つ"),
        DrillSkill::Accuracy => text(
            "Accuracy: a typo stops you; runs under 97% accuracy do not rank",
            "正確さ: 誤打で止まる．正確さ 97% 未満の回は順位に載らない",
        ),
        DrillSkill::Speed => text(
            "Speed: chase a pacer 5% faster than your recent speed",
            "速さ: 最近の速さより 5% 速いペーサーを追いかける",
        ),
        DrillSkill::Recovery => text(
            "Recovery: a typo moves on, and the missed prompt comes back",
            "立ち直り: 誤打しても先へ進み，間違えた問題をもう一度打つ",
        ),
        DrillSkill::Blind => text(
            "No-look: keys you know well fade from the on-screen keyboard",
            "キーボードを見ない: 慣れたキーから画面の鍵盤が消えていく",
        ),
        DrillSkill::Anticipation => text(
            "Anticipation: a pacer 10% slower; check each letter before pressing",
            "先走り: 10% 遅いペーサーで，次の文字を確かめてから押す",
        ),
        DrillSkill::Preview => text(
            "Look-ahead: three prompts ahead are shown; read the next one",
            "先読み: 3 問先まで見せる．打っている文字ではなく次を読む",
        ),
        DrillSkill::Overlap => text(
            "Rollover: press the next key before releasing the last; relax",
            "重なり: 前のキーを離す前に次を押してよい．力を抜く",
        ),
        DrillSkill::Spelling => text(
            "Spelling: keep to the romaji spelling the analysis suggests",
            "綴り: 分析が勧めるローマ字の綴りに統一する",
        ),
        DrillSkill::Weak => text(
            "Weak transitions: more of your slow or error-prone key pairs",
            "弱い遷移: 遅い・間違えやすいキーの移り変わりを多く打つ",
        ),
        DrillSkill::Rhythm => text(
            "Rhythm: drift is measured at the ticks; the pendulum is shown at 1/2 or 1/4 speed",
            "リズム: 目盛りが実際の拍．拍とのずれを測り，振り子は 1/2・1/4 などで表示する",
        ),
        DrillSkill::Burst => {
            let items = burst_items_for(app, cell);
            match (cell.unit, ja) {
                (PromptUnit::Phrase, true) => format!("バースト: 句 {items} 個を全力で 3 本．最高値を記録"),
                (PromptUnit::Phrase, false) => format!("Burst: three all-out sprints of {items} phrases; best counts"),
                (PromptUnit::Sentence, true) => format!("バースト: 短文 {items} 個を全力で 3 本．最高値を記録"),
                (PromptUnit::Sentence, false) => format!("Burst: three all-out sprints of {items} sentence(s); best counts"),
                (_, true) => format!("バースト: 単語 {items} 個を全力で 3 本．最高値を記録"),
                (_, false) => format!("Burst: three all-out sprints of {items} words; best counts"),
            }
        }
        DrillSkill::Endurance => {
            let minutes = app.config.endurance.minutes;
            if ja {
                format!("持久: {minutes} 分間打ち続け，前半と後半を比べる")
            } else {
                format!("Endurance: {minutes} minutes non-stop; the halves are compared")
            }
        }
        DrillSkill::Unlock => text(
            "Unlock keys: start with a few keys and add one as you get fast",
            "キー解放: 少ないキーから始め，速くなったら 1 つずつ増やす",
        ),
    }
}

fn burst_items_for(app: &App, cell: DrillCell) -> usize {
    let mut config = app.config.clone();
    config.mode.drill = Some(cell);
    drill::burst_items(&config)
}

/// What the prompts of a cell are (the second description line).
pub fn unit_description(app: &App, cell: DrillCell, lang: Lang) -> String {
    let ja = lang == Lang::Ja;
    let text = |en: &str, japanese: &str| if ja { japanese.to_owned() } else { en.to_owned() };
    let practice = language_name(app.practice_lang, lang);
    let course = app.config.mode.phrase.course.as_str();
    match (cell.unit, cell.skill) {
        (PromptUnit::Char, DrillSkill::Unlock) => text(
            "Prompts: random letters of the unlocked keys (any language)",
            "打つもの: 解放済みのキーのランダムな文字列（英字．言語に依らない）",
        ),
        (PromptUnit::Char, _) => text(
            "Prompts: random letters of the keys you pick (any language)",
            "打つもの: 選んだキーのランダムな文字列（英字．言語に依らない）",
        ),
        (PromptUnit::Ngram, DrillSkill::Weak) => {
            if ja {
                format!("打つもの: 弱い遷移の反復ドリル（{practice}の記録から）")
            } else {
                format!("Prompts: drills of your weak pairs (from your {practice} records)")
            }
        }
        (PromptUnit::Ngram, _) => text(
            "Prompts: 2–3 letter combos of the keys you pick (any language)",
            "打つもの: 選んだキーの 2〜3 文字の組み合わせ（英字．言語に依らない）",
        ),
        (PromptUnit::Word, DrillSkill::Unlock) => text(
            "Prompts: English words of the unlocked keys (any language)",
            "打つもの: 解放済みのキーで書ける英単語（言語に依らない）",
        ),
        (PromptUnit::Word, _) => match app.practice_lang {
            Lang::Ja => text("Prompts: Japanese words, typed in romaji", "打つもの: 日本語の単語（ローマ字で打つ）"),
            Lang::En => text("Prompts: English words", "打つもの: 英単語"),
        },
        (PromptUnit::Phrase, _) => {
            if ja {
                format!("打つもの: {practice}の句（コース: {course}．[C] で変更）")
            } else {
                format!("Prompts: {practice} phrases (course: {course}, [C] to change)")
            }
        }
        (PromptUnit::Sentence, _) => {
            if ja {
                format!("打つもの: {practice}の短文")
            } else {
                format!("Prompts: {practice} sentences")
            }
        }
        (PromptUnit::Passage, _) => match app.practice_lang {
            Lang::Ja => text(
                "Prompts: Japanese passages (one work's sentences in order)",
                "打つもの: 日本語の長文（同じ作品の文を本文の順につなぐ）",
            ),
            Lang::En => text(
                "Prompts: passages of 3–5 English sentences in a row",
                "打つもの: 英語の短文を 3〜5 文続けた長文",
            ),
        },
    }
}

/// `text` padded with spaces to `width` display columns (centered).
fn centered(text: &str, width: usize) -> String {
    let used = text.width();
    if used >= width {
        return text.to_owned();
    }
    let left = (width - used) / 2;
    format!("{}{text}{}", " ".repeat(left), " ".repeat(width - used - left))
}

/// `text` padded with spaces to `width` display columns (left aligned).
fn padded(text: &str, width: usize) -> String {
    let used = text.width();
    format!("{text}{}", " ".repeat(width.saturating_sub(used)))
}

fn table_lines(app: &App, theme: &Theme, lang: Lang) -> Vec<Line<'static>> {
    let cursor = app.drill_cell;
    let mut lines = Vec::new();
    let corner = match lang {
        Lang::En => "Skill \\ Prompts",
        Lang::Ja => "技能＼打つもの",
    };
    let mut header = vec![Span::styled(
        format!("  {}", padded(corner, LABEL_WIDTH)),
        Style::default().fg(theme.muted),
    )];
    for unit in PromptUnit::ALL {
        let style = if unit == cursor.unit {
            Style::default().fg(theme.warning).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.accent)
        };
        header.push(Span::styled(centered(unit_label(unit, lang), CELL_WIDTH), style));
    }
    lines.push(Line::from(header));
    for skill in DrillSkill::ALL {
        let current_row = skill == cursor.skill;
        let mut spans = vec![
            Span::styled(if current_row { "▶ " } else { "  " }, Style::default().fg(theme.accent)),
            Span::styled(
                padded(skill_label(skill, lang), LABEL_WIDTH),
                if current_row {
                    Style::default().fg(theme.warning).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.fg)
                },
            ),
        ];
        for unit in PromptUnit::ALL {
            let cell = DrillCell::new(skill, unit);
            let available = drill::is_available_in(cell, app.practice_lang);
            let mark = if available { "○" } else { "—" };
            let selected = cell == cursor;
            let text = if selected { format!("[{mark}]") } else { mark.to_owned() };
            let style = match (selected, available) {
                (true, true) => Style::default().fg(theme.warning).add_modifier(Modifier::BOLD | Modifier::REVERSED),
                (true, false) => Style::default().fg(theme.muted).add_modifier(Modifier::REVERSED),
                (false, true) => Style::default().fg(theme.fg),
                (false, false) => Style::default().fg(theme.muted).add_modifier(Modifier::DIM),
            };
            spans.push(Span::styled(centered(&text, CELL_WIDTH), style));
        }
        lines.push(Line::from(spans));
    }
    lines
}

pub fn render(frame: &mut Frame, app: &App) {
    let theme = Theme::from_name(&app.config.ui.theme);
    let lang = app.config.general.lang;
    let area = frame.area();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(match lang {
            Lang::En => " INDIVIDUAL TRAINING ",
            Lang::Ja => " 個別の訓練 ",
        })
        .style(Style::default().fg(theme.fg));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    lines.push(match &app.mode_grade_line {
        Some(grade) => Line::from(Span::styled(
            grade.clone(),
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center),
        None => Line::default(),
    });
    let minutes = crate::practice::menu::bundled_catalog()
        .and_then(|catalog| crate::practice::menu::warmup_step(catalog, lang, app.practice_lang))
        .map_or(1, |step| step.minutes);
    let practice = language_name(app.practice_lang, lang);
    let top = match lang {
        Lang::En => format!("[W] {minutes}-minute warm-up    [L] Practice language: {practice}"),
        Lang::Ja => format!("[W] {minutes} 分のウォームアップ    [L] 練習言語: {practice}"),
    };
    lines.push(Line::from(Span::styled(top, Style::default().fg(theme.accent))));
    // 14 rows since MYTASK-3640: the table follows the top line directly so
    // that it, the two description lines and the key guide fit 80×24.
    lines.extend(table_lines(app, &theme, lang));

    let cell = app.drill_cell;
    if drill::is_available_in(cell, app.practice_lang) {
        lines.push(Line::from(Span::styled(
            skill_description(app, cell, lang),
            Style::default().fg(theme.fg),
        )));
        lines.push(Line::from(Span::styled(unit_description(app, cell, lang), Style::default().fg(theme.fg))));
    } else {
        lines.push(Line::from(Span::styled(cell_label(cell, lang), Style::default().fg(theme.muted))));
        lines.push(Line::from(Span::styled(
            unavailable_reason(cell, app.practice_lang, lang).to_owned(),
            Style::default().fg(theme.muted),
        )));
    }
    let warning = app.mode_warning.clone().unwrap_or_default();
    lines.push(Line::from(Span::styled(warning, Style::default().fg(theme.mistype))));

    let preview = match lang {
        Lang::En => format!("[[]/[]] Preview: {}", app.config.session.preview_items),
        Lang::Ja => format!("[[]/[]] 先読み: {}", app.config.session.preview_items),
    };
    let prompt_style = crate::ui::settings::prompt_style_label(&app.config.display.prompt_style, lang);
    let options = match lang {
        Lang::En => format!("{preview}  [Z] Prompt text: {prompt_style}  ({})", app.practice_theme.label(lang)),
        Lang::Ja => format!("{preview}  [Z] 問題文の表示: {prompt_style}  （{}）", app.practice_theme.label(lang)),
    };
    lines.push(Line::from(Span::styled(options, Style::default().fg(theme.accent))));
    let keys = match lang {
        Lang::En => "[←↓↑→/hjkl] Move  [ENTER] Start  [W] Warm-up  [L] Language  [ESC] Back",
        Lang::Ja => "[←↓↑→/hjkl] 升目  [ENTER] 開始  [W] ウォームアップ  [L] 言語  [ESC] 戻る",
    };
    lines.push(Line::from(Span::styled(keys, Style::default().fg(theme.accent))));
    frame.render_widget(Paragraph::new(lines), left_inset(inner));
}
