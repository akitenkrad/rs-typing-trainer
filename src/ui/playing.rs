//! Playing screen — stats, keyboard, item timer, centred prompt, and footer.

use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::App;
use crate::game::word::{ExitKind, WordState};
use crate::i18n::Msg;
use crate::layout::geometry::{Finger, geometry};
use crate::ui::bigtext::{BigTextSize, StyledCharacter, font, window_by_widths, window_range};
use crate::ui::keyboard::{KeyFeedback, KeyboardState, KeyboardStyle, KeyboardView, UnlockKeyboardState, resolve_style};
use crate::ui::theme::Theme;

/// How long the mistype flash effect lingers on screen.
const MISTYPE_FLASH_DURATION: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptLayoutStage {
    XlargeSelected,
    LargeSelected,
    LargeSimplified,
    LargeHidden,
    NormalKeyboard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayingLayout {
    pub stage: PromptLayoutStage,
    pub bigtext: Option<BigTextSize>,
    pub keyboard_style: Option<KeyboardStyle>,
    pub prompt_rows: u16,
    pub show_timer: bool,
}

fn mistype_flash_active(app: &App) -> bool {
    let Some(game) = app.game.as_ref() else {
        return false;
    };
    let Some(at) = game.last_mistype_flash else {
        return false;
    };
    display_now(game).saturating_duration_since(at) < MISTYPE_FLASH_DURATION
}

pub fn render(frame: &mut Frame, app: &App) {
    render_with_footer(frame, app, None);
}

/// Render the ordinary playing screen, optionally replacing its final inner
/// row. Replay uses this to add controls without maintaining a second screen.
pub fn render_with_footer(frame: &mut Frame, app: &App, footer: Option<Line<'static>>) {
    let theme = Theme::from_name(&app.config.ui.theme);
    let lang = app.config.general.lang;
    let area = frame.area();

    let flashing = app.active_bench_id.is_none() && !app.config.session.blind && mistype_flash_active(app);
    let (border_style, title) = if flashing {
        (
            Style::default().fg(theme.mistype).add_modifier(Modifier::BOLD),
            format!(" {}  ✗ {} ", Msg::AppTitle.text(lang), Msg::MistypeTitle.text(lang)),
        )
    } else {
        (Style::default().fg(theme.fg), format!(" {} ", Msg::AppTitle.text(lang)))
    };

    let outer = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(border_style)
        .style(Style::default().fg(theme.fg));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let layout = resolve_playing_layout(app, inner);
    let show_focus = app.config.mode.kind == crate::config::ModeKind::Adaptive && !app.adaptive_focus.is_empty();
    let show_rhythm = app.config.mode.kind == crate::config::ModeKind::Rhythm;
    let unlock_rows = u16::from(app.config.mode.kind == crate::config::ModeKind::Unlock) * 2;
    let keyboard_rows = layout
        .keyboard_style
        .map_or(0, |style| style.height(app.config.keyboard.finger_guide) + unlock_rows);

    let mut constraints = vec![
        Constraint::Length(1), // top stats
    ];
    if show_focus {
        constraints.push(Constraint::Length(1));
    }
    if show_rhythm {
        constraints.push(Constraint::Length(1));
    }
    // Order chosen by the user: keyboard on top, then the item timer, the
    // metrics, and the prompt in the lower area. The prompt starts on the row
    // right below the metrics and takes every spare row, so its content is
    // top-aligned there rather than vertically centred.
    if layout.keyboard_style.is_some() {
        constraints.push(Constraint::Length(keyboard_rows));
    }
    if layout.show_timer {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Length(1)); // combo, errors, accuracy
    constraints.push(Constraint::Min(layout.prompt_rows));
    constraints.push(Constraint::Length(1)); // footer
    let chunks = Layout::default().direction(Direction::Vertical).constraints(constraints).split(inner);

    let mut index = 0;
    render_top_bar(frame, app, &theme, chunks[index]);
    index += 1;
    if show_focus {
        render_adaptive_focus(frame, app, &theme, chunks[index]);
        index += 1;
    }
    if show_rhythm {
        render_rhythm(frame, app, &theme, chunks[index]);
        index += 1;
    }
    if let Some(keyboard_style) = layout.keyboard_style {
        render_keyboard(frame, app, chunks[index], keyboard_style);
        index += 1;
    }
    if layout.show_timer {
        render_word_timer(frame, app, &theme, chunks[index]);
        index += 1;
    }
    if let Some(game) = app.game.as_ref() {
        render_sub_stats(frame, game, &theme, chunks[index], lang);
    }
    index += 1;
    render_word(frame, app, &theme, chunks[index], layout.bigtext);
    index += 1;
    let footer_index = index;
    if let Some(footer) = footer {
        frame.render_widget(
            Paragraph::new(footer)
                .alignment(Alignment::Center)
                .style(Style::default().fg(theme.muted)),
            chunks[footer_index],
        );
    } else {
        render_footer(frame, app, &theme, chunks[footer_index]);
    }
}

pub fn resolve_playing_layout(app: &App, inner: Rect) -> PlayingLayout {
    let show_focus = app.config.mode.kind == crate::config::ModeKind::Adaptive && !app.adaptive_focus.is_empty();
    let show_rhythm = app.config.mode.kind == crate::config::ModeKind::Rhythm;
    let base_rows = 3 + u16::from(show_focus) + u16::from(show_rhythm);
    let keyboard_requested = app.active_bench_id.is_some() || app.config.keyboard.show != "hidden";
    let requested_style = KeyboardStyle::parse(&app.config.keyboard.style);
    let unlock_rows = u16::from(app.config.mode.kind == crate::config::ModeKind::Unlock) * 2;
    let keyboard_height = |style: Option<KeyboardStyle>| style.map_or(0, |style| style.height(app.config.keyboard.finger_guide) + unlock_rows);
    let fits = |size: Option<BigTextSize>, style: Option<KeyboardStyle>| {
        let prompt_rows = prompt_rows(app, size, inner.width);
        let width_ok = style.is_none_or(|style| style.width() <= inner.width);
        width_ok && base_rows.saturating_add(prompt_rows).saturating_add(keyboard_height(style)) <= inner.height
    };
    let selected = keyboard_requested.then_some(requested_style);
    let mut candidates = Vec::new();
    if matches!(app.config.display.prompt_size.as_str(), "auto" | "xlarge") {
        candidates.push((PromptLayoutStage::XlargeSelected, Some(BigTextSize::Dot16), selected));
    }
    if app.config.display.prompt_size != "normal" {
        candidates.push((PromptLayoutStage::LargeSelected, Some(BigTextSize::Dot12), selected));
        if keyboard_requested {
            let simpler: &[KeyboardStyle] = match requested_style {
                KeyboardStyle::Keycap => &[KeyboardStyle::Raised, KeyboardStyle::Plain],
                KeyboardStyle::Raised => &[KeyboardStyle::Plain],
                KeyboardStyle::Plain => &[],
            };
            candidates.extend(
                simpler
                    .iter()
                    .copied()
                    .map(|style| (PromptLayoutStage::LargeSimplified, Some(BigTextSize::Dot12), Some(style))),
            );
        }
        candidates.push((PromptLayoutStage::LargeHidden, Some(BigTextSize::Dot12), None));
    }
    for (stage, bigtext, keyboard_style) in candidates {
        if fits(bigtext, keyboard_style) {
            let used = base_rows + prompt_rows(app, bigtext, inner.width) + keyboard_height(keyboard_style);
            return PlayingLayout {
                stage,
                bigtext,
                keyboard_style,
                prompt_rows: prompt_rows(app, bigtext, inner.width),
                show_timer: timer_requested(app) && used < inner.height,
            };
        }
    }

    let normal_rows = prompt_rows(app, None, inner.width);
    let keyboard_room = inner.height.saturating_sub(base_rows + normal_rows + unlock_rows);
    let keyboard_style = keyboard_requested
        .then(|| {
            resolve_style(
                &app.config.keyboard.style,
                Rect::new(inner.x, inner.y, inner.width, keyboard_room),
                app.config.keyboard.finger_guide,
            )
        })
        .flatten();
    let used = base_rows + normal_rows + keyboard_height(keyboard_style);
    PlayingLayout {
        stage: PromptLayoutStage::NormalKeyboard,
        bigtext: None,
        keyboard_style,
        prompt_rows: normal_rows.min(inner.height.saturating_sub(base_rows)),
        show_timer: timer_requested(app) && used < inner.height,
    }
}

fn timer_requested(app: &App) -> bool {
    !matches!(
        app.config.mode.kind,
        crate::config::ModeKind::Sentence | crate::config::ModeKind::Endurance
    ) && !app.config.session.item_timeout().is_zero()
}

fn prompt_rows(app: &App, size: Option<BigTextSize>, _width: u16) -> u16 {
    let has_progress = app.config.pacer.target_wpm.is_some() || app.game.as_ref().is_some_and(|game| game.ghost.is_some());
    let japanese = app.game.as_ref().is_some_and(|game| game.current_prompt.reading.is_some());
    let sentence = matches!(
        app.config.mode.kind,
        crate::config::ModeKind::Sentence | crate::config::ModeKind::Endurance
    );
    let romaji =
        matches!(app.config.mode.kind, crate::config::ModeKind::Romaji) || app.config.mode.kind == crate::config::ModeKind::Adaptive && japanese;
    let Some(size) = size else {
        // Word: header, letters, caret. Romaji: surface, kana, romaji.
        // Sentence: at least one text row, its caret and one more row; the
        // prompt area also takes every spare row (see `render_with_footer`).
        return 3 + u16::from(has_progress);
    };
    let height = size.cell_height();
    if romaji {
        2 * height + 4 + u16::from(has_progress)
    } else if sentence && japanese {
        2 * height + 7 + u16::from(has_progress)
    } else if sentence {
        height + 4 + u16::from(has_progress)
    } else {
        height + 1 + u16::from(has_progress)
    }
}

fn display_now(game: &crate::app::GameState) -> Instant {
    game.started_at.checked_add(game.elapsed).unwrap_or(game.started_at)
}

fn render_rhythm(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let Some(game) = app.game.as_ref() else { return };
    let bpm = app.rhythm_bpm.unwrap_or(180.0);
    let period = 60.0 / bpm.max(f64::EPSILON);
    let phase = (game.elapsed.as_secs_f64() % period) / period;
    let beat = if phase < 0.2 { "●" } else { "○" };
    let text = match app.config.general.lang {
        crate::i18n::Lang::En => format!("{beat} Beat  {bpm:.0} BPM"),
        crate::i18n::Lang::Ja => format!("{beat} 拍  {bpm:.0} BPM"),
    };
    frame.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.warning).add_modifier(Modifier::BOLD)),
        area,
    );
}

fn render_adaptive_focus(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let label = match app.config.general.lang {
        crate::i18n::Lang::En => "Today's focus",
        crate::i18n::Lang::Ja => "今日の重点",
    };
    let transitions = app
        .adaptive_focus
        .iter()
        .take(5)
        .map(|row| format!("{}→{}", row.from, row.to))
        .collect::<Vec<_>>()
        .join("  ");
    frame.render_widget(
        Paragraph::new(format!("{label}: {transitions}"))
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.warning).add_modifier(Modifier::BOLD)),
        area,
    );
}

fn render_keyboard(frame: &mut Frame, app: &App, area: Rect, keyboard_style: KeyboardStyle) {
    let Some(game) = app.game.as_ref() else { return };
    let Some(mut geometry) = geometry(&app.config.keyboard.geometry) else {
        return;
    };
    if app.config.keyboard.digit6_finger == "right-index" {
        geometry = geometry.with_digit6_finger(Finger::RightIndex);
    }
    let layout = app.practice_layout();
    let next = game
        .session
        .expected()
        .and_then(|expected| expected.chars().next())
        .and_then(|expected| layout.stroke_for(expected));
    let last = game
        .last_key_flash
        .filter(|at| display_now(game).saturating_duration_since(*at) < Duration::from_millis(150))
        .and_then(|_| {
            game.last_pressed_code.as_ref().map(|pressed| KeyFeedback {
                pressed_code: pressed.clone(),
                expected_code: game.last_expected_code.clone(),
                correct: game.last_key_correct,
            })
        });
    let state = KeyboardState {
        next,
        last,
        heatmap: Default::default(),
        fade: app.keyboard_fade.clone(),
        unlock: app.unlock_state.as_ref().map(|unlock| {
            let unlocked_codes = unlock
                .unlocked
                .chars()
                .filter_map(|character| layout.stroke_for(character).map(|stroke| stroke.code))
                .collect();
            let next_code = crate::practice::unlock::unlock_sequence(layout, &geometry, app.config.unlock.order)
                .into_iter()
                .find(|character| !unlock.unlocked.contains(*character))
                .and_then(|character| layout.stroke_for(character).map(|stroke| stroke.code));
            UnlockKeyboardState { unlocked_codes, next_code }
        }),
    };
    let keyboard_area = if app.config.mode.kind == crate::config::ModeKind::Unlock && area.height >= 3 {
        Rect::new(area.x, area.y, area.width, area.height - 2)
    } else {
        area
    };
    crate::ui::keyboard::render(
        frame,
        keyboard_area,
        &geometry,
        layout,
        &state,
        KeyboardView {
            show: if app.active_bench_id.is_some() {
                "full"
            } else {
                &app.config.keyboard.show
            },
            style: keyboard_style.as_str(),
            finger_guide: app.config.keyboard.finger_guide,
        },
    );
    if app.config.mode.kind == crate::config::ModeKind::Unlock && area.height >= 3 {
        let streak = app.unlock_state.as_ref().map_or(0, |state| state.streak);
        let labels = match app.config.general.lang {
            crate::i18n::Lang::En => (" Unlocked  ", " Next  ", " Locked"),
            crate::i18n::Lang::Ja => (" 解放済み  ", " 次に解放  ", " 未解放"),
        };
        let legend = Line::from(vec![
            Span::styled("■", Style::default().fg(Color::Black).bg(Color::DarkGray)),
            Span::raw(labels.0),
            Span::styled("■", Style::default().fg(Color::Black).bg(Color::Yellow)),
            Span::raw(labels.1),
            Span::styled("□", Style::default().fg(Color::DarkGray)),
            Span::raw(labels.2),
        ]);
        let progress = match app.config.general.lang {
            crate::i18n::Lang::En => format!("Next unlock: {streak} / 3 (accuracy 97%+ · {:.0} WPM+)", app.config.unlock.target_wpm),
            crate::i18n::Lang::Ja => format!(
                "次の解放まで: {streak} / 3 回（正確さ 97% 以上・{:.0} WPM 以上）",
                app.config.unlock.target_wpm
            ),
        };
        frame.render_widget(
            Paragraph::new(legend).alignment(Alignment::Center),
            Rect::new(area.x, area.bottom() - 2, area.width, 1),
        );
        frame.render_widget(
            Paragraph::new(progress).alignment(Alignment::Center),
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
        );
    }
}

fn render_top_bar(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let Some(game) = app.game.as_ref() else { return };
    let elapsed = game.elapsed;
    let total = if app.config.mode.kind == crate::config::ModeKind::Endurance {
        Duration::from_secs(app.config.endurance.minutes * 60)
    } else {
        app.config.session.duration()
    };
    let remaining = total.saturating_sub(elapsed);
    let secs = elapsed.as_secs_f64().max(0.001);
    let wpm = (game.metrics.correct_chars as f64 / 5.0) / (secs / 60.0);

    let mut spans = vec![
        Span::styled(
            format!(" {}: ", Msg::Time.text(app.config.general.lang)),
            Style::default().fg(theme.muted),
        ),
        Span::styled(
            format!(
                "{:02}:{:02} / {:02}:{:02}",
                elapsed.as_secs() / 60,
                elapsed.as_secs() % 60,
                total.as_secs() / 60,
                total.as_secs() % 60,
            ),
            Style::default().fg(if remaining.as_secs() < 10 { theme.warning } else { theme.fg }),
        ),
        Span::raw("     "),
        Span::styled(format!("{}: ", Msg::Wpm.text(app.config.general.lang)), Style::default().fg(theme.muted)),
        Span::styled(format!("{:5.1}", wpm), Style::default().fg(theme.accent)),
        Span::raw("     "),
        Span::styled(
            format!("{}: ", Msg::Words.text(app.config.general.lang)),
            Style::default().fg(theme.muted),
        ),
        Span::styled(format!("{}", game.metrics.completed_words), Style::default().fg(theme.fg)),
    ];
    if let Some(target) = app.config.pacer.target_wpm.filter(|_| app.active_bench_id.is_none()) {
        spans.push(Span::raw("     "));
        spans.push(Span::styled(
            match app.config.general.lang {
                crate::i18n::Lang::En => format!("Pace: {target:.0} WPM"),
                crate::i18n::Lang::Ja => format!("ペース: {target:.0} WPM"),
            },
            Style::default().fg(theme.warning),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_word(frame: &mut Frame, app: &App, theme: &Theme, area: Rect, bigtext: Option<BigTextSize>) {
    let Some(game) = app.game.as_ref() else { return };

    if game.ime_warning {
        let warning = match app.config.general.lang {
            crate::i18n::Lang::En => "Please turn IME off.",
            crate::i18n::Lang::Ja => "IME をオフにしてください．",
        };
        frame.render_widget(
            Paragraph::new(warning)
                .alignment(Alignment::Center)
                .style(Style::default().fg(theme.mistype).add_modifier(Modifier::BOLD)),
            top_aligned(area, 1),
        );
        return;
    }
    if let Some(size) = bigtext
        && let Ok(bitmap_font) = font(size)
    {
        // Keep one blank column on each side so a window that fills the whole
        // width never touches the outer border. Every large window is sized
        // from this inset width, so no glyph is drawn outside it.
        let area = Rect::new(area.x.saturating_add(1), area.y, area.width.saturating_sub(2), area.height);
        match app.config.mode.kind {
            crate::config::ModeKind::Romaji => render_big_japanese(frame, app, theme, area, bitmap_font, false),
            crate::config::ModeKind::Adaptive if game.current_prompt.reading.is_some() => {
                render_big_japanese(frame, app, theme, area, bitmap_font, false)
            }
            crate::config::ModeKind::Sentence | crate::config::ModeKind::Endurance if game.current_prompt.reading.is_some() => {
                render_big_japanese(frame, app, theme, area, bitmap_font, true)
            }
            crate::config::ModeKind::Sentence | crate::config::ModeKind::Endurance => render_big_sentence(frame, app, theme, area, bitmap_font),
            _ => render_big_word(frame, app, theme, area, bitmap_font),
        }
        return;
    }
    match app.config.mode.kind {
        crate::config::ModeKind::Romaji => {
            render_romaji_word(frame, app, theme, area);
            return;
        }
        crate::config::ModeKind::Adaptive if game.current_prompt.reading.is_some() => {
            render_romaji_word(frame, app, theme, area);
            return;
        }
        crate::config::ModeKind::Sentence | crate::config::ModeKind::Endurance => {
            if game.current_prompt.reading.is_some() {
                render_japanese_prompt(frame, app, theme, area);
            } else {
                render_sentence(frame, app, theme, area);
            }
            return;
        }
        _ => {}
    }

    let blind = app.active_bench_id.is_none() && app.config.session.blind;
    let flashing = !blind && mistype_flash_active(app);

    // Spans per grapheme with status colour.
    let mut spans: Vec<Span> = Vec::new();
    for (i, g) in game.current_word.word.graphemes.iter().enumerate() {
        let style = if blind {
            Style::default().fg(theme.muted)
        } else if i < game.typed_index {
            Style::default().fg(theme.correct).add_modifier(Modifier::BOLD)
        } else if i == game.typed_index {
            // Current cursor — flash inverted red on mistype, otherwise underline.
            if flashing {
                Style::default()
                    .fg(theme.fg)
                    .bg(theme.mistype)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(theme.fg).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            }
        } else {
            Style::default().fg(theme.muted)
        };
        spans.push(Span::styled(format!(" {} ", g), style));
    }

    let header = match &game.current_word.state {
        WordState::Entering { .. } => Span::styled(
            format!(" ▷▷▷  ({})  ▷▷▷", Msg::Locking.text(app.config.general.lang)),
            Style::default().fg(theme.muted),
        ),
        WordState::Active if blind => Span::raw(""),
        WordState::Active => {
            if flashing {
                let typed = game.last_mistype_char.as_deref().unwrap_or("?");
                let expected = game.last_mistype_expected.as_deref().unwrap_or("?");
                Span::styled(
                    format!(
                        "  ✗ {}  {} '{}'  {} '{}'",
                        Msg::Mistype.text(app.config.general.lang),
                        Msg::Got.text(app.config.general.lang),
                        typed,
                        Msg::Expected.text(app.config.general.lang),
                        expected
                    ),
                    Style::default().fg(theme.mistype).add_modifier(Modifier::BOLD),
                )
            } else if game.mistypes_in_word > 0 {
                Span::styled(
                    format!("  ✗ {}: {}", Msg::MistypesInWord.text(app.config.general.lang), game.mistypes_in_word),
                    Style::default().fg(theme.warning),
                )
            } else {
                Span::raw("")
            }
        }
        WordState::Exiting { kind, .. } => match kind {
            ExitKind::Correct => Span::styled(
                format!("  ✓ {}", Msg::Correct.text(app.config.general.lang)),
                Style::default().fg(theme.correct),
            ),
            ExitKind::Mistyped => Span::styled(
                format!("  ✗ {}", Msg::CompletedWithMistypes.text(app.config.general.lang)),
                Style::default().fg(theme.warning),
            ),
            ExitKind::Timeout => Span::styled(
                format!("  ⌛ {}", Msg::Timeout.text(app.config.general.lang)),
                Style::default().fg(theme.mistype),
            ),
        },
    };

    // Caret indicator — turns into ✗ in mistype color while flashing.
    let caret_pos = game.typed_index.min(game.current_word.word.len().saturating_sub(1));
    let mut caret_line = String::new();
    let caret_glyph = if blind {
        "   "
    } else if flashing {
        " ✗ "
    } else {
        " ▲ "
    };
    for i in 0..game.current_word.word.len() {
        caret_line.push_str(if i == caret_pos { caret_glyph } else { "   " });
    }
    let caret_color = if flashing { theme.mistype } else { theme.accent };

    let mut lines = vec![Line::from(header)];
    if let Some(markers) = progress_markers(app, 3) {
        lines.push(markers);
    }
    lines.extend([
        Line::from(spans),
        Line::from(Span::styled(
            caret_line,
            Style::default()
                .fg(caret_color)
                .add_modifier(if flashing { Modifier::BOLD } else { Modifier::empty() }),
        )),
    ]);
    let content_area = top_aligned(area, lines.len() as u16);
    let para = Paragraph::new(lines).alignment(Alignment::Center);
    frame.render_widget(para, content_area);
}

fn progress_style(index: usize, current: usize, blind: bool, flashing: bool, theme: &Theme) -> (Style, bool) {
    if index < current && blind {
        (Style::default(), false)
    } else if index < current {
        (Style::default().fg(theme.correct).add_modifier(Modifier::BOLD), true)
    } else if index == current && flashing {
        (
            Style::default()
                .fg(theme.fg)
                .bg(theme.mistype)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
            true,
        )
    } else if index == current {
        (Style::default().fg(theme.warning).add_modifier(Modifier::BOLD), true)
    } else {
        (Style::default().fg(theme.muted), true)
    }
}

fn styled_window(text: &str, range: std::ops::Range<usize>, current: usize, blind: bool, flashing: bool, theme: &Theme) -> Vec<StyledCharacter> {
    text.chars()
        .enumerate()
        .skip(range.start)
        .take(range.len())
        .map(|(index, character)| {
            let (style, visible) = progress_style(index, current, blind, flashing, theme);
            if visible {
                StyledCharacter::new(character, style)
            } else {
                StyledCharacter::hidden(character, style)
            }
        })
        .collect()
}

fn render_centered_line(frame: &mut Frame, area: Rect, y: u16, line: Line<'static>) {
    if y < area.height {
        frame.render_widget(
            Paragraph::new(line).alignment(Alignment::Center),
            Rect::new(area.x, area.y + y, area.width, 1),
        );
    }
}

fn render_centered_lines(frame: &mut Frame, area: Rect, y: u16, lines: Vec<Line<'static>>) {
    if y >= area.height {
        return;
    }
    let height = (lines.len() as u16).min(area.height - y);
    frame.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center),
        Rect::new(area.x, area.y + y, area.width, height),
    );
}

fn big_status_line(app: &App, theme: &Theme) -> Option<Line<'static>> {
    let game = app.game.as_ref()?;
    let WordState::Exiting { kind, .. } = &game.current_word.state else {
        return None;
    };
    let (message, color) = match kind {
        ExitKind::Correct => (format!("✓ {}", Msg::Correct.text(app.config.general.lang)), theme.correct),
        ExitKind::Mistyped => (format!("✗ {}", Msg::CompletedWithMistypes.text(app.config.general.lang)), theme.warning),
        ExitKind::Timeout => (format!("⌛ {}", Msg::Timeout.text(app.config.general.lang)), theme.mistype),
    };
    Some(Line::from(Span::styled(message, Style::default().fg(color).add_modifier(Modifier::BOLD))))
}

fn big_marker_line(
    app: &App,
    theme: &Theme,
    rendered: &crate::ui::bigtext::RenderedBigText,
    range: &std::ops::Range<usize>,
) -> Option<Line<'static>> {
    let (pacer, ghost) = progress_positions(app)?;
    let mut markers = Vec::new();
    if let Some(position) = pacer
        && range.start <= position
        && position <= range.end
    {
        markers.push((
            position.saturating_sub(range.start).min(range.len()),
            '▾',
            Style::default().fg(theme.warning),
        ));
    }
    if let Some(position) = ghost
        && range.start <= position
        && position <= range.end
    {
        let relative = position.saturating_sub(range.start).min(range.len());
        if let Some(marker) = markers.iter_mut().find(|marker| marker.0 == relative) {
            marker.1 = '◇';
        } else {
            markers.push((relative, '◆', Style::default().fg(theme.accent)));
        }
    }
    Some(rendered.marker_line(&markers))
}

fn render_big_word(frame: &mut Frame, app: &App, theme: &Theme, area: Rect, bitmap_font: &crate::ui::bigtext::BitmapFont) {
    let Some(game) = app.game.as_ref() else { return };
    let text = game.current_prompt.target.as_str();
    let count = text.chars().count();
    if count == 0 {
        return;
    }
    let current = game.typed_index.min(count - 1);
    let range = window_range(bitmap_font, text, current, area.width);
    let blind = app.active_bench_id.is_none() && app.config.session.blind;
    let flashing = !blind && mistype_flash_active(app);
    let characters = styled_window(text, range.clone(), game.typed_index, blind, flashing, theme);
    let rendered = bitmap_font.render(&characters);
    let mut y = 0;
    if let Some(markers) = big_marker_line(app, theme, &rendered, &range) {
        render_centered_line(frame, area, y, markers);
        y += 1;
    }
    render_centered_lines(frame, area, y, rendered.lines.clone());
    y += rendered.lines.len() as u16;
    let cursor = current.saturating_sub(range.start);
    let cursor_style = Style::default()
        .fg(if flashing { theme.mistype } else { theme.accent })
        .add_modifier(Modifier::BOLD);
    render_centered_line(
        frame,
        area,
        y,
        big_status_line(app, theme).unwrap_or_else(|| rendered.cursor_line(cursor, cursor_style)),
    );
}

fn render_big_sentence(frame: &mut Frame, app: &App, theme: &Theme, area: Rect, bitmap_font: &crate::ui::bigtext::BitmapFont) {
    let Some(game) = app.game.as_ref() else { return };
    let text = game.current_prompt.text.as_str();
    let count = text.chars().count();
    if count == 0 {
        return;
    }
    let current = game.typed_index.min(count - 1);
    let range = window_range(bitmap_font, text, current, area.width);
    let blind = app.active_bench_id.is_none() && app.config.session.blind;
    let flashing = !blind && mistype_flash_active(app);
    let rendered = bitmap_font.render(&styled_window(text, range.clone(), game.typed_index, blind, flashing, theme));
    let mut y = 0;
    if let Some(markers) = big_marker_line(app, theme, &rendered, &range) {
        render_centered_line(frame, area, y, markers);
        y += 1;
    }
    render_centered_lines(frame, area, y, rendered.lines.clone());
    y += rendered.lines.len() as u16;
    render_centered_line(
        frame,
        area,
        y,
        big_status_line(app, theme).unwrap_or_else(|| {
            rendered.cursor_line(
                current.saturating_sub(range.start),
                Style::default()
                    .fg(if flashing { theme.mistype } else { theme.accent })
                    .add_modifier(Modifier::BOLD),
            )
        }),
    );
    y += 1;
    if y < area.height {
        render_sentence(frame, app, theme, Rect::new(area.x, area.y + y, area.width, area.height - y));
    }
}

fn render_big_japanese(frame: &mut Frame, app: &App, theme: &Theme, area: Rect, bitmap_font: &crate::ui::bigtext::BitmapFont, sentence: bool) {
    let Some(game) = app.game.as_ref() else { return };
    let (surface_label, reading_label, romaji_label) = match app.config.general.lang {
        crate::i18n::Lang::En => ("Surface", "Kana", "Romaji"),
        crate::i18n::Lang::Ja => ("表記", "かな", "ローマ字"),
    };
    let surface = game.current_prompt.text.as_str();
    let reading = game.current_prompt.reading.as_deref().unwrap_or_default();
    let guide = game.session.current_guide();
    let completed_reading = game.session.completed_reading_chars();
    let surface_count = surface.chars().count();
    let surface_current = if surface_count == 0 {
        0
    } else if sentence && !reading.is_empty() {
        completed_reading.saturating_mul(surface_count) / reading.chars().count().max(1)
    } else {
        0
    }
    .min(surface_count.saturating_sub(1));
    let surface_range = window_range(bitmap_font, surface, surface_current, area.width);
    let surface_rendered = bitmap_font.render(
        &surface
            .chars()
            .skip(surface_range.start)
            .take(surface_range.len())
            .map(|character| StyledCharacter::new(character, Style::default().fg(theme.fg).add_modifier(Modifier::BOLD)))
            .collect::<Vec<_>>(),
    );
    let mut y = 0;
    render_centered_line(
        frame,
        area,
        y,
        Line::from(Span::styled(format!("{surface_label}:"), Style::default().fg(theme.muted))),
    );
    y += 1;
    let surface_height = surface_rendered.lines.len() as u16;
    render_centered_lines(frame, area, y, surface_rendered.lines);
    y += surface_height;
    let blind = app.active_bench_id.is_none() && app.config.session.blind;
    // A long reading does not fit on one row; show the part around the kana
    // being typed, like the large windows above and below it.
    let reading_widths = reading
        .chars()
        .map(|character| UnicodeWidthChar::width(character).unwrap_or(0) as u16)
        .collect::<Vec<_>>();
    let label_width = UnicodeWidthStr::width(format!("{reading_label}: ").as_str()) as u16;
    let reading_range = window_by_widths(&reading_widths, completed_reading, area.width.saturating_sub(label_width));
    let reading_spans = reading
        .chars()
        .enumerate()
        .skip(reading_range.start)
        .take(reading_range.len())
        .map(|(index, character)| {
            let color = if blind {
                theme.muted
            } else if index < completed_reading {
                theme.correct
            } else if index == completed_reading {
                theme.warning
            } else {
                theme.muted
            };
            Span::styled(character.to_string(), Style::default().fg(color).add_modifier(Modifier::BOLD))
        })
        .collect::<Vec<_>>();
    render_centered_line(
        frame,
        area,
        y,
        Line::from(
            std::iter::once(Span::styled(format!("{reading_label}: "), Style::default().fg(theme.muted)))
                .chain(reading_spans)
                .collect::<Vec<_>>(),
        ),
    );
    y += 1;
    render_centered_line(
        frame,
        area,
        y,
        Line::from(Span::styled(format!("{romaji_label}:"), Style::default().fg(theme.muted))),
    );
    y += 1;

    let guide_count = guide.chars().count();
    if guide_count > 0 {
        let current = game.typed_index.min(guide_count - 1);
        let range = window_range(bitmap_font, &guide, current, area.width);
        let flashing = !blind && mistype_flash_active(app);
        let rendered = bitmap_font.render(&styled_window(&guide, range.clone(), game.typed_index, blind, flashing, theme));
        if let Some(markers) = big_marker_line(app, theme, &rendered, &range) {
            render_centered_line(frame, area, y, markers);
            y += 1;
        }
        render_centered_lines(frame, area, y, rendered.lines.clone());
        y += rendered.lines.len() as u16;
        render_centered_line(
            frame,
            area,
            y,
            big_status_line(app, theme).unwrap_or_else(|| {
                rendered.cursor_line(
                    current.saturating_sub(range.start),
                    Style::default()
                        .fg(if flashing { theme.mistype } else { theme.accent })
                        .add_modifier(Modifier::BOLD),
                )
            }),
        );
        y += 1;
    }
    if sentence && y < area.height {
        render_japanese_prompt(frame, app, theme, Rect::new(area.x, area.y + y, area.width, area.height - y));
    }
}

fn render_romaji_word(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    render_japanese_prompt(frame, app, theme, area);
}

fn render_japanese_prompt(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let Some(game) = app.game.as_ref() else { return };
    let (surface_label, reading_label, romaji_label) = match app.config.general.lang {
        crate::i18n::Lang::En => ("Surface", "Kana", "Romaji"),
        crate::i18n::Lang::Ja => ("表記", "かな", "ローマ字"),
    };
    let guide = game.session.current_guide();
    let completed_reading_chars = game.session.completed_reading_chars();
    let reading = game.current_prompt.reading.clone().unwrap_or_default();
    let blind = app.active_bench_id.is_none() && app.config.session.blind;
    let label_style = Style::default().fg(theme.muted);
    let progress = |index: usize, current: usize| {
        if blind {
            Style::default().fg(theme.muted)
        } else if index < current {
            Style::default().fg(theme.correct).add_modifier(Modifier::BOLD)
        } else if index == current {
            Style::default().fg(theme.warning).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(theme.muted)
        }
    };

    let mut surface = text_cells(&format!("{surface_label}: "), |_| label_style);
    let surface_text = game.current_prompt.text.as_str();
    let surface_count = surface_text.chars().count();
    // The surface has no cursor of its own; follow the kana proportionally so
    // a long wrapped sentence keeps the row that is being typed.
    let surface_cursor = surface.len() + completed_reading_chars.saturating_mul(surface_count) / reading.chars().count().max(1);
    surface.extend(text_cells(surface_text, |_| Style::default().fg(theme.fg)));
    if let WordState::Exiting { kind, .. } = &game.current_word.state {
        let (message, color) = match kind {
            ExitKind::Correct => (Msg::Correct.text(app.config.general.lang), theme.correct),
            ExitKind::Mistyped => (Msg::CompletedWithMistypes.text(app.config.general.lang), theme.warning),
            ExitKind::Timeout => (Msg::Timeout.text(app.config.general.lang), theme.mistype),
        };
        surface.extend(text_cells(&format!("    {message}"), |_| Style::default().fg(color)));
    }
    let mut kana = text_cells(&format!("{reading_label}: "), |_| label_style);
    let kana_cursor = kana.len() + completed_reading_chars;
    kana.extend(text_cells(&reading, |index| progress(index, completed_reading_chars)));
    let mut romaji = text_cells(&format!("{romaji_label}: "), |_| label_style);
    let romaji_cursor = romaji.len() + game.typed_index;
    romaji.extend(text_cells(&guide, |index| progress(index, game.typed_index)));

    let markers = progress_markers(app, 1);
    let width = usize::from(area.width.max(1));
    let blocks = [(surface, surface_cursor), (kana, kana_cursor), (romaji, romaji_cursor)].map(|(cells, cursor)| {
        let lines = wrap_cells(&cells, width);
        let cursor_line = cursor_line_index(&lines, cursor);
        (lines, cursor_line)
    });
    let room = usize::from(area.height).saturating_sub(usize::from(markers.is_some()));
    let ranges = fit_blocks(&blocks.iter().map(|(lines, cursor)| (lines.len(), *cursor)).collect::<Vec<_>>(), room);
    let mut lines = Vec::new();
    let mut content_width = 1;
    for ((block, _), range) in blocks.iter().zip(ranges) {
        for line in &block[range] {
            content_width = content_width.max(line.iter().map(|(text, _)| UnicodeWidthStr::width(text.as_str())).sum::<usize>());
            lines.push(Line::from(
                line.iter().map(|(text, style)| Span::styled(text.clone(), *style)).collect::<Vec<_>>(),
            ));
        }
    }
    let desired_height = lines.len() as u16 + u16::from(markers.is_some());
    let content_area = top_centered_rect(area, content_width as u16, desired_height);
    let mut text_area = content_area;
    if let Some(markers) = markers
        && content_area.height > 0
    {
        frame.render_widget(Paragraph::new(markers), Rect::new(content_area.x, content_area.y, content_area.width, 1));
        text_area = Rect::new(content_area.x, content_area.y + 1, content_area.width, content_area.height - 1);
    }
    frame.render_widget(Paragraph::new(lines), text_area);
}

type StyledCell = (String, Style);

fn text_cells(text: &str, style: impl Fn(usize) -> Style) -> Vec<StyledCell> {
    text.chars()
        .enumerate()
        .map(|(index, character)| (character.to_string(), style(index)))
        .collect()
}

/// Wraps cells by display width, character by character. Japanese has no
/// spaces to break at, so word wrapping would push a whole sentence onto the
/// next row and leave its label alone.
fn wrap_cells(cells: &[StyledCell], width: usize) -> Vec<Vec<StyledCell>> {
    let mut lines = vec![Vec::new()];
    let mut used = 0;
    for cell in cells {
        let cell_width = UnicodeWidthStr::width(cell.0.as_str());
        if used + cell_width > width && used > 0 {
            lines.push(Vec::new());
            used = 0;
        }
        lines.last_mut().expect("at least one line").push(cell.clone());
        used += cell_width;
    }
    lines
}

fn cursor_line_index(lines: &[Vec<StyledCell>], cursor: usize) -> usize {
    let mut seen = 0;
    for (index, line) in lines.iter().enumerate() {
        seen += line.len();
        if cursor < seen {
            return index;
        }
    }
    lines.len().saturating_sub(1)
}

/// Chooses which wrapped rows of each block to show in `room` rows: every row
/// when they fit, otherwise each block's cursor row first and then the rows
/// around it, one block at a time.
fn fit_blocks(blocks: &[(usize, usize)], room: usize) -> Vec<std::ops::Range<usize>> {
    let total = blocks.iter().map(|(lines, _)| lines).sum::<usize>();
    if total <= room {
        return blocks.iter().map(|(lines, _)| 0..*lines).collect();
    }
    let mut budget = room;
    let mut ranges = blocks
        .iter()
        .map(|(_, cursor)| {
            if budget > 0 {
                budget -= 1;
                *cursor..*cursor + 1
            } else {
                *cursor..*cursor
            }
        })
        .collect::<Vec<_>>();
    while budget > 0 {
        let mut changed = false;
        for (range, (lines, _)) in ranges.iter_mut().zip(blocks) {
            if budget == 0 {
                break;
            }
            if range.end < *lines && range.start < range.end {
                range.end += 1;
            } else if range.start > 0 && range.start < range.end {
                range.start -= 1;
            } else {
                continue;
            }
            budget -= 1;
            changed = true;
        }
        if !changed {
            break;
        }
    }
    ranges
}

fn render_sentence(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let Some(game) = app.game.as_ref() else { return };
    let markers = progress_markers(app, 1);
    let content_width = area.width.clamp(1, 72);
    let width = usize::from(content_width);
    let graphemes = game.current_prompt.text.graphemes(true).map(str::to_owned).collect::<Vec<_>>();
    let display_start = if app.config.mode.sentence.hide_typed {
        game.typed_index.min(graphemes.len())
    } else {
        0
    };
    let visible = &graphemes[display_start..];
    let cursor = if game.typed_index < graphemes.len() {
        game.typed_index.saturating_sub(display_start)
    } else {
        visible.len().saturating_sub(1)
    };
    let flashing = mistype_flash_active(app);
    let ranges = wrap_graphemes(visible, width);
    let mut lines = Vec::with_capacity(ranges.len() + 1);
    let mut caret_row = None;
    for (start, end) in ranges {
        let spans = visible[start..end]
            .iter()
            .enumerate()
            .map(|(offset, grapheme)| {
                let visible_index = start + offset;
                let absolute_index = display_start + visible_index;
                let style = if absolute_index < game.typed_index {
                    Style::default().fg(theme.correct).add_modifier(Modifier::BOLD)
                } else if absolute_index == game.typed_index {
                    if flashing {
                        Style::default()
                            .fg(theme.fg)
                            .bg(theme.mistype)
                            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
                    } else {
                        Style::default().fg(theme.fg).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                    }
                } else {
                    Style::default().fg(theme.muted)
                };
                Span::styled(grapheme.clone(), style)
            })
            .collect::<Vec<_>>();
        lines.push(Line::from(spans));
        if cursor >= start && cursor < end {
            let column = visible[start..cursor]
                .iter()
                .map(|grapheme| UnicodeWidthStr::width(grapheme.as_str()))
                .sum::<usize>();
            let glyph = if flashing { "✗" } else { "▲" };
            caret_row = Some(lines.len());
            let line_width = visible[start..end]
                .iter()
                .map(|grapheme| UnicodeWidthStr::width(grapheme.as_str()))
                .sum::<usize>();
            lines.push(Line::from(Span::styled(
                format!("{}{glyph}{}", " ".repeat(column), " ".repeat(line_width.saturating_sub(column + 1))),
                Style::default()
                    .fg(if flashing { theme.mistype } else { theme.accent })
                    .add_modifier(if flashing { Modifier::BOLD } else { Modifier::empty() }),
            )));
        }
    }
    // Keep the caret row visible when the wrapped sentence is taller than the
    // area: drop whole leading rows rather than cutting off the current line.
    let room = usize::from(area.height.saturating_sub(u16::from(markers.is_some())).max(1));
    if let Some(caret_row) = caret_row
        && caret_row >= room
    {
        lines.drain(..caret_row + 1 - room);
    }
    let desired_height = lines.len() as u16 + u16::from(markers.is_some());
    let content_area = top_centered_rect(area, content_width, desired_height.min(area.height));
    let (marker_area, text_area) = if markers.is_some() && content_area.height > 1 {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(content_area);
        (Some(chunks[0]), chunks[1])
    } else {
        (None, content_area)
    };
    if let (Some(marker_area), Some(markers)) = (marker_area, markers) {
        frame.render_widget(Paragraph::new(markers).alignment(Alignment::Center), marker_area);
    }
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), text_area);
}

fn top_aligned(area: Rect, desired_height: u16) -> Rect {
    top_centered_rect(area, area.width, desired_height)
}

/// Horizontally centred, but anchored to the top of `area` so the prompt sits
/// directly below the metrics row instead of drifting towards the keyboard.
fn top_centered_rect(area: Rect, desired_width: u16, desired_height: u16) -> Rect {
    let width = desired_width.min(area.width);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y,
        width,
        desired_height.min(area.height),
    )
}

fn progress_markers(app: &App, cell_width: usize) -> Option<Line<'static>> {
    let (pacer, ghost) = progress_positions(app)?;
    let game = app.game.as_ref()?;
    let characters = game.current_prompt.target.chars().collect::<Vec<_>>();
    let length = characters.len();
    let prefix = if cell_width == 1 && game.current_prompt.reading.is_some() {
        let label = match app.config.general.lang {
            crate::i18n::Lang::En => "Romaji: ",
            crate::i18n::Lang::Ja => "ローマ字: ",
        };
        UnicodeWidthStr::width(label)
    } else {
        0
    };
    let columns = (0..=length)
        .map(|position| {
            prefix
                + characters
                    .iter()
                    .take(position)
                    .map(|character| UnicodeWidthChar::width(*character).unwrap_or(0) + if cell_width == 3 { 2 } else { 0 })
                    .sum::<usize>()
                + usize::from(cell_width == 3 && position < length)
        })
        .collect::<Vec<_>>();
    let total_width = prefix
        + characters
            .iter()
            .map(|character| UnicodeWidthChar::width(*character).unwrap_or(0) + if cell_width == 3 { 2 } else { 0 })
            .sum::<usize>();
    let mut cells = vec![' '; total_width.max(1)];
    if let Some(position) = pacer {
        let column = columns[position.min(length)].min(cells.len() - 1);
        cells[column] = '▾';
    }
    if let Some(position) = ghost {
        let column = columns[position.min(length)].min(cells.len() - 1);
        cells[column] = if cells[column] == ' ' { '◆' } else { '◇' };
    }
    Some(Line::from(cells.into_iter().collect::<String>()))
}

fn progress_positions(app: &App) -> Option<(Option<usize>, Option<usize>)> {
    let game = app.game.as_ref()?;
    let item = game.session.log().items.last()?;
    let session_elapsed = game.elapsed;
    let item_elapsed = game.elapsed.saturating_sub(Duration::from_micros(item.shown_us));
    let length = game.current_prompt.target.chars().count();
    let pacer = app
        .config
        .pacer
        .target_wpm
        .map(|target| crate::practice::pacer::cursor_position(target, item_elapsed, length));
    let ghost = game.ghost.as_ref().and_then(|track| {
        if app.active_bench_id.is_some() {
            Some(track.position_for_item(session_elapsed, item.idx))
        } else if track.applies_to_sentence(&game.current_prompt.text) {
            Some(track.position_for_item(item_elapsed, 0))
        } else {
            None
        }
    });
    (pacer.is_some() || ghost.is_some()).then_some((pacer, ghost))
}

fn wrap_graphemes(graphemes: &[String], width: usize) -> Vec<(usize, usize)> {
    if graphemes.is_empty() {
        return vec![(0, 0)];
    }
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < graphemes.len() {
        let mut end = start;
        let mut current_width = 0usize;
        let mut last_word_break = None;
        while end < graphemes.len() {
            let grapheme_width = UnicodeWidthStr::width(graphemes[end].as_str());
            if current_width > 0 && current_width + grapheme_width > width {
                break;
            }
            current_width += grapheme_width;
            end += 1;
            if graphemes[end - 1].chars().all(char::is_whitespace) {
                last_word_break = Some(end);
            }
        }
        if end < graphemes.len() {
            end = last_word_break.filter(|word_break| *word_break > start).unwrap_or(end);
        }
        ranges.push((start, end));
        start = end;
    }
    ranges
}

fn render_word_timer(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let Some(game) = app.game.as_ref() else { return };
    let limit = app.config.session.item_timeout();
    let elapsed = if matches!(game.current_word.state, WordState::Active) {
        display_now(game).saturating_duration_since(game.word_started_at)
    } else {
        std::time::Duration::ZERO
    };
    let ratio = if limit.is_zero() {
        1.0
    } else {
        (1.0 - elapsed.as_secs_f64() / limit.as_secs_f64()).clamp(0.0, 1.0)
    };
    let bar_color = if ratio < 0.2 { theme.mistype } else { theme.accent };
    let label = format!(
        "{}  {:.1} / {:.1} s",
        Msg::WordTimer.text(app.config.general.lang),
        elapsed.as_secs_f32(),
        limit.as_secs_f32()
    );
    let gauge = Gauge::default()
        .block(Block::default())
        .gauge_style(Style::default().fg(bar_color))
        .ratio(ratio)
        .label(label);
    frame.render_widget(gauge, area);
}

fn render_sub_stats(frame: &mut Frame, game: &crate::app::GameState, theme: &Theme, area: Rect, lang: crate::i18n::Lang) {
    let acc = if game.metrics.total_chars_typed == 0 {
        100.0
    } else {
        100.0 * game.metrics.correct_chars as f64 / game.metrics.total_chars_typed as f64
    };
    let line = Line::from(vec![
        Span::styled(format!("{}: ", Msg::Combo.text(lang)), Style::default().fg(theme.muted)),
        Span::styled(format!("{:>3}", game.combo), Style::default().fg(theme.accent)),
        Span::raw("    "),
        Span::styled(format!("{}: ", Msg::Mistypes.text(lang)), Style::default().fg(theme.muted)),
        Span::styled(format!("{:>3}", game.metrics.mistypes_total), Style::default().fg(theme.mistype)),
        Span::raw("    "),
        Span::styled(format!("{}: ", Msg::Accuracy.text(lang)), Style::default().fg(theme.muted)),
        Span::styled(format!("{:>5.1}%", acc), Style::default().fg(theme.correct)),
    ]);
    frame.render_widget(Paragraph::new(line).alignment(Alignment::Center), area);
}

fn render_footer(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let Some(game) = app.game.as_ref() else { return };
    let abort = Msg::AbortHint.text(app.config.general.lang);
    let blind_notice = if app.active_bench_id.is_none() && app.config.session.blind {
        match app.config.general.lang {
            crate::i18n::Lang::En => "Blind: advance forced    ",
            crate::i18n::Lang::Ja => "ブラインド: advance を強制    ",
        }
    } else {
        ""
    };
    let reading_report = if app.config.mode.kind == crate::config::ModeKind::Romaji
        || app.config.mode.kind == crate::config::ModeKind::Sentence && app.config.mode.sentence.lang == "ja"
    {
        match app.config.general.lang {
            crate::i18n::Lang::En => "[F2] Reading is wrong    ",
            crate::i18n::Lang::Ja => "[F2] 読みが違う    ",
        }
    } else {
        ""
    };
    let abort_width = UnicodeWidthStr::width(abort) + UnicodeWidthStr::width(reading_report) + UnicodeWidthStr::width(blind_notice);
    let area_width = usize::from(area.width);
    let separator_width = usize::from(app.config.session.preview_items > 0) * 2;
    let preview_width = area_width.saturating_sub(abort_width + separator_width);
    let preview = if app.config.session.preview_items > 0 {
        let previews = game
            .preview_prompts
            .iter()
            .take(app.config.session.preview_items)
            .map(|prompt| prompt.text.as_str())
            .collect::<Vec<_>>()
            .join("  ·  ");
        truncate_to_width(&format!("{}: {previews}", Msg::Next.text(app.config.general.lang)), preview_width)
    } else {
        String::new()
    };
    let padding = area_width.saturating_sub(UnicodeWidthStr::width(preview.as_str()) + abort_width);
    let spans = vec![
        Span::styled(preview, Style::default().fg(theme.muted)),
        Span::raw(" ".repeat(padding)),
        Span::styled(blind_notice, Style::default().fg(theme.warning)),
        Span::styled(reading_report, Style::default().fg(theme.warning)),
        Span::styled(abort, Style::default().fg(theme.warning)),
    ];
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn truncate_to_width(text: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_owned();
    }
    if max_width == 0 {
        return String::new();
    }
    let content_width = max_width.saturating_sub(1);
    let mut result = String::new();
    let mut width = 0;
    for character in text.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if width + character_width > content_width {
            break;
        }
        result.push(character);
        width += character_width;
    }
    result.push('…');
    result
}
