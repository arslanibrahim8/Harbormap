//! Resolves TypeScript and JavaScript import specifiers to files of the worktree,
//! bundler style: relative paths, `paths`, `baseUrl`, extension and index probing.

use std::collections::BTreeSet;

use super::tsconfig::{TsConfig, TsConfigs, join_norm};

/// The extensions tried when a path has no exact match, in this order.
const EXTENSIONS: &[&str] = &[".ts", ".tsx", ".d.ts", ".js", ".jsx", ".mjs", ".cjs"];

/// Where a specifier points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleTarget {
    /// A file of the worktree, as a relative path.
    File(String),
    /// A package outside the worktree, by package name.
    External(String),
    /// Looks like it belongs to the worktree, but no file matches.
    Unresolved,
}

/// Resolves specifiers against the files of a worktree and its tsconfigs.
#[derive(Debug, Clone)]
pub struct ModuleResolver {
    known: BTreeSet<String>,
    tsconfigs: TsConfigs,
}

impl ModuleResolver {
    /// `known` is the set of relative file paths that exist in the worktree.
    pub fn new(known: BTreeSet<String>, tsconfigs: TsConfigs) -> Self {
        Self { known, tsconfigs }
    }

    /// Every tsconfig file read, extended ones included. The resolver must be rebuilt
    /// when one of them changes.
    pub fn config_sources(&self) -> BTreeSet<String> {
        self.tsconfigs
            .by_dir
            .values()
            .flat_map(|c| c.sources.iter().cloned())
            .collect()
    }

    /// Resolves `specifier` as imported from the file `from_rel`.
    pub fn resolve(&self, from_rel: &str, specifier: &str) -> ModuleTarget {
        let from_dir = from_rel.rsplit_once('/').map_or("", |(dir, _)| dir);

        // 1. Relative to the importing file. A relative specifier that matches
        // nothing is unresolved, never external.
        if is_relative(specifier) {
            return self.probe_or_unresolved(join_norm(from_dir, specifier));
        }
        // An absolute path points outside the worktree's own naming.
        if specifier.starts_with('/') {
            return ModuleTarget::Unresolved;
        }

        let config = self.nearest_config(from_dir);
        if let Some(config) = config {
            // 2. `paths`, in file order. A prefix the project claims is not a package.
            let mut claimed = false;
            for (pattern, targets) in &config.paths {
                let Some(captured) = match_pattern(pattern, specifier) else {
                    continue;
                };
                claimed = true;
                for target in targets {
                    let path = target.replacen('*', captured, 1);
                    if let Some(file) = self.probe(&path) {
                        return ModuleTarget::File(file);
                    }
                }
            }
            if claimed {
                return ModuleTarget::Unresolved;
            }
            // 3. `baseUrl`.
            if let Some(base) = &config.base_url
                && let Some(file) = join_norm(base, specifier).and_then(|p| self.probe(&p))
            {
                return ModuleTarget::File(file);
            }
        }

        // 4. A package.
        ModuleTarget::External(package_name(specifier).to_string())
    }

    /// The config of the nearest folder at or above `dir` that has one.
    fn nearest_config(&self, mut dir: &str) -> Option<&TsConfig> {
        loop {
            if let Some(config) = self.tsconfigs.by_dir.get(dir) {
                return Some(config);
            }
            if dir.is_empty() {
                return None;
            }
            dir = dir.rsplit_once('/').map_or("", |(parent, _)| parent);
        }
    }

    fn probe_or_unresolved(&self, path: Option<String>) -> ModuleTarget {
        match path.and_then(|p| self.probe(&p)) {
            Some(file) => ModuleTarget::File(file),
            None => ModuleTarget::Unresolved,
        }
    }

    /// Finds the file `path` names: exact, with an extension, or as a folder index.
    /// A `.js` path also tries the TypeScript file it is compiled from.
    fn probe(&self, path: &str) -> Option<String> {
        if path.is_empty() {
            return self.probe_index("");
        }
        if self.known.contains(path) {
            return Some(path.to_string());
        }
        if let Some(stem) = path.strip_suffix(".js") {
            for ext in [".ts", ".tsx"] {
                let candidate = format!("{stem}{ext}");
                if self.known.contains(&candidate) {
                    return Some(candidate);
                }
            }
        }
        for ext in EXTENSIONS {
            let candidate = format!("{path}{ext}");
            if self.known.contains(&candidate) {
                return Some(candidate);
            }
        }
        self.probe_index(path)
    }

    fn probe_index(&self, dir: &str) -> Option<String> {
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        EXTENSIONS
            .iter()
            .map(|ext| format!("{prefix}index{ext}"))
            .find(|candidate| self.known.contains(candidate))
    }
}

fn is_relative(specifier: &str) -> bool {
    specifier == "."
        || specifier == ".."
        || specifier.starts_with("./")
        || specifier.starts_with("../")
}

/// What the `*` of `pattern` captured in `specifier`; the empty string when the pattern
/// has no `*` and equals the specifier. `None` when they do not match.
fn match_pattern<'a>(pattern: &str, specifier: &'a str) -> Option<&'a str> {
    match pattern.split_once('*') {
        None => (pattern == specifier).then_some(""),
        Some((prefix, suffix)) => specifier.strip_prefix(prefix)?.strip_suffix(suffix),
    }
}

/// The package a bare specifier names: its first segment, or the first two when it
/// starts with `@`.
fn package_name(specifier: &str) -> &str {
    let mut end = specifier.len();
    let mut segments = 0;
    let wanted = if specifier.starts_with('@') { 2 } else { 1 };
    for (i, c) in specifier.char_indices() {
        if c == '/' {
            segments += 1;
            if segments == wanted {
                end = i;
                break;
            }
        }
    }
    &specifier[..end]
}
