//! Configuration loading and validation.
//!
//! Reads `config.toml` from the current directory or
//! `~/.config/rs-typing-trainer/config.toml`. Falls back to defaults if neither
//! exists. The character set is decomposed into Unicode grapheme clusters so
//! symbols, combining characters and emoji can be supported transparently.
//!
//! Note: config and data paths are anchored at `$HOME/.config/` and
//! `$HOME/.local/share/` directly rather than via `dirs::config_dir()` /
//! `dirs::data_dir()`, so the layout matches the XDG Base Directory style on
//! every platform (including macOS).

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, Formatted, Item, Value};
use unicode_segmentation::UnicodeSegmentation;

use crate::error::{AppError, AppResult, ConfigError};
use crate::i18n::Lang;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub keyboard: KeyboardConfig,
    #[serde(default)]
    pub session: SessionConfig,
    #[serde(default)]
    pub mode: ModeConfig,
    #[serde(default)]
    pub data: DataConfig,
    #[serde(default)]
    pub practice: PracticeConfig,
    #[serde(default)]
    pub analysis: AnalysisConfig,
    #[serde(default)]
    pub adaptive: AdaptiveConfig,
    #[serde(default)]
    pub pacer: PacerConfig,
    #[serde(default)]
    pub unlock: UnlockConfig,
    #[serde(default)]
    pub gate: GateConfig,
    #[serde(default)]
    pub rhythm: RhythmConfig,
    #[serde(default)]
    pub burst: BurstConfig,
    #[serde(default)]
    pub endurance: EnduranceConfig,
    /// Legacy v1 section retained as a compatibility view.
    #[serde(default)]
    pub game: GameConfig,
    /// Legacy v1 section retained as a compatibility view.
    #[serde(default)]
    pub characters: CharactersConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub metrics: MetricsConfig,
    #[serde(default)]
    pub storage: StorageConfig,

    /// Charset decomposed into grapheme clusters. Populated after load.
    #[serde(skip)]
    pub charset_graphemes: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GeneralConfig {
    #[serde(default)]
    pub lang: Lang,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardConfig {
    #[serde(default = "default_layout")]
    pub layout: String,
    #[serde(default = "default_input_layout")]
    pub input_layout: String,
    #[serde(default = "default_geometry")]
    pub geometry: String,
    #[serde(default = "default_keyboard_show")]
    pub show: String,
    #[serde(default = "default_heatmap")]
    pub heatmap: String,
    #[serde(default = "default_true")]
    pub finger_guide: bool,
    #[serde(default = "default_digit6_finger")]
    pub digit6_finger: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ErrorMode {
    #[default]
    Stop,
    Advance,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionConfig {
    #[serde(default = "default_duration_seconds")]
    pub duration_seconds: u64,
    #[serde(default = "default_item_timeout_seconds")]
    pub item_timeout_seconds: u64,
    #[serde(default)]
    pub error_mode: ErrorMode,
    #[serde(default = "default_preview_items")]
    pub preview_items: usize,
    #[serde(default)]
    pub blind: bool,
    #[serde(default)]
    pub retry_on_error: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModeKind {
    #[default]
    Random,
    Words,
    Romaji,
    Sentence,
    Ngram,
    Adaptive,
    Unlock,
    Rhythm,
    Burst,
    Endurance,
}

impl ModeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Random => "random",
            Self::Words => "words",
            Self::Romaji => "romaji",
            Self::Sentence => "sentence",
            Self::Ngram => "ngram",
            Self::Adaptive => "adaptive",
            Self::Unlock => "unlock",
            Self::Rhythm => "rhythm",
            Self::Burst => "burst",
            Self::Endurance => "endurance",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyFilter {
    #[default]
    Include,
    Only,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModeConfig {
    #[serde(default)]
    pub kind: ModeKind,
    #[serde(default = "default_mode_keys")]
    pub keys: String,
    #[serde(default)]
    pub key_filter: KeyFilter,
    #[serde(default = "default_base_keys")]
    pub base_keys: String,
    #[serde(default)]
    pub random: RandomModeConfig,
    #[serde(default)]
    pub ngram: NgramModeConfig,
    #[serde(default)]
    pub romaji: RomajiModeConfig,
    #[serde(default)]
    pub sentence: SentenceModeConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RandomModeConfig {
    #[serde(default = "default_word_length")]
    pub word_length: usize,
    #[serde(default = "default_true")]
    pub allow_repeat: bool,
    #[serde(default)]
    pub allow_consecutive_same_word: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NgramModeConfig {
    #[serde(default = "default_ngram_n")]
    pub n: usize,
    #[serde(default = "default_ngram_source")]
    pub source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RomajiModeConfig {
    #[serde(default = "default_true")]
    pub accept_apostrophe: bool,
    #[serde(default = "default_true")]
    pub accept_xn: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SentenceModeConfig {
    #[serde(default = "default_sentence_lang")]
    pub lang: String,
    #[serde(default)]
    pub hide_typed: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DataConfig {
    #[serde(default)]
    pub words_en: Option<PathBuf>,
    #[serde(default)]
    pub words_ja: Option<PathBuf>,
    #[serde(default)]
    pub sentences_en: Option<PathBuf>,
    #[serde(default)]
    pub sentences_ja: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PracticeConfig {
    #[serde(default = "default_true")]
    pub daily_rotation: bool,
    #[serde(default = "default_core_ratio")]
    pub core_ratio: f64,
    #[serde(default = "default_true")]
    pub weekly_theme: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct AnalysisConfig {
    #[serde(default = "default_pause_ms")]
    pub pause_ms: u64,
    #[serde(default = "default_shrinkage_k")]
    pub shrinkage_k: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdaptiveConfig {
    #[serde(default)]
    pub error_weight: Option<f64>,
    #[serde(default = "default_drill_ratio")]
    pub drill_ratio: f64,
    #[serde(default = "default_adaptive_lang")]
    pub lang: String,
    #[serde(default = "default_adaptive_max_share")]
    pub max_share: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PacerConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_pacer_ratio")]
    pub ratio: f64,
    #[serde(default = "default_pacer_min_wpm")]
    pub min_wpm: f64,
    #[serde(skip)]
    pub target_wpm: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnlockOrder {
    #[default]
    Frequency,
    Rows,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnlockConfig {
    #[serde(default)]
    pub order: UnlockOrder,
    #[serde(default = "default_unlock_target_wpm")]
    pub target_wpm: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GateConfig {
    #[serde(default)]
    pub min_accuracy: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RhythmConfig {
    #[serde(default)]
    pub bpm: Option<f64>,
    #[serde(default)]
    pub bell: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BurstConfig {
    #[serde(default = "default_burst_words")]
    pub words: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnduranceConfig {
    #[serde(default = "default_endurance_minutes")]
    pub minutes: u64,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            pause_ms: default_pause_ms(),
            shrinkage_k: default_shrinkage_k(),
        }
    }
}

impl Default for AdaptiveConfig {
    fn default() -> Self {
        Self {
            error_weight: None,
            drill_ratio: default_drill_ratio(),
            lang: default_adaptive_lang(),
            max_share: default_adaptive_max_share(),
        }
    }
}

impl Default for BurstConfig {
    fn default() -> Self {
        Self {
            words: default_burst_words(),
        }
    }
}

impl Default for EnduranceConfig {
    fn default() -> Self {
        Self {
            minutes: default_endurance_minutes(),
        }
    }
}

impl Default for PacerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            ratio: default_pacer_ratio(),
            min_wpm: default_pacer_min_wpm(),
            target_wpm: None,
        }
    }
}

impl Default for UnlockConfig {
    fn default() -> Self {
        Self {
            order: UnlockOrder::Frequency,
            target_wpm: default_unlock_target_wpm(),
        }
    }
}

impl Default for PracticeConfig {
    fn default() -> Self {
        Self {
            daily_rotation: true,
            core_ratio: default_core_ratio(),
            weekly_theme: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameConfig {
    pub duration_seconds: u64,
    pub word_timeout_seconds: u64,
    pub word_length: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharactersConfig {
    pub charset: String,
    pub allow_repeat: bool,
    pub allow_consecutive_same_word: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default = "default_true")]
    pub animation_enabled: bool,
    #[serde(default = "default_animation_speed")]
    pub animation_speed: f32,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_true")]
    pub show_next_preview: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetricsConfig {
    /// Legacy v1 database-path override. `[storage] db_path` takes precedence.
    #[serde(default)]
    pub db_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Preferred v2 override. Takes precedence over `[metrics] db_path`.
    #[serde(default)]
    pub db_path: Option<PathBuf>,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            duration_seconds: 60,
            word_timeout_seconds: 5,
            word_length: 3,
        }
    }
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            duration_seconds: default_duration_seconds(),
            item_timeout_seconds: default_item_timeout_seconds(),
            error_mode: ErrorMode::Stop,
            preview_items: default_preview_items(),
            blind: false,
            retry_on_error: false,
        }
    }
}

impl SessionConfig {
    pub fn duration(&self) -> Duration {
        Duration::from_secs(self.duration_seconds)
    }

    pub fn item_timeout(&self) -> Duration {
        Duration::from_secs(self.item_timeout_seconds)
    }
}

impl Default for ModeConfig {
    fn default() -> Self {
        Self {
            kind: ModeKind::Random,
            keys: default_mode_keys(),
            key_filter: KeyFilter::Include,
            base_keys: default_base_keys(),
            random: RandomModeConfig::default(),
            ngram: NgramModeConfig::default(),
            romaji: RomajiModeConfig::default(),
            sentence: SentenceModeConfig::default(),
        }
    }
}

impl Default for RandomModeConfig {
    fn default() -> Self {
        Self {
            word_length: default_word_length(),
            allow_repeat: true,
            allow_consecutive_same_word: false,
        }
    }
}

impl Default for NgramModeConfig {
    fn default() -> Self {
        Self {
            n: default_ngram_n(),
            source: default_ngram_source(),
        }
    }
}

impl Default for RomajiModeConfig {
    fn default() -> Self {
        Self {
            accept_apostrophe: true,
            accept_xn: true,
        }
    }
}

impl Default for SentenceModeConfig {
    fn default() -> Self {
        Self {
            lang: default_sentence_lang(),
            hide_typed: false,
        }
    }
}

impl Default for KeyboardConfig {
    fn default() -> Self {
        Self {
            layout: default_layout(),
            input_layout: default_input_layout(),
            geometry: default_geometry(),
            show: default_keyboard_show(),
            heatmap: default_heatmap(),
            finger_guide: true,
            digit6_finger: default_digit6_finger(),
        }
    }
}

fn default_layout() -> String {
    "qwerty".to_owned()
}

fn default_duration_seconds() -> u64 {
    60
}

fn default_item_timeout_seconds() -> u64 {
    5
}

fn default_preview_items() -> usize {
    1
}

fn default_mode_keys() -> String {
    "bmwvz".to_owned()
}

fn default_base_keys() -> String {
    "home".to_owned()
}

fn default_word_length() -> usize {
    3
}

fn default_ngram_n() -> usize {
    2
}

fn default_ngram_source() -> String {
    "pairs".to_owned()
}

fn default_sentence_lang() -> String {
    "en".to_owned()
}

fn default_animation_speed() -> f32 {
    1.0
}

fn default_core_ratio() -> f64 {
    0.65
}

fn default_pause_ms() -> u64 {
    2_000
}

fn default_shrinkage_k() -> f64 {
    10.0
}

fn default_drill_ratio() -> f64 {
    0.3
}

fn default_adaptive_lang() -> String {
    "en".to_owned()
}

fn default_adaptive_max_share() -> f64 {
    0.4
}

fn default_burst_words() -> usize {
    12
}

fn default_endurance_minutes() -> u64 {
    4
}

fn default_pacer_ratio() -> f64 {
    1.07
}

fn default_pacer_min_wpm() -> f64 {
    20.0
}

fn default_unlock_target_wpm() -> f64 {
    35.0
}

fn default_theme() -> String {
    "default".to_owned()
}

fn default_input_layout() -> String {
    "same".to_owned()
}

fn default_geometry() -> String {
    "ansi".to_owned()
}

fn default_keyboard_show() -> String {
    "full".to_owned()
}

fn default_heatmap() -> String {
    "off".to_owned()
}

fn default_true() -> bool {
    true
}

fn default_digit6_finger() -> String {
    "left-index".to_owned()
}

impl Default for CharactersConfig {
    fn default() -> Self {
        Self {
            charset: "abcdefghijklmnopqrstuvwxyz".to_string(),
            allow_repeat: true,
            allow_consecutive_same_word: false,
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            animation_enabled: true,
            animation_speed: 1.0,
            theme: "default".to_string(),
            show_next_preview: true,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        let mut c = Self {
            general: GeneralConfig::default(),
            keyboard: KeyboardConfig::default(),
            session: SessionConfig::default(),
            mode: ModeConfig::default(),
            data: DataConfig::default(),
            practice: PracticeConfig::default(),
            analysis: AnalysisConfig::default(),
            adaptive: AdaptiveConfig::default(),
            pacer: PacerConfig::default(),
            unlock: UnlockConfig::default(),
            gate: GateConfig::default(),
            rhythm: RhythmConfig::default(),
            burst: BurstConfig::default(),
            endurance: EnduranceConfig::default(),
            game: GameConfig::default(),
            characters: CharactersConfig::default(),
            ui: UiConfig::default(),
            metrics: MetricsConfig::default(),
            storage: StorageConfig::default(),
            charset_graphemes: Vec::new(),
        };
        c.charset_graphemes = parse_charset(&c.characters.charset).unwrap_or_default();
        c
    }
}

impl GameConfig {
    pub fn duration(&self) -> Duration {
        Duration::from_secs(self.duration_seconds)
    }
    pub fn word_timeout(&self) -> Duration {
        Duration::from_secs(self.word_timeout_seconds)
    }
}

impl Config {
    /// Load config, falling back to defaults if no file is found.
    pub fn load(home: &Path) -> AppResult<Self> {
        Self::load_from(home, None)
    }

    /// Load an explicit file, or use the normal search path when omitted.
    pub fn load_from(home: &Path, path: Option<&Path>) -> AppResult<Self> {
        if let Some(path) = path {
            Self::from_path(path)
        } else if let Some(path) = find_config_path(home) {
            Self::from_path(&path)
        } else {
            let c = Self::default();
            c.validate()?;
            Ok(c)
        }
    }

    pub fn from_path(path: &Path) -> AppResult<Self> {
        let raw = std::fs::read_to_string(path)?;
        Self::from_toml(&raw)
    }

    pub fn from_toml(raw: &str) -> AppResult<Self> {
        let mut cfg: Config = toml::from_str(raw).map_err(AppError::ConfigParse)?;
        let root: toml::Value = toml::from_str(raw).map_err(AppError::ConfigParse)?;
        if root.get("session").is_none() {
            cfg.session.duration_seconds = cfg.game.duration_seconds;
            cfg.session.item_timeout_seconds = cfg.game.word_timeout_seconds;
        } else {
            cfg.game.duration_seconds = cfg.session.duration_seconds;
            cfg.game.word_timeout_seconds = cfg.session.item_timeout_seconds;
        }
        if root.get("mode").is_none() {
            cfg.mode.kind = ModeKind::Random;
            cfg.mode.keys.clone_from(&cfg.characters.charset);
            cfg.mode.random = RandomModeConfig {
                word_length: cfg.game.word_length,
                allow_repeat: cfg.characters.allow_repeat,
                allow_consecutive_same_word: cfg.characters.allow_consecutive_same_word,
            };
        } else {
            cfg.game.word_length = cfg.mode.random.word_length;
            cfg.characters.charset.clone_from(&cfg.mode.keys);
            cfg.characters.allow_repeat = cfg.mode.random.allow_repeat;
            cfg.characters.allow_consecutive_same_word = cfg.mode.random.allow_consecutive_same_word;
        }
        if root.get("storage").is_none() {
            cfg.storage.db_path.clone_from(&cfg.metrics.db_path);
        }
        cfg.charset_graphemes = if cfg.characters.charset.is_empty() {
            Vec::new()
        } else {
            parse_charset(&cfg.characters.charset)?
        };
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.session.duration_seconds == 0 {
            return Err(ConfigError::InvalidDuration(self.session.duration_seconds));
        }
        if self.mode.random.word_length == 0 || self.game.word_length == 0 {
            return Err(ConfigError::InvalidWordLength(self.mode.random.word_length.min(self.game.word_length)));
        }
        if self.mode.kind == ModeKind::Random
            && !self.charset_graphemes.is_empty()
            && (!self.characters.allow_repeat || !self.mode.random.allow_repeat)
            && self.charset_graphemes.len() < self.game.word_length
        {
            return Err(ConfigError::CharsetTooSmall {
                count: self.charset_graphemes.len(),
                length: self.game.word_length,
            });
        }
        match self.ui.theme.as_str() {
            "default" | "mono" | "neon" => {}
            other => return Err(ConfigError::UnknownTheme(other.to_string())),
        }
        if !matches!(self.keyboard.geometry.as_str(), "ansi" | "jis") {
            return Err(ConfigError::UnknownKeyboardSetting {
                field: "geometry",
                value: self.keyboard.geometry.clone(),
            });
        }
        if !matches!(self.keyboard.show.as_str(), "full" | "dim" | "hidden" | "auto") {
            return Err(ConfigError::UnknownKeyboardSetting {
                field: "show",
                value: self.keyboard.show.clone(),
            });
        }
        if !matches!(self.keyboard.heatmap.as_str(), "off" | "errors" | "latency") {
            return Err(ConfigError::UnknownKeyboardSetting {
                field: "heatmap",
                value: self.keyboard.heatmap.clone(),
            });
        }
        if !matches!(self.keyboard.digit6_finger.as_str(), "left-index" | "right-index") {
            return Err(ConfigError::UnknownKeyboardSetting {
                field: "digit6_finger",
                value: self.keyboard.digit6_finger.clone(),
            });
        }
        if !(0.0..=1.0).contains(&self.adaptive.drill_ratio) {
            return Err(ConfigError::UnknownKeyboardSetting {
                field: "adaptive.drill_ratio",
                value: self.adaptive.drill_ratio.to_string(),
            });
        }
        if !matches!(self.adaptive.lang.as_str(), "en" | "ja")
            || !(0.0..=1.0).contains(&self.adaptive.max_share)
            || self.adaptive.max_share == 0.0
            || self.session.preview_items > 3
            || !(10..=15).contains(&self.burst.words)
            || !(3..=5).contains(&self.endurance.minutes)
            || self.rhythm.bpm.is_some_and(|bpm| bpm <= 0.0)
        {
            return Err(ConfigError::UnknownKeyboardSetting {
                field: "training",
                value: "adaptive language/share, preview, rhythm, burst, or endurance".to_owned(),
            });
        }
        if self.pacer.ratio <= 0.0 || self.pacer.min_wpm <= 0.0 || self.unlock.target_wpm <= 0.0 || !(0.0..=1.0).contains(&self.gate.min_accuracy) {
            return Err(ConfigError::UnknownKeyboardSetting {
                field: "training",
                value: "ratio, target_wpm, or min_accuracy".to_owned(),
            });
        }
        Ok(())
    }

    /// Resolve the path to the SQLite database file used to persist sessions.
    pub fn resolve_db_path(&self, home: &Path) -> PathBuf {
        crate::record::storage::storage_plan(home, self).path
    }
}

/// Update only `[keyboard].layout`, preserving comments and unrelated keys.
pub fn save_keyboard_layout(path: &Path, layout: &str) -> AppResult<()> {
    let raw = if path.exists() { std::fs::read_to_string(path)? } else { String::new() };
    let mut document = raw.parse::<DocumentMut>()?;
    if !document.as_table().contains_key("keyboard") {
        document["keyboard"] = Item::Table(toml_edit::Table::new());
    }
    set_string_preserving_decor(&mut document["keyboard"]["layout"], layout);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, document.to_string())?;
    Ok(())
}

/// Save the values editable on the settings screen without reserializing the
/// rest of the document.
pub fn save_settings(path: &Path, config: &Config) -> AppResult<()> {
    let raw = if path.exists() { std::fs::read_to_string(path)? } else { String::new() };
    let mut document = raw.parse::<DocumentMut>()?;
    if !document.as_table().contains_key("keyboard") {
        document["keyboard"] = Item::Table(toml_edit::Table::new());
    }
    if !document.as_table().contains_key("general") {
        document["general"] = Item::Table(toml_edit::Table::new());
    }
    if !document.as_table().contains_key("session") {
        document["session"] = Item::Table(toml_edit::Table::new());
    }
    for (key, value) in [
        ("layout", config.keyboard.layout.as_str()),
        ("input_layout", config.keyboard.input_layout.as_str()),
        ("geometry", config.keyboard.geometry.as_str()),
        ("show", config.keyboard.show.as_str()),
    ] {
        set_string_preserving_decor(&mut document["keyboard"][key], value);
    }
    let language = match config.general.lang {
        Lang::En => "en",
        Lang::Ja => "ja",
    };
    set_string_preserving_decor(&mut document["general"]["lang"], language);
    set_string_preserving_decor(
        &mut document["session"]["error_mode"],
        match config.session.error_mode {
            ErrorMode::Stop => "stop",
            ErrorMode::Advance => "advance",
        },
    );
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, document.to_string())?;
    Ok(())
}

/// Update the mode choices owned by the mode/key selection screens while
/// preserving comments and unrelated keys.
pub fn save_mode_selection(path: &Path, mode: &ModeConfig) -> AppResult<()> {
    let raw = if path.exists() { std::fs::read_to_string(path)? } else { String::new() };
    let mut document = raw.parse::<DocumentMut>()?;
    if !document.as_table().contains_key("mode") {
        document["mode"] = Item::Table(toml_edit::Table::new());
    }
    set_string_preserving_decor(&mut document["mode"]["kind"], mode.kind.as_str());
    set_string_preserving_decor(&mut document["mode"]["keys"], &mode.keys);
    set_string_preserving_decor(
        &mut document["mode"]["key_filter"],
        match mode.key_filter {
            KeyFilter::Include => "include",
            KeyFilter::Only => "only",
        },
    );
    set_string_preserving_decor(&mut document["mode"]["base_keys"], &mode.base_keys);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, document.to_string())?;
    Ok(())
}

/// Persist controls edited directly on the mode-selection screen.
pub fn save_mode_controls(path: &Path, config: &Config) -> AppResult<()> {
    let raw = if path.exists() { std::fs::read_to_string(path)? } else { String::new() };
    let mut document = raw.parse::<DocumentMut>()?;
    for section in ["session", "adaptive", "pacer"] {
        if !document.as_table().contains_key(section) {
            document[section] = Item::Table(toml_edit::Table::new());
        }
    }
    set_integer_preserving_decor(&mut document["session"]["preview_items"], config.session.preview_items as i64);
    set_string_preserving_decor(&mut document["adaptive"]["lang"], &config.adaptive.lang);
    set_bool_preserving_decor(&mut document["pacer"]["enabled"], config.pacer.enabled);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, document.to_string())?;
    Ok(())
}

pub fn config_path_for_write(home: &Path, explicit: Option<&Path>) -> PathBuf {
    explicit
        .map(Path::to_path_buf)
        .or_else(|| find_config_path(home))
        .unwrap_or_else(|| home.join(".config").join("rs-typing-trainer").join("config.toml"))
}

pub(crate) fn set_string_preserving_decor(item: &mut Item, value: &str) {
    if let Some(old) = item.as_value_mut() {
        let decor = old.decor().clone();
        let mut formatted = Formatted::new(value.to_owned());
        *formatted.decor_mut() = decor;
        *old = Value::String(formatted);
    } else {
        *item = toml_edit::value(value);
    }
}

fn set_integer_preserving_decor(item: &mut Item, value: i64) {
    let decor = item.as_value().map(|old| old.decor().clone());
    *item = toml_edit::value(value);
    if let (Some(decor), Some(new)) = (decor, item.as_value_mut()) {
        *new.decor_mut() = decor;
    }
}

fn set_bool_preserving_decor(item: &mut Item, value: bool) {
    let decor = item.as_value().map(|old| old.decor().clone());
    *item = toml_edit::value(value);
    if let (Some(decor), Some(new)) = (decor, item.as_value_mut()) {
        *new.decor_mut() = decor;
    }
}

/// `$HOME/.local/share/rs-typing-trainer/sessions.db`, used when neither
/// `storage.db_path` nor legacy `metrics.db_path` overrides it. Falls back to
/// a CWD-relative path if the home directory cannot be resolved.
pub fn default_db_path(home: &Path) -> PathBuf {
    home.join(".local").join("share").join("rs-typing-trainer").join("sessions.db")
}

/// Decompose the raw charset string into grapheme clusters.
pub fn parse_charset(raw: &str) -> Result<Vec<String>, ConfigError> {
    let graphemes: Vec<String> = raw.graphemes(true).map(|s| s.to_owned()).collect();
    if graphemes.is_empty() {
        return Err(ConfigError::EmptyCharset);
    }
    Ok(graphemes)
}

fn find_config_path(home: &Path) -> Option<PathBuf> {
    let cwd = PathBuf::from("config.toml");
    if cwd.exists() {
        return Some(cwd);
    }
    let p = home.join(".config").join("rs-typing-trainer").join("config.toml");
    if p.exists() {
        return Some(p);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_alpha_charset() {
        let g = parse_charset("abc").unwrap();
        assert_eq!(g, vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_emoji_charset() {
        let g = parse_charset("🍎🍊🍋").unwrap();
        assert_eq!(g.len(), 3);
    }

    #[test]
    fn empty_charset_errors() {
        assert!(parse_charset("").is_err());
    }

    #[test]
    fn default_config_validates() {
        let c = Config::default();
        c.validate().unwrap();
    }

    #[test]
    fn no_repeat_with_small_charset_errors() {
        let mut c = Config::default();
        c.characters.charset = "ab".to_string();
        c.characters.allow_repeat = false;
        c.charset_graphemes = parse_charset(&c.characters.charset).unwrap();
        assert!(c.validate().is_err());
    }
}
