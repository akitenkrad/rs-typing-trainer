//! Playing screen — stats, keyboard, item timer, centred prompt, and footer.

use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::App;
use crate::game::word::WordState;
use crate::i18n::Msg;
use crate::layout::geometry::{Finger, geometry};
use crate::ui::bigtext::BigTextSize;
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

pub(crate) fn mistype_flash_active(app: &App) -> bool {
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
    crate::ui::prompt::render(frame, app, &theme, chunks[index], layout.bigtext);
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
    if app.config.display.prompt_style != "normal" {
        candidates.push((PromptLayoutStage::XlargeSelected, Some(BigTextSize::Dot16), selected));
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

fn prompt_rows(app: &App, size: Option<BigTextSize>, width: u16) -> u16 {
    crate::ui::prompt::rows(app, size, width)
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
        .map(|row| {
            format!(
                "{}→{} ({})",
                row.from,
                row.to,
                row.reason(match app.config.general.lang {
                    crate::i18n::Lang::En => "en",
                    crate::i18n::Lang::Ja => "ja",
                })
            )
        })
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
    crate::ui::keyboard::render_with_images(
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
        crate::ui::keyboard::KeyboardImageContext {
            detection: &app.graphics,
            cache: &app.keyboard_images,
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

pub(crate) fn progress_positions(app: &App) -> Option<(Option<usize>, Option<usize>)> {
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
