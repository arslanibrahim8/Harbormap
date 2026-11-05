//! TypeScript, TSX and JavaScript support: extraction, tsconfig loading and
//! module resolution.

pub mod extract;
pub mod resolve;
pub mod tsconfig;
pub mod web;

pub use extract::extract;
