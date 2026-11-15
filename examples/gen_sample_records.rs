//! Deterministically regenerate the synthetic `.ttrec` files in `examples/records/`.

use std::collections::HashMap;
use std::f64::consts::TAU;
use std::path::{Path, PathBuf};

use chrono::{TimeZone, Utc};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rs_typing_trainer::config::{Config, ErrorMode, ModeKind};
use rs_typing_trainer::data::{DataCatalog, DataKind};
use rs_typing_trainer::engine::romaji::RomajiJudge;
use rs_typing_trainer::engine::session::{ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};
use rs_typing_trainer::layout::builtin_layout;
use rs_typing_trainer::layout::geometry::{Finger, geometry};
use rs_typing_trainer::record::{SessionRecord, ttrec};

const NOTICE_JA: &str = "合成した記録であり実際のプレイではない";

#[derive(Clone)]
struct Prompt {
    text: String,
    reading: Option<String>,
    typed: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = DataCatalog::load(&Default::default(), Path::new("."))?;
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/records");
    std::fs::create_dir_all(&output)?;

    let words = select_english(
        &catalog,
        DataKind::EnglishWord,
        &[
            "morning", "quiet", "river", "bright", "garden", "silver", "little", "forest", "orange", "music", "friend", "travel", "window", "simple",
            "stone",
        ],
    );
    write_sample(
        &output.join("dvorak-words-en.ttrec"),
        synthesize(words, "dvorak", ModeKind::Words, 0xD0_70_AA_01, 155.0, 250, &[23, 61]),
    )?;

    let japanese = select_japanese(&catalog, &["知る", "下", "美しい", "死ぬ", "自然", "自身", "もし", "足", "主義", "仕事"]);
    let japanese = japanese
        .into_iter()
        .enumerate()
        .map(|(index, mut prompt)| {
            if index % 2 == 1 {
                prompt.typed = prompt.typed.replace("shi", "si").replace("chi", "ti").replace("tsu", "tu");
            }
            prompt
        })
        .collect();
    write_sample(
        &output.join("qwerty-romaji-ja.ttrec"),
        synthesize(japanese, "qwerty", ModeKind::Romaji, 0x0A_11_5E_02, 170.0, 280, &[19, 47]),
    )?;

    let sentences = select_english(
        &catalog,
        DataKind::EnglishSentence,
        &[
            "\"Are you ready?\" \"I was born ready.\"",
            "\"Can I ask you a question?\" \"No.\"",
            "\"Do you believe him?\" \"Not really!\"",
        ],
    );
    write_sample(
        &output.join("dvorak-sentence-en.ttrec"),
        synthesize(sentences, "dvorak", ModeKind::Sentence, 0x5E_17_EE_03, 175.0, 300, &[31, 76]),
    )?;
    Ok(())
}

fn select_english(catalog: &DataCatalog, kind: DataKind, texts: &[&str]) -> Vec<Prompt> {
    texts
        .iter()
        .map(|text| {
            let item = catalog
                .fixed_items(kind)
                .into_iter()
                .find(|item| item.text == *text)
                .unwrap_or_else(|| panic!("bundled prompt not found: {text}"));
            Prompt {
                text: item.text.clone(),
                reading: None,
                typed: item.text.clone(),
            }
        })
        .collect()
}

fn select_japanese(catalog: &DataCatalog, texts: &[&str]) -> Vec<Prompt> {
    texts
        .iter()
        .map(|text| {
            let item = catalog
                .fixed_items(DataKind::JapaneseWord)
                .into_iter()
                .find(|item| item.text == *text)
                .unwrap_or_else(|| panic!("bundled prompt not found: {text}"));
            let reading = item.reading.clone().expect("Japanese prompt reading");
            Prompt {
                text: item.text.clone(),
                typed: RomajiJudge::new(&reading).guide().to_owned(),
                reading: Some(reading),
            }
        })
        .collect()
}

fn synthesize(
    prompts: Vec<Prompt>,
    layout_name: &str,
    mode: ModeKind,
    seed: u64,
    median_ms: f64,
    item_gap_ms: u64,
    error_positions: &[usize],
) -> SessionRecord {
    let layout = builtin_layout(layout_name).expect("bundled layout");
    let geometry = geometry("ansi").expect("bundled geometry");
    let fingers = geometry
        .rows
        .iter()
        .flatten()
        .map(|key| (key.code, key.finger))
        .collect::<HashMap<_, _>>();
    let mut rng = StdRng::seed_from_u64(seed);
    let mut items = Vec::new();
    let mut keystrokes = Vec::new();
    let mut time_us = 0u64;
    let mut correct_position = 0usize;
    let mut previous_code: Option<String> = None;

    for (item_index, prompt) in prompts.into_iter().enumerate() {
        if item_index > 0 {
            time_us += item_gap_ms * 1_000;
        }
        let shown_us = time_us;
        for (position, character) in prompt.typed.chars().enumerate() {
            let expected = character.to_string();
            let code = layout.stroke_for(character).map(|stroke| stroke.code);
            let factor = transition_factor(previous_code.as_deref(), code.as_deref(), &fingers);
            time_us += lognormal_interval_us(&mut rng, median_ms, factor);
            if error_positions.contains(&correct_position) {
                let wrong = if character == 'x' { 'z' } else { 'x' };
                keystrokes.push(KeystrokeLog {
                    seq: keystrokes.len() as u32,
                    t_us: time_us,
                    release_us: Some(time_us + 62_000),
                    item_idx: item_index as u32,
                    pos: position as u32,
                    kind: KeystrokeKind::Char,
                    typed: Some(wrong.to_string()),
                    expected: Some(expected.clone()),
                    correct: false,
                    code: layout.stroke_for(wrong).map(|stroke| stroke.code),
                });
                time_us += 82_000;
                keystrokes.push(KeystrokeLog {
                    seq: keystrokes.len() as u32,
                    t_us: time_us,
                    release_us: None,
                    item_idx: item_index as u32,
                    pos: position as u32,
                    kind: KeystrokeKind::Backspace,
                    typed: None,
                    expected: Some(expected.clone()),
                    correct: false,
                    code: Some("Backspace".to_owned()),
                });
                time_us += 105_000;
            }
            keystrokes.push(KeystrokeLog {
                seq: keystrokes.len() as u32,
                t_us: time_us,
                release_us: Some(time_us + rng.gen_range(55_000..=105_000)),
                item_idx: item_index as u32,
                pos: position as u32,
                kind: KeystrokeKind::Char,
                typed: Some(expected.clone()),
                expected: Some(expected),
                correct: true,
                code: code.clone(),
            });
            previous_code = code;
            correct_position += 1;
        }
        items.push(ItemLog {
            idx: item_index as u32,
            text: prompt.text,
            reading: prompt.reading,
            shown_us,
            done_us: Some(time_us),
            outcome: ItemOutcome::Completed,
        });
    }
    time_us += 450_000;
    let log = SessionLog {
        duration_us: time_us,
        error_mode: ErrorMode::Stop,
        items,
        keystrokes,
    };
    let mut config = Config::default();
    config.keyboard.layout = layout_name.to_owned();
    config.mode.kind = mode;
    let mut record = SessionRecord::from_log(&config, log, Utc.with_ymd_and_hms(2026, 11, 14, 9, 0, 0).unwrap());
    record.header.release_events = true;
    record.header.mode_params = serde_json::json!({
        "synthetic_notice_ja": NOTICE_JA,
        "generator": "examples/gen_sample_records.rs",
        "seed": seed,
        "median_interval_ms": median_ms,
        "transition_class_adjustment": true,
    })
    .to_string();
    record
}

fn lognormal_interval_us(rng: &mut StdRng, median_ms: f64, transition_factor: f64) -> u64 {
    let u1 = rng.r#gen::<f64>().clamp(f64::MIN_POSITIVE, 1.0);
    let u2 = rng.r#gen::<f64>();
    let standard_normal = (-2.0 * u1.ln()).sqrt() * (TAU * u2).cos();
    let milliseconds = (median_ms * (0.24 * standard_normal).exp() * transition_factor).clamp(65.0, 430.0);
    (milliseconds * 1_000.0).round() as u64
}

fn transition_factor(previous: Option<&str>, current: Option<&str>, fingers: &HashMap<&str, Finger>) -> f64 {
    let (Some(previous), Some(current)) = (previous, current) else {
        return 1.0;
    };
    let (Some(previous), Some(current)) = (fingers.get(previous), fingers.get(current)) else {
        return 1.0;
    };
    if previous == current {
        1.24
    } else if left_hand(*previous) == left_hand(*current) {
        1.09
    } else {
        0.88
    }
}

fn left_hand(finger: Finger) -> bool {
    matches!(finger, Finger::LeftPinky | Finger::LeftRing | Finger::LeftMiddle | Finger::LeftIndex)
}

fn write_sample(path: &Path, record: SessionRecord) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::fs::File::create(path)?;
    ttrec::write(&record, file)?;
    Ok(())
}
