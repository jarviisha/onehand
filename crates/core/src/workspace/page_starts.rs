//! What a project's page sending a prompt comes to: whether a send is taken,
//! what the page says while it is under way, and what follows once the new
//! session's agent is up or never comes up. The front end carries the
//! effects out; it decides none of this.

use crate::chat::Link;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// The sends under way from the pages of one window.
#[derive(Debug, Default)]
pub struct PageStarts {
    /// The projects whose page is starting a session, by folder.
    starting: HashSet<PathBuf>,
    /// The sessions started off screen for a page, waiting on their agent,
    /// with the page's project.
    waits: HashMap<u64, PathBuf>,
}

/// What the front end carries out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Say `said` on `root`'s page, its words locked; none: done.
    Busy { root: PathBuf, said: Option<String> },
    /// Say on `root`'s page why its send started nothing; its words stay.
    Failed { root: PathBuf, why: String },
    /// `root`'s send started its session: what was written there goes.
    Sent { root: PathBuf },
    /// Show session `uid`.
    Show { uid: u64 },
    /// Close session `uid`, which never came up.
    Close { uid: u64 },
}

impl PageStarts {
    /// Whether a send from `root`'s page is taken: only while that page is
    /// the one `on_screen`, and one at a time, so a second press never cuts a
    /// worktree twice. Taken, it is under way until it ends.
    pub fn take(&mut self, root: &Path, on_screen: Option<&Path>) -> bool {
        on_screen == Some(root) && self.starting.insert(root.to_path_buf())
    }

    /// A worktree on `branch` is being cut for `root`'s send.
    pub fn cutting(&self, root: &Path, branch: &str) -> Vec<Effect> {
        busy(root, format!("Making worktree {branch}…"))
    }

    /// The conversation `title` is being written out for `root`'s send.
    pub fn writing(&self, root: &Path, title: &str) -> Vec<Effect> {
        busy(root, format!("Writing out “{title}”…"))
    }

    /// Session `uid`, running `agent`, was started off screen for `root`'s
    /// send, and waits on its agent.
    pub fn started(&mut self, root: &Path, uid: u64, agent: &str) -> Vec<Effect> {
        self.waits.insert(uid, root.to_path_buf());
        busy(root, format!("Starting {agent}…"))
    }

    /// `root`'s send started nothing, for `why`.
    pub fn gave_up(&mut self, root: &Path, why: String) -> Vec<Effect> {
        self.starting.remove(root);
        vec![Effect::Failed {
            root: root.to_path_buf(),
            why,
        }]
    }

    /// Session `uid`'s link stands at `link`; `why` it was lost, if it was,
    /// and whether the person still `stands` on the page it was sent from.
    /// Up, the session is the person's and its page's words go, shown if the
    /// person still stands there; never up, it goes and the page says why.
    pub fn link(&mut self, uid: u64, link: Link, why: Option<String>, stands: bool) -> Vec<Effect> {
        if link == Link::Connecting {
            return Vec::new();
        }
        let Some(root) = self.waits.remove(&uid) else {
            return Vec::new();
        };
        self.starting.remove(&root);
        match link {
            Link::Connected => {
                let mut fx = vec![
                    Effect::Busy {
                        root: root.clone(),
                        said: None,
                    },
                    Effect::Sent { root },
                ];
                if stands {
                    fx.push(Effect::Show { uid });
                }
                fx
            }
            Link::Lost | Link::Connecting => vec![
                Effect::Close { uid },
                Effect::Failed {
                    root,
                    why: why.unwrap_or_else(|| "The agent did not start".to_string()),
                },
            ],
        }
    }

    /// Whether session `uid` waits on its agent for a page.
    pub fn waits_on(&self, uid: u64) -> bool {
        self.waits.contains_key(&uid)
    }

    /// The project whose page started session `uid`, while it waits.
    pub fn page_of(&self, uid: u64) -> Option<&Path> {
        self.waits.get(&uid).map(PathBuf::as_path)
    }
}

fn busy(root: &Path, said: String) -> Vec<Effect> {
    vec![Effect::Busy {
        root: root.to_path_buf(),
        said: Some(said),
    }]
}

#[cfg(test)]
mod tests;
