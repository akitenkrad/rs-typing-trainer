use std::fs;

use rs_typing_trainer::config::Config;
use rs_typing_trainer::record::storage::{prepare_storage, storage_plan};
use tempfile::TempDir;

#[test]
fn storage_path_and_copy_rules() {
    let home = TempDir::new().unwrap();
    let default = Config::default();

    let plan = storage_plan(home.path(), &default);
    assert_eq!(plan.path, home.path().join(".local/share/rs-typing-trainer/sessions.db"));
    assert_eq!(default.resolve_db_path(home.path()), plan.path);
    assert!(!prepare_storage(&plan).unwrap());

    let old = home.path().join(".cache/rs-typing-trainer/sessions.db");
    fs::create_dir_all(old.parent().unwrap()).unwrap();
    fs::write(&old, b"legacy").unwrap();
    let plan = storage_plan(home.path(), &default);
    assert!(prepare_storage(&plan).unwrap());
    assert_eq!(fs::read(&plan.path).unwrap(), b"legacy");
    assert_eq!(fs::read(&old).unwrap(), b"legacy");

    fs::write(&old, b"old-stays").unwrap();
    fs::write(&plan.path, b"new-wins").unwrap();
    assert!(!prepare_storage(&plan).unwrap());
    assert_eq!(fs::read(&plan.path).unwrap(), b"new-wins");
    assert_eq!(fs::read(&old).unwrap(), b"old-stays");
}

#[test]
fn configured_storage_wins_and_disables_copy() {
    let home = TempDir::new().unwrap();
    let old = home.path().join(".cache/rs-typing-trainer/sessions.db");
    fs::create_dir_all(old.parent().unwrap()).unwrap();
    fs::write(&old, b"legacy").unwrap();

    let storage_only = Config::from_toml(
        r#"
[storage]
db_path = "/tmp/storage.db"
"#,
    )
    .unwrap();
    let plan = storage_plan(home.path(), &storage_only);
    assert_eq!(plan.path, std::path::PathBuf::from("/tmp/storage.db"));
    assert!(!plan.copy_legacy);
    assert!(!prepare_storage(&plan).unwrap());

    let both = Config::from_toml(
        r#"
[storage]
db_path = "/tmp/storage.db"
[metrics]
db_path = "/tmp/metrics.db"
"#,
    )
    .unwrap();
    assert_eq!(storage_plan(home.path(), &both).path, std::path::PathBuf::from("/tmp/storage.db"));
}
