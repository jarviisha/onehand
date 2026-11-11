//! A new task as the launcher describes it, or as a schedule or a tool
//! starts one off screen, before its place is asked for: what blocks it,
//! where it works, the agent it runs and where it came from.

use super::{Grant, Origin, Task};
use crate::preflight::Finding;
use crate::workflow::{Brief, Place, Setup, Template};
use std::path::{Path, PathBuf};

/// What a start off screen is asked for, by a schedule or one of onehand's
/// tools: its project, its workflow, its brief, what its task keeps of where
/// it came from, and the agent and mode it runs with (none: the first agent,
/// and its own mode).
#[derive(Debug, Clone, PartialEq)]
pub struct Starting {
    pub project: PathBuf,
    pub template: Template,
    pub brief: Brief,
    pub origin: Origin,
    pub grant: Grant,
    pub agent: Option<String>,
    pub mode: Option<String>,
}

/// A launch that may go: a workflow, its brief, its setup, and what its task
/// keeps of where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Launch {
    template: Template,
    brief: Brief,
    setup: Setup,
    origin: Option<Origin>,
    grant: Grant,
}

impl Launch {
    /// The run the launcher's fields describe on the project at `root`, with
    /// its `check` command, on `agent` (the first configured, for a task that
    /// names none), or why it cannot run: the first finding of its preflight
    /// that blocks, a workflow that cannot be read, or no title.
    pub fn new(
        found: &[Finding],
        workflow: Result<Template, String>,
        brief: Brief,
        root: &Path,
        check: Option<String>,
        agent: Option<String>,
    ) -> Result<Self, String> {
        if let Some(block) = found.iter().find(|f| f.blocks) {
            return Err(block.text.clone());
        }
        let template = workflow?;
        if brief.title.is_empty() {
            return Err("Say what to do in the title".to_string());
        }
        let setup = Setup {
            repo: root.to_path_buf(),
            dir: root.to_path_buf(),
            branch: None,
            agent,
            check,
            mode: None,
            forge: None,
        };
        Ok(Self {
            template,
            brief,
            setup,
            origin: None,
            grant: Grant::default(),
        })
    }

    /// `starting`, off screen, on the project's `check` command, on the agent
    /// it names or else `first`, or why it cannot: every finding of its
    /// preflight that blocks, said together, since nobody is at a launcher
    /// to fix them one at a time.
    pub fn off_screen(
        found: &[Finding],
        starting: Starting,
        check: Option<String>,
        first: Option<String>,
    ) -> Result<Self, String> {
        let blocks: Vec<&str> = found
            .iter()
            .filter(|f| f.blocks)
            .map(|f| f.text.as_str())
            .collect();
        if !blocks.is_empty() {
            return Err(blocks.join("; "));
        }
        let setup = Setup {
            repo: starting.project.clone(),
            dir: starting.project,
            branch: None,
            agent: starting.agent,
            check,
            mode: starting.mode,
            forge: None,
        };
        Ok(Self {
            template: starting.template,
            brief: starting.brief,
            setup: on_agent(setup, first),
            origin: Some(starting.origin),
            grant: starting.grant,
        })
    }

    /// The repository a worktree cut for it is of.
    pub fn repo(&self) -> &Path {
        &self.setup.repo
    }

    /// The folder it works in.
    pub fn dir(&self) -> &Path {
        &self.setup.dir
    }

    /// Whether its workflow works on a branch in a worktree, cut for it
    /// before its place is asked for: its own place, so it never waits.
    pub fn cuts(&self) -> bool {
        self.template.place == Place::Worktree
    }

    /// The title a worktree cut for it is named for.
    pub fn title(&self) -> &str {
        &self.brief.title
    }

    /// It works on the worktree `cut` for it.
    pub fn on_worktree(mut self, cut: Worktree) -> Self {
        self.setup = Setup {
            dir: cut.dir,
            branch: Some(cut.branch),
            forge: cut.forge,
            ..self.setup
        };
        self
    }

    /// The new task, as task `id`.
    pub fn task(self, id: String) -> Task {
        Task {
            origin: self.origin,
            grant: self.grant,
            ..Task::new(id, self.template, self.brief, self.setup)
        }
    }
}

/// A worktree cut for a new task: the folder it works in, its branch, and
/// the forge the branch goes to, none when no forge serves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub dir: PathBuf,
    pub branch: String,
    pub forge: Option<String>,
}

/// A worktree of the repository `root` sits in, cut for a task titled
/// `title`, its forge the first of `connectors` serving it. Blocking.
pub fn cut_blocking(
    root: &Path,
    title: &str,
    connectors: &[&'static dyn crate::connector::Connector],
) -> Result<Worktree, String> {
    // Git checks out repositories, not folders: the worktree is of the
    // repository the project sits in, put beside it, and the run works in the
    // same folder of it the project is.
    let top = crate::worktree::repo_top_blocking(root)
        .ok_or_else(|| format!("{} is not in a git repository", root.display()))?;
    // ponytail: only local branches count here; a `workflow/` branch whose
    // worktree is gone but whose pull request is on the forge is reused. Ask
    // the forge as an issue's claim does if that is seen.
    let (branch, made) =
        crate::worktree::cut_blocking(&top, &crate::workflow::branch_for(title), "HEAD", |_| {
            Ok(false)
        })?;
    let dir = crate::worktree::subtree_or(made, &top, root);
    let forge = crate::connector::serving(connectors, &top)
        .ok()
        .map(|at| connectors[at].name().to_string());
    Ok(Worktree { dir, branch, forge })
}

/// `setup`, on the agent it names or, naming none, on `first`: the one a
/// new session takes.
pub fn on_agent(mut setup: Setup, first: Option<String>) -> Setup {
    setup.agent = setup.agent.or(first);
    setup
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preflight::About;
    use crate::workflow::builtin;

    fn brief(title: &str) -> Brief {
        Brief {
            title: title.into(),
            body: String::new(),
            instructions: None,
        }
    }

    #[test]
    fn a_launch_is_refused_by_its_first_block_then_by_no_title() {
        let block = |text: &str| Finding {
            about: About::Workflow,
            blocks: true,
            text: text.into(),
            change: None,
            task: None,
            checkable: false,
        };
        let template = || Ok(builtin::all().remove(0));
        let root = Path::new("/a");
        let found = [block("first"), block("second")];
        let refused = Launch::new(&found, template(), brief("Go"), root, None, None);
        assert_eq!(refused.unwrap_err(), "first");
        let refused = Launch::new(&[], template(), brief(""), root, None, None);
        assert_eq!(refused.unwrap_err(), "Say what to do in the title");
    }

    fn block(text: &str) -> Finding {
        Finding {
            about: About::Workflow,
            blocks: true,
            text: text.into(),
            change: None,
            task: None,
            checkable: false,
        }
    }

    fn starting() -> Starting {
        Starting {
            project: "/a".into(),
            template: builtin::all().remove(0),
            brief: brief(""),
            origin: Origin::Tool,
            grant: Grant::default(),
            agent: None,
            mode: Some("plan".into()),
        }
    }

    /// Off screen, every block is said at once, and a start with no title
    /// is not refused for it.
    #[test]
    fn a_start_off_screen_is_refused_by_every_block_together() {
        let found = [block("first"), block("second")];
        let refused = Launch::off_screen(&found, starting(), None, None);
        assert_eq!(refused.unwrap_err(), "first; second");
        assert!(Launch::off_screen(&[], starting(), None, None).is_ok());
    }

    /// Its task keeps where it came from, the first agent when it names
    /// none, its mode and the project's check.
    #[test]
    fn a_start_off_screen_keeps_its_origin_agent_and_mode() {
        let launch = Launch::off_screen(&[], starting(), Some("make".into()), Some("a".into()));
        let task = launch.unwrap().task("1".into());
        assert_eq!(task.origin, Some(Origin::Tool));
        assert_eq!(task.setup.agent.as_deref(), Some("a"));
        assert_eq!(task.setup.mode.as_deref(), Some("plan"));
        assert_eq!(task.setup.check.as_deref(), Some("make"));
        assert_eq!(task.setup.dir, PathBuf::from("/a"));
    }

    #[test]
    fn a_worktree_launch_works_on_the_branch_cut_for_it() {
        let worktree = builtin::all()
            .into_iter()
            .find(|t| t.place == Place::Worktree)
            .unwrap();
        let root = Path::new("/a");
        let launch = Launch::new(
            &[],
            Ok(worktree),
            brief("Go"),
            root,
            None,
            Some("claude".into()),
        );
        let launch = launch.unwrap();
        assert!(launch.cuts());
        let task = launch
            .on_worktree(Worktree {
                dir: "/w".into(),
                branch: "workflow/go".into(),
                forge: Some("github".into()),
            })
            .task("1".into());
        assert_eq!(task.setup.dir, PathBuf::from("/w"));
        assert_eq!(task.setup.repo, PathBuf::from("/a"));
        assert_eq!(task.setup.branch.as_deref(), Some("workflow/go"));
        assert_eq!(task.setup.agent.as_deref(), Some("claude"));
    }
}
