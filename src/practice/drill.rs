//! The individual-training table (MYTASK-3638): rows are the skill a session
//! trains, columns what its prompts are made of.
//!
//! A cell becomes a session the same way a menu step does: a recorded mode
//! plus settings applied to an in-memory copy of the configuration
//! ([`config_for_cell`]). The skills that the menu builds from settings
//! (accuracy, speed, recovery, no-look) take the settings of their catalog
//! prescription, so a cell and the matching menu step behave alike.

use std::collections::BTreeMap;

use crate::config::{Config, DrillCell, DrillSkill, ModeKind, PromptUnit};
use crate::i18n::Lang;

/// Whether a cell can be chosen, and whether it was playable before the
/// table existed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    /// A mode that already existed (○).
    Existing,
    /// Playable because the prompt generation was extended for the table (△).
    Extended,
    /// The combination means little and cannot be started (—).
    Unavailable,
}

/// The availability of every cell, as agreed for MYTASK-3638 (and the four
/// rows MYTASK-3640 added for the menu's prescriptions). Spelling is
/// Japanese only; see [`is_available_in`].
pub fn availability(cell: DrillCell) -> Availability {
    use Availability::{Existing, Extended, Unavailable};
    use DrillSkill as S;
    use PromptUnit as U;
    match (cell.skill, cell.unit) {
        (S::Normal | S::Accuracy | S::Speed | S::Recovery | S::Blind, _) => Existing,
        // MYTASK-3640: anticipation, look-ahead, rollover and spelling need
        // text that reads on: words, phrases, sentences and passages.
        (S::Anticipation | S::Preview | S::Overlap | S::Spelling, U::Word | U::Phrase | U::Sentence | U::Passage) => Existing,
        (S::Weak, U::Ngram | U::Word) => Existing,
        (S::Weak, U::Phrase | U::Sentence) => Extended,
        (S::Rhythm, U::Word) => Existing,
        (S::Rhythm, U::Ngram | U::Phrase | U::Sentence) => Extended,
        (S::Burst, U::Word) => Existing,
        (S::Burst, U::Phrase | U::Sentence) => Extended,
        (S::Endurance, U::Sentence) => Existing,
        (S::Endurance, U::Word | U::Phrase | U::Passage) => Extended,
        (S::Unlock, U::Char | U::Word) => Existing,
        _ => Unavailable,
    }
}

pub fn is_available(cell: DrillCell) -> bool {
    availability(cell) != Availability::Unavailable
}

/// Whether a cell can be started in a practice language: the spelling row
/// practices romaji, so it is Japanese only.
pub fn is_available_in(cell: DrillCell, lang: Lang) -> bool {
    is_available(cell) && !(cell.skill == DrillSkill::Spelling && lang == Lang::En)
}

/// Rows whose cells run the plain mode of their unit and differ only in the
/// settings of their catalog prescription.
pub fn is_setting_row(skill: DrillSkill) -> bool {
    matches!(
        skill,
        DrillSkill::Normal
            | DrillSkill::Accuracy
            | DrillSkill::Speed
            | DrillSkill::Recovery
            | DrillSkill::Blind
            | DrillSkill::Anticipation
            | DrillSkill::Preview
            | DrillSkill::Overlap
            | DrillSkill::Spelling
    )
}

/// The mode a cell runs and records (`sessions.mode`). Cells that existed
/// before the table record the same mode as before; new cells record the
/// mode of their skill, and the prompt unit goes to `mode_params`.
pub fn mode_for(cell: DrillCell, lang: Lang) -> Option<ModeKind> {
    if !is_available_in(cell, lang) {
        return None;
    }
    Some(match cell.skill {
        DrillSkill::Weak => ModeKind::Adaptive,
        DrillSkill::Rhythm => ModeKind::Rhythm,
        DrillSkill::Burst => ModeKind::Burst,
        DrillSkill::Endurance => ModeKind::Endurance,
        DrillSkill::Unlock => ModeKind::Unlock,
        // The setting rows run the plain mode of their unit.
        _ => match cell.unit {
            PromptUnit::Char => ModeKind::Random,
            PromptUnit::Ngram => ModeKind::Ngram,
            PromptUnit::Word if lang == Lang::Ja => ModeKind::Romaji,
            PromptUnit::Word => ModeKind::Words,
            PromptUnit::Phrase => ModeKind::Phrase,
            PromptUnit::Sentence | PromptUnit::Passage => ModeKind::Sentence,
        },
    })
}

/// Whether a menu step of mode `kind` in `lang` runs the cell's own mode, so
/// that it can start the way the cell does (MYTASK-3640). The word cells of
/// the setting rows run `romaji` in Japanese on the table, while a menu step
/// keeps recording `words` and is fed the same Japanese words; both count as
/// the cell's mode. Any other mode is a level that overrides the cell's mode.
pub fn runs_as(cell: DrillCell, kind: ModeKind, lang: Lang) -> bool {
    match mode_for(cell, lang) {
        Some(mode) if mode == kind => true,
        Some(ModeKind::Romaji) => kind == ModeKind::Words && cell.unit == PromptUnit::Word,
        _ => false,
    }
}

/// Whether the practice language changes the prompts of a cell. Random
/// letters, combinations and key unlocking use English letters whatever the
/// language; weak transitions come from the model of the practice language.
pub fn uses_language(cell: DrillCell) -> bool {
    !(cell.skill == DrillSkill::Unlock || cell.unit == PromptUnit::Char || (cell.unit == PromptUnit::Ngram && cell.skill != DrillSkill::Weak))
}

/// The catalog prescription whose first level supplies a skill's settings.
pub fn catalog_prescription(skill: DrillSkill) -> Option<&'static str> {
    match skill {
        DrillSkill::Accuracy => Some("accuracy.gate"),
        DrillSkill::Speed => Some("speed.pacer"),
        DrillSkill::Recovery => Some("recovery.retry"),
        DrillSkill::Blind => Some("blind.fade"),
        DrillSkill::Anticipation => Some("accuracy.anticipation"),
        DrillSkill::Preview => Some("preview.wide"),
        DrillSkill::Overlap => Some("overlap.burst"),
        DrillSkill::Spelling => Some("japanese.spelling"),
        _ => None,
    }
}

/// The settings a cell applies on top of the user's configuration. Every
/// cell starts with the pacer off; the speed row switches it on through its
/// catalog settings.
pub fn cell_settings(cell: DrillCell) -> Result<BTreeMap<String, toml::Value>, String> {
    let mut settings = BTreeMap::new();
    settings.insert("pacer.enabled".to_owned(), toml::Value::Boolean(false));
    if let Some(id) = catalog_prescription(cell.skill) {
        let catalog = crate::practice::menu::bundled_catalog()?;
        let level = catalog
            .prescription(id)
            .and_then(|prescription| prescription.levels.first())
            .ok_or_else(|| format!("menu catalog has no level for {id}"))?;
        for (key, value) in &level.settings {
            if !key.starts_with(crate::practice::menu::TARGET_SETTING_PREFIX) {
                settings.insert(key.clone(), value.clone());
            }
        }
    }
    Ok(settings)
}

fn language_code(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "en",
        Lang::Ja => "ja",
    }
}

/// The in-memory configuration of a session started from a cell. The
/// configuration file is never written with these values.
pub fn config_for_cell(base: &Config, cell: DrillCell, lang: Lang) -> Result<Config, String> {
    let kind = mode_for(cell, lang).ok_or_else(|| format!("{} × {} cannot be played", cell.skill.as_str(), cell.unit.as_str()))?;
    let settings = cell_settings(cell)?;
    let mut config = crate::practice::menu::config_for_step(base, kind.as_str(), &settings)?;
    apply_cell_prompts(&mut config, cell, lang);
    Ok(config)
}

/// How a cell makes its prompts: the language of the per-mode language
/// settings and the cell itself (`mode.drill`, whose unit the prompt
/// generator follows). The table and today's menu (MYTASK-3640) both start a
/// cell through this.
pub fn apply_cell_prompts(config: &mut Config, cell: DrillCell, lang: Lang) {
    let code = language_code(if uses_language(cell) { lang } else { Lang::En }).to_owned();
    config.adaptive.lang.clone_from(&code);
    config.mode.phrase.lang.clone_from(&code);
    config.mode.sentence.lang = code;
    config.mode.drill = Some(cell);
}

/// The cell a mode belongs to when nothing names one: the row of its own
/// skill, or the plain row, with the mode's own unit.
pub fn cell_for_mode(kind: ModeKind) -> DrillCell {
    let skill = match kind {
        ModeKind::Adaptive => DrillSkill::Weak,
        ModeKind::Rhythm => DrillSkill::Rhythm,
        ModeKind::Burst => DrillSkill::Burst,
        ModeKind::Endurance => DrillSkill::Endurance,
        ModeKind::Unlock => DrillSkill::Unlock,
        _ => DrillSkill::Normal,
    };
    DrillCell::new(skill, PromptUnit::natural(kind))
}

/// The cell the table opens on: the saved cell, or the one matching the
/// configured mode.
pub fn initial_cell(config: &Config) -> DrillCell {
    if let (Some(skill), Some(unit)) = (config.drill.skill, config.drill.unit) {
        let cell = DrillCell::new(skill, unit);
        if is_available(cell) {
            return cell;
        }
    }
    cell_for_mode(config.mode.kind)
}

/// The practice language the table opens with: the saved one, or the
/// language of the configured mode.
pub fn initial_lang(config: &Config) -> Lang {
    if let Some(lang) = config.drill.lang {
        return lang;
    }
    let code = match config.mode.kind {
        ModeKind::Words => "en",
        ModeKind::Romaji => "ja",
        ModeKind::Phrase => config.mode.phrase.lang.as_str(),
        ModeKind::Adaptive => config.adaptive.lang.as_str(),
        _ => config.mode.sentence.lang.as_str(),
    };
    if code == "ja" { Lang::Ja } else { Lang::En }
}

/// Cells whose prompts are built from keys chosen on the key picker; they
/// cannot start without a selection.
pub fn requires_keys(cell: DrillCell) -> bool {
    matches!(cell.unit, PromptUnit::Char | PromptUnit::Ngram) && !matches!(cell.skill, DrillSkill::Unlock | DrillSkill::Weak)
}

/// Cells that pass through the key picker before starting, as `ngram`,
/// `words` and `romaji` always did: combinations, and the word cells of the
/// setting-based rows. Random letters open it only while no key is chosen.
pub fn uses_key_picker(cell: DrillCell) -> bool {
    (cell.unit == PromptUnit::Ngram && requires_keys(cell)) || (cell.unit == PromptUnit::Word && is_setting_row(cell.skill))
}

/// Items in one burst sprint: `burst.words` words, or the same distance in
/// phrases (about three words each) or a sentence.
pub fn burst_items(config: &Config) -> usize {
    let words = config.burst.words.max(1);
    match config.mode.prompt_unit() {
        PromptUnit::Phrase => words.div_ceil(3),
        PromptUnit::Sentence | PromptUnit::Passage => words.div_ceil(12),
        _ => words,
    }
}

/// Whether a session uses Japanese words for a word unit: `romaji`, or a
/// table cell played in Japanese.
pub fn japanese_words(config: &crate::config::ModeConfig) -> bool {
    config.kind == ModeKind::Romaji || (config.drill.is_some() && config.prompt_unit() == PromptUnit::Word && config.sentence.lang == "ja")
}
