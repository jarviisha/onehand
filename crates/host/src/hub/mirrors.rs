//! The mirrors the host keeps of offloaded projects: whose each one is, which
//! side may change its code, and how many prompts went to it. The host is the
//! one to say who holds the key, so a prompt and the key's way back to the
//! laptop never cross: it refuses whichever comes second.
//!
//! **A branch mirror has no key.** It works on a branch of its own, which
//! the laptop fetches; it is made with the key on the host's side, so its
//! prompts go through, and the key is never handed back.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use onehand_core::host::wire::{Holder, MirrorState};
use serde::{Deserialize, Serialize};

use super::Hub;
use crate::serve::log;

/// `dir/mirrors.json`: every mirror's owner, key and generation.
const MIRRORS: &str = "mirrors.json";

/// `dir/mirrors/<laptop>/<name>`: where the mirrors themselves live.
const MIRRORS_DIR: &str = "mirrors";

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Mirror {
    owner: String,
    key: Holder,
    generation: u64,
    /// The branch a branch mirror works on; none for one with a key.
    #[serde(default)]
    pub(super) branch: Option<String>,
}

pub(super) type Mirrors = Result<BTreeMap<PathBuf, Mirror>, String>;

/// The mirrors kept in `dir`; none for no file, and why for one that cannot
/// be read, which refuses every mirror call rather than forget whose is whose.
pub(super) fn load(dir: &Path) -> Mirrors {
    match fs::read(dir.join(MIRRORS)) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(e) => Err(e.to_string()),
    }
    .map_err(|e| {
        let why = format!("{MIRRORS} could not be read, so no mirror is used: {e}");
        log(&why);
        why
    })
}

/// A branch the laptop may name for a branch mirror: `onehand/` and one
/// plain name.
fn branch_name(branch: &str) -> bool {
    branch
        .strip_prefix("onehand/")
        .is_some_and(|name| plain(name) && !name.contains("..") && !name.ends_with(".lock"))
}

/// A name the laptop or the host may put in a path as one part of it.
fn plain(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

impl Hub {
    /// The attached laptop's mirror called `name`, made the first time: with
    /// `branch`, a repository of its own that works on that branch.
    pub(crate) fn mirror(
        &mut self,
        name: &str,
        branch: Option<&str>,
    ) -> Result<MirrorState, String> {
        let owner = self.owner()?;
        if !plain(name) {
            return Err(format!("{name} cannot name a mirror"));
        }
        if let Some(branch) = branch.filter(|b| !branch_name(b)) {
            return Err(format!("{branch} cannot name a mirror's branch"));
        }
        let path = self.dir.join(MIRRORS_DIR).join(&owner).join(name);
        let mut mirrors = self.mirrors.clone()?;
        match mirrors.get(&path) {
            Some(mirror) if mirror.owner != owner => return Err(elsewhere(&path)),
            Some(_) => {}
            None => {
                fs::create_dir_all(&path)
                    .map_err(|e| format!("Could not make {}: {e}", path.display()))?;
                if branch.is_some() {
                    super::branch::init_blocking(&path)?;
                }
                let mirror = Mirror {
                    owner,
                    key: match branch {
                        Some(_) => Holder::Host,
                        None => Holder::Laptop,
                    },
                    generation: 0,
                    branch: branch.map(str::to_string),
                };
                mirrors.insert(path.clone(), mirror);
                self.keep(mirrors)?;
            }
        }
        self.mirror_state(&path)
    }

    /// Hand the key of the mirror at `path` to `to`. Back to the laptop only
    /// while nothing works there and no prompt went there since `generation`:
    /// the laptop copied the code back as it stood then.
    pub(crate) fn mirror_key(
        &mut self,
        path: &Path,
        to: Holder,
        generation: u64,
    ) -> Result<MirrorState, String> {
        let owner = self.owner()?;
        let mut mirrors = self.mirrors.clone()?;
        let mirror = mirrors.get_mut(path).ok_or_else(|| unknown(path))?;
        if mirror.owner != owner {
            return Err(elsewhere(path));
        }
        if mirror.branch.is_some() {
            return Err(format!(
                "{} has no key; it works on a branch",
                path.display()
            ));
        }
        if to == Holder::Laptop {
            if self.working(path) {
                return Err("Sessions are still working on the host".into());
            }
            if mirror.generation != generation {
                return Err("A prompt went to the host since the code was copied".into());
            }
        }
        mirror.key = to;
        self.keep(mirrors)?;
        self.mirror_state(path)
    }

    /// Refused when `path` lies in a mirror the attached laptop does not own,
    /// or where mirrors are kept and in none of them.
    pub(crate) fn may_use(&self, path: &Path) -> Result<(), String> {
        if !self.in_mirrors(path) {
            return Ok(());
        }
        let owner = self.owner()?;
        let (_, mirror) = self.mirror_of(path)?;
        match mirror.owner == owner {
            true => Ok(()),
            false => Err(elsewhere(path)),
        }
    }

    /// Before a prompt to a session in `cwd`: refused while the laptop holds
    /// its mirror's key, else the mirror's generation moved on.
    pub(super) fn prompt_to(&mut self, cwd: &Path) -> Result<(), String> {
        if !self.in_mirrors(cwd) {
            return Ok(());
        }
        let (path, _) = self.mirror_of(cwd)?;
        let mut mirrors = self.mirrors.clone()?;
        let mirror = mirrors.get_mut(&path).ok_or_else(|| unknown(cwd))?;
        if self.updating.contains(&path) {
            return Err(
                "The host's branch is being updated from the laptop; send the prompt again in a \
                 moment"
                    .into(),
            );
        }
        if mirror.key == Holder::Laptop {
            return Err(
                "The laptop holds this project's code; send the prompt again to send it up first"
                    .into(),
            );
        }
        mirror.generation += 1;
        self.keep(mirrors)
    }

    /// Start updating the branch mirror at `path` from the laptop: its
    /// branch, once nothing works there and no update runs. Until
    /// [`Hub::end_update`], a prompt to it is refused, so no agent works
    /// while git merges under it.
    pub(crate) fn begin_update(&mut self, path: &Path) -> Result<String, String> {
        let owner = self.owner()?;
        let mirror = self
            .mirrors
            .as_ref()?
            .get(path)
            .ok_or_else(|| unknown(path))?;
        if mirror.owner != owner {
            return Err(elsewhere(path));
        }
        let Some(branch) = mirror.branch.clone() else {
            return Err(format!("{} has no branch to update", path.display()));
        };
        if self.working(path) {
            return Err("Sessions are still working on the host".into());
        }
        if !self.updating.insert(path.to_path_buf()) {
            return Err("The host's branch is being updated already".into());
        }
        Ok(branch)
    }

    pub(crate) fn end_update(&mut self, path: &Path) {
        self.updating.remove(path);
    }

    /// The branch mirror `cwd` lies in, and its branch, if it does.
    pub(super) fn branch_of(&self, cwd: &Path) -> Option<(PathBuf, String)> {
        if !self.in_mirrors(cwd) {
            return None;
        }
        let (path, mirror) = self.mirror_of(cwd).ok()?;
        Some((path, mirror.branch.clone()?))
    }

    /// The mirror `path` lies in, and the path it is kept under.
    fn mirror_of(&self, path: &Path) -> Result<(PathBuf, &Mirror), String> {
        self.mirrors
            .as_ref()?
            .iter()
            .find(|(mirror, _)| path.starts_with(mirror))
            .map(|(mirror, m)| (mirror.clone(), m))
            .ok_or_else(|| unknown(path))
    }

    pub(crate) fn mirror_state(&self, path: &Path) -> Result<MirrorState, String> {
        let mirror = self
            .mirrors
            .as_ref()?
            .get(path)
            .ok_or_else(|| unknown(path))?;
        Ok(MirrorState {
            path: path.to_path_buf(),
            key: mirror.key,
            generation: mirror.generation,
            working: self.working(path),
            // ponytail: one git read under the hub's lock; cache it if
            // mirror calls ever queue behind it.
            head: mirror
                .branch
                .as_deref()
                .and_then(|branch| super::branch::head_blocking(path, branch)),
        })
    }

    /// Whether a session in the mirror at `path`, or a folder in it, is
    /// mid-turn or has work running aside.
    fn working(&self, path: &Path) -> bool {
        self.sessions
            .values()
            .any(|s| s.cwd.starts_with(path) && !s.ended && (s.busy || !s.aside.is_empty()))
    }

    /// Whether `path` lies where mirrors are kept, whatever `mirrors.json`
    /// says: spelled with `..` or through a link, it is held as it resolves,
    /// and so refused unless it is spelled as its mirror is.
    fn in_mirrors(&self, path: &Path) -> bool {
        let kept = self.dir.join(MIRRORS_DIR);
        let resolved = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        path.starts_with(&kept) || resolved(path).starts_with(resolved(&kept))
    }

    /// The attached laptop, which a mirror is kept for.
    fn owner(&self) -> Result<String, String> {
        self.laptop
            .clone()
            .filter(|id| plain(id))
            .ok_or_else(|| "This laptop's onehand is too old to keep a mirror".into())
    }

    /// Write `mirrors` whole, then take it as the mirrors.
    fn keep(&mut self, mirrors: BTreeMap<PathBuf, Mirror>) -> Result<(), String> {
        let text = serde_json::to_string(&mirrors).map_err(|e| e.to_string())?;
        onehand_core::config::write_atomic(&self.dir.join(MIRRORS), &text)
            .map_err(|e| format!("Could not keep the mirrors: {e}"))?;
        self.mirrors = Ok(mirrors);
        Ok(())
    }
}

fn elsewhere(path: &Path) -> String {
    format!("{} is offloaded from another laptop", path.display())
}

fn unknown(path: &Path) -> String {
    format!("{} is not a mirror on this host", path.display())
}
