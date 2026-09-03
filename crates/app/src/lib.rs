//! onehand — a desktop GUI host for AI coding agents, on GPUI.
//!
//! Split lib + thin binary so the logic stays reachable from tests without
//! opening a window.
//!
//! **Only what `main.rs` needs is `pub`.** Everything else is private, and that
//! is load-bearing rather than tidiness: rustc's `dead_code` analysis stops at
//! a `pub` item in a library, because something outside the crate might use it.
//! Nothing outside this crate ever will — there is one binary and it needs two
//! names. While these modules were all `pub`, a field written and never read
//! looked exactly like a working feature to the compiler, which is how a rail
//! badge, a persisted layout and a session state machine each shipped as dead
//! code across four phases. Tests live inside the crate, so
//! they are unaffected. Keep new modules private.

mod acp;
pub mod assets;
mod chat;
mod controls;
mod dialogs;
#[cfg(test)]
mod guards;
mod icons;
mod plugins;
mod rail;
mod remote;
pub mod shell;
mod state;
mod statusbar;
mod terminal;
mod theme;
mod workbench;
mod zoom;
