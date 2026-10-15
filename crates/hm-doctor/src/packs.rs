//! Signature packs compiled into the binary and validated on load. A broken entry is
//! skipped with a warning; it never stops Harbormap.

use std::collections::HashSet;

use hm_core::signature::{Level, Signature, SignatureSet};

pub const SIGNATURE_FILES: &[(&str, &str)] = &[
    (
        "signatures/web.toml",
        include_str!("../../../packs/signatures/web.toml"),
    ),
    (
        "signatures/servers.toml",
        include_str!("../../../packs/signatures/servers.toml"),
    ),
    (
        "signatures/data.toml",
        include_str!("../../../packs/signatures/data.toml"),
    ),
    (
        "signatures/system.toml",
        include_str!("../../../packs/signatures/system.toml"),
    ),
    (
        "signatures/tools.toml",
        include_str!("../../../packs/signatures/tools.toml"),
    ),
];

#[derive(Debug, Clone)]
pub struct Packs {
    pub signatures: SignatureSet,
    pub warnings: Vec<String>,
}

pub fn load_embedded() -> Packs {
    let mut warnings = Vec::new();
    let mut signatures = Vec::new();
    for (file, text) in SIGNATURE_FILES {
        signatures.extend(parse_signatures(file, text, &mut warnings));
    }
    let signatures = drop_duplicate_ids(signatures, &mut warnings);
    Packs {
        signatures: SignatureSet::new(signatures),
        warnings,
    }
}

/// Parses one signature file entry by entry, so one bad entry does not hide the rest.
pub fn parse_signatures(file: &str, text: &str, warnings: &mut Vec<String>) -> Vec<Signature> {
    let table: toml::Table = match toml::from_str(text) {
        Ok(table) => table,
        Err(error) => {
            warnings.push(format!("{file}: {error}"));
            return Vec::new();
        }
    };
    let Some(entries) = table.get("signature").and_then(toml::Value::as_array) else {
        warnings.push(format!("{file}: no [[signature]] entries"));
        return Vec::new();
    };
    let mut signatures = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let label = entry
            .get("id")
            .and_then(toml::Value::as_str)
            .unwrap_or("?")
            .to_string();
        let parsed = entry
            .clone()
            .try_into::<Signature>()
            .map_err(|error| error.to_string())
            .and_then(|signature| validate(&signature).map(|()| signature));
        match parsed {
            Ok(signature) => signatures.push(signature),
            Err(problem) => warnings.push(format!(
                "{file}: signature #{} ({label}) skipped: {}",
                index + 1,
                problem.trim()
            )),
        }
    }
    signatures
}

fn validate(signature: &Signature) -> Result<(), String> {
    let id_ok = !signature.id.is_empty()
        && signature
            .id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !id_ok {
        return Err("id must use lowercase letters, digits and dashes".into());
    }
    if signature.name.trim().is_empty() {
        return Err("name is empty".into());
    }
    if signature.when.is_empty() || signature.when.iter().any(|when| when.is_empty()) {
        return Err("every `when` block needs at least one condition".into());
    }
    if signature.fixture.is_empty() {
        return Err("at least one fixture is required".into());
    }
    for level in [Level::Beginner, Level::Builder, Level::Expert] {
        if signature.texts.get(level).trim().is_empty() {
            return Err(format!("the {level:?} text is empty"));
        }
    }
    if let Some(os) = &signature.os
        && !["macos", "linux", "windows"].contains(&os.as_str())
    {
        return Err("os must be macos, linux or windows".into());
    }
    Ok(())
}

fn drop_duplicate_ids(signatures: Vec<Signature>, warnings: &mut Vec<String>) -> Vec<Signature> {
    let mut seen = HashSet::new();
    signatures
        .into_iter()
        .filter(|signature| {
            let fresh = seen.insert(signature.id.clone());
            if !fresh {
                warnings.push(format!("duplicate signature id {} skipped", signature.id));
            }
            fresh
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_packs_load_without_warnings() {
        let packs = load_embedded();
        assert!(packs.warnings.is_empty(), "{:#?}", packs.warnings);
        assert!(
            packs.signatures.len() >= 30,
            "only {} signatures",
            packs.signatures.len()
        );
    }

    #[test]
    fn a_broken_entry_is_skipped_with_a_warning() {
        let text = r#"
[[signature]]
id = "good"
name = "Good"
role = "work"
kind = "dev_server"
[[signature.when]]
exe = ["good"]
[signature.texts]
beginner = "b"
builder = "b"
expert = "e"
[[signature.fixture]]
exe = "good"

[[signature]]
id = "Bad Id"
name = "Bad"
role = "work"
kind = "dev_server"
[[signature.when]]
exe = ["bad"]
[signature.texts]
beginner = "b"
builder = "b"
expert = "e"
[[signature.fixture]]
exe = "bad"

[[signature]]
id = "typo"
name = "Typo"
role = "work"
kind = "dev_server"
[[signature.when]]
exee = ["typo"]
[signature.texts]
beginner = "b"
builder = "b"
expert = "e"
"#;
        let mut warnings = Vec::new();
        let signatures = parse_signatures("test.toml", text, &mut warnings);
        assert_eq!(signatures.len(), 1);
        assert_eq!(signatures[0].id, "good");
        assert_eq!(warnings.len(), 2, "{warnings:#?}");
        assert!(warnings[0].contains("Bad Id"));
        assert!(warnings[1].contains("typo"));
    }

    #[test]
    fn duplicate_ids_keep_the_first_entry() {
        let packs = load_embedded();
        let mut signatures: Vec<Signature> = packs.signatures.iter().cloned().collect();
        signatures.push(signatures[0].clone());
        let mut warnings = Vec::new();
        let kept = drop_duplicate_ids(signatures, &mut warnings);
        assert_eq!(kept.len(), packs.signatures.len());
        assert_eq!(warnings.len(), 1);
    }
}
