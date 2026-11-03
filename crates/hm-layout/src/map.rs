//! The map the server holds: the group tree of the current facts plus the links it
//! counts arrows from. This is the skeleton; positions, arrows and views come on top
//! of it (L5a plan, Task 3), and `build` already takes the previous map so cached
//! positions can be reused.

use crate::input::{InLink, Input};
use crate::tree::{self, Tree};

/// One build of the map. Two builds of the same input compare equal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Map {
    tree: Tree,
    /// The input links as the tree reads them ([`Input::clean_links`]).
    links: Vec<InLink>,
}

impl Map {
    /// Builds the map of `input`; the root box is named `project_name`. `previous` is
    /// the map of the last build in this run (not used yet).
    pub fn build(input: &Input, project_name: &str, previous: Option<&Map>) -> Map {
        let _ = previous;
        Map {
            tree: tree::build(input, project_name),
            links: input.clean_links(),
        }
    }

    /// The group tree.
    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    /// The links the map was built from: valid, one per `(src, dst)`, refs added up,
    /// sorted by `(src, dst)`.
    pub fn links(&self) -> &[InLink] {
        &self.links
    }
}
