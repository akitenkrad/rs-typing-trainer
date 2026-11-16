use std::collections::BTreeMap;

use rs_typing_trainer::config::Config;

fn documented_defaults(path: &str) -> BTreeMap<String, String> {
    let text = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap();
    text.lines()
        .filter(|line| line.starts_with("| `"))
        .map(|line| {
            let columns = line.split('|').map(str::trim).collect::<Vec<_>>();
            let key = columns[1].trim_matches('`').to_owned();
            let value = columns[3].trim_matches('`').to_owned();
            (key, value)
        })
        .collect()
}

fn flatten(prefix: &str, value: &toml::Value, output: &mut BTreeMap<String, String>) {
    match value {
        toml::Value::Table(table) => {
            for (key, value) in table {
                let path = if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") };
                flatten(&path, value, output);
            }
        }
        toml::Value::String(value) => {
            output.insert(prefix.to_owned(), format!("\"{value}\""));
        }
        toml::Value::Integer(value) => {
            output.insert(prefix.to_owned(), value.to_string());
        }
        toml::Value::Float(value) => {
            output.insert(prefix.to_owned(), format!("{value:?}"));
        }
        toml::Value::Boolean(value) => {
            output.insert(prefix.to_owned(), value.to_string());
        }
        other => panic!("unexpected config value at {prefix}: {other:?}"),
    }
}

#[test]
fn configuration_tables_match_every_struct_default() {
    let mut actual = BTreeMap::new();
    flatten("", &toml::Value::try_from(Config::default()).unwrap(), &mut actual);
    let unset = [
        "adaptive.error_weight",
        "data.phrases_en",
        "data.phrases_ja",
        "data.sentences_en",
        "data.sentences_ja",
        "data.words_en",
        "data.words_ja",
        "metrics.db_path",
        "rhythm.bpm",
        "storage.db_path",
    ];
    for path in ["docs/configuration.md", "docs/configuration.ja.md"] {
        let documented = documented_defaults(path);
        for (key, value) in &actual {
            assert_eq!(documented.get(key), Some(value), "{path}: {key}");
        }
        for key in unset {
            assert_eq!(documented.get(key).map(String::as_str), Some("—"), "{path}: {key}");
        }
        assert_eq!(
            documented.len(),
            actual.len() + unset.len(),
            "{path} contains an unknown or duplicate key"
        );
    }
}
