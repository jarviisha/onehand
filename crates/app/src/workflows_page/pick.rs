//! What the Workflows page has picked, and the order its list shows.
//!
//! **A pick is a key, never a place.** The list is read again after every
//! save, delete and import, and a place in it would then name another
//! workflow: the person's own by its file, a shipped one by its id.

use crate::workflow::Entry;
use std::path::PathBuf;

/// The workflow the page has open.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) enum Pick {
    /// One of the person's own, by its file, read or not.
    Own(PathBuf),
    /// One onehand ships, by its id.
    Shipped(String),
    /// The new workflow being written, not saved yet.
    New,
}

/// The key `entry` is picked by; `None` for a shipped one that cannot be read.
pub(super) fn pick_of(entry: &Entry) -> Option<Pick> {
    match (&entry.file, &entry.template) {
        (Some(file), _) => Some(Pick::Own(file.clone())),
        (None, Ok(template)) => Some(Pick::Shipped(template.id.clone())),
        (None, Err(_)) => None,
    }
}

/// The entries in the order the list shows them: the person's own first,
/// then the shipped, each as read, capped.
pub(super) fn listed(entries: &[Entry]) -> Vec<usize> {
    let own = (0..entries.len()).filter(|&i| entries[i].file.is_some());
    let shipped = (0..entries.len()).filter(|&i| entries[i].file.is_none());
    own.chain(shipped)
        .take(crate::workflow::TEMPLATES_SHOWN)
        .collect()
}

/// Where `pick` is among `entries` now, if it still is.
pub(super) fn repick(entries: &[Entry], pick: &Pick) -> Option<usize> {
    entries
        .iter()
        .position(|entry| pick_of(entry).as_ref() == Some(pick))
}

#[cfg(test)]
mod tests;
