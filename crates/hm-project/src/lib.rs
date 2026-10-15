//! Turns a raw inventory into a snapshot: which project and worktree each process
//! and container belongs to, what each program is (signatures), which services
//! exist and who owns every listening port.

pub mod git;
pub mod manifest;
pub mod matcher;
pub mod paths;
pub mod resolve;
pub mod world;

pub use resolve::{ResolveInput, build_snapshot};
pub use world::{FixtureWorld, RepoInfo, Scenario, World};
