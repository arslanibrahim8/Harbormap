//! What the layout reads: the files on the map and the links between them. The
//! caller (the server, from `hm_index::view`) builds it from the facts store; the
//! layout knows nothing about SQLite.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The files on the map and the counted links between them.
///
/// `files` is sorted by path and has no duplicates; `links` has at most one entry per
/// `(src, dst)` pair and never `src == dst`. The layout reads links through
/// [`Input::clean_links`], which drops links that break these rules and adds up
/// duplicates.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Input {
    pub files: Vec<InFile>,
    pub links: Vec<InLink>,
}

/// One file on the map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InFile {
    /// Repository-relative, `/`-separated.
    pub path: String,
    /// Lines of code.
    pub loc: u32,
    /// The file is produced by a tool rather than written by hand.
    pub generated: bool,
}

/// `src` uses `dst`; both are indexes into [`Input::files`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct InLink {
    pub src: u32,
    pub dst: u32,
    /// How many references the link stands for (at least 1).
    pub refs: u32,
}

impl Input {
    /// The links the layout uses: both ends in range and different, one entry per
    /// `(src, dst)` with the `refs` of duplicates added up, sorted by `(src, dst)`.
    pub fn clean_links(&self) -> Vec<InLink> {
        let n = self.files.len();
        let mut refs: BTreeMap<(u32, u32), u32> = BTreeMap::new();
        for l in &self.links {
            if l.src != l.dst && (l.src as usize) < n && (l.dst as usize) < n {
                let r = refs.entry((l.src, l.dst)).or_default();
                *r = r.saturating_add(l.refs);
            }
        }
        refs.into_iter()
            .map(|((src, dst), refs)| InLink { src, dst, refs })
            .collect()
    }
}
