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
    /// When it took its signal, which the latest-first list sorts by.
    pub(super) since: std::time::Instant,
    /// Whole minutes since then, as the row says it.
    pub(super) age: u64,
}

/// A session waits on the person: an answer, or a turn or an agent that broke.
pub(super) fn needs_attention(signal: Option<SessionSignal>) -> bool {
    matches!(
        signal,
        Some(SessionSignal::Lost | SessionSignal::Failed | SessionSignal::AwaitingUser)
    )
}

/// What a session's state is called on its row; a session with no signal is
/// idle.
pub(super) fn word(signal: Option<SessionSignal>) -> &'static str {
    signal.map_or("Idle", super::signal_word)
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
pub(super) fn meta(signal: Option<SessionSignal>, who: &str, age: u64) -> String {
    let age = age_label(age);
    let age = match signal {
        Some(SessionSignal::AwaitingUser) => format!("waiting {age}"),
        _ => age,
    };
    format!("{} · {who} · {age}", word(signal))
}

/// The sessions a filter and a query keep, as indices into `items`, in the
/// order they are listed. The query matches a title or the project's name,
/// ignoring case; `projects` names each project by its index.
pub(super) fn visible(
    items: &[Item],
    projects: &[&str],
    filter: Filter,
    query: &str,
) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    let mut out: Vec<usize> = (0..items.len())
        .filter(|i| {
            let s = &items[*i];
            (query.is_empty()
                || s.title.to_lowercase().contains(&query)
                || projects
                    .get(s.root)
                    .is_some_and(|name| name.to_lowercase().contains(&query)))
                && (filter != Filter::NeedsAttention || needs_attention(s.signal))
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

pub(super) fn attention_count(items: &[Item]) -> usize {
    items.iter().filter(|s| needs_attention(s.signal)).count()
}

/// What a folded project shows: the most urgent state that waits on the
/// person, else running while any session runs, else nothing. Done is not
/// waiting on anyone.
pub(super) fn badge(items: &[Item], root: usize) -> Option<SessionSignal> {
    let mine = || items.iter().filter(move |s| s.root == root);
    SessionSignal::most_urgent(
        mine().filter_map(|s| s.signal.filter(|_| needs_attention(s.signal))),
    )
    .or_else(|| {
        mine()
            .any(|s| s.signal == Some(SessionSignal::Busy))
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
        flat: bool,
    },
    /// An open project with no sessions.
    Empty,
    /// What the cap left out of a project's sessions, or of a flat list's.
    More {
        group: Option<usize>,
        hidden: usize,
    },
    NoMatch,
    /// The list's last row: the way to grow it.
    AddProject,
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
        capped(&mut rows, None, shown, true, shape);
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
            capped(&mut rows, Some(root), mine, false, shape);
        }
    }
    if rows.is_empty() && (searching || shape.filter != Filter::ByProject) {
        rows.push(Row::NoMatch);
    }
    rows.push(Row::AddProject);
    rows
}

/// A group's sessions up to [`SESSION_CAP`], and a row saying how many the
/// cap left out, until the person asks for all of them.
fn capped(
    rows: &mut Vec<Row>,
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
    rows.extend(
        sessions
            .into_iter()
            .take(cap)
            .map(|item| Row::Session { item, flat }),
    );
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
            | Row::AddProject => None,
        })
        .collect()
}
