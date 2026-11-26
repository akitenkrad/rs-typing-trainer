#![allow(dead_code)]

use std::ops::{Deref, DerefMut};
use std::time::Instant;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rs_typing_trainer::app::{App, Screen};
use rs_typing_trainer::config::Config;
use tempfile::TempDir;

pub struct TestApp {
    app: App,
    _home: TempDir,
}

impl Deref for TestApp {
    type Target = App;

    fn deref(&self) -> &Self::Target {
        &self.app
    }
}

impl DerefMut for TestApp {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.app
    }
}

pub fn test_app(mut config: Config) -> TestApp {
    let home = TempDir::new().expect("temporary home");
    if config.storage.db_path.is_none() {
        config.storage.db_path = Some(home.path().join("data").join("sessions.db"));
    }
    let mut app = App::new(config, home.path().to_path_buf());
    // The title cursor starts on «Today's training». Most tests begin
    // individual training from the title with Enter, so they start one row
    // lower; `title_ui` checks the real starting row with `App::new` directly.
    app.title_selected = 1;
    TestApp { app, _home: home }
}

pub fn test_app_at(mut config: Config, date: chrono::NaiveDate) -> TestApp {
    let home = TempDir::new().expect("temporary home");
    if config.storage.db_path.is_none() {
        config.storage.db_path = Some(home.path().join("data").join("sessions.db"));
    }
    let mut app = App::new_at(config, home.path().to_path_buf(), date);
    // The title cursor starts on «Today's training». Most tests begin
    // individual training from the title with Enter, so they start one row
    // lower; `title_ui` checks the real starting row with `App::new` directly.
    app.title_selected = 1;
    TestApp { app, _home: home }
}

/// Accepts the pre-session briefing when a test is interested in the session
/// that follows it rather than the briefing itself.
pub fn accept_briefing(app: &mut App, now: Instant) {
    if app.screen == Screen::Briefing {
        app.handle_key_at(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), now);
    }
}

/// The phrase that marks the metrics row of the playing screen: the combo of
/// the ordinary sub stats, or — when the session has a live goal
/// (MYTASK-3641) — the goal's note that takes that row, as it reads at the
/// start of a session.
pub fn metrics_phrase(app: &App, lang: rs_typing_trainer::i18n::Lang) -> &'static str {
    use rs_typing_trainer::i18n::Lang;
    use rs_typing_trainer::practice::menu::live_goal::GoalKind;

    let Some(state) = app.menu_live_goal.as_ref() else {
        return if lang == Lang::Ja { "連続正解" } else { "Combo" };
    };
    match (state.goal.kind, lang) {
        (GoalKind::EnduranceSplit, Lang::En) => "Second half in",
        (GoalKind::EnduranceSplit, Lang::Ja) => "後半まで",
        (GoalKind::Sprint, Lang::En) => "Words 0 /",
        (GoalKind::Sprint, Lang::Ja) => "語 0 /",
        (GoalKind::Metronome, Lang::En) => "Recent intervals",
        (GoalKind::Metronome, Lang::Ja) => "直近の間隔",
        // A fresh database has no focus for the adaptive plan.
        (GoalKind::WeakTransition, Lang::En) => "No target transitions",
        (GoalKind::WeakTransition, Lang::Ja) => "対象の遷移がありません",
        (GoalKind::Reach, Lang::En) => "Remote rows",
        (GoalKind::Reach, Lang::Ja) => "遠い段（",
        (kind, _) => panic!("no metrics phrase for {kind:?}"),
    }
}
