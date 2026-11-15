use std::path::PathBuf;

use rs_typing_trainer::config::{Config, ErrorMode, KeyFilter, ModeKind};
use rs_typing_trainer::graphics::GraphicsPreference;

#[test]
fn reads_all_v2_training_sections() {
    let config = Config::from_toml(
        r#"
[general]
lang = "ja"
[display]
prompt_size = "xlarge"
graphics = "sixel"
[keyboard]
layout = "colemak-dh-ansi"
input_layout = "qwerty"
geometry = "ansi"
show = "dim"
heatmap = "errors"
[session]
duration_seconds = 90
item_timeout_seconds = 0
error_mode = "advance"
preview_items = 2
[mode]
kind = "romaji"
keys = "ch"
key_filter = "only"
base_keys = "home"
[mode.random]
word_length = 4
allow_repeat = false
allow_consecutive_same_word = true
[mode.ngram]
n = 3
source = "corpus"
[mode.romaji]
accept_apostrophe = false
accept_xn = true
[mode.sentence]
lang = "ja"
[data]
words_en = "/tmp/test-words.tsv"
words_ja = "/tmp/test-ja.tsv"
sentences_en = "/tmp/test-en-sentences.tsv"
sentences_ja = "/tmp/test-ja-sentences.tsv"
[practice]
daily_rotation = false
core_ratio = 0.7
weekly_theme = false
auto_difficulty = false
[analysis]
pause_ms = 1750
shrinkage_k = 12.5
half_life_days = 21.0
min_transition_n = 7
spelling_suggestions = false
[adaptive]
error_weight = 420.0
drill_ratio = 0.25
[pacer]
enabled = true
ratio = 1.08
[unlock]
order = "rows"
target_wpm = 40.0
[gate]
min_accuracy = 0.95
[ui]
animation_enabled = false
animation_speed = 2.0
theme = "mono"
[storage]
db_path = "/tmp/test-sessions.db"
"#,
    )
    .unwrap();

    assert_eq!(config.session.duration_seconds, 90);
    assert_eq!(config.display.prompt_style, "large", "legacy prompt_size = \"xlarge\" reads as large");
    assert_eq!(config.display.graphics, GraphicsPreference::Sixel);
    assert_eq!(config.session.item_timeout_seconds, 0);
    assert_eq!(config.session.error_mode, ErrorMode::Advance);
    assert_eq!(config.session.preview_items, 2);
    assert_eq!(config.mode.kind, ModeKind::Romaji);
    assert_eq!(config.mode.key_filter, KeyFilter::Only);
    assert_eq!(config.mode.random.word_length, 4);
    assert_eq!(config.mode.ngram.n, 3);
    assert_eq!(config.mode.ngram.source, "corpus");
    assert!(!config.mode.romaji.accept_apostrophe);
    assert!(config.mode.romaji.accept_xn);
    assert_eq!(config.mode.sentence.lang, "ja");
    assert_eq!(config.data.words_ja, Some(PathBuf::from("/tmp/test-ja.tsv")));
    assert!(!config.practice.daily_rotation);
    assert_eq!(config.practice.core_ratio, 0.7);
    assert!(!config.practice.weekly_theme);
    assert!(!config.practice.auto_difficulty);
    assert_eq!(config.analysis.pause_ms, 1_750);
    assert_eq!(config.analysis.shrinkage_k, 12.5);
    assert_eq!(config.analysis.half_life_days, 21.0);
    assert_eq!(config.analysis.min_transition_n, 7);
    assert!(!config.analysis.spelling_suggestions);
    assert_eq!(config.adaptive.error_weight, Some(420.0));
    assert_eq!(config.adaptive.drill_ratio, 0.25);
    assert!(config.pacer.enabled);
    assert_eq!(config.pacer.ratio, 1.08);
    assert_eq!(config.unlock.order, rs_typing_trainer::config::UnlockOrder::Rows);
    assert_eq!(config.unlock.target_wpm, 40.0);
    assert_eq!(config.gate.min_accuracy, 0.95);
    assert_eq!(config.storage.db_path, Some(PathBuf::from("/tmp/test-sessions.db")));
}

#[test]
fn auto_difficulty_defaults_to_enabled() {
    assert!(Config::default().practice.auto_difficulty);
}

#[test]
fn graphics_defaults_to_auto_and_round_trips_all_values() {
    assert_eq!(Config::default().display.graphics, GraphicsPreference::Auto);

    for (value, expected) in [
        ("auto", GraphicsPreference::Auto),
        ("kitty", GraphicsPreference::Kitty),
        ("sixel", GraphicsPreference::Sixel),
        ("iterm2", GraphicsPreference::Iterm2),
        ("off", GraphicsPreference::Off),
    ] {
        let config = Config::from_toml(&format!("[display]\ngraphics = \"{value}\"\n")).unwrap();
        assert_eq!(config.display.graphics, expected, "{value}");
    }

    assert!(Config::from_toml("[display]\ngraphics = \"unknown\"\n").is_err());
}

#[test]
fn v1_fixture_is_mapped_to_v2() {
    let config = Config::from_toml(include_str!("fixtures/config_v1.toml")).unwrap();

    assert_eq!(config.session.duration_seconds, 45);
    assert_eq!(config.session.item_timeout_seconds, 5);
    assert_eq!(config.session.error_mode, ErrorMode::Stop);
    assert_eq!(config.mode.kind, ModeKind::Random);
    assert_eq!(config.mode.keys, "bmwvz");
    assert_eq!(config.mode.random.word_length, 3);
    assert!(config.mode.random.allow_repeat);
    assert!(!config.mode.random.allow_consecutive_same_word);
    assert_eq!(config.analysis.min_transition_n, 5);
    assert!(config.analysis.spelling_suggestions);
}

#[test]
fn analysis_half_life_must_be_positive() {
    assert!(Config::from_toml("[analysis]\nhalf_life_days = 0.0\n").is_err());
}

#[test]
fn v2_sections_win_when_v1_and_v2_coexist() {
    let config = Config::from_toml(
        r#"
[game]
duration_seconds = 12
word_timeout_seconds = 9
word_length = 8
[characters]
charset = "xyz"
allow_repeat = false
allow_consecutive_same_word = true
[session]
duration_seconds = 60
item_timeout_seconds = 0
[mode]
kind = "words"
keys = "abc"
[mode.random]
word_length = 5
allow_repeat = true
allow_consecutive_same_word = false
"#,
    )
    .unwrap();

    assert_eq!(config.session.duration_seconds, 60);
    assert_eq!(config.session.item_timeout_seconds, 0);
    assert_eq!(config.mode.kind, ModeKind::Words);
    assert_eq!(config.mode.keys, "abc");
    assert_eq!(config.mode.random.word_length, 5);
}

#[test]
fn legacy_metrics_path_maps_to_storage_but_v2_storage_wins() {
    let legacy = Config::from_toml("[metrics]\ndb_path = \"/tmp/legacy.db\"\n").unwrap();
    assert_eq!(legacy.storage.db_path, Some(PathBuf::from("/tmp/legacy.db")));

    let mixed = Config::from_toml("[metrics]\ndb_path = \"/tmp/legacy.db\"\n[storage]\ndb_path = \"/tmp/v2.db\"\n").unwrap();
    assert_eq!(mixed.storage.db_path, Some(PathBuf::from("/tmp/v2.db")));
}

#[test]
fn empty_keys_are_valid_for_data_backed_modes() {
    for kind in ["words", "romaji", "sentence"] {
        let config = Config::from_toml(&format!(
            r#"
[mode]
kind = "{kind}"
keys = ""
"#,
        ))
        .unwrap();

        assert!(config.mode.keys.is_empty(), "{kind}");
        assert!(config.charset_graphemes.is_empty(), "{kind}");
    }
}

#[test]
fn empty_keys_do_not_make_random_config_fail_to_load() {
    let config = Config::from_toml(
        r#"
[mode]
kind = "random"
keys = ""
"#,
    )
    .unwrap();

    assert_eq!(config.mode.kind, ModeKind::Random);
    assert!(config.mode.keys.is_empty());
}
