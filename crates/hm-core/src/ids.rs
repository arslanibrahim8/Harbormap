//! Stable IDs derived from content, never from counters (v0.1 spec §3).

use std::path::Path;

/// `p_` + 12 hex characters of BLAKE3 over the canonical project root.
pub fn project_id(canonical_root: &Path) -> String {
    let hash = blake3::hash(canonical_root.to_string_lossy().as_bytes());
    format!("p_{}", &hash.to_hex()[..12])
}

/// `s_` + 12 hex characters of BLAKE3 over project, worktree and service name, so a
/// service keeps its ID across restarts.
pub fn service_id(project_id: Option<&str>, worktree: Option<&Path>, name: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(project_id.unwrap_or("").as_bytes());
    hasher.update(&[0]);
    if let Some(worktree) = worktree {
        hasher.update(worktree.to_string_lossy().as_bytes());
    }
    hasher.update(&[0]);
    hasher.update(name.as_bytes());
    format!("s_{}", &hasher.finalize().to_hex()[..12])
}

/// `e_` + 12 hex characters of BLAKE3 over the engine's own ID (or its endpoint when
/// the engine reports none), so two sockets that reach one daemon share an ID.
pub fn engine_id(key: &str) -> String {
    format!("e_{}", &blake3::hash(key.as_bytes()).to_hex()[..12])
}

/// `f_` + 12 hex characters of BLAKE3 over the repository-relative path (always `/`
/// separated), so a file keeps its ID on every machine and checkout location.
pub fn file_id(rel_path: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"file\0");
    hasher.update(rel_path.as_bytes());
    format!("f_{}", &hasher.finalize().to_hex()[..12])
}

/// `y_` + 12 hex characters of BLAKE3 over the file ID, owner, name, kind and
/// disambiguator, NUL-separated. It holds no line number, so editing above a symbol
/// keeps its ID.
pub fn symbol_id(
    file_id: &str,
    owner: &str,
    name: &str,
    kind: &str,
    disambiguator: &str,
) -> String {
    let mut hasher = blake3::Hasher::new();
    for (i, part) in [file_id, owner, name, kind, disambiguator]
        .iter()
        .enumerate()
    {
        if i > 0 {
            hasher.update(&[0]);
        }
        hasher.update(part.as_bytes());
    }
    format!("y_{}", &hasher.finalize().to_hex()[..12])
}

/// `w_` + 12 hex characters of BLAKE3 over the canonical worktree root; it names the
/// folder that holds that worktree's Learn store.
pub fn worktree_key(canonical_root: &Path) -> String {
    let hash = blake3::hash(canonical_root.to_string_lossy().as_bytes());
    format!("w_{}", &hash.to_hex()[..12])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_stable_and_distinct() {
        let a = project_id(Path::new("/work/shop"));
        assert_eq!(a, project_id(Path::new("/work/shop")));
        assert_ne!(a, project_id(Path::new("/work/blog")));
        assert!(a.starts_with("p_") && a.len() == 14);

        let web = service_id(Some(&a), Some(Path::new("/work/shop")), "web");
        assert_eq!(
            web,
            service_id(Some(&a), Some(Path::new("/work/shop")), "web")
        );
        assert_ne!(
            web,
            service_id(Some(&a), Some(Path::new("/work/shop-wt")), "web")
        );
        assert_ne!(
            web,
            service_id(Some(&a), Some(Path::new("/work/shop")), "api")
        );
        assert!(web.starts_with("s_") && web.len() == 14);
    }

    #[test]
    fn learn_ids_are_relative_and_stable() {
        let f = file_id("src/orders/service.ts");
        assert_eq!(f, file_id("src/orders/service.ts"));
        assert_ne!(f, file_id("src/orders/mail.ts"));
        assert!(f.starts_with("f_") && f.len() == 14);
        let y = symbol_id(&f, "OrderService", "find", "method", "");
        assert_eq!(y, symbol_id(&f, "OrderService", "find", "method", ""));
        assert_ne!(
            y,
            symbol_id(&f, "OrderService", "find", "method", "sig:abc")
        );
        assert_ne!(y, symbol_id(&f, "", "find", "method", ""));
        assert!(y.starts_with("y_") && y.len() == 14);
        let w = worktree_key(Path::new("/work/shop"));
        assert!(w.starts_with("w_") && w != worktree_key(Path::new("/work/shop-wt")));
    }
}
