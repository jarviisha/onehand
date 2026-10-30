//! onehand's own tools, as an MCP server answers for them.
//!
//! Everything that controls onehand from outside a window speaks MCP over the
//! socket beside the instance lock: one JSON-RPC message per line. What may be
//! asked is decided here, once — the catalogue, how a call's arguments are
//! read and refused, and how each message is answered given who sent it —
//! so the socket and the app only carry it. The app performs a [`Call`] and
//! answers with the text the caller reads.
//!
//! **No tool answers a card, a permission or an approval.** Those wait for a
//! person, whoever asks; there is nothing here to call for them.

pub mod open;
pub mod state;
pub mod turns;

use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// What a tool call asks the app to do, its arguments read and checked.
#[derive(Debug, PartialEq)]
pub enum Call {
    ListProjects,
    ListAgents,
    ListWorkflows,
    ReadWorkflow {
        workflow: String,
    },
    SaveWorkflow {
        text: String,
    },
    StartWorkflow {
        project: String,
        workflow: String,
        title: String,
        brief: Option<String>,
    },
    StartIssue {
        project: String,
        issue: u64,
        workflow: Option<String>,
    },
    ListTasks {
        project: Option<String>,
    },
    StopTask {
        task: String,
    },
    OpenFile {
        path: PathBuf,
    },
    ListIssues {
        project: Option<String>,
    },
    CreateIssue {
        project: Option<String>,
        title: String,
        body: String,
    },
    OpenSession {
        project: String,
        agent: String,
        prompt: Option<String>,
        /// Folders outside the project the session's agent may also write.
        dirs: Vec<PathBuf>,
        /// The caller's id for this open: the same id again answers with
        /// the same session rather than opening a second.
        request: Option<String>,
        /// The turn number `prompt` is, in the caller's count.
        turn: Option<u64>,
        /// The model the session runs, by the agent's own value.
        model: Option<String>,
        /// What the session is called wherever it is listed, its spaces
        /// collapsed.
        name: Option<String>,
    },
    SessionState {
        session: String,
    },
    SendPrompt {
        session: String,
        text: String,
        turn: u64,
    },
    CancelTurn {
        session: String,
    },
}

/// Who a connection speaks for, fixed by its `initialize`.
#[derive(Clone, Debug, PartialEq)]
pub enum Peer {
    /// A program outside onehand: a script, the system's cron.
    Outside,
    /// The agent of the session this token was given to, through the proxy
    /// onehand starts for it.
    Session(String),
}

/// What a connection may do right now. Asked for on every message, so a
/// switch turned off holds from the next one.
#[derive(Debug)]
pub enum Standing {
    /// Every tool.
    On,
    /// Connected, and allowed nothing until switched on: the tools are not
    /// listed and no call is made.
    Off,
    /// These tools and no other, by name: a schedule's session. Only they
    /// are listed, and a call to another is refused whatever asked for it.
    Only(Vec<String>),
    /// Not allowed to talk at all, and why.
    Refused(String),
}

/// One connection's state.
#[derive(Debug, Default)]
pub struct Conn {
    peer: Option<Peer>,
}

impl Conn {
    /// Who it speaks for, once its `initialize` is answered.
    pub fn peer(&self) -> Option<&Peer> {
        self.peer.as_ref()
    }
}

/// What one line comes to.
#[derive(Debug)]
pub enum Handled {
    /// Write this line back.
    Reply(String),
    /// Perform `call`, then write [`answered`] for `id`.
    Perform { id: Value, call: Box<Call> },
    /// A notification: nothing is written.
    Quiet,
}

/// The MCP version answered when the client names none.
const PROTOCOL_VERSION: &str = "2025-06-18";

/// The JSON-RPC error a connection gets when it may not talk.
const NOT_ALLOWED: i64 = -32001;
/// The one a call gets before the connection said who it is.
const NOT_INITIALIZED: i64 = -32002;

/// Answer one line read off `conn`, `standing` saying what its peer may do.
pub fn handle(conn: &mut Conn, line: &str, standing: impl Fn(&Peer) -> Standing) -> Handled {
    let Ok(msg) = serde_json::from_str::<Value>(line) else {
        return Handled::Reply(error(&Value::Null, -32700, "not a JSON-RPC line"));
    };
    // A batch, or a value that is no message at all, is answered rather than
    // taken for a notification: a client waiting on it would wait for ever.
    if !msg.is_object() {
        return Handled::Reply(error(
            &Value::Null,
            -32600,
            "one message per line, as an object",
        ));
    }
    let Some(id) = msg.get("id").cloned() else {
        return Handled::Quiet;
    };
    let params = &msg["params"];
    let Some(method) = msg["method"].as_str() else {
        return Handled::Reply(error(&id, -32600, "a request names its method"));
    };
    let reply = |result: Value| Handled::Reply(ok(&id, result));
    let fail = |code: i64, why: &str| Handled::Reply(error(&id, code, why));
    match method {
        "initialize" => {
            let peer = match params["_meta"]["onehand"]["session"].as_str() {
                Some(token) => Peer::Session(token.to_string()),
                None => Peer::Outside,
            };
            if let Standing::Refused(why) = standing(&peer) {
                return fail(NOT_ALLOWED, &why);
            }
            conn.peer = Some(peer);
            let version = params["protocolVersion"]
                .as_str()
                .unwrap_or(PROTOCOL_VERSION);
            reply(json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": true}},
                "serverInfo": {"name": "onehand", "version": env!("CARGO_PKG_VERSION")},
            }))
        }
        "ping" => reply(json!({})),
        "tools/list" | "tools/call" => {
            let Some(peer) = &conn.peer else {
                return fail(NOT_INITIALIZED, "initialize first");
            };
            let standing = standing(peer);
            if let Standing::Refused(why) = &standing {
                return fail(NOT_ALLOWED, why);
            }
            let outside = matches!(peer, Peer::Outside);
            let allowed = |name: &str| match &standing {
                _ if outside && SESSION_ONLY.contains(&name) => false,
                Standing::On => true,
                Standing::Only(names) => names.iter().any(|n| n == name),
                Standing::Off | Standing::Refused(_) => false,
            };
            if method == "tools/list" {
                let tools: Vec<Value> = catalogue()
                    .into_iter()
                    .filter(|tool| tool["name"].as_str().is_some_and(allowed))
                    .collect();
                return reply(json!({ "tools": tools }));
            }
            let name = params["name"].as_str().unwrap_or("");
            if !TOOLS.contains(&name) {
                return fail(-32602, &format!("no tool named `{name}`"));
            }
            if !allowed(name) {
                let why = match standing {
                    _ if outside && SESSION_ONLY.contains(&name) => {
                        format!("`{name}` is for a session's agent, about a worker it opened")
                    }
                    Standing::Only(_) => format!("`{name}` is not among this session's tools"),
                    _ => "onehand's tools are switched off for this session".to_string(),
                };
                return Handled::Reply(answered(&id, Err(why)));
            }
            match parse(name, &params["arguments"]) {
                Ok(call) => Handled::Perform {
                    id,
                    call: Box::new(call),
                },
                Err(why) => Handled::Reply(answered(&id, Err(why))),
            }
        }
        _ => fail(-32601, &format!("no method `{method}`")),
    }
}

/// The reply to call `id`, whose outcome is `outcome`: the text the caller
/// reads, marked an error when it was refused.
pub fn answered(id: &Value, outcome: Result<String, String>) -> String {
    let (text, refused) = match outcome {
        Ok(text) => (text, false),
        Err(why) => (why, true),
    };
    ok(
        id,
        json!({"content": [{"type": "text", "text": text}], "isError": refused}),
    )
}

fn ok(id: &Value, result: Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string()
}

fn error(id: &Value, code: i64, message: &str) -> String {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}).to_string()
}

/// The tools, in the order they are listed.
const TOOLS: [&str; 16] = [
    "list_projects",
    "list_agents",
    "open_session",
    "list_workflows",
    "read_workflow",
    "save_workflow",
    "start_workflow",
    "start_issue",
    "list_tasks",
    "stop_task",
    "open_file",
    "list_issues",
    "create_issue",
    "session_state",
    "send_prompt",
    "cancel_turn",
];

/// The tools about a session's workers: an outside program owns none, so it
/// is never offered them.
const SESSION_ONLY: [&str; 3] = ["session_state", "send_prompt", "cancel_turn"];

/// The tools' names, in the order they are listed: what a schedule may give
/// its sessions.
pub fn tool_names() -> &'static [&'static str] {
    &TOOLS
}

/// Each tool's description and input schema, as `tools/list` gives them.
fn catalogue() -> Vec<Value> {
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
    let tools = [
        (
            "list_projects",
            "The projects of the window, with their paths and whether each has a check command",
            none(),
        ),
        (
            "list_agents",
            "The agents a session can be opened with",
            none(),
        ),
        (
            "open_session",
            "Open a session on a project with an agent, without showing it, and send it a \
             first prompt if one is given. It appears in the rail. For a session's agent it \
             answers once the session is up, on its model, its first prompt accepted, with \
             the handle the session is named by",
            schema(
                json!({
                    "project": text(
                        "A project of the window, by its name or its path; for a session's \
                         agent, also the top level of a git worktree of a repository one of \
                         the window's projects belongs to"
                    ),
                    "agent": text("An agent's name, as list_agents gives it"),
                    "prompt": text("The session's first prompt"),
                    "dirs": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Absolute folders outside the project the session's \
                                        agent may also write, for a session's agent only",
                    },
                    "request": text(
                        "Your id for this open, required from a session's agent: the same id \
                         again answers with the session it opened rather than opening another"
                    ),
                    "turn": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "The turn number the prompt is, required with a prompt \
                                        from a session's agent",
                    },
                    "model": text("The model the session runs, by the agent's own value"),
                    "name": text(
                        "A short name saying the session's job, shown wherever it is listed, \
                         at most 40 characters; for a session's agent only"
                    ),
                }),
                &["project", "agent"],
            ),
        ),
        (
            "list_workflows",
            "The workflows in the library, by id and name",
            none(),
        ),
        (
            "read_workflow",
            "A workflow's file, as TOML: what save_workflow takes back once changed",
            schema(
                json!({"workflow": text("A workflow's id, as list_workflows gives it")}),
                &["workflow"],
            ),
        ),
        (
            "save_workflow",
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
            "The tasks of the window's projects, with their state and what can be done next",
            schema(json!({"project": text(project)}), &[]),
        ),
        (
            "stop_task",
            "Stop a task, as the Tasks page's Stop does",
            schema(
                json!({"task": text("A task's id, as list_tasks gives it")}),
                &["task"],
            ),
        ),
        (
            "open_file",
            "Open a file of the project on screen in the Workbench, without taking focus",
            schema(json!({"path": text("The file's absolute path")}), &["path"]),
        ),
        (
            "list_issues",
            "The project's own open issues, as its Issues tab keeps them. Read them before \
             writing one, so a problem already written is not written twice",
            schema(
                json!({"project": text("A project of the window; this session's when left out")}),
                &[],
            ),
        ),
        (
            "create_issue",
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
            "What a worker you opened is doing, one fact per line: its link, its turn and how \
             the last ended, what waits on a person, its activity, who holds it, your turns sent \
             and over, what it runs with. It never says the worker stopped: only `link: closed` \
             does",
            schema(
                json!({"session": text("The worker's handle, as open_session answered it")}),
                &["session"],
            ),
        ),
        (
            "send_prompt",
            "Give a worker you opened its next turn. Never queued: refused while a turn runs, \
             while it connects or once its link is lost, and while a person holds it. Each turn \
             number goes once: the same turn and text again answers that it was already sent, \
             so a lost answer can be retried; the same turn with other text is refused",
            schema(
                json!({
                    "session": text("The worker's handle, as open_session answered it"),
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
            "Cancel the turn a worker you opened is running, as the person's Stop does. \
             Refused while a permission or a question waits on a person, which only they \
             answer, and while a person holds the worker",
            schema(
                json!({"session": text("The worker's handle, as open_session answered it")}),
                &["session"],
            ),
        ),
    ];
    tools
        .into_iter()
        .map(|(name, description, input)| {
            json!({"name": name, "description": description, "inputSchema": input})
        })
        .collect()
}

/// Read tool `name`'s `arguments` into a call, or say which argument is wrong.
fn parse(name: &str, arguments: &Value) -> Result<Call, String> {
    let empty = Map::new();
    let args = match arguments {
        Value::Null => &empty,
        Value::Object(args) => args,
        _ => return Err("the arguments must be an object".to_string()),
    };
    let known: &[&str] = match name {
        "list_tasks" => &["project"],
        "stop_task" => &["task"],
        "open_file" => &["path"],
        "list_issues" => &["project"],
        "create_issue" => &["project", "title", "body"],
        "open_session" => &[
            "project", "agent", "prompt", "dirs", "request", "turn", "model", "name",
        ],
        "read_workflow" => &["workflow"],
        "save_workflow" => &["toml"],
        "start_workflow" => &["project", "workflow", "title", "brief"],
        "start_issue" => &["project", "issue", "workflow"],
        "session_state" => &["session"],
        "send_prompt" => &["session", "text", "turn"],
        "cancel_turn" => &["session"],
        _ => &[],
    };
    if let Some(extra) = args.keys().find(|k| !known.contains(&k.as_str())) {
        return Err(format!("`{name}` takes no argument `{extra}`"));
    }
    let optional = |key: &str| -> Result<Option<String>, String> {
        match args.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) if !s.trim().is_empty() => Ok(Some(s.clone())),
            Some(_) => Err(format!("`{key}` must be a string that is not empty")),
        }
    };
    let required = |key: &str| optional(key)?.ok_or_else(|| format!("`{key}` is required"));
    Ok(match name {
        "list_projects" => Call::ListProjects,
        "list_agents" => Call::ListAgents,
        "list_workflows" => Call::ListWorkflows,
        "read_workflow" => Call::ReadWorkflow {
            workflow: required("workflow")?,
        },
        "save_workflow" => Call::SaveWorkflow {
            text: required("toml")?,
        },
        "start_workflow" => Call::StartWorkflow {
            project: required("project")?,
            workflow: required("workflow")?,
            title: required("title")?,
            brief: optional("brief")?,
        },
        "start_issue" => Call::StartIssue {
            project: required("project")?,
            issue: args
                .get("issue")
                .and_then(Value::as_u64)
                .filter(|&n| n > 0)
                .ok_or("`issue` must be an issue's number, a whole number from 1")?,
            workflow: optional("workflow")?,
        },
        "list_tasks" => Call::ListTasks {
            project: optional("project")?,
        },
        "stop_task" => Call::StopTask {
            task: required("task")?,
        },
        "open_file" => Call::OpenFile {
            path: PathBuf::from(required("path")?),
        },
        "list_issues" => Call::ListIssues {
            project: optional("project")?,
        },
        "create_issue" => Call::CreateIssue {
            project: optional("project")?,
            title: required("title")?,
            body: optional("body")?.unwrap_or_default(),
        },
        "open_session" => Call::OpenSession {
            project: required("project")?,
            agent: required("agent")?,
            prompt: optional("prompt")?,
            dirs: folders(args.get("dirs"))?,
            request: optional("request")?,
            turn: match args.get("turn") {
                None | Some(Value::Null) => None,
                Some(turn) => Some(
                    turn.as_u64()
                        .filter(|&n| n > 0)
                        .ok_or("`turn` must be a turn number, a whole number from 1")?,
                ),
            },
            model: optional("model")?,
            name: optional("name")?
                .map(|name| name.split_whitespace().collect::<Vec<_>>().join(" "))
                .map(|name| match name.chars().count() {
                    ..=NAME_MAX => Ok(name),
                    _ => Err(format!("`name` must be at most {NAME_MAX} characters")),
                })
                .transpose()?,
        },
        "session_state" => Call::SessionState {
            session: required("session")?,
        },
        "send_prompt" => Call::SendPrompt {
            session: required("session")?,
            text: required("text")?,
            turn: args
                .get("turn")
                .and_then(Value::as_u64)
                .filter(|&n| n > 0)
                .ok_or("`turn` must be a turn number, a whole number from 1")?,
        },
        "cancel_turn" => Call::CancelTurn {
            session: required("session")?,
        },
        _ => return Err(format!("no tool named `{name}`")),
    })
}

/// The longest name a session may be given, so a rail row still shows it.
const NAME_MAX: usize = 40;

/// A list of folder paths, as `dirs` takes it: absent is none.
fn folders(value: Option<&Value>) -> Result<Vec<PathBuf>, String> {
    let refused = || "`dirs` must be a list of folder paths".to_string();
    match value {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::String(path) if !path.trim().is_empty() => Ok(PathBuf::from(path)),
                _ => Err(refused()),
            })
            .collect(),
        Some(_) => Err(refused()),
    }
}

/// The project of `projects` (name, path) that `named` names: by its path,
/// or by its name when only one project has it.
pub fn pick_project<'a>(
    projects: &'a [(String, PathBuf)],
    named: &str,
) -> Result<&'a Path, String> {
    let as_path = Path::new(named.trim_end_matches('/'));
    if let Some((_, path)) = projects.iter().find(|(_, path)| path == as_path) {
        return Ok(path);
    }
    let alike: Vec<&Path> = projects
        .iter()
        .filter(|(name, _)| name == named)
        .map(|(_, path)| path.as_path())
        .collect();
    match alike.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("no project named {named} in this window")),
        many => {
            let paths: Vec<String> = many.iter().map(|p| p.display().to_string()).collect();
            Err(format!(
                "{} projects are named {named}: {}; name one by its path",
                many.len(),
                paths.join(", ")
            ))
        }
    }
}

/// `line` as the proxy sends it on: an `initialize` carries the session's
/// token, whatever the client put there; every other line goes as it is.
pub fn with_token(line: &str, token: &str) -> String {
    let Ok(mut msg) = serde_json::from_str::<Value>(line) else {
        return line.to_string();
    };
    if msg["method"] != "initialize" {
        return line.to_string();
    }
    if !msg["params"].is_object() {
        msg["params"] = json!({});
    }
    if !msg["params"]["_meta"].is_object() {
        msg["params"]["_meta"] = json!({});
    }
    msg["params"]["_meta"]["onehand"] = json!({"session": token});
    msg.to_string()
}

/// The `mcpServers` entry a session is given: onehand at `command`, as the
/// proxy to the socket at `socket`, speaking for the session `token` names.
/// The stdio kind, which every ACP agent takes.
pub fn proxy_entry(command: &Path, socket: &Path, token: &str) -> Value {
    json!({
        "name": "onehand",
        "command": command.to_string_lossy(),
        "args": ["--mcp-proxy", socket.to_string_lossy(), token],
        "env": [],
    })
}

/// A token nobody can guess, for one session.
///
/// Two of std's randomly seeded hashers, rather than a crate for it: the
/// token keeps one agent from speaking for another session, and the socket
/// is the user's own already.
pub fn new_token() -> String {
    use std::hash::{BuildHasher as _, Hasher as _};
    let half = || {
        std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish()
    };
    format!("{:016x}{:016x}", half(), half())
}

/// What a connection is sent when its session's switch changes, so the
/// agent asks for the list again.
pub fn tools_changed() -> String {
    json!({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"}).to_string()
}

/// Carry an agent's MCP conversation between `input` and `output` (its
/// stdin and stdout) and the socket at `socket`, speaking for the session
/// `token` was given to. Returns when the socket closes. Blocking.
pub fn proxy(
    socket: &Path,
    token: &str,
    input: impl std::io::BufRead + Send + 'static,
    mut output: impl std::io::Write,
) -> std::io::Result<()> {
    use std::io::Write as _;
    let stream = std::os::unix::net::UnixStream::connect(socket)?;
    let mut to_socket = stream.try_clone()?;
    let token = token.to_string();
    // Its own thread, so the socket's end closing ends the proxy even while
    // the agent has nothing to send.
    std::thread::spawn(move || {
        for line in input.lines() {
            let Ok(line) = line else { break };
            if writeln!(to_socket, "{}", with_token(&line, &token)).is_err() {
                break;
            }
        }
        let _ = to_socket.shutdown(std::net::Shutdown::Write);
    });
    let mut from_socket = std::io::BufReader::new(stream);
    let mut line = String::new();
    while std::io::BufRead::read_line(&mut from_socket, &mut line)? > 0 {
        output.write_all(line.as_bytes())?;
        output.flush()?;
        line.clear();
    }
    Ok(())
}

/// How many lines a list answers with.
pub const LISTED: usize = 50;

/// `lines` as one answer, cut at [`LISTED`] and saying how many were left
/// out; `empty` when there are none.
pub fn listed(lines: Vec<String>, empty: &str) -> String {
    if lines.is_empty() {
        return empty.to_string();
    }
    let left_out = lines.len().saturating_sub(LISTED);
    let mut said: Vec<String> = lines.into_iter().take(LISTED).collect();
    if left_out > 0 {
        said.push(format!("… and {left_out} more not listed"));
    }
    said.join("\n")
}

#[cfg(test)]
mod tests;
