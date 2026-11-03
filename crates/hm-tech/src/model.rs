//! The catalog's data model, as written in the pack files.

use serde::{Deserialize, Serialize};

/// A technology's group. Declaration order is display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Language,
    Framework,
    Ui,
    Data,
    Auth,
    Validation,
    Testing,
    Build,
    Deploy,
    Other,
}

impl Category {
    pub const ALL: [Category; 10] = [
        Category::Language,
        Category::Framework,
        Category::Ui,
        Category::Data,
        Category::Auth,
        Category::Validation,
        Category::Testing,
        Category::Build,
        Category::Deploy,
        Category::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Category::Language => "language",
            Category::Framework => "framework",
            Category::Ui => "ui",
            Category::Data => "data",
            Category::Auth => "auth",
            Category::Validation => "validation",
            Category::Testing => "testing",
            Category::Build => "build",
            Category::Deploy => "deploy",
            Category::Other => "other",
        }
    }
}

/// A package registry whose names the catalog can match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ecosystem {
    Npm,
    Pypi,
    Go,
    Maven,
    Nuget,
    Crates,
}

impl Ecosystem {
    pub const ALL: [Ecosystem; 6] = [
        Ecosystem::Npm,
        Ecosystem::Pypi,
        Ecosystem::Go,
        Ecosystem::Maven,
        Ecosystem::Nuget,
        Ecosystem::Crates,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Ecosystem::Npm => "npm",
            Ecosystem::Pypi => "pypi",
            Ecosystem::Go => "go",
            Ecosystem::Maven => "maven",
            Ecosystem::Nuget => "nuget",
            Ecosystem::Crates => "crates",
        }
    }
}

/// How much detail a description gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Beginner,
    Builder,
}

/// The language a description is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Locale {
    En,
    Tr,
}

/// One description per level, in one language.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Texts {
    pub beginner: String,
    pub builder: String,
}

/// One technology: its texts and everything that identifies it in a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tech {
    pub id: String,
    pub name: String,
    pub category: Category,
    pub en: Texts,
    pub tr: Texts,
    #[serde(default)]
    pub npm: Vec<String>,
    #[serde(default)]
    pub pypi: Vec<String>,
    #[serde(default)]
    pub go: Vec<String>,
    #[serde(default)]
    pub maven: Vec<String>,
    #[serde(default)]
    pub nuget: Vec<String>,
    #[serde(default)]
    pub crates: Vec<String>,
    #[serde(default)]
    pub languages: Vec<String>,
}

impl Tech {
    /// The description for a language and level.
    pub fn text(&self, locale: Locale, level: Level) -> &str {
        let texts = match locale {
            Locale::En => &self.en,
            Locale::Tr => &self.tr,
        };
        match level {
            Level::Beginner => &texts.beginner,
            Level::Builder => &texts.builder,
        }
    }

    /// The package keys (names or `prefix*` patterns) this technology has in one ecosystem.
    pub fn keys(&self, ecosystem: Ecosystem) -> &[String] {
        match ecosystem {
            Ecosystem::Npm => &self.npm,
            Ecosystem::Pypi => &self.pypi,
            Ecosystem::Go => &self.go,
            Ecosystem::Maven => &self.maven,
            Ecosystem::Nuget => &self.nuget,
            Ecosystem::Crates => &self.crates,
        }
    }
}
