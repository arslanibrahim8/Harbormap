//! The Learn indexer: walks a worktree, parses source files with tree-sitter and
//! writes the facts that the store keeps (Learn spec §5).

pub mod facts;
pub mod hash;
pub mod lang;
pub mod query;
pub mod ts;
pub mod walk;
pub mod watch;
