//! Color palette per theme.

use ratatui::style::Color;

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub fg: Color,
    pub muted: Color,
    pub correct: Color,
    pub mistype: Color,
    pub accent: Color,
    pub warning: Color,
    pub bg_panel: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            fg: Color::White,
            muted: Color::DarkGray,
            correct: Color::LightGreen,
            mistype: Color::LightRed,
            accent: Color::LightCyan,
            warning: Color::LightYellow,
            bg_panel: Color::Reset,
        }
    }
}

impl Theme {
    pub fn from_name(name: &str) -> Self {
        match name {
            "mono" => Self::mono(),
            "neon" => Self::neon(),
            _ => Self::default(),
        }
    }

    pub fn mono() -> Self {
        Self {
            fg: Color::White,
            muted: Color::DarkGray,
            correct: Color::Gray,
            mistype: Color::White,
            accent: Color::White,
            warning: Color::Gray,
            bg_panel: Color::Reset,
        }
    }

    pub fn neon() -> Self {
        Self {
            fg: Color::LightMagenta,
            muted: Color::Magenta,
            correct: Color::LightGreen,
            mistype: Color::LightRed,
            accent: Color::LightCyan,
            warning: Color::LightYellow,
            bg_panel: Color::Reset,
        }
    }
}
