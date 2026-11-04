//! A host's folders, files and git status, asked for over the attach. Errors
//! come back as the host said them; naming the host is the caller's to do.

use std::path::Path;
use std::time::SystemTime;

use super::{Call, HostConn, HostError};
use crate::editor::SaveOutcome;
use crate::gitstat::GitStatus;
use crate::tree::TreeEntry;

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
}
