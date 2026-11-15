//! Playing screen — stats, keyboard, item timer, centred prompt, and footer.

use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph, Wrap};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::App;
use crate::game::word::{ExitKind, WordState};
use crate::i18n::Msg;
use crate::layout::geometry::{Finger, geometry};
use crate::ui::keyboard::{KeyFeedback, KeyboardState, KeyboardView, UnlockKeyboardState};
use crate::ui::theme::Theme;

/// How long the mistype flash effect lingers on screen.
const MISTYPE_FLASH_DURATION: Duration = Duration::from_millis(250);

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

    let sentence = matches!(
        app.config.mode.kind,
        crate::config::ModeKind::Sentence | crate::config::ModeKind::Endurance
    );
    let mut show_keyboard = app.active_bench_id.is_some() || app.config.keyboard.show != "hidden";
    let mut show_sub_stats = true;
    let mut show_timer = !sentence && !app.config.session.item_timeout().is_zero();
    let has_progress = app.config.pacer.target_wpm.is_some() || app.game.as_ref().is_some_and(|game| game.ghost.is_some());
    let word_rows = if sentence { if has_progress { 4 } else { 3 } } else { 5 };
    let show_focus = app.config.mode.kind == crate::config::ModeKind::Adaptive && !app.adaptive_focus.is_empty();
    let show_rhythm = app.config.mode.kind == crate::config::ModeKind::Rhythm;
    let keyboard_rows = if app.config.mode.kind == crate::config::ModeKind::Unlock { 8 } else { 6 };
    let required_height = |keyboard: bool, sub_stats: bool, timer: bool| {
        3 + word_rows
            + u16::from(show_focus)
            + u16::from(show_rhythm)
            + u16::from(sub_stats) * 2
            + u16::from(timer) * 2
            + u16::from(keyboard) * (keyboard_rows + 1)
    };
    if required_height(show_keyboard, show_sub_stats, show_timer) > inner.height {
        show_keyboard = false;
    }
    if required_height(show_keyboard, show_sub_stats, show_timer) > inner.height {
        show_sub_stats = false;
    }
    if required_height(show_keyboard, show_sub_stats, show_timer) > inner.height {
        show_timer = false;
    }

    let mut constraints = vec![
        Constraint::Length(1), // top stats
    ];
    if show_focus {
        constraints.push(Constraint::Length(1));
    }
    if show_rhythm {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Length(1));
    if show_keyboard {
        constraints.push(Constraint::Length(keyboard_rows));
    }
    if show_timer {
        constraints.push(Constraint::Length(1));
    }
    if show_sub_stats {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Min(word_rows));
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
    frame.render_widget(separator_line(), chunks[index]);
    index += 1;
    if show_keyboard {
        render_keyboard(frame, app, chunks[index]);
        index += 1;
    }
    if show_timer {
        render_word_timer(frame, app, &theme, chunks[index]);
        index += 1;
    }
    if show_sub_stats {
        if let Some(game) = app.game.as_ref() {
            render_sub_stats(frame, game, &theme, chunks[index], lang);
        }
        index += 1;
    }
    render_word(frame, app, &theme, chunks[index]);
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

fn render_keyboard(frame: &mut Frame, app: &App, area: Rect) {
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
    let keyboard_area = if app.config.mode.kind == crate::config::ModeKind::Unlock && area.height >= 8 {
        Rect::new(area.x, area.y, area.width, 6)
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
            finger_guide: app.config.keyboard.finger_guide,
        },
    );
    if app.config.mode.kind == crate::config::ModeKind::Unlock && area.height >= 8 {
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
            Rect::new(area.x, area.y + 6, area.width, 1),
        );
        frame.render_widget(
            Paragraph::new(progress).alignment(Alignment::Center),
            Rect::new(area.x, area.y + 7, area.width, 1),
        );
    }
}

fn separator_line() -> Paragraph<'static> {
    Paragraph::new("─".repeat(200))
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

fn render_word(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
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
            vertically_centered(area, 1),
        );
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
    let content_area = vertically_centered(area, lines.len() as u16);
    let para = Paragraph::new(lines).alignment(Alignment::Center);
    frame.render_widget(para, content_area);
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
    let reading_spans = reading
        .chars()
        .enumerate()
        .map(|(index, character)| {
            let style = if blind {
                Style::default().fg(theme.muted)
            } else if index < completed_reading_chars {
                Style::default().fg(theme.correct).add_modifier(Modifier::BOLD)
            } else if index == completed_reading_chars {
                Style::default().fg(theme.warning).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Style::default().fg(theme.muted)
            };
            Span::styled(character.to_string(), style)
        })
        .collect::<Vec<_>>();
    let guide_spans = guide
        .chars()
        .enumerate()
        .map(|(index, character)| {
            let style = if blind {
                Style::default().fg(theme.muted)
            } else if index < game.typed_index {
                Style::default().fg(theme.correct).add_modifier(Modifier::BOLD)
            } else if index == game.typed_index {
                Style::default().fg(theme.warning).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Style::default().fg(theme.muted)
            };
            Span::styled(character.to_string(), style)
        })
        .collect::<Vec<_>>();
    let mut surface_spans = vec![
        Span::styled(format!("{surface_label}: "), Style::default().fg(theme.muted)),
        Span::styled(game.current_prompt.text.clone(), Style::default().fg(theme.fg)),
    ];
    let mut surface_suffix = String::new();
    if let WordState::Exiting { kind, .. } = &game.current_word.state {
        let (message, color) = match kind {
            ExitKind::Correct => (Msg::Correct.text(app.config.general.lang), theme.correct),
            ExitKind::Mistyped => (Msg::CompletedWithMistypes.text(app.config.general.lang), theme.warning),
            ExitKind::Timeout => (Msg::Timeout.text(app.config.general.lang), theme.mistype),
        };
        surface_suffix = format!("    {message}");
        surface_spans.push(Span::styled(surface_suffix.clone(), Style::default().fg(color)));
    }
    let surface = Line::from(surface_spans);
    let kana = Line::from(
        std::iter::once(Span::styled(format!("{reading_label}: "), Style::default().fg(theme.muted)))
            .chain(reading_spans)
            .collect::<Vec<_>>(),
    );
    let romaji = Line::from(
        std::iter::once(Span::styled(format!("{romaji_label}: "), Style::default().fg(theme.muted)))
            .chain(guide_spans)
            .collect::<Vec<_>>(),
    );
    let markers = progress_markers(app, 1);
    let surface_width = UnicodeWidthStr::width(format!("{surface_label}: {}{surface_suffix}", game.current_prompt.text).as_str());
    let kana_width = UnicodeWidthStr::width(format!("{reading_label}: {reading}").as_str());
    let romaji_width = UnicodeWidthStr::width(format!("{romaji_label}: {guide}").as_str());
    let content_width = surface_width.max(kana_width).max(romaji_width).clamp(1, usize::from(area.width.max(1))) as u16;
    let row_heights = [surface_width, kana_width, romaji_width].map(|width| width.div_ceil(usize::from(content_width)).max(1) as u16);
    let desired_height = row_heights.iter().sum::<u16>() + u16::from(markers.is_some());
    let content_area = centered_rect(area, content_width, desired_height.min(area.height));
    let constraints = if desired_height <= area.height {
        let mut constraints = Vec::with_capacity(4);
        if markers.is_some() {
            constraints.push(Constraint::Length(1));
        }
        constraints.extend(row_heights.map(Constraint::Length));
        constraints
    } else if markers.is_some() {
        vec![
            Constraint::Length(1),
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
        ]
    } else {
        vec![Constraint::Ratio(1, 3), Constraint::Ratio(1, 3), Constraint::Ratio(1, 3)]
    };
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(content_area);
    let offset = usize::from(markers.is_some());
    if let Some(markers) = markers {
        frame.render_widget(Paragraph::new(markers), rows[0]);
    }
    render_wrapped_guide(frame, surface, rows[offset], 0);
    render_wrapped_guide(
        frame,
        kana,
        rows[offset + 1],
        UnicodeWidthStr::width(format!("{reading_label}: ").as_str())
            + reading
                .chars()
                .take(completed_reading_chars)
                .map(|character| UnicodeWidthChar::width(character).unwrap_or(0))
                .sum::<usize>(),
    );
    render_wrapped_guide(
        frame,
        romaji,
        rows[offset + 2],
        UnicodeWidthStr::width(format!("{romaji_label}: ").as_str()) + game.typed_index,
    );
}

fn render_wrapped_guide(frame: &mut Frame, line: Line<'static>, area: Rect, cursor_width: usize) {
    let width = usize::from(area.width.max(1));
    let cursor_row = cursor_width / width;
    let visible_height = usize::from(area.height.max(1));
    let scroll = cursor_row.saturating_sub(visible_height.saturating_sub(1)) as u16;
    frame.render_widget(
        Paragraph::new(line)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        area,
    );
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
    let desired_height = lines.len() as u16 + u16::from(markers.is_some());
    let content_area = centered_rect(area, content_width, desired_height.min(area.height));
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

fn vertically_centered(area: Rect, desired_height: u16) -> Rect {
    centered_rect(area, area.width, desired_height.min(area.height))
}

fn centered_rect(area: Rect, desired_width: u16, desired_height: u16) -> Rect {
    let width = desired_width.min(area.width);
    let height = desired_height.min(area.height);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn progress_markers(app: &App, cell_width: usize) -> Option<Line<'static>> {
    let game = app.game.as_ref()?;
    let item = game.session.log().items.last()?;
    let session_elapsed = game.elapsed;
    let item_elapsed = game.elapsed.saturating_sub(Duration::from_micros(item.shown_us));
    let characters = game.current_prompt.target.chars().collect::<Vec<_>>();
    let length = characters.len();
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
    if pacer.is_none() && ghost.is_none() {
        return None;
    }
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
