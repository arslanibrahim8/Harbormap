//! `package.json` manifests and the lockfiles beside them: what each project declares and
//! which version its lockfile pins (Learn spec §5.2). Route detection reads it to know
//! whether a folder is a Next.js app, and the entry rules read `bin` and `main`.

mod lock;
mod npm;

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::Path;

pub use lock::{LockIndex, parse_lockfile};
pub use npm::{Dependency, NpmManifest, parse_package_json};

use crate::hash::content_hash;
use npm::dir_of;

/// The lockfile names, in the order they are preferred when one folder holds several.
pub const LOCKFILES: &[&str] = &[
    "pnpm-lock.yaml",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
];

/// Files larger than this are not read. The walker leaves a big lockfile unparsed, but
/// the versions in it are still wanted.
const MAX_READ_BYTES: u64 = 64 * 1024 * 1024;

/// What earlier `Manifests::load` calls parsed, keyed by relative path, so a file whose
/// bytes did not change is not parsed again.
#[derive(Default)]
pub struct ManifestCache {
    /// path -> (BLAKE3 of the bytes, parsed file); `lockfile` and `resolved` are not set.
    npm: HashMap<String, (String, NpmManifest)>,
    locks: HashMap<String, (String, Result<LockIndex, String>)>,
    parses: usize,
}

impl ManifestCache {
    /// How many files this cache has parsed since it was made. For tests.
    #[doc(hidden)]
    pub fn parses(&self) -> usize {
        self.parses
    }

    /// The parsed `package.json` at `rel`; `Err` is why it could not be read.
    fn manifest(&mut self, root: &Path, rel: &str) -> Result<NpmManifest, String> {
        let (hash, text) = match read_hashed(root, rel) {
            Ok(read) => read,
            Err(why) => {
                self.npm.remove(rel);
                return Err(why);
            }
        };
        if let Some((_, parsed)) = self.npm.get(rel).filter(|(old, _)| *old == hash) {
            return Ok(parsed.clone());
        }
        let parsed = parse_package_json(rel, &text);
        self.parses += 1;
        self.npm.insert(rel.to_string(), (hash, parsed.clone()));
        Ok(parsed)
    }

    /// Makes sure the lockfile at `rel` is parsed and cached. `Err` is why it is not
    /// usable.
    fn ensure_lock(&mut self, root: &Path, rel: &str) -> Result<(), String> {
        let (hash, text) = match read_hashed(root, rel) {
            Ok(read) => read,
            Err(why) => {
                self.locks.remove(rel);
                return Err(why);
            }
        };
        if let Some((_, parsed)) = self.locks.get(rel).filter(|(old, _)| *old == hash) {
            return parsed.as_ref().map(|_| ()).map_err(Clone::clone);
        }
        let parsed = parse_lockfile(rel, &text);
        self.parses += 1;
        let verdict = parsed.as_ref().map(|_| ()).map_err(Clone::clone);
        self.locks.insert(rel.to_string(), (hash, parsed));
        verdict
    }
}

/// BLAKE3 of the bytes of `rel` under `root`, and its text; `Err` is a short reason for
/// a note.
fn read_hashed(root: &Path, rel: &str) -> Result<(String, String), String> {
    let path = root.join(rel);
    let len = fs::metadata(&path)
        .map_err(|e| format!("could not be read: {e}"))?
        .len();
    if len > MAX_READ_BYTES {
        return Err("is larger than 64 MiB, not read".to_string());
    }
    let bytes = fs::read(&path).map_err(|e| format!("could not be read: {e}"))?;
    Ok((
        content_hash(&bytes),
        String::from_utf8_lossy(&bytes).into_owned(),
    ))
}

/// Every `package.json` of a worktree, with versions from its lockfile.
#[derive(Debug, Default)]
pub struct Manifests {
    /// Sorted by path.
    pub npm: Vec<NpmManifest>,
    /// What could not be read, one line each, sorted: the notes of every manifest and of
    /// every lockfile that was used.
    pub notes: Vec<String>,
}

fn is_package_json(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    name == "package.json" && !rel.split('/').any(|part| part == "node_modules")
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// The nearest lockfile at or above `root`: the first folder going up that holds one,
/// and in it the first of [`LOCKFILES`].
fn lockfile_for(root: &str, known: &BTreeSet<String>) -> Option<String> {
    let mut dir = root;
    loop {
        for name in LOCKFILES {
            let candidate = join(dir, name);
            if known.contains(&candidate) {
                return Some(candidate);
            }
        }
        if dir.is_empty() {
            return None;
        }
        dir = dir_of(dir);
    }
}

/// The path from the lockfile's folder to the manifest's folder; `.` when the same.
fn importer_of(manifest_root: &str, lock_rel: &str) -> String {
    let lock_dir = dir_of(lock_rel);
    let inside = if lock_dir.is_empty() {
        manifest_root
    } else {
        manifest_root
            .strip_prefix(lock_dir)
            .map_or(manifest_root, |r| r.trim_start_matches('/'))
    };
    if inside.is_empty() {
        ".".to_string()
    } else {
        inside.to_string()
    }
}

impl Manifests {
    /// Reads every `package.json` in `known` (the walked relative paths, so never one
    /// under `node_modules`) from under `root`, and for each the nearest lockfile. A file
    /// whose bytes did not change since `cache` saw it is not parsed again. A problem is
    /// a line in `notes`, never an error.
    pub fn load(root: &Path, known: &BTreeSet<String>, cache: &mut ManifestCache) -> Manifests {
        cache.npm.retain(|rel, _| known.contains(rel));
        cache.locks.retain(|rel, _| known.contains(rel));

        let mut notes: BTreeSet<String> = BTreeSet::new();
        let mut npm: Vec<NpmManifest> = Vec::new();
        for rel in known.iter().filter(|r| is_package_json(r)) {
            match cache.manifest(root, rel) {
                Ok(mut manifest) => {
                    manifest.lockfile = lockfile_for(&manifest.root, known);
                    npm.push(manifest);
                }
                Err(why) => {
                    notes.insert(format!("{rel}: {why}"));
                }
            }
        }

        // Parse each lockfile once; a lockfile that fails is one note, not one per manifest.
        let wanted: BTreeSet<String> = npm.iter().filter_map(|m| m.lockfile.clone()).collect();
        for rel in &wanted {
            if let Err(why) = cache.ensure_lock(root, rel) {
                notes.insert(format!("{rel}: {why}"));
            }
        }
        for manifest in &mut npm {
            let Some(lock_rel) = manifest.lockfile.as_deref() else {
                continue;
            };
            if let Some((_, Ok(index))) = cache.locks.get(lock_rel) {
                let importer = importer_of(&manifest.root, lock_rel);
                for dep in &mut manifest.deps {
                    dep.resolved = index.resolved(&importer, &dep.name, &dep.declared);
                }
            }
        }

        notes.extend(npm.iter().flat_map(|m| m.notes.iter().cloned()));
        Manifests {
            npm,
            notes: notes.into_iter().collect(),
        }
    }

    /// The manifest of the nearest folder at or above `rel`: the longest root that holds
    /// the file.
    pub fn root_of(&self, rel: &str) -> Option<&NpmManifest> {
        self.npm
            .iter()
            .filter(|m| {
                m.root.is_empty()
                    || rel
                        .strip_prefix(m.root.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            })
            .max_by_key(|m| m.root.len())
    }

    /// True when the manifest that governs `rel` lists `package` in any section.
    pub fn declares(&self, rel: &str, package: &str) -> bool {
        self.root_of(rel)
            .is_some_and(|m| m.deps.iter().any(|d| d.name == package))
    }
}
