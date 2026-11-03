//! What `list::build` reads: facts about a project, gathered elsewhere.
//!
//! Paths are repository-relative with `/`; the empty string is the repository root.

use serde::{Deserialize, Serialize};

use crate::model::Ecosystem;

/// Everything known about a project that the technologies list is built from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TechInput {
    /// Manifest roots: directories that hold a manifest.
    pub roots: Vec<String>,
    /// Packages the project itself defines (workspace members); never listed.
    pub own_packages: Vec<PackageRef>,
    pub declared: Vec<DeclaredDep>,
    /// One row per (importer file, package).
    pub imports: Vec<ExternalImport>,
    /// Ecosystems whose imports were measured; only these can claim "declared but not imported".
    pub import_coverage: Vec<Ecosystem>,
    /// Any file paths; only the ones the catalog knows are kept.
    pub configs: Vec<String>,
    pub images: Vec<DockerImage>,
    pub languages: Vec<LanguageCount>,
}

/// A package by ecosystem and name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageRef {
    pub ecosystem: Ecosystem,
    pub name: String,
}

/// A dependency a manifest declares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclaredDep {
    pub ecosystem: Ecosystem,
    pub name: String,
    /// The version requirement as written.
    pub declared: Option<String>,
    /// The version a lockfile pins.
    pub resolved: Option<String>,
    pub dev: bool,
    /// The manifest and the line it is declared on.
    pub file: String,
    pub line: u32,
    /// The directory of that manifest.
    pub root: String,
}

/// A source file importing a package from outside the project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalImport {
    pub ecosystem: Ecosystem,
    /// The import specifier as written (npm subpaths such as `lodash/fp` are allowed).
    pub package: String,
    pub importer: String,
}

/// An image a Dockerfile or compose file refers to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerImage {
    pub image: String,
    pub file: String,
    pub line: u32,
}

/// How many files of a language live under a path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanguageCount {
    pub path: String,
    pub language: String,
    pub files: u32,
}
