//! What a project's menu says about it, read off its row once for the rail
//! and the project page alike.

/// What a project's menu says about it, wherever that menu is drawn.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectFacts {
    /// Held at the top of the rail.
    pub pinned: bool,
    /// A git repository, so there is a branch to rename and a worktree to split.
    pub is_repo: bool,
    /// It has a check command, so its page offers to run it.
    pub check: bool,
    /// Removing it asks first, which its menu's ellipsis says.
    pub asks: bool,
    /// Whether offloading is offered, and which way: `Some(false)` sends a
    /// folder here to a host, `Some(true)` runs an offloaded one here again,
    /// offered only while this machine holds its code.
    pub offload: Option<bool>,
    /// It is its project's own folder, which has the project's name: a
    /// worktree's row is named for its folder, and renames nothing.
    pub renames: bool,
    /// It has settings of its own, kept in Settings: a project whose sessions
    /// run here, a worker's row aside.
    pub settings: bool,
    /// It has a board this machine runs, so its menu offers it.
    pub board: bool,
}

impl ProjectFacts {
    /// The facts about `root`, given whether the last `git status` sweep found
    /// a repository there. One place, because the rail and the project page
    /// both build these and a copy in each is two answers to "is the switch
    /// offered here".
    pub fn of(root: &onehand_core::workspace::ProjectRoot, is_repo: bool) -> Self {
        // What acts on the folder by itself reaches only one whose sessions
        // run here: not a host's, nor an offloaded one's.
        let local = root.runs_here();
        Self {
            pinned: root.pinned,
            is_repo: local && is_repo,
            check: local && root.check.is_some(),
            asks: false,
            renames: root.project.is_some(),
            settings: local && !root.transient,
            // Read from the project store by whoever has it at hand.
            board: false,
            offload: match (&root.host, &root.offload) {
                _ if root.transient => None,
                (Some(_), _) => None,
                (None, Some(_)) => Some(true),
                (None, None) => Some(false),
            },
        }
    }
}
