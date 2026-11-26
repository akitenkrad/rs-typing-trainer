//! Bundled bilingual explanations shown before and after drills.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::config::DrillSkill;
use crate::practice::menu::LocalizedText;

const BRIEFING_VERSION: &str = "briefing-v1";
const BRIEFING_DATA: &str = include_str!("../../data/drills/briefing-v1.toml");

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillBriefing {
    pub skill: DrillSkill,
    pub purpose: LocalizedText,
    pub goal: LocalizedText,
    pub content: LocalizedText,
    pub tips: Vec<LocalizedText>,
    pub missed: LocalizedText,
    pub met: LocalizedText,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepBriefing {
    pub id: String,
    #[serde(default)]
    pub purpose: Option<LocalizedText>,
    #[serde(default)]
    pub goal: Option<LocalizedText>,
    #[serde(default)]
    pub content: Option<LocalizedText>,
    #[serde(default)]
    pub tips: Option<Vec<LocalizedText>>,
    #[serde(default)]
    pub missed: Option<LocalizedText>,
    #[serde(default)]
    pub met: Option<LocalizedText>,
}

/// A complete explanation after applying an optional fixed-step override to
/// the explanation of the cell's skill row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedBriefing {
    pub skill: DrillSkill,
    pub purpose: LocalizedText,
    pub goal: LocalizedText,
    pub content: LocalizedText,
    pub tips: Vec<LocalizedText>,
    pub missed: LocalizedText,
    pub met: LocalizedText,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BriefingCatalog {
    version: String,
    #[serde(rename = "skill")]
    skills: Vec<SkillBriefing>,
    #[serde(rename = "step")]
    steps: Vec<StepBriefing>,
}

impl BriefingCatalog {
    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn skills(&self) -> &[SkillBriefing] {
        &self.skills
    }

    pub fn steps(&self) -> &[StepBriefing] {
        &self.steps
    }

    pub fn skill(&self, skill: DrillSkill) -> Option<&SkillBriefing> {
        self.skills.iter().find(|entry| entry.skill == skill)
    }

    pub fn step(&self, id: &str) -> Option<&StepBriefing> {
        self.steps.iter().find(|entry| entry.id == id)
    }

    fn validate(&self) -> Result<(), String> {
        if self.version != BRIEFING_VERSION {
            return Err(format!("briefing version must be {BRIEFING_VERSION}"));
        }
        if self.skills.len() != DrillSkill::ALL.len() {
            return Err(format!("briefing must contain {} skills", DrillSkill::ALL.len()));
        }
        let mut skills = BTreeSet::new();
        for entry in &self.skills {
            if !skills.insert(entry.skill.as_str()) {
                return Err(format!("duplicate briefing skill: {}", entry.skill.as_str()));
            }
            validate_text(&entry.purpose, "purpose")?;
            validate_text(&entry.goal, "goal")?;
            validate_text(&entry.content, "content")?;
            validate_text(&entry.missed, "missed")?;
            validate_text(&entry.met, "met")?;
            if entry.tips.is_empty() {
                return Err(format!("{} tips must not be empty", entry.skill.as_str()));
            }
            for tip in &entry.tips {
                validate_text(tip, "tip")?;
            }
        }
        for skill in DrillSkill::ALL {
            if !skills.contains(skill.as_str()) {
                return Err(format!("missing briefing skill: {}", skill.as_str()));
            }
        }

        let menu = super::menu::bundled_catalog()?;
        let fixed_ids = menu.fixed_steps.iter().map(|step| step.id.as_str()).collect::<BTreeSet<_>>();
        let mut step_ids = BTreeSet::new();
        for entry in &self.steps {
            if !step_ids.insert(entry.id.as_str()) {
                return Err(format!("duplicate briefing step: {}", entry.id));
            }
            if !fixed_ids.contains(entry.id.as_str()) {
                return Err(format!("unknown briefing step: {}", entry.id));
            }
            for (field, text) in [
                ("purpose", entry.purpose.as_ref()),
                ("goal", entry.goal.as_ref()),
                ("content", entry.content.as_ref()),
                ("missed", entry.missed.as_ref()),
                ("met", entry.met.as_ref()),
            ] {
                if let Some(text) = text {
                    validate_text(text, field)?;
                }
            }
            if let Some(tips) = &entry.tips {
                if tips.is_empty() {
                    return Err(format!("{} tips override must not be empty", entry.id));
                }
                for tip in tips {
                    validate_text(tip, "tip")?;
                }
            }
        }
        Ok(())
    }
}

fn validate_text(text: &LocalizedText, field: &str) -> Result<(), String> {
    if text.en.trim().is_empty() || text.ja.trim().is_empty() {
        Err(format!("briefing {field} must contain non-empty en and ja text"))
    } else {
        Ok(())
    }
}

pub fn parse(raw: &str) -> Result<BriefingCatalog, String> {
    let catalog = toml::from_str::<BriefingCatalog>(raw).map_err(|error| format!("briefing catalog parse error: {error}"))?;
    catalog.validate()?;
    Ok(catalog)
}

pub fn bundled() -> Result<&'static BriefingCatalog, String> {
    static CATALOG: OnceLock<Result<BriefingCatalog, String>> = OnceLock::new();
    CATALOG.get_or_init(|| parse(BRIEFING_DATA)).as_ref().map_err(Clone::clone)
}

/// Resolve the explanation of one row of the individual-training table.
pub fn resolve_skill(skill: DrillSkill) -> Result<ResolvedBriefing, String> {
    let entry = bundled()?
        .skill(skill)
        .ok_or_else(|| format!("missing briefing skill: {}", skill.as_str()))?;
    Ok(ResolvedBriefing {
        skill,
        purpose: entry.purpose.clone(),
        goal: entry.goal.clone(),
        content: entry.content.clone(),
        tips: entry.tips.clone(),
        missed: entry.missed.clone(),
        met: entry.met.clone(),
    })
}

/// Resolve a menu step by overlaying its fixed-step fields on the explanation
/// of the skill row played by that step. Unknown/non-fixed step IDs simply use
/// the skill explanation.
pub fn resolve_step(id: &str, skill: DrillSkill) -> Result<ResolvedBriefing, String> {
    let mut resolved = resolve_skill(skill)?;
    let Some(step) = bundled()?.step(id) else {
        return Ok(resolved);
    };
    if let Some(value) = &step.purpose {
        resolved.purpose = value.clone();
    }
    if let Some(value) = &step.goal {
        resolved.goal = value.clone();
    }
    if let Some(value) = &step.content {
        resolved.content = value.clone();
    }
    if let Some(value) = &step.tips {
        resolved.tips = value.clone();
    }
    if let Some(value) = &step.missed {
        resolved.missed = value.clone();
    }
    if let Some(value) = &step.met {
        resolved.met = value.clone();
    }
    Ok(resolved)
}

/// Resolve the explanation for a concrete menu step. Its table cell supplies
/// the fallback skill; stored/legacy steps without a cell use the normal row.
pub fn resolve_menu_step(step: &super::menu::MenuStep) -> Result<ResolvedBriefing, String> {
    let skill = super::menu::step_cell(step).map_or(DrillSkill::Normal, |cell| cell.skill);
    resolve_step(&step.prescription_id, skill)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_finish_overrides_only_purpose_and_falls_back_to_burst() {
        let catalog = bundled().unwrap();
        let burst = catalog.skill(DrillSkill::Burst).unwrap();
        let finish = catalog.step("fixed.finish").unwrap();

        let resolved = resolve_step("fixed.finish", DrillSkill::Burst).unwrap();

        assert_eq!(resolved.purpose, finish.purpose.clone().unwrap());
        assert_eq!(resolved.goal, burst.goal);
        assert_eq!(resolved.content, burst.content);
        assert_eq!(resolved.tips, burst.tips);
        assert_eq!(resolved.missed, burst.missed);
        assert_eq!(resolved.met, burst.met);
    }

    #[test]
    fn skill_and_step_resolution_preserve_both_languages() {
        let catalog = bundled().unwrap();
        let accuracy = catalog.skill(DrillSkill::Accuracy).unwrap();
        let resolved_skill = resolve_skill(DrillSkill::Accuracy).unwrap();
        assert_eq!(resolved_skill.purpose.en, accuracy.purpose.en);
        assert_eq!(resolved_skill.purpose.ja, accuracy.purpose.ja);
        assert_eq!(resolved_skill.tips[0].en, accuracy.tips[0].en);
        assert_eq!(resolved_skill.tips[0].ja, accuracy.tips[0].ja);

        let warmup = catalog.step("fixed.warmup").unwrap();
        let resolved_step = resolve_step("fixed.warmup", DrillSkill::Normal).unwrap();
        assert_eq!(resolved_step.goal.en, warmup.goal.as_ref().unwrap().en);
        assert_eq!(resolved_step.goal.ja, warmup.goal.as_ref().unwrap().ja);
        assert_eq!(resolved_step.met.en, warmup.met.as_ref().unwrap().en);
        assert_eq!(resolved_step.met.ja, warmup.met.as_ref().unwrap().ja);
    }
}
