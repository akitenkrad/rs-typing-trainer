use rs_typing_trainer::practice::menu::{MenuMetric, MetricSource, bundled_catalog, parse_catalog};

fn minimal_catalog(prescription: &str) -> String {
    format!(
        r#"
version = "menu-v1"

[[fixed_step]]
role = "warmup"
id = "fixed.warmup"
title = {{ en = "Warm-up", ja = "ウォームアップ" }}
mode = "words"
minutes = 2
settings = {{ "pacer.enabled" = false }}
focus = {{ en = "Relax", ja = "力を抜く" }}

{prescription}
"#
    )
}

fn prescription(id: &str, alternate: &str, mode: &str, metric: &str, evidence: &str, settings: &str) -> String {
    format!(
        r#"
[[prescription]]
id = "{id}"
component = "speed"
title = {{ en = "Pacer", ja = "ペーサー" }}
evidence = "{evidence}"
evidence_note = {{ en = "Note", ja = "注記" }}
alternate = "{alternate}"

[[prescription.level]]
mode = "{mode}"
minutes = 4
settings = {{ {settings} }}
focus = {{ en = "Follow", ja = "追う" }}
advance = {{ metric = "{metric}", rule = "ge-own-median", sessions = 3, source = "check", note = {{ en = "Three times", ja = "3 回" }} }}
"#
    )
}

#[test]
fn bundled_catalog_contains_all_prescriptions_and_fixed_steps() {
    let catalog = bundled_catalog().unwrap();

    assert_eq!(catalog.version, "menu-v1");
    assert_eq!(catalog.prescriptions.len(), 14);
    assert_eq!(catalog.fixed_steps.len(), 3);
    assert!(catalog.prescriptions.iter().all(|row| !row.levels.is_empty()));
    assert!(
        catalog
            .prescriptions
            .iter()
            .all(|row| !row.title.en.is_empty() && !row.title.ja.is_empty())
    );
    assert!(
        catalog
            .prescriptions
            .iter()
            .flat_map(|row| &row.levels)
            .all(|level| !level.focus.en.is_empty() && !level.focus.ja.is_empty())
    );
}

#[test]
fn catalog_rejects_duplicate_ids_and_broken_alternates() {
    let first = prescription("speed.pacer", "speed.pacer", "phrase", "net-wpm", "medium", "\"pacer.enabled\" = true");
    let duplicate = prescription("speed.pacer", "speed.pacer", "phrase", "net-wpm", "medium", "\"pacer.enabled\" = true");
    let error = parse_catalog(&minimal_catalog(&format!("{first}\n{duplicate}"))).unwrap_err();
    assert!(error.contains("duplicate"), "{error}");

    let broken = prescription("speed.pacer", "speed.missing", "phrase", "net-wpm", "medium", "\"pacer.enabled\" = true");
    let error = parse_catalog(&minimal_catalog(&broken)).unwrap_err();
    assert!(error.contains("alternate"), "{error}");
}

#[test]
fn catalog_rejects_empty_levels_unknown_modes_metrics_and_evidence() {
    let no_level = r#"
[[prescription]]
id = "speed.pacer"
component = "speed"
title = { en = "Pacer", ja = "ペーサー" }
evidence = "medium"
evidence_note = { en = "Note", ja = "注記" }
alternate = "speed.pacer"
"#;
    let error = parse_catalog(&minimal_catalog(no_level)).unwrap_err();
    assert!(error.contains("level"), "{error}");

    for (mode, metric, evidence, expected) in [
        ("missing", "net-wpm", "medium", "mode"),
        ("phrase", "invented", "medium", "metric"),
        ("phrase", "net-wpm", "strong", "evidence"),
    ] {
        let row = prescription("speed.pacer", "speed.pacer", mode, metric, evidence, "\"pacer.enabled\" = true");
        let error = parse_catalog(&minimal_catalog(&row)).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn catalog_rejects_unknown_setting_keys_and_wrong_value_types() {
    for (settings, expected) in [("\"pacer.missing\" = true", "setting"), ("\"pacer.enabled\" = \"yes\"", "type")] {
        let row = prescription("speed.pacer", "speed.pacer", "phrase", "net-wpm", "medium", settings);
        let error = parse_catalog(&minimal_catalog(&row)).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn every_metric_declares_its_existing_analysis_source() {
    // MYTASK-3641: the spelling-usage rate is measured by the Japanese
    // analysis now, so no metric is unavailable.
    for metric in MenuMetric::all() {
        assert_ne!(metric.source(), MetricSource::Unavailable, "{metric:?}");
    }
    assert_eq!(MenuMetric::SpellingUsageRate.source(), MetricSource::JapaneseSpellingUsage);
}
