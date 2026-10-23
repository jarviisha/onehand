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
#[derive(Debug)]
pub enum Peer {
    /// A program outside onehand: a script, the system's cron.
    Outside,
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
    let Some(id) = msg.get("id").cloned() else {
        return Handled::Quiet;
    };
    let params = &msg["params"];
    let method = msg["method"].as_str().unwrap_or("");
    let reply = |result: Value| Handled::Reply(ok(&id, result));
    let fail = |code: i64, why: &str| Handled::Reply(error(&id, code, why));
    match method {
        "initialize" => {
            if !params["_meta"]["onehand"]["session"].is_null() {
                return fail(NOT_ALLOWED, "no such session");
            }
            let peer = Peer::Outside;
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
const TOOLS: [&str; 7] = [
    "list_projects",
    "list_agents",
    "open_session",
    "list_workflows",
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
