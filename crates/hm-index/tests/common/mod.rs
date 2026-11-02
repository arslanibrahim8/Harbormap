//! Helpers shared by the integration tests; later tasks add to it.
#![allow(dead_code)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The folder of fixture `name` under `tests/fixtures`.
pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// The text of a fixture file; `rel` is relative to `tests/fixtures`, as in
/// `ts-basic/src/db.ts`.
pub fn read_fixture(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// A copy of fixture `name` in a temporary folder that is not a git repository. Every
/// file on disk comes along, including the ignored and skipped ones.
pub fn copy_fixture(name: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create a temporary directory");
    copy_tree(&fixture(name), dir.path()).expect("copy the fixture");
    dir
}

/// A resolver for fixture `name`: the walked files are the known set and every
/// tsconfig in it is loaded.
pub fn resolver_for(name: &str) -> hm_index::ts::resolve::ModuleResolver {
    let root = fixture(name);
    let walked = hm_index::walk::walk(&root).expect("walk the fixture");
    let known: std::collections::BTreeSet<String> =
        walked.files.iter().map(|f| f.rel.clone()).collect();
    let tsconfigs = hm_index::ts::tsconfig::load_tsconfigs(&root, &known);
    hm_index::ts::resolve::ModuleResolver::new(known, tsconfigs)
}

/// What `named_edges_for` found, with ids mapped back to readable names.
#[derive(Debug, Default)]
pub struct Named {
    /// (kind, source, destination), as `path` or `path#Owner.name`.
    pub edges: std::collections::BTreeSet<(String, String, String)>,
    /// (file, kind, text, status).
    pub refs: std::collections::BTreeSet<(String, String, String, String)>,
}

/// Extracts every parsed file of fixture `name`, resolves each one and names the ids.
pub fn named_edges_for(name: &str) -> Named {
    use hm_core::ids::file_id;
    use hm_index::lang::Lang;
    use hm_index::query::display_name;
    use hm_index::ts::resolve::{Project, resolve_file, symbol_ids};
    use std::collections::{BTreeMap, HashMap};

    let root = fixture(name);
    let modules = resolver_for(name);
    let walked = hm_index::walk::walk(&root).expect("walk the fixture");
    let mut facts = BTreeMap::new();
    for f in &walked.files {
        let Some(lang) = f.lang.filter(Lang::parsed_in_l1) else {
            continue;
        };
        let src = fs::read_to_string(&f.abs).expect("read a fixture file");
        facts.insert(f.rel.clone(), hm_index::ts::extract(&src, lang));
    }

    let mut names: HashMap<String, String> = HashMap::new();
    for (rel, file_facts) in &facts {
        names.insert(file_id(rel), display_name(rel, None));
        for (id, sym) in symbol_ids(rel, file_facts)
            .into_iter()
            .zip(&file_facts.symbols)
        {
            names.insert(id, display_name(rel, Some(sym)));
        }
    }
    let name_of = |id: &str| names.get(id).cloned().unwrap_or_else(|| format!("?{id}"));

    let project = Project {
        facts: &facts,
        modules: &modules,
    };
    let mut out = Named::default();
    for rel in facts.keys() {
        let resolved = resolve_file(&project, rel);
        for e in resolved.edges {
            out.edges
                .insert((e.kind.into(), name_of(&e.src_id), name_of(&e.dst_id)));
        }
        for r in resolved.refs {
            out.refs
                .insert((rel.clone(), r.kind.into(), r.text, r.status.into()));
        }
    }
    out
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            fs::create_dir_all(&target)?;
            copy_tree(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}
