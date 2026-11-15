//! Application state machine. Owns the current screen, the optional
//! in-progress game, and the last completed result.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use rand::rngs::ThreadRng;
use rand::{Rng, thread_rng};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

use crate::analysis::metrics::{AnalysisSettings, DetailedMetrics, analyze_records};
use crate::bench::{BenchPart, BenchResultSummary, BenchSettings, generate_version, next_parallel_index};
use crate::config::{Config, KeyFilter, KeyboardConfig, ModeConfig, ModeKind, SessionConfig};
use crate::data::{DataCatalog, DataKind, TrainingDataSource};
use crate::engine::modes::{GeneratedPrompt, ModeGenerator};
use crate::engine::romaji::RomajiJudge;
use crate::engine::session::{InputEffect, ItemLog, KeystrokeKind, KeystrokeLog, Session, SessionPhase};
use crate::error::AppResult;
use crate::game::input::{InputAction, classify};
use crate::game::word::{ExitKind, Word, WordSlot, WordState};
use crate::i18n::{Lang, Msg};
use crate::layout::{KeyboardLayout, builtin_layout, builtin_layout_names, remap_char};
use crate::metrics::stats::{GameResult, LiveMetrics};
use crate::practice::adaptive::{AdaptiveOptions, TransitionCost, build_plan};
use crate::practice::deck::{DeckOptions, KeySelection, WeeklyTheme, build_daily_deck, weekly_theme};
use crate::practice::ghost::{GhostCondition, GhostTrack, select_ghost};
use crate::practice::reading_report::{ReadingReportKind, append_reading_report, load_reading_exclusions};
use crate::practice::recovery::RecoveryMetrics;
use crate::practice::session_variants::{BurstSeries, EnduranceComparison};
use crate::practice::unlock::{UnlockState, apply_result, initial_keys, mode_config, unlock_sequence};
use crate::record::SessionRecord;
use crate::record::db::{self, SessionSummary};
use unicode_segmentation::UnicodeSegmentation;

/// Slide-in / slide-out duration for word transitions. During these phases
/// any key input is dropped (R-06).
pub const WORD_ENTER_DURATION: Duration = Duration::from_millis(200);
pub const WORD_EXIT_DURATION: Duration = Duration::from_millis(200);
pub const COUNTDOWN_DURATION: Duration = Duration::from_millis(3000);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Screen {
    Title,
    ModeSelect,
    KeyPicker,
    Countdown { remaining: f32 },
    Playing,
    Result,
    History,
    HistoryDetail,
    Settings,
    BenchSelect,
    BenchSummary,
    Analysis,
}

/// Source of the result being shown on the result screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ResultOrigin {
    /// Just-finished game — `[SPACE/ENTER]` restarts.
    JustPlayed,
    /// Loaded from history — `[SPACE/ENTER]` returns to the history list.
    History,
}

/// Sort order for the history screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HistorySort {
    DateDesc,
    DateAsc,
    WpmDesc,
    AccuracyDesc,
    MaxComboDesc,
    WordsDesc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnalysisPeriod {
    All,
    Days30,
    Days7,
}

impl HistorySort {
    pub fn label(self, lang: Lang) -> &'static str {
        match self {
            HistorySort::DateDesc => Msg::SortDateNewest.text(lang),
            HistorySort::DateAsc => Msg::SortDateOldest.text(lang),
            HistorySort::WpmDesc => Msg::SortWpm.text(lang),
            HistorySort::AccuracyDesc => Msg::SortAccuracy.text(lang),
            HistorySort::MaxComboDesc => Msg::SortMaxCombo.text(lang),
            HistorySort::WordsDesc => Msg::SortWords.text(lang),
        }
    }

    pub fn next(self) -> Self {
        match self {
            HistorySort::DateDesc => HistorySort::DateAsc,
            HistorySort::DateAsc => HistorySort::WpmDesc,
            HistorySort::WpmDesc => HistorySort::AccuracyDesc,
            HistorySort::AccuracyDesc => HistorySort::MaxComboDesc,
            HistorySort::MaxComboDesc => HistorySort::WordsDesc,
            HistorySort::WordsDesc => HistorySort::DateDesc,
        }
    }

    /// True if the sort is by an aggregate score (vs. by date), so the UI
    /// should prefix entries with their rank number.
    pub fn is_ranking(self) -> bool {
        !matches!(self, HistorySort::DateDesc | HistorySort::DateAsc)
    }
}

pub struct App {
    pub screen: Screen,
    pub config: Config,
    pub game: Option<GameState>,
    pub last_result: Option<GameResult>,
    pub legacy_wpm: Option<f64>,
    pub result_origin: ResultOrigin,
    pub last_save_id: Option<i64>,
    pub last_save_error: Option<String>,
    pub history: Vec<SessionSummary>,
    pub history_selected: usize,
    pub history_sort: HistorySort,
    pub history_load_error: Option<String>,
    pub should_quit: bool,
    pub storage_notice: bool,
    pub elapsed_total: Duration,
    pub rng: ThreadRng,
    pub layouts: BTreeMap<String, KeyboardLayout>,
    pub release_events_supported: bool,
    pub config_path: Option<PathBuf>,
    pub title_selected: usize,
    pub settings_selected: usize,
    pub settings_save_error: Option<String>,
    pub mode_selected: usize,
    pub key_cursor_row: usize,
    pub key_cursor_col: usize,
    pub mode_warning: Option<String>,
    pub practice_theme: WeeklyTheme,
    pub home_dir: PathBuf,
    pub bench_include_d: bool,
    pub bench_selected: usize,
    pub active_bench_id: Option<String>,
    pub bench_comparison: Option<crate::bench::BenchComparison>,
    pub bench_results: Vec<BenchResultSummary>,
    pub analysis_tab: usize,
    pub analysis_period: AnalysisPeriod,
    pub analysis_mode_filter: Option<String>,
    pub analysis_records: Vec<SessionRecord>,
    pub analysis_details: DetailedMetrics,
    pub analysis_summary: crate::analysis::summary::SummaryReport,
    pub analysis_summary_scroll: u16,
    pub analysis_transition_selected: usize,
    pub analysis_keyboard_latency: bool,
    pub adaptive_focus: Vec<TransitionCost>,
    pub adaptive_result: Vec<AdaptiveResultRow>,
    pub next_pacer_target_wpm: Option<f64>,
    pub gate_failed: bool,
    pub unlock_state: Option<UnlockState>,
    pub new_unlocked_key: Option<char>,
    pub keyboard_fade: HashMap<String, crate::practice::keyboard_fade::KeyFade>,
    pub rhythm_bpm: Option<f64>,
    pub rhythm_deviation_ms: Option<f64>,
    pub burst_series: BurstSeries,
    pub endurance_comparison: Option<EnduranceComparison>,
    pub recovery_metrics: RecoveryMetrics,
    pub recommend_preview: bool,
    pub recommend_rhythm: bool,
    analysis_as_of: chrono::NaiveDate,
    bench_run: Option<BenchRun>,
    settings_original: Option<(KeyboardConfig, String, Lang, crate::config::ErrorMode)>,
}

pub struct GameState {
    pub session: Session,
    pub current_word: WordSlot,
    pub next_word: Word,
    pub current_prompt: GeneratedPrompt,
    pub next_prompt: GeneratedPrompt,
    pub mode_generator: ModeGenerator,
    pub preview_prompts: Vec<GeneratedPrompt>,
    pub typed_index: usize,
    pub mistypes_in_word: u32,
    pub started_at: Instant,
    pub started_wall: chrono::DateTime<chrono::Utc>,
    pub word_started_at: Instant,
    pub metrics: LiveMetrics,
    pub combo: u32,
    pub last_mistype_flash: Option<Instant>,
    /// The grapheme that was wrongly typed at `last_mistype_flash`. Cleared
    /// after the flash window elapses or the word advances.
    pub last_mistype_char: Option<String>,
    /// The grapheme that was expected when `last_mistype_char` was typed.
    pub last_mistype_expected: Option<String>,
    pub last_key_flash: Option<Instant>,
    pub last_pressed_code: Option<String>,
    pub last_expected_code: Option<String>,
    pub last_key_correct: bool,
    pub ime_warning: bool,
    pub bench_prompts: Option<Vec<GeneratedPrompt>>,
    pub bench_cursor: usize,
    pub ghost: Option<GhostTrack>,
    /// Last event-loop time injected into the session, used by deterministic UI timing.
    pub elapsed: Duration,
    pub retry_prompt: Option<GeneratedPrompt>,
    pub current_is_retry: bool,
    pub last_rhythm_beat: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AdaptiveResultRow {
    pub from: String,
    pub to: String,
    pub session_iki_ms: Option<f64>,
    pub prior_estimated_iki_ms: f64,
}

struct BenchRun {
    include_d: bool,
    parallel_index: u8,
    part: BenchPart,
    original_session: SessionConfig,
    original_mode: ModeConfig,
}

impl App {
    pub fn new(config: Config, home_dir: PathBuf) -> Self {
        Self::new_at(config, home_dir, chrono::Local::now().date_naive())
    }

    pub fn new_at(config: Config, home_dir: PathBuf, date: chrono::NaiveDate) -> Self {
        let mut app = Self::new_without_storage(config, home_dir, date);
        app.refresh_recommendations();
        app
    }

    fn new_without_storage(config: Config, home_dir: PathBuf, date: chrono::NaiveDate) -> Self {
        let mode_selected = mode_index(config.mode.kind);
        let practice_theme = weekly_theme(date, config.practice.weekly_theme);
        let layouts = builtin_layout_names()
            .into_iter()
            .map(|name| (name.to_owned(), builtin_layout(name).expect("bundled layouts must be valid")))
            .collect();
        Self {
            screen: Screen::Title,
            config,
            game: None,
            last_result: None,
            legacy_wpm: None,
            result_origin: ResultOrigin::JustPlayed,
            last_save_id: None,
            last_save_error: None,
            history: Vec::new(),
            history_selected: 0,
            history_sort: HistorySort::DateDesc,
            history_load_error: None,
            should_quit: false,
            storage_notice: false,
            elapsed_total: Duration::ZERO,
            rng: thread_rng(),
            layouts,
            release_events_supported: false,
            config_path: None,
            title_selected: 0,
            settings_selected: 0,
            settings_save_error: None,
            mode_selected,
            key_cursor_row: 0,
            key_cursor_col: 0,
            mode_warning: None,
            practice_theme,
            home_dir,
            bench_include_d: false,
            bench_selected: 0,
            active_bench_id: None,
            bench_comparison: None,
            bench_results: Vec::new(),
            analysis_tab: 0,
            analysis_period: AnalysisPeriod::All,
            analysis_mode_filter: None,
            analysis_records: Vec::new(),
            analysis_details: DetailedMetrics::default(),
            analysis_summary: crate::analysis::summary::SummaryReport::default(),
            analysis_summary_scroll: 0,
            analysis_transition_selected: 0,
            analysis_keyboard_latency: false,
            adaptive_focus: Vec::new(),
            adaptive_result: Vec::new(),
            next_pacer_target_wpm: None,
            gate_failed: false,
            unlock_state: None,
            new_unlocked_key: None,
            keyboard_fade: HashMap::new(),
            rhythm_bpm: None,
            rhythm_deviation_ms: None,
            burst_series: BurstSeries::default(),
            endurance_comparison: None,
            recovery_metrics: RecoveryMetrics::default(),
            recommend_preview: false,
            recommend_rhythm: false,
            analysis_as_of: date,
            bench_run: None,
            settings_original: None,
        }
    }

    /// Build the ordinary playing state for a portable record without
    /// loading configuration, history, or any other user data.
    pub(crate) fn for_replay(record: &SessionRecord, lang: Lang) -> Self {
        let mut config = Config::default();
        config.general.lang = lang;
        config.keyboard.layout = record.header.layout.clone();
        config.keyboard.input_layout = record.header.input_layout.clone();
        config.keyboard.geometry = record.header.geometry.clone();
        config.keyboard.show = "full".to_owned();
        config.keyboard.finger_guide = true;
        config.session.error_mode = record.log.error_mode;
        config.session.item_timeout_seconds = 0;
        config.session.retry_on_error = false;
        config.session.duration_seconds = config
            .session
            .duration_seconds
            .max(record.log.duration_us.saturating_add(999_999) / 1_000_000);
        config.ui.animation_enabled = false;
        config.mode.kind = replay_mode_kind(&record.header.mode);
        if record.log.items.iter().any(|item| item.reading.is_some()) {
            config.mode.sentence.lang = "ja".to_owned();
            config.adaptive.lang = "ja".to_owned();
        }
        if let Ok(params) = serde_json::from_str::<serde_json::Value>(&record.header.mode_params) {
            if let Some(lang) = params.get("lang").and_then(serde_json::Value::as_str) {
                config.mode.sentence.lang = lang.to_owned();
            }
            config.pacer.target_wpm = params.get("pacer_target_wpm").and_then(serde_json::Value::as_f64);
        }

        let date = chrono::NaiveDate::from_ymd_opt(1970, 1, 1).expect("valid replay date");
        let mut app = Self::new_without_storage(config, PathBuf::new(), date);
        let prompts = record.log.items.iter().map(replay_prompt).collect::<Vec<_>>();
        let first_prompt = prompts
            .first()
            .cloned()
            .unwrap_or_else(|| GeneratedPrompt::fixed(" ".to_owned(), None, " ".to_owned()));
        let next_prompt = prompts.get(1).cloned().unwrap_or_else(|| first_prompt.clone());
        let preview_prompts = if prompts.len() > 1 {
            prompts[1..].to_vec()
        } else {
            vec![next_prompt.clone()]
        };
        let catalog = DataCatalog::load(&crate::config::DataConfig::default(), std::path::Path::new("")).expect("bundled prompt data must be valid");
        let mode_generator = ModeGenerator::new(app.config.mode.clone(), catalog, app.practice_layout().clone(), 0);
        let duration = app.config.session.duration();
        let first_shown = record
            .log
            .items
            .first()
            .map_or(Duration::ZERO, |item| Duration::from_micros(item.shown_us));
        let mut session = Session::with_error_mode(duration, Duration::ZERO, first_prompt.clone().into_item(), record.log.error_mode);
        session.activate(first_shown);
        let started_at = Instant::now();
        let started_wall = chrono::DateTime::parse_from_rfc3339(&record.header.started_at)
            .map(|value| value.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());
        app.game = Some(GameState {
            session,
            current_word: WordSlot {
                word: word_from_text(&first_prompt.target),
                state: WordState::Active,
            },
            next_word: word_from_text(&next_prompt.target),
            current_prompt: first_prompt,
            next_prompt,
            mode_generator,
            preview_prompts,
            typed_index: 0,
            mistypes_in_word: 0,
            started_at,
            started_wall,
            word_started_at: started_at.checked_add(first_shown).unwrap_or(started_at),
            metrics: LiveMetrics::default(),
            combo: 0,
            last_mistype_flash: None,
            last_mistype_char: None,
            last_mistype_expected: None,
            last_key_flash: None,
            last_pressed_code: None,
            last_expected_code: None,
            last_key_correct: false,
            ime_warning: false,
            bench_prompts: None,
            bench_cursor: 0,
            ghost: None,
            elapsed: first_shown,
            retry_prompt: None,
            current_is_retry: false,
            last_rhythm_beat: 0,
        });
        app.screen = Screen::Playing;
        app
    }

    pub(crate) fn replay_show_item(&mut self, item: &ItemLog, following: &[ItemLog]) {
        let prompt = replay_prompt(item);
        let next_prompt = following.first().map(replay_prompt).unwrap_or_else(|| prompt.clone());
        let elapsed = Duration::from_micros(item.shown_us);
        let Some(game) = self.game.as_mut() else { return };
        if matches!(game.session.phase(), SessionPhase::Entering | SessionPhase::Active) {
            game.session.skip_item(elapsed);
        }
        game.session.advance(prompt.clone().into_item(), elapsed);
        game.session.activate(elapsed);
        game.current_word = WordSlot {
            word: word_from_text(&prompt.target),
            state: WordState::Active,
        };
        game.current_prompt = prompt;
        game.next_word = word_from_text(&next_prompt.target);
        game.next_prompt = next_prompt;
        game.preview_prompts = if following.is_empty() {
            vec![game.next_prompt.clone()]
        } else {
            following.iter().map(replay_prompt).collect()
        };
        game.word_started_at = game.started_at.checked_add(elapsed).unwrap_or(game.started_at);
        game.elapsed = elapsed;
        reset_item_display_state(game);
    }

    pub(crate) fn replay_apply_key(&mut self, key: &KeystrokeLog) {
        let pause_ms = self.config.analysis.pause_ms;
        let practice_layout = self.practice_layout().clone();
        let Some(game) = self.game.as_mut() else { return };
        let elapsed = Duration::from_micros(key.t_us);
        game.elapsed = elapsed;
        let now = game.started_at.checked_add(elapsed).unwrap_or(game.started_at);
        let effect = match key.kind {
            KeystrokeKind::Char => game
                .session
                .input_char_with_code(elapsed, key.typed.as_deref().unwrap_or_default(), key.code.clone()),
            KeystrokeKind::Backspace => game.session.input_backspace_with_code(elapsed, key.code.clone()),
        };
        if effect == InputEffect::Dropped {
            return;
        }
        game.last_key_flash = Some(now);
        game.last_pressed_code = key.code.clone();
        game.last_expected_code = key
            .expected
            .as_deref()
            .and_then(|value| value.chars().next())
            .and_then(|value| practice_layout.stroke_for(value).map(|stroke| stroke.code));
        game.last_key_correct = key.correct;
        match effect {
            InputEffect::Dropped => {}
            InputEffect::Correct => game.typed_index = game.session.position(),
            InputEffect::Mistype => {
                game.mistypes_in_word += 1;
                game.last_mistype_flash = Some(now);
                game.last_mistype_char = key.typed.clone();
                game.last_mistype_expected = key.expected.clone();
            }
            InputEffect::ItemCompleted | InputEffect::ItemCompletedMistyped => {
                if effect == InputEffect::ItemCompletedMistyped {
                    game.mistypes_in_word += 1;
                }
                game.typed_index = game.session.position();
                game.current_word.state = WordState::Exiting {
                    kind: if game.mistypes_in_word == 0 {
                        ExitKind::Correct
                    } else {
                        ExitKind::Mistyped
                    },
                    progress: 0.0,
                };
            }
            InputEffect::Backspace => {}
        }
        Self::refresh_live_metrics(game, pause_ms);
    }

    pub(crate) fn replay_finish(&mut self, duration: Duration) {
        if let Some(game) = self.game.as_mut() {
            game.elapsed = duration;
        }
    }

    pub fn with_storage_notice(mut self, copied: bool) -> Self {
        self.storage_notice = copied;
        self
    }

    pub fn with_release_events(mut self, supported: bool) -> Self {
        self.release_events_supported = supported;
        self
    }

    pub fn with_config_path(mut self, path: PathBuf) -> Self {
        self.config_path = Some(path);
        self
    }

    pub fn add_layouts(&mut self, layouts: impl IntoIterator<Item = KeyboardLayout>) {
        for layout in layouts {
            self.layouts.insert(layout.name().to_owned(), layout);
        }
    }

    pub fn practice_layout(&self) -> &KeyboardLayout {
        self.layouts
            .get(&self.config.keyboard.layout)
            .or_else(|| self.layouts.get("qwerty"))
            .expect("qwerty bundled layout must exist")
    }

    pub fn input_layout(&self) -> Option<&KeyboardLayout> {
        if self.config.keyboard.input_layout == "same" {
            None
        } else {
            self.layouts.get(&self.config.keyboard.input_layout)
        }
    }

    pub fn db_path(&self) -> PathBuf {
        self.config.resolve_db_path(&self.home_dir)
    }

    fn refresh_recommendations(&mut self) {
        let records = db::load_records_filtered(&self.db_path(), None, None).unwrap_or_default();
        let geometry = crate::layout::geometry::geometry(&self.config.keyboard.geometry).or_else(|| crate::layout::geometry::geometry("ansi"));
        let Some(geometry) = geometry else { return };
        let bench_a = records
            .iter()
            .filter(|record| record.header.bench_id.as_deref().is_some_and(|id| id.contains("/A/")))
            .collect::<Vec<_>>();
        let reaction_scores = bench_a
            .iter()
            .filter_map(|record| {
                let details = analyze_records(
                    std::slice::from_ref(*record),
                    self.practice_layout(),
                    &geometry,
                    AnalysisSettings {
                        pause_ms: self.config.analysis.pause_ms,
                        shrinkage_k: self.config.analysis.shrinkage_k,
                    },
                );
                (details.reaction_median_ms > 0.0).then_some(-details.reaction_median_ms)
            })
            .collect::<Vec<_>>();
        self.recommend_preview = reaction_scores
            .last()
            .is_some_and(|latest| crate::practice::recommendations::recommend_preview(*latest, &reaction_scores));
        let rhythm_scores = bench_a.iter().map(|record| -record.header.iki_cv).collect::<Vec<_>>();
        self.recommend_rhythm = rhythm_scores
            .last()
            .is_some_and(|latest| crate::practice::rhythm::recommend_rhythm(*latest, &rhythm_scores));
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        self.handle_key_at(key, Instant::now());
    }

    pub fn handle_key_at(&mut self, key: KeyEvent, now: Instant) {
        if key.kind == KeyEventKind::Release {
            self.handle_release(key, now);
            return;
        }
        if key.kind != KeyEventKind::Press {
            return;
        }
        if self.screen == Screen::Playing && key.code == KeyCode::F(2) {
            self.report_current_reading(now);
            return;
        }
        let action = classify(key);
        match self.screen {
            Screen::Title => self.handle_title_key(key, action),
            Screen::ModeSelect => self.handle_mode_select_key(key, action),
            Screen::KeyPicker => self.handle_key_picker_key(key, action),
            Screen::Countdown { .. } => {
                if matches!(key.code, KeyCode::Esc) {
                    if self.bench_run.is_some() {
                        self.abort_benchmark();
                    } else {
                        self.screen = Screen::Title;
                    }
                }
            }
            Screen::Playing => self.handle_playing_key(action, now),
            Screen::Result => self.handle_result_key(key, action),
            Screen::History => self.handle_history_key(key, action),
            Screen::HistoryDetail => self.handle_history_detail_key(key, action),
            Screen::Settings => self.handle_settings_key(key, action),
            Screen::BenchSelect => self.handle_bench_select_key(key, action),
            Screen::BenchSummary => self.handle_bench_summary_key(key, action),
            Screen::Analysis => self.handle_analysis_key(key, action),
        }
    }

    fn handle_release(&mut self, key: KeyEvent, now: Instant) {
        if self.screen != Screen::Playing {
            return;
        }
        let code = match key.code {
            KeyCode::Char(character) => self
                .input_layout()
                .unwrap_or_else(|| self.practice_layout())
                .stroke_for(character)
                .map(|stroke| stroke.code),
            KeyCode::Backspace => Some("Backspace".to_owned()),
            _ => None,
        };
        let Some(code) = code else { return };
        let Some(game) = self.game.as_mut() else { return };
        let elapsed = now.saturating_duration_since(game.started_at);
        game.session.release_key(elapsed, &code);
    }

    fn handle_title_key(&mut self, key: KeyEvent, action: InputAction) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.title_selected = self.title_selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.title_selected = (self.title_selected + 1).min(5),
            KeyCode::Enter => self.activate_title_selection(),
            KeyCode::Char(' ') => self.open_mode_select(),
            KeyCode::Char('h') | KeyCode::Char('H') => self.open_history(),
            KeyCode::Char('b') | KeyCode::Char('B') => {
                self.bench_include_d = false;
                self.bench_selected = 0;
                self.screen = Screen::BenchSelect;
            }
            KeyCode::Char('a') | KeyCode::Char('A') => self.open_analysis(),
            KeyCode::Char('s') | KeyCode::Char('S') => {
                self.settings_selected = 0;
                self.settings_save_error = None;
                self.settings_original = Some((
                    self.config.keyboard.clone(),
                    self.config.display.prompt_style.clone(),
                    self.config.general.lang,
                    self.config.session.error_mode,
                ));
                self.screen = Screen::Settings;
            }
            KeyCode::Char('q') | KeyCode::Char('Q') => self.should_quit = true,
            KeyCode::Esc => self.should_quit = true,
            _ => {}
        }
        if matches!(action, InputAction::Abort) {
            self.should_quit = true;
        }
    }

    fn open_mode_select(&mut self) {
        self.mode_selected = mode_index(self.config.mode.kind);
        self.screen = Screen::ModeSelect;
    }

    fn activate_title_selection(&mut self) {
        match self.title_selected {
            0 => self.open_mode_select(),
            1 => {
                self.bench_include_d = false;
                self.bench_selected = 0;
                self.screen = Screen::BenchSelect;
            }
            2 => self.open_analysis(),
            3 => self.open_history(),
            4 => {
                self.settings_selected = 0;
                self.settings_save_error = None;
                self.settings_original = Some((
                    self.config.keyboard.clone(),
                    self.config.display.prompt_style.clone(),
                    self.config.general.lang,
                    self.config.session.error_mode,
                ));
                self.screen = Screen::Settings;
            }
            5 => self.should_quit = true,
            _ => {}
        }
    }

    fn handle_mode_select_key(&mut self, key: KeyEvent, action: InputAction) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.mode_selected = self.mode_selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.mode_selected = (self.mode_selected + 1).min(MODES.len() - 1),
            KeyCode::Char('p') | KeyCode::Char('P') => {
                let selected = MODES[self.mode_selected];
                if matches!(selected, ModeKind::Words | ModeKind::Sentence | ModeKind::Adaptive) {
                    self.config.pacer.enabled = !self.config.pacer.enabled;
                }
            }
            KeyCode::Char('z') | KeyCode::Char('Z') => {
                self.config.display.prompt_style = if self.config.display.prompt_style == "normal" {
                    "large"
                } else {
                    "normal"
                }
                .to_owned();
            }
            KeyCode::Char('l') | KeyCode::Char('L') if MODES[self.mode_selected] == ModeKind::Adaptive => {
                self.config.adaptive.lang = if self.config.adaptive.lang == "en" { "ja" } else { "en" }.to_owned();
            }
            KeyCode::Char(']') => {
                self.config.session.preview_items = (self.config.session.preview_items + 1).min(3);
            }
            KeyCode::Char('[') => {
                self.config.session.preview_items = self.config.session.preview_items.saturating_sub(1);
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.config.mode.kind = MODES[self.mode_selected];
                if self.config.mode.kind == ModeKind::Burst && self.burst_series.complete() {
                    self.burst_series = BurstSeries::default();
                }
                if self.mode_requires_keys() && self.config.mode.keys.is_empty() {
                    self.key_cursor_row = 0;
                    self.key_cursor_col = 0;
                    self.mode_warning = Some(self.select_keys_message().to_owned());
                    self.screen = Screen::KeyPicker;
                } else if matches!(
                    self.config.mode.kind,
                    ModeKind::Random
                        | ModeKind::Sentence
                        | ModeKind::Adaptive
                        | ModeKind::Unlock
                        | ModeKind::Rhythm
                        | ModeKind::Burst
                        | ModeKind::Endurance
                ) {
                    if let Some(path) = self.config_path.as_ref()
                        && let Err(error) = crate::config::save_mode_selection(path, &self.config.mode)
                    {
                        self.mode_warning = Some(error.to_string());
                        return;
                    }
                    if let Some(path) = self.config_path.as_ref()
                        && let Err(error) = crate::config::save_mode_controls(path, &self.config)
                    {
                        self.mode_warning = Some(error.to_string());
                        return;
                    }
                    self.start_countdown();
                } else {
                    self.key_cursor_row = 0;
                    self.key_cursor_col = 0;
                    self.refresh_mode_warning();
                    self.screen = Screen::KeyPicker;
                }
            }
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => self.screen = Screen::Title,
            _ => {}
        }
        if matches!(action, InputAction::Abort) {
            self.screen = Screen::Title;
        }
    }

    fn handle_bench_select_key(&mut self, key: KeyEvent, action: InputAction) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.bench_selected = self.bench_selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.bench_selected = (self.bench_selected + 1).min(1),
            KeyCode::Char('d') | KeyCode::Char('D') => self.bench_include_d = !self.bench_include_d,
            KeyCode::Enter if self.bench_selected == 0 => self.start_benchmark(),
            KeyCode::Enter => self.bench_include_d = !self.bench_include_d,
            KeyCode::Char(' ') => self.start_benchmark(),
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => self.screen = Screen::Title,
            _ => {}
        }
        if matches!(action, InputAction::Abort) {
            self.screen = Screen::Title;
        }
    }

    fn handle_analysis_key(&mut self, key: KeyEvent, action: InputAction) {
        match key.code {
            KeyCode::Tab | KeyCode::Right => self.analysis_tab = (self.analysis_tab + 1) % 6,
            KeyCode::BackTab | KeyCode::Left => self.analysis_tab = (self.analysis_tab + 5) % 6,
            KeyCode::Up | KeyCode::Char('k') if self.analysis_tab == 0 => {
                self.analysis_summary_scroll = self.analysis_summary_scroll.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') if self.analysis_tab == 0 => {
                let maximum = self.analysis_summary.lines.len().saturating_sub(1).min(u16::MAX as usize) as u16;
                self.analysis_summary_scroll = (self.analysis_summary_scroll + 1).min(maximum);
            }
            KeyCode::Up | KeyCode::Char('k') if self.analysis_tab == 3 => {
                self.analysis_transition_selected = self.analysis_transition_selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') if self.analysis_tab == 3 => {
                self.analysis_transition_selected =
                    (self.analysis_transition_selected + 1).min(self.analysis_details.bigrams.len().saturating_sub(1));
            }
            KeyCode::Enter if self.analysis_tab == 3 => self.practice_selected_transition(),
            KeyCode::Enter if self.analysis_tab == 2 => {
                self.analysis_keyboard_latency = !self.analysis_keyboard_latency;
            }
            KeyCode::Char('p') | KeyCode::Char('P') => {
                self.analysis_period = match self.analysis_period {
                    AnalysisPeriod::All => AnalysisPeriod::Days30,
                    AnalysisPeriod::Days30 => AnalysisPeriod::Days7,
                    AnalysisPeriod::Days7 => AnalysisPeriod::All,
                };
                self.reload_analysis();
            }
            KeyCode::Char('m') | KeyCode::Char('M') => {
                const MODES: [Option<&str>; 12] = [
                    None,
                    Some("random"),
                    Some("words"),
                    Some("romaji"),
                    Some("sentence"),
                    Some("ngram"),
                    Some("bench"),
                    Some("adaptive"),
                    Some("unlock"),
                    Some("rhythm"),
                    Some("burst"),
                    Some("endurance"),
                ];
                let current = MODES.iter().position(|value| *value == self.analysis_mode_filter.as_deref()).unwrap_or(0);
                self.analysis_mode_filter = MODES[(current + 1) % MODES.len()].map(str::to_owned);
                self.reload_analysis();
            }
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => self.screen = Screen::Title,
            _ => {}
        }
        if matches!(action, InputAction::Abort) {
            self.screen = Screen::Title;
        }
    }

    fn open_analysis(&mut self) {
        self.analysis_tab = 0;
        self.analysis_period = AnalysisPeriod::All;
        self.analysis_mode_filter = None;
        self.analysis_transition_selected = 0;
        self.analysis_keyboard_latency = false;
        self.analysis_summary_scroll = 0;
        self.reload_analysis();
        self.screen = Screen::Analysis;
    }

    fn reload_analysis(&mut self) {
        let today = self.analysis_as_of;
        let since = match self.analysis_period {
            AnalysisPeriod::All => None,
            AnalysisPeriod::Days30 => Some(today - chrono::Duration::days(29)),
            AnalysisPeriod::Days7 => Some(today - chrono::Duration::days(6)),
        };
        self.analysis_records = db::load_records_filtered(&self.db_path(), since, self.analysis_mode_filter.as_deref()).unwrap_or_default();
        let geometry = crate::layout::geometry::geometry(&self.config.keyboard.geometry)
            .or_else(|| crate::layout::geometry::geometry("ansi"))
            .expect("bundled ansi geometry");
        self.analysis_details = analyze_records(
            &self.analysis_records,
            self.practice_layout(),
            &geometry,
            AnalysisSettings {
                pause_ms: self.config.analysis.pause_ms,
                shrinkage_k: self.config.analysis.shrinkage_k,
            },
        );
        self.analysis_summary = crate::analysis::summary::generate_with_metrics(
            &self.analysis_records,
            &self.config,
            self.practice_layout(),
            &self.analysis_details,
            today,
            self.config.general.lang,
        );
        self.analysis_summary_scroll = 0;
        self.analysis_transition_selected = self
            .analysis_transition_selected
            .min(self.analysis_details.bigrams.len().saturating_sub(1));
    }

    fn practice_selected_transition(&mut self) {
        let mut rows = self.analysis_details.bigrams.clone();
        rows.sort_by(|left, right| right.estimated_iki_ms.total_cmp(&left.estimated_iki_ms));
        let Some(row) = rows.get(self.analysis_transition_selected) else {
            return;
        };
        self.config.mode.kind = ModeKind::Ngram;
        self.config.mode.keys = format!("{}{}", row.from, row.to);
        self.config.mode.ngram.n = 2;
        self.config.mode.ngram.source = "pairs".to_owned();
        self.key_cursor_row = 0;
        self.key_cursor_col = 0;
        self.screen = Screen::KeyPicker;
    }

    fn start_benchmark(&mut self) {
        let previous = db::latest_bench_parallel(&self.db_path(), crate::bench::BENCH_SERIES).ok().flatten();
        let run = BenchRun {
            include_d: self.bench_include_d,
            parallel_index: next_parallel_index(previous),
            part: BenchPart::A,
            original_session: self.config.session.clone(),
            original_mode: self.config.mode.clone(),
        };
        self.bench_results.clear();
        self.bench_run = Some(run);
        self.configure_bench_part();
    }

    fn configure_bench_part(&mut self) {
        let Some(run) = self.bench_run.as_ref() else { return };
        let settings = BenchSettings::fixed(run.part, &self.config);
        self.config.session.duration_seconds = settings.duration_seconds;
        self.config.session.item_timeout_seconds = settings.item_timeout_seconds;
        self.config.session.error_mode = settings.error_mode;
        self.config.session.preview_items = settings.preview_items;
        self.config.mode.kind = match run.part {
            BenchPart::A => ModeKind::Words,
            BenchPart::B => ModeKind::Ngram,
            BenchPart::C => ModeKind::Romaji,
            BenchPart::D => ModeKind::Random,
        };
        self.active_bench_id = Some(format!("{}/{}/{:02}", crate::bench::BENCH_SERIES, run.part.as_str(), run.parallel_index));
        self.start_countdown();
    }

    fn continue_benchmark(&mut self) {
        let next = self.bench_run.as_ref().and_then(|run| run.part.next(run.include_d));
        if let Some(next) = next {
            if let Some(run) = self.bench_run.as_mut() {
                run.part = next;
            }
            self.configure_bench_part();
        } else if let Some(run) = self.bench_run.take() {
            self.config.session = run.original_session;
            self.config.mode = run.original_mode;
            self.active_bench_id = None;
            self.screen = Screen::BenchSummary;
        }
    }

    fn abort_benchmark(&mut self) {
        self.game = None;
        if let Some(run) = self.bench_run.take() {
            self.config.session = run.original_session;
            self.config.mode = run.original_mode;
        }
        self.active_bench_id = None;
        self.screen = Screen::Title;
    }

    fn handle_bench_summary_key(&mut self, key: KeyEvent, action: InputAction) {
        if matches!(
            key.code,
            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q')
        ) || matches!(action, InputAction::Abort)
        {
            self.screen = Screen::Title;
        }
    }

    pub fn active_bench_part(&self) -> Option<BenchPart> {
        self.bench_run.as_ref().map(|run| run.part)
    }

    pub fn next_bench_part(&self) -> Option<BenchPart> {
        self.bench_run.as_ref().and_then(|run| run.part.next(run.include_d))
    }

    fn handle_key_picker_key(&mut self, key: KeyEvent, action: InputAction) {
        let row_lengths = self.picker_row_lengths();
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => self.key_cursor_col = self.key_cursor_col.saturating_sub(1),
            KeyCode::Right | KeyCode::Char('l') => {
                let max = row_lengths.get(self.key_cursor_row).copied().unwrap_or(1).saturating_sub(1);
                self.key_cursor_col = (self.key_cursor_col + 1).min(max);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.key_cursor_row = self.key_cursor_row.saturating_sub(1);
                let max = row_lengths.get(self.key_cursor_row).copied().unwrap_or(1).saturating_sub(1);
                self.key_cursor_col = self.key_cursor_col.min(max);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.key_cursor_row = (self.key_cursor_row + 1).min(row_lengths.len().saturating_sub(1));
                let max = row_lengths.get(self.key_cursor_row).copied().unwrap_or(1).saturating_sub(1);
                self.key_cursor_col = self.key_cursor_col.min(max);
            }
            KeyCode::Char(' ') => {
                self.toggle_picker_key();
                self.refresh_mode_warning();
            }
            KeyCode::Tab => {
                self.config.mode.key_filter = match self.config.mode.key_filter {
                    KeyFilter::Include => KeyFilter::Only,
                    KeyFilter::Only => KeyFilter::Include,
                };
                self.refresh_mode_warning();
            }
            KeyCode::Enter => {
                if self.mode_requires_keys() && self.config.mode.keys.is_empty() {
                    self.mode_warning = Some(self.select_keys_message().to_owned());
                    return;
                }
                if let Some(path) = self.config_path.as_ref()
                    && let Err(error) = crate::config::save_mode_selection(path, &self.config.mode)
                {
                    self.mode_warning = Some(error.to_string());
                    return;
                }
                if let Some(path) = self.config_path.as_ref()
                    && let Err(error) = crate::config::save_mode_controls(path, &self.config)
                {
                    self.mode_warning = Some(error.to_string());
                    return;
                }
                self.start_countdown();
            }
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => self.screen = Screen::ModeSelect,
            _ => {}
        }
        if matches!(action, InputAction::Abort) {
            self.screen = Screen::ModeSelect;
        }
    }

    fn picker_row_lengths(&self) -> Vec<usize> {
        crate::layout::geometry::geometry(&self.config.keyboard.geometry)
            .map(|geometry| geometry.rows.into_iter().skip(1).take(3).map(|row| row.len()).collect())
            .unwrap_or_else(|| vec![12, 11, 10])
    }

    pub fn picker_code(&self) -> Option<String> {
        crate::layout::geometry::geometry(&self.config.keyboard.geometry).and_then(|geometry| {
            geometry
                .rows
                .get(self.key_cursor_row + 1)?
                .get(self.key_cursor_col)
                .map(|key| key.code.to_owned())
        })
    }

    fn toggle_picker_key(&mut self) {
        let Some(code) = self.picker_code() else { return };
        let Some(character) = self.practice_layout().key(&code).map(|pair| pair.normal) else {
            return;
        };
        if let Some(position) = self.config.mode.keys.find(character) {
            self.config.mode.keys.remove(position);
        } else {
            self.config.mode.keys.push(character);
        }
    }

    fn refresh_mode_warning(&mut self) {
        if self.mode_requires_keys() && self.config.mode.keys.is_empty() {
            self.mode_warning = Some(self.select_keys_message().to_owned());
            return;
        }
        let layout = self.practice_layout().clone();
        self.mode_warning = match DataCatalog::load(&self.config.data, &self.home_dir) {
            Ok(catalog) => ModeGenerator::new(self.config.mode.clone(), catalog, layout, 0)
                .warning()
                .map(str::to_owned),
            Err(error) => Some(error.to_string()),
        };
    }

    fn mode_requires_keys(&self) -> bool {
        matches!(self.config.mode.kind, ModeKind::Random | ModeKind::Ngram)
    }

    fn select_keys_message(&self) -> &'static str {
        match self.config.general.lang {
            Lang::En => "Please select keys.",
            Lang::Ja => "キーを選んでください",
        }
    }

    fn handle_settings_key(&mut self, key: KeyEvent, action: InputAction) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.settings_selected = self.settings_selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.settings_selected = (self.settings_selected + 1).min(7);
            }
            KeyCode::Left | KeyCode::Char('h') => self.cycle_setting(-1),
            KeyCode::Right | KeyCode::Char('l') => self.cycle_setting(1),
            KeyCode::Enter => {
                if let Some(path) = self.config_path.as_ref() {
                    match crate::config::save_settings(path, &self.config) {
                        Ok(()) => {
                            self.settings_save_error = None;
                            self.settings_original = None;
                            self.screen = Screen::Title;
                        }
                        Err(error) => self.settings_save_error = Some(error.to_string()),
                    }
                } else {
                    self.settings_original = None;
                    self.screen = Screen::Title;
                }
            }
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => self.cancel_settings(),
            _ => {}
        }
        if matches!(action, InputAction::Abort) {
            self.cancel_settings();
        }
    }

    fn cancel_settings(&mut self) {
        if let Some((keyboard, prompt_style, language, error_mode)) = self.settings_original.take() {
            self.config.keyboard = keyboard;
            self.config.display.prompt_style = prompt_style;
            self.config.general.lang = language;
            self.config.session.error_mode = error_mode;
        }
        self.settings_save_error = None;
        self.screen = Screen::Title;
    }

    fn cycle_setting(&mut self, direction: isize) {
        match self.settings_selected {
            0 => {
                let options = self.layout_options();
                cycle_string(&mut self.config.keyboard.layout, &options, direction);
            }
            1 => {
                let mut options = vec!["same".to_owned()];
                options.extend(self.layout_options());
                cycle_string(&mut self.config.keyboard.input_layout, &options, direction);
            }
            2 => cycle_string(&mut self.config.keyboard.geometry, &["ansi".to_owned(), "jis".to_owned()], direction),
            3 => cycle_string(
                &mut self.config.keyboard.show,
                &["full".to_owned(), "dim".to_owned(), "hidden".to_owned(), "auto".to_owned()],
                direction,
            ),
            4 => cycle_string(
                &mut self.config.keyboard.style,
                &["keycap".to_owned(), "raised".to_owned(), "plain".to_owned()],
                direction,
            ),
            5 => cycle_string(
                &mut self.config.display.prompt_style,
                &["large".to_owned(), "normal".to_owned()],
                direction,
            ),
            6 => {
                self.config.general.lang = match self.config.general.lang {
                    Lang::En => Lang::Ja,
                    Lang::Ja => Lang::En,
                };
            }
            7 => {
                self.config.session.error_mode = match self.config.session.error_mode {
                    crate::config::ErrorMode::Stop => crate::config::ErrorMode::Advance,
                    crate::config::ErrorMode::Advance => crate::config::ErrorMode::Stop,
                };
            }
            _ => {}
        }
    }

    fn layout_options(&self) -> Vec<String> {
        let mut options = builtin_layout_names().into_iter().map(str::to_owned).collect::<Vec<_>>();
        let custom = self.layouts.keys().filter(|name| !options.contains(name)).cloned().collect::<Vec<_>>();
        options.extend(custom);
        options
    }

    fn handle_result_key(&mut self, key: KeyEvent, action: InputAction) {
        match key.code {
            KeyCode::Char(' ') | KeyCode::Enter => match self.result_origin {
                ResultOrigin::JustPlayed if self.bench_run.is_some() => self.continue_benchmark(),
                ResultOrigin::JustPlayed if self.config.mode.kind == ModeKind::Burst && self.burst_series.complete() => {
                    self.screen = Screen::Title;
                }
                ResultOrigin::JustPlayed => self.start_countdown(),
                ResultOrigin::History => self.open_history(),
            },
            KeyCode::Char('h') | KeyCode::Char('H') => self.open_history(),
            KeyCode::Char('q') | KeyCode::Char('Q') => self.should_quit = true,
            KeyCode::Esc => match self.result_origin {
                ResultOrigin::JustPlayed if self.bench_run.is_some() => self.abort_benchmark(),
                ResultOrigin::JustPlayed => self.should_quit = true,
                ResultOrigin::History => self.open_history(),
            },
            _ => {}
        }
        if matches!(action, InputAction::Abort) && key.code != KeyCode::Esc {
            match self.result_origin {
                ResultOrigin::JustPlayed if self.bench_run.is_some() => self.abort_benchmark(),
                ResultOrigin::JustPlayed => self.should_quit = true,
                ResultOrigin::History => self.open_history(),
            }
        }
    }

    fn handle_history_key(&mut self, key: KeyEvent, action: InputAction) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.history_selected > 0 {
                    self.history_selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.history_selected + 1 < self.history.len() {
                    self.history_selected += 1;
                }
            }
            KeyCode::PageUp => {
                self.history_selected = self.history_selected.saturating_sub(10);
            }
            KeyCode::PageDown => {
                let max = self.history.len().saturating_sub(1);
                self.history_selected = (self.history_selected + 10).min(max);
            }
            KeyCode::Home => {
                self.history_selected = 0;
            }
            KeyCode::End => {
                if !self.history.is_empty() {
                    self.history_selected = self.history.len() - 1;
                }
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                self.history_sort = self.history_sort.next();
                self.history_selected = 0;
                self.open_history();
            }
            KeyCode::Enter => self.open_history_detail(),
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                self.screen = Screen::Title;
            }
            _ => {}
        }
        if matches!(action, InputAction::Abort) {
            self.screen = Screen::Title;
        }
    }

    fn handle_history_detail_key(&mut self, key: KeyEvent, action: InputAction) {
        match key.code {
            KeyCode::Esc | KeyCode::Backspace | KeyCode::Char(' ') | KeyCode::Enter => {
                self.open_history();
            }
            KeyCode::Char('q') | KeyCode::Char('Q') => self.should_quit = true,
            _ => {}
        }
        if matches!(action, InputAction::Abort) {
            self.open_history();
        }
    }

    fn handle_playing_key(&mut self, action: InputAction, now: Instant) {
        // Resolve timer boundaries before judging an event that arrived on
        // the same event-loop iteration as the next render tick.
        let _ = self.tick_playing(Duration::ZERO, now);
        if self.screen != Screen::Playing {
            return;
        }
        match action {
            InputAction::Abort => {
                // Esc during play: abort back to title.
                if let Some(game) = self.game.as_mut() {
                    game.session.abort(now.saturating_duration_since(game.started_at));
                }
                if self.bench_run.is_some() {
                    self.abort_benchmark();
                } else {
                    self.game = None;
                    self.screen = Screen::Title;
                }
            }
            InputAction::Char(g) => {
                self.process_grapheme(&g, now);
            }
            InputAction::Backspace => self.process_backspace(now),
            InputAction::Ignored => {}
        }
    }

    fn report_current_reading(&mut self, now: Instant) {
        let Some(game) = self.game.as_ref() else { return };
        let Some(reading) = game.current_prompt.reading.clone() else { return };
        let kind = match self.config.mode.kind {
            ModeKind::Romaji => ReadingReportKind::Romaji,
            ModeKind::Sentence if self.config.mode.sentence.lang == "ja" => ReadingReportKind::Sentence,
            _ => return,
        };
        let text = game.current_prompt.text.clone();
        let db_path = self.db_path();
        let directory = db_path.parent().unwrap_or_else(|| std::path::Path::new("."));
        if let Err(error) = append_reading_report(directory, chrono::Utc::now(), kind, &text, &reading) {
            self.mode_warning = Some(error.to_string());
            return;
        }
        if let Some(game) = self.game.as_mut() {
            game.mode_generator.exclude(&text, Some(&reading));
            for prompt in &mut game.preview_prompts {
                if prompt.text == text && prompt.reading.as_deref() == Some(reading.as_str()) {
                    *prompt = game.mode_generator.next_prompt();
                }
            }
            game.next_prompt = game.preview_prompts[0].clone();
            game.next_word = word_from_text(&game.next_prompt.target);
            let elapsed = now.saturating_duration_since(game.started_at);
            game.session.skip_item(elapsed);
        }
        self.advance_to_next_word(now);
    }

    fn process_grapheme(&mut self, g: &str, now: Instant) {
        let pause_ms = self.config.analysis.pause_ms;
        let practice_layout = self.practice_layout().clone();
        let input_layout = self.input_layout().cloned();
        let character = g.chars().next();
        let code = character.and_then(|character| {
            input_layout
                .as_ref()
                .unwrap_or(&practice_layout)
                .stroke_for(character)
                .map(|stroke| stroke.code)
        });
        let remapped = character
            .map(|character| remap_char(character, input_layout.as_ref(), &practice_layout).to_string())
            .unwrap_or_else(|| g.to_owned());
        let Some(game) = self.game.as_mut() else { return };
        if game.session.log().keystrokes.is_empty() && !g.is_ascii() {
            game.ime_warning = true;
            return;
        }
        let elapsed = now.saturating_duration_since(game.started_at);
        let effect = game.session.input_char_with_code(elapsed, &remapped, code.clone());
        if effect != InputEffect::Dropped {
            let expected = game.session.log().keystrokes.last().and_then(|key| key.expected.as_deref());
            game.last_key_flash = Some(now);
            game.last_pressed_code = code;
            game.last_expected_code = expected
                .and_then(|value| value.chars().next())
                .and_then(|value| practice_layout.stroke_for(value).map(|stroke| stroke.code));
            game.last_key_correct = !matches!(effect, InputEffect::Mistype | InputEffect::ItemCompletedMistyped);
        }
        match effect {
            InputEffect::Dropped => return,
            InputEffect::Correct => {
                game.typed_index = game.session.position();
            }
            InputEffect::Mistype => {
                game.mistypes_in_word += 1;
                game.last_mistype_flash = Some(now);
                game.last_mistype_char = Some(remapped.clone());
                game.last_mistype_expected = game.session.log().keystrokes.last().and_then(|key| key.expected.clone());
            }
            InputEffect::ItemCompleted | InputEffect::ItemCompletedMistyped => {
                if effect == InputEffect::ItemCompletedMistyped {
                    game.mistypes_in_word += 1;
                    if self.active_bench_id.is_none() && self.config.session.retry_on_error && !game.current_is_retry {
                        game.retry_prompt = Some(game.current_prompt.clone());
                    }
                }
                game.typed_index = game.session.position();
                game.current_word.state = WordState::Exiting {
                    kind: if game.mistypes_in_word == 0 {
                        ExitKind::Correct
                    } else {
                        ExitKind::Mistyped
                    },
                    progress: 0.0,
                };
            }
            InputEffect::Backspace => {}
        }
        Self::refresh_live_metrics(game, pause_ms);
        let burst_finished = self.config.mode.kind == ModeKind::Burst && game.metrics.completed_words as usize >= self.config.burst.words;
        if burst_finished {
            self.finish_game(now);
        }
    }

    fn process_backspace(&mut self, now: Instant) {
        let pause_ms = self.config.analysis.pause_ms;
        let Some(game) = self.game.as_mut() else { return };
        let elapsed = now.saturating_duration_since(game.started_at);
        if game.session.input_backspace_with_code(elapsed, Some("Backspace".to_owned())) != InputEffect::Dropped {
            Self::refresh_live_metrics(game, pause_ms);
        }
    }

    fn refresh_live_metrics(game: &mut GameState, pause_ms: u64) {
        let analysis = crate::analysis::analyze_with_pause(game.session.log(), Duration::from_millis(pause_ms));
        game.combo = analysis.current_combo;
        game.metrics = analysis.live;
    }

    fn start_countdown(&mut self) {
        self.screen = Screen::Countdown { remaining: 3.0 };
        self.last_save_id = None;
        self.last_save_error = None;
    }

    fn open_history(&mut self) {
        let loaded = if self.history_sort.is_ranking() {
            db::ranked_sessions(&self.db_path())
        } else {
            db::list(&self.db_path())
        };
        match loaded {
            Ok(mut items) => {
                sort_history(&mut items, self.history_sort);
                if self.history_selected >= items.len() {
                    self.history_selected = items.len().saturating_sub(1);
                }
                self.history = items;
                self.history_load_error = None;
            }
            Err(e) => {
                self.history.clear();
                self.history_selected = 0;
                self.history_load_error = Some(e.to_string());
            }
        }
        self.screen = Screen::History;
    }

    fn open_history_detail(&mut self) {
        let Some(entry) = self.history.get(self.history_selected) else {
            return;
        };
        let path = self.db_path();
        match db::load_result(&path, entry.id, &self.config) {
            Ok(Some(result)) => {
                self.last_result = Some(result);
                self.legacy_wpm = db::legacy_wpm(&path, entry.id).ok().flatten();
                self.result_origin = ResultOrigin::History;
                self.last_save_id = Some(entry.id);
                self.last_save_error = None;
                self.screen = Screen::HistoryDetail;
            }
            Ok(None) => {
                self.legacy_wpm = None;
                self.history_load_error = Some(format!("{}: #{}", Msg::SessionNotFound.text(self.config.general.lang), entry.id));
            }
            Err(e) => {
                self.legacy_wpm = None;
                self.history_load_error = Some(e.to_string());
            }
        }
    }

    fn start_game(&mut self, now: Instant, date: chrono::NaiveDate) {
        let is_bench = self.bench_run.is_some();
        let mut catalog = if is_bench {
            DataCatalog::load(&crate::config::DataConfig::default(), &self.home_dir)
        } else {
            DataCatalog::load(&self.config.data, &self.home_dir).or_else(|_| DataCatalog::load(&crate::config::DataConfig::default(), &self.home_dir))
        }
        .expect("bundled prompt data must be valid");
        let db_path = self.db_path();
        let records = db::load_records_filtered(&db_path, None, None).unwrap_or_default();
        let practice_layout = self.practice_layout().clone();
        let physical = crate::layout::geometry::geometry(&self.config.keyboard.geometry)
            .or_else(|| crate::layout::geometry::geometry("ansi"))
            .expect("bundled keyboard geometry");
        let mut generator_config = self.config.mode.clone();
        self.new_unlocked_key = None;
        self.adaptive_focus.clear();
        self.adaptive_result.clear();
        self.mode_warning = None;
        self.rhythm_deviation_ms = None;
        self.endurance_comparison = None;
        self.recovery_metrics = RecoveryMetrics::default();
        self.keyboard_fade = if !is_bench && self.config.keyboard.show == "auto" {
            let details = analyze_records(
                &records,
                &practice_layout,
                &physical,
                AnalysisSettings {
                    pause_ms: self.config.analysis.pause_ms,
                    shrinkage_k: self.config.analysis.shrinkage_k,
                },
            );
            crate::practice::keyboard_fade::key_fade_states(&details, self.config.analysis.shrinkage_k)
        } else {
            HashMap::new()
        };
        self.rhythm_bpm = (!is_bench && self.config.mode.kind == ModeKind::Rhythm).then(|| {
            self.config.rhythm.bpm.unwrap_or_else(|| {
                let mut values = records
                    .iter()
                    .filter(|record| record.header.metrics_version > 0 && record.header.iki_median_ms > 0.0)
                    .map(|record| record.header.iki_median_ms)
                    .collect::<Vec<_>>();
                values.sort_by(f64::total_cmp);
                crate::practice::rhythm::default_bpm(values.get(values.len() / 2).copied().unwrap_or(0.0))
            })
        });
        if !is_bench && self.config.mode.kind == ModeKind::Unlock {
            let state = db::load_unlock_state(&db_path, &self.config.keyboard.layout)
                .ok()
                .flatten()
                .unwrap_or_else(|| UnlockState::new(initial_keys(&practice_layout)));
            generator_config = mode_config(generator_config, &state.unlocked);
            self.unlock_state = Some(state);
        } else {
            self.unlock_state = None;
        }
        let pacer_supported = matches!(self.config.mode.kind, ModeKind::Words | ModeKind::Sentence | ModeKind::Adaptive);
        self.config.pacer.target_wpm = (!is_bench && pacer_supported && self.config.pacer.enabled)
            .then(|| crate::practice::pacer::target_wpm(&records, self.config.pacer.ratio, self.config.pacer.min_wpm));
        if !is_bench
            && let Some(directory) = db_path.parent()
            && let Ok(exclusions) = load_reading_exclusions(directory)
        {
            let excluded = exclusions.into_iter().map(|(_, text, reading)| (text, reading)).collect::<HashSet<_>>();
            catalog.exclude_readings(&excluded);
        }
        if !is_bench && let Some(kind) = daily_data_kind(&self.config) {
            let kind_name = daily_kind_name(kind);
            let salt = db::practice_salt(&db_path).unwrap_or_else(|_| b"fallback-practice-salt".to_vec());
            let exposures = db::load_item_exposures(&db_path, kind_name).unwrap_or_default();
            let mistakes = db::load_item_mistakes(&db_path, kind_name).unwrap_or_default();
            let candidates = catalog.fixed_items(kind).into_iter().cloned().collect::<Vec<_>>();
            let uses_key_filter = matches!(self.config.mode.kind, ModeKind::Words | ModeKind::Romaji | ModeKind::Unlock);
            let key_selection =
                (uses_key_filter && !generator_config.keys.is_empty() && !matches!(kind, DataKind::EnglishSentence | DataKind::JapaneseSentence))
                    .then(|| KeySelection {
                        filter: generator_config.key_filter,
                        selected: generator_config.keys.clone(),
                        allowed: allowed_practice_characters(&generator_config, &practice_layout),
                    });
            let options = DeckOptions {
                date,
                user_salt: salt,
                size: if matches!(kind, DataKind::EnglishSentence | DataKind::JapaneseSentence) {
                    512
                } else {
                    2_048
                },
                core_ratio: self.config.practice.core_ratio,
                weekly_theme: self.config.practice.weekly_theme,
                daily_rotation: self.config.practice.daily_rotation,
                key_selection,
            };
            if let Some(deck) = build_daily_deck(&candidates, &options, &exposures, &mistakes) {
                self.practice_theme = deck.theme;
                let keys = deck.items.into_iter().map(|item| item.key).collect::<HashSet<_>>();
                catalog.retain_kind_keys(kind, &keys);
            }
        }
        let seed = self.rng.r#gen::<u64>();
        let adaptive_plan = if !is_bench && self.config.mode.kind == ModeKind::Adaptive {
            let japanese = self.config.adaptive.lang == "ja";
            let details = analyze_records(
                &records,
                &practice_layout,
                &physical,
                AnalysisSettings {
                    pause_ms: self.config.analysis.pause_ms,
                    shrinkage_k: self.config.analysis.shrinkage_k,
                },
            );
            let previous = db::load_adaptive_focus(&db_path, date.pred_opt().unwrap_or(date), &self.config.keyboard.layout).unwrap_or_default();
            let corpus = if japanese {
                catalog
                    .words_ja()
                    .iter()
                    .map(|item| crate::engine::romaji::RomajiJudge::new(&item.reading).guide().to_owned())
                    .collect::<Vec<_>>()
            } else {
                catalog.words_en().to_vec()
            };
            let plan = build_plan(
                &details,
                &corpus,
                AdaptiveOptions {
                    error_weight_ms: self.config.adaptive.error_weight,
                    drill_ratio: self.config.adaptive.drill_ratio,
                    min_transition_types: 50,
                    max_share: self.config.adaptive.max_share,
                },
                &previous,
            );
            self.adaptive_focus = plan.transitions.iter().take(5).cloned().collect();
            let focus = plan.focus();
            let _ = db::save_adaptive_focus(&db_path, date, &self.config.keyboard.layout, &focus);
            if plan.fallback_to_words {
                self.mode_warning = Some(
                    plan.warning(match self.config.general.lang {
                        Lang::En => "en",
                        Lang::Ja => "ja",
                    })
                    .to_owned(),
                );
            }
            Some(plan)
        } else {
            None
        };
        let bench_prompts = self.bench_run.as_ref().map(|run| {
            generate_version(run.part, run.parallel_index, &catalog, &practice_layout, &physical)
                .prompts
                .into_iter()
                .map(|prompt| GeneratedPrompt::fixed(prompt.text, prompt.reading, prompt.target))
                .collect::<Vec<_>>()
        });
        let mut mode_generator = if let Some(plan) = adaptive_plan.as_ref() {
            ModeGenerator::new_adaptive(
                self.config.mode.clone(),
                catalog,
                practice_layout,
                seed,
                plan,
                self.config.adaptive.lang == "ja",
            )
        } else {
            ModeGenerator::new(generator_config, catalog, practice_layout, seed)
        };
        if self.mode_warning.is_none() {
            self.mode_warning = mode_generator.warning().map(str::to_owned);
        }
        let preview_count = self.config.session.preview_items.max(1);
        let (first_prompt, preview_prompts, bench_cursor) = if let Some(prompts) = bench_prompts.as_ref() {
            let first = prompts[0].clone();
            let preview = (0..preview_count)
                .map(|offset| prompts[(offset + 1) % prompts.len()].clone())
                .collect::<Vec<_>>();
            (first, preview, preview_count + 1)
        } else {
            let first = mode_generator.next_prompt();
            let preview = (0..preview_count).map(|_| mode_generator.next_prompt()).collect::<Vec<_>>();
            (first, preview, 0)
        };
        let next_prompt = preview_prompts[0].clone();
        let ghost = if let Some(bench_id) = self.active_bench_id.as_ref() {
            select_ghost(&records, &GhostCondition::Bench { bench_id: bench_id.clone() })
        } else if self.config.mode.kind == ModeKind::Sentence {
            select_ghost(
                &records,
                &GhostCondition::Sentence {
                    text: first_prompt.text.clone(),
                },
            )
        } else {
            None
        };
        let first = word_from_text(&first_prompt.target);
        let next = word_from_text(&next_prompt.target);
        let item_timeout = if matches!(self.config.mode.kind, ModeKind::Sentence | ModeKind::Endurance) {
            Duration::ZERO
        } else {
            self.config.session.item_timeout()
        };
        let effective_error_mode = if !is_bench && self.config.session.blind {
            crate::config::ErrorMode::Advance
        } else {
            self.config.session.error_mode
        };
        let session_duration = if self.config.mode.kind == ModeKind::Endurance {
            Duration::from_secs(self.config.endurance.minutes * 60)
        } else {
            self.config.session.duration()
        };
        let session = Session::with_error_mode(session_duration, item_timeout, first_prompt.clone().into_item(), effective_error_mode);
        self.game = Some(GameState {
            session,
            current_word: WordSlot::new_entering(first),
            next_word: next,
            current_prompt: first_prompt,
            next_prompt,
            mode_generator,
            preview_prompts,
            typed_index: 0,
            mistypes_in_word: 0,
            started_at: now,
            started_wall: chrono::Utc::now(),
            word_started_at: now,
            metrics: LiveMetrics::default(),
            combo: 0,
            last_mistype_flash: None,
            last_mistype_char: None,
            last_mistype_expected: None,
            last_key_flash: None,
            last_pressed_code: None,
            last_expected_code: None,
            last_key_correct: false,
            ime_warning: false,
            bench_prompts,
            bench_cursor,
            ghost,
            elapsed: Duration::ZERO,
            retry_prompt: None,
            current_is_retry: false,
            last_rhythm_beat: 0,
        });
        self.screen = Screen::Playing;
    }

    /// Called once per render tick. Drives countdown / animation / timeout.
    pub fn on_tick(&mut self, dt: Duration) -> AppResult<()> {
        self.on_tick_at_date(dt, Instant::now(), chrono::Local::now().date_naive())
    }

    pub fn on_tick_at(&mut self, dt: Duration, now: Instant) -> AppResult<()> {
        self.on_tick_at_date(dt, now, chrono::Local::now().date_naive())
    }

    pub fn on_tick_at_date(&mut self, dt: Duration, now: Instant, date: chrono::NaiveDate) -> AppResult<()> {
        self.elapsed_total += dt;
        match self.screen {
            Screen::Countdown { remaining } => {
                let new_remaining = remaining - dt.as_secs_f32();
                if new_remaining <= 0.0 {
                    self.start_game(now, date);
                } else {
                    self.screen = Screen::Countdown { remaining: new_remaining };
                }
            }
            Screen::Playing => self.tick_playing(dt, now)?,
            _ => {}
        }
        Ok(())
    }

    fn tick_playing(&mut self, dt: Duration, now: Instant) -> AppResult<()> {
        let pause_ms = self.config.analysis.pause_ms;
        let tick = {
            let game = self.game.as_mut().expect("playing without game");
            let elapsed = now.saturating_duration_since(game.started_at);
            game.elapsed = elapsed;
            game.session.tick(elapsed)
        };
        if self.config.mode.kind == ModeKind::Rhythm
            && let (Some(game), Some(bpm)) = (self.game.as_mut(), self.rhythm_bpm)
        {
            let beat = crate::practice::rhythm::beat_index(bpm, game.elapsed);
            if beat > game.last_rhythm_beat {
                if self.config.rhythm.bell {
                    eprint!("\x07");
                }
                game.last_rhythm_beat = beat;
            }
        }
        if tick.session_finished {
            self.finish_game(now);
            return Ok(());
        }
        if tick.item_timed_out
            && let Some(g) = self.game.as_mut()
        {
            g.current_word.state = WordState::Exiting {
                kind: ExitKind::Timeout,
                progress: 0.0,
            };
            Self::refresh_live_metrics(g, pause_ms);
        }

        // Advance entering / exiting display states. The engine remains the
        // authority for whether input is accepted during these phases.
        let mut promote_to_next: Option<()> = None;
        if let Some(g) = self.game.as_mut() {
            let speed = self.config.ui.animation_speed.max(0.1);
            match &mut g.current_word.state {
                WordState::Entering { progress } => {
                    if !self.config.ui.animation_enabled {
                        *progress = 1.0;
                    } else {
                        *progress += dt.as_secs_f32() * speed / WORD_ENTER_DURATION.as_secs_f32();
                    }
                    if *progress >= 1.0 {
                        g.current_word.state = WordState::Active;
                        g.word_started_at = now;
                        let elapsed = now.saturating_duration_since(g.started_at);
                        g.session.activate(elapsed);
                    }
                }
                WordState::Exiting { progress, .. } => {
                    if !self.config.ui.animation_enabled {
                        *progress = 1.0;
                    } else {
                        *progress += dt.as_secs_f32() * speed / WORD_EXIT_DURATION.as_secs_f32();
                    }
                    if *progress >= 1.0 {
                        promote_to_next = Some(());
                    }
                }
                WordState::Active => {}
            }
        }

        if promote_to_next.is_some() {
            self.advance_to_next_word(now);
        }
        Ok(())
    }

    fn advance_to_next_word(&mut self, now: Instant) {
        if let Some(g) = self.game.as_mut() {
            if let Some(retry_prompt) = g.retry_prompt.take() {
                let elapsed = now.saturating_duration_since(g.started_at);
                g.session.advance(retry_prompt.clone().into_item(), elapsed);
                g.current_word = WordSlot::new_entering(word_from_text(&retry_prompt.target));
                g.current_prompt = retry_prompt;
                g.current_is_retry = true;
                reset_item_display_state(g);
                return;
            }
            let new_next_prompt = if let Some(prompts) = g.bench_prompts.as_ref() {
                let prompt = prompts[g.bench_cursor % prompts.len()].clone();
                g.bench_cursor += 1;
                prompt
            } else {
                g.mode_generator.next_prompt()
            };
            let promoted_prompt = g.preview_prompts.remove(0);
            g.preview_prompts.push(new_next_prompt);
            let next_prompt = g.preview_prompts[0].clone();
            let new_next = word_from_text(&next_prompt.target);
            let promoted = std::mem::replace(&mut g.next_word, new_next);
            g.next_prompt = next_prompt;
            let elapsed = now.saturating_duration_since(g.started_at);
            g.session.advance(promoted_prompt.clone().into_item(), elapsed);
            g.current_word = WordSlot::new_entering(promoted);
            g.current_prompt = promoted_prompt;
            g.current_is_retry = false;
            reset_item_display_state(g);
        }
    }

    fn finish_game(&mut self, _now: Instant) {
        let Some(game) = self.game.take() else { return };
        let started_wall = game.started_wall;
        let log = game.session.into_log();
        let db_path = self.db_path();
        if self.active_bench_id.is_none()
            && let Some(kind) = daily_data_kind(&self.config)
        {
            let shown_date = (started_wall + chrono::Duration::microseconds(log.duration_us.min(i64::MAX as u64) as i64))
                .with_timezone(&chrono::Local)
                .date_naive();
            let keys = log
                .items
                .iter()
                .filter(|item| item.outcome != crate::engine::session::ItemOutcome::Aborted)
                .map(|item| data_item_key(kind, &item.text, item.reading.as_deref()))
                .collect::<Vec<_>>();
            let key_refs = keys.iter().map(String::as_str).collect::<Vec<_>>();
            let _ = db::record_item_exposures(&db_path, daily_kind_name(kind), &key_refs, shown_date);
            let mistyped = log
                .keystrokes
                .iter()
                .filter(|key| !key.correct)
                .map(|key| key.item_idx)
                .collect::<HashSet<_>>();
            for item in log.items.iter().filter(|item| mistyped.contains(&item.idx)) {
                let key = data_item_key(kind, &item.text, item.reading.as_deref());
                let _ = db::record_item_mistake(&db_path, daily_kind_name(kind), &key, shown_date);
            }
        }
        let mut result = GameResult::from_log(&self.config, &log);
        let mut record = SessionRecord::from_log(&self.config, log, started_wall);
        self.recovery_metrics = crate::practice::recovery::recovery_metrics(&record.log);
        if self.config.mode.kind == ModeKind::Rhythm
            && let Some(bpm) = self.rhythm_bpm
        {
            let times = record
                .log
                .keystrokes
                .iter()
                .filter(|key| key.kind == crate::engine::session::KeystrokeKind::Char)
                .map(|key| Duration::from_micros(key.t_us))
                .collect::<Vec<_>>();
            self.rhythm_deviation_ms = Some(crate::practice::rhythm::median_beat_deviation_ms(bpm, &times));
        }
        if self.config.mode.kind == ModeKind::Endurance {
            self.endurance_comparison = Some(crate::practice::session_variants::compare_endurance_log(&record.log));
        }
        if self.config.mode.kind == ModeKind::Burst {
            self.burst_series.push(record.header.net_wpm);
        }
        record.header.release_events = self.release_events_supported;
        self.gate_failed = !crate::practice::gate::is_eligible(self.config.gate.min_accuracy, record.header.accuracy);
        self.next_pacer_target_wpm =
            self.config.pacer.target_wpm.map(|target| {
                crate::practice::pacer::next_target_wpm(target, record.header.net_wpm, record.header.accuracy, self.config.pacer.min_wpm)
            });
        if self.active_bench_id.is_none() && self.config.mode.kind == ModeKind::Adaptive {
            let geometry = crate::layout::geometry::geometry(&self.config.keyboard.geometry)
                .or_else(|| crate::layout::geometry::geometry("ansi"))
                .expect("bundled keyboard geometry");
            let details = analyze_records(
                &[record.clone()],
                self.practice_layout(),
                &geometry,
                AnalysisSettings {
                    pause_ms: self.config.analysis.pause_ms,
                    shrinkage_k: self.config.analysis.shrinkage_k,
                },
            );
            self.adaptive_result = self
                .adaptive_focus
                .iter()
                .map(|prior| AdaptiveResultRow {
                    from: prior.from.clone(),
                    to: prior.to.clone(),
                    session_iki_ms: details
                        .bigrams
                        .iter()
                        .find(|row| row.from == prior.from && row.to == prior.to)
                        .map(|row| row.median_ms),
                    prior_estimated_iki_ms: prior.estimated_iki_ms,
                })
                .collect();
        }
        if self.active_bench_id.is_none()
            && self.config.mode.kind == ModeKind::Unlock
            && let Some(mut state) = self.unlock_state.take()
        {
            let geometry = crate::layout::geometry::geometry(&self.config.keyboard.geometry)
                .or_else(|| crate::layout::geometry::geometry("ansi"))
                .expect("bundled keyboard geometry");
            let sequence = unlock_sequence(self.practice_layout(), &geometry, self.config.unlock.order);
            self.new_unlocked_key = apply_result(
                &mut state,
                record.header.accuracy,
                record.header.iki_median_ms,
                self.config.unlock.target_wpm,
                &sequence,
            );
            let _ = db::save_unlock_state(&db_path, &self.config.keyboard.layout, &state);
            self.unlock_state = Some(state);
        }
        if let Some(bench_id) = self.active_bench_id.clone() {
            record.header.mode = "bench".to_owned();
            self.bench_comparison = db::bench_comparison(&db_path, &bench_id, record.header.net_wpm, record.header.accuracy).ok();
            record.header.bench_id = Some(bench_id);
            record.header.error_mode = "stop".to_owned();
            if let (Some(part), Some(comparison)) = (self.active_bench_part(), self.bench_comparison) {
                self.bench_results.push(BenchResultSummary {
                    id: record.header.bench_id.clone().unwrap_or_default(),
                    part,
                    wpm: record.header.net_wpm,
                    accuracy: record.header.accuracy,
                    comparison,
                });
            }
        } else {
            self.bench_comparison = None;
        }
        if let Ok(finished) = chrono::DateTime::parse_from_rfc3339(&record.header.finished_at) {
            result.finished_at = finished.with_timezone(&chrono::Local);
        }

        match db::save(&db_path, &record) {
            Ok(id) => {
                self.last_save_id = Some(id);
                self.last_save_error = None;
            }
            Err(e) => {
                self.last_save_id = None;
                self.last_save_error = Some(e.to_string());
                eprintln!(
                    "{}: {}: {e}",
                    Msg::AppTitle.text(self.config.general.lang),
                    Msg::FailedToSaveSession.text(self.config.general.lang)
                );
            }
        }
        self.refresh_recommendations();
        self.last_result = Some(result);
        self.legacy_wpm = None;
        self.result_origin = ResultOrigin::JustPlayed;
        self.screen = Screen::Result;
    }
}

const MODES: [ModeKind; 10] = [
    ModeKind::Random,
    ModeKind::Words,
    ModeKind::Romaji,
    ModeKind::Sentence,
    ModeKind::Ngram,
    ModeKind::Adaptive,
    ModeKind::Unlock,
    ModeKind::Rhythm,
    ModeKind::Burst,
    ModeKind::Endurance,
];

fn daily_data_kind(config: &Config) -> Option<DataKind> {
    match config.mode.kind {
        ModeKind::Adaptive if config.adaptive.lang == "ja" => Some(DataKind::JapaneseWord),
        ModeKind::Words | ModeKind::Adaptive | ModeKind::Unlock | ModeKind::Rhythm | ModeKind::Burst => Some(DataKind::EnglishWord),
        ModeKind::Romaji => Some(DataKind::JapaneseWord),
        ModeKind::Sentence | ModeKind::Endurance if config.mode.sentence.lang == "ja" => Some(DataKind::JapaneseSentence),
        ModeKind::Sentence | ModeKind::Endurance => Some(DataKind::EnglishSentence),
        ModeKind::Ngram if config.mode.ngram.source == "corpus" => Some(DataKind::EnglishWord),
        ModeKind::Random | ModeKind::Ngram => None,
    }
}

fn daily_kind_name(kind: DataKind) -> &'static str {
    match kind {
        DataKind::EnglishWord => "word-en",
        DataKind::JapaneseWord => "word-ja",
        DataKind::EnglishSentence => "sentence-en",
        DataKind::JapaneseSentence => "sentence-ja",
    }
}

fn data_item_key(kind: DataKind, text: &str, reading: Option<&str>) -> String {
    match kind {
        DataKind::EnglishWord => format!("en-word:{text}"),
        DataKind::EnglishSentence => format!("en-sentence:{text}"),
        DataKind::JapaneseWord => format!("ja-word:{text}\t{}", reading.unwrap_or_default()),
        DataKind::JapaneseSentence => format!("ja-sentence:{text}\t{}", reading.unwrap_or_default()),
    }
}

fn allowed_practice_characters(mode: &ModeConfig, layout: &KeyboardLayout) -> String {
    let mut characters = mode.keys.clone();
    if mode.base_keys == "home" {
        for code in [
            "KeyA",
            "KeyS",
            "KeyD",
            "KeyF",
            "KeyG",
            "KeyH",
            "KeyJ",
            "KeyK",
            "KeyL",
            "Semicolon",
            "Quote",
        ] {
            if let Some(pair) = layout.key(code) {
                characters.push(pair.normal);
            }
        }
    } else if mode.base_keys != "none" {
        characters.push_str(&mode.base_keys);
    }
    characters
}

fn mode_index(mode: ModeKind) -> usize {
    MODES.iter().position(|candidate| *candidate == mode).unwrap_or(0)
}

fn replay_mode_kind(name: &str) -> ModeKind {
    match name {
        "words" => ModeKind::Words,
        "romaji" => ModeKind::Romaji,
        "sentence" => ModeKind::Sentence,
        "ngram" => ModeKind::Ngram,
        "adaptive" => ModeKind::Adaptive,
        "unlock" => ModeKind::Unlock,
        "rhythm" => ModeKind::Rhythm,
        "burst" => ModeKind::Burst,
        "endurance" => ModeKind::Endurance,
        _ => ModeKind::Random,
    }
}

fn replay_prompt(item: &ItemLog) -> GeneratedPrompt {
    let target = item
        .reading
        .as_deref()
        .map(|reading| RomajiJudge::new(reading).guide().to_owned())
        .unwrap_or_else(|| item.text.clone());
    GeneratedPrompt::fixed(item.text.clone(), item.reading.clone(), target)
}

fn word_from_text(text: &str) -> Word {
    Word::from_graphemes(&text.graphemes(true).map(str::to_owned).collect::<Vec<_>>())
}

fn reset_item_display_state(game: &mut GameState) {
    game.typed_index = 0;
    game.mistypes_in_word = 0;
    game.last_mistype_flash = None;
    game.last_mistype_char = None;
    game.last_mistype_expected = None;
    game.last_key_flash = None;
    game.last_pressed_code = None;
    game.last_expected_code = None;
    game.ime_warning = false;
}

fn sort_history(items: &mut [SessionSummary], sort: HistorySort) {
    use std::cmp::Ordering;
    match sort {
        HistorySort::DateDesc => items.sort_by(|a, b| b.finished_at.cmp(&a.finished_at).then(b.id.cmp(&a.id))),
        HistorySort::DateAsc => items.sort_by(|a, b| a.finished_at.cmp(&b.finished_at).then(a.id.cmp(&b.id))),
        HistorySort::WpmDesc => items.sort_by(|a, b| {
            b.wpm
                .partial_cmp(&a.wpm)
                .unwrap_or(Ordering::Equal)
                .then(b.finished_at.cmp(&a.finished_at))
        }),
        HistorySort::AccuracyDesc => items.sort_by(|a, b| {
            b.accuracy
                .partial_cmp(&a.accuracy)
                .unwrap_or(Ordering::Equal)
                .then(b.finished_at.cmp(&a.finished_at))
        }),
        HistorySort::MaxComboDesc => items.sort_by(|a, b| b.max_combo.cmp(&a.max_combo).then(b.finished_at.cmp(&a.finished_at))),
        HistorySort::WordsDesc => items.sort_by(|a, b| b.completed_words.cmp(&a.completed_words).then(b.finished_at.cmp(&a.finished_at))),
    }
}

fn cycle_string(current: &mut String, options: &[String], direction: isize) {
    if options.is_empty() {
        return;
    }
    let current_index = options.iter().position(|value| value == current).unwrap_or(0) as isize;
    let next = (current_index + direction).rem_euclid(options.len() as isize) as usize;
    current.clone_from(&options[next]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration as ChronoDuration, Local};

    fn summary(id: i64, secs_offset: i64, wpm: f64, acc: f64, combo: u32, words: u32) -> SessionSummary {
        SessionSummary {
            id,
            finished_at: Local::now() + ChronoDuration::seconds(secs_offset),
            duration: Duration::from_secs(60),
            wpm,
            accuracy: acc,
            max_combo: combo,
            completed_words: words,
            metrics_version: 1,
        }
    }

    #[test]
    fn sort_date_desc_newest_first() {
        let mut v = vec![summary(1, 0, 50.0, 0.9, 5, 30), summary(2, 100, 40.0, 0.8, 3, 20)];
        sort_history(&mut v, HistorySort::DateDesc);
        assert_eq!(v[0].id, 2);
    }

    #[test]
    fn sort_wpm_desc_highest_first() {
        let mut v = vec![summary(1, 0, 50.0, 0.9, 5, 30), summary(2, 100, 80.0, 0.7, 3, 20)];
        sort_history(&mut v, HistorySort::WpmDesc);
        assert_eq!(v[0].id, 2);
        assert_eq!(v[1].id, 1);
    }

    #[test]
    fn sort_accuracy_desc_highest_first() {
        let mut v = vec![summary(1, 0, 50.0, 0.7, 5, 30), summary(2, 100, 80.0, 0.95, 3, 20)];
        sort_history(&mut v, HistorySort::AccuracyDesc);
        assert_eq!(v[0].id, 2);
    }

    #[test]
    fn sort_cycle_visits_each_mode_once() {
        let mut s = HistorySort::DateDesc;
        let mut seen = vec![s];
        for _ in 0..5 {
            s = s.next();
            seen.push(s);
        }
        let last = s.next();
        assert_eq!(last, HistorySort::DateDesc);
        assert_eq!(seen.len(), 6);
    }
}
