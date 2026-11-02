//! Loads `tsconfig.json` and `jsconfig.json` files: `baseUrl`, `paths` and the
//! relative `extends` chain behind them (Learn spec §5.2).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use jsonc_parser::ast;

/// How many `extends` hops are followed from one config.
const MAX_EXTENDS_HOPS: usize = 10;

/// The file names that are read as configs.
const CONFIG_NAMES: &[&str] = &["tsconfig.json", "jsconfig.json"];

/// The settings of one config that module resolution needs, with `extends` applied.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TsConfig {
    /// The `baseUrl` as a repository-relative folder, resolved against the file that
    /// set it. The root folder is the empty string.
    pub base_url: Option<String>,
    /// `paths` patterns in file order. Targets are repository-relative paths, resolved
    /// against `base_url`, or else against the folder of the file that set them.
    pub paths: Patterns,
    /// Every config file read for this one, extended ones included (relative paths).
    pub sources: Vec<String>,
}

/// All configs of a worktree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TsConfigs {
    /// Keyed by the relative folder of a `tsconfig.json` or `jsconfig.json`; the root
    /// folder is the empty string.
    pub by_dir: BTreeMap<String, TsConfig>,
    /// What was not read and why, one line each.
    pub notes: Vec<String>,
}

/// `paths` patterns with their targets, in file order.
type Patterns = Vec<(String, Vec<String>)>;

/// `paths` as one file wrote it: targets are not resolved yet, because `baseUrl` can
/// come from a different file in the chain.
#[derive(Debug, Clone, Default)]
struct Raw {
    base_url: Option<String>,
    /// The patterns and the folder of the file that set them.
    paths: Option<(Patterns, String)>,
    sources: Vec<String>,
}

/// Loads every `tsconfig.json` and `jsconfig.json` in `known` (relative paths of the
/// walked files) from under `root`. Relative `extends` values are followed up to 10
/// hops; a package name in `extends` is not fetched and adds a line to `notes`.
pub fn load_tsconfigs(root: &Path, known: &BTreeSet<String>) -> TsConfigs {
    let mut out = TsConfigs::default();
    for rel in known {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        if !CONFIG_NAMES.contains(&name) {
            continue;
        }
        let dir = dir_of(rel).to_string();
        // A folder with both files uses the tsconfig.
        if name == "jsconfig.json" && known.contains(&rel.replace("jsconfig.json", "tsconfig.json"))
        {
            continue;
        }
        let mut stack = Vec::new();
        let Some(raw) = load_raw(root, rel, &mut stack, &mut out.notes) else {
            continue;
        };
        out.by_dir.insert(dir, finish(raw));
    }
    out
}

/// Reads one config and its `extends` chain. `stack` holds the files being read, which
/// stops a loop and bounds the depth.
fn load_raw(
    root: &Path,
    rel: &str,
    stack: &mut Vec<String>,
    notes: &mut Vec<String>,
) -> Option<Raw> {
    if stack.len() > MAX_EXTENDS_HOPS || stack.iter().any(|s| s == rel) {
        return None;
    }
    let text = fs::read_to_string(root.join(rel)).ok()?;
    // The AST keeps the order of `paths`, which the patterns are tried in; a serde
    // value would sort the keys.
    let parsed = jsonc_parser::parse_to_ast(&text, &Default::default(), &Default::default());
    let Some(ast::Value::Object(config)) = parsed.ok().and_then(|p| p.value) else {
        notes.push(format!(
            "{rel} is not valid JSON, so Harbormap does not read it"
        ));
        return None;
    };
    let dir = dir_of(rel).to_string();
    stack.push(rel.to_string());

    let mut raw = Raw {
        sources: vec![rel.to_string()],
        ..Raw::default()
    };
    // `extends` is a string, or a list of strings in TypeScript 5; later ones win.
    let extends: Vec<&str> = match config.get("extends").map(|p| &p.value) {
        Some(ast::Value::StringLit(s)) => vec![s.value.as_ref()],
        Some(ast::Value::Array(a)) => a
            .elements
            .iter()
            .filter_map(|v| v.as_string_lit().map(|s| s.value.as_ref()))
            .collect(),
        _ => Vec::new(),
    };
    for target in extends {
        if !(target.starts_with("./") || target.starts_with("../")) {
            notes.push(format!(
                "{rel} extends {target}, which Harbormap does not read"
            ));
            continue;
        }
        let Some(mut parent_rel) = join_norm(&dir, target) else {
            notes.push(format!(
                "{rel} extends {target}, which is outside the repository"
            ));
            continue;
        };
        if !parent_rel.ends_with(".json") {
            parent_rel.push_str(".json");
        }
        if let Some(parent) = load_raw(root, &parent_rel, stack, notes) {
            overlay(&mut raw, parent);
        }
    }

    let options = config.get_object("compilerOptions");
    if let Some(base) = options.and_then(|o| o.get_string("baseUrl")) {
        raw.base_url = join_norm(&dir, &base.value);
    }
    if let Some(paths) = options.and_then(|o| o.get_object("paths")) {
        let patterns = paths
            .properties
            .iter()
            .map(|prop| {
                let targets = match &prop.value {
                    ast::Value::Array(a) => a
                        .elements
                        .iter()
                        .filter_map(|v| v.as_string_lit().map(|s| s.value.to_string()))
                        .collect(),
                    _ => Vec::new(),
                };
                (prop.name.as_str().to_string(), targets)
            })
            .collect();
        raw.paths = Some((patterns, dir));
    }
    stack.pop();
    Some(raw)
}

/// Applies `parent` under `raw`: what `raw` already set stays, and `sources` are merged.
/// `paths` is replaced as a whole, not merged by pattern, as TypeScript does.
fn overlay(raw: &mut Raw, parent: Raw) {
    if parent.base_url.is_some() {
        raw.base_url = parent.base_url;
    }
    if parent.paths.is_some() {
        raw.paths = parent.paths;
    }
    for source in parent.sources {
        if !raw.sources.contains(&source) {
            raw.sources.push(source);
        }
    }
}

/// Resolves the `paths` targets against `baseUrl`, or else the folder that set them.
fn finish(raw: Raw) -> TsConfig {
    let mut paths = Vec::new();
    if let Some((patterns, set_in)) = &raw.paths {
        let anchor = raw.base_url.as_deref().unwrap_or(set_in);
        for (pattern, targets) in patterns {
            // A target that climbs above the root can match nothing, so it is dropped.
            let targets = targets
                .iter()
                .filter_map(|t| join_norm(anchor, t))
                .collect();
            paths.push((pattern.clone(), targets));
        }
    }
    TsConfig {
        base_url: raw.base_url,
        paths,
        sources: raw.sources,
    }
}

/// The folder part of a relative path; the root folder is the empty string.
fn dir_of(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Joins `rel` to the folder `dir` and normalises `.` and `..` lexically. `None` when
/// the result climbs above the root.
pub(crate) fn join_norm(dir: &str, rel: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in dir.split('/').chain(rel.split('/')) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    Some(parts.join("/"))
}
