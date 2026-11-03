//! What the layout reads: the files on the map and the links between them. The
//! caller (the server, from `hm_index::view`) builds it from the facts store; the
//! layout knows nothing about SQLite.

use serde::{Deserialize, Serialize};

/// The files on the map and the counted links between them.
///
/// `files` is sorted by path and has no duplicates; `links` has at most one entry per
/// `(src, dst)` pair and never `src == dst`. Links that break these rules are ignored.
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
