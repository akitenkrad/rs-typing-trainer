//! MYTASK-3641: starting a session from the table builds its live goal from
//! the cell's earlier sessions (its line, previous value and best) and, for
//! the spelling row, from the Japanese analysis. Keep that short for a player
//! with many sessions: 200 of the cell and 300 of other cells.
use std::time::{Duration, Instant};

use chrono::{TimeZone, Utc};
use rs_typing_trainer::app::App;
use rs_typing_trainer::config::{Config, DrillCell, DrillSkill, ModeKind, PromptUnit};
use rs_typing_trainer::engine::session::{ItemLog, ItemOutcome, KeystrokeKind, KeystrokeLog, SessionLog};
use rs_typing_trainer::i18n::Lang;
use rs_typing_trainer::practice::adaptive::{FocusReason, TransitionCost};
use rs_typing_trainer::record::{SessionRecord, db};

const SAME_CELL: usize = 200;
const OTHER_CELLS: usize = 300;
const KEYSTROKES: usize = 300;
const BUDGET: Duration = Duration::from_millis(150);

/// One session of about `KEYSTROKES` keystrokes, one word per item, with key
/// releases and a mistype every 23 keystrokes.
fn log(words: &[(&str, Option<&str>)], seed: u64) -> SessionLog {
    let mut items = Vec::new();
    let mut keystrokes = Vec::new();
    let mut time_us = 100_000_u64;
    let mut index = 0_usize;
    while keystrokes.len() < KEYSTROKES {
        let (typed, reading) = words[(index + seed as usize) % words.len()];
        let idx = index as u32;
        let shown = time_us;
        for (pos, character) in typed.chars().enumerate() {
            let wrong = keystrokes.len() % 23 == 11;
            if wrong {
                keystrokes.push(KeystrokeLog {
                    seq: keystrokes.len() as u32,
                    t_us: time_us,
                    release_us: Some(time_us + 50_000),
                    item_idx: idx,
                    pos: pos as u32,
                    kind: KeystrokeKind::Char,
                    typed: Some("z".to_owned()),
                    expected: Some(character.to_string()),
                    correct: false,
                    code: None,
                });
                time_us += 140_000;
            }
            keystrokes.push(KeystrokeLog {
                seq: keystrokes.len() as u32,
                t_us: time_us,
                release_us: Some(time_us + 40_000 + (keystrokes.len() as u64 * 37 + seed) % 90_000),
                item_idx: idx,
                pos: pos as u32,
                kind: KeystrokeKind::Char,
                typed: Some(character.to_string()),
                expected: Some(character.to_string()),
                correct: true,
                code: None,
            });
            time_us += 110_000 + (keystrokes.len() as u64 * 53 + seed * 7) % 80_000;
        }
        items.push(ItemLog {
            idx,
            text: reading.map_or_else(|| typed.to_owned(), str::to_owned),
            reading: reading.map(str::to_owned),
            kind: None,
            shown_us: shown,
            done_us: Some(time_us),
            outcome: ItemOutcome::Completed,
        });
        time_us += 200_000;
        index += 1;
    }
    SessionLog {
        duration_us: time_us,
        error_mode: Default::default(),
        items,
        keystrokes,
    }
}

const ENGLISH: [(&str, Option<&str>); 8] = [
    ("the ", None),
    ("quick ", None),
    ("brown ", None),
    ("fox ", None),
    ("jumps ", None),
    ("over ", None),
    ("lazy ", None),
    ("dog ", None),
];
const JAPANESE: [(&str, Option<&str>); 4] = [
    ("shikashi", Some("しかし")),
    ("tsuchi", Some("つち")),
    ("sushi", Some("すし")),
    ("kashi", Some("かし")),
];

/// Save `count` sessions of `cell` (or of no cell), a minute apart.
fn save_sessions(path: &std::path::Path, cell: Option<DrillCell>, japanese: bool, count: usize, offset: usize) -> Vec<SessionRecord> {
    let mut config = Config::default();
    config.mode.kind = if japanese { ModeKind::Romaji } else { ModeKind::Words };
    config.mode.drill = cell;
    config.keyboard.layout = "qwerty".to_owned();
    if japanese {
        config.mode.sentence.lang = "ja".to_owned();
    }
    (0..count)
        .map(|index| {
            let words: &[(&str, Option<&str>)] = if japanese { &JAPANESE } else { &ENGLISH };
            let started = Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap() + chrono::Duration::minutes((offset + index) as i64);
            let mut record = SessionRecord::from_log(&config, log(words, index as u64), started);
            record.header.release_events = true;
            record.header.id = Some(db::save(path, &record).unwrap());
            record
        })
        .collect()
}

fn focus(pair: &str) -> TransitionCost {
    TransitionCost {
        from: pair[..1].to_owned(),
        to: pair[1..].to_owned(),
        kind: "drill".to_owned(),
        frequency: 1,
        frequency_quartile: 1,
        observation_n: 10,
        estimated_iki_ms: 150.0,
        estimated_error_rate: 0.0,
        target_iki_ms: 120.0,
        cost: 1.0,
        weight: 1.0,
        sampled_iki_ms: 150.0,
        sampled_error_rate: 0.0,
        posterior_log_variance: 0.0,
        progress_multiplier: 1.0,
        reason_kind: FocusReason::Slow,
        review_days: None,
    }
}

fn measure(app: &App, cell: DrillCell, practice: Lang, records: &[SessionRecord]) -> Duration {
    let mut samples = Vec::new();
    for _ in 0..3 {
        let start = Instant::now();
        let state = app.cell_live_goal(cell, practice, Duration::from_secs(60), records).expect("a goal");
        samples.push(start.elapsed());
        // The spelling row has values only when the analysis suggests a
        // spelling; the synthetic sessions may give none, but the analysis
        // that looks for one is run and timed all the same.
        if cell.skill != DrillSkill::Spelling || state.goal.spelling.is_some() {
            assert_eq!(state.goal.history.len(), SAME_CELL, "{cell:?}: every earlier session of the cell is read");
        }
        std::hint::black_box(state);
    }
    samples.sort();
    samples[1]
}

#[test]
fn starting_a_cell_with_many_earlier_sessions_stays_short() {
    let _guard = crate::perf_lock();
    let cases = [
        (DrillCell::new(DrillSkill::Recovery, PromptUnit::Word), Lang::En),
        (DrillCell::new(DrillSkill::Unlock, PromptUnit::Word), Lang::En),
        (DrillCell::new(DrillSkill::Weak, PromptUnit::Word), Lang::En),
        (DrillCell::new(DrillSkill::Spelling, PromptUnit::Word), Lang::Ja),
        (DrillCell::new(DrillSkill::Overlap, PromptUnit::Word), Lang::En),
    ];
    for (cell, practice) in cases {
        let home = tempfile::TempDir::new().unwrap();
        let mut config = Config::default();
        config.storage.db_path = Some(home.path().join("sessions.db"));
        let path = config.storage.db_path.clone().unwrap();
        let japanese = practice == Lang::Ja;
        let mut records = save_sessions(&path, Some(cell), japanese, SAME_CELL, 0);
        records.extend(save_sessions(
            &path,
            Some(DrillCell::new(DrillSkill::Normal, PromptUnit::Word)),
            japanese,
            OTHER_CELLS / 2,
            SAME_CELL,
        ));
        records.extend(save_sessions(
            &path,
            None,
            !japanese,
            OTHER_CELLS - OTHER_CELLS / 2,
            SAME_CELL + OTHER_CELLS,
        ));
        let mut app = App::new_at(config, home.path().to_path_buf(), chrono::NaiveDate::from_ymd_opt(2026, 11, 20).unwrap());
        if cell.skill == DrillSkill::Weak {
            // The focus the adaptive plan gives before the goal is built.
            app.adaptive_focus = ["th", "he", "ow", "qu", "az"].iter().map(|pair| focus(pair)).collect();
        }
        let median = measure(&app, cell, practice, &records);
        println!(
            "cell goal {:?}×{:?}: {SAME_CELL} of the cell + {OTHER_CELLS} others median={:.1} ms (debug_assertions={})",
            cell.skill,
            cell.unit,
            median.as_secs_f64() * 1000.0,
            cfg!(debug_assertions)
        );
        if !cfg!(debug_assertions) {
            assert!(median <= BUDGET, "{cell:?}: median {median:?} exceeds {BUDGET:?}");
        }
    }
}
