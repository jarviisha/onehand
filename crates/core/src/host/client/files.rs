//! A host's folders, files and git status, asked for over the attach. Errors
//! come back as the host said them; naming the host is the caller's to do.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::{Call, HostConn, HostError};
use crate::editor::SaveOutcome;
use crate::gitstat::GitStatus;
use crate::host::wire::{Holder, MirrorState};
use crate::tree::TreeEntry;

/// How long a clone may take: a big repository over a slow link takes
/// minutes, and one stuck asking for credentials must still end.
const CLONE_WAIT: Duration = Duration::from_secs(15 * 60);

impl HostConn {
    /// The `ssh` destination this connection reaches.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// `dir`'s listing, and how many entries its bound left out.
    pub async fn read_dir(&self, dir: &Path) -> Result<(Vec<TreeEntry>, usize), HostError> {
        let call = Call::ReadDir {
            path: dir.to_path_buf(),
        };
        self.ask_for(call, |a| a.listing).await
    }

    /// The file's text and its modification time.
    pub async fn read_file(&self, path: &Path) -> Result<(String, Option<SystemTime>), HostError> {
        let call = Call::ReadFile {
            path: path.to_path_buf(),
        };
        self.ask_for(call, |a| a.file).await
    }

    /// Write `text` unless the file changed since `expected`.
    pub async fn save_file(
        &self,
        path: &Path,
        text: String,
        expected: Option<SystemTime>,
    ) -> Result<SaveOutcome, HostError> {
        let call = Call::SaveFile {
            path: path.to_path_buf(),
            text,
            expected,
        };
        self.ask_for(call, |a| a.saved).await
    }

    /// The repository status of `root`, None when it is not in one.
    pub async fn git_status(&self, root: &Path) -> Result<Option<GitStatus>, HostError> {
        let call = Call::GitStatus {
            root: root.to_path_buf(),
        };
        self.ask_for(call, |a| a.git).await
    }

    /// The host user's home folder.
    pub async fn home(&self) -> Result<PathBuf, HostError> {
        self.ask_for(Call::Home, |a| a.home).await
    }

    /// Whether the host answers the cards of sessions in `cwd` by itself.
    pub async fn bypass(&self, cwd: &Path) -> Result<bool, HostError> {
        let call = Call::Bypass {
            cwd: cwd.to_path_buf(),
            on: None,
        };
        self.ask_for(call, |a| a.bypass).await
    }

    /// Set whether the host answers `cwd`'s cards by itself; what it is now.
    pub async fn set_bypass(&self, cwd: &Path, on: bool) -> Result<bool, HostError> {
        let call = Call::Bypass {
            cwd: cwd.to_path_buf(),
            on: Some(on),
        };
        self.ask_for(call, |a| a.bypass).await
    }

    /// This laptop's mirror called `name`, made on first use.
    pub async fn mirror(&self, name: &str) -> Result<MirrorState, HostError> {
        let call = Call::Mirror { name: name.into() };
        self.ask_for(call, |a| a.mirror).await
    }

    /// Hand the key of the mirror at `path` to `to`; refused for the laptop
    /// while something works there or a prompt went there since `generation`.
    pub async fn mirror_key(
        &self,
        path: &Path,
        to: Holder,
        generation: u64,
    ) -> Result<MirrorState, HostError> {
        let call = Call::MirrorKey {
            path: path.to_path_buf(),
            to,
            generation,
        };
        self.ask_for(call, |a| a.mirror).await
    }

    /// The files of the mirror at `path` its copy carries, relative to it.
    pub async fn mirror_files(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        let call = Call::MirrorFiles {
            path: path.to_path_buf(),
        };
        self.ask_for(call, |a| a.mirror_files).await
    }

    /// Have the host `git clone` `url` into `into`, which must not exist yet.
    pub async fn clone_repo(&self, url: &str, into: &Path) -> Result<(), HostError> {
        let call = Call::Clone {
            url: url.into(),
            into: into.to_path_buf(),
        };
        self.ask_for_within(call, CLONE_WAIT, |a| a.done).await
    }
}
