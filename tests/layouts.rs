use std::collections::HashSet;

use rs_typing_trainer::layout::{builtin_layout, builtin_layout_names};

const TOP: [&str; 12] = [
    "KeyQ",
    "KeyW",
    "KeyE",
    "KeyR",
    "KeyT",
    "KeyY",
    "KeyU",
    "KeyI",
    "KeyO",
    "KeyP",
    "BracketLeft",
    "BracketRight",
];
const HOME: [&str; 11] = [
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
];
const BOTTOM: [&str; 10] = ["KeyZ", "KeyX", "KeyC", "KeyV", "KeyB", "KeyN", "KeyM", "Comma", "Period", "Slash"];

fn row(layout: &rs_typing_trainer::layout::KeyboardLayout, codes: &[&str]) -> String {
    codes.iter().map(|code| layout.key(code).unwrap().normal).collect()
}

#[test]
fn bundled_layouts_match_the_required_vectors() {
    let vectors = [
        ("qwerty", "qwertyuiop[]", "asdfghjkl;'", "zxcvbnm,./"),
        ("dvorak", "',.pyfgcrl/=", "aoeuidhtns-", ";qjkxbmwvz"),
        ("colemak", "qwfpgjluy;[]", "arstdhneio'", "zxcvbkm,./"),
        ("colemak-dh", "qwfpbjluy;[]", "arstgmneio'", "zxcdvkh,./"),
        // Official ANSI Angle(Z) placement relocates Z to the centre key.
        // Source: https://colemakmods.github.io/mod-dh/keyboards.html
        ("colemak-dh-ansi", "qwfpbjluy;[]", "arstgmneio'", "xcdvzkh,./"),
        ("workman", "qdrwbjfup;[]", "ashtgyneoi'", "zxmcvkl,./"),
    ];

    for (name, top, home, bottom) in vectors {
        let layout = builtin_layout(name).unwrap();
        assert_eq!(row(&layout, &TOP), top, "{name} top row");
        assert_eq!(row(&layout, &HOME), home, "{name} home row");
        assert_eq!(row(&layout, &BOTTOM), bottom, "{name} bottom row");
    }
}

#[test]
fn dvorak_and_jis_punctuation_match_the_required_vectors() {
    let dvorak = builtin_layout("dvorak").unwrap();
    let number_codes = [
        "Backquote",
        "Digit1",
        "Digit2",
        "Digit3",
        "Digit4",
        "Digit5",
        "Digit6",
        "Digit7",
        "Digit8",
        "Digit9",
        "Digit0",
        "Minus",
        "Equal",
    ];
    assert_eq!(row(&dvorak, &number_codes), "`1234567890[]");
    for (normal, shifted) in [
        ('\'', '"'),
        (',', '<'),
        ('.', '>'),
        (';', ':'),
        ('/', '?'),
        ('=', '+'),
        ('-', '_'),
        ('[', '{'),
        (']', '}'),
    ] {
        let stroke = dvorak.stroke_for(normal).unwrap();
        assert_eq!(dvorak.output(&stroke.code, true), Some(shifted));
    }

    let jis = builtin_layout("jis-qwerty").unwrap();
    assert_eq!(row(&jis, &TOP[..10]), "qwertyuiop");
    assert_eq!(jis.key("Digit2").unwrap().shift, '"');
    assert_eq!(jis.key("Digit7").unwrap().shift, '\'');
    assert_eq!(jis.key("BracketLeft").unwrap().normal, '@');
    assert_eq!(jis.key("Quote").unwrap().normal, ':');
    assert_eq!(jis.key("Equal").unwrap().normal, '^');
}

#[test]
fn every_bundled_layout_is_unique_roundtrips_and_contains_ascii_letters() {
    assert_eq!(
        builtin_layout_names().to_vec(),
        vec!["qwerty", "dvorak", "colemak", "colemak-dh", "colemak-dh-ansi", "workman", "jis-qwerty",]
    );

    for name in builtin_layout_names() {
        let layout = builtin_layout(name).unwrap();
        let mut seen = HashSet::new();
        for (code, pair) in layout.keys() {
            for (character, shift) in [(pair.normal, false), (pair.shift, true)] {
                if pair.normal != pair.shift {
                    assert!(seen.insert(character), "duplicate {character:?} in {name}");
                }
                let stroke = layout.stroke_for(character).unwrap();
                assert_eq!(stroke.code, *code);
                assert_eq!(layout.output(&stroke.code, stroke.shift), Some(character), "{name} {code} shift={shift}");
            }
        }
        for character in 'a'..='z' {
            assert!(layout.stroke_for(character).is_some(), "{name} lacks {character}");
        }
    }
}
