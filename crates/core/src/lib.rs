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
pub mod connector;
pub mod diff;
pub mod editor;
pub mod gitstat;
pub mod instance;
pub mod issues;
pub mod preflight;
pub mod process;
pub mod remote;
pub mod task;
pub mod tree;
pub mod unattended;
pub mod workflow;
pub mod workspace;
pub mod worktree;

/// `12s` / `1m 12s`: how long something took, at the coarseness it is read
/// at. Seconds alone up to a minute; past it, a bare count of seconds is a
/// number the reader has to divide before it means anything.
pub fn duration(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        _ => format!("{}m {}s", secs / 60, secs % 60),
    }
}

/// `3m ago` / `2h ago` / `5d ago`: how long before `now` the moment `then` was,
/// both in seconds since the epoch.
pub fn rel_time(now: u64, then: u64) -> String {
    let secs = now.saturating_sub(then);
    match secs {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86_399 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_duration_reads_in_seconds_then_minutes() {
        assert_eq!(super::duration(0), "0s");
        assert_eq!(super::duration(12), "12s");
        assert_eq!(super::duration(59), "59s");
        assert_eq!(super::duration(60), "1m 0s");
        assert_eq!(super::duration(72), "1m 12s");
    }
}
