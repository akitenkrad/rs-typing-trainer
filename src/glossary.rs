//! Bundled bilingual glossary used by the game and generated documentation.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::i18n::Lang;
use crate::practice::menu::LocalizedText;

const GLOSSARY_VERSION: &str = "glossary-v1";
const GLOSSARY_DATA: &str = include_str!("../data/glossary/glossary-v1.toml");

pub const CATEGORY_ORDER: [&str; 7] = ["speed", "keystroke", "analysis", "menu", "drill", "bench", "record"];

pub const SCREEN_IDS: [&str; 16] = [
    "analysis.today",
    "analysis.coach",
    "analysis.trends",
    "analysis.keyboard",
    "analysis.transitions",
    "analysis.fingers",
    "analysis.bench",
    "analysis.skills",
    "analysis.growth",
    "result",
    "review",
    "menu",
    "briefing",
    "bench",
    "history",
    "playing",
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlossaryTerm {
    pub id: String,
    pub category: String,
    pub term: LocalizedText,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub short: LocalizedText,
    #[serde(default)]
    pub detail: Option<LocalizedText>,
    pub screens: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlossaryCatalog {
    version: String,
    #[serde(rename = "term")]
    terms: Vec<GlossaryTerm>,
}

impl GlossaryCatalog {
    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn terms(&self) -> &[GlossaryTerm] {
        &self.terms
    }

    pub fn term(&self, id: &str) -> Option<&GlossaryTerm> {
        self.terms.iter().find(|term| term.id == id)
    }

    fn validate(&self) -> Result<(), String> {
        if self.version != GLOSSARY_VERSION {
            return Err(format!("glossary version must be {GLOSSARY_VERSION}"));
        }

        let mut ids = BTreeSet::new();
        for entry in &self.terms {
            if entry.id.is_empty() || !entry.id.is_ascii() {
                return Err(format!("glossary id must be non-empty ASCII: {}", entry.id));
            }
            if !ids.insert(entry.id.as_str()) {
                return Err(format!("duplicate glossary id: {}", entry.id));
            }
            if !CATEGORY_ORDER.contains(&entry.category.as_str()) {
                return Err(format!("unknown glossary category for {}: {}", entry.id, entry.category));
            }
            validate_text(&entry.term, &entry.id, "term")?;
            validate_text(&entry.short, &entry.id, "short")?;
            if let Some(detail) = &entry.detail {
                validate_text(detail, &entry.id, "detail")?;
            }
            for screen in &entry.screens {
                if !SCREEN_IDS.contains(&screen.as_str()) {
                    return Err(format!("unknown glossary screen for {}: {screen}", entry.id));
                }
            }
        }
        Ok(())
    }
}

fn validate_text(text: &LocalizedText, id: &str, field: &str) -> Result<(), String> {
    if text.en.trim().is_empty() || text.ja.trim().is_empty() {
        Err(format!("glossary {field} for {id} must contain non-empty en and ja text"))
    } else {
        Ok(())
    }
}

pub fn parse(raw: &str) -> Result<GlossaryCatalog, String> {
    let catalog = toml::from_str::<GlossaryCatalog>(raw).map_err(|error| format!("glossary catalog parse error: {error}"))?;
    catalog.validate()?;
    Ok(catalog)
}

pub fn bundled() -> Result<&'static GlossaryCatalog, String> {
    static CATALOG: OnceLock<Result<GlossaryCatalog, String>> = OnceLock::new();
    CATALOG.get_or_init(|| parse(GLOSSARY_DATA)).as_ref().map_err(Clone::clone)
}

pub fn category_name(category: &str, lang: Lang) -> Option<&'static str> {
    match (category, lang) {
        ("speed", Lang::En) => Some("Speed and accuracy"),
        ("speed", Lang::Ja) => Some("速さと正確さ"),
        ("keystroke", Lang::En) => Some("Keystroke metrics"),
        ("keystroke", Lang::Ja) => Some("打鍵の指標"),
        ("analysis", Lang::En) => Some("Analysis"),
        ("analysis", Lang::Ja) => Some("分析"),
        ("menu", Lang::En) => Some("Practice menu"),
        ("menu", Lang::Ja) => Some("練習メニュー"),
        ("drill", Lang::En) => Some("Drills"),
        ("drill", Lang::Ja) => Some("訓練"),
        ("bench", Lang::En) => Some("Benchmark"),
        ("bench", Lang::Ja) => Some("ベンチマーク"),
        ("record", Lang::En) => Some("Records"),
        ("record", Lang::Ja) => Some("記録"),
        _ => None,
    }
}

pub fn screen_name(screen: &str, lang: Lang) -> Option<&'static str> {
    match (screen, lang) {
        ("analysis.today", Lang::En) => Some("Analysis · Today"),
        ("analysis.today", Lang::Ja) => Some("分析・今日"),
        ("analysis.coach", Lang::En) => Some("Analysis · Coach"),
        ("analysis.coach", Lang::Ja) => Some("分析・コーチ"),
        ("analysis.trends", Lang::En) => Some("Analysis · Trends"),
        ("analysis.trends", Lang::Ja) => Some("分析・推移"),
        ("analysis.keyboard", Lang::En) => Some("Analysis · Keyboard"),
        ("analysis.keyboard", Lang::Ja) => Some("分析・キーボード"),
        ("analysis.transitions", Lang::En) => Some("Analysis · Transitions"),
        ("analysis.transitions", Lang::Ja) => Some("分析・遷移"),
        ("analysis.fingers", Lang::En) => Some("Analysis · Fingers and hands"),
        ("analysis.fingers", Lang::Ja) => Some("分析・指・手"),
        ("analysis.bench", Lang::En) => Some("Analysis · Benchmark"),
        ("analysis.bench", Lang::Ja) => Some("分析・ベンチ"),
        ("analysis.skills", Lang::En) => Some("Analysis · Skills"),
        ("analysis.skills", Lang::Ja) => Some("分析・スキル"),
        ("analysis.growth", Lang::En) => Some("Analysis · Growth"),
        ("analysis.growth", Lang::Ja) => Some("分析・成長"),
        ("result", Lang::En) => Some("Result"),
        ("result", Lang::Ja) => Some("結果"),
        ("review", Lang::En) => Some("Review"),
        ("review", Lang::Ja) => Some("振り返り"),
        ("menu", Lang::En) => Some("Today's menu"),
        ("menu", Lang::Ja) => Some("今日のメニュー"),
        ("briefing", Lang::En) => Some("Briefing"),
        ("briefing", Lang::Ja) => Some("開始前の説明"),
        ("bench", Lang::En) => Some("Benchmark"),
        ("bench", Lang::Ja) => Some("ベンチマーク"),
        ("history", Lang::En) => Some("History"),
        ("history", Lang::Ja) => Some("履歴"),
        ("playing", Lang::En) => Some("Practice"),
        ("playing", Lang::Ja) => Some("練習中"),
        _ => None,
    }
}

pub fn generate_docs(catalog: &GlossaryCatalog, lang: Lang) -> String {
    let mut output = String::new();
    match lang {
        Lang::En => output.push_str("**English** | [日本語](glossary.ja.md)\n\n# Glossary\n\n"),
        Lang::Ja => output.push_str("[English](glossary.md) | **日本語**\n\n# 用語集\n\n"),
    }

    for category in CATEGORY_ORDER {
        let heading = category_name(category, lang).expect("known glossary category");
        writeln!(output, "## {heading}\n").expect("writing to a String cannot fail");
        for entry in catalog.terms().iter().filter(|entry| entry.category == category) {
            writeln!(output, "### {}\n", entry.term.text(lang)).expect("writing to a String cannot fail");
            if !entry.aliases.is_empty() {
                let label = match lang {
                    Lang::En => "Aliases",
                    Lang::Ja => "別名",
                };
                writeln!(output, "{label}: {}\n", entry.aliases.join(", ")).expect("writing to a String cannot fail");
            }
            writeln!(output, "{}\n", entry.short.text(lang)).expect("writing to a String cannot fail");
            if let Some(detail) = &entry.detail {
                writeln!(output, "{}\n", detail.text(lang)).expect("writing to a String cannot fail");
            }
        }
    }
    output
}
