//! What happens next for the recents list: a workspace opened, bound,
//! named or forgotten, and the list written after.
//!
//! **Written whole, in order.** Every change answers one write of the list
//! as it stands ([`Effect::Write`]), and the front end runs its writes one
//! after another, so an older copy never lands over a newer one. Nothing is
//! put back when a write fails: the list is a convenience, and the next
//! change writes it again.
//!
//! **Only an unbind's failure is said.** A workspace unbound and then still
//! remembered comes back at the next launch, so a forget that did not land
//! is told to the person ([`Signal::ForgetFailed`]); any other write that
//! fails is only logged ([`Signal::WriteFailed`]).

use super::AppState;
use std::ops::Deref;
use std::path::{Path, PathBuf};

/// Why a workspace is forgotten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forgot {
    /// Its storage was unbound: it must not be reopened at the next launch.
    Unbound,
    /// Its file is gone.
    Gone,
}

/// What a person is told, or the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// The write that forgot an unbound workspace failed: why. Said, every
    /// time: the workspace would come back at the next launch.
    ForgetFailed(String),
    /// Another write of the list failed: why. Only logged.
    WriteFailed(String),
}

/// What the front end carries out.
pub enum Effect {
    /// Write `state` to `path` after every write before it, and report
    /// [`Input::Written`] with `pending`. A late answer is **recorded**: the
    /// file holds that copy or not, whatever changed since.
    Write {
        pending: u64,
        state: AppState,
        path: PathBuf,
    },
    Signal(Signal),
}

/// What the front end reports back.
pub enum Input {
    /// The write `pending` landed, or failed.
    Written {
        pending: u64,
        result: Result<(), String>,
    },
}

/// The recents' drive. See the module header.
pub struct RecentsDrive {
    state: AppState,
    path: PathBuf,
    minted: u64,
    /// The writes in flight whose failure is said.
    said: Vec<u64>,
}

impl Deref for RecentsDrive {
    type Target = AppState;

    fn deref(&self) -> &AppState {
        &self.state
    }
}

impl RecentsDrive {
    /// The recents remembered between launches, written back where they
    /// were read.
    pub fn load() -> Self {
        Self::new(AppState::load(), AppState::path())
    }

    /// `state`, written to `path`.
    pub fn new(state: AppState, path: PathBuf) -> Self {
        Self {
            state,
            path,
            minted: 0,
            said: Vec::new(),
        }
    }

    /// The workspace in `dir` was opened in a window: it is the most recent.
    #[must_use = "its effects are to be carried"]
    pub fn opened(&mut self, dir: PathBuf) -> Vec<Effect> {
        self.state.touch(dir);
        vec![self.write(false)]
    }

    /// The workspace now stored in `dir` is called `name`: the most recent,
    /// and named so, in one write.
    #[must_use = "its effects are to be carried"]
    pub fn bound(&mut self, dir: PathBuf, name: &str) -> Vec<Effect> {
        self.state.touch(dir.clone());
        self.state.set_name(&dir, name);
        vec![self.write(false)]
    }

    /// The workspace in `dir` is known by `name`; written only when that is
    /// new of a workspace remembered.
    #[must_use = "its effects are to be carried"]
    pub fn named(&mut self, dir: &Path, name: &str) -> Vec<Effect> {
        if !self.state.set_name(dir, name) {
            return Vec::new();
        }
        vec![self.write(false)]
    }

    /// Forget the workspace in `dir`, for `why`.
    #[must_use = "its effects are to be carried"]
    pub fn forgot(&mut self, dir: &Path, why: Forgot) -> Vec<Effect> {
        self.state.forget(dir);
        let said = match why {
            Forgot::Unbound => true,
            Forgot::Gone => false,
        };
        vec![self.write(said)]
    }

    /// What the front end reports back.
    #[must_use = "its effects are to be carried"]
    pub fn handle(&mut self, input: Input) -> Vec<Effect> {
        match input {
            Input::Written { pending, result } => {
                let said = self.said.contains(&pending);
                self.said.retain(|p| *p != pending);
                match result {
                    Ok(()) => Vec::new(),
                    Err(why) if said => vec![Effect::Signal(Signal::ForgetFailed(why))],
                    Err(why) => vec![Effect::Signal(Signal::WriteFailed(why))],
                }
            }
        }
    }

    fn write(&mut self, said: bool) -> Effect {
        self.minted += 1;
        if said {
            self.said.push(self.minted);
        }
        Effect::Write {
            pending: self.minted,
            state: self.state.clone(),
            path: self.path.clone(),
        }
    }
}

#[cfg(test)]
mod tests;
