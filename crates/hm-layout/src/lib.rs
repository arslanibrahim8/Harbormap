//! The Learn map layout: groups files folder-first, counts arrows between groups and
//! precomputes positions, so the browser never lays anything out (Learn spec §10.3,
//! v0.2 spec §3–§5.3).

pub mod input;
pub mod map;
pub mod roles;
#[doc(hidden)]
pub mod synth;
pub mod tree;

pub use input::{InFile, InLink, Input};
pub use map::Map;
pub use roles::{Role, role_for, role_word};
pub use tree::{GROUP_BUDGET, HUB_IN_DEGREE, Node, NodeKind, OVERVIEW_BUDGET, Tree};
