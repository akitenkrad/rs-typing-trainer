//! Keyboard layouts, reverse lookup, and input-layout remapping.

use std::collections::{BTreeMap, btree_map::Entry};
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub mod geometry;

const BUILTINS: [(&str, &str); 7] = [
    ("qwerty", include_str!("defs/qwerty.toml")),
    ("dvorak", include_str!("defs/dvorak.toml")),
    ("colemak", include_str!("defs/colemak.toml")),
    ("colemak-dh", include_str!("defs/colemak-dh.toml")),
    ("colemak-dh-ansi", include_str!("defs/colemak-dh-ansi.toml")),
    ("workman", include_str!("defs/workman.toml")),
    ("jis-qwerty", include_str!("defs/jis-qwerty.toml")),
];

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct KeyPair {
    pub normal: char,
    pub shift: char,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyStroke {
    pub code: String,
    pub shift: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyboardLayout {
    name: String,
    keys: BTreeMap<String, KeyPair>,
    reverse: BTreeMap<char, KeyStroke>,
}

#[derive(Debug)]
pub enum LayoutError {
    UnknownBuiltin(String),
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    Duplicate {
        path: PathBuf,
        character: char,
        first: String,
        second: String,
    },
}

impl fmt::Display for LayoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownBuiltin(name) => write!(formatter, "unknown bundled layout: {name}"),
            Self::Io { path, source } => write!(formatter, "{}: {source}", path.display()),
            Self::Parse { path, source } => write!(formatter, "{}: {source}", path.display()),
            Self::Duplicate {
                path,
                character,
                first,
                second,
            } => write!(
                formatter,
                "{}: character {character:?} is assigned to both {first} and {second}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for LayoutError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Deserialize)]
struct LayoutFile {
    name: String,
    keys: BTreeMap<String, KeyPair>,
}

impl KeyboardLayout {
    fn parse(source: &str, path: PathBuf) -> Result<Self, LayoutError> {
        let parsed: LayoutFile = toml::from_str(source).map_err(|source| LayoutError::Parse { path: path.clone(), source })?;
        let mut reverse = BTreeMap::new();
        for (code, pair) in &parsed.keys {
            for (character, shift) in [(pair.normal, false), (pair.shift, true)] {
                match reverse.entry(character) {
                    Entry::Vacant(entry) => {
                        entry.insert(KeyStroke { code: code.clone(), shift });
                    }
                    Entry::Occupied(entry) if entry.get().code == *code => {}
                    Entry::Occupied(entry) => {
                        return Err(LayoutError::Duplicate {
                            path,
                            character,
                            first: entry.get().code.clone(),
                            second: code.clone(),
                        });
                    }
                }
            }
        }
        Ok(Self {
            name: parsed.name,
            keys: parsed.keys,
            reverse,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn keys(&self) -> &BTreeMap<String, KeyPair> {
        &self.keys
    }

    pub fn key(&self, code: &str) -> Option<&KeyPair> {
        self.keys.get(code)
    }

    pub fn stroke_for(&self, character: char) -> Option<KeyStroke> {
        self.reverse.get(&character).cloned()
    }

    pub fn output(&self, code: &str, shift: bool) -> Option<char> {
        self.key(code).map(|pair| if shift { pair.shift } else { pair.normal })
    }
}

pub fn builtin_layout_names() -> [&'static str; 7] {
    BUILTINS.map(|(name, _)| name)
}

pub fn builtin_layout(name: &str) -> Result<KeyboardLayout, LayoutError> {
    let (_, source) = BUILTINS
        .iter()
        .find(|(builtin_name, _)| *builtin_name == name)
        .ok_or_else(|| LayoutError::UnknownBuiltin(name.to_owned()))?;
    KeyboardLayout::parse(source, PathBuf::from(format!("<bundled:{name}>")))
}

pub fn load_user_layouts(config_dir: &Path) -> Result<Vec<KeyboardLayout>, LayoutError> {
    let directory = config_dir.join("layouts");
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let entries = std::fs::read_dir(&directory).map_err(|source| LayoutError::Io {
        path: directory.clone(),
        source,
    })?;
    let mut paths = entries
        .map(|entry| {
            entry.map(|value| value.path()).map_err(|source| LayoutError::Io {
                path: directory.clone(),
                source,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    paths.retain(|path| path.extension().is_some_and(|extension| extension == "toml"));
    paths.sort();

    paths
        .into_iter()
        .map(|path| {
            let source = std::fs::read_to_string(&path).map_err(|source| LayoutError::Io { path: path.clone(), source })?;
            KeyboardLayout::parse(&source, path)
        })
        .collect()
}

pub fn remap_char(typed: char, input_layout: Option<&KeyboardLayout>, practice_layout: &KeyboardLayout) -> char {
    let Some(input_layout) = input_layout else {
        return typed;
    };
    input_layout
        .stroke_for(typed)
        .and_then(|stroke| practice_layout.output(&stroke.code, stroke.shift))
        .unwrap_or(typed)
}
