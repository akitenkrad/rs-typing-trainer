use std::collections::BTreeSet;

use rs_typing_trainer::config::DrillSkill;
use rs_typing_trainer::practice::{briefing, menu};

#[test]
fn bundled_briefing_covers_every_skill_with_nonempty_bilingual_text() {
    let catalog = briefing::bundled().expect("bundled briefing parses");

    assert_eq!(catalog.skills().len(), DrillSkill::ALL.len());
    for skill in DrillSkill::ALL {
        let entry = catalog.skill(skill).unwrap_or_else(|| panic!("missing {skill:?}"));
        for text in [&entry.purpose, &entry.goal, &entry.content, &entry.missed, &entry.met] {
            assert!(!text.en.trim().is_empty(), "empty English text for {skill:?}");
            assert!(!text.ja.trim().is_empty(), "empty Japanese text for {skill:?}");
        }
        assert!(!entry.tips.is_empty(), "missing tips for {skill:?}");
        for tip in &entry.tips {
            assert!(!tip.en.trim().is_empty(), "empty English tip for {skill:?}");
            assert!(!tip.ja.trim().is_empty(), "empty Japanese tip for {skill:?}");
        }
    }
}

#[test]
fn bundled_briefing_step_ids_exist_in_the_menu_catalog_and_are_nonempty() {
    let briefing = briefing::bundled().expect("bundled briefing parses");
    let menu = menu::bundled_catalog().expect("bundled menu parses");
    let fixed_ids = menu.fixed_steps.iter().map(|step| step.id.as_str()).collect::<BTreeSet<_>>();

    for step in briefing.steps() {
        assert!(fixed_ids.contains(step.id.as_str()), "unknown fixed step {}", step.id);
        for text in [
            step.purpose.as_ref(),
            step.goal.as_ref(),
            step.content.as_ref(),
            step.missed.as_ref(),
            step.met.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            assert!(!text.en.trim().is_empty(), "empty English override for {}", step.id);
            assert!(!text.ja.trim().is_empty(), "empty Japanese override for {}", step.id);
        }
        for tip in step.tips.as_deref().unwrap_or_default() {
            assert!(!tip.en.trim().is_empty(), "empty English tip override for {}", step.id);
            assert!(!tip.ja.trim().is_empty(), "empty Japanese tip override for {}", step.id);
        }
    }
}
