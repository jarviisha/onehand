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

use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// What a tool call asks the app to do, its arguments read and checked.
#[derive(Debug, PartialEq)]
pub enum Call {
    ListProjects,
    ListAgents,
    ListWorkflows,
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
    OpenSession {
        project: String,
        agent: String,
        prompt: Option<String>,
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
    Perform { id: Value, call: Call },
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
            let on = matches!(standing, Standing::On);
            if method == "tools/list" {
                return reply(json!({"tools": if on { catalogue() } else { json!([]) }}));
            }
            let name = params["name"].as_str().unwrap_or("");
            if !TOOLS.contains(&name) {
                return fail(-32602, &format!("no tool named `{name}`"));
            }
            if !on {
                let why = "onehand's tools are switched off for this session";
                return Handled::Reply(answered(&id, Err(why.to_string())));
            }
            match parse(name, &params["arguments"]) {
                Ok(call) => Handled::Perform { id, call },
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
const TOOLS: [&str; 9] = [
    "list_projects",
    "list_agents",
    "open_session",
    "list_workflows",
    "start_workflow",
    "start_issue",
    "list_tasks",
    "stop_task",
    "open_file",
];

/// Each tool's description and input schema, as `tools/list` gives them.
fn catalogue() -> Value {
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
             first prompt if one is given. It appears in the rail",
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
            "list_workflows",
            "The workflows in the library, by id and name",
            none(),
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
        "open_session" => &["project", "agent", "prompt"],
        "start_workflow" => &["project", "workflow", "title", "brief"],
        "start_issue" => &["project", "issue", "workflow"],
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
        "open_session" => Call::OpenSession {
            project: required("project")?,
            agent: required("agent")?,
            prompt: optional("prompt")?,
        },
        _ => return Err(format!("no tool named `{name}`")),
    })
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
