use proptest::prelude::*;
use rs_typing_trainer::engine::judge::{Judge, JudgeResult};
use rs_typing_trainer::engine::romaji::RomajiJudge;

fn mora() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec![
        "あ", "い", "う", "え", "お", "か", "き", "く", "け", "こ", "し", "ち", "つ", "ふ", "じ", "ん", "っ", "きゃ", "しゃ", "ちゃ", "じゃ", "てぃ",
        "ふぁ", "しぇ", "ー", "、", "。",
    ])
}

proptest! {
    #[test]
    fn representative_guide_is_always_accepted(parts in prop::collection::vec(mora(), 1..12)) {
        let reading = parts.concat();
        let mut judge = RomajiJudge::new(&reading);
        let guide = judge.guide().to_owned();
        for character in guide.chars() {
            prop_assert_ne!(judge.input(&character.to_string()), JudgeResult::Mistype);
        }
        prop_assert!(judge.is_complete(), "reading={reading:?}, guide={guide:?}");
    }

    #[test]
    fn accepted_spellings_never_lose_all_candidates(parts in prop::collection::vec(mora(), 1..7)) {
        let reading = parts.concat();
        let spellings = RomajiJudge::new(&reading).accepted_spellings(128);
        prop_assert!(!spellings.is_empty());
        for spelling in spellings {
            prop_assert!(RomajiJudge::new(&reading).spelling_segments(&spelling).is_some());
            let mut judge = RomajiJudge::new(&reading);
            for character in spelling.chars() {
                prop_assert_ne!(
                    judge.input(&character.to_string()),
                    JudgeResult::Mistype,
                    "reading={:?}, spelling={:?}",
                    reading,
                    spelling
                );
                prop_assert!(!judge.expected_candidates().is_empty() || judge.is_complete());
            }
            prop_assert!(judge.is_complete());
        }
    }
}
