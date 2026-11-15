use std::io::BufReader;
use std::process::Command;
use std::time::Duration;

use rs_typing_trainer::record::{SessionRecord, ttrec};
use rs_typing_trainer::replay::{PlaybackTiming, Replay};

const SAMPLES: [(&str, &str, &str); 3] = [
    ("dvorak-words-en.ttrec", "words", "dvorak"),
    ("qwerty-romaji-ja.ttrec", "romaji", "qwerty"),
    ("dvorak-sentence-en.ttrec", "sentence", "dvorak"),
];

fn load(name: &str) -> SessionRecord {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("records")
        .join(name);
    ttrec::read(BufReader::new(std::fs::File::open(path).unwrap())).unwrap()
}

#[test]
fn all_sample_records_are_readable_and_marked_as_synthetic() {
    for (name, mode, layout) in SAMPLES {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join("records")
            .join(name);
        let first = std::fs::read_to_string(&path).unwrap().lines().next().unwrap().to_owned();
        assert!(first.contains("合成した記録であり実際のプレイではない"), "{name}: {first}");
        let record = load(name);
        assert_eq!(record.header.mode, mode);
        assert_eq!(record.header.layout, layout);
        assert!(!record.log.items.is_empty());
        assert!(!record.log.keystrokes.is_empty());
    }
}

#[test]
fn print_summary_and_final_replay_metrics_match_the_record() {
    for (name, _, _) in SAMPLES {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join("records")
            .join(name);
        let output = Command::new(env!("CARGO_BIN_EXE_rs-typing-trainer"))
            .args(["replay", path.to_str().unwrap(), "--print"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{name}: {}", String::from_utf8_lossy(&output.stderr));
        let summary = String::from_utf8(output.stdout).unwrap();
        assert!(summary.contains("Mode:"), "{name}: {summary}");
        assert!(summary.contains("Layout:"), "{name}: {summary}");
        assert!(summary.contains("Prompt:"), "{name}: {summary}");
        assert!(summary.contains("Keystrokes:"), "{name}: {summary}");

        let record = load(name);
        let expected = rs_typing_trainer::analysis::analyze(&record.log);
        let mut replay = Replay::new(record.clone(), PlaybackTiming::Recorded { speed: 100.0 }, false, false);
        replay.advance_to(Duration::from_secs(60));
        assert!(replay.is_finished(), "{name}");
        assert_eq!(replay.metrics(), expected, "{name}");
        assert!((expected.net_wpm - record.header.net_wpm).abs() < 1e-9, "{name}");
        assert!((expected.accuracy - record.header.accuracy).abs() < 1e-9, "{name}");
    }
}

#[test]
fn samples_have_the_intended_human_scale() {
    let words = load("dvorak-words-en.ttrec");
    assert!((50.0..=70.0).contains(&words.header.net_wpm), "{}", words.header.net_wpm);
    assert!(words.header.accuracy < 1.0 && words.header.accuracy >= 0.95);

    let romaji = load("qwerty-romaji-ja.ttrec");
    let typed = romaji.log.keystrokes.iter().filter_map(|key| key.typed.as_deref()).collect::<String>();
    assert!(typed.contains("shi") && typed.contains("si"), "{typed}");

    let sentence = load("dvorak-sentence-en.ttrec");
    assert!(
        (18_000..=23_000).contains(&sentence.header.duration_ms),
        "{}",
        sentence.header.duration_ms
    );
}
