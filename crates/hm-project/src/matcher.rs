//! Signature matching (v0.1 spec §8). Signatures are tried in priority order and the
//! first match wins; `when` blocks are alternatives, their conditions all must hold.

use std::collections::BTreeSet;

use hm_core::model::Confidence;
use hm_core::names::{basename, exe_stem};
use hm_core::signature::{Fixture, Signature, SignatureSet, When};

/// What is known about one process when matching.
#[derive(Debug, Clone, Default)]
pub struct ProcFacts {
    /// Stem of the executable path, or of the process name when the path is unknown.
    pub exe: String,
    /// Stem of the process name.
    pub name: String,
    pub argv: Vec<String>,
    /// Executable stems of the ancestors, nearest first.
    pub ancestors: Vec<String>,
    pub cwd_files: Vec<String>,
    pub packages: BTreeSet<String>,
    /// "macos", "linux" or "windows".
    pub os: String,
}

impl ProcFacts {
    /// Facts for a signature fixture, as the pack tests use them.
    pub fn from_fixture(fixture: &Fixture, os: &str) -> Self {
        let exe = fixture.exe.as_deref().map(exe_stem).unwrap_or_default();
        ProcFacts {
            name: exe.clone(),
            exe,
            argv: fixture.argv.clone(),
            ancestors: fixture.ancestors.iter().map(|a| exe_stem(a)).collect(),
            cwd_files: fixture.cwd_files.clone(),
            packages: fixture.packages.iter().map(|p| p.to_lowercase()).collect(),
            os: os.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Match<'s> {
    pub signature: &'s Signature,
    pub confidence: Confidence,
}

pub fn match_process<'s>(set: &'s SignatureSet, facts: &ProcFacts) -> Option<Match<'s>> {
    set.iter()
        .filter(|signature| os_allows(signature, &facts.os))
        .find_map(|signature| {
            signature
                .when
                .iter()
                .filter(|when| when.image.is_empty())
                .find(|when| process_matches(when, facts))
                .map(|when| Match {
                    signature,
                    confidence: confidence(when),
                })
        })
}

pub fn match_image<'s>(set: &'s SignatureSet, image: &str, os: &str) -> Option<Match<'s>> {
    let name = image_name(image);
    set.iter()
        .filter(|signature| os_allows(signature, os))
        .find_map(|signature| {
            signature
                .when
                .iter()
                .filter(|when| !when.image.is_empty())
                .find(|when| {
                    when.image
                        .iter()
                        .any(|candidate| candidate.eq_ignore_ascii_case(&name))
                })
                .map(|when| Match {
                    signature,
                    confidence: confidence(when),
                })
        })
}

/// `docker.io/bitnami/postgresql:16@sha256:...` → `postgresql`.
pub fn image_name(image: &str) -> String {
    let without_digest = image.split('@').next().unwrap_or(image);
    let last = without_digest.rsplit('/').next().unwrap_or(without_digest);
    last.split(':').next().unwrap_or(last).to_ascii_lowercase()
}

fn os_allows(signature: &Signature, os: &str) -> bool {
    signature.os.as_deref().is_none_or(|only| only == os)
}

fn confidence(when: &When) -> Confidence {
    if when.probably {
        Confidence::Probably
    } else {
        Confidence::Sure
    }
}

fn process_matches(when: &When, facts: &ProcFacts) -> bool {
    let exe_ok = when.exe.is_empty()
        || when
            .exe
            .iter()
            .any(|pattern| glob(pattern, &facts.exe) || glob(pattern, &facts.name));
    let basename_ok = when.argv_basename.is_empty()
        || facts.argv.iter().any(|arg| {
            let base = script_stem(arg);
            when.argv_basename
                .iter()
                .any(|wanted| wanted.eq_ignore_ascii_case(&base))
        });
    let contains_ok = when.argv_contains.is_empty()
        || facts.argv.iter().any(|arg| {
            when.argv_contains
                .iter()
                .any(|needle| arg.contains(needle.as_str()))
        });
    let prefix_ok = when.argv_prefix.is_empty()
        || facts.argv.iter().any(|arg| {
            when.argv_prefix
                .iter()
                .any(|prefix| arg.starts_with(prefix.as_str()))
        });
    let ancestor_ok = when.ancestor_exe.is_empty()
        || facts.ancestors.iter().any(|ancestor| {
            when.ancestor_exe
                .iter()
                .any(|pattern| glob(pattern, ancestor))
        });
    let files_ok = when.cwd_files.is_empty()
        || facts
            .cwd_files
            .iter()
            .any(|file| when.cwd_files.iter().any(|pattern| glob(pattern, file)));
    let package_ok = when.package.is_empty()
        || when
            .package
            .iter()
            .any(|package| facts.packages.contains(&package.to_lowercase()));
    exe_ok && basename_ok && contains_ok && prefix_ok && ancestor_ok && files_ok && package_ok
}

/// Basename without a script or executable extension: `vite.js` → `vite`.
pub(crate) fn script_stem(arg: &str) -> String {
    let base = basename(arg).to_ascii_lowercase();
    for ext in [".js", ".cjs", ".mjs", ".ts", ".py", ".exe", ".cmd"] {
        if let Some(stem) = base.strip_suffix(ext) {
            return stem.to_string();
        }
    }
    base
}

/// Case-insensitive match with one `*` wildcard at the start or the end.
fn glob(pattern: &str, value: &str) -> bool {
    let (pattern, value) = (pattern.to_ascii_lowercase(), value.to_ascii_lowercase());
    if let Some(suffix) = pattern.strip_prefix('*') {
        value.ends_with(suffix)
    } else if let Some(prefix) = pattern.strip_suffix('*') {
        value.starts_with(prefix)
    } else {
        pattern == value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_names_drop_registry_tag_and_digest() {
        assert_eq!(image_name("postgres:16-alpine"), "postgres");
        assert_eq!(image_name("docker.io/bitnami/postgresql:16"), "postgresql");
        assert_eq!(image_name("localhost:5000/team/api@sha256:abc"), "api");
        assert_eq!(image_name("Redis"), "redis");
    }

    #[test]
    fn globs_support_one_wildcard() {
        assert!(glob("python*", "python3.13"));
        assert!(glob("*.csproj", "Shop.Api.csproj"));
        assert!(glob("node", "Node"));
        assert!(!glob("node", "nodemon"));
    }

    #[test]
    fn script_stems_ignore_extensions_and_separators() {
        assert_eq!(script_stem(r"C:\app\node_modules\vite\bin\vite.js"), "vite");
        assert_eq!(script_stem("manage.py"), "manage");
        assert_eq!(script_stem("vite:dev"), "vite:dev");
    }
}
