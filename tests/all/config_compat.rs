use std::path::Path;

use rs_typing_trainer::config::Config;
use rs_typing_trainer::i18n::Lang;
use tempfile::TempDir;

const V1_CONFIG: &str = include_str!("../fixtures/config_v1.toml");

#[test]
fn v1_config_remains_compatible_and_defaults_to_english() {
    let home = TempDir::new().unwrap();
    let config = Config::load_from(home.path(), Some(Path::new("tests/fixtures/config_v1.toml"))).unwrap();

    assert_eq!(config.charset_graphemes, ["b", "m", "w", "v", "z"]);
    assert_eq!(config.game.duration_seconds, 45);
    assert_eq!(config.general.lang, Lang::En);
    assert_eq!(config.keyboard.layout, "qwerty");
    assert_eq!(config.keyboard.input_layout, "same");
    assert_eq!(config.keyboard.geometry, "ansi");
    assert_eq!(config.keyboard.show, "full");
    assert_eq!(config.display.prompt_style, "large");
    assert_eq!(config.keyboard.heatmap, "off");
    assert!(config.keyboard.finger_guide);
    assert_eq!(config.keyboard.digit6_finger, "left-index");
}

#[test]
fn general_section_can_select_japanese() {
    let config = Config::from_toml(&format!("{V1_CONFIG}\n[general]\nlang = \"ja\"\n")).unwrap();

    assert_eq!(config.charset_graphemes, ["b", "m", "w", "v", "z"]);
    assert_eq!(config.game.duration_seconds, 45);
    assert_eq!(config.general.lang, Lang::Ja);
}

#[test]
fn digit_six_finger_can_be_overridden() {
    let config = Config::from_toml(&format!("{V1_CONFIG}\n[keyboard]\ndigit6_finger = \"right-index\"\n")).unwrap();

    assert_eq!(config.keyboard.digit6_finger, "right-index");
}
