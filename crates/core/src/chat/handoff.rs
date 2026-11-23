//! A past conversation carried into a new session: written as Markdown to a
//! file the new session can read, and a line in its first prompt pointing
//! the agent at it.
//!
//! **A file, not the transcript pasted in.** A long conversation pasted into a
//! prompt spends the new session's context on tool output it may never need;
//! a file is read as far as the agent wants, by any agent that reads files.

use std::path::{Path, PathBuf};

/// The folder, inside a session's own, that what onehand hands an agent is
/// written under, and what keeps git from seeing it: a `.gitignore` of its
/// own, so no line goes into the person's files.
const HANDOFF_DIR: &str = ".onehand";
const CONTINUED: &str = "continued";

/// Write the conversation kept in `kept` as Markdown under `folder`, where
/// git ignores it: the file written. Blocking: it reads and writes files.
pub fn write_blocking(kept: &Path, folder: &Path) -> Result<PathBuf, String> {
    let snapshot =
        super::store::load(kept).ok_or_else(|| format!("{} could not be read", kept.display()))?;
    // Named for the folder it is kept in, which the store already made safe
    // and one per conversation: two conversations never share a file.
    let name = kept
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| format!("{} names no conversation", kept.display()))?;
    let mut chat = super::Chat::default();
    chat.resume_from(snapshot);
    let text = super::export_markdown(&chat);
    let base = folder.join(HANDOFF_DIR);
    let dir = base.join(CONTINUED);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let ignore = base.join(".gitignore");
    if !ignore.exists() {
        std::fs::write(&ignore, "*\n").map_err(|e| format!("{}: {e}", ignore.display()))?;
    }
    let file = dir.join(format!("{name}.md"));
    std::fs::write(&file, text).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(file)
}

/// The first prompt of a session continuing `title`, kept in `file`: the
/// person's `text` first, so the new conversation is named for it, then the
/// hand-off, asking for a summary first when `summarise`.
pub fn lead_in(text: &str, file: &Path, title: &str, summarise: bool) -> String {
    let ask = match summarise {
        true => {
            "Read it first, and open your reply with a short summary of it: what was decided, \
             what is left to do, and the files it touched. Then go on with what I asked."
        }
        false => "Read it before you go on, as far as you need.",
    };
    format!(
        "{text}\n\nThis continues an earlier conversation, \"{title}\", kept in {}. {ask}",
        file.display()
    )
}

#[cfg(test)]
mod tests;
