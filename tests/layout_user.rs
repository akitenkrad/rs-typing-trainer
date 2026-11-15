use std::fs;

use rs_typing_trainer::layout::load_user_layouts;
use tempfile::TempDir;

#[test]
fn user_layouts_are_loaded_from_the_supplied_config_directory() {
    let dir = TempDir::new().unwrap();
    let layouts = dir.path().join("layouts");
    fs::create_dir(&layouts).unwrap();
    fs::write(
        layouts.join("tiny.toml"),
        r#"
name = "tiny"

[keys.KeyA]
normal = "x"
shift = "X"

[keys.KeyB]
normal = "y"
shift = "Y"
"#,
    )
    .unwrap();

    let loaded = load_user_layouts(dir.path()).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name(), "tiny");
    assert_eq!(loaded[0].output("KeyA", false), Some('x'));
    assert_eq!(loaded[0].stroke_for('Y').unwrap().code, "KeyB");
}

#[test]
fn duplicate_character_error_names_the_file_and_character() {
    let dir = TempDir::new().unwrap();
    let layouts = dir.path().join("layouts");
    fs::create_dir(&layouts).unwrap();
    fs::write(
        layouts.join("duplicate.toml"),
        r#"
name = "duplicate"

[keys.KeyA]
normal = "x"
shift = "X"

[keys.KeyB]
normal = "x"
shift = "Y"
"#,
    )
    .unwrap();

    let error = load_user_layouts(dir.path()).unwrap_err().to_string();
    assert!(error.contains("duplicate.toml"), "{error}");
    assert!(error.contains('x'), "{error}");
}
