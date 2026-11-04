//! A host's folders, files and git status, asked for over the attach. Errors
//! come back as the host said them; naming the host is the caller's to do,
//! except for the home folder and a clone, which name it themselves.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::{on_host, Answer, Call, HostConn, HostError};
use crate::editor::SaveOutcome;
use crate::gitstat::GitStatus;
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
        self.ask_for(Call::Home, |a| a.home)
            .await
            .map_err(|e| on_host(&self.host, e))
    }

    /// Have the host `git clone` `url` into `into`, which must not exist yet.
    pub async fn clone_repo(&self, url: &str, into: &Path) -> Result<(), HostError> {
        let call = Call::Clone {
            url: url.into(),
            into: into.to_path_buf(),
        };
        let cloned = async {
            let reply = self.ask_within(call, CLONE_WAIT).await?;
            let unexpected = HostError::Protocol(format!("unexpected answer: {reply:?}"));
            Answer::of(reply).done.ok_or(unexpected)
        };
        cloned.await.map_err(|e| on_host(&self.host, e))
    }
}
