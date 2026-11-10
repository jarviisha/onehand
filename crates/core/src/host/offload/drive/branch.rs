//! An offloaded project on a branch: no key and no lock. Its host's sessions
//! work on `onehand/<host>` in a mirror of its own; the branch is fetched
//! here into a remote-tracking ref when it moved, and counted against `HEAD`
//! for the person to merge. The code goes up only when the person asks, as a
//! commit merged into the host's branch.

use super::{Effect, Errand, Moving, Offload, OffloadDrive};
use crate::workspace::HostName;
use std::path::{Path, PathBuf};

/// What is known here of an offloaded project's branch.
#[derive(Clone, Debug)]
pub struct BranchState {
    /// `onehand/<host>`.
    pub name: String,
    /// The commits the branch, as fetched, has that `HEAD` does not; `None`
    /// until first counted.
    pub ahead: Option<usize>,
    /// Why the last *Update host from laptop* did not land, until one does:
    /// a fetch after it says nothing of it.
    pub refused: Option<String>,
}

impl OffloadDrive {
    /// Start keeping `path`, offloaded to `host` on `branch`, and count what
    /// it has to merge. A folder already kept is left as it is.
    pub fn track_branch(&mut self, path: PathBuf, host: HostName, branch: String) -> Vec<Effect> {
        if self.kept_as_is(&path, false) {
            return Vec::new();
        }
        let state = BranchState {
            name: branch,
            ahead: None,
            refused: None,
        };
        let mut fx = self.keep(path.clone(), Offload::new(host, None, Some(state)));
        fx.extend(self.refresh(&path, false).unwrap_or_default());
        fx
    }

    /// Fetch `path`'s branch if its host moved it, then count what it has to
    /// merge; `None` when it is not on a branch, or the branch moves now.
    /// Never attaches: with the host away it counts what was fetched.
    pub(super) fn refresh(&mut self, path: &Path, poll: bool) -> Option<Vec<Effect>> {
        let o = self.roots.get_mut(path)?;
        let name = o.branch.as_ref()?.name.clone();
        o.claim(Moving::Back).then_some(())?;
        let errand = Errand::refresh(path.to_path_buf(), o.host.clone(), name, poll);
        Some(vec![Effect::Run(errand)])
    }

    /// *Update host from laptop*: this folder's commit merged into the host's
    /// branch, once nothing works there, then the branch fetched back. Why
    /// not, on the project's page.
    pub fn update_host(&mut self, path: &Path) -> Vec<Effect> {
        let Some(o) = self.roots.get_mut(path) else {
            return Vec::new();
        };
        let Some(name) = o.branch.as_ref().map(|b| b.name.clone()) else {
            return Vec::new();
        };
        if !o.claim(Moving::Up) {
            return Vec::new();
        }
        let errand = Errand::update_host(path.to_path_buf(), o.host.clone(), name);
        vec![Effect::Run(errand)]
    }
}

/// Keep what a fetch (`Back`) or an update (`Up`) came to, the branch no
/// longer moving. Each failure goes only with its own kind's next success:
/// an update's conflict stands through the fetches the poll makes after it.
pub(super) fn landed_on(o: &mut Offload, by: Moving, counted: Result<usize, String>) {
    o.moving = None;
    let Some(branch) = o.branch.as_mut() else {
        return;
    };
    match (by, counted) {
        (Moving::Up, Ok(ahead)) => {
            branch.ahead = Some(ahead);
            branch.refused = None;
            o.failed = None;
        }
        (Moving::Up, Err(why)) => branch.refused = Some(why),
        (Moving::Back, Ok(ahead)) => {
            branch.ahead = Some(ahead);
            o.failed = None;
        }
        (Moving::Back, Err(why)) => o.failed = Some(why),
    }
}
