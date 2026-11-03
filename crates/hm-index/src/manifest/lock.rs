//! Reads the versions a lockfile pins: `package-lock.json` and `npm-shrinkwrap.json`
//! (v1 to v3), `pnpm-lock.yaml` (v5 to v9) and `yarn.lock` (v1 and Berry).
//!
//! Formats: <https://docs.npmjs.com/cli/v11/configuring-npm/package-lock-json>. pnpm and
//! yarn files are read line by line, as there is no YAML crate; both tools write their
//! files with a fixed layout.

use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;

/// What a failed read says when the file is not in a layout this reader knows.
const NOT_RECOGNISED: &str = "lockfile format not recognised";

/// Version prefixes that name a local package, not a published version.
const LOCAL_PREFIXES: &[&str] = &["link:", "workspace:", "file:"];

/// The `dependencies`, `devDependencies` and `optionalDependencies` sections of a
/// pnpm importer.
const PNPM_SECTIONS: &[&str] = &["dependencies", "devDependencies", "optionalDependencies"];

/// name -> pinned version; `None` for a local package.
type Versions = BTreeMap<String, Option<String>>;

/// The versions one lockfile pins.
#[derive(Debug, Clone)]
pub struct LockIndex {
    kind: Kind,
}

#[derive(Debug, Clone)]
enum Kind {
    /// npm v2/v3 `packages` keyed by `.../node_modules/<name>`, or the v1 `dependencies`
    /// tree (top level only) when there is no `packages`.
    Npm {
        packages: Versions,
        legacy: Versions,
    },
    /// pnpm: importer folder -> name -> version.
    Pnpm {
        importers: BTreeMap<String, Versions>,
    },
    /// yarn: `name@range` key -> version.
    Yarn { versions: HashMap<String, String> },
}

/// Reads `text` as the lockfile at `rel`; the file's name picks the format. An `Err` is a
/// short reason, such as `lockfile format not recognised`.
pub fn parse_lockfile(rel: &str, text: &str) -> Result<LockIndex, String> {
    let file = rel.rsplit('/').next().unwrap_or(rel);
    let kind = match file {
        "package-lock.json" | "npm-shrinkwrap.json" => parse_npm(text)?,
        "pnpm-lock.yaml" => parse_pnpm(text)?,
        "yarn.lock" => parse_yarn(text)?,
        _ => return Err(NOT_RECOGNISED.to_string()),
    };
    Ok(LockIndex { kind })
}

impl LockIndex {
    /// The version pinned for `name`, declared as `declared`, in the manifest at
    /// `importer`: the path from the lockfile's folder to the manifest's folder, `.`
    /// when they are the same. Peer suffixes are dropped (`16.0.10(react@19)` is
    /// `16.0.10`); a local package (`link:`, `workspace:`, `file:`) has no version.
    pub fn resolved(&self, importer: &str, name: &str, declared: &str) -> Option<String> {
        match &self.kind {
            Kind::Npm { packages, legacy } => {
                let nested = format!("{importer}/node_modules/{name}");
                let hoisted = format!("node_modules/{name}");
                let keys = [(importer != ".").then_some(nested), Some(hoisted)];
                for key in keys.into_iter().flatten() {
                    if let Some(found) = packages.get(&key) {
                        return found.clone();
                    }
                }
                legacy.get(name).cloned().flatten()
            }
            Kind::Pnpm { importers } => importers.get(importer)?.get(name).cloned().flatten(),
            Kind::Yarn { versions } => {
                if LOCAL_PREFIXES.iter().any(|p| declared.starts_with(p)) {
                    return None;
                }
                let plain = format!("{name}@{declared}");
                let berry = format!("{name}@npm:{declared}");
                versions
                    .get(&plain)
                    .or_else(|| versions.get(&berry))
                    .and_then(|v| clean_version(v))
            }
        }
    }
}

/// Drops quotes, peer suffixes (`(react@19)` in pnpm v6+, `_react@19` before) and a
/// trailing comment; a local package gives `None`.
fn clean_version(raw: &str) -> Option<String> {
    let v = raw.trim().trim_matches(['\'', '"']).trim();
    if v.is_empty() || LOCAL_PREFIXES.iter().any(|p| v.starts_with(p)) {
        return None;
    }
    let end = v.find(['(', '_', ' ']).unwrap_or(v.len());
    (end > 0).then(|| v[..end].to_string())
}

#[derive(Deserialize)]
struct NpmLock {
    #[serde(rename = "lockfileVersion")]
    lockfile_version: Option<serde_json::Value>,
    packages: Option<BTreeMap<String, NpmPackage>>,
    dependencies: Option<BTreeMap<String, NpmPackage>>,
}

#[derive(Deserialize)]
struct NpmPackage {
    version: Option<serde_json::Value>,
}

impl NpmPackage {
    fn version(&self) -> Option<String> {
        self.version.as_ref()?.as_str().and_then(clean_version)
    }
}

fn parse_npm(text: &str) -> Result<Kind, String> {
    let lock: NpmLock = match serde_json::from_str(text) {
        Ok(lock) => lock,
        Err(e) if e.is_syntax() || e.is_eof() => {
            return Err(format!("not valid JSON (line {})", e.line()));
        }
        Err(_) => return Err(NOT_RECOGNISED.to_string()),
    };
    if lock.lockfile_version.is_none() {
        return Err(NOT_RECOGNISED.to_string());
    }
    // The workspace entries themselves (`apps/web`, `""`) are not dependencies.
    let packages: Versions = lock
        .packages
        .into_iter()
        .flatten()
        .filter(|(key, _)| key.contains("node_modules/"))
        .map(|(key, pkg)| (key, pkg.version()))
        .collect();
    let legacy: Versions = lock
        .dependencies
        .into_iter()
        .flatten()
        .map(|(key, pkg)| (key, pkg.version()))
        .collect();
    // A v2 file holds both; `packages` is the one npm reads.
    let legacy = if packages.is_empty() {
        legacy
    } else {
        Versions::new()
    };
    Ok(Kind::Npm { packages, legacy })
}

/// Splits `key: rest` of a YAML line. A key in single or double quotes may hold `:` and
/// `@` (`'@shop/ui':`). `rest` has its trailing comment removed.
fn split_key(content: &str) -> Option<(String, &str)> {
    let (key, after) = match content.chars().next()? {
        quote @ ('\'' | '"') => {
            let inner = &content[1..];
            let end = inner.find(quote)?;
            (
                inner[..end].to_string(),
                inner[end + 1..].strip_prefix(':')?,
            )
        }
        _ => {
            let at = content.find(':')?;
            (content[..at].to_string(), &content[at + 1..])
        }
    };
    let rest = after.split(" #").next().unwrap_or("").trim();
    Some((key, rest))
}

fn parse_pnpm(text: &str) -> Result<Kind, String> {
    #[derive(PartialEq, Clone, Copy)]
    enum Top {
        Other,
        Importers,
        // v5 and the v6 files of a project without workspaces write the dependency
        // sections at the top, in the shape of one importer.
        Single,
    }
    let mut saw_version = false;
    let mut importers: BTreeMap<String, Versions> = BTreeMap::new();
    let mut top = Top::Other;
    let mut importer: Option<String> = None;
    let mut active = false;
    let mut pending: Option<String> = None;

    for line in text.lines() {
        let content = line.trim_start();
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        let indent = line.len() - content.len();
        let Some((key, rest)) = split_key(content.trim_end()) else {
            continue;
        };
        if indent == 0 {
            top = Top::Other;
            importer = None;
            active = false;
            pending = None;
            match key.as_str() {
                "lockfileVersion" => saw_version = true,
                "importers" => top = Top::Importers,
                k if PNPM_SECTIONS.contains(&k) => {
                    top = Top::Single;
                    importer = Some(".".to_string());
                    importers.entry(".".to_string()).or_default();
                    active = true;
                }
                _ => {}
            }
            continue;
        }
        // Indents: importers 2 (folder), 4 (section), 6 (name), 8 (field); a top-level
        // section has the name at 2 and the field at 4.
        let (name_indent, in_importers) = match top {
            Top::Other => continue,
            Top::Importers => (6, true),
            Top::Single => (2, false),
        };
        if in_importers && indent == 2 {
            importers.entry(key.clone()).or_default();
            importer = Some(key);
            active = false;
            pending = None;
        } else if in_importers && indent == 4 {
            active = PNPM_SECTIONS.contains(&key.as_str());
            pending = None;
        } else if indent == name_indent && active {
            if rest.is_empty() {
                pending = Some(key);
            } else {
                pending = None;
                // v5 writes `name: version` on one line.
                insert_first(&mut importers, &importer, key, rest);
            }
        } else if indent == name_indent + 2
            && key == "version"
            && let Some(name) = pending.take()
        {
            insert_first(&mut importers, &importer, name, rest);
        }
    }
    if !saw_version {
        return Err(NOT_RECOGNISED.to_string());
    }
    Ok(Kind::Pnpm { importers })
}

fn insert_first(
    importers: &mut BTreeMap<String, Versions>,
    importer: &Option<String>,
    name: String,
    raw: &str,
) {
    if let Some(map) = importer.as_ref().and_then(|i| importers.get_mut(i)) {
        map.entry(name).or_insert_with(|| clean_version(raw));
    }
}

fn parse_yarn(text: &str) -> Result<Kind, String> {
    let mut recognised = false;
    let mut versions: HashMap<String, String> = HashMap::new();
    let mut keys: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Some(comment) = line.strip_prefix('#') {
            recognised |= comment.contains("yarn lockfile v1");
        } else if !line.starts_with(' ') {
            keys.clear();
            if line.trim_end() == "__metadata:" {
                recognised = true;
            } else if let Some(header) = line.trim_end().strip_suffix(':') {
                // v1 quotes each key, Berry quotes the whole joined header.
                keys.extend(
                    header
                        .split(',')
                        .map(|k| k.trim().trim_matches('"').trim().to_string())
                        .filter(|k| !k.is_empty()),
                );
            }
        } else if !keys.is_empty() && line.starts_with("  ") && !line.starts_with("   ") {
            let Some(rest) = line.trim().strip_prefix("version") else {
                continue;
            };
            if !rest.starts_with([' ', ':']) {
                continue;
            }
            let version = rest.trim_start_matches(':').trim().trim_matches('"');
            for key in &keys {
                versions.insert(key.clone(), version.to_string());
            }
        }
    }
    if !recognised && versions.is_empty() {
        return Err(NOT_RECOGNISED.to_string());
    }
    Ok(Kind::Yarn { versions })
}
