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
}
