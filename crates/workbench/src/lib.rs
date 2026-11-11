//! How a Workbench mode is mounted in GPUI: the trait it implements and how
//! it asks the panel for things. What a mode is apart from that (its name,
//! its declaration, where it is offered and the requests between it and the
//! panel) is core's, `onehand_core::workbench`, re-exported here.
//!
//! A mode owns its state and its view; the panel owns neither. It keeps the
//! list, remembers which one is showing, draws the strip that switches between
//! them, and passes on the handful of things the shell asks of "the Workbench"
//! without knowing which mode answers.
//!
//! Deliberately not a versioned ABI. Every mode is a Rust trait implemented in
//! this workspace and compiled into this binary, so cargo is the version check
//! and the compiler is the capability check.

// Nothing here is `pub` unless the binary names it: `dead_code` stops at a
// `pub` item in a library, so one that lost its last caller looks exactly like
// a working feature.
#![warn(unreachable_pub)]

use gpui::{AnyView, App, Window};
/// Where a mode's files are, re-exported so a mode that reads none of them
/// still names the type its `set_root` is handed without depending on core.
pub use onehand_core::disk::Disk;
pub use onehand_core::workbench::{
    ModeId, Request, TERMINAL_KEY_CONTEXT, WORKBENCH_KEY_CONTEXT, WorkbenchModeSpec,
};
use std::path::Path;
use std::rc::Rc;

/// One Workbench mode.
///
/// Everything past [`Self::spec`] and [`Self::view`] has a default that does
/// nothing, which is what keeps a mode down to the part of this that it
/// actually has an answer for — the file tree has no opinion about saving and
/// the quick editor none about a terminal's font size.
pub trait WorkbenchMode {
    /// The ID, the word on the strip, and the two facts the panel would
    /// otherwise have to work out by matching that ID against a list.
    fn spec(&self) -> WorkbenchModeSpec;

    /// The body, drawn while this mode is the one showing.
    fn view(&self) -> AnyView;

    /// Point this mode at a project root, and the disk its files are on.
    ///
    /// Every mode here is per root, so this is not a request that one of them
    /// takes and the rest ignore — it is told to all of them, and the reply is
    /// nothing because there is nothing to decide. A mode that touches no
    /// files ignores the disk.
    fn set_root(&mut self, _root: &Path, _disk: &Disk, _cx: &mut App) {}

    /// Drop everything held for a root that has left the workspace.
    fn forget_root(&mut self, _root: &Path, _cx: &mut App) {}

    /// Open a file, and say whether this mode was the one that edits files.
    ///
    /// A method rather than a broadcast [`Request`] for the reason
    /// [`Self::focus`] is: building a buffer needs a `Window`, and a request
    /// can arrive from a path that has none. [`Request::OpenFile`] is the same
    /// thing travelling the other way — what a mode raises through its [`Ask`]
    /// when a click inside it means a file should be opened.
    fn open_file(&mut self, _path: &Path, _window: &mut Window, _cx: &mut App) -> bool {
        false
    }

    /// Put the caret where this mode's work happens, and say whether there was
    /// anywhere to put it.
    ///
    /// A method rather than a [`Request`] because it is the one thing a mode is
    /// asked that needs a `Window`, and requests arrive from paths that have
    /// none — a `git status` sweep landing, a turn ending. `false` is the
    /// honest answer from a mode that is clicked rather than typed into, and
    /// the panel then takes focus itself.
    fn focus(&self, _window: &mut Window, _cx: &mut App) -> bool {
        false
    }

    /// Answer a [`Request`], and say whether this mode was the one that did.
    ///
    /// Broadcast to every mode in the panel's order, because the shell asks the
    /// *Workbench* to save, to start, to reap — it has no business knowing
    /// which mode owns a buffer or a child process. The `bool` is what lets the
    /// panel act on the answer where the answer matters: a mode that answers
    /// [`Request::Reap`] is one whose child has just stopped existing, which is
    /// what decides whether the caret has to be moved out of it.
    fn handle(&mut self, _request: &Request<'_>, _cx: &mut App) -> bool {
        false
    }

    /// How many of `root`'s open files hold edits that closing it would throw
    /// away, which the shell asks before it removes a project.
    fn unsaved(&self, _root: &Path, _cx: &App) -> usize {
        0
    }

    /// The processes this mode keeps running, each with what a person calls
    /// it: what the shell asks before it removes a folder one may work in.
    fn processes(&self, _cx: &App) -> Vec<onehand_core::worktree::removal::Process> {
        Vec::new()
    }
}

/// How a mode reaches back into the panel.
///
/// Handed to a mode when it is built rather than set afterwards, so there is no
/// window in which a mode is on screen holding nothing to ask through.
pub type Ask = Rc<dyn Fn(&Request<'_>, &mut Window, &mut App)>;
