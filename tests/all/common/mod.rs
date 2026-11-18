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
    TestApp {
        app: App::new(config, home.path().to_path_buf()),
        _home: home,
    }
}

pub fn test_app_at(mut config: Config, date: chrono::NaiveDate) -> TestApp {
    let home = TempDir::new().expect("temporary home");
    if config.storage.db_path.is_none() {
        config.storage.db_path = Some(home.path().join("data").join("sessions.db"));
    }
    TestApp {
        app: App::new_at(config, home.path().to_path_buf(), date),
        _home: home,
    }
}
