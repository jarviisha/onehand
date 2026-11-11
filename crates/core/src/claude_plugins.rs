//! Claude Code's plugins, as the Workbench's Plugins mode manages them: what
//! its command line reports and changes ([`cli`]), what an installed plugin
//! carries ([`inventory`]), and what happens next for one mode ([`drive`]).
//!
//! These are Claude Code's plugins. onehand has no plugin system of its own.

pub mod cli;
pub mod drive;
pub mod inventory;
