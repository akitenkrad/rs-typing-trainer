use rs_typing_trainer::engine::judge::{Judge, JudgeResult};
use rs_typing_trainer::engine::romaji::RomajiJudge;

fn accepts(kana: &str, input: &str) -> bool {
    let mut judge = RomajiJudge::new(kana);
    let mut completed = false;
    for character in input.chars() {
        match judge.input(&character.to_string()) {
            JudgeResult::Mistype => return false,
            JudgeResult::ItemCompleted => completed = true,
            JudgeResult::Correct => {}
        }
    }
    completed && judge.is_complete()
}

#[test]
fn accepts_and_rejects_context_sensitive_examples() {
    for (kana, accepted, rejected) in [
        ("しんや", vec!["shinnya", "sinnya", "shin'ya", "shixnya"], vec!["shinya"]),
        ("かんい", vec!["kanni", "kan'i"], vec!["kani"]),
        ("さんぽ", vec!["sanpo", "sannpo"], vec![]),
        ("ほん", vec!["honn", "hon'", "honxn"], vec!["hon"]),
        ("きっと", vec!["kitto", "kiltuto", "kixtuto", "kiltsuto"], vec!["kito"]),
        ("あっ", vec!["altu", "axtu", "altsu", "axtsu"], vec!["a"]),
        ("しゃしん", vec!["shashinn", "syasinn", "shashin'"], vec!["shashin"]),
        ("ちず", vec!["chizu", "tizu"], vec![]),
        ("つくえ", vec!["tsukue", "tukue"], vec![]),
        ("ふじ", vec!["fuji", "huzi"], vec![]),
        ("てぃーしゃつ", vec!["thi-shatsu", "texi-syatu"], vec![]),
    ] {
        for input in accepted {
            assert!(accepts(kana, input), "{kana:?} should accept {input:?}");
        }
        for input in rejected {
            assert!(!accepts(kana, input), "{kana:?} should reject {input:?}");
        }
    }
}

#[test]
fn n_before_punctuation_requires_an_explicit_n_spelling() {
    // A punctuation mark is not a following consonant mora. Therefore the
    // single `n` cannot commit ん; this follows the design's stated boundary.
    assert!(accepts("こうえん、", "kouenn,"));
    assert!(accepts("こうえん、", "kouen',"));
    assert!(!accepts("こうえん、", "kouen,"));
}

#[test]
fn guide_tracks_the_spelling_selected_by_the_user() {
    let mut judge = RomajiJudge::new("し");
    assert_eq!(judge.guide(), "shi");
    assert_eq!(judge.input("s"), JudgeResult::Correct);
    assert_eq!(judge.guide(), "shi");
    assert_eq!(judge.input("i"), JudgeResult::ItemCompleted);
    assert_eq!(judge.guide(), "si");
}

#[test]
fn representative_guides_cover_hiragana_classes_and_are_accepted() {
    let readings = [
        "あいうえお",
        "かきくけこがぎぐげご",
        "さしすせそざじずぜぞ",
        "たちつてとだぢづでど",
        "なにぬねの",
        "はひふへほばびぶべぼぱぴぷぺぽ",
        "まみむめもやゆよらりるれろわをゔ",
        "ぁぃぅぇぉゃゅょゎ",
        "きゃきゅきょしゃしゅしょちゃちゅちょ",
        "にゃにゅにょひゃひゅひょみゃみゅみょりゃりゅりょ",
        "ぎゃぎゅぎょじゃじゅじょびゃびゅびょぴゃぴゅぴょ",
        "てぃでぃふぁふぃふぇふぉしぇじぇちぇ",
        "きっとあっほん",
        "ー、。？！",
        "abc123!?",
    ];

    for reading in readings {
        let guide = RomajiJudge::new(reading).guide().to_owned();
        assert!(accepts(reading, &guide), "guide {guide:?} for {reading:?}");
    }
}
