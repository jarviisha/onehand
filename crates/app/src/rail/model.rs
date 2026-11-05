//! What the rail's list holds, without drawing it: the sessions under a
//! filter and a search, how a folded project rolls them up, and the rows in
//! the order both the drawing and the keyboard read.

use crate::chat::pane::SessionSignal;

/// How many sessions a group lists before a row says how many more there are.
pub(super) const SESSION_CAP: usize = 6;

/// Which sessions the list shows, and how.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Filter {
    /// Under their projects, in the order the person dragged them into.
    ByProject,
    /// Flat, in the order they were started.
    All,
    /// Flat, only those waiting on the person.
    NeedsAttention,
    /// Flat, the latest change of state first.
    Recent,
}

impl Filter {
    pub(super) const ALL: [Filter; 4] = [
        Filter::ByProject,
        Filter::All,
        Filter::NeedsAttention,
        Filter::Recent,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Filter::ByProject => "By project",
            Filter::All => "All sessions",
            Filter::NeedsAttention => "Needs attention",
            Filter::Recent => "Recent activity",
        }
    }
}

/// One session as the list filters and sorts it.
pub(super) struct Item {
    /// Its project, by index into the workspace's projects.
    pub(super) root: usize,
    /// Its place among its project's sessions.
    pub(super) session: usize,
    pub(super) uid: u64,
    pub(super) title: String,
    pub(super) agent: String,
    pub(super) signal: Option<SessionSignal>,
    /// For a lead, the most urgent state of the workers listed under it.
    pub(super) workers: Option<SessionSignal>,
    /// When it took its signal, which the latest-first list sorts by.
    pub(super) since: std::time::Instant,
    /// Whole minutes since then, as the row says it.
    pub(super) age: u64,
    /// Work the agent runs on beyond its turn, still alive.
    pub(super) background: usize,
    /// Subagents the session has working, nested ones included.
    pub(super) subagents: usize,
    /// A turn is open, which is what Stop ends; background work outlives it.
    pub(super) in_turn: bool,
    /// The lead whose worker it is, opening or open.
    pub(super) lead: Option<u64>,
    /// For a lead, the workers under it waiting on the person.
    pub(super) waiting: usize,
    /// For a worker whose lead is listed, the lead's index among the items:
    /// it is drawn under the lead, wherever its own project is. Set by
    /// [`nest_workers`] over the items as they stay: nothing reorders them
    /// after, as lists sort indices into them instead.
    pub(super) under: Option<usize>,
    /// The host its project is on, which a session waiting for it names.
    pub(super) host: Option<String>,
}

impl Item {
    /// The most urgent state of itself and its workers: what the attention
    /// filter and a folded project read, so a waiting worker shows through
    /// its lead wherever the tree is folded or filtered.
    pub(super) fn rolled(&self) -> Option<SessionSignal> {
        SessionSignal::most_urgent(self.signal.into_iter().chain(self.workers))
    }
}

/// A session waits on the person: an answer, or a turn or an agent that broke.
pub(crate) fn needs_attention(signal: Option<SessionSignal>) -> bool {
    matches!(
        signal,
        Some(SessionSignal::Lost | SessionSignal::Failed | SessionSignal::AwaitingUser)
    )
}

/// What a session's state is called on its row; a session with no signal is
/// idle.
pub(super) fn word(signal: Option<SessionSignal>) -> &'static str {
    signal.map_or("Idle", SessionSignal::word)
}

/// A state's word on its row: a session waiting for its host names it.
pub(crate) fn state_word(signal: Option<SessionSignal>, host: Option<&str>) -> String {
    match (signal, host) {
        (Some(SessionSignal::HostWait), Some(host)) => format!("Waiting for {host}"),
        (Some(SessionSignal::HostLost), Some(host)) => format!("Not connected to {host}"),
        _ => word(signal).to_string(),
    }
}

/// The age as the row says it: `now`, `4m`, `2h`, `3d`.
pub(super) fn age_label(minutes: u64) -> String {
    match minutes {
        0 => "now".into(),
        m if m < 60 => format!("{m}m"),
        m if m < 60 * 24 => format!("{}h", m / 60),
        m => format!("{}d", m / (60 * 24)),
    }
}

/// The line under a session's title, in one order for every state:
/// `state · who · age`, where who is the agent, or the project in a flat list.
/// A lead's waiting workers, then live background work and working subagents,
/// are counted after the state, and only when there are some.
pub(super) fn meta(
    signal: Option<SessionSignal>,
    host: Option<&str>,
    background: usize,
    subagents: usize,
    waiting: usize,
    who: &str,
    age: u64,
) -> String {
    let age = age_label(age);
    let age = match signal {
        Some(SessionSignal::AwaitingUser) => format!("waiting {age}"),
        _ => age,
    };
    let mut state = state_word(signal, host);
    if let Some(said) = workers_said(waiting) {
        state += &format!(" · {said}");
    }
    if background > 0 {
        state += &format!(" · {background} in background");
    }
    match subagents {
        0 => {}
        1 => state += " · 1 subagent",
        n => state += &format!(" · {n} subagents"),
    }
    format!("{state} · {who} · {age}")
}

/// A lead's waiting workers, as its row counts them; nothing for none.
pub(super) fn workers_said(waiting: usize) -> Option<String> {
    match waiting {
        0 => None,
        1 => Some("1 worker needs you".into()),
        n => Some(format!("{n} workers need you")),
    }
}

/// Where [`workers_said`] sits in [`meta`]'s line, right after the state's
/// word, so its ink lands on its own bytes; nothing when none wait.
pub(super) fn waiting_span(
    signal: Option<SessionSignal>,
    host: Option<&str>,
    waiting: usize,
) -> Option<std::ops::Range<usize>> {
    let start = state_word(signal, host).len() + " · ".len();
    workers_said(waiting).map(|said| start..start + said.len())
}

/// The sessions a filter and a query keep, as indices into `items`, in the
/// order they are listed. The query matches a title or the project's name,
/// ignoring case; `projects` names each project by its index. Only the top of
/// each tree is listed here: a lead is kept when it or a worker under it
/// matches, and its workers follow it in [`rows`].
pub(super) fn visible(
    items: &[Item],
    projects: &[&str],
    filter: Filter,
    query: &str,
) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    let matches = |s: &Item| {
        query.is_empty()
            || s.title.to_lowercase().contains(&query)
            || projects
                .get(s.root)
                .is_some_and(|name| name.to_lowercase().contains(&query))
    };
    let mut out: Vec<usize> = (0..items.len())
        .filter(|i| {
            let s = &items[*i];
            s.under.is_none()
                && (matches(s) || workers_under(items, *i).any(|w| matches(&items[w])))
                && (filter != Filter::NeedsAttention || needs_attention(s.rolled()))
        })
        .collect();
    match filter {
        Filter::ByProject => {}
        // `uid` is the workspace-wide creation counter.
        Filter::All | Filter::NeedsAttention => out.sort_by_key(|i| items[*i].uid),
        Filter::Recent => out.sort_by_key(|i| std::cmp::Reverse(items[*i].since)),
    }
    out
}

/// The workers listed under the item at `lead`, in the order it opened them.
// ponytail: a scan of every item per lead, quadratic in a window's sessions;
// group the workers by lead once in `nest_workers` if a window ever holds
// hundreds.
fn workers_under(items: &[Item], lead: usize) -> impl Iterator<Item = usize> + '_ {
    let mut mine: Vec<usize> = (0..items.len())
        .filter(|w| items[*w].under == Some(lead))
        .collect();
    mine.sort_by_key(|w| items[*w].uid);
    mine.into_iter()
}

/// Each worker whose lead is listed linked to it, to be drawn under it. A
/// lead keeps its own state on its row, so it never reads as failed for a
/// worker that did; it counts its workers waiting on the person, and rolls
/// up the most urgent state of itself and them for what looks past the row.
/// A worker whose lead is not listed stays a row of its own, as any session.
pub(super) fn nest_workers(items: &mut [Item]) {
    for w in 0..items.len() {
        items[w].under = items[w]
            .lead
            .and_then(|lead| items.iter().position(|item| item.uid == lead));
    }
    // One level only: a worker switched on to lead workers of its own keeps
    // its place under its lead, and its workers are rows of their own.
    let nested: Vec<bool> = items.iter().map(|item| item.under.is_some()).collect();
    for item in items.iter_mut() {
        if item.under.is_some_and(|lead| nested[lead]) {
            item.under = None;
        }
    }
    for lead in 0..items.len() {
        let mine: Vec<Option<SessionSignal>> = items
            .iter()
            .filter(|w| w.under == Some(lead))
            .map(|w| w.signal)
            .collect();
        items[lead].waiting = mine.iter().filter(|s| needs_attention(**s)).count();
        items[lead].workers = SessionSignal::most_urgent(mine.into_iter().flatten());
    }
}

/// The projects that have sessions and all of them drawn under a lead
/// elsewhere: a worktree holding only workers.
pub(super) fn emptied(items: &[Item]) -> Vec<usize> {
    let mut roots: Vec<usize> = items
        .iter()
        .map(|item| item.root)
        .filter(|root| {
            items
                .iter()
                .all(|item| item.root != *root || item.under.is_some())
        })
        .collect();
    roots.sort_unstable();
    roots.dedup();
    roots
}

pub(super) fn attention_count(items: &[Item]) -> usize {
    items.iter().filter(|s| needs_attention(s.signal)).count()
}

/// What a folded project shows: the most urgent state that waits on the
/// person, else running while any session runs, else nothing. Done is not
/// waiting on anyone.
pub(super) fn badge(items: &[Item], root: usize) -> Option<SessionSignal> {
    // A worker counts through its lead's rolled state, never twice.
    let mine = || {
        items
            .iter()
            .filter(move |s| s.root == root && s.under.is_none())
    };
    SessionSignal::most_urgent(
        mine().filter_map(|s| s.rolled().filter(|&state| needs_attention(Some(state)))),
    )
    .or_else(|| {
        mine()
            .any(|s| s.rolled() == Some(SessionSignal::Busy))
            .then_some(SessionSignal::Busy)
    })
}

/// The attention chip's click: on, or back to the filter it replaced.
pub(super) fn toggle_attention(current: Filter, before: Filter) -> Filter {
    if current == Filter::NeedsAttention {
        before
    } else {
        Filter::NeedsAttention
    }
}

/// One row of the list, before it is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Row {
    /// Air between two projects.
    Gap,
    Project {
        root: usize,
        open: bool,
    },
    Session {
        item: usize,
        place: Place,
    },
    /// An open project with no sessions.
    Empty,
    /// What the cap left out of a project's sessions, or of a flat list's.
    More {
        group: Option<usize>,
        hidden: usize,
    },
    NoMatch,
    /// The way to grow the list.
    AddProject,
    /// The list's last row: the way to grow it with a folder on a host.
    AddHostProject,
}

/// Where a session's row stands: in a flat list or a project's tree, and
/// under its lead or not.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Place {
    pub(super) flat: bool,
    /// A worker drawn under its lead's row.
    pub(super) nested: bool,
}

impl Place {
    /// How many indents its row is set in: one under its project, one more
    /// under its lead.
    pub(super) fn depth(self) -> u8 {
        u8::from(!self.flat) + u8::from(self.nested)
    }

    /// Whether its line names its project rather than its agent: in a flat
    /// list, unless it stands under its lead, whose tree is its project.
    pub(super) fn names_project(self) -> bool {
        self.flat && !self.nested
    }

    /// Whether it is dragged to reorder: only a project's own rows, as a
    /// worker's place follows its lead's.
    pub(super) fn drags(self) -> bool {
        !self.flat && !self.nested
    }
}

/// What the rows are built from, besides the sessions.
pub(super) struct Shape<'a> {
    /// Every project's name, by index.
    pub(super) projects: &'a [&'a str],
    /// The projects in the order they are drawn: pinned first.
    pub(super) order: &'a [usize],
    pub(super) filter: Filter,
    pub(super) query: &'a str,
    /// Whether a project's sessions are showing, outside a search.
    pub(super) open: &'a dyn Fn(usize) -> bool,
    /// Whether a group was shown in full past the cap: a project, or `None`
    /// for the flat list.
    pub(super) uncapped: &'a dyn Fn(Option<usize>) -> bool,
}

/// The list as it is drawn, before it is drawn: one source for the elements
/// and for the order the keyboard walks, so the two never differ. A search
/// opens every project, so no match hides behind a fold, and leaves out the
/// projects with nothing that matches.
pub(super) fn rows(items: &[Item], shape: &Shape) -> Vec<Row> {
    let query = shape.query.trim().to_lowercase();
    let searching = !query.is_empty();
    let shown = visible(items, shape.projects, shape.filter, &query);
    let mut rows = Vec::new();
    if shape.filter != Filter::ByProject {
        capped(&mut rows, items, None, shown, true, shape);
    } else {
        for &root in shape.order {
            let mine: Vec<usize> = shown
                .iter()
                .copied()
                .filter(|s| items[*s].root == root)
                .collect();
            let named = shape.projects[root].to_lowercase().contains(&query);
            if searching && mine.is_empty() && !named {
                continue;
            }
            let open = searching || (shape.open)(root);
            if !rows.is_empty() {
                rows.push(Row::Gap);
            }
            rows.push(Row::Project { root, open });
            if !open {
                continue;
            }
            if mine.is_empty() {
                rows.push(Row::Empty);
            }
            capped(&mut rows, items, Some(root), mine, false, shape);
        }
    }
    if rows.is_empty() && (searching || shape.filter != Filter::ByProject) {
        rows.push(Row::NoMatch);
    }
    rows.extend([Row::AddProject, Row::AddHostProject]);
    rows
}

/// A group's sessions up to [`SESSION_CAP`], each lead followed by its
/// workers, and a row saying how many the cap left out, until the person asks
/// for all of them. A lead and its workers count as one, so the cap never
/// cuts a tree.
fn capped(
    rows: &mut Vec<Row>,
    items: &[Item],
    group: Option<usize>,
    sessions: Vec<usize>,
    flat: bool,
    shape: &Shape,
) {
    let cap = if (shape.uncapped)(group) {
        sessions.len()
    } else {
        SESSION_CAP
    };
    let hidden = sessions.len().saturating_sub(cap);
    for item in sessions.into_iter().take(cap) {
        rows.push(Row::Session {
            item,
            place: Place {
                flat,
                nested: false,
            },
        });
        rows.extend(workers_under(items, item).map(|item| Row::Session {
            item,
            place: Place { flat, nested: true },
        }));
    }
    if hidden > 0 {
        rows.push(Row::More { group, hidden });
    }
}

/// The sessions in the order the list draws them, as indices into the items.
pub(super) fn order(rows: &[Row]) -> Vec<usize> {
    rows.iter()
        .filter_map(|row| match row {
            Row::Session { item, .. } => Some(*item),
            Row::Gap
            | Row::Project { .. }
            | Row::Empty
            | Row::More { .. }
            | Row::NoMatch
            | Row::AddProject
            | Row::AddHostProject => None,
        })
        .collect()
}
