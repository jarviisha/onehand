//! onehand's tools as they are listed: each one's name, what it does, its
//! arguments' schema and who may call it, in one table that the list, the
//! allow-check and the reading of a call's arguments all read.

use serde_json::{json, Value};

/// One of onehand's tools: everything about it a caller is told or held to,
/// save how its arguments become a [`super::Call`], which `parse` there reads.
pub(super) struct Tool {
    pub(super) name: &'static str,
    /// What it does, as the agent reads it.
    about: &'static str,
    /// Its arguments' schema. The arguments a call may pass are its
    /// `properties`, and no others.
    pub(super) input: Value,
    /// About a session's workers: an outside program owns none, so it is
    /// never offered it.
    pub(super) for_sessions_only: bool,
}

impl Tool {
    /// The tool as `tools/list` gives it.
    pub(super) fn listed(&self) -> Value {
        json!({"name": self.name, "description": self.about, "inputSchema": self.input})
    }

    /// Whether a call may pass argument `key`.
    pub(super) fn takes(&self, key: &str) -> bool {
        self.input["properties"]
            .as_object()
            .is_some_and(|properties| properties.contains_key(key))
    }
}

/// Every tool, in the order they are listed, built once.
pub(super) fn tools() -> &'static [Tool] {
    static TOOLS: std::sync::OnceLock<Vec<Tool>> = std::sync::OnceLock::new();
    TOOLS.get_or_init(catalogue)
}

/// The tool named `name`, if there is one.
pub(super) fn tool(name: &str) -> Option<&'static Tool> {
    tools().iter().find(|tool| tool.name == name)
}

/// The tools' names, in the order they are listed: what a schedule may give
/// its sessions.
pub fn tool_names() -> &'static [&'static str] {
    static NAMES: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    NAMES.get_or_init(|| tools().iter().map(|tool| tool.name).collect())
}

/// Each tool, with its description and input schema.
fn catalogue() -> Vec<Tool> {
    let none = || json!({"type": "object", "properties": {}, "additionalProperties": false});
    let text = |about: &str| json!({"type": "string", "description": about});
    let schema = |properties: Value, required: &[&str]| {
        json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
        })
    };
    let project = "A project of the window, by its name or its path";
    // Each: its name, whether it is for a session's agent alone (about its
    // workers), what it does and its arguments.
    let tools = [
        (
            "list_projects",
            false,
            "The projects of the window, with their paths and whether each has a check command",
            none(),
        ),
        (
            "list_agents",
            false,
            "The agents a session can be opened with",
            none(),
        ),
        (
            "open_session",
            false,
            "Open a session on a project with an agent, without showing it, and send it a \
             first prompt if one is given. It appears in the rail, the person's own, and \
             shares the project with any session already on it. To hand a job to a worker \
             you then follow, use open_worker",
            schema(
                json!({
                    "project": text(project),
                    "agent": text("An agent's name, as list_agents gives it"),
                    "prompt": text("The session's first prompt"),
                }),
                &["project", "agent"],
            ),
        ),
        (
            "open_worker",
            true,
            "Open a worker: a session that works a job for you on a folder of its own, shown \
             under you rather than in the rail, which you follow with session_state and give \
             turns with send_prompt. It answers once the worker is up, on its model, its first \
             prompt accepted, with the handle it is named by. A folder another session is open \
             on is refused",
            schema(
                json!({
                    "project": text(
                        "A project of the window, by its name or its path, or the top level of \
                         a git worktree of a repository one of the window's projects belongs to"
                    ),
                    "agent": text("An agent's name, as list_agents gives it"),
                    "prompt": text("The worker's first prompt"),
                    "dirs": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Absolute folders outside the project the worker may \
                                        also write",
                    },
                    "request": text(
                        "Your id for this open: the same id again answers with the worker it \
                         opened rather than opening another"
                    ),
                    "turn": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "The turn number the prompt is, required with a prompt",
                    },
                    "model": text("The model the worker runs, by the agent's own value"),
                    "name": text(
                        "A short name saying what the worker works on, shown wherever it is \
                         listed, at most 40 characters"
                    ),
                }),
                &["project", "agent", "request"],
            ),
        ),
        (
            "list_workflows",
            false,
            "The workflows in the library, by id and name",
            none(),
        ),
        (
            "read_workflow",
            false,
            "A workflow's file, as TOML: what save_workflow takes back once changed",
            schema(
                json!({"workflow": text("A workflow's id, as list_workflows gives it")}),
                &["workflow"],
            ),
        ),
        (
            "save_workflow",
            false,
            "Save a workflow from its TOML, as read_workflow gives it. It goes over the \
             person's own workflow with its id, else into a new one: a shipped workflow \
             (its id starting builtin:) is never changed, only copied under a name of its \
             own. Refused, saying why, when the TOML does not read, when the workflow has \
             problems, or when another workflow has its name",
            schema(
                json!({"toml": text("The whole workflow file, as TOML")}),
                &["toml"],
            ),
        ),
        (
            "start_workflow",
            false,
            "Start a task of a workflow on a project, on a brief, as the Run workflow \
             dialog does. It runs off screen and is listed on the Tasks page; it waits its \
             turn when another task works in the same checkout",
            schema(
                json!({
                    "project": text(project),
                    "workflow": text("A workflow's id, as list_workflows gives it"),
                    "title": text("What to do, in one line"),
                    "brief": text("The details: what, why, and how to tell it is done"),
                }),
                &["project", "workflow", "title"],
            ),
        ),
        (
            "start_issue",
            false,
            "Start a task working one of the project's own issues, the issue as its brief, \
             with a person watching. Nothing is claimed or said on the forge",
            schema(
                json!({
                    "project": text(project),
                    "issue": {"type": "integer", "minimum": 1,
                              "description": "The issue's number in the project's Issues tab"},
                    "workflow": text(
                        "A workflow's id; the one unattended runs use when left out"
                    ),
                }),
                &["project", "issue"],
            ),
        ),
        (
            "list_tasks",
            false,
            "The tasks of the window's projects, with their state and what can be done next",
            schema(json!({"project": text(project)}), &[]),
        ),
        (
            "stop_task",
            false,
            "Stop a task, as the Tasks page's Stop does",
            schema(
                json!({"task": text("A task's id, as list_tasks gives it")}),
                &["task"],
            ),
        ),
        (
            "open_file",
            false,
            "Open a file of the project on screen in the Workbench, without taking focus",
            schema(json!({"path": text("The file's absolute path")}), &["path"]),
        ),
        (
            "list_issues",
            false,
            "The project's own open issues, as its Issues tab keeps them. Read them before \
             writing one, so a problem already written is not written twice",
            schema(
                json!({"project": text("A project of the window; this session's when left out")}),
                &[],
            ),
        ),
        (
            "create_issue",
            false,
            "Write one issue into the project's own issues. An open issue with the same title \
             is not written again; its number is answered instead",
            schema(
                json!({
                    "project": text("A project of the window; this session's when left out"),
                    "title": text("What is wrong, in one line"),
                    "body": text("Where it is (file and line), why it matters, how to see it"),
                }),
                &["title"],
            ),
        ),
        (
            "session_state",
            true,
            "What a worker you opened is doing, one fact per line: its link, its turn and how \
             the last ended, what waits on a person, its activity, who holds it, your turns sent \
             and over, what it runs with. It never says the worker stopped: only `link: closed` \
             does",
            schema(
                json!({"session": text("The worker's handle, as open_worker answered it")}),
                &["session"],
            ),
        ),
        (
            "send_prompt",
            true,
            "Give a worker you opened its next turn. Never queued: refused while a turn runs, \
             while it connects or once its link is lost, and while a person holds it. Each turn \
             number goes once: the same turn and text again answers that it was already sent, \
             so a lost answer can be retried; the same turn with other text is refused",
            schema(
                json!({
                    "session": text("The worker's handle, as open_worker answered it"),
                    "text": text("The turn's prompt"),
                    "turn": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Your number for this turn, above the last you sent",
                    },
                }),
                &["session", "text", "turn"],
            ),
        ),
        (
            "cancel_turn",
            true,
            "Cancel the turn a worker you opened is running, as the person's Stop does. \
             Refused while a permission or a question waits on a person, which only they \
             answer, and while a person holds the worker",
            schema(
                json!({"session": text("The worker's handle, as open_worker answered it")}),
                &["session"],
            ),
        ),
    ];
    tools
        .into_iter()
        .map(|(name, for_sessions_only, about, input)| Tool {
            name,
            about,
            input,
            for_sessions_only,
        })
        .collect()
}
