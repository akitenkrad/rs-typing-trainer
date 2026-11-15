//! Application-wide error type.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to parse config TOML: {0}")]
    ConfigParse(#[from] toml::de::Error),

    #[error("Failed to edit config TOML: {0}")]
    ConfigEdit(#[from] toml_edit::TomlError),

    #[error("Config error: {0}")]
    Config(#[from] ConfigError),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("Record error: {0}")]
    Record(String),
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("charset is empty")]
    EmptyCharset,

    #[error("duration_seconds must be > 0 (got {0})")]
    InvalidDuration(u64),

    #[error("word_timeout_seconds must be > 0 (got {0})")]
    InvalidWordTimeout(u64),

    #[error("word_length must be >= 1 (got {0})")]
    InvalidWordLength(usize),

    #[error("allow_repeat = false requires charset grapheme count ({count}) >= word_length ({length})")]
    CharsetTooSmall { count: usize, length: usize },

    #[error("unknown theme: {0} (expected: default | mono | neon)")]
    UnknownTheme(String),

    #[error("unknown keyboard {field}: {value}")]
    UnknownKeyboardSetting { field: &'static str, value: String },
}

pub type AppResult<T> = Result<T, AppError>;
