//! The socket's `onehand/route` call: an agent's command, handed by its
//! shim to the running onehand, decided and built on the project's build
//! host there, its lines streamed back as they come.

use std::cell::RefCell;
use std::io::{BufRead as _, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use serde_json::{json, Value};

use super::drive::route_drive;
use super::{decide, Ask, Decision, Machine};
use crate::config::HostOs;
use crate::host::build::cli::{project_of_blocking, told};
use crate::host::build::{build_cancellable, reachable_blocking, Job};

const METHOD: &str = "onehand/route";
const LINE: &str = "onehand/line";

/// The line a shim sends: run `program` with `args` from `cwd`, made
/// absolute because the process reading it has a working folder of its own.
pub(crate) fn route_request(program: &str, args: &[String], cwd: &Path) -> String {
    let cwd = std::path::absolute(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let params = json!({"program": program, "args": args, "cwd": cwd.to_string_lossy()});
    json!({"jsonrpc": "2.0", "id": 1, "method": METHOD, "params": params}).to_string()
}

/// The command a line asks to route; `None` for any other line.
pub fn route_ask(line: &str) -> Option<Ask> {
    let msg = serde_json::from_str::<Value>(line).ok()?;
    if msg["method"] != METHOD {
        return None;
    }
    let params = &msg["params"];
    let args = params["args"]
        .as_array()
        .map(|args| {
            args.iter()
                .filter_map(|a| a.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Some(Ask {
        program: params["program"].as_str().unwrap_or_default().to_string(),
        args,
        cwd: PathBuf::from(params["cwd"].as_str().unwrap_or_default()),
    })
}

/// What a routed run says while it runs.
#[derive(Debug, PartialEq, Eq)]
pub enum Said {
    /// A line the command printed.
    Out(String),
    /// What onehand says of its own.
    Note(String),
    /// A host's standing changed: the front end's to hear, nothing the
    /// shim is told.
    Reached,
}

/// The line `said` is for the shim; none for what is not its.
pub fn said_line(said: &Said) -> Option<String> {
    let params = match said {
        Said::Out(line) => json!({"out": line}),
        Said::Note(note) => json!({"note": note}),
        Said::Reached => return None,
    };
    Some(json!({"jsonrpc": "2.0", "method": LINE, "params": params}).to_string())
}

/// How a routed run ended.
#[derive(Debug, PartialEq, Eq)]
pub enum RouteEnd {
    /// The shim runs the command itself; `fell_back` when it was meant for
    /// a host that did not answer.
    Here { fell_back: bool },
    /// It ran on the host and came to this exit code.
    Exit(i32),
}

pub fn end_line(end: &RouteEnd) -> String {
    let result = match end {
        RouteEnd::Here { fell_back } => json!({"run": "here", "fell_back": fell_back}),
        RouteEnd::Exit(code) => json!({"exit": code}),
    };
    json!({"jsonrpc": "2.0", "id": 1, "result": result}).to_string()
}

/// Decide where `ask` runs and, for the build host, build it there, handing
/// `emit` what it prints and what onehand says, until it ends or `cancel`
/// is set.
pub fn serve_blocking(ask: &Ask, emit: &mut dyn FnMut(Said), cancel: &AtomicBool) -> RouteEnd {
    let here = RouteEnd::Here { fell_back: false };
    if !ask.cwd.is_absolute() {
        return here;
    }
    let found = project_of_blocking(&crate::config::config_dir(), &ask.cwd);
    let declared = crate::config::AppConfig::load_resolved().0.hosts;
    let hosts_on = route_drive().hosts_on();
    let routed = match decide(found, hosts_on, &Machine::this(&declared), ask) {
        Decision::Here => return here,
        Decision::Host(routed) => routed,
    };
    let host = &routed.host;
    emit(Said::Note(format!("running on {host}")));
    // A host that just did not answer is not asked again yet. Only Linux
    // hosts are routed to.
    let answered = !route_drive().recently_down(host, Instant::now()) && {
        let answered = reachable_blocking(host);
        if route_drive().reached(host, answered) {
            emit(Said::Reached);
        }
        answered
    };
    if !answered {
        emit(Said::Note(format!("{host} did not answer; building here")));
        return RouteEnd::Here { fell_back: true };
    }
    let job = Job {
        top: &routed.top,
        within: &routed.within,
        entry: &routed.entry,
        os: HostOs::Linux,
        command: &routed.command,
        fresh_only: true,
        wait: true,
    };
    // The command's lines and onehand's notes both go to the one `emit`.
    let emit = RefCell::new(emit);
    let note = |note: &str| (emit.borrow_mut())(Said::Note(note.to_string()));
    let built = build_cancellable(
        &job,
        &mut |line| (emit.borrow_mut())(Said::Out(line.to_string())),
        &mut |n| note(n),
        cancel,
    );
    match built {
        Ok(built) => RouteEnd::Exit(told(built, &mut |n| note(n))),
        Err(why) => {
            note(&why);
            RouteEnd::Exit(1)
        }
    }
}

/// Why a shim's call never reached its end.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Unreached {
    /// Connecting to the socket was refused permission: the agent's
    /// sandbox forbids it.
    Blocked,
    /// No onehand answered, or it went before the run ended, and why.
    Gone(String),
}

/// Hand `program` with `args`, from `cwd`, to the onehand on `socket`,
/// printing the command's lines on `out` and onehand's notes on `err` as
/// they come, and answer how it ended.
pub(crate) fn client_blocking(
    socket: &Path,
    program: &str,
    args: &[String],
    cwd: &Path,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<RouteEnd, Unreached> {
    let gone = |e: std::io::Error| Unreached::Gone(e.to_string());
    let mut stream = std::os::unix::net::UnixStream::connect(socket).map_err(|e| {
        match e.kind() {
            // EPERM and EACCES both.
            std::io::ErrorKind::PermissionDenied => Unreached::Blocked,
            _ => gone(e),
        }
    })?;
    writeln!(stream, "{}", route_request(program, args, cwd)).map_err(gone)?;
    for line in std::io::BufReader::new(&stream).lines() {
        let msg: Value = match serde_json::from_str(&line.map_err(gone)?) {
            Ok(msg) => msg,
            Err(_) => continue,
        };
        if let Some(result) = msg.get("result") {
            if let Some(code) = result["exit"].as_i64() {
                return Ok(RouteEnd::Exit(code as i32));
            }
            let fell_back = result["fell_back"].as_bool().unwrap_or(false);
            return Ok(RouteEnd::Here { fell_back });
        }
        // An answer with no result is an error: a onehand that does not
        // know the call, which would otherwise be waited on for ever.
        if msg.get("id").is_some() {
            let why = msg["error"]["message"].as_str().map(str::to_string);
            return Err(Unreached::Gone(why.unwrap_or_else(|| msg.to_string())));
        }
        let params = &msg["params"];
        if let Some(text) = params["out"].as_str() {
            let _ = writeln!(out, "{text}");
            let _ = out.flush();
        } else if let Some(note) = params["note"].as_str() {
            let _ = writeln!(err, "onehand: {note}");
            let _ = err.flush();
        }
    }
    Err(Unreached::Gone("onehand went before the run ended".into()))
}

#[cfg(test)]
mod tests;
