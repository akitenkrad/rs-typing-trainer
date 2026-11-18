#![allow(dead_code)]

use std::ops::{Deref, DerefMut};

use rs_typing_trainer::app::App;
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
