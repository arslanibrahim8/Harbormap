//! Types for signature packs (`packs/signatures/*.toml`). Loading and validation live
//! in `hm-doctor`; matching lives in `hm-project`.

use serde::{Deserialize, Serialize};

use crate::model::{Kind, Role};

/// The three explanation levels (Product design §3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Beginner,
    #[default]
    Builder,
    Expert,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Texts {
    pub beginner: String,
    pub builder: String,
    pub expert: String,
}

impl Texts {
    pub fn get(&self, level: Level) -> &str {
        match level {
            Level::Beginner => &self.beginner,
            Level::Builder => &self.builder,
            Level::Expert => &self.expert,
        }
    }
}

/// How to recognise one program, and what to say about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signature {
    /// Lowercase letters, digits and dashes; unique across all packs.
    pub id: String,
    /// Display name, e.g. "Vite" or "PostgreSQL".
    pub name: String,
    pub role: Role,
    pub kind: Kind,
    #[serde(default)]
    pub default_port: Option<u16>,
    /// The program serves HTTP, so a port becomes an `http://localhost:<port>` link.
    #[serde(default)]
    pub http: bool,
    /// The program forwards ports for containers (Docker Desktop, OrbStack, ...).
    #[serde(default)]
    pub forwarder: bool,
    /// Higher wins when several signatures match. Generic runtimes use 10.
    #[serde(default = "default_priority")]
    pub priority: i32,
    /// Restricts the signature to one OS ("macos", "linux" or "windows").
    #[serde(default)]
    pub os: Option<String>,
    /// Files whose change needs a restart (used by STALE-001 from alpha.2).
    #[serde(default)]
    pub restart_globs: Vec<String>,
    #[serde(default)]
    pub restart_globs_verified: bool,
    /// Alternatives: the signature matches when any `when` block matches.
    pub when: Vec<When>,
    pub texts: Texts,
    /// Examples that must match this signature; checked by tests.
    #[serde(default)]
    pub fixture: Vec<Fixture>,
}

fn default_priority() -> i32 {
    50
}

/// One way to recognise a program. Every listed condition must hold; inside one
/// condition, any listed value may match. Comparisons ignore case.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct When {
    /// Executable stem (`node`, `postgres`); a trailing `*` matches a prefix (`python*`).
    pub exe: Vec<String>,
    /// Basename of any argument (argv[0] included), ignoring `.js`, `.cjs`, `.mjs`, `.ts`, `.py`, `.exe`, `.cmd`.
    pub argv_basename: Vec<String>,
    /// Substring of any argument, case-sensitive.
    pub argv_contains: Vec<String>,
    /// Prefix of any argument, including argv[0] (`next-server`).
    pub argv_prefix: Vec<String>,
    /// Executable stem of any ancestor process.
    pub ancestor_exe: Vec<String>,
    /// File name or `*.ext` pattern present in the working directory.
    pub cwd_files: Vec<String>,
    /// Dependency declared in the nearest package.json, pyproject.toml or requirements file.
    pub package: Vec<String>,
    /// Last path segment of a container image, without tag or digest.
    pub image: Vec<String>,
    /// The match is a good guess rather than certain; the UI says "probably".
    pub probably: bool,
}

impl When {
    pub fn is_empty(&self) -> bool {
        self.exe.is_empty()
            && self.argv_basename.is_empty()
            && self.argv_contains.is_empty()
            && self.argv_prefix.is_empty()
            && self.ancestor_exe.is_empty()
            && self.cwd_files.is_empty()
            && self.package.is_empty()
            && self.image.is_empty()
    }
}

/// A process or container description that must match its signature.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Fixture {
    pub exe: Option<String>,
    pub argv: Vec<String>,
    pub ancestors: Vec<String>,
    pub cwd_files: Vec<String>,
    pub packages: Vec<String>,
    pub image: Option<String>,
    pub os: Option<String>,
}

/// Signatures in match order: highest priority first, then by ID.
#[derive(Debug, Clone, Default)]
pub struct SignatureSet {
    signatures: Vec<Signature>,
}

impl SignatureSet {
    pub fn new(mut signatures: Vec<Signature>) -> Self {
        signatures.sort_by(|a, b| b.priority.cmp(&a.priority).then_with(|| a.id.cmp(&b.id)));
        Self { signatures }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Signature> {
        self.signatures.iter()
    }

    pub fn get(&self, id: &str) -> Option<&Signature> {
        self.signatures.iter().find(|signature| signature.id == id)
    }

    pub fn len(&self) -> usize {
        self.signatures.len()
    }

    pub fn is_empty(&self) -> bool {
        self.signatures.is_empty()
    }
}
