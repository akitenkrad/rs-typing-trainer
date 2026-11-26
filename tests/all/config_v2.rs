use std::fs;
use std::path::PathBuf;

use rs_typing_trainer::config::{BenchSeries, Config, ErrorMode, KeyFilter, ModeKind, save_mode_controls};
use rs_typing_trainer::graphics::GraphicsPreference;
use rs_typing_trainer::i18n::Lang;
use tempfile::TempDir;

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
[mode.phrase]
lang = "ja"
course = "mixed"
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
    assert_eq!(config.mode.phrase.lang, "ja");
    assert_eq!(config.mode.phrase.course, "mixed");
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
fn benchmark_series_defaults_to_v3_and_accepts_only_v2_or_v3() {
    assert_eq!(Config::default().bench.series, BenchSeries::V3);
    assert_eq!(Config::from_toml("").unwrap().bench.series, BenchSeries::V3);
    assert_eq!(Config::from_toml("[bench]\nseries = \"v2\"\n").unwrap().bench.series, BenchSeries::V2);
    assert_eq!(Config::from_toml("[bench]\nseries = \"v3\"\n").unwrap().bench.series, BenchSeries::V3);
    assert!(Config::from_toml("[bench]\nseries = \"v4\"\n").is_err());
}

#[test]
fn menu_languages_default_to_the_display_language_and_accept_one_or_two_languages() {
    let english = Config::from_toml("[general]\nlang = \"en\"\n").unwrap();
    assert_eq!(english.menu.practice_languages(english.general.lang), vec![Lang::En]);

    let japanese = Config::from_toml("[general]\nlang = \"ja\"\n").unwrap();
    assert_eq!(japanese.menu.practice_languages(japanese.general.lang), vec![Lang::Ja]);

    let one = Config::from_toml("[menu]\nlanguages = [\"ja\"]\n").unwrap();
    assert_eq!(one.menu.practice_languages(Lang::En), vec![Lang::Ja]);

    let two = Config::from_toml("[menu]\nlanguages = [\"en\", \"ja\"]\n").unwrap();
    assert_eq!(two.menu.practice_languages(Lang::Ja), vec![Lang::En, Lang::Ja]);
}

#[test]
fn menu_languages_reject_empty_duplicate_unknown_and_more_than_two_values() {
    for raw in [
        "[menu]\nlanguages = []\n",
        "[menu]\nlanguages = [\"en\", \"en\"]\n",
        "[menu]\nlanguages = [\"fr\"]\n",
        "[menu]\nlanguages = [\"en\", \"ja\", \"en\"]\n",
    ] {
        assert!(Config::from_toml(raw).is_err(), "accepted invalid config: {raw}");
    }
}

#[test]
fn phrase_mode_defaults_and_validation_are_stable() {
    let config = Config::default();
    assert_eq!(config.mode.phrase.lang, "en");
    assert_eq!(config.mode.phrase.course, "normal");
    assert!(Config::from_toml("[mode.phrase]\nlang = \"ja\"\ncourse = \"long\"\n").is_ok());
    assert!(Config::from_toml("[mode.phrase]\nlang = \"fr\"\n").is_err());
    assert!(Config::from_toml("[mode.phrase]\ncourse = \"tiny\"\n").is_err());
}

#[test]
fn phrase_mode_controls_are_saved_without_replacing_other_mode_settings() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("config.toml");
    fs::write(&path, "[mode]\nkeys = \"abc\" # keep\n").unwrap();
    let mut config = Config::from_path(&path).unwrap();
    config.mode.phrase.lang = "ja".into();
    config.mode.phrase.course = "long".into();

    save_mode_controls(&path, &config).unwrap();

    let saved = fs::read_to_string(&path).unwrap();
    assert!(saved.contains("keys = \"abc\" # keep"), "{saved}");
    let reloaded = Config::from_path(&path).unwrap();
    assert_eq!(reloaded.mode.phrase.lang, "ja");
    assert_eq!(reloaded.mode.phrase.course, "long");
}

#[test]
fn result_input_delay_defaults_to_1500_and_is_clamped_to_10000_ms() {
    assert_eq!(Config::default().session.result_input_delay_ms, 1_500);
    assert_eq!(
        Config::from_toml("[session]\nresult_input_delay_ms = 20000\n")
            .unwrap()
            .session
            .result_input_delay_ms,
        10_000
    );
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
fn big_font_defaults_to_biz_ud_and_accepts_builtins_or_a_path() {
    assert_eq!(Config::default().display.big_font, "biz-ud-gothic");
    assert_eq!(Config::from_toml("[display]\n").unwrap().display.big_font, "biz-ud-gothic");

    for value in ["biz-ud-gothic", "shinonome", "~/Fonts/my font.ttc", "/tmp/custom.otf"] {
        let config = Config::from_toml(&format!("[display]\nbig_font = {value:?}\n")).unwrap();
        assert_eq!(config.display.big_font, value);
    }
}

#[test]
fn v1_fixture_is_mapped_to_v2() {
    let config = Config::from_toml(include_str!("../fixtures/config_v1.toml")).unwrap();

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
fn legacy_session_values_fill_missing_keys_in_a_partial_session_section() {
    let config = Config::from_toml(
        r#"
[game]
duration_seconds = 6
word_timeout_seconds = 3
word_length = 3
[session]
preview_items = 1
"#,
    )
    .unwrap();

    assert_eq!(config.session.duration_seconds, 6);
    assert_eq!(config.session.item_timeout_seconds, 3);
    assert_eq!(config.game.duration_seconds, 6);
    assert_eq!(config.game.word_timeout_seconds, 3);
}

#[test]
fn explicit_session_values_win_per_key_over_legacy_game_values() {
    let config = Config::from_toml(
        r#"
[game]
duration_seconds = 6
word_timeout_seconds = 3
word_length = 3
[session]
duration_seconds = 30
preview_items = 1
"#,
    )
    .unwrap();

    assert_eq!(config.session.duration_seconds, 30);
    assert_eq!(config.session.item_timeout_seconds, 3);
    assert_eq!(config.game.duration_seconds, 30);
    assert_eq!(config.game.word_timeout_seconds, 3);
}

#[test]
fn legacy_session_values_are_used_without_a_session_section() {
    let config = Config::from_toml(
        r#"
[game]
duration_seconds = 6
word_timeout_seconds = 3
word_length = 3
"#,
    )
    .unwrap();

    assert_eq!(config.session.duration_seconds, 6);
    assert_eq!(config.session.item_timeout_seconds, 3);
}

#[test]
fn session_timing_defaults_when_neither_new_nor_legacy_keys_are_present() {
    let config = Config::from_toml("[session]\npreview_items = 1\n").unwrap();

    assert_eq!(config.session.duration_seconds, 60);
    assert_eq!(config.session.item_timeout_seconds, 5);
}

#[test]
fn legacy_duration_survives_mode_controls_round_trip() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("config.toml");
    fs::write(&path, "[game]\nduration_seconds = 6\n").unwrap();
    let config = Config::from_path(&path).unwrap();

    save_mode_controls(&path, &config).unwrap();

    let saved = fs::read_to_string(&path).unwrap();
    assert!(saved.contains("duration_seconds = 6"), "{saved}");
    let reloaded = Config::from_path(&path).unwrap();
    assert_eq!(reloaded.session.duration_seconds, 6);
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

/// MYTASK-3623: `[menu]` is part of the written configuration. Unset practice
/// languages are not written, so they read back as "follow the display
/// language"; set ones read back unchanged.
#[test]
fn menu_section_is_written_and_reads_back_to_the_same_settings() {
    let default = toml::to_string(&Config::default()).unwrap();
    assert!(default.contains("[menu]\nminutes = 15\n"), "{default}");
    assert!(!default.contains("languages"), "{default}");
    let read = Config::from_toml(&default).unwrap();
    assert_eq!(read.menu, Config::default().menu);
    assert_eq!(read.menu.languages, None);
    assert_eq!(read.menu.practice_languages(Lang::Ja), vec![Lang::Ja], "follows the display language");

    for languages in [vec![Lang::Ja], vec![Lang::En, Lang::Ja]] {
        let mut config = Config::default();
        config.menu.minutes = 20;
        config.menu.languages = Some(languages.clone());
        let written = toml::to_string(&config).unwrap();
        let read = Config::from_toml(&written).unwrap();
        assert_eq!(read.menu, config.menu, "{written}");
        // The legacy v1 alias `characters.charset` is derived on read, so only
        // the `[menu]` table is compared on the second round trip.
        let menu_table = |raw: &str| raw.parse::<toml::Table>().unwrap()["menu"].clone();
        assert_eq!(
            menu_table(&toml::to_string(&read).unwrap()),
            menu_table(&written),
            "a second round trip is stable"
        );
    }
}

#[test]
fn saving_settings_with_practice_languages_writes_a_menu_section_to_a_new_file() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    let mut config = Config::default();
    config.menu.languages = Some(vec![Lang::Ja]);
    rs_typing_trainer::config::save_settings(&path, &config).unwrap();

    let written = fs::read_to_string(&path).unwrap();
    assert!(written.contains("[menu]"), "{written}");
    assert!(written.contains("languages = [\"ja\"]"), "{written}");
    assert_eq!(Config::from_path(&path).unwrap().menu.languages, Some(vec![Lang::Ja]));
}
