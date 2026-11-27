//! A new task as the launcher describes it, or as a schedule or a tool
//! starts one off screen, before its place is asked for: what blocks it,
//! where it works, the agent it runs and where it came from.

use super::{Grant, Origin, Task};
use crate::config::AgentSpec;
use crate::preflight::Finding;
use crate::workflow::{Brief, Current, Offer, Place, Setup, Template};
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
    /// The board's column it starts in, when a board starts it.
    pub column: Option<String>,
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
    column: Option<String>,
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
            column: None,
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
            column: starting.column,
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
        let mut task = Task {
            origin: self.origin,
            grant: self.grant,
            ..Task::new(id, self.template, self.brief, self.setup)
        };
        if let Some(run) = task.runs.first_mut() {
            run.column = self.column;
        }
        task
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
    parent: Option<&Path>,
    connectors: &[&'static dyn crate::connector::Connector],
) -> Result<Worktree, String> {
    // Git checks out repositories, not folders: the worktree is of the
    // repository the project sits in, put beside it, and the run works in the
    // same folder of it the project is.
    let top = crate::worktree::repo_top_blocking(root)
        .ok_or_else(|| format!("{} is not in a git repository", root.display()))?;
    // ponytail: only local branches count here; a `workflow/` branch whose
    // worktree is gone but whose pull request is on the forge is reused. Ask
    // the forge whether the branch has a pull request if that is seen.
    let (branch, made) =
        crate::worktree::cut_blocking(&top, &crate::workflow::branch_for(title), parent)?;
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

/// Where every task's agent builds, shared across tasks.
///
/// A worktree is kept on disk after its task, so a build directory inside
/// each would be gigabytes per task with nothing that ever cleans it; one
/// shared directory also turns the second task's build from cold into
/// incremental. Under the cache directory, because a build is something that
/// can always be made again. Its name is the one earlier builds used, so
/// what they built stays warm.
pub fn target_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|dir| dir.join("onehand").join("unattended-target"))
}

/// `base` as a task starts it: with every build it makes pointed at one
/// shared directory, through the one process a task gets to set anything on.
pub fn run_spec(mut base: AgentSpec) -> AgentSpec {
    if let Some(target) = target_dir() {
        base.env
            .insert("CARGO_TARGET_DIR".into(), target.display().to_string());
    }
    base
}

/// What agent `spec` offered when it last came up, by `offers`, started as
/// it is configured now; a task's agent is started through [`run_spec`], so
/// what it offered counts as the configured spec's too.
pub fn offered(spec: &AgentSpec, offers: &[(AgentSpec, Offer)]) -> Option<Offer> {
    let wrapped = run_spec(spec.clone());
    offers
        .iter()
        .find(|(seen, _)| seen == spec || *seen == wrapped)
        .map(|(_, offer)| offer.clone())
}

/// What a new task would be given now, for a retry with current settings:
/// the first agent configured, the workflow's own timeout and mode, and the
/// project's `check`.
pub fn now_for(first_agent: Option<String>, check: Option<String>) -> Current {
    Current {
        agent: first_agent,
        mode: None,
        check,
        timeout: None,
    }
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
            closes: None,
        }
    }

    #[test]
    fn a_launch_is_refused_by_its_first_block_then_by_no_title() {
        let block = |text: &str| Finding {
            about: About::Workflow,
            blocks: true,
            text: text.into(),
            change: None,
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
            column: None,
        }
    }

    /// A start a board makes in one of its columns: its first run keeps the
    /// column's name, which a run read from an older file has none of.
    #[test]
    fn a_run_keeps_the_column_it_started_in() {
        let given = Starting {
            column: Some("Work".into()),
            ..starting()
        };
        let task = Launch::off_screen(&[], given, None, None)
            .unwrap()
            .task("t".into());
        assert_eq!(task.runs[0].column.as_deref(), Some("Work"));
        let mut old = serde_json::to_value(&task.runs[0]).unwrap();
        old.as_object_mut().unwrap().remove("column");
        let read: crate::workflow::Run = serde_json::from_value(old).unwrap();
        assert!(read.column.is_none());
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

    /// A task's build directory reaches the agent through its environment,
    /// so a preset keeps launching as a preset.
    #[test]
    fn a_task_puts_the_build_directory_in_the_agents_environment() {
        let base = AgentSpec {
            name: "Codex".into(),
            preset: Some(crate::config::Preset::Codex.into()),
            ..Default::default()
        };
        let run = run_spec(base.clone());
        assert_eq!(run.preset, base.preset);
        assert!(run.command.is_empty(), "{:?}", run.command);
        if let Some(dir) = target_dir() {
            assert_eq!(
                run.env.get("CARGO_TARGET_DIR"),
                Some(&dir.display().to_string())
            );
        }
    }
}
