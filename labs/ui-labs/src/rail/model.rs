//! What the rail draws, without drawing it: the projects, the sessions with
//! one status each, the filters, and the rules that pick and roll them up.

pub(super) struct Project {
    pub(super) name: &'static str,
    pub(super) branch: &'static str,
    /// Files changed and not committed.
    pub(super) changes: usize,
    /// Commits the branch is ahead of and behind its remote. Sample values:
    /// the lab reads no repository.
    pub(super) ahead: usize,
    pub(super) behind: usize,
    /// An unattended run: what it is doing, said by the tooltip and the menu.
    pub(super) auto: Option<&'static str>,
}

pub(super) static PROJECTS: [Project; 3] = [
    Project {
        name: "atlas-api",
        branch: "main",
        changes: 3,
        ahead: 2,
        behind: 0,
        auto: Some("Unattended run working on issue 42"),
    },
    Project {
        name: "atlas-api-internal-billing-reconciliation-service",
        branch: "main",
        changes: 0,
        ahead: 0,
        behind: 0,
        auto: None,
    },
    Project {
        name: "docs-site",
        branch: "gh-pages",
        changes: 0,
        ahead: 0,
        behind: 1,
        auto: None,
    },
];

/// A session's one status, most urgent first, so the least of a project's
/// statuses is the one its folded row rolls up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Status {
    Failed,
    NeedsInput,
    Running,
    DoneUnread,
    Idle,
}

impl Status {
    pub(super) fn label(self) -> &'static str {
        match self {
            Status::Failed => "Failed",
            Status::NeedsInput => "Needs input",
            Status::Running => "Running",
            Status::DoneUnread => "Done",
            Status::Idle => "Idle",
        }
    }

    /// Waits on the person: the count in the Sessions header, the filter, and
    /// what a folded project rolls up.
    pub(super) fn needs_attention(self) -> bool {
        matches!(self, Status::NeedsInput | Status::Failed)
    }
}

#[derive(Clone)]
pub(super) struct Session {
    pub(super) project: usize,
    pub(super) title: &'static str,
    pub(super) agent: &'static str,
    pub(super) status: Status,
    /// Minutes in its current status: for one that needs input, how long it
    /// has waited on the person.
    pub(super) age: u32,
    /// Lines added and removed, when the session has changed files.
    pub(super) diff: Option<(u32, u32)>,
}

impl Session {
    /// The line under the title, in one order for every status:
    /// `status · who · time`. The diff is drawn apart, never in its place.
    pub(super) fn meta(&self, who: &str) -> String {
        let time = self.age_label();
        let time = if self.status == Status::NeedsInput {
            format!("waiting {time}")
        } else {
            time
        };
        format!("{} · {who} · {time}", self.status.label())
    }

    /// The age as the row says it: `4m`, `2h`, `3d`.
    pub(super) fn age_label(&self) -> String {
        match self.age {
            0 => "now".into(),
            m if m < 60 => format!("{m}m"),
            m if m < 60 * 24 => format!("{}h", m / 60),
            m => format!("{}d", m / (60 * 24)),
        }
    }
}

pub(super) fn seed() -> Vec<Session> {
    let s = |project, title, agent, status, age, diff| Session {
        project,
        title,
        agent,
        status,
        age,
        diff,
    };
    vec![
        s(
            0,
            "Inspect dashboard",
            "claude",
            Status::NeedsInput,
            4,
            None,
        ),
        s(
            0,
            "Fix flaky retry test",
            "claude",
            Status::Running,
            1,
            Some((12, 3)),
        ),
        s(0, "Audit interface fast", "codex", Status::Failed, 25, None),
        s(
            0,
            "Rename the retry configuration keys across both services",
            "claude",
            Status::Idle,
            2 * 60 * 24,
            None,
        ),
        s(
            1,
            "Reconcile ledger totals",
            "claude",
            Status::DoneUnread,
            90,
            Some((48, 7)),
        ),
        s(
            1,
            "Backfill ledger exports",
            "codex",
            Status::Running,
            12,
            None,
        ),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Filter {
    ByProject,
    All,
    NeedsAttention,
    Newest,
}

impl Filter {
    pub(super) const ALL: [Filter; 4] = [
        Filter::ByProject,
        Filter::All,
        Filter::NeedsAttention,
        Filter::Newest,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Filter::ByProject => "By project",
            Filter::All => "All sessions",
            Filter::NeedsAttention => "Needs attention",
            Filter::Newest => "Newest first",
        }
    }
}

/// The sessions a filter and a query keep, in the order they are listed. The
/// query matches a title or a project's name, ignoring case.
pub(super) fn visible(sessions: &[Session], filter: Filter, query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    let mut out: Vec<usize> = (0..sessions.len())
        .filter(|i| {
            let s = &sessions[*i];
            (query.is_empty()
                || s.title.to_lowercase().contains(&query)
                || PROJECTS[s.project].name.to_lowercase().contains(&query))
                && (filter != Filter::NeedsAttention || s.status.needs_attention())
        })
        .collect();
    if filter == Filter::Newest {
        out.sort_by_key(|i| sessions[*i].age);
    }
    out
}

pub(super) fn attention_count(sessions: &[Session]) -> usize {
    sessions
        .iter()
        .filter(|s| s.status.needs_attention())
        .count()
}

/// What a folded project shows: the most urgent status that waits on the
/// person, else Running while any session runs, else nothing.
pub(super) fn badge(sessions: &[Session], project: usize) -> Option<Status> {
    let mine = sessions.iter().filter(|s| s.project == project);
    mine.clone()
        .map(|s| s.status)
        .filter(|s| s.needs_attention())
        .min()
        .or_else(|| {
            mine.clone()
                .any(|s| s.status == Status::Running)
                .then_some(Status::Running)
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
