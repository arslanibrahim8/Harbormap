//! Content, signature, body, interface and facts hashes (Learn spec §5.3). All are
//! BLAKE3; the short ones keep the first 32 hex characters.

use crate::facts::{ExportFact, FileFacts, ImportFact};
use tree_sitter::Node;

/// Separates the tokens of a body hash.
const TOKEN_SEP: &[u8] = b"\x1f";

/// BLAKE3 over the raw bytes, 64 hex characters.
pub fn content_hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn short(hash: blake3::Hash) -> String {
    hash.to_hex()[..32].to_string()
}

/// BLAKE3 over `parts` separated by NUL bytes, 32 hex characters.
pub fn hash_parts(parts: &[&str]) -> String {
    let mut hasher = blake3::Hasher::new();
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            hasher.update(b"\0");
        }
        hasher.update(part.as_bytes());
    }
    short(hasher.finalize())
}

/// The hash of nothing, for a symbol without a body.
pub(crate) fn empty_body_hash() -> String {
    short(blake3::Hasher::new().finalize())
}

/// BLAKE3 over the leaf tokens of `node`, comments skipped, string literals kept,
/// tokens joined with `\u{1f}`. Formatting and comments do not change it.
pub fn body_hash(node: Node, src: &[u8]) -> String {
    let mut hasher = blake3::Hasher::new();
    let mut first = true;
    hash_tokens(node, src, &mut hasher, &mut first);
    short(hasher.finalize())
}

fn hash_tokens(node: Node, src: &[u8], hasher: &mut blake3::Hasher, first: &mut bool) {
    if node.kind() == "comment" {
        return;
    }
    if node.child_count() == 0 {
        let token = &src[node.byte_range()];
        if token.is_empty() {
            return;
        }
        if !*first {
            hasher.update(TOKEN_SEP);
        }
        *first = false;
        hasher.update(token);
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        hash_tokens(child, src, hasher, first);
    }
}

fn export_key(e: &ExportFact) -> String {
    format!(
        "{}\u{1f}{}\u{1f}{}\u{1f}{}",
        e.name,
        e.local.as_deref().unwrap_or(""),
        e.from.as_deref().unwrap_or(""),
        e.star
    )
}

fn import_key(i: &ImportFact) -> String {
    let mut names: Vec<String> = i
        .names
        .iter()
        .map(|n| format!("{}>{}", n.imported, n.local))
        .collect();
    names.sort();
    format!("{}\u{1f}{}", i.specifier, names.join(","))
}

fn finish(mut keys: Vec<String>) -> String {
    keys.sort();
    let parts: Vec<&str> = keys.iter().map(String::as_str).collect();
    hash_parts(&parts)
}

/// What other files can see of this one: the signatures of its exported symbols
/// (and of the methods of its exported classes) and its export facts. A body edit
/// does not change it.
pub fn interface_hash(f: &FileFacts) -> String {
    let exported_classes: Vec<&str> = f
        .symbols
        .iter()
        .filter(|s| s.exported && s.owner.is_empty())
        .map(|s| s.name.as_str())
        .collect();
    let mut keys: Vec<String> = Vec::new();
    for s in &f.symbols {
        let visible = s.exported || exported_classes.contains(&s.owner.as_str());
        if !visible {
            continue;
        }
        let name = if s.default_export { "default" } else { &s.name };
        keys.push(format!(
            "s\u{1f}{}\u{1f}{}\u{1f}{}",
            s.owner, name, s.sig_hash
        ));
    }
    keys.extend(
        f.exports
            .iter()
            .map(|e| format!("e\u{1f}{}", export_key(e))),
    );
    finish(keys)
}

/// Everything about the file's structure that is not a body: symbol signatures,
/// imports and exports. It never includes `body_hash`.
pub fn facts_hash(f: &FileFacts) -> String {
    let mut keys: Vec<String> = Vec::new();
    for s in &f.symbols {
        keys.push(format!(
            "s\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
            s.owner,
            s.name,
            s.kind.as_str(),
            s.sig_hash
        ));
    }
    keys.extend(
        f.imports
            .iter()
            .map(|i| format!("i\u{1f}{}", import_key(i))),
    );
    keys.extend(
        f.exports
            .iter()
            .map(|e| format!("e\u{1f}{}", export_key(e))),
    );
    finish(keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_have_the_documented_lengths() {
        assert_eq!(content_hash(b"abc").len(), 64);
        assert_eq!(hash_parts(&["a", "b"]).len(), 32);
        assert_eq!(empty_body_hash().len(), 32);
    }

    #[test]
    fn parts_are_separated() {
        assert_ne!(hash_parts(&["ab", "c"]), hash_parts(&["a", "bc"]));
        assert_eq!(hash_parts(&["a", "b"]), hash_parts(&["a", "b"]));
    }
}
