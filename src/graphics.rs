//! Terminal image-protocol detection and cell-pixel accounting.

use std::fmt;
use std::io::{self, IsTerminal};
use std::time::Duration;

use clap::ValueEnum;
use image::{DynamicImage, Rgba, RgbaImage};
use ratatui_image::FontSize;
use ratatui_image::picker::cap_parser::QueryStdioOptions;
use ratatui_image::picker::{Capability, Picker, ProtocolType};
use serde::{Deserialize, Serialize};

use crate::i18n::Lang;

/// Maximum quiet interval while waiting for terminal capability replies.
pub const QUERY_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum GraphicsPreference {
    #[default]
    Auto,
    Kitty,
    Sixel,
    Iterm2,
    Off,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GraphicsMode {
    Kitty,
    Sixel,
    Iterm2,
    #[default]
    Off,
}

impl GraphicsMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Kitty => "kitty",
            Self::Sixel => "sixel",
            Self::Iterm2 => "iterm2",
            Self::Off => "off",
        }
    }

    const fn protocol_type(self) -> Option<ProtocolType> {
        match self {
            Self::Kitty => Some(ProtocolType::Kitty),
            Self::Sixel => Some(ProtocolType::Sixel),
            Self::Iterm2 => Some(ProtocolType::Iterm2),
            Self::Off => None,
        }
    }
}

impl fmt::Display for GraphicsMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CellPixels {
    pub width: u16,
    pub height: u16,
}

impl CellPixels {
    pub const fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryProtocol {
    Kitty,
    Iterm2,
    Sixel,
    Halfblocks,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryResult {
    pub kitty: bool,
    pub iterm2: bool,
    pub sixel: bool,
    pub cell_pixels: Option<CellPixels>,
}

impl QueryResult {
    pub const fn new(protocol: QueryProtocol, cell_pixels: Option<CellPixels>) -> Self {
        Self {
            kitty: matches!(protocol, QueryProtocol::Kitty),
            iterm2: matches!(protocol, QueryProtocol::Iterm2),
            sixel: matches!(protocol, QueryProtocol::Sixel),
            cell_pixels,
        }
    }

    const fn preferred_mode(&self) -> GraphicsMode {
        if self.kitty {
            GraphicsMode::Kitty
        } else if self.iterm2 {
            GraphicsMode::Iterm2
        } else if self.sixel {
            GraphicsMode::Sixel
        } else {
            GraphicsMode::Off
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphicsReason {
    Disabled,
    NotTerminal,
    MultiplexerAuto,
    Detected(GraphicsMode),
    Explicit(GraphicsMode),
    ExplicitWithWindowSize,
    NoImageProtocol,
    QueryUnavailable,
    MissingCellSize(GraphicsMode),
}

impl GraphicsReason {
    pub fn text(self, lang: Lang) -> String {
        match (self, lang) {
            (Self::Disabled, Lang::En) => "graphics are disabled by configuration".to_owned(),
            (Self::Disabled, Lang::Ja) => "設定で画像を無効にしています".to_owned(),
            (Self::NotTerminal, Lang::En) => "standard input or output is not a terminal".to_owned(),
            (Self::NotTerminal, Lang::Ja) => "標準入出力が端末ではありません".to_owned(),
            (Self::MultiplexerAuto, Lang::En) => "tmux/screen passthrough cannot be verified in auto mode".to_owned(),
            (Self::MultiplexerAuto, Lang::Ja) => "tmux では passthrough を確かめられないため auto を無効にしました".to_owned(),
            (Self::Detected(mode), Lang::En) => format!("terminal query detected {mode}"),
            (Self::Detected(mode), Lang::Ja) => format!("端末への問い合わせで {mode} を検出しました"),
            (Self::Explicit(mode), Lang::En) => format!("{mode} was selected explicitly"),
            (Self::Explicit(mode), Lang::Ja) => format!("{mode} が明示されています"),
            (Self::ExplicitWithWindowSize, Lang::En) => "the explicitly selected protocol uses cell pixels calculated from TIOCGWINSZ".to_owned(),
            (Self::ExplicitWithWindowSize, Lang::Ja) => "明示された方式に TIOCGWINSZ から計算したセルの画素数を使います".to_owned(),
            (Self::NoImageProtocol, Lang::En) => "the terminal query found only halfblocks (no image protocol)".to_owned(),
            (Self::NoImageProtocol, Lang::Ja) => "問い合わせ結果が半ブロックだけで，画像方式を利用できません".to_owned(),
            (Self::QueryUnavailable, Lang::En) => "the terminal query did not provide an image protocol".to_owned(),
            (Self::QueryUnavailable, Lang::Ja) => "端末への問い合わせから画像方式を判定できませんでした".to_owned(),
            (Self::MissingCellSize(mode), Lang::En) => format!("{mode} was selected, but cell pixel size is unavailable"),
            (Self::MissingCellSize(mode), Lang::Ja) => format!("{mode} が指定されていますが，1 セルの画素数を取得できません"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphicsDetection {
    pub mode: GraphicsMode,
    pub cell_pixels: Option<CellPixels>,
    pub reason: GraphicsReason,
}

impl Default for GraphicsDetection {
    fn default() -> Self {
        Self {
            mode: GraphicsMode::Off,
            cell_pixels: None,
            reason: GraphicsReason::QueryUnavailable,
        }
    }
}

impl GraphicsDetection {
    /// Build an image picker from startup detection without querying the terminal again.
    #[allow(deprecated)]
    pub fn picker(&self) -> Option<Picker> {
        let cell_pixels = self.cell_pixels?;
        let protocol = self.mode.protocol_type()?;
        let mut picker = Picker::from_fontsize(FontSize::new(cell_pixels.width, cell_pixels.height));
        picker.set_protocol_type(protocol);
        if self.mode == GraphicsMode::Sixel {
            picker.set_background_color(Some(Rgba([0, 0, 0, 255])));
        }
        Some(picker)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetectionInput {
    pub configured: GraphicsPreference,
    pub override_mode: Option<GraphicsPreference>,
    pub is_terminal: bool,
    pub in_tmux: bool,
    pub in_screen: bool,
    pub query: Option<QueryResult>,
    pub fallback_cell_pixels: Option<CellPixels>,
}

pub fn decide_graphics(input: DetectionInput) -> GraphicsDetection {
    let preference = input.override_mode.unwrap_or(input.configured);
    if preference == GraphicsPreference::Off {
        return off(GraphicsReason::Disabled);
    }
    if !input.is_terminal {
        return off(GraphicsReason::NotTerminal);
    }
    if preference == GraphicsPreference::Auto && (input.in_tmux || input.in_screen) {
        return off(GraphicsReason::MultiplexerAuto);
    }

    if preference == GraphicsPreference::Auto {
        let Some(query) = input.query else {
            return off(GraphicsReason::QueryUnavailable);
        };
        let mode = query.preferred_mode();
        if mode == GraphicsMode::Off {
            return off(GraphicsReason::NoImageProtocol);
        }
        let Some(cell_pixels) = query.cell_pixels.or(input.fallback_cell_pixels) else {
            return off(GraphicsReason::MissingCellSize(mode));
        };
        return GraphicsDetection {
            mode,
            cell_pixels: Some(cell_pixels),
            reason: GraphicsReason::Detected(mode),
        };
    }

    let mode = match preference {
        GraphicsPreference::Kitty => GraphicsMode::Kitty,
        GraphicsPreference::Sixel => GraphicsMode::Sixel,
        GraphicsPreference::Iterm2 => GraphicsMode::Iterm2,
        GraphicsPreference::Auto | GraphicsPreference::Off => unreachable!(),
    };
    let queried_cell_pixels = input.query.and_then(|query| query.cell_pixels);
    let (cell_pixels, reason) = if let Some(cell_pixels) = queried_cell_pixels {
        (cell_pixels, GraphicsReason::Explicit(mode))
    } else if let Some(cell_pixels) = input.fallback_cell_pixels {
        (cell_pixels, GraphicsReason::ExplicitWithWindowSize)
    } else {
        return off(GraphicsReason::MissingCellSize(mode));
    };
    GraphicsDetection {
        mode,
        cell_pixels: Some(cell_pixels),
        reason,
    }
}

const fn off(reason: GraphicsReason) -> GraphicsDetection {
    GraphicsDetection {
        mode: GraphicsMode::Off,
        cell_pixels: None,
        reason,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalPixelSize {
    pub columns: u16,
    pub rows: u16,
    pub width: u16,
    pub height: u16,
}

pub fn update_cell_pixels(previous: Option<CellPixels>, size: TerminalPixelSize) -> Option<CellPixels> {
    if size.columns == 0 || size.rows == 0 || size.width == 0 || size.height == 0 {
        return previous;
    }
    let next = CellPixels::new(size.width / size.columns, size.height / size.rows);
    if next.width == 0 || next.height == 0 { previous } else { Some(next) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalEnvironment {
    pub is_terminal: bool,
    pub in_tmux: bool,
    pub in_screen: bool,
    pub ssh: bool,
    pub term: Option<String>,
    pub term_program: Option<String>,
}

impl TerminalEnvironment {
    pub fn current() -> Self {
        Self {
            is_terminal: io::stdin().is_terminal() && io::stdout().is_terminal(),
            in_tmux: nonempty_env("TMUX"),
            in_screen: nonempty_env("STY"),
            ssh: nonempty_env("SSH_CONNECTION") || nonempty_env("SSH_CLIENT") || nonempty_env("SSH_TTY"),
            term: std::env::var("TERM").ok(),
            term_program: std::env::var("TERM_PROGRAM").ok(),
        }
    }
}

fn nonempty_env(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| !value.is_empty())
}

pub struct RuntimeDetection {
    pub result: GraphicsDetection,
    pub environment: TerminalEnvironment,
    pub picker: Option<Picker>,
}

/// Query stdio at most once, then apply the pure detection rules.
pub fn detect_stdio(configured: GraphicsPreference, override_mode: Option<GraphicsPreference>) -> RuntimeDetection {
    let environment = TerminalEnvironment::current();
    let effective = override_mode.unwrap_or(configured);
    let should_query = environment.is_terminal
        && effective != GraphicsPreference::Off
        && !(effective == GraphicsPreference::Auto && (environment.in_tmux || environment.in_screen));

    let mut picker = should_query
        .then(|| {
            Picker::from_query_stdio_with_options(QueryStdioOptions {
                timeout: QUERY_TIMEOUT,
                ..QueryStdioOptions::default()
            })
        })
        .and_then(Result::ok);
    let query = picker.as_ref().map(query_result_from_picker);
    let fallback_cell_pixels = terminal_cell_pixels();
    let result = decide_graphics(DetectionInput {
        configured,
        override_mode,
        is_terminal: environment.is_terminal,
        in_tmux: environment.in_tmux,
        in_screen: environment.in_screen,
        query,
        fallback_cell_pixels,
    });

    if let Some(protocol_type) = result.mode.protocol_type() {
        if let Some(current) = picker.as_mut() {
            current.set_protocol_type(protocol_type);
        } else if let Some(cell) = result.cell_pixels {
            #[allow(deprecated)]
            let mut replacement = Picker::from_fontsize(FontSize::new(cell.width, cell.height));
            replacement.set_protocol_type(protocol_type);
            picker = Some(replacement);
        }
    } else {
        picker = None;
    }

    RuntimeDetection { result, environment, picker }
}

fn query_result_from_picker(picker: &Picker) -> QueryResult {
    let mut result = QueryResult {
        kitty: picker.capabilities().contains(&Capability::Kitty),
        iterm2: false,
        sixel: picker.capabilities().contains(&Capability::Sixel),
        cell_pixels: picker.capabilities().iter().find_map(|capability| match capability {
            Capability::CellSize(Some((width, height))) => Some(CellPixels::new(*width, *height)),
            _ => None,
        }),
    };
    match picker.protocol_type() {
        ProtocolType::Kitty => result.kitty = true,
        ProtocolType::Iterm2 => result.iterm2 = true,
        ProtocolType::Sixel => result.sixel = true,
        ProtocolType::Halfblocks => {}
    }
    result
}

pub fn terminal_cell_pixels() -> Option<CellPixels> {
    let size = ratatui::crossterm::terminal::window_size().ok()?;
    update_cell_pixels(
        None,
        TerminalPixelSize {
            columns: size.columns,
            rows: size.rows,
            width: size.width,
            height: size.height,
        },
    )
}

/// Build the protocol test image at an exact terminal-cell pixel multiple.
pub fn make_demo_image(cell: CellPixels, columns: u16, rows: u16) -> DynamicImage {
    let width = u32::from(cell.width) * u32::from(columns);
    let height = u32::from(cell.height) * u32::from(rows);
    let mut image = RgbaImage::new(width, height);
    let x_denominator = width.saturating_sub(1).max(1);
    let y_denominator = height.saturating_sub(1).max(1);
    for y in 0..height {
        for x in 0..width {
            let red = (x * 220 / x_denominator) as u8;
            let green = (y * 220 / y_denominator) as u8;
            let blue = 224_u8.saturating_sub(((x + y) % 128) as u8);
            image.put_pixel(x, y, Rgba([red, green, blue, 255]));
        }
    }

    for x in 0..width {
        if x % u32::from(cell.width.max(1)) == 0 {
            for y in 0..height {
                image.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
    }
    for y in 0..height {
        if y % u32::from(cell.height.max(1)) == 0 {
            for x in 0..width {
                image.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
    }

    if width > 0 && height > 0 {
        for (x, y, color) in [
            (0, 0, [255, 64, 64, 255]),
            (width - 1, 0, [64, 255, 64, 255]),
            (0, height - 1, [64, 128, 255, 255]),
            (width - 1, height - 1, [255, 224, 64, 255]),
        ] {
            image.put_pixel(x, y, Rgba(color));
        }
    }
    DynamicImage::ImageRgba8(image)
}
