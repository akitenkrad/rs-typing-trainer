use rs_typing_trainer::layout::{builtin_layout, remap_char};

#[test]
fn qwerty_input_is_remapped_to_dvorak() {
    let qwerty = builtin_layout("qwerty").unwrap();
    let dvorak = builtin_layout("dvorak").unwrap();

    for (typed, expected) in [('d', 'e'), ('s', 'o'), ('D', 'E'), ('q', '\'')] {
        assert_eq!(remap_char(typed, Some(&qwerty), &dvorak), expected);
    }
}

#[test]
fn dvorak_input_is_remapped_to_qwerty() {
    let qwerty = builtin_layout("qwerty").unwrap();
    let dvorak = builtin_layout("dvorak").unwrap();
    assert_eq!(remap_char('e', Some(&dvorak), &qwerty), 'd');
}

#[test]
fn same_and_unknown_characters_pass_through() {
    let dvorak = builtin_layout("dvorak").unwrap();
    assert_eq!(remap_char('d', None, &dvorak), 'd');
    assert_eq!(remap_char('🍎', Some(&dvorak), &dvorak), '🍎');
}
