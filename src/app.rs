//! Application state machine. Owns the current screen, the optional
//! in-progress game, and the last completed result.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use rand::rngs::ThreadRng;
use rand::{Rng, thread_rng};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

use crate::analysis::metrics::{AnalysisSettings, DetailedMetrics, analyze_records};
use crate::analysis::prescription::{
    ControlCandidate, DifficultyState, Evidence, Posterior, PrescriptionBaseline, PrescriptionTarget, adjust_difficulty,
    evaluate_stored_prescription, prescription_trends, real_word_records_since, record_prescription_session, select_controls,
};
use crate::bench::{
    BenchPart, BenchResultSummary, BenchResume, BenchSettings, generate_parallel_versions_for_series, is_preferred_bench_series, next_parallel_index,
    parse_bench_id, preferred_bench_series,
};
use crate::config::{Config, KeyFilter, KeyboardConfig, ModeConfig, ModeKind, SessionConfig};
use crate::data::{DataCatalog, DataKind, TrainingDataSource};
use crate::engine::modes::{GeneratedPrompt, ModeGenerator};
use crate::engine::romaji::RomajiJudge;
use crate::engine::session::{InputEffect, ItemLog, KeystrokeKind, KeystrokeLog, Session, SessionPhase};
use crate::error::AppResult;
use crate::game::input::{InputAction, classify};
use crate::game::word::{ExitKind, Word, WordSlot, WordState};
use crate::graphics::{GraphicsDetection, GraphicsPreference, TerminalPixelSize, update_cell_pixels};
use crate::i18n::{Lang, Msg};
use crate::layout::{KeyboardLayout, builtin_layout, builtin_layout_names, remap_char};
use crate::metrics::stats::{GameResult, LiveMetrics};
use crate::practice::adaptive::{AdaptiveHistory, AdaptiveOptions, TransitionCost, build_plan_seeded};
use crate::practice::deck::{DeckOptions, KeySelection, WeeklyTheme, build_daily_deck, weekly_theme};
use crate::practice::ghost::{GhostCondition, GhostTrack, select_ghost};
use crate::practice::reading_report::{ReadingReportKind, append_reading_report, load_reading_exclusions};
use crate::practice::recovery::RecoveryMetrics;
use crate::practice::review::update_review;
use crate::practice::session_variants::{BurstSeries, EnduranceComparison};
use crate::practice::unlock::{UnlockState, apply_result, initial_keys, mode_config, unlock_sequence};
use crate::record::SessionRecord;
use crate::record::db::{self, SessionSummary};
use crate::ui::analysis_image::AnalysisImageCache;
use crate::ui::bigtext_image::BigTextImageState;
use crate::ui::keyboard_image::KeyboardImageCache;
use crate::ui::result_image::ResultImageCache;
use crate::ui::title_image::TitleImageCache;
use unicode_segmentation::UnicodeSegmentation;

/// Slide-in / slide-out duration for word transitions. During these phases
/// any key input is dropped (R-06).
pub const WORD_ENTER_DURATION: Duration = Duration::from_millis(200);
pub const WORD_EXIT_DURATION: Duration = Duration::from_millis(200);
pub const COUNTDOWN_DURATION: Duration = Duration::from_millis(3000);
const ABORT_CONFIRMATION_DURATION: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Screen {
    Title,
    Menu,
    MenuFocus { remaining: f32 },
    ModeSelect,
    KeyPicker,
    Countdown { remaining: f32 },
    Playing,
    Result,
    History,
    HistoryDetail,
    Replay,
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResultComparison {
    pub previous_wpm_delta: Option<f64>,
    pub previous_cps_delta: Option<f64>,
    pub previous_accuracy_delta: Option<f64>,
    pub best_wpm: Option<f64>,
    pub best_cps: Option<f64>,
    pub best_accuracy: Option<f64>,
    pub best_wpm_delta: Option<f64>,
    pub best_cps_delta: Option<f64>,
    pub best_accuracy_delta: Option<f64>,
    pub recent_wpm: Vec<f64>,
}

/// Sort order for the history screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HistorySort {
    DateDesc,
    DateAsc,
    WpmDesc,
    AccuracyDesc,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HistoryKindFilter {
    #[default]
    All,
    Practice,
    Bench,
}

impl HistoryKindFilter {
    fn next(self) -> Self {
        match self {
            Self::All => Self::Practice,
            Self::Practice => Self::Bench,
            Self::Bench => Self::All,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HistoryOrigin {
    #[default]
    Title,
    Result,
}

#[derive(Clone, Debug)]
pub enum HistoryEntry {
    Session(SessionSummary),
    BenchRun {
        id: i64,
        series: String,
        form: String,
        parts: Vec<SessionSummary>,
    },
    BenchPart {
        run_id: i64,
        series: String,
        form: String,
        summary: SessionSummary,
    },
}

#[derive(Clone, Debug)]
pub struct HistoryDetailData {
    pub summary: SessionSummary,
    pub record: SessionRecord,
    pub personal_best_wpm: Option<f64>,
    pub previous_wpm_delta: Option<f64>,
    pub export_notice: Option<String>,
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
        }
    }

    pub fn next(self) -> Self {
        match self {
            HistorySort::DateDesc => HistorySort::DateAsc,
            HistorySort::DateAsc => HistorySort::WpmDesc,
            HistorySort::WpmDesc => HistorySort::AccuracyDesc,
            HistorySort::AccuracyDesc => HistorySort::DateDesc,
        }
    }

    /// True if the sort is by an aggregate score (vs. by date), so the UI
    /// should prefix entries with their rank number.
    pub fn is_ranking(self) -> bool {
        self == HistorySort::WpmDesc
    }
}

#[derive(Clone)]
struct HistoryReturnState {
    result: Option<GameResult>,
    result_record: Option<SessionRecord>,
    result_comparison: ResultComparison,
    legacy_wpm: Option<f64>,
    last_save_id: Option<i64>,
    last_save_error: Option<String>,
}

pub struct App {
    pub screen: Screen,
    pub config: Config,
    /// Image protocol selected at startup.
    pub graphics: GraphicsDetection,
    pub(crate) keyboard_images: RefCell<KeyboardImageCache>,
    pub(crate) bigtext_images: RefCell<BigTextImageState>,
    pub(crate) analysis_images: RefCell<AnalysisImageCache>,
    pub(crate) result_images: RefCell<ResultImageCache>,
    pub(crate) title_images: RefCell<TitleImageCache>,
    pub game: Option<GameState>,
    pub last_result: Option<GameResult>,
    pub result_record: Option<SessionRecord>,
    pub result_comparison: ResultComparison,
    pub legacy_wpm: Option<f64>,
    pub result_origin: ResultOrigin,
    pub last_save_id: Option<i64>,
    pub last_save_error: Option<String>,
    pub result_insights: Vec<String>,
    pub result_grade_line: Option<String>,
    pub mode_grade_line: Option<String>,
    pub history: Vec<HistoryEntry>,
    history_sessions: Vec<SessionSummary>,
    pub history_ranks: Vec<Option<usize>>,
    pub history_selected: usize,
    pub history_sort: HistorySort,
    pub history_kind_filter: HistoryKindFilter,
    pub history_mode_filter: Option<String>,
    pub history_language_filter: Option<String>,
    pub history_period: AnalysisPeriod,
    pub history_expanded: HashSet<i64>,
    pub history_origin: HistoryOrigin,
    pub history_load_error: Option<String>,
    pub history_detail: Option<HistoryDetailData>,
    pub history_replay: Option<Box<crate::replay::Replay>>,
    history_replay_started: Option<Instant>,
    pub history_export_dir: Option<PathBuf>,
    history_return_state: Option<HistoryReturnState>,
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
    pub bench_summary_records: Vec<SessionRecord>,
    pub bench_summary_current: Vec<SessionRecord>,
    pub bench_summary_analysis: crate::analysis::bench_summary::BenchmarkRunAnalysis,
    pub bench_summary_grades: crate::analysis::grades::GradesAnalysis,
    pub bench_resume: Option<BenchResume>,
    pub analysis_tab: usize,
    pub analysis_period: AnalysisPeriod,
    pub analysis_mode_filter: Option<String>,
    pub analysis_records: Vec<SessionRecord>,
    pub analysis_details: DetailedMetrics,
    pub analysis_summary: crate::analysis::summary::SummaryReport,
    pub analysis_coach: Option<crate::analysis::coach::CoachReport>,
    pub analysis_summary_scroll: u16,
    pub analysis_transition_selected: usize,
    pub analysis_keyboard_view: usize,
    pub analysis_confusion_selected: usize,
    pub analysis_goal_picker: bool,
    pub analysis_goal_selected: usize,
    pub adaptive_focus: Vec<TransitionCost>,
    adaptive_candidates: Vec<TransitionCost>,
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
    pub daily_menu: Option<crate::practice::menu::DailyMenu>,
    pub menu_error: Option<String>,
    pub menu_selected: usize,
    pub menu_completed: BTreeSet<usize>,
    pub menu_run_id: Option<i64>,
    pub menu_active_step: Option<usize>,
    pub menu_step_outcome: Option<crate::practice::menu::AdvanceOutcome>,
    pub menu_step_progress: Option<crate::practice::menu::ProgressUpdate>,
    pub menu_step_reference: Option<crate::practice::menu::ReferenceJudgment>,
    pub menu_step_others: Vec<crate::practice::menu::progress::StepJudgment>,
    pub menu_step_deferral: Option<crate::practice::menu::progress::JudgmentDeferral>,
    analysis_as_of: chrono::NaiveDate,
    bench_run: Option<BenchRun>,
    ui_now: Instant,
    input_blocked_until: Option<Instant>,
    abort_confirm_until: Option<Instant>,
    settings_original: Option<(
        KeyboardConfig,
        crate::config::DisplayConfig,
        Lang,
        crate::config::ErrorMode,
        crate::config::MenuConfig,
    )>,
    menu_original_config: Option<Config>,
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
        app.refresh_daily_menu();
        app
    }

    fn new_without_storage(config: Config, home_dir: PathBuf, date: chrono::NaiveDate) -> Self {
        let now = Instant::now();
        let mode_selected = mode_index(config.mode.kind);
        let practice_theme = weekly_theme(date, config.practice.weekly_theme);
        let layouts = builtin_layout_names()
            .into_iter()
            .map(|name| (name.to_owned(), builtin_layout(name).expect("bundled layouts must be valid")))
            .collect();
        Self {
            screen: Screen::Title,
            config,
            graphics: GraphicsDetection::default(),
            keyboard_images: RefCell::new(KeyboardImageCache::new()),
            bigtext_images: RefCell::new(BigTextImageState::new()),
            analysis_images: RefCell::new(AnalysisImageCache::new()),
            result_images: RefCell::new(ResultImageCache::new()),
            title_images: RefCell::new(TitleImageCache::new()),
            game: None,
            last_result: None,
            result_record: None,
            result_comparison: ResultComparison::default(),
            legacy_wpm: None,
            result_origin: ResultOrigin::JustPlayed,
            last_save_id: None,
            last_save_error: None,
            result_insights: Vec::new(),
            result_grade_line: None,
            mode_grade_line: None,
            history: Vec::new(),
            history_sessions: Vec::new(),
            history_ranks: Vec::new(),
            history_selected: 0,
            history_sort: HistorySort::DateDesc,
            history_kind_filter: HistoryKindFilter::All,
            history_mode_filter: None,
            history_language_filter: None,
            history_period: AnalysisPeriod::All,
            history_expanded: HashSet::new(),
            history_origin: HistoryOrigin::Title,
            history_load_error: None,
            history_detail: None,
            history_replay: None,
            history_replay_started: None,
            history_export_dir: None,
            history_return_state: None,
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
            bench_summary_records: Vec::new(),
            bench_summary_current: Vec::new(),
            bench_summary_analysis: crate::analysis::bench_summary::BenchmarkRunAnalysis::default(),
            bench_summary_grades: crate::analysis::grades::GradesAnalysis::default(),
            bench_resume: None,
            analysis_tab: 0,
            analysis_period: AnalysisPeriod::All,
            analysis_mode_filter: None,
            analysis_records: Vec::new(),
            analysis_details: DetailedMetrics::default(),
            analysis_summary: crate::analysis::summary::SummaryReport::default(),
            analysis_coach: None,
            analysis_summary_scroll: 0,
            analysis_transition_selected: 0,
            analysis_keyboard_view: 0,
            analysis_confusion_selected: 0,
            analysis_goal_picker: false,
            analysis_goal_selected: 0,
            adaptive_focus: Vec::new(),
            adaptive_candidates: Vec::new(),
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
            daily_menu: None,
            menu_error: None,
            menu_selected: 0,
            menu_completed: BTreeSet::new(),
            menu_run_id: None,
            menu_active_step: None,
            menu_step_outcome: None,
            menu_step_progress: None,
            menu_step_reference: None,
            menu_step_others: Vec::new(),
            menu_step_deferral: None,
            analysis_as_of: date,
            bench_run: None,
            ui_now: now,
            input_blocked_until: None,
            abort_confirm_until: None,
            settings_original: None,
            menu_original_config: None,
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
                config.mode.phrase.lang = lang.to_owned();
            }
            if let Some(course) = params.get("course").and_then(serde_json::Value::as_str) {
                config.mode.phrase.course = course.to_owned();
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

    pub fn set_graphics_detection(&mut self, graphics: GraphicsDetection) {
        self.graphics = graphics;
        self.keyboard_images.get_mut().clear();
        self.analysis_images.get_mut().clear();
        self.result_images.get_mut().clear();
        self.title_images.get_mut().clear();
    }

    /// Refresh cell pixels from a resize ioctl without querying terminal capabilities again.
    pub fn update_graphics_cell_pixels(&mut self, size: TerminalPixelSize) {
        let next = update_cell_pixels(self.graphics.cell_pixels, size);
        if self.graphics.cell_pixels != next {
            self.graphics.cell_pixels = next;
            self.keyboard_images.get_mut().clear();
            self.analysis_images.get_mut().clear();
            self.result_images.get_mut().clear();
            self.title_images.get_mut().clear();
        }
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
        let series = preferred_bench_series(records.iter().filter_map(|record| record.header.bench_id.as_deref()), BenchPart::A);
        let bench_a = records
            .iter()
            .filter(|record| {
                record
                    .header
                    .bench_id
                    .as_deref()
                    .and_then(parse_bench_id)
                    .is_some_and(|(candidate_series, part, _)| {
                        part == BenchPart::A && is_preferred_bench_series(candidate_series, BenchPart::A, series)
                    })
            })
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
                        half_life_days: self.config.analysis.half_life_days,
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

    fn refresh_daily_menu(&mut self) {
        match crate::practice::menu::build_from_storage(
            &self.db_path(),
            &self.home_dir,
            &self.config,
            self.config.general.lang,
            self.analysis_as_of,
        ) {
            Ok(mut menu) => {
                let language_key = crate::practice::menu::menu_run_language_key(&self.config, self.config.general.lang);
                let existing = db::find_menu_run_for_day(
                    &self.db_path(),
                    menu.date,
                    &self.config.keyboard.layout,
                    &language_key,
                    &menu.catalog_version,
                    menu.requested_minutes,
                )
                .ok()
                .flatten();
                if let Some(run) = &existing
                    && let Ok(stored) = serde_json::from_str::<crate::practice::menu::DailyMenu>(&run.plan_json)
                {
                    menu = stored;
                }
                self.menu_run_id = existing.as_ref().map(|run| run.id);
                self.menu_completed = existing
                    .and_then(|run| db::completed_menu_steps(&self.db_path(), run.id).ok())
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|index| index.checked_sub(1))
                    .collect();
                self.daily_menu = Some(menu);
                self.menu_error = None;
                self.menu_selected = self
                    .menu_selected
                    .min(self.daily_menu.as_ref().map_or(0, |menu| menu.steps.len().saturating_sub(1)));
            }
            Err(error) => {
                self.daily_menu = None;
                self.menu_run_id = None;
                self.menu_completed.clear();
                self.menu_error = Some(error.to_string());
            }
        }
    }

    fn open_menu(&mut self) {
        self.refresh_daily_menu();
        self.menu_active_step = None;
        self.clear_menu_step_result();
        self.menu_original_config = None;
        self.screen = Screen::Menu;
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        self.handle_key_at(key, Instant::now());
    }

    pub fn handle_key_at(&mut self, key: KeyEvent, now: Instant) {
        self.ui_now = now;
        if key.kind == KeyEventKind::Release {
            self.handle_release(key, now);
            return;
        }
        if self.result_input_delay_remaining().is_some() {
            return;
        }
        if key.kind != KeyEventKind::Press {
            if key.code != KeyCode::Esc {
                self.abort_confirm_until = None;
            }
            return;
        }
        if self.screen == Screen::Playing && key.code == KeyCode::F(2) {
            self.abort_confirm_until = None;
            self.report_current_reading(now);
            return;
        }
        let action = classify(key);
        if !matches!(action, InputAction::Abort) {
            self.abort_confirm_until = None;
        }
        match self.screen {
            Screen::Title => self.handle_title_key(key, action),
            Screen::Menu => self.handle_menu_key(key, action),
            Screen::MenuFocus { .. } => self.handle_menu_focus_key(key),
            Screen::ModeSelect => self.handle_mode_select_key(key, action),
            Screen::KeyPicker => self.handle_key_picker_key(key, action),
            Screen::Countdown { .. } => {
                if matches!(action, InputAction::Abort) && self.menu_active_step.is_some() {
                    self.abort_menu_step();
                } else if matches!(action, InputAction::Abort) && self.confirm_abort(now) {
                    if self.bench_run.is_some() {
                        self.abort_benchmark();
                    } else {
                        self.game = None;
                        self.screen = Screen::Title;
                    }
                }
            }
            Screen::Playing => self.handle_playing_key(action, now),
            Screen::Result => self.handle_result_key(key, action, now),
            Screen::History => self.handle_history_key(key, action),
            Screen::HistoryDetail => self.handle_history_detail_key(key, action, now),
            Screen::Replay => self.handle_history_replay_key(key, action, now),
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
                self.bench_resume = db::incomplete_benchmark(&self.db_path(), crate::bench::BENCH_SERIES, self.analysis_as_of)
                    .ok()
                    .flatten();
                self.screen = Screen::BenchSelect;
            }
            KeyCode::Char('a') | KeyCode::Char('A') => self.open_analysis(),
            KeyCode::Char('m') | KeyCode::Char('M') => self.open_menu(),
            KeyCode::Char('s') | KeyCode::Char('S') => {
                self.settings_selected = 0;
                self.settings_save_error = None;
                self.settings_original = Some((
                    self.config.keyboard.clone(),
                    self.config.display.clone(),
                    self.config.general.lang,
                    self.config.session.error_mode,
                    self.config.menu.clone(),
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
        self.refresh_mode_grade();
        self.screen = Screen::ModeSelect;
    }

    fn handle_menu_key(&mut self, key: KeyEvent, action: InputAction) {
        let last = self.daily_menu.as_ref().map_or(0, |menu| menu.steps.len().saturating_sub(1));
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.menu_selected = self.menu_selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.menu_selected = (self.menu_selected + 1).min(last),
            KeyCode::Enter | KeyCode::Char(' ') => self.begin_menu_step(self.menu_selected),
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                self.menu_active_step = None;
                self.clear_menu_step_result();
                self.menu_original_config = None;
                self.screen = Screen::Title;
            }
            _ => {}
        }
        if matches!(action, InputAction::Abort) && key.code != KeyCode::Esc {
            self.menu_active_step = None;
            self.clear_menu_step_result();
            self.menu_original_config = None;
            self.screen = Screen::Title;
        }
    }

    fn handle_menu_focus_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.abort_menu_step();
        } else {
            self.start_countdown();
        }
    }

    fn begin_menu_step(&mut self, index: usize) {
        if self.menu_run_id.is_none() {
            let Some(menu) = self.daily_menu.as_ref() else { return };
            let plan = match serde_json::to_string(menu) {
                Ok(plan) => plan,
                Err(error) => {
                    self.menu_error = Some(error.to_string());
                    return;
                }
            };
            let language_key = crate::practice::menu::menu_run_language_key(&self.config, self.config.general.lang);
            match db::ensure_menu_run(
                &self.db_path(),
                menu.date,
                &self.config.keyboard.layout,
                &language_key,
                &menu.catalog_version,
                menu.requested_minutes,
                &plan,
            ) {
                Ok(run) => {
                    // The day's run already holds recorded steps of another
                    // plan: keep using that plan so step indices stay valid.
                    if run.plan_json != plan
                        && let Ok(stored) = serde_json::from_str::<crate::practice::menu::DailyMenu>(&run.plan_json)
                    {
                        self.menu_completed = db::completed_menu_steps(&self.db_path(), run.id)
                            .unwrap_or_default()
                            .into_iter()
                            .filter_map(|index| index.checked_sub(1))
                            .collect();
                        self.daily_menu = Some(stored);
                    }
                    self.menu_run_id = Some(run.id);
                }
                Err(error) => {
                    self.menu_error = Some(error.to_string());
                    return;
                }
            }
        }
        let Some(step) = self.daily_menu.as_ref().and_then(|menu| menu.steps.get(index)).cloned() else {
            return;
        };
        let base = self.config.clone();
        match crate::practice::menu::config_for_menu_step(&base, &step) {
            Ok(mut config) => {
                let new_step = self.menu_active_step != Some(index);
                if new_step && config.mode.kind == ModeKind::Burst {
                    self.burst_series = BurstSeries::default();
                }
                config.session.duration_seconds = u64::from(step.minutes) * 60;
                self.menu_original_config = Some(base);
                self.config = config;
                self.menu_active_step = Some(index);
                self.menu_selected = index;
                self.clear_menu_step_result();
                self.input_blocked_until = None;
                self.abort_confirm_until = None;
                self.screen = Screen::MenuFocus { remaining: 2.0 };
            }
            Err(error) => self.menu_error = Some(error),
        }
    }

    fn restore_menu_config(&mut self) {
        if let Some(config) = self.menu_original_config.take() {
            self.config = config;
        }
    }

    fn abort_menu_step(&mut self) {
        self.game = None;
        self.restore_menu_config();
        self.menu_active_step = None;
        self.clear_menu_step_result();
        self.abort_confirm_until = None;
        self.input_blocked_until = None;
        self.screen = Screen::Menu;
    }

    fn continue_menu(&mut self) {
        let Some(current) = self.menu_active_step else {
            self.screen = Screen::Menu;
            return;
        };
        let burst_incomplete = self
            .daily_menu
            .as_ref()
            .and_then(|menu| menu.steps.get(current))
            .is_some_and(|step| step.mode == "burst" && !self.burst_series.complete());
        if burst_incomplete {
            self.begin_menu_step(current);
            return;
        }
        let next = current + 1;
        if self.daily_menu.as_ref().is_some_and(|menu| next < menu.steps.len()) {
            self.begin_menu_step(next);
        } else {
            self.menu_active_step = None;
            self.clear_menu_step_result();
            self.screen = Screen::Menu;
        }
    }

    fn refresh_mode_grade(&mut self) {
        let records = db::load_records_filtered(&self.db_path(), None, None).unwrap_or_default();
        let prescriptions = db::load_prescriptions(&self.db_path()).unwrap_or_default();
        let selected_goal = db::load_selected_goal(&self.db_path()).unwrap_or_default();
        self.mode_grade_line = crate::analysis::coach::build(
            &records,
            &self.config,
            &self.home_dir,
            &prescriptions,
            selected_goal,
            self.analysis_as_of,
            self.config.general.lang,
        )
        .ok()
        .and_then(|report| crate::analysis::coach::compact_grades(&report.grades, self.config.general.lang));
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
                    self.config.display.clone(),
                    self.config.general.lang,
                    self.config.session.error_mode,
                    self.config.menu.clone(),
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
                if matches!(selected, ModeKind::Words | ModeKind::Sentence | ModeKind::Phrase | ModeKind::Adaptive) {
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
            KeyCode::Char('l') | KeyCode::Char('L') if MODES[self.mode_selected] == ModeKind::Phrase => {
                self.config.mode.phrase.lang = if self.config.mode.phrase.lang == "en" { "ja" } else { "en" }.to_owned();
            }
            KeyCode::Char('c') | KeyCode::Char('C') if MODES[self.mode_selected] == ModeKind::Phrase => {
                self.config.mode.phrase.course = match self.config.mode.phrase.course.as_str() {
                    "short" => "normal",
                    "normal" => "long",
                    "long" => "mixed",
                    _ => "short",
                }
                .to_owned();
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
                        | ModeKind::Phrase
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
        let has_resume = self.bench_resume.is_some();
        let start_index = usize::from(has_resume);
        let optional_index = start_index + 1;
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.bench_selected = self.bench_selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.bench_selected = (self.bench_selected + 1).min(optional_index),
            KeyCode::Char('d') | KeyCode::Char('D') => self.bench_include_d = !self.bench_include_d,
            KeyCode::Enter if has_resume && self.bench_selected == 0 => self.resume_benchmark(),
            KeyCode::Enter if self.bench_selected == start_index => self.start_benchmark(),
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
            KeyCode::Up | KeyCode::Char('k') if self.analysis_tab == 0 && self.analysis_goal_picker => {
                self.analysis_goal_selected = self.analysis_goal_selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') if self.analysis_tab == 0 && self.analysis_goal_picker => {
                let count = self.analysis_coach.as_ref().map_or(0, |report| report.goals.len());
                self.analysis_goal_selected = (self.analysis_goal_selected + 1).min(count.saturating_sub(1));
            }
            KeyCode::Enter if self.analysis_tab == 0 && self.analysis_goal_picker => {
                if let Some(goal) = self
                    .analysis_coach
                    .as_ref()
                    .and_then(|report| report.goals.get(self.analysis_goal_selected))
                    .cloned()
                {
                    let _ = db::save_selected_goal(&self.db_path(), &goal);
                }
                self.analysis_goal_picker = false;
                self.reload_analysis();
            }
            KeyCode::Esc if self.analysis_tab == 0 && self.analysis_goal_picker => {
                self.analysis_goal_picker = false;
            }
            KeyCode::Left if self.analysis_tab == 2 && self.analysis_keyboard_view == 3 => {
                self.analysis_confusion_selected = self.analysis_confusion_selected.saturating_sub(1);
            }
            KeyCode::Right if self.analysis_tab == 2 && self.analysis_keyboard_view == 3 => {
                let count = self.analysis_coach.as_ref().map_or(0, |report| {
                    report
                        .diagnosis
                        .errors
                        .confusion
                        .iter()
                        .map(|row| row.expected.as_str())
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                });
                self.analysis_confusion_selected = (self.analysis_confusion_selected + 1).min(count.saturating_sub(1));
            }
            KeyCode::Tab | KeyCode::Right => self.analysis_tab = (self.analysis_tab + 1) % 6,
            KeyCode::BackTab | KeyCode::Left => self.analysis_tab = (self.analysis_tab + 5) % 6,
            KeyCode::Up | KeyCode::Char('k') if self.analysis_tab == 0 => {
                self.analysis_summary_scroll = self.analysis_summary_scroll.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') if self.analysis_tab == 0 => {
                let maximum = self
                    .analysis_coach
                    .as_ref()
                    .map_or(self.analysis_summary.lines.len(), |report| report.lines.len())
                    .saturating_sub(1)
                    .min(u16::MAX as usize) as u16;
                self.analysis_summary_scroll = (self.analysis_summary_scroll + 1).min(maximum);
            }
            KeyCode::Up | KeyCode::Char('k') if self.analysis_tab == 3 => {
                self.analysis_transition_selected = self.analysis_transition_selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') if self.analysis_tab == 3 => {
                let eligible = self
                    .analysis_details
                    .bigrams
                    .iter()
                    .filter(|row| row.n >= self.config.analysis.min_transition_n)
                    .count();
                self.analysis_transition_selected = (self.analysis_transition_selected + 1).min(eligible.saturating_sub(1));
            }
            KeyCode::Enter if self.analysis_tab == 3 => self.practice_selected_transition(),
            KeyCode::Enter if self.analysis_tab == 2 => {
                self.analysis_keyboard_view = (self.analysis_keyboard_view + 1) % 4;
                self.analysis_confusion_selected = 0;
            }
            KeyCode::Char('t') | KeyCode::Char('T') if self.analysis_tab == 0 => {
                if self.analysis_coach.as_ref().is_some_and(|report| !report.goals.is_empty()) {
                    self.analysis_goal_picker = !self.analysis_goal_picker;
                    self.analysis_goal_selected = 0;
                }
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
                const MODES: [Option<&str>; 13] = [
                    None,
                    Some("random"),
                    Some("words"),
                    Some("romaji"),
                    Some("sentence"),
                    Some("phrase"),
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
        self.analysis_keyboard_view = 0;
        self.analysis_confusion_selected = 0;
        self.analysis_goal_picker = false;
        self.analysis_goal_selected = 0;
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
                half_life_days: self.config.analysis.half_life_days,
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
        let mut prescriptions = db::load_prescriptions(&self.db_path()).unwrap_or_default();
        if let Some(since) = since {
            prescriptions.retain(|row| row.created_at.date_naive() >= since);
        }
        if self.analysis_mode_filter.as_deref().is_some_and(|mode| mode != "adaptive") {
            prescriptions.clear();
        }
        let selected_goal = db::load_selected_goal(&self.db_path()).unwrap_or_default();
        self.analysis_coach = crate::analysis::coach::build(
            &self.analysis_records,
            &self.config,
            &self.home_dir,
            &prescriptions,
            selected_goal,
            today,
            self.config.general.lang,
        )
        .ok();
        self.analysis_summary_scroll = 0;
        self.analysis_transition_selected = self.analysis_transition_selected.min(
            self.analysis_details
                .bigrams
                .iter()
                .filter(|row| row.n >= self.config.analysis.min_transition_n)
                .count()
                .saturating_sub(1),
        );
    }

    fn practice_selected_transition(&mut self) {
        let mut rows = self.analysis_details.bigrams.clone();
        rows.retain(|row| row.n >= self.config.analysis.min_transition_n);
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
        self.bench_summary_records.clear();
        self.bench_summary_current.clear();
        self.bench_summary_analysis = crate::analysis::bench_summary::BenchmarkRunAnalysis::default();
        self.bench_summary_grades = crate::analysis::grades::GradesAnalysis::default();
        self.bench_resume = None;
        self.bench_run = Some(run);
        self.configure_bench_part();
    }

    fn resume_benchmark(&mut self) {
        let Some(resume) = self.bench_resume.take() else { return };
        let run = BenchRun {
            include_d: self.bench_include_d,
            parallel_index: resume.parallel_index,
            part: resume.part,
            original_session: self.config.session.clone(),
            original_mode: self.config.mode.clone(),
        };
        self.bench_results = resume.completed;
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

    fn continue_benchmark(&mut self, now: Instant) {
        let next = self.bench_run.as_ref().and_then(|run| run.part.next(run.include_d));
        if let Some(next) = next {
            if let Some(run) = self.bench_run.as_mut() {
                run.part = next;
            }
            self.configure_bench_part();
        } else if let Some(parallel_index) = self.bench_run.as_ref().map(|run| run.parallel_index) {
            if let Ok(results) = db::bench_results_for_day_form(&self.db_path(), crate::bench::BENCH_SERIES, self.analysis_as_of, parallel_index)
                && !results.is_empty()
            {
                self.bench_results = results;
            }
            self.refresh_bench_summary(parallel_index);
            self.leave_benchmark(Screen::BenchSummary);
            self.begin_result_input_delay(now);
        }
    }

    fn refresh_bench_summary(&mut self, parallel_index: u8) {
        let records = db::load_records_filtered(&self.db_path(), None, Some("bench")).unwrap_or_default();
        let mut selected = BTreeMap::new();
        for record in &records {
            let Some((series, part, index)) = record.header.bench_id.as_deref().and_then(parse_bench_id) else {
                continue;
            };
            let on_summary_day = chrono::DateTime::parse_from_rfc3339(&record.header.finished_at)
                .is_ok_and(|finished| finished.with_timezone(&chrono::Local).date_naive() == self.analysis_as_of);
            if series == crate::bench::BENCH_SERIES && index == parallel_index && on_summary_day {
                selected.insert(part, record.clone());
            }
        }
        let current = selected.into_values().collect::<Vec<_>>();
        let settings = AnalysisSettings {
            pause_ms: self.config.analysis.pause_ms,
            shrinkage_k: self.config.analysis.shrinkage_k,
            half_life_days: self.config.analysis.half_life_days,
        };
        let geometry = crate::layout::geometry::geometry(&self.config.keyboard.geometry)
            .or_else(|| crate::layout::geometry::geometry("ansi"))
            .expect("bundled keyboard geometry");
        let prescriptions = db::load_prescriptions(&self.db_path()).unwrap_or_default();
        self.bench_summary_analysis = crate::analysis::bench_summary::analyze_benchmark_run(
            &current,
            &records,
            self.practice_layout(),
            &geometry,
            settings,
            self.config.analysis.min_transition_n,
            &prescriptions,
        );
        if let Ok(report) = crate::analysis::coach::build(
            &records,
            &self.config,
            &self.home_dir,
            &prescriptions,
            None,
            self.analysis_as_of,
            self.config.general.lang,
        ) {
            self.bench_summary_grades = report.grades;
        }
        self.bench_summary_records = records;
        self.bench_summary_current = current;
    }

    fn abort_benchmark(&mut self) {
        self.leave_benchmark(Screen::Title);
    }

    fn leave_benchmark(&mut self, destination: Screen) {
        self.game = None;
        if let Some(run) = self.bench_run.take() {
            self.config.session = run.original_session;
            self.config.mode = run.original_mode;
        }
        self.active_bench_id = None;
        self.bench_resume = None;
        self.abort_confirm_until = None;
        self.input_blocked_until = None;
        self.screen = destination;
    }

    fn begin_result_input_delay(&mut self, now: Instant) {
        let delay = self.config.session.result_input_delay();
        self.input_blocked_until = (!delay.is_zero()).then_some(now + delay);
        self.ui_now = now;
    }

    fn confirm_abort(&mut self, now: Instant) -> bool {
        if self.abort_confirm_until.is_some_and(|deadline| now <= deadline) {
            self.abort_confirm_until = None;
            true
        } else {
            self.abort_confirm_until = Some(now + ABORT_CONFIRMATION_DURATION);
            false
        }
    }

    pub fn abort_confirmation_active(&self) -> bool {
        self.abort_confirm_until.is_some_and(|deadline| self.ui_now <= deadline)
    }

    pub fn result_input_delay_remaining(&self) -> Option<Duration> {
        let applies =
            matches!(self.screen, Screen::BenchSummary) || matches!(self.screen, Screen::Result) && self.result_origin == ResultOrigin::JustPlayed;
        applies
            .then_some(self.input_blocked_until?)
            .filter(|deadline| self.ui_now < *deadline)
            .map(|deadline| deadline.saturating_duration_since(self.ui_now))
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
                self.settings_selected = (self.settings_selected + 1).min(11);
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
        if let Some((keyboard, display, language, error_mode, menu)) = self.settings_original.take() {
            self.config.keyboard = keyboard;
            self.config.display = display;
            self.config.general.lang = language;
            self.config.session.error_mode = error_mode;
            self.config.menu = menu;
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
                self.config.display.graphics = match (self.config.display.graphics, direction.is_positive()) {
                    (GraphicsPreference::Auto, true) => GraphicsPreference::Kitty,
                    (GraphicsPreference::Kitty, true) => GraphicsPreference::Sixel,
                    (GraphicsPreference::Sixel, true) => GraphicsPreference::Iterm2,
                    (GraphicsPreference::Iterm2, true) => GraphicsPreference::Off,
                    (GraphicsPreference::Off, true) => GraphicsPreference::Auto,
                    (GraphicsPreference::Auto, false) => GraphicsPreference::Off,
                    (GraphicsPreference::Kitty, false) => GraphicsPreference::Auto,
                    (GraphicsPreference::Sixel, false) => GraphicsPreference::Kitty,
                    (GraphicsPreference::Iterm2, false) => GraphicsPreference::Sixel,
                    (GraphicsPreference::Off, false) => GraphicsPreference::Iterm2,
                };
            }
            7 => {
                self.config.general.lang = match self.config.general.lang {
                    Lang::En => Lang::Ja,
                    Lang::Ja => Lang::En,
                };
            }
            8 => {
                let options = [None, Some(vec![Lang::En]), Some(vec![Lang::Ja]), Some(vec![Lang::En, Lang::Ja])];
                let current = options.iter().position(|value| *value == self.config.menu.languages).unwrap_or(0);
                let next = if direction.is_positive() {
                    (current + 1) % options.len()
                } else {
                    (current + options.len() - 1) % options.len()
                };
                self.config.menu.languages = options[next].clone();
            }
            9 => {
                self.config.session.error_mode = match self.config.session.error_mode {
                    crate::config::ErrorMode::Stop => crate::config::ErrorMode::Advance,
                    crate::config::ErrorMode::Advance => crate::config::ErrorMode::Stop,
                };
            }
            10 => cycle_string(
                &mut self.config.display.feedback,
                &["summary".to_owned(), "detailed".to_owned()],
                direction,
            ),
            11 => {
                let configured_path = self
                    .settings_original
                    .as_ref()
                    .map(|(_, display, _, _, _)| display.big_font.as_str())
                    .filter(|value| !matches!(*value, "biz-ud-gothic" | "shinonome"));
                let mut options = vec!["biz-ud-gothic".to_owned(), "shinonome".to_owned()];
                if let Some(path) = configured_path {
                    options.push(path.to_owned());
                }
                cycle_string(&mut self.config.display.big_font, &options, direction);
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

    fn handle_result_key(&mut self, key: KeyEvent, action: InputAction, now: Instant) {
        match key.code {
            KeyCode::Char(' ') | KeyCode::Enter => match self.result_origin {
                ResultOrigin::JustPlayed if self.menu_active_step.is_some() => self.continue_menu(),
                ResultOrigin::JustPlayed if self.bench_run.is_some() => self.continue_benchmark(now),
                ResultOrigin::JustPlayed if self.config.mode.kind == ModeKind::Burst && self.burst_series.complete() => {
                    self.screen = Screen::Title;
                }
                ResultOrigin::JustPlayed => self.start_countdown(),
                ResultOrigin::History => self.open_history(),
            },
            KeyCode::Char('h') | KeyCode::Char('H') if self.bench_run.is_none() => self.open_history(),
            KeyCode::Char('q') | KeyCode::Char('Q') if self.bench_run.is_none() => self.should_quit = true,
            KeyCode::Esc => match self.result_origin {
                ResultOrigin::JustPlayed if self.menu_active_step.is_some() => {
                    self.menu_active_step = None;
                    self.clear_menu_step_result();
                    self.screen = Screen::Menu;
                }
                ResultOrigin::JustPlayed if self.bench_run.is_some() => {
                    if self.confirm_abort(now) {
                        self.abort_benchmark();
                    }
                }
                ResultOrigin::JustPlayed => self.should_quit = true,
                ResultOrigin::History => self.open_history(),
            },
            _ => {}
        }
        if matches!(action, InputAction::Abort) && key.code != KeyCode::Esc {
            match self.result_origin {
                ResultOrigin::JustPlayed if self.menu_active_step.is_some() => {
                    self.menu_active_step = None;
                    self.clear_menu_step_result();
                    self.screen = Screen::Menu;
                }
                ResultOrigin::JustPlayed if self.bench_run.is_some() => {
                    if self.confirm_abort(now) {
                        self.abort_benchmark();
                    }
                }
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
                self.rebuild_history();
            }
            KeyCode::Tab => {
                self.history_kind_filter = self.history_kind_filter.next();
                self.history_selected = 0;
                self.rebuild_history();
            }
            KeyCode::Char('m') | KeyCode::Char('M') => {
                self.cycle_history_mode();
                self.history_selected = 0;
                self.rebuild_history();
            }
            KeyCode::Char('l') | KeyCode::Char('L') => {
                self.history_language_filter = match self.history_language_filter.as_deref() {
                    None => Some("en".to_owned()),
                    Some("en") => Some("ja".to_owned()),
                    _ => None,
                };
                self.history_selected = 0;
                self.rebuild_history();
            }
            KeyCode::Char('p') | KeyCode::Char('P') => {
                self.history_period = match self.history_period {
                    AnalysisPeriod::All => AnalysisPeriod::Days30,
                    AnalysisPeriod::Days30 => AnalysisPeriod::Days7,
                    AnalysisPeriod::Days7 => AnalysisPeriod::All,
                };
                self.history_selected = 0;
                self.rebuild_history();
            }
            KeyCode::Enter => self.activate_history_entry(),
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                self.leave_history();
            }
            _ => {}
        }
        if matches!(action, InputAction::Abort) && !matches!(key.code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q')) {
            self.leave_history();
        }
    }

    fn handle_history_detail_key(&mut self, key: KeyEvent, action: InputAction, now: Instant) {
        match key.code {
            KeyCode::Esc | KeyCode::Backspace | KeyCode::Char(' ') | KeyCode::Enter => {
                self.screen = Screen::History;
            }
            KeyCode::Char('r') | KeyCode::Char('R') => self.start_history_replay(now),
            KeyCode::Char('e') | KeyCode::Char('E') => self.export_history_detail(),
            KeyCode::Char('q') | KeyCode::Char('Q') => self.should_quit = true,
            _ => {}
        }
        if matches!(action, InputAction::Abort) {
            self.screen = Screen::History;
        }
    }

    fn start_history_replay(&mut self, now: Instant) {
        let Some(record) = self
            .history_detail
            .as_ref()
            .filter(|detail| detail.record.header.metrics_version != 0)
            .map(|detail| detail.record.clone())
        else {
            return;
        };
        let mut replay = crate::replay::Replay::new_with_lang(
            record,
            crate::replay::PlaybackTiming::Recorded { speed: 1.0 },
            false,
            false,
            self.config.general.lang,
        );
        replay.set_big_font(&self.config.display.big_font, &self.home_dir);
        replay.set_graphics_detection(self.graphics.clone());
        self.history_replay = Some(Box::new(replay));
        self.history_replay_started = Some(now);
        self.screen = Screen::Replay;
    }

    fn handle_history_replay_key(&mut self, key: KeyEvent, action: InputAction, now: Instant) {
        let elapsed = self
            .history_replay_started
            .map_or(Duration::ZERO, |started| now.saturating_duration_since(started));
        match key.code {
            KeyCode::Char(' ') => {
                if let Some(replay) = self.history_replay.as_mut() {
                    replay.toggle_pause(elapsed);
                }
            }
            KeyCode::Right => {
                if let Some(replay) = self.history_replay.as_mut() {
                    replay.step();
                }
            }
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => self.finish_history_replay(),
            _ => {}
        }
        if matches!(action, InputAction::Abort) && !matches!(key.code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q')) {
            self.finish_history_replay();
        }
    }

    fn finish_history_replay(&mut self) {
        self.history_replay = None;
        self.history_replay_started = None;
        self.screen = Screen::HistoryDetail;
    }

    fn export_history_detail(&mut self) {
        let Some(detail) = self.history_detail.as_mut().filter(|detail| detail.record.header.metrics_version != 0) else {
            return;
        };
        let id = detail.summary.id;
        let directory = self
            .history_export_dir
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let path = directory.join(format!("session-{id}.ttrec"));
        let result = std::fs::File::create(&path)
            .and_then(|file| crate::record::ttrec::write(&detail.record, file).map_err(|error| std::io::Error::other(error.to_string())));
        detail.export_notice = Some(match result {
            Ok(()) => match self.config.general.lang {
                Lang::En => format!("Exported: {}", path.display()),
                Lang::Ja => format!("書き出しました: {}", path.display()),
            },
            Err(error) => match self.config.general.lang {
                Lang::En => format!("Export failed: {error}"),
                Lang::Ja => format!("書き出せませんでした: {error}"),
            },
        });
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
                if self.menu_active_step.is_some() {
                    if let Some(game) = self.game.as_mut() {
                        game.session.abort(now.saturating_duration_since(game.started_at));
                    }
                    self.abort_menu_step();
                } else if self.confirm_abort(now) {
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
        self.input_blocked_until = None;
        self.abort_confirm_until = None;
    }

    fn open_history(&mut self) {
        let entering = !matches!(self.screen, Screen::History | Screen::HistoryDetail);
        if entering {
            self.history_origin = if self.screen == Screen::Result && self.result_origin == ResultOrigin::JustPlayed {
                self.history_return_state = Some(HistoryReturnState {
                    result: self.last_result.clone(),
                    result_record: self.result_record.clone(),
                    result_comparison: self.result_comparison.clone(),
                    legacy_wpm: self.legacy_wpm,
                    last_save_id: self.last_save_id,
                    last_save_error: self.last_save_error.clone(),
                });
                HistoryOrigin::Result
            } else {
                self.history_return_state = None;
                HistoryOrigin::Title
            };
            self.history_selected = 0;
            self.history_sort = HistorySort::DateDesc;
            self.history_kind_filter = HistoryKindFilter::All;
            self.history_mode_filter = None;
            self.history_language_filter = None;
            self.history_period = AnalysisPeriod::All;
            self.history_expanded.clear();
            self.history_detail = None;
        }
        match db::list(&self.db_path()) {
            Ok(items) => {
                self.history_sessions = items;
                self.history_load_error = None;
                self.rebuild_history();
            }
            Err(e) => {
                self.history_sessions.clear();
                self.history.clear();
                self.history_ranks.clear();
                self.history_selected = 0;
                self.history_load_error = Some(e.to_string());
            }
        }
        self.screen = Screen::History;
    }

    fn leave_history(&mut self) {
        match self.history_origin {
            HistoryOrigin::Title => self.screen = Screen::Title,
            HistoryOrigin::Result => {
                if let Some(state) = self.history_return_state.take() {
                    self.last_result = state.result;
                    self.result_record = state.result_record;
                    self.result_comparison = state.result_comparison;
                    self.legacy_wpm = state.legacy_wpm;
                    self.last_save_id = state.last_save_id;
                    self.last_save_error = state.last_save_error;
                }
                self.result_origin = ResultOrigin::JustPlayed;
                self.screen = Screen::Result;
            }
        }
    }

    fn activate_history_entry(&mut self) {
        let Some(entry) = self.history.get(self.history_selected) else {
            return;
        };
        if let HistoryEntry::BenchRun { id, .. } = entry {
            let id = *id;
            if !self.history_expanded.remove(&id) {
                self.history_expanded.insert(id);
            }
            self.rebuild_history();
            return;
        }
        self.open_history_detail();
    }

    pub(crate) fn history_sessions_empty(&self) -> bool {
        self.history_sessions.is_empty()
    }

    fn cycle_history_mode(&mut self) {
        let filter = HistoryFilter {
            mode: None,
            language: self.history_language_filter.as_deref(),
            period: self.history_period,
            today: self.analysis_as_of,
        };
        let recorded = self
            .history_sessions
            .iter()
            .filter(|summary| match self.history_kind_filter {
                HistoryKindFilter::All => true,
                HistoryKindFilter::Practice => bench_identity(summary).is_none(),
                HistoryKindFilter::Bench => bench_identity(summary).is_some(),
            })
            .filter(|summary| summary_matches(summary, filter))
            .map(history_effective_mode)
            .collect::<BTreeSet<_>>();
        const ORDER: [&str; 11] = [
            "random",
            "words",
            "romaji",
            "sentence",
            "phrase",
            "ngram",
            "adaptive",
            "unlock",
            "rhythm",
            "burst",
            "endurance",
        ];
        let mut options = vec![None];
        options.extend(
            ORDER
                .into_iter()
                .filter(|mode| recorded.contains(*mode))
                .map(|mode| Some(mode.to_owned())),
        );
        for mode in recorded {
            if !options.iter().any(|candidate| candidate.as_deref() == Some(mode.as_str())) {
                options.push(Some(mode));
            }
        }
        let current = options.iter().position(|value| *value == self.history_mode_filter).unwrap_or(0);
        self.history_mode_filter = options[(current + 1) % options.len()].clone();
    }

    fn rebuild_history(&mut self) {
        let mut rows = build_history_rows(
            &self.history_sessions,
            self.history_kind_filter,
            self.history_mode_filter.as_deref(),
            self.history_language_filter.as_deref(),
            self.history_period,
            self.analysis_as_of,
        );
        sort_history_entries(&mut rows, self.history_sort);
        let ranking =
            self.history_sort == HistorySort::WpmDesc && self.history_mode_filter.is_some() && self.history_kind_filter != HistoryKindFilter::All;
        let mut ranks = rank_history_entries(&rows, ranking, self.history_kind_filter);
        let mut expanded_rows = Vec::new();
        let mut expanded_ranks = Vec::new();
        for (row, rank) in rows.into_iter().zip(ranks.drain(..)) {
            let expanded = matches!(&row, HistoryEntry::BenchRun { id, .. } if self.history_expanded.contains(id));
            let children = match &row {
                HistoryEntry::BenchRun { id, series, form, parts } if expanded => parts
                    .iter()
                    .cloned()
                    .map(|summary| HistoryEntry::BenchPart {
                        run_id: *id,
                        series: series.clone(),
                        form: form.clone(),
                        summary,
                    })
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            };
            expanded_rows.push(row);
            expanded_ranks.push(rank);
            for child in children {
                expanded_rows.push(child);
                expanded_ranks.push(rank);
            }
        }
        self.history = expanded_rows;
        self.history_ranks = expanded_ranks;
        self.history_selected = self.history_selected.min(self.history.len().saturating_sub(1));
    }

    fn open_history_detail(&mut self) {
        let Some(entry) = self.history.get(self.history_selected) else {
            return;
        };
        let entry = match entry {
            HistoryEntry::Session(summary) | HistoryEntry::BenchPart { summary, .. } => summary.clone(),
            HistoryEntry::BenchRun { .. } => return,
        };
        let path = self.db_path();
        match (db::load_result(&path, entry.id, &self.config), db::load_record(&path, entry.id)) {
            (Ok(Some(result)), Ok(Some(record))) => {
                let records = db::load_records_filtered(&path, None, None).unwrap_or_else(|_| vec![record.clone()]);
                let result_comparison = build_result_comparison(&records, &record);
                let personal_best_wpm = result_comparison.best_wpm;
                let previous_wpm_delta = result_comparison.previous_wpm_delta;
                self.last_result = Some(result);
                self.result_record = Some(record.clone());
                self.result_comparison = result_comparison;
                self.legacy_wpm = db::legacy_wpm(&path, entry.id).ok().flatten();
                self.result_origin = ResultOrigin::History;
                self.last_save_id = Some(entry.id);
                self.last_save_error = None;
                self.history_detail = Some(HistoryDetailData {
                    summary: entry,
                    record,
                    personal_best_wpm,
                    previous_wpm_delta,
                    export_notice: None,
                });
                self.screen = Screen::HistoryDetail;
            }
            (Ok(None), _) | (_, Ok(None)) => {
                self.legacy_wpm = None;
                self.history_load_error = Some(format!("{}: #{}", Msg::SessionNotFound.text(self.config.general.lang), entry.id));
            }
            (Err(e), _) | (_, Err(e)) => {
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
        self.adaptive_candidates.clear();
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
                    half_life_days: self.config.analysis.half_life_days,
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
        let japanese_menu_step = self
            .menu_active_step
            .and_then(|index| self.daily_menu.as_ref().and_then(|menu| menu.steps.get(index)))
            .is_some_and(|step| step.language == Lang::Ja);
        if japanese_menu_step
            && matches!(
                self.config.mode.kind,
                ModeKind::Words | ModeKind::Ngram | ModeKind::Unlock | ModeKind::Rhythm | ModeKind::Burst
            )
        {
            // These modes use the English word pool by default. Keep their
            // gameplay behavior, but feed the generator Japanese word prompts
            // for a Japanese menu step.
            generator_config.kind = ModeKind::Romaji;
        }
        let pacer_supported = matches!(
            self.config.mode.kind,
            ModeKind::Words | ModeKind::Sentence | ModeKind::Phrase | ModeKind::Adaptive
        );
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
                size: if matches!(
                    kind,
                    DataKind::EnglishPhrase | DataKind::JapanesePhrase | DataKind::EnglishSentence | DataKind::JapaneseSentence
                ) {
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
                    half_life_days: self.config.analysis.half_life_days,
                },
            );
            let previous = db::load_adaptive_focus(&db_path, date.pred_opt().unwrap_or(date), &self.config.keyboard.layout).unwrap_or_default();
            let difficulty = db::load_adaptive_difficulty(&db_path).ok().flatten().unwrap_or(DifficultyState {
                pacer_multiplier: 1.0,
                drill_ratio: self.config.adaptive.drill_ratio,
            });
            let recent_accuracy = records
                .iter()
                .filter(|record| record.header.mode == "adaptive")
                .map(|record| record.header.accuracy)
                .collect::<Vec<_>>();
            let difficulty = adjust_difficulty(difficulty, &recent_accuracy, self.config.practice.auto_difficulty);
            let _ = db::save_adaptive_difficulty(&db_path, &difficulty);
            self.config.pacer.target_wpm = self.config.pacer.target_wpm.map(|target| target * difficulty.pacer_multiplier);
            let as_of = date.and_hms_opt(23, 59, 59).expect("valid end of day").and_utc();
            let due_reviews = db::load_review_schedules(&db_path, &self.config.keyboard.layout)
                .unwrap_or_default()
                .into_iter()
                .filter(|row| row.next_due <= as_of)
                .map(|row| {
                    let days = (date - row.last_reviewed_at.date_naive()).num_days().max(1);
                    (row.from, row.to, days)
                })
                .collect::<Vec<_>>();
            let prescriptions = db::load_prescriptions(&db_path).unwrap_or_default();
            let trends = prescription_trends(&prescriptions, as_of);
            let history = AdaptiveHistory {
                improving: trends.improving,
                stalled: trends.stalled,
                excluded_controls: trends.controls,
                due_reviews,
            };
            let corpus = if japanese {
                catalog
                    .words_ja()
                    .iter()
                    .map(|item| crate::engine::romaji::RomajiJudge::new(&item.reading).guide().to_owned())
                    .collect::<Vec<_>>()
            } else {
                catalog.words_en().to_vec()
            };
            let plan = build_plan_seeded(
                &details,
                &corpus,
                AdaptiveOptions {
                    error_weight_ms: self.config.adaptive.error_weight,
                    drill_ratio: difficulty.drill_ratio,
                    min_transition_types: 50,
                    min_transition_n: self.config.analysis.min_transition_n,
                    max_share: self.config.adaptive.max_share,
                },
                &previous,
                seed,
                &history,
            );
            self.adaptive_candidates = plan.transitions.clone();
            self.adaptive_focus = if plan.fallback_to_words {
                Vec::new()
            } else {
                plan.transitions.iter().take(5).cloned().collect()
            };
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
            generate_parallel_versions_for_series(crate::bench::BENCH_SERIES, run.part, &catalog, &practice_layout, &physical)
                [usize::from(run.parallel_index.clamp(1, crate::bench::PARALLEL_VERSION_COUNT) - 1)]
            .clone()
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
        let item_timeout = if matches!(self.config.mode.kind, ModeKind::Sentence | ModeKind::Phrase | ModeKind::Endurance) {
            Duration::ZERO
        } else {
            self.config.session.item_timeout()
        };
        let effective_error_mode = if !is_bench && self.config.session.blind {
            crate::config::ErrorMode::Advance
        } else {
            self.config.session.error_mode
        };
        let session_duration = if self.menu_active_step.is_some() {
            self.config.session.duration()
        } else if self.config.mode.kind == ModeKind::Endurance {
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
        self.ui_now = now;
        self.analysis_as_of = date;
        self.elapsed_total += dt;
        match self.screen {
            Screen::MenuFocus { remaining } => {
                let new_remaining = remaining - dt.as_secs_f32();
                if new_remaining <= 0.0 {
                    self.start_countdown();
                } else {
                    self.screen = Screen::MenuFocus { remaining: new_remaining };
                }
            }
            Screen::Countdown { remaining } => {
                let new_remaining = remaining - dt.as_secs_f32();
                if new_remaining <= 0.0 {
                    self.start_game(now, date);
                } else {
                    self.screen = Screen::Countdown { remaining: new_remaining };
                }
            }
            Screen::Playing => self.tick_playing(dt, now)?,
            Screen::Replay => {
                let elapsed = self
                    .history_replay_started
                    .map_or(Duration::ZERO, |started| now.saturating_duration_since(started));
                if let Some(replay) = self.history_replay.as_mut() {
                    replay.advance_to(elapsed);
                    if replay.is_finished() {
                        self.finish_history_replay();
                    }
                }
            }
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

    fn finish_game(&mut self, now: Instant) {
        let Some(game) = self.game.take() else { return };
        let started_wall = game.started_wall;
        let ghost_opponent = game.ghost.as_ref().map(|ghost| ghost.opponent().to_owned());
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
        record.set_ghost_opponent(ghost_opponent);
        let menu_context = self.menu_active_step.and_then(|index| {
            self.daily_menu
                .as_ref()
                .and_then(|menu| menu.steps.get(index))
                .cloned()
                .zip(self.menu_run_id)
                .map(|(step, run_id)| (index, step, run_id))
        });
        if let Some((index, step, run_id)) = &menu_context {
            record.set_menu_context(*run_id, &step.prescription_id, index + 1);
            // Provisional outcome from this session alone; replaced by the
            // stored judgment once the step is recorded below.
            self.menu_step_outcome = Some(
                step.advance_condition
                    .as_ref()
                    .map_or(crate::practice::menu::AdvanceOutcome::CannotDetermine, |condition| {
                        crate::practice::menu::evaluate_current_advance(condition, &record)
                    }),
            );
        }
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
                    half_life_days: self.config.analysis.half_life_days,
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
                record.header.id = Some(id);
                self.last_save_id = Some(id);
                self.last_save_error = None;
                if let Some(geometry) = crate::layout::geometry::geometry(&record.header.geometry) {
                    let layout = self.practice_layout().clone();
                    let _ = db::update_transition_model_cache(
                        &db_path,
                        &record,
                        &layout,
                        &geometry,
                        AnalysisSettings {
                            pause_ms: self.config.analysis.pause_ms,
                            shrinkage_k: self.config.analysis.shrinkage_k,
                            half_life_days: self.config.analysis.half_life_days,
                        },
                    );
                }
                if record.header.mode == "adaptive" {
                    let geometry = crate::layout::geometry::geometry(&record.header.geometry)
                        .or_else(|| crate::layout::geometry::geometry("ansi"))
                        .expect("bundled keyboard geometry");
                    let practice_layout = self.practice_layout().clone();
                    let _ = record_adaptive_state(AdaptiveRecordContext {
                        db_path: &db_path,
                        layout: &record.header.layout,
                        practice_layout: &practice_layout,
                        geometry: &geometry,
                        settings: AnalysisSettings {
                            pause_ms: self.config.analysis.pause_ms,
                            shrinkage_k: self.config.analysis.shrinkage_k,
                            half_life_days: self.config.analysis.half_life_days,
                        },
                        focus: &self.adaptive_focus,
                        candidates: &self.adaptive_candidates,
                        results: &self.adaptive_result,
                        finished_at: &record.header.finished_at,
                    });
                }
                // After R3 has evaluated this session, so a verdict closed by
                // this very session can be counted.
                if let Some((index, step, run_id)) = &menu_context
                    && (step.mode != "burst" || self.burst_series.complete())
                {
                    self.record_menu_step_completion(*run_id, *index, &record);
                }
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
        self.refresh_result_coaching(&record);
        self.refresh_recommendations();
        let result_records = db::load_records_filtered(&db_path, None, None).unwrap_or_else(|_| vec![record.clone()]);
        self.result_comparison = build_result_comparison(&result_records, &record);
        self.result_record = Some(record);
        self.last_result = Some(result);
        self.legacy_wpm = None;
        self.result_origin = ResultOrigin::JustPlayed;
        self.restore_menu_config();
        self.screen = Screen::Result;
        self.abort_confirm_until = None;
        self.begin_result_input_delay(now);
    }

    fn record_menu_step_completion(&mut self, run_id: i64, index: usize, record: &SessionRecord) {
        let db_path = self.db_path();
        let home = self.home_dir.clone();
        let config = self.config.clone();
        let layout = self.practice_layout().clone();
        let context = crate::practice::menu::progress::JudgeContext {
            db_path: &db_path,
            home: &home,
            config: &config,
            layout: &layout,
            as_of: self.analysis_as_of,
        };
        match crate::practice::menu::progress::record_step_completion(&context, run_id, index, record) {
            Ok(report) => {
                self.menu_completed.insert(index);
                if let Some(progress) = report.own.as_ref().and_then(|own| own.progress) {
                    self.menu_step_outcome = Some(progress.outcome);
                }
                self.menu_step_progress = report.own.as_ref().and_then(|own| own.progress);
                self.menu_step_reference = report.own.as_ref().and_then(|own| own.reference);
                self.menu_step_others = report.others;
                self.menu_step_deferral = report.deferred;
                // §8.1: once the run's check step is recorded, (re)write its
                // skill snapshots. They are derived from the keystroke logs
                // and can be rebuilt (`stats --rebuild-skills`), so a failure
                // is reported and the recorded step is kept.
                if let Err(error) = crate::practice::menu::skills::record_run_snapshots(&context, run_id) {
                    eprintln!(
                        "{}: failed to record skill snapshots of menu run {run_id}: {error}",
                        Msg::AppTitle.text(self.config.general.lang)
                    );
                }
            }
            Err(error) => self.last_save_error = Some(error.to_string()),
        }
    }

    fn clear_menu_step_result(&mut self) {
        self.menu_step_outcome = None;
        self.menu_step_progress = None;
        self.menu_step_reference = None;
        self.menu_step_others.clear();
        self.menu_step_deferral = None;
    }

    fn refresh_result_coaching(&mut self, record: &SessionRecord) {
        self.result_insights.clear();
        self.result_grade_line = None;
        let records = db::load_records_filtered(&self.db_path(), None, None).unwrap_or_else(|_| vec![record.clone()]);
        let prescriptions = db::load_prescriptions(&self.db_path()).unwrap_or_default();
        let selected_goal = db::load_selected_goal(&self.db_path()).unwrap_or_default();
        let Ok(report) = crate::analysis::coach::build(
            &records,
            &self.config,
            &self.home_dir,
            &prescriptions,
            selected_goal,
            self.analysis_as_of,
            self.config.general.lang,
        ) else {
            return;
        };
        if let Ok(session_report) = crate::analysis::coach::build(
            std::slice::from_ref(record),
            &self.config,
            &self.home_dir,
            &[],
            None,
            self.analysis_as_of,
            self.config.general.lang,
        ) {
            self.result_insights = crate::analysis::coach::session_insights(&session_report, record, self.config.general.lang);
        }
        if self.result_insights.len() < 3
            && let Some(insight) = crate::analysis::coach::form_insight(&report, record, self.config.general.lang)
        {
            self.result_insights.push(insight);
        }
        if let Some((_, part, _)) = record.header.bench_id.as_deref().and_then(parse_bench_id) {
            let language = if part == BenchPart::C { "ja" } else { "en" };
            self.result_grade_line = crate::analysis::coach::benchmark_grade_line(&report.grades, language, self.config.general.lang);
        }
    }
}

struct AdaptiveRecordContext<'a> {
    db_path: &'a std::path::Path,
    layout: &'a str,
    practice_layout: &'a KeyboardLayout,
    geometry: &'a crate::layout::geometry::KeyboardGeometry,
    settings: AnalysisSettings,
    focus: &'a [TransitionCost],
    candidates: &'a [TransitionCost],
    results: &'a [AdaptiveResultRow],
    finished_at: &'a str,
}

fn record_adaptive_state(context: AdaptiveRecordContext<'_>) -> AppResult<()> {
    let AdaptiveRecordContext {
        db_path,
        layout,
        practice_layout,
        geometry,
        settings,
        focus,
        candidates,
        results,
        finished_at,
    } = context;
    let at = chrono::DateTime::parse_from_rfc3339(finished_at)
        .map_err(|error| crate::error::AppError::Record(format!("invalid adaptive finish time: {error}")))?
        .with_timezone(&chrono::Utc);
    let mut prescriptions = db::load_prescriptions(db_path)?;
    let excluded = focus.iter().map(|row| (row.from.clone(), row.to.clone())).collect::<Vec<_>>();
    for target_row in focus
        .iter()
        .filter(|row| row.reason_kind != crate::practice::adaptive::FocusReason::Review)
    {
        let pool = candidates
            .iter()
            .filter(|row| row.from != target_row.from || row.to != target_row.to)
            .map(|row| ControlCandidate::new(&row.from, &row.to, &row.kind, row.frequency_quartile, row.estimated_iki_ms))
            .collect::<Vec<_>>();
        let controls = select_controls(
            &target_row.kind,
            target_row.frequency_quartile,
            target_row.estimated_iki_ms,
            &pool,
            &excluded,
        );
        let control_baselines = controls
            .iter()
            .filter_map(|control| {
                candidates
                    .iter()
                    .find(|row| row.from == control.from && row.to == control.to)
                    .map(transition_posterior)
            })
            .collect::<Vec<_>>();
        if control_baselines.len() != controls.len() || controls.is_empty() {
            continue;
        }
        let target = PrescriptionTarget {
            from: target_row.from.clone(),
            to: target_row.to.clone(),
            kind: target_row.kind.clone(),
            frequency_quartile: target_row.frequency_quartile,
        };
        let prescription = record_prescription_session(
            &prescriptions,
            at,
            layout,
            &target,
            &controls,
            &PrescriptionBaseline {
                target: transition_posterior(target_row),
                controls: control_baselines,
            },
        );
        let id = db::save_prescription(db_path, &prescription)?;
        let mut saved = prescription;
        saved.id = Some(id);
        if let Some(index) = prescriptions.iter().position(|row| row.id == Some(id)) {
            prescriptions[index] = saved;
        } else {
            prescriptions.push(saved);
        }
    }

    let all_records = db::load_records_filtered(db_path, None, None)?;
    for prescription in prescriptions.iter().filter(|row| row.status == "active") {
        let Ok(target) = serde_json::from_value::<PrescriptionTarget>(prescription.target.clone()) else {
            continue;
        };
        let Ok(controls) = serde_json::from_value::<Vec<ControlCandidate>>(prescription.control.clone()) else {
            continue;
        };
        let Ok(baseline) = serde_json::from_value::<PrescriptionBaseline>(prescription.baseline.clone()) else {
            continue;
        };
        if controls.len() != baseline.controls.len() {
            continue;
        }
        let evidence_records = real_word_records_since(&all_records, prescription.created_at);
        let details = analyze_records(&evidence_records, practice_layout, geometry, settings);
        let target_current = posterior_from_metrics(&details, &target.from, &target.to, baseline.target);
        let control_current = controls
            .iter()
            .zip(&baseline.controls)
            .map(|(control, fallback)| posterior_from_metrics(&details, &control.from, &control.to, *fallback))
            .collect::<Vec<_>>();
        if let Some(updated) = evaluate_stored_prescription(prescription, &Evidence::real_words(target_current, control_current), at) {
            db::save_prescription(db_path, &updated)?;
        }
    }

    let schedules = db::load_review_schedules(db_path, layout)?;
    for target in focus {
        let previous = schedules.iter().find(|row| row.from == target.from && row.to == target.to).cloned();
        let mastered = target.estimated_iki_ms <= target.target_iki_ms * 1.05;
        let session_iki = results
            .iter()
            .find(|row| row.from == target.from && row.to == target.to)
            .and_then(|row| row.session_iki_ms);
        let due_review = target.reason_kind == crate::practice::adaptive::FocusReason::Review;
        if previous.is_none() && mastered {
            db::save_review_schedule(db_path, &update_review(None, layout, &target.from, &target.to, at, true, false))?;
        } else if due_review && let Some(session_iki) = session_iki {
            let retained = session_iki <= target.target_iki_ms * 1.05;
            db::save_review_schedule(
                db_path,
                &update_review(previous, layout, &target.from, &target.to, at, retained, !retained),
            )?;
        }
    }
    Ok(())
}

fn transition_posterior(row: &TransitionCost) -> Posterior {
    Posterior {
        estimated_ms: row.estimated_iki_ms,
        log_variance: row.posterior_log_variance,
        effective_n: row.observation_n as f64,
    }
}

fn posterior_from_metrics(metrics: &DetailedMetrics, from: &str, to: &str, fallback: Posterior) -> Posterior {
    metrics.bigrams.iter().find(|row| row.from == from && row.to == to).map_or(
        Posterior {
            effective_n: 0.0,
            ..fallback
        },
        |row| Posterior {
            estimated_ms: row.estimated_iki_ms,
            log_variance: if row.estimated_iki_lo_ms > 0.0 && row.estimated_iki_hi_ms > row.estimated_iki_lo_ms {
                ((row.estimated_iki_hi_ms.ln() - row.estimated_iki_lo_ms.ln()) / (2.0 * 1.644_853_626_951_472_2)).powi(2)
            } else {
                fallback.log_variance
            },
            effective_n: row.n as f64,
        },
    )
}

const MODES: [ModeKind; 11] = [
    ModeKind::Random,
    ModeKind::Words,
    ModeKind::Romaji,
    ModeKind::Sentence,
    ModeKind::Phrase,
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
        ModeKind::Phrase if config.mode.phrase.lang == "ja" => Some(DataKind::JapanesePhrase),
        ModeKind::Phrase => Some(DataKind::EnglishPhrase),
        ModeKind::Ngram if config.mode.ngram.source == "corpus" => Some(DataKind::EnglishWord),
        ModeKind::Random | ModeKind::Ngram => None,
    }
}

fn daily_kind_name(kind: DataKind) -> &'static str {
    match kind {
        DataKind::EnglishWord => "word-en",
        DataKind::JapaneseWord => "word-ja",
        DataKind::EnglishPhrase => "phrase-en",
        DataKind::JapanesePhrase => "phrase-ja",
        DataKind::EnglishSentence => "sentence-en",
        DataKind::JapaneseSentence => "sentence-ja",
    }
}

fn data_item_key(kind: DataKind, text: &str, reading: Option<&str>) -> String {
    match kind {
        DataKind::EnglishWord => format!("en-word:{text}"),
        DataKind::EnglishPhrase => format!("en-phrase:{text}"),
        DataKind::EnglishSentence => format!("en-sentence:{text}"),
        DataKind::JapaneseWord => format!("ja-word:{text}\t{}", reading.unwrap_or_default()),
        DataKind::JapanesePhrase => format!("ja-phrase:{text}\t{}", reading.unwrap_or_default()),
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
        "phrase" => ModeKind::Phrase,
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
    let mut prompt = GeneratedPrompt::fixed(item.text.clone(), item.reading.clone(), target);
    if let Some(kind) = item.kind {
        prompt.kind = kind;
    }
    prompt
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

impl HistoryEntry {
    pub fn primary_summary(&self) -> &SessionSummary {
        match self {
            Self::Session(summary) | Self::BenchPart { summary, .. } => summary,
            Self::BenchRun { parts, .. } => parts.iter().find(|part| bench_part(part).as_deref() == Some("A")).unwrap_or(&parts[0]),
        }
    }

    pub fn finished_at(&self) -> chrono::DateTime<chrono::Local> {
        match self {
            Self::BenchRun { parts, .. } => parts[0].finished_at,
            _ => self.primary_summary().finished_at,
        }
    }

    pub fn wpm(&self) -> f64 {
        self.primary_summary().wpm
    }

    pub fn accuracy(&self) -> f64 {
        match self {
            Self::BenchRun { parts, .. } => parts.iter().map(|part| part.accuracy).sum::<f64>() / parts.len() as f64,
            _ => self.primary_summary().accuracy,
        }
    }

    pub fn duration(&self) -> Duration {
        match self {
            Self::BenchRun { parts, .. } => parts.iter().fold(Duration::ZERO, |total, part| total.saturating_add(part.duration)),
            _ => self.primary_summary().duration,
        }
    }

    pub fn score_eligible(&self) -> bool {
        self.primary_summary().score_eligible
    }
}

fn history_effective_mode(summary: &SessionSummary) -> String {
    if let Some(part) = bench_part(summary) {
        return match part.as_str() {
            "A" => "words",
            "B" => "ngram",
            "C" => "romaji",
            "D" => "random",
            _ => summary.mode.as_str(),
        }
        .to_owned();
    }
    summary.mode.clone()
}

pub fn history_language(summary: &SessionSummary) -> Option<&'static str> {
    if let Some(part) = bench_part(summary) {
        return match part.as_str() {
            "A" | "B" | "D" => Some("en"),
            "C" => Some("ja"),
            _ => None,
        };
    }
    match summary.mode.as_str() {
        "words" => Some("en"),
        "romaji" => Some("ja"),
        _ => serde_json::from_str::<serde_json::Value>(&summary.mode_params)
            .ok()
            .and_then(|value| value.get("lang").and_then(serde_json::Value::as_str).map(str::to_owned))
            .and_then(|language| match language.as_str() {
                "en" => Some("en"),
                "ja" => Some("ja"),
                _ => None,
            }),
    }
}

fn bench_identity(summary: &SessionSummary) -> Option<(String, String, String)> {
    let (series, part, form) = parse_bench_id(summary.bench_id.as_deref()?)?;
    let form_label = summary.bench_id.as_deref()?.rsplit('/').next()?.to_owned();
    debug_assert_eq!(form_label.parse::<u8>().ok(), Some(form));
    Some((series.to_owned(), form_label, part.as_str().to_owned()))
}

pub fn build_result_comparison(records: &[SessionRecord], current: &SessionRecord) -> ResultComparison {
    let mut matching = records
        .iter()
        .filter(|candidate| same_record_condition(candidate, current))
        .filter(|candidate| record_order(candidate) <= record_order(current))
        .collect::<Vec<_>>();
    if !matching.iter().any(|candidate| same_record_identity(candidate, current)) {
        matching.push(current);
    }
    matching.sort_by_key(|record| record_order(record));

    let previous = matching
        .iter()
        .copied()
        .rev()
        .find(|candidate| record_order(candidate) < record_order(current));
    let eligible = matching
        .iter()
        .copied()
        .filter(|record| record_score_eligible(record))
        .collect::<Vec<_>>();
    let prior_eligible = eligible
        .iter()
        .copied()
        .filter(|record| record_order(record) < record_order(current))
        .collect::<Vec<_>>();
    let best_wpm = eligible.iter().map(|record| record.header.net_wpm).max_by(f64::total_cmp);
    let best_cps = eligible.iter().map(|record| record.header.cps).max_by(f64::total_cmp);
    let best_accuracy = eligible.iter().map(|record| record.header.accuracy).max_by(f64::total_cmp);
    let prior_best_wpm = prior_eligible.iter().map(|record| record.header.net_wpm).max_by(f64::total_cmp);
    let prior_best_cps = prior_eligible.iter().map(|record| record.header.cps).max_by(f64::total_cmp);
    let prior_best_accuracy = prior_eligible.iter().map(|record| record.header.accuracy).max_by(f64::total_cmp);
    let recent_start = matching.len().saturating_sub(10);
    ResultComparison {
        previous_wpm_delta: previous.map(|record| current.header.net_wpm - record.header.net_wpm),
        previous_cps_delta: previous.map(|record| current.header.cps - record.header.cps),
        previous_accuracy_delta: previous.map(|record| current.header.accuracy - record.header.accuracy),
        best_wpm,
        best_cps,
        best_accuracy,
        best_wpm_delta: prior_best_wpm.map(|best| current.header.net_wpm - best),
        best_cps_delta: prior_best_cps.map(|best| current.header.cps - best),
        best_accuracy_delta: prior_best_accuracy.map(|best| current.header.accuracy - best),
        recent_wpm: matching[recent_start..].iter().map(|record| record.header.net_wpm).collect(),
    }
}

fn same_record_condition(left: &SessionRecord, right: &SessionRecord) -> bool {
    match (bench_record_identity(left), bench_record_identity(right)) {
        (Some((left_series, left_part)), Some((right_series, right_part))) => left_series == right_series && left_part == right_part,
        (None, None) => left.header.mode == right.header.mode,
        _ => false,
    }
}

fn bench_record_identity(record: &SessionRecord) -> Option<(&str, &str)> {
    let (series, part, _) = parse_bench_id(record.header.bench_id.as_deref()?)?;
    Some((series, part.as_str()))
}

fn same_record_identity(left: &SessionRecord, right: &SessionRecord) -> bool {
    match (left.header.id, right.header.id) {
        (Some(left), Some(right)) => left == right,
        _ => left.header.finished_at == right.header.finished_at && left.header.mode == right.header.mode,
    }
}

fn record_order(record: &SessionRecord) -> (chrono::DateTime<chrono::FixedOffset>, i64) {
    let timestamp = chrono::DateTime::parse_from_rfc3339(&record.header.finished_at).unwrap_or_else(|_| chrono::DateTime::UNIX_EPOCH.fixed_offset());
    (timestamp, record.header.id.unwrap_or(i64::MAX))
}

fn record_score_eligible(record: &SessionRecord) -> bool {
    serde_json::from_str::<serde_json::Value>(&record.header.mode_params)
        .ok()
        .and_then(|value| value.get("score_eligible").and_then(serde_json::Value::as_bool))
        .unwrap_or(true)
}

pub fn bench_part(summary: &SessionSummary) -> Option<String> {
    bench_identity(summary).map(|(_, _, part)| part)
}

#[derive(Clone, Copy)]
struct HistoryFilter<'a> {
    mode: Option<&'a str>,
    language: Option<&'a str>,
    period: AnalysisPeriod,
    today: chrono::NaiveDate,
}

fn summary_matches(summary: &SessionSummary, filter: HistoryFilter<'_>) -> bool {
    let mode_matches = filter.mode.is_none_or(|expected| history_effective_mode(summary) == expected);
    let language_matches = filter.language.is_none_or(|expected| history_language(summary) == Some(expected));
    let since = match filter.period {
        AnalysisPeriod::All => None,
        AnalysisPeriod::Days30 => Some(filter.today - chrono::Duration::days(29)),
        AnalysisPeriod::Days7 => Some(filter.today - chrono::Duration::days(6)),
    };
    let period_matches = since.is_none_or(|minimum| summary.finished_at.date_naive() >= minimum);
    mode_matches && language_matches && period_matches
}

fn build_history_rows(
    sessions: &[SessionSummary],
    kind: HistoryKindFilter,
    mode: Option<&str>,
    language: Option<&str>,
    period: AnalysisPeriod,
    today: chrono::NaiveDate,
) -> Vec<HistoryEntry> {
    let filter = HistoryFilter {
        mode,
        language,
        period,
        today,
    };
    let mut rows = Vec::new();
    if kind != HistoryKindFilter::Bench {
        rows.extend(
            sessions
                .iter()
                .filter(|summary| bench_identity(summary).is_none())
                .filter(|summary| summary_matches(summary, filter))
                .cloned()
                .map(HistoryEntry::Session),
        );
    }
    if kind == HistoryKindFilter::Practice {
        return rows;
    }

    let mut by_form: BTreeMap<(String, u8), Vec<SessionSummary>> = BTreeMap::new();
    for summary in sessions.iter().filter(|summary| bench_identity(summary).is_some()) {
        if let Some((series, form, _)) = bench_identity(summary) {
            let form_number = form.parse::<u8>().expect("parse_bench_id already validated the form");
            by_form.entry((series, form_number)).or_default().push(summary.clone());
        }
    }
    for ((series, _), mut candidates) in by_form {
        candidates.sort_by(|left, right| left.started_at.cmp(&right.started_at).then(left.id.cmp(&right.id)));
        let form = candidates
            .first()
            .and_then(bench_identity)
            .map_or_else(|| "0".to_owned(), |(_, form, _)| form);
        let mut run = Vec::new();
        for candidate in candidates {
            let starts_new = run.last().is_some_and(|previous: &SessionSummary| {
                candidate.started_at.signed_duration_since(previous.finished_at) > chrono::Duration::minutes(30)
            });
            if starts_new {
                push_bench_run(&mut rows, &series, &form, std::mem::take(&mut run), filter);
            }
            run.push(candidate);
        }
        push_bench_run(&mut rows, &series, &form, run, filter);
    }
    rows
}

fn push_bench_run(rows: &mut Vec<HistoryEntry>, series: &str, form: &str, parts: Vec<SessionSummary>, filter: HistoryFilter<'_>) {
    let id = parts.first().map(|part| part.id);
    let mut parts = parts.into_iter().filter(|summary| summary_matches(summary, filter)).collect::<Vec<_>>();
    parts.sort_by(|left, right| bench_part(left).cmp(&bench_part(right)).then(left.finished_at.cmp(&right.finished_at)));
    if let (Some(id), false) = (id, parts.is_empty()) {
        rows.push(HistoryEntry::BenchRun {
            id,
            series: series.to_owned(),
            form: form.to_owned(),
            parts,
        });
    }
}

fn sort_history_entries(items: &mut [HistoryEntry], sort: HistorySort) {
    use std::cmp::Ordering;
    match sort {
        HistorySort::DateDesc => items.sort_by(|a, b| {
            b.finished_at()
                .cmp(&a.finished_at())
                .then(b.primary_summary().id.cmp(&a.primary_summary().id))
        }),
        HistorySort::DateAsc => items.sort_by(|a, b| {
            a.finished_at()
                .cmp(&b.finished_at())
                .then(a.primary_summary().id.cmp(&b.primary_summary().id))
        }),
        HistorySort::WpmDesc => items.sort_by(|a, b| {
            b.wpm()
                .partial_cmp(&a.wpm())
                .unwrap_or(Ordering::Equal)
                .then(b.finished_at().cmp(&a.finished_at()))
        }),
        HistorySort::AccuracyDesc => items.sort_by(|a, b| {
            b.accuracy()
                .partial_cmp(&a.accuracy())
                .unwrap_or(Ordering::Equal)
                .then(b.finished_at().cmp(&a.finished_at()))
        }),
    }
}

fn rank_history_entries(items: &[HistoryEntry], enabled: bool, kind: HistoryKindFilter) -> Vec<Option<usize>> {
    if !enabled {
        return vec![None; items.len()];
    }
    let mut practice_rank = 0;
    let mut bench_ranks: HashMap<String, usize> = HashMap::new();
    items
        .iter()
        .map(|entry| {
            if !entry.score_eligible() {
                return None;
            }
            match (kind, entry) {
                (HistoryKindFilter::Practice, HistoryEntry::Session(_)) => {
                    practice_rank += 1;
                    Some(practice_rank)
                }
                (HistoryKindFilter::Bench, HistoryEntry::BenchRun { series, .. }) => {
                    let rank = bench_ranks.entry(series.clone()).or_default();
                    *rank += 1;
                    Some(*rank)
                }
                _ => None,
            }
        })
        .collect()
}

#[cfg(test)]
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
        let finished_at = Local::now() + ChronoDuration::seconds(secs_offset);
        SessionSummary {
            id,
            started_at: finished_at - ChronoDuration::seconds(60),
            finished_at,
            duration: Duration::from_secs(60),
            mode: "words".to_owned(),
            mode_params: "{}".to_owned(),
            bench_id: None,
            layout: "qwerty".to_owned(),
            input_layout: "same".to_owned(),
            error_mode: "stop".to_owned(),
            wpm,
            accuracy: acc,
            max_combo: combo,
            completed_words: words,
            metrics_version: 1,
            score_eligible: true,
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
    fn sort_cycle_visits_four_modes_once() {
        let mut s = HistorySort::DateDesc;
        let mut seen = vec![s];
        for _ in 0..3 {
            s = s.next();
            seen.push(s);
        }
        let last = s.next();
        assert_eq!(last, HistorySort::DateDesc);
        assert_eq!(seen.len(), 4);
    }
}
