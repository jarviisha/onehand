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

/// Whether `entry` answers `query`: its name, or a readable one's
/// description, holding the query's words as typed, case aside. An empty
/// query is answered by every entry.
pub(super) fn matches(entry: &Entry, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    let holds = |text: &str| text.to_lowercase().contains(&query);
    holds(&entry.name()) || entry.template.as_ref().is_ok_and(|t| holds(&t.description))
}

/// What the page shows of the entries: their places in its order, and how
/// many answered the query before the cap cut them.
pub(super) struct Listed {
    pub(super) shown: Vec<usize>,
    pub(super) matched: usize,
}

/// The entries in the order the page shows them: the person's own first,
/// then the shipped, those answering `query`, capped. The cap comes after
/// the query, so a match is never hidden behind workflows that do not match.
pub(super) fn listed(entries: &[Entry], query: &str) -> Listed {
    let own = (0..entries.len()).filter(|&i| entries[i].file.is_some());
    let shipped = (0..entries.len()).filter(|&i| entries[i].file.is_none());
    let matching: Vec<usize> = own
        .chain(shipped)
        .filter(|&i| matches(&entries[i], query))
        .collect();
    let matched = matching.len();
    Listed {
        shown: matching
            .into_iter()
            .take(crate::workflow::TEMPLATES_SHOWN)
            .collect(),
        matched,
    }
}

/// Where `pick` is among `entries` now, if it still is.
pub(super) fn repick(entries: &[Entry], pick: &Pick) -> Option<usize> {
    entries
        .iter()
        .position(|entry| pick_of(entry).as_ref() == Some(pick))
}

#[cfg(test)]
mod tests;
