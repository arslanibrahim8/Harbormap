//! The Learn indexer: walks a worktree, parses source files with tree-sitter and
//! writes the facts that the store keeps (Learn spec §5).

pub mod facts;
pub mod go;
pub mod hash;
pub mod index;
pub mod lang;
pub mod location;
pub mod py;
pub mod query;
#[doc(hidden)]
pub mod synth;
pub mod ts;
pub mod walk;
pub mod watch;
