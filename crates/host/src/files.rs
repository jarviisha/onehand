//! The file calls: a folder's listing, a file to read or save, a folder's git
//! status, the user's home folder, a clone. Each touches the disk, so the
//! service runs them off its connection loop and a slow one holds nothing
//! else up.

use std::path::Path;
use std::process::{Command, Stdio};

use onehand_core::editor::{read_blocking, save_blocking};
use onehand_core::gitstat;
use onehand_core::host::wire::{Call, Reply};
use onehand_core::tree::read_dir_sorted;

/// Answer a file call. Blocking.
///
/// The reply must also serialize: a modification time before 1970 cannot be
/// written as JSON, and a line the service cannot write would leave the laptop
/// waiting for an answer that never comes.
pub(crate) fn answer(call: Call) -> Result<Reply, String> {
    let reply = match call {
        Call::ReadDir { path } => {
            // The listing reads a folder it cannot open as empty, which on the
            // laptop would look like a real, empty tree: say why instead.
            std::fs::read_dir(&path).map_err(|e| e.to_string())?;
            let (entries, cut) = read_dir_sorted(&path);
            Reply::Listing { entries, cut }
        }
        Call::ReadFile { path } => {
            let (text, mtime) = read_blocking(&path)?;
            Reply::File { text, mtime }
        }
        Call::SaveFile {
            path,
            text,
            expected,
        } => Reply::Saved(save_blocking(&path, text, expected)),
        Call::GitStatus { root } => Reply::Git(gitstat::read_blocking(&root)),
        Call::Home => Reply::Home(dirs::home_dir().ok_or("the host user has no home folder")?),
        Call::Clone { url, into } => {
            clone(&url, &into)?;
            Reply::Done
        }
        Call::Hello { .. }
        | Call::Shutdown
        | Call::List
        | Call::Open { .. }
        | Call::Watch { .. }
        | Call::Request { .. }
        | Call::Probe { .. }
        | Call::TermOpen { .. }
        | Call::TermList
        | Call::TermWatch { .. }
        | Call::TermWrite { .. }
        | Call::TermResize { .. }
        | Call::TermClose { .. } => return Err("not a file call".into()),
    };
    serde_json::to_vec(&reply).map_err(|e| format!("the reply cannot be sent: {e}"))?;
    Ok(reply)
}

/// `git clone` `url` into `into`, which must be absolute and not exist yet.
/// Nobody is there to type a password, so git is told never to ask for one:
/// a repository that needs one fails rather than waits. Blocking.
fn clone(url: &str, into: &Path) -> Result<(), String> {
    if !into.is_absolute() {
        return Err("the folder to clone into starts at /".into());
    }
    // A dangling link counts as there: git would refuse it too.
    if into.symlink_metadata().is_ok() {
        return Err(format!("there is already {}", into.display()));
    }
    let output = Command::new("git")
        .arg("clone")
        .arg("--")
        .arg(url)
        .arg(into)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .env("SSH_ASKPASS_REQUIRE", "never")
        .output()
        .map_err(|e| format!("git could not run: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    // `into` was absent before git ran, so whatever is there now is what
    // this clone left half made.
    let _ = std::fs::remove_dir_all(into);
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(stderr
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map_or_else(
            || format!("git exited with {}", output.status),
            str::to_string,
        ))
}

#[cfg(test)]
mod tests;
