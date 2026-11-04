//! The file calls: a folder's listing, a file to read or save, a folder's git
//! status. Each touches the disk, so the service runs them off its connection
//! loop and a slow one holds nothing else up.

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
