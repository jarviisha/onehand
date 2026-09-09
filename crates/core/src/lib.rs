//! onehand-core — the GUI-free half of onehand.
//!
//! Everything here is pure logic: config parsing, the workspace tree, `@`/`/`
//! completion, `git status` parsing, the branch and folder rules behind
//! splitting a project into a worktree, and the bounded directory-tree
//! flatten. No module in this crate may depend on a GUI framework — that
//! invariant is what kept this half intact through a whole front-end rewrite,
//! and what would let it survive another.
//!
//! **Nothing here is `pub` unless something outside this crate names it.**
//! rustc's `dead_code` analysis stops at a `pub` item in a library, because
//! something outside the crate might use it — and nothing outside this
//! workspace ever will. While every item here was `pub`, one that had lost its
//! last caller looked exactly like a working feature to the compiler, and
//! seventeen functions accumulated that way. `pub(crate)` is what puts them
//! back in the compiler's reach, so widening one is a decision to be made
//! rather than the default.
//!
//! The lint below is the cheap half of that and catches nothing today: every
//! module is `pub` and the app names all of them, so no item here is
//! unreachable from the crate root. It earns its line the first time a module
//! is made private and something inside it is left `pub`.

#![warn(unreachable_pub)]

pub mod acp;
pub mod agent;
pub mod attachment;
pub mod chat;
pub mod completion;
pub mod config;
pub mod diff;
pub mod editor;
pub mod gitstat;
pub mod remote;
pub mod tree;
pub mod workspace;
pub mod worktree;
