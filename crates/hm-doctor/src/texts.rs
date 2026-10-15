//! Card texts (`packs/texts/cards.toml`): sentence templates per card kind and level.

use std::collections::BTreeMap;

use serde::Deserialize;

use hm_core::signature::{Level, Texts};

pub const CARDS_FILE: (&str, &str) = (
    "texts/cards.toml",
    include_str!("../../../packs/texts/cards.toml"),
);

/// Templates for one card kind: an optional title and sentences per level.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelLines {
    #[serde(default)]
    pub title: Option<Texts>,
    pub beginner: Vec<String>,
    pub builder: Vec<String>,
    pub expert: Vec<String>,
}

impl LevelLines {
    pub fn get(&self, level: Level) -> &[String] {
        match level {
            Level::Beginner => &self.beginner,
            Level::Builder => &self.builder,
            Level::Expert => &self.expert,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardTexts {
    pub process: LevelLines,
    pub container: LevelLines,
    pub elsewhere: LevelLines,
    pub app: LevelLines,
    pub system: LevelLines,
    pub unknown: LevelLines,
    pub free: LevelLines,
    /// Keyed by bind scope: `loopback`, `all`, `specific`.
    pub reach: BTreeMap<String, Texts>,
    /// Keyed by container health: `healthy`, `unhealthy`, `starting`.
    pub health: BTreeMap<String, Texts>,
    /// Said when a signature matched only "probably".
    pub guess: Option<Texts>,
}

/// Parses the embedded card texts. Problems become warnings and empty templates.
pub fn load_embedded() -> (CardTexts, Vec<String>) {
    let mut warnings = Vec::new();
    let cards = parse_cards(CARDS_FILE.0, CARDS_FILE.1, &mut warnings);
    (cards, warnings)
}

pub fn parse_cards(file: &str, text: &str, warnings: &mut Vec<String>) -> CardTexts {
    match toml::from_str::<CardTexts>(text) {
        Ok(cards) => {
            for key in ["loopback", "all", "specific"] {
                if !cards.reach.contains_key(key) {
                    warnings.push(format!("{file}: [reach.{key}] is missing"));
                }
            }
            for key in ["healthy", "unhealthy", "starting"] {
                if !cards.health.contains_key(key) {
                    warnings.push(format!("{file}: [health.{key}] is missing"));
                }
            }
            cards
        }
        Err(error) => {
            warnings.push(format!("{file}: {error}"));
            CardTexts::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_card_texts_load_without_warnings() {
        let (cards, warnings) = load_embedded();
        assert!(warnings.is_empty(), "{warnings:#?}");
        assert!(!cards.process.beginner.is_empty());
        assert!(cards.process.title.is_some());
        assert!(cards.guess.is_some());
    }

    #[test]
    fn a_broken_file_gives_empty_templates_and_a_warning() {
        let mut warnings = Vec::new();
        let cards = parse_cards("cards.toml", "[process\nbroken", &mut warnings);
        assert!(cards.process.beginner.is_empty());
        assert_eq!(warnings.len(), 1);
    }
}
