//! What a lead keeps of its workers across a restart, in a file beside its
//! conversation: so it moves and is deleted with that conversation.
//!
//! **A file that cannot be read keeps nothing.** Missing, unreadable or
//! malformed, it reads as no worker kept, and the next write replaces it:
//! a lead loses what it held rather than failing to open its conversation.
//!
//! **A write never leaves half a file.** It goes to a file beside it, then
//! is renamed over the old one, so a crash mid-write leaves the old one
//! whole.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The file's name inside the lead's conversation directory.
const FILE: &str = "workers.json";

/// Everything kept for one lead.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    /// The workers it still held, in the order they opened.
    pub workers: Vec<KeptWorker>,
    /// The workers that ended, so their handles keep answering closed.
    pub closed: Vec<KeptClosed>,
}

/// A worker the lead held, as it comes back.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
pub struct KeptWorker {
    pub handle: String,
    /// Its conversation's session id.
    pub conversation: String,
    pub agent: String,
    /// The folder its session is on.
    pub folder: PathBuf,
    /// The folders it may write besides its own.
    pub dirs: Vec<PathBuf>,
    /// Its name, as the lead gave it.
    pub name: Option<String>,
    /// The lead's last turn number and text.
    pub turn: Option<u64>,
    pub text: Option<String>,
    /// The person holds it: they prompted it since the lead's last turn.
    pub person: bool,
}

/// A worker that ended, by its handle.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
pub struct KeptClosed {
    pub handle: String,
    /// Why it did not come back, when that is what ended it.
    pub why: Option<String>,
}

/// What is kept beside the conversation in `conv_dir`. Blocking.
pub fn read_blocking(conv_dir: &Path) -> Kept {
    let path = conv_dir.join(FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => return Kept::default(),
        Err(why) => {
            eprintln!("onehand: cannot read {}: {why}", path.display());
            return Kept::default();
        }
    };
    serde_json::from_str(&text).unwrap_or_else(|why| {
        eprintln!("onehand: {} is not kept workers: {why}", path.display());
        Kept::default()
    })
}

/// Keep `kept` beside the conversation in `conv_dir`; nothing kept removes
/// the file. Blocking.
pub fn write_blocking(conv_dir: &Path, kept: &Kept) -> std::io::Result<()> {
    let path = conv_dir.join(FILE);
    if *kept == Kept::default() {
        return match std::fs::remove_file(&path) {
            Err(why) if why.kind() != std::io::ErrorKind::NotFound => Err(why),
            Ok(()) | Err(_) => Ok(()),
        };
    }
    // A lead's first turn may still be running, before its conversation
    // was first written.
    std::fs::create_dir_all(conv_dir)?;
    let text = serde_json::to_string_pretty(kept).map_err(std::io::Error::other)?;
    let tmp = conv_dir.join(format!("{FILE}.tmp"));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &path)
}

#[cfg(test)]
mod tests;
