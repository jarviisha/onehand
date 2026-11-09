//! A new task as the launcher describes it, before its place is asked for:
//! what blocks it, where it works, and the agent it runs.

use super::Task;
use crate::preflight::Finding;
use crate::workflow::{Brief, Place, Setup, Template};
use std::path::{Path, PathBuf};

/// A launch that may go: a workflow, its brief and its setup.
#[derive(Debug)]
pub struct Launch {
    template: Template,
    brief: Brief,
    setup: Setup,
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
        })
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

    /// It works on the worktree at `dir`, on `branch`, going to the forge
    /// `forge` when one serves it.
    pub fn on_worktree(mut self, dir: PathBuf, branch: String, forge: Option<String>) -> Self {
        self.setup = Setup {
            dir,
            branch: Some(branch),
            forge,
            ..self.setup
        };
        self
    }

    /// The new task, as task `id`.
    pub fn task(self, id: String) -> Task {
        Task::new(id, self.template, self.brief, self.setup)
    }
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
            .on_worktree("/w".into(), "workflow/go".into(), Some("github".into()))
            .task("1".into());
        assert_eq!(task.setup.dir, PathBuf::from("/w"));
        assert_eq!(task.setup.repo, PathBuf::from("/a"));
        assert_eq!(task.setup.branch.as_deref(), Some("workflow/go"));
        assert_eq!(task.setup.agent.as_deref(), Some("claude"));
    }
}
