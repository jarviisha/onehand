//! onehand-core — the GUI-free half of onehand.
//!
//! Everything here is pure logic: config parsing, the workspace tree, `@`/`/`
//! completion, `path:line:col` token parsing, `git status` parsing, the branch
//! and folder rules behind splitting a project into a worktree, and the
//! bounded directory-tree flatten. No module in this crate may depend on a GUI
//! framework — that invariant is what kept this half intact through a whole
//! front-end rewrite, and what would let it survive another.

pub mod acp;
pub mod agent;
pub mod attachment;
pub mod chat;
pub mod completion;
pub mod config;
pub mod diff;
pub mod editor;
pub mod gitstat;
pub mod parse;
pub mod remote;
pub mod tree;
pub mod workspace;
pub mod worktree;
