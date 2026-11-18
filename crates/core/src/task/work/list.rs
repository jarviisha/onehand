//! The Issues page's list: which issues it shows, in what order, and what
//! each row and the list's head say.
//!
//! One function, [`list`], decides all of it from what the page holds — the
//! issues, the filters, and the list as it is on screen — so the page draws
//! what it is told and decides nothing.
//!
//! **The list does not move under a person.** A row that stops matching a
//! filter stays, saying where it went, until another issue is picked or a
//! filter changes; a new match is added below the rows already there; and an
//! issue opened from elsewhere that the filters leave out is pinned at the
//! top.

use crate::issues::{matches_query, IssueKey, LocalIssue};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// How many rows the list draws. The rest are counted, and said.
pub(crate) const LIST_CAP: usize = 500;

/// What the list is narrowed to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filters {
    /// The closed issues rather than the open ones.
    pub closed: bool,
    /// One project, by its root.
    pub project: Option<PathBuf>,
    pub label: Option<String>,
    /// The search, as [`matches_query`] reads it.
    pub query: String,
}

/// One issue the page holds.
#[derive(Debug, Clone)]
pub struct Item<'a> {
    pub root: &'a Path,
    pub key: IssueKey,
    pub issue: &'a LocalIssue,
}

/// The list as it is on screen, which [`list`] keeps still.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Held {
    /// The rows on screen, in order; empty for a list drawn afresh, which a
    /// change of filter is.
    pub order: Vec<IssueKey>,
    /// An issue opened from elsewhere, shown even when the filters leave it
    /// out.
    pub pinned: Option<IssueKey>,
}

/// One row.
#[derive(Debug, Clone)]
pub struct Row<'a> {
    pub item: &'a Item<'a>,
    /// It no longer matches the filters and stays until another issue is
    /// picked or a filter changes: where it went, *now closed*.
    pub left: Option<String>,
    /// Pinned at the top though the filters leave it out.
    pub outside: bool,
}

/// The list and what its head says.
#[derive(Debug, Clone)]
pub struct Listed<'a> {
    pub rows: Vec<Row<'a>>,
    /// How many rows [`LIST_CAP`] left out.
    pub left_out: usize,
    /// How many issues the other filters let through, open and closed: the
    /// counts the open/closed switch carries.
    pub open: usize,
    pub closed: usize,
}

impl Listed<'_> {
    /// The rows on screen, to hold the list still on the next draw.
    pub fn order(&self) -> Vec<IssueKey> {
        self.rows
            .iter()
            .filter(|row| !row.outside)
            .map(|row| row.item.key.clone())
            .collect()
    }

    /// The rows that stopped matching and stay until another issue is
    /// picked.
    pub fn stopped_matching(&self) -> Vec<IssueKey> {
        self.rows
            .iter()
            .filter(|row| !row.outside && row.left.is_some())
            .map(|row| row.item.key.clone())
            .collect()
    }
}

impl Held {
    /// `picked` was picked: the rows of `stopped` that stopped matching
    /// leave, but never the one picked, which stays selected where it is;
    /// and a pin on another issue goes.
    pub fn pick(&mut self, picked: &IssueKey, stopped: &[IssueKey]) {
        self.order
            .retain(|key| key == picked || !stopped.contains(key));
        if self.pinned.as_ref() != Some(picked) {
            self.pinned = None;
        }
    }
}

/// The list the page draws: `items` narrowed by `filters`, held still by
/// `held`.
pub fn list<'a>(items: &'a [Item<'a>], filters: &Filters, held: &Held) -> Listed<'a> {
    let base = |item: &Item<'_>| {
        filters
            .project
            .as_deref()
            .is_none_or(|only| item.root == only)
            && filters
                .label
                .as_ref()
                .is_none_or(|label| item.issue.labels.contains(label))
            && matches_query(item.issue, &filters.query)
    };
    let matches = |item: &Item<'_>| base(item) && item.issue.open != filters.closed;

    let through: Vec<&Item<'_>> = items.iter().filter(|item| base(item)).collect();
    let open = through.iter().filter(|item| item.issue.open).count();
    let closed = through.len() - open;

    let row = |item: &'a Item<'a>| Row {
        item,
        left: None,
        outside: false,
    };

    let mut rows: Vec<Row<'a>> = Vec::new();
    let mut placed: HashSet<&IssueKey> = HashSet::new();
    for key in &held.order {
        let Some(item) = items.iter().find(|item| &item.key == key) else {
            continue;
        };
        if !placed.insert(&item.key) {
            continue;
        }
        let mut drawn = row(item);
        if !matches(item) {
            drawn.left = Some(where_it_is(item, filters).to_string());
        }
        rows.push(drawn);
    }
    let mut fresh: Vec<&'a Item<'a>> = items
        .iter()
        .filter(|item| matches(item) && !placed.contains(&item.key))
        .collect();
    fresh.sort_by(|a, b| {
        b.issue
            .updated
            .cmp(&a.issue.updated)
            .then_with(|| a.root.cmp(b.root))
            .then_with(|| a.key.number.cmp(&b.key.number))
    });
    rows.extend(fresh.into_iter().map(row));

    let pinned = held
        .pinned
        .as_ref()
        .filter(|pinned| !rows.iter().any(|row| &row.item.key == *pinned))
        .and_then(|pinned| items.iter().find(|item| &item.key == pinned));
    if let Some(item) = pinned {
        let mut drawn = row(item);
        drawn.outside = true;
        rows.insert(0, drawn);
    }

    let left_out = rows.len().saturating_sub(LIST_CAP);
    rows.truncate(LIST_CAP);

    Listed {
        rows,
        left_out,
        open,
        closed,
    }
}

/// Where an issue that stopped matching went, in a word or two.
fn where_it_is(item: &Item<'_>, filters: &Filters) -> &'static str {
    match (item.issue.open, filters.closed) {
        (false, false) => "now closed",
        (true, true) => "now open",
        (false, true) | (true, false) => "no longer matches the filters",
    }
}

#[cfg(test)]
mod tests;
