//! The prompt area of the playing screen.
//!
//! Two styles are drawn here (`display.prompt_style`):
//!
//! * **large** — bitmap glyphs (16 or 12 dots) for the item being typed, with
//!   one blank row between each large block and the normal-size rows (kana,
//!   the full sentence, helper messages). Labels sit in a column to the left
//!   of the block they name. Blank rows are the first thing given up when the
//!   area is short; glyphs are never cut.
//! * **normal** — terminal text inside a rounded card with two columns and
//!   one row of padding. Word-like modes widen the letter spacing to one
//!   blank cell, the current character is reversed, and Japanese prompts are
//!   laid out as a label column and a body column with a blank row between
//!   the rows.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Paragraph};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, GameState};
use crate::config::ModeKind;
use crate::game::word::{ExitKind, WordState};
use crate::i18n::{Lang, Msg};
use crate::ui::bigtext::{BigTextSize, BitmapFont, RenderedBigText, StyledCharacter, font, window_by_widths, window_range};
use crate::ui::playing::{mistype_flash_active, progress_positions};
use crate::ui::theme::Theme;

/// Card chrome: border plus inner padding.
const CARD_PAD_X: u16 = 2;
const CARD_PAD_Y: u16 = 1;
/// Widest card body for running text, so long lines stay readable.
const SENTENCE_MAX_WIDTH: u16 = 72;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PromptKind {
    /// One item at a time with wide letter spacing (words, random, n-gram,
    /// English adaptive, unlock, rhythm, burst).
    Word,
    /// Surface / kana / romaji (romaji, Japanese adaptive, Japanese sentences).
    Japanese { sentence: bool },
    /// Running English text (sentence, endurance).
    Sentence,
}

fn prompt_kind(app: &App, game: &GameState) -> PromptKind {
    let japanese = game.current_prompt.reading.is_some();
    match app.config.mode.kind {
        ModeKind::Romaji => PromptKind::Japanese { sentence: false },
        ModeKind::Adaptive if japanese => PromptKind::Japanese { sentence: false },
        ModeKind::Sentence | ModeKind::Endurance if japanese => PromptKind::Japanese { sentence: true },
        ModeKind::Sentence | ModeKind::Endurance => PromptKind::Sentence,
        _ => PromptKind::Word,
    }
}

/// Everything the prompt renderers need about the current frame.
struct Ctx<'a> {
    app: &'a App,
    game: &'a GameState,
    theme: &'a Theme,
    lang: Lang,
    blind: bool,
    flashing: bool,
}

impl<'a> Ctx<'a> {
    fn new(app: &'a App, theme: &'a Theme) -> Option<Self> {
        let game = app.game.as_ref()?;
        let blind = app.active_bench_id.is_none() && app.config.session.blind;
        Some(Self {
            app,
            game,
            theme,
            lang: app.config.general.lang,
            blind,
            flashing: !blind && mistype_flash_active(app),
        })
    }

    fn labels(&self) -> [&'static str; 3] {
        match self.lang {
            Lang::En => ["Surface", "Kana", "Romaji"],
            Lang::Ja => ["表記", "かな", "ローマ字"],
        }
    }

    fn label_style(&self) -> Style {
        Style::default().fg(self.theme.muted)
    }

    /// Typed text in normal-size rows: a calm colour.
    fn done_style(&self) -> Style {
        Style::default().fg(self.theme.correct)
    }

    /// The character to type next in normal-size rows: reversed, or on a red
    /// background while a mistype is flashing.
    fn current_style(&self) -> Style {
        if self.flashing {
            Style::default().fg(Color::Black).bg(self.theme.mistype).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(self.theme.accent).add_modifier(Modifier::BOLD | Modifier::REVERSED)
        }
    }

    /// Text still to type in normal-size rows: bright and bold.
    fn pending_style(&self) -> Style {
        Style::default().fg(self.theme.fg).add_modifier(Modifier::BOLD)
    }

    fn caret_style(&self) -> Style {
        Style::default()
            .fg(if self.flashing { self.theme.mistype } else { self.theme.accent })
            .add_modifier(Modifier::BOLD)
    }

    fn status_line(&self) -> Option<Line<'static>> {
        let WordState::Exiting { kind, .. } = &self.game.current_word.state else {
            return None;
        };
        let (message, color) = match kind {
            ExitKind::Correct => (format!("✓ {}", Msg::Correct.text(self.lang)), self.theme.correct),
            ExitKind::Mistyped => (format!("✗ {}", Msg::CompletedWithMistypes.text(self.lang)), self.theme.warning),
            ExitKind::Timeout => (format!("⌛ {}", Msg::Timeout.text(self.lang)), self.theme.mistype),
        };
        Some(Line::from(Span::styled(message, Style::default().fg(color).add_modifier(Modifier::BOLD))))
    }

    /// The helper row under a word: locking, mistype details, the number of
    /// mistypes in this word, or the result once the word is done.
    fn word_helper_line(&self) -> Option<Line<'static>> {
        let lang = self.lang;
        match &self.game.current_word.state {
            WordState::Entering { .. } => Some(Line::from(Span::styled(
                format!("▷▷▷  ({})  ▷▷▷", Msg::Locking.text(lang)),
                Style::default().fg(self.theme.muted),
            ))),
            WordState::Active if self.blind => None,
            WordState::Active if self.flashing => {
                let typed = self.game.last_mistype_char.as_deref().unwrap_or("?");
                let expected = self.game.last_mistype_expected.as_deref().unwrap_or("?");
                Some(Line::from(Span::styled(
                    format!(
                        "✗ {}  {} '{}'  {} '{}'",
                        Msg::Mistype.text(lang),
                        Msg::Got.text(lang),
                        typed,
                        Msg::Expected.text(lang),
                        expected
                    ),
                    Style::default().fg(self.theme.mistype).add_modifier(Modifier::BOLD),
                )))
            }
            WordState::Active if self.game.mistypes_in_word > 0 => Some(Line::from(Span::styled(
                format!("✗ {}: {}", Msg::MistypesInWord.text(lang), self.game.mistypes_in_word),
                Style::default().fg(self.theme.warning),
            ))),
            WordState::Active => None,
            WordState::Exiting { .. } => self.status_line(),
        }
    }

    fn helper_line(&self, kind: PromptKind) -> Option<Line<'static>> {
        match kind {
            PromptKind::Word => self.word_helper_line(),
            _ => self.status_line(),
        }
    }
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// Draw the prompt into `area` (the rows below the metrics, above the footer).
pub(crate) fn render(frame: &mut Frame, app: &App, theme: &Theme, area: Rect, bigtext: Option<BigTextSize>) {
    let Some(ctx) = Ctx::new(app, theme) else { return };
    if ctx.game.ime_warning {
        let warning = match ctx.lang {
            Lang::En => "Please turn IME off.",
            Lang::Ja => "IME をオフにしてください．",
        };
        frame.render_widget(
            Paragraph::new(warning)
                .alignment(Alignment::Center)
                .style(Style::default().fg(theme.mistype).add_modifier(Modifier::BOLD)),
            Rect::new(area.x, area.y, area.width, area.height.min(1)),
        );
        return;
    }
    // Keep one blank column on each side so nothing touches the outer frame.
    let area = Rect::new(area.x.saturating_add(1), area.y, area.width.saturating_sub(2), area.height);
    let kind = prompt_kind(app, ctx.game);
    if let Some(size) = bigtext
        && let Ok(bitmap_font) = font(size)
    {
        match kind {
            PromptKind::Word => render_big_word(frame, &ctx, area, bitmap_font),
            PromptKind::Sentence => render_big_sentence(frame, &ctx, area, bitmap_font),
            PromptKind::Japanese { sentence } => render_big_japanese(frame, &ctx, area, bitmap_font, sentence),
        }
        return;
    }
    render_card(frame, &ctx, kind, area);
}

/// Rows the prompt needs at a given style; used to choose the screen layout.
///
/// Large text: the glyph blocks plus the rows that always accompany them
/// (kana, cursor, the blank rows between blocks and at least two rows of the
/// full sentence). The optional helper row of word modes and its blank row
/// are not counted; they appear when there is room.
///
/// Normal text: the whole card with its padding and blank rows, plus one row
/// for the helper message, so the keyboard gives way before the card.
pub(crate) fn rows(app: &App, size: Option<BigTextSize>, width: u16) -> u16 {
    let theme = Theme::from_name(&app.config.ui.theme);
    let Some(ctx) = Ctx::new(app, &theme) else {
        return 3;
    };
    let kind = prompt_kind(app, ctx.game);
    let markers = u16::from(progress_positions(app).is_some());
    let Some(size) = size else {
        let content_width = card_content_width(width.saturating_sub(2), kind);
        let (segments, gaps) = card_segments(&ctx, kind, content_width);
        let content = segments.iter().map(|segment| segment.rows.len()).sum::<usize>() as u16;
        let gap_rows = if gaps { segments.len().saturating_sub(1) as u16 } else { 0 };
        return content + gap_rows + 2 + 2 * CARD_PAD_Y + 1;
    };
    let height = size.cell_height();
    match kind {
        // Surface, gap, kana, gap, romaji, cursor.
        PromptKind::Japanese { sentence: false } => 2 * height + 4 + markers,
        // As above, then the three full-sentence rows (and a blank row
        // before them when there is one to spare).
        PromptKind::Japanese { sentence: true } => 2 * height + 7 + markers,
        // Window, cursor, gap and two rows of the full sentence.
        PromptKind::Sentence => height + 4 + markers,
        PromptKind::Word => height + 1 + markers,
    }
}

// ---------------------------------------------------------------------------
// Shared drawing helpers
// ---------------------------------------------------------------------------

/// Draw `line` with its left edge at absolute column `x` on row `y` of `area`.
fn put(frame: &mut Frame, area: Rect, x: u16, y: u16, line: Line<'static>) {
    if y >= area.height || x >= area.right() {
        return;
    }
    let width = (line.width() as u16).min(area.right() - x);
    frame.render_widget(Paragraph::new(line), Rect::new(x, area.y + y, width, 1));
}

fn put_centered(frame: &mut Frame, area: Rect, y: u16, line: Line<'static>) {
    if y < area.height {
        frame.render_widget(
            Paragraph::new(line).alignment(Alignment::Center),
            Rect::new(area.x, area.y + y, area.width, 1),
        );
    }
}

fn centered_x(area: Rect, width: u16) -> u16 {
    area.x + area.width.saturating_sub(width) / 2
}

fn blank(width: usize) -> String {
    " ".repeat(width)
}

/// How many of `wanted` optional blank rows fit when `fixed` rows must be shown.
fn gaps_that_fit(available: u16, fixed: u16, wanted: u16) -> u16 {
    available.saturating_sub(fixed).min(wanted)
}

/// A group of rows that is trimmed around `cursor` when space is short.
struct Segment {
    rows: Vec<Line<'static>>,
    cursor: usize,
}

/// Chooses which rows of each segment to show in `room` rows: every row when
/// they fit, otherwise each segment's cursor row first and then the rows
/// around it, one segment at a time.
fn fit_segments(segments: &[(usize, usize)], room: usize) -> Vec<std::ops::Range<usize>> {
    let total = segments.iter().map(|(rows, _)| rows).sum::<usize>();
    if total <= room {
        return segments.iter().map(|(rows, _)| 0..*rows).collect();
    }
    let mut budget = room;
    let mut ranges = segments
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
        for (range, (rows, _)) in ranges.iter_mut().zip(segments) {
            if budget == 0 {
                break;
            }
            if range.end < *rows && range.start < range.end {
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

/// Stacks segments into at most `room` rows. `None` marks a blank row between
/// two segments; blank rows are dropped before any text row.
fn stack_segments(segments: &[Segment], room: usize, gaps: bool) -> Vec<Option<Line<'static>>> {
    let content = segments.iter().map(|segment| segment.rows.len()).sum::<usize>();
    let wanted = if gaps { segments.len().saturating_sub(1) } else { 0 };
    let gap_count = room.saturating_sub(content).min(wanted);
    let ranges = fit_segments(
        &segments.iter().map(|segment| (segment.rows.len(), segment.cursor)).collect::<Vec<_>>(),
        room.saturating_sub(gap_count),
    );
    let mut rows = Vec::new();
    for (index, (segment, range)) in segments.iter().zip(ranges).enumerate() {
        if index > 0 && index <= gap_count {
            rows.push(None);
        }
        rows.extend(segment.rows[range].iter().cloned().map(Some));
    }
    rows
}

type StyledCell = (String, Style);

fn cells_line(cells: &[StyledCell]) -> Line<'static> {
    Line::from(cells.iter().map(|(text, style)| Span::styled(text.clone(), *style)).collect::<Vec<_>>())
}

/// Wraps cells by display width, character by character. Japanese has no
/// spaces to break at, so word wrapping would leave a row nearly empty.
fn wrap_cells(cells: &[StyledCell], width: usize) -> Vec<Vec<StyledCell>> {
    let mut lines: Vec<Vec<StyledCell>> = vec![Vec::new()];
    let mut used = 0;
    for cell in cells {
        let cell_width = UnicodeWidthStr::width(cell.0.as_str());
        if used + cell_width > width && used > 0 {
            // Closing punctuation never starts a row: carry the previous
            // character down with it.
            let carried = if no_line_start(&cell.0) && lines.last().is_some_and(|line| line.len() > 1) {
                lines.last_mut().and_then(Vec::pop)
            } else {
                None
            };
            used = carried.as_ref().map_or(0, |cell| UnicodeWidthStr::width(cell.0.as_str()));
            lines.push(carried.into_iter().collect());
        }
        lines.last_mut().expect("at least one line").push(cell.clone());
        used += cell_width;
    }
    lines
}

fn no_line_start(text: &str) -> bool {
    matches!(
        text,
        "、" | "。" | "，" | "．" | "," | "." | "」" | "』" | "）" | ")" | "！" | "？" | "!" | "?" | "ー" | "ゃ" | "ゅ" | "ょ" | "っ"
    )
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

/// A row of pacer (`▾`) and ghost (`◆`, `◇` when both coincide) markers.
/// `columns[p]` is the column of character position `p` (one entry per
/// position, including the end of the text).
fn marker_row(columns: &[usize], pacer: Option<usize>, ghost: Option<usize>, theme: &Theme) -> Line<'static> {
    let width = columns.last().copied().unwrap_or(0) + 1;
    let mut cells = vec![(' ', Style::default()); width];
    let column_of = |position: usize| columns[position.min(columns.len() - 1)].min(width - 1);
    if let Some(position) = pacer {
        cells[column_of(position)] = ('▾', Style::default().fg(theme.warning));
    }
    if let Some(position) = ghost {
        let column = column_of(position);
        cells[column] = if cells[column].0 == ' ' {
            ('◆', Style::default().fg(theme.accent))
        } else {
            ('◇', Style::default().fg(theme.accent))
        };
    }
    Line::from(
        cells
            .into_iter()
            .map(|(symbol, style)| Span::styled(symbol.to_string(), style))
            .collect::<Vec<_>>(),
    )
}

// ---------------------------------------------------------------------------
// Large text
// ---------------------------------------------------------------------------

/// Bitmap style of one large character.
fn big_style(index: usize, current: usize, ctx: &Ctx<'_>) -> (Style, bool) {
    let theme = ctx.theme;
    if index < current && ctx.blind {
        (Style::default(), false)
    } else if index < current {
        (Style::default().fg(theme.correct).add_modifier(Modifier::BOLD), true)
    } else if index == current && ctx.flashing {
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

fn big_window(text: &str, range: std::ops::Range<usize>, current: usize, ctx: &Ctx<'_>) -> Vec<StyledCharacter> {
    text.chars()
        .enumerate()
        .skip(range.start)
        .take(range.len())
        .map(|(index, character)| {
            let (style, visible) = big_style(index, current, ctx);
            if visible {
                StyledCharacter::new(character, style)
            } else {
                StyledCharacter::hidden(character, style)
            }
        })
        .collect()
}

fn big_marker_line(ctx: &Ctx<'_>, rendered: &RenderedBigText, range: &std::ops::Range<usize>) -> Option<Line<'static>> {
    let (pacer, ghost) = progress_positions(ctx.app)?;
    let mut markers = Vec::new();
    if let Some(position) = pacer
        && range.start <= position
        && position <= range.end
    {
        markers.push((
            position.saturating_sub(range.start).min(range.len()),
            '▾',
            Style::default().fg(ctx.theme.warning),
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
            markers.push((relative, '◆', Style::default().fg(ctx.theme.accent)));
        }
    }
    Some(rendered.marker_line(&markers))
}

/// A large window of `text` around `current`, with its marker row (if any).
struct BigBlock {
    rendered: RenderedBigText,
    markers: Option<Line<'static>>,
    cursor: usize,
}

impl BigBlock {
    fn new(ctx: &Ctx<'_>, bitmap_font: &BitmapFont, text: &str, current: usize, max_width: u16) -> Option<Self> {
        let count = text.chars().count();
        if count == 0 {
            return None;
        }
        let current_clamped = current.min(count - 1);
        let range = window_range(bitmap_font, text, current_clamped, max_width);
        let rendered = bitmap_font.render(&big_window(text, range.clone(), current, ctx));
        let markers = big_marker_line(ctx, &rendered, &range);
        Some(Self {
            rendered,
            markers,
            cursor: current_clamped.saturating_sub(range.start),
        })
    }

    fn width(&self) -> u16 {
        self.rendered.width
    }

    /// Draws markers, glyph rows and the cursor (or `status`) from row `y`;
    /// returns the row after the cursor.
    fn draw(&self, frame: &mut Frame, ctx: &Ctx<'_>, area: Rect, x: u16, mut y: u16, status: Option<Line<'static>>) -> u16 {
        if let Some(markers) = &self.markers {
            put(frame, area, x, y, markers.clone());
            y += 1;
        }
        for line in &self.rendered.lines {
            put(frame, area, x, y, line.clone());
            y += 1;
        }
        match status {
            Some(status) => put_centered(frame, area, y, status),
            None => put(frame, area, x, y, self.rendered.cursor_line(self.cursor, ctx.caret_style())),
        }
        y + 1
    }

    fn rows(&self) -> u16 {
        u16::from(self.markers.is_some()) + self.rendered.lines.len() as u16 + 1
    }
}

fn render_big_word(frame: &mut Frame, ctx: &Ctx<'_>, area: Rect, bitmap_font: &BitmapFont) {
    let text = ctx.game.current_prompt.target.as_str();
    let Some(block) = BigBlock::new(ctx, bitmap_font, text, ctx.game.typed_index, area.width) else {
        return;
    };
    let helper = ctx.word_helper_line();
    let fixed = block.rows();
    // Helper row first, then the blank row above it.
    let show_helper = helper.is_some() && area.height > fixed;
    let gap = u16::from(show_helper && area.height > fixed + 1);
    let status = if show_helper { None } else { ctx.status_line() };
    let y = block.draw(frame, ctx, area, centered_x(area, block.width()), 0, status);
    if show_helper && let Some(helper) = helper {
        put_centered(frame, area, y + gap, helper);
    }
}

fn render_big_sentence(frame: &mut Frame, ctx: &Ctx<'_>, area: Rect, bitmap_font: &BitmapFont) {
    let text = ctx.game.current_prompt.text.as_str();
    let Some(block) = BigBlock::new(ctx, bitmap_font, text, ctx.game.typed_index, area.width) else {
        return;
    };
    let mut y = block.draw(frame, ctx, area, centered_x(area, block.width()), 0, ctx.status_line());
    let width = area.width.clamp(1, SENTENCE_MAX_WIDTH);
    let segment = sentence_segment(ctx, usize::from(width), false, false);
    let rest = area.height.saturating_sub(y);
    // The blank row goes first when the full text does not fit.
    let gap = u16::from(rest > segment.rows.len() as u16);
    y += gap;
    for line in stack_segments(&[segment], usize::from(rest - gap), false).into_iter().flatten() {
        put_centered(frame, area, y, line);
        y += 1;
    }
}

fn render_big_japanese(frame: &mut Frame, ctx: &Ctx<'_>, area: Rect, bitmap_font: &BitmapFont, sentence: bool) {
    let game = ctx.game;
    let labels = ctx.labels();
    let label_column = labels.iter().map(|label| UnicodeWidthStr::width(*label)).max().unwrap_or(0) as u16 + 2;
    let body_width = area.width.saturating_sub(label_column).max(1);

    let surface = game.current_prompt.text.as_str();
    let reading = game.current_prompt.reading.as_deref().unwrap_or_default();
    let guide = game.session.current_guide();
    let completed_reading = game.session.completed_reading_chars();
    let surface_count = surface.chars().count();
    // The surface has no cursor of its own; in a sentence it follows the kana
    // proportionally so the window keeps the part being typed.
    let surface_current = if sentence && surface_count > 0 {
        (completed_reading.saturating_mul(surface_count) / reading.chars().count().max(1)).min(surface_count - 1)
    } else {
        0
    };
    let surface_range = window_range(bitmap_font, surface, surface_current, body_width);
    let surface_rendered = bitmap_font.render(
        &surface
            .chars()
            .skip(surface_range.start)
            .take(surface_range.len())
            .map(|character| StyledCharacter::new(character, Style::default().fg(ctx.theme.fg).add_modifier(Modifier::BOLD)))
            .collect::<Vec<_>>(),
    );
    let kana = big_kana_line(ctx, reading, completed_reading, body_width);
    let romaji = BigBlock::new(ctx, bitmap_font, &guide, game.typed_index, body_width);

    let content_width = surface_rendered
        .width
        .max(kana.width() as u16)
        .max(romaji.as_ref().map_or(0, BigBlock::width));
    let x0 = centered_x(area, label_column + content_width);
    let body_x = x0 + label_column;
    let label = |text: &'static str| Line::from(Span::styled(text, ctx.label_style()));

    let surface_rows = surface_rendered.lines.len() as u16;
    let romaji_rows = romaji.as_ref().map_or(0, BigBlock::rows);
    let table_rows = if sentence { 3 } else { 0 };
    let fixed = surface_rows + 1 + romaji_rows + table_rows;
    let gaps = gaps_that_fit(area.height, fixed, if sentence { 3 } else { 2 });

    let mut y = 0;
    put(frame, area, x0, y, label(labels[0]));
    for line in surface_rendered.lines {
        put(frame, area, body_x, y, line);
        y += 1;
    }
    y += u16::from(gaps >= 1);
    put(frame, area, x0, y, label(labels[1]));
    put(frame, area, body_x, y, kana);
    y += 1;
    y += u16::from(gaps >= 2);
    if let Some(romaji) = romaji {
        let glyph_row = y + u16::from(romaji.markers.is_some());
        put(frame, area, x0, glyph_row, label(labels[2]));
        y = romaji.draw(frame, ctx, area, body_x, y, ctx.status_line());
    }
    if sentence {
        y += u16::from(gaps >= 3);
        if y < area.height {
            let rest = Rect::new(area.x, area.y + y, area.width, area.height - y);
            let segments = japanese_segments(ctx, usize::from(area.width), false);
            draw_block(frame, rest, stack_segments(&segments, usize::from(rest.height), false));
        }
    }
}

/// The kana row under the large surface: bold, in the accent colour, with the
/// kana being typed reversed. A long reading shows the part around it.
fn big_kana_line(ctx: &Ctx<'_>, reading: &str, completed: usize, max_width: u16) -> Line<'static> {
    let widths = reading
        .chars()
        .map(|character| UnicodeWidthChar::width(character).unwrap_or(0) as u16)
        .collect::<Vec<_>>();
    let range = window_by_widths(&widths, completed, max_width);
    Line::from(
        reading
            .chars()
            .enumerate()
            .skip(range.start)
            .take(range.len())
            .map(|(index, character)| {
                let style = if ctx.blind {
                    Style::default().fg(ctx.theme.muted)
                } else if index < completed {
                    Style::default().fg(ctx.theme.correct).add_modifier(Modifier::BOLD)
                } else if index == completed {
                    Style::default().fg(ctx.theme.accent).add_modifier(Modifier::BOLD | Modifier::REVERSED)
                } else {
                    Style::default().fg(ctx.theme.accent).add_modifier(Modifier::BOLD)
                };
                Span::styled(character.to_string(), style)
            })
            .collect::<Vec<_>>(),
    )
}

/// Draws left-aligned rows as one horizontally centred block from the top of
/// `area`; `None` rows stay blank.
fn draw_block(frame: &mut Frame, area: Rect, rows: Vec<Option<Line<'static>>>) {
    let width = rows.iter().flatten().map(Line::width).max().unwrap_or(0) as u16;
    let x = centered_x(area, width);
    for (y, row) in rows.into_iter().enumerate() {
        if let Some(line) = row {
            put(frame, area, x, y as u16, line);
        }
    }
}

// ---------------------------------------------------------------------------
// Normal text: rows
// ---------------------------------------------------------------------------

/// The word with one blank cell between characters, the caret row under it
/// and the marker row above it.
fn word_segment(ctx: &Ctx<'_>) -> Segment {
    let graphemes = &ctx.game.current_word.word.graphemes;
    let current = ctx.game.typed_index;
    let mut cells: Vec<StyledCell> = Vec::new();
    let mut columns = Vec::with_capacity(graphemes.len() + 1);
    let mut column = 0;
    for (index, grapheme) in graphemes.iter().enumerate() {
        if index > 0 {
            cells.push((" ".to_owned(), Style::default()));
            column += 1;
        }
        columns.push(column);
        let width = UnicodeWidthStr::width(grapheme.as_str());
        let cell = if index < current && ctx.blind {
            (blank(width), Style::default())
        } else if index < current {
            (grapheme.clone(), ctx.done_style())
        } else if index == current {
            (grapheme.clone(), ctx.current_style())
        } else {
            (grapheme.clone(), ctx.pending_style())
        };
        cells.push(cell);
        column += width;
    }
    columns.push(column + 1);
    let mut rows = Vec::new();
    if let Some((pacer, ghost)) = progress_positions(ctx.app) {
        rows.push(marker_row(&columns, pacer, ghost, ctx.theme));
    }
    let cursor = rows.len();
    rows.push(cells_line(&cells));
    let caret_index = current.min(graphemes.len().saturating_sub(1));
    let caret_column = columns.get(caret_index).copied().unwrap_or(0);
    let caret = if ctx.blind {
        " "
    } else if ctx.flashing {
        "✗"
    } else {
        "▲"
    };
    rows.push(Line::from(vec![Span::raw(blank(caret_column)), Span::styled(caret, ctx.caret_style())]));
    Segment { rows, cursor }
}

/// Running text wrapped to `width`. In a card (`card`), the row being typed
/// is bright, the other rows are dimmed and a caret row follows the current
/// row; under the large window (`!card`) the rows are plain. The current
/// character is reversed in both.
fn sentence_segment(ctx: &Ctx<'_>, width: usize, card: bool, with_markers: bool) -> Segment {
    let game = ctx.game;
    let graphemes = game.current_prompt.text.graphemes(true).map(str::to_owned).collect::<Vec<_>>();
    let display_start = if ctx.app.config.mode.sentence.hide_typed {
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
    let markers = if with_markers { progress_positions(ctx.app) } else { None };
    let mut rows = Vec::new();
    let mut cursor_row = 0;
    for (start, end) in wrap_graphemes(visible, width.max(1)) {
        let current_row = cursor >= start && cursor < end.max(start + 1);
        let column_of = |index: usize| {
            visible[start..index.clamp(start, end)]
                .iter()
                .map(|grapheme| UnicodeWidthStr::width(grapheme.as_str()))
                .sum::<usize>()
        };
        if current_row && let Some((pacer, ghost)) = markers {
            let columns = (start..=end).map(column_of).collect::<Vec<_>>();
            let relative = |position: usize| position.saturating_sub(display_start).saturating_sub(start);
            rows.push(marker_row(&columns, pacer.map(relative), ghost.map(relative), ctx.theme));
        }
        let spans = visible[start..end]
            .iter()
            .enumerate()
            .map(|(offset, grapheme)| {
                let absolute = display_start + start + offset;
                let mut style = if absolute < game.typed_index && ctx.blind {
                    return Span::raw(blank(UnicodeWidthStr::width(grapheme.as_str())));
                } else if absolute < game.typed_index {
                    ctx.done_style()
                } else if absolute == game.typed_index {
                    ctx.current_style()
                } else if card {
                    ctx.pending_style()
                } else {
                    Style::default().fg(ctx.theme.fg)
                };
                if card && !current_row && absolute != game.typed_index {
                    style = style.remove_modifier(Modifier::BOLD).add_modifier(Modifier::DIM);
                }
                Span::styled(grapheme.clone(), style)
            })
            .collect::<Vec<_>>();
        if current_row {
            cursor_row = rows.len();
        }
        rows.push(Line::from(spans));
        if card && current_row {
            let glyph = if ctx.blind {
                " "
            } else if ctx.flashing {
                "✗"
            } else {
                "▲"
            };
            rows.push(Line::from(vec![
                Span::raw(blank(column_of(cursor))),
                Span::styled(glyph, ctx.caret_style()),
            ]));
        }
    }
    Segment { rows, cursor: cursor_row }
}

/// Surface, kana and romaji as three segments: a label column (muted) and a
/// body column, each body wrapped to the width left by the labels. With
/// `with_markers`, the pacer/ghost row sits directly above the romaji row
/// being typed.
fn japanese_segments(ctx: &Ctx<'_>, width: usize, with_markers: bool) -> Vec<Segment> {
    let game = ctx.game;
    let labels = ctx.labels();
    let label_column = labels.iter().map(|label| UnicodeWidthStr::width(*label)).max().unwrap_or(0) + 2;
    let body_width = width.saturating_sub(label_column).max(2);

    let surface_text = game.current_prompt.text.as_str();
    let reading = game.current_prompt.reading.clone().unwrap_or_default();
    let guide = game.session.current_guide();
    let completed = game.session.completed_reading_chars();
    let surface_count = surface_text.chars().count();
    let surface_cursor = completed.saturating_mul(surface_count) / reading.chars().count().max(1);
    let progress = |index: usize, current: usize| {
        if index < current && ctx.blind {
            Style::default().fg(ctx.theme.muted)
        } else if index < current {
            ctx.done_style()
        } else if index == current {
            ctx.current_style()
        } else {
            Style::default().fg(ctx.theme.fg)
        }
    };
    let cells = |text: &str, style: &dyn Fn(usize) -> Style| -> Vec<StyledCell> {
        text.chars()
            .enumerate()
            .map(|(index, character)| (character.to_string(), style(index)))
            .collect()
    };
    let surface = cells(surface_text, &|_| Style::default().fg(ctx.theme.fg).add_modifier(Modifier::BOLD));
    let kana = cells(&reading, &|index| progress(index, completed));
    let romaji = cells(&guide, &|index| {
        let style = progress(index, game.typed_index);
        if index > game.typed_index {
            style.add_modifier(Modifier::BOLD)
        } else {
            style
        }
    });
    let markers = if with_markers { progress_positions(ctx.app) } else { None };

    let mut segments = Vec::with_capacity(3);
    for (index, (body, cursor)) in [(surface, surface_cursor), (kana, completed), (romaji, game.typed_index)]
        .into_iter()
        .enumerate()
    {
        let wrapped = wrap_cells(&body, body_width);
        let mut seen = 0;
        let mut cursor_line = wrapped.len().saturating_sub(1);
        for (line_index, line) in wrapped.iter().enumerate() {
            if cursor < seen + line.len() {
                cursor_line = line_index;
                break;
            }
            seen += line.len();
        }
        let mut rows = Vec::new();
        let mut segment_cursor = 0;
        let mut start = 0;
        for (line_index, line) in wrapped.iter().enumerate() {
            let label = if line_index == 0 { labels[index] } else { "" };
            if index == 2
                && line_index == cursor_line
                && let Some((pacer, ghost)) = markers
            {
                let mut columns = Vec::with_capacity(line.len() + 1);
                let mut column = label_column;
                for cell in line {
                    columns.push(column);
                    column += UnicodeWidthStr::width(cell.0.as_str());
                }
                columns.push(column);
                let relative = |position: usize| position.saturating_sub(start);
                rows.push(marker_row(&columns, pacer.map(relative), ghost.map(relative), ctx.theme));
            }
            if line_index == cursor_line {
                segment_cursor = rows.len();
            }
            let mut spans = vec![Span::styled(
                format!("{label}{}", blank(label_column - UnicodeWidthStr::width(label))),
                ctx.label_style(),
            )];
            spans.extend(cells_line(line).spans);
            rows.push(Line::from(spans));
            start += line.len();
        }
        segments.push(Segment {
            rows,
            cursor: segment_cursor,
        });
    }
    segments
}

// ---------------------------------------------------------------------------
// Normal text: the card
// ---------------------------------------------------------------------------

/// Widest body a card may have inside `width` columns.
fn card_content_width(width: u16, kind: PromptKind) -> u16 {
    let inner = width.saturating_sub(2 + 2 * CARD_PAD_X).max(1);
    match kind {
        // Japanese rows are short in characters, so they may use the width.
        PromptKind::Word | PromptKind::Japanese { .. } => inner,
        PromptKind::Sentence => inner.min(SENTENCE_MAX_WIDTH),
    }
}

/// The card's segments and whether a blank row separates them.
fn card_segments(ctx: &Ctx<'_>, kind: PromptKind, width: u16) -> (Vec<Segment>, bool) {
    match kind {
        PromptKind::Word => (vec![word_segment(ctx)], false),
        PromptKind::Sentence => (vec![sentence_segment(ctx, usize::from(width), true, true)], false),
        PromptKind::Japanese { .. } => (japanese_segments(ctx, usize::from(width), true), true),
    }
}

/// Draws the prompt as a rounded card, centred and anchored to the top of
/// `area`, with the helper message under it. When rows are short the card
/// gives up, in order, the blank rows between its rows, its padding, the
/// helper row and its border; text rows go last.
fn render_card(frame: &mut Frame, ctx: &Ctx<'_>, kind: PromptKind, area: Rect) {
    let content_width = card_content_width(area.width, kind);
    let (segments, gaps) = card_segments(ctx, kind, content_width);
    let helper = ctx.helper_line(kind);
    let height = usize::from(area.height);
    let content = segments.iter().map(|segment| segment.rows.len()).sum::<usize>();
    let wanted_gaps = if gaps { segments.len().saturating_sub(1) } else { 0 };
    let helper_rows = usize::from(helper.is_some());
    let pad = usize::from(CARD_PAD_Y);

    let (border, padding, show_helper, room) = if content + 2 + 2 * pad + helper_rows <= height {
        (true, true, true, height - 2 - 2 * pad - helper_rows)
    } else if content + 2 + helper_rows <= height {
        (true, false, true, height - 2 - helper_rows)
    } else if content + 2 <= height {
        (true, false, false, height - 2)
    } else {
        (false, false, false, height)
    };
    let rows = stack_segments(&segments, room.min(content + wanted_gaps), gaps);
    let body_width = rows.iter().flatten().map(Line::width).max().unwrap_or(1) as u16;
    let pad_x = if border { CARD_PAD_X } else { 0 };
    let pad_y = if padding { CARD_PAD_Y } else { 0 };
    let chrome_x = if border { 2 + 2 * pad_x } else { 0 };
    let card_width = (body_width + chrome_x).min(area.width);
    let card_height = (rows.len() as u16 + if border { 2 + 2 * pad_y } else { 0 }).min(area.height);
    let card = Rect::new(centered_x(area, card_width), area.y, card_width, card_height);
    if border {
        frame.render_widget(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(ctx.theme.muted)),
            card,
        );
    }
    let offset = u16::from(border);
    let body = Rect::new(
        card.x + offset + pad_x,
        card.y + offset + pad_y,
        card.width.saturating_sub(2 * (offset + pad_x)),
        card.height.saturating_sub(2 * (offset + pad_y)),
    );
    for (y, row) in rows.into_iter().enumerate() {
        if let Some(line) = row {
            put(frame, body, body.x, y as u16, line);
        }
    }
    if show_helper && let Some(helper) = helper {
        put_centered(frame, area, card_height, helper);
    }
}
