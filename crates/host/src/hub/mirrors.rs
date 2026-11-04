//! The mirrors the host keeps of offloaded projects: whose each one is, which
//! side may change its code, and how many prompts went to it. The host is the
//! one to say who holds the key, so a prompt and the key's way back to the
//! laptop never cross: it refuses whichever comes second.

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
    /// The attached laptop's mirror called `name`, made the first time.
    pub(crate) fn mirror(&mut self, name: &str) -> Result<MirrorState, String> {
        let owner = self.owner()?;
        if !plain(name) {
            return Err(format!("{name} cannot name a mirror"));
        }
        let path = self.dir.join(MIRRORS_DIR).join(&owner).join(name);
        let mut mirrors = self.mirrors.clone()?;
        match mirrors.get(&path) {
            Some(mirror) if mirror.owner != owner => return Err(elsewhere(&path)),
            Some(_) => {}
            None => {
                fs::create_dir_all(&path)
                    .map_err(|e| format!("Could not make {}: {e}", path.display()))?;
                let mirror = Mirror {
                    owner,
                    key: Holder::Laptop,
                    generation: 0,
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

    /// Refused when `cwd` is a mirror the attached laptop does not own.
    pub(crate) fn may_use(&self, cwd: &Path) -> Result<(), String> {
        if !self.in_mirrors(cwd) {
            return Ok(());
        }
        let owner = self.owner()?;
        match self.mirrors.as_ref()?.get(cwd) {
            Some(mirror) if mirror.owner == owner => Ok(()),
            Some(_) => Err(elsewhere(cwd)),
            None => Err(unknown(cwd)),
        }
    }

    /// Before a prompt to a session in `cwd`: refused while the laptop holds
    /// the mirror's key, else the mirror's generation moved on.
    pub(super) fn prompt_to(&mut self, cwd: &Path) -> Result<(), String> {
        if !self.in_mirrors(cwd) {
            return Ok(());
        }
        let mut mirrors = self.mirrors.clone()?;
        let mirror = mirrors.get_mut(cwd).ok_or_else(|| unknown(cwd))?;
        if mirror.key == Holder::Laptop {
            return Err(
                "The laptop holds this project's code; send the prompt again to send it up first"
                    .into(),
            );
        }
        mirror.generation += 1;
        self.keep(mirrors)
    }

    fn mirror_state(&self, path: &Path) -> Result<MirrorState, String> {
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
        })
    }

    /// Whether a session in `cwd` is mid-turn or has work running aside.
    fn working(&self, cwd: &Path) -> bool {
        self.sessions
            .values()
            .any(|s| s.cwd == cwd && !s.ended && (s.busy || !s.aside.is_empty()))
    }

    /// Whether `cwd` lies where mirrors are kept, whatever `mirrors.json` says.
    fn in_mirrors(&self, cwd: &Path) -> bool {
        cwd.starts_with(self.dir.join(MIRRORS_DIR))
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
