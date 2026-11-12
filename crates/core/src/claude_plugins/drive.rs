//! What happens next for one Plugins mode: when the list is read, one change
//! at a time, what a change that went through means for the agent on screen,
//! and a refusal said in the project it was made in.
//!
//! **One per mode**, as the mode is one per window: the project it lists is
//! the one the panel points it at, and every command runs `claude` there.
//!
//! **A listing is read when it is drawn.** Being drawn is what says it is
//! worth reading, so the front end asks for it as it draws ([`PluginsDrive::list_if_due`]),
//! and never while a change is out: the change's own landing asks for one,
//! and a listing read halfway through it is already stale.
//!
//! **What a change came to is only certain from what is listed after it**:
//! the command line's answer is a sentence. Every change that lands asks for
//! the list again.
//!
//! The drive reads no clock: the time a change lands is told it, and so is
//! when the agent on screen started.

use super::cli::{Catalog, Change, Scope, Verb};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// How many changes are kept to count from. Far past how many anybody makes
/// between two restarts.
const MADE_CAP: usize = 256;

/// One change that went through: where it was made, whether it reaches every
/// project, and when.
struct Made {
    root: PathBuf,
    /// A change at the global scope, which every project's agent loads.
    everywhere: bool,
    at: Instant,
}

/// What the front end carries out.
pub enum Effect {
    /// Read what is installed for `root` and what the marketplaces offer
    /// ([`super::cli::list_blocking`]), off the UI loop, and report
    /// [`Input::Listed`] with `token`. A late answer is **dropped** by its
    /// token: a newer read, or the project moving on, supersedes it.
    List { root: PathBuf, token: u64 },
    /// Make `change` in `root` ([`super::cli::change_blocking`]), off the UI
    /// loop, and report [`Input::Changed`]. A late answer is **recorded**:
    /// the change was made or it was not, whichever project is on screen.
    Change { root: PathBuf, change: Change },
}

/// What the front end reports back.
pub enum Input {
    /// What read `token` found.
    Listed {
        token: u64,
        listed: Result<Catalog, String>,
    },
    /// What `change`, made in `root`, came to, landing at `now`.
    Changed {
        root: PathBuf,
        change: Change,
        done: Result<(), String>,
        now: crate::Now,
    },
}

/// One Plugins mode's drive. See the module header.
pub struct PluginsDrive {
    /// The project the list is of, and every change runs in.
    root: Option<PathBuf>,
    /// What is installed there, or why it could not be read.
    catalog: Option<Result<Catalog, String>>,
    /// The list is to be read again the next time it is drawn.
    stale: bool,
    /// The newest read's token.
    reading: u64,
    /// The one change on its way.
    busy: Option<Change>,
    /// Why the last change was refused, while its project is on screen.
    refused: Option<String>,
    /// The scope the last install was made at, which the next one takes.
    install_scope: Scope,
    /// Every change that went through, newest last.
    made: Vec<Made>,
    /// When the agent on screen started, or `None` while none is running.
    since: Option<Instant>,
}

impl Default for PluginsDrive {
    fn default() -> Self {
        Self {
            root: None,
            catalog: None,
            stale: false,
            reading: 0,
            busy: None,
            refused: None,
            install_scope: Scope::User,
            made: Vec::new(),
            since: None,
        }
    }
}

impl PluginsDrive {
    /// The project the list is of.
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// What is installed there, or why it could not be read; `None` until a
    /// read lands.
    pub fn catalog(&self) -> Option<&Result<Catalog, String>> {
        self.catalog.as_ref()
    }

    /// The change on its way, if one is.
    pub fn busy(&self) -> Option<&Change> {
        self.busy.as_ref()
    }

    /// Why the last change in this project was refused.
    pub fn refused(&self) -> Option<&str> {
        self.refused.as_deref()
    }

    /// The scope the next install is made at.
    pub fn install_scope(&self) -> Scope {
        self.install_scope
    }

    /// How many changes the agent on screen has not loaded: those made since
    /// it started, in this project or globally. None while no agent is
    /// running, since a session started next loads all of them. Counts what
    /// went through this mode alone: a plugin installed from a terminal is
    /// not seen here.
    pub fn pending(&self) -> usize {
        let (Some(root), Some(since)) = (&self.root, self.since) else {
            return 0;
        };
        self.made
            .iter()
            .filter(|made| made.at > since && (made.everywhere || made.root == *root))
            .count()
    }

    /// List `root`. Another project's list and refusal go: they were about
    /// that project.
    pub fn set_root(&mut self, root: &Path) {
        if self.root.as_deref() == Some(root) {
            return;
        }
        self.root = Some(root.to_path_buf());
        self.catalog = None;
        self.refused = None;
        self.stale = true;
        // A read on its way is about the project before.
        self.reading += 1;
    }

    /// `root` has left the workspace. A change on its way there still lands
    /// and is recorded.
    pub fn forget_root(&mut self, root: &Path) {
        if self.root.as_deref() != Some(root) {
            return;
        }
        self.root = None;
        self.catalog = None;
        self.refused = None;
        // A read on its way is about a project that is gone.
        self.reading += 1;
    }

    /// The agent on screen started at `since`, or none is running.
    pub fn agent_started(&mut self, since: Option<Instant>) {
        self.since = since;
    }

    /// Something other than this mode may have changed what is installed (a
    /// terminal, or the agent itself during a turn): read the list again the
    /// next time it is drawn.
    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    /// Read the list again from nothing, after a read that failed.
    pub fn retry(&mut self) {
        self.catalog = None;
        self.stale = true;
    }

    /// The list is about to be drawn: read it if it is due.
    #[must_use = "its effects are to be carried"]
    pub fn list_if_due(&mut self) -> Vec<Effect> {
        let Some(root) = self
            .root
            .clone()
            .filter(|_| self.stale && self.busy.is_none())
        else {
            return Vec::new();
        };
        self.stale = false;
        self.reading += 1;
        vec![Effect::List {
            root,
            token: self.reading,
        }]
    }

    /// Make `change` in the project listed, unless one is on its way: there
    /// is one at a time, and a press meanwhile makes nothing. An install
    /// sets the scope the next one takes.
    #[must_use = "its effects are to be carried"]
    pub fn change(&mut self, change: Change) -> Vec<Effect> {
        if self.busy.is_some() {
            return Vec::new();
        }
        let Some(root) = self.root.clone() else {
            return Vec::new();
        };
        if change.verb == Verb::Install {
            self.install_scope = change.scope;
        }
        self.busy = Some(change.clone());
        self.refused = None;
        vec![Effect::Change { root, change }]
    }

    /// What the world reports back.
    #[must_use = "its effects are to be carried"]
    pub fn handle(&mut self, input: Input) -> Vec<Effect> {
        match input {
            Input::Listed { token, listed } => {
                if token == self.reading {
                    self.catalog = Some(listed);
                }
                Vec::new()
            }
            Input::Changed {
                root,
                change,
                done,
                now,
            } => {
                self.busy = None;
                match done {
                    // Fetching a catalog changes nothing an agent loads.
                    Ok(()) if change.verb == Verb::CheckUpdates => {}
                    Ok(()) => {
                        self.made.push(Made {
                            // A move out of the global scope takes it away
                            // from every project, as much as one into it
                            // adds it.
                            everywhere: change.scope == Scope::User
                                || change.verb == Verb::Move(Scope::User),
                            root,
                            at: now.at,
                        });
                        let over = self.made.len().saturating_sub(MADE_CAP);
                        self.made.drain(..over);
                    }
                    Err(why) if self.root.as_ref() == Some(&root) => self.refused = Some(why),
                    Err(_) => {}
                }
                self.stale = true;
                Vec::new()
            }
        }
    }
}

#[cfg(test)]
mod tests;
