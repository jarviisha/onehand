//! The socket's `onehand/route` call: an agent's command, handed by its
//! shim to the running onehand, decided and built on the project's build
//! host there, its lines streamed back as they come.

use std::cell::RefCell;
use std::io::{BufRead as _, Read as _, Write};
use std::ops::DerefMut;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use serde_json::{json, Value};

use super::drive::{route_drive, RouteDrive};
use super::{decide, Ask, Decision, Machine};
use crate::config::HostOs;
use crate::host::build::cli::{project_of_blocking, told};
use crate::host::build::{build_cancellable, reachable_blocking, AskedBy, Job, Told};
use crate::workspace::HostName;

const METHOD: &str = "onehand/route";
const LINE: &str = "onehand/line";

/// The longest line either side of a routed run sends: past it the reader
/// drops the run rather than hold the line in memory.
pub const LINE_CAP: usize = 64 * 1024;

/// The most of a command's line one `out` line holds, as JSON writes it
/// (a control character such as a colour's escape takes six bytes), leaving
/// the rest of [`LINE_CAP`] for the JSON around it.
const OUT_CHUNK: usize = LINE_CAP - 256;

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
    /// The command is starting on this host: from here on, a run cut off
    /// is the build failing, not one to run here again.
    Started(HostName),
    /// A host's standing changed: the front end's to hear, nothing the
    /// shim is told.
    Reached,
}

/// The lines `said` is for the shim, a long one cut into several of at
/// most [`OUT_CHUNK`] bytes each; none for what is not the shim's.
pub fn said_lines(said: &Said) -> Vec<String> {
    let (key, text) = match said {
        Said::Out(line) => ("out", line),
        Said::Note(note) => ("note", note),
        Said::Started(host) => {
            let params = json!({"note": format!("running on {host}"), "started": host.0});
            return vec![json!({"jsonrpc": "2.0", "method": LINE, "params": params}).to_string()];
        }
        Said::Reached => return Vec::new(),
    };
    chunks(text)
        .map(|chunk| json!({"jsonrpc": "2.0", "method": LINE, "params": {key: chunk}}).to_string())
        .collect()
}

/// `text` in pieces each at most [`OUT_CHUNK`] bytes once written as a JSON
/// string, cut between characters; an empty one is one empty piece.
fn chunks(mut text: &str) -> impl Iterator<Item = &str> {
    let mut first = true;
    std::iter::from_fn(move || {
        if text.is_empty() && !std::mem::take(&mut first) {
            return None;
        }
        first = false;
        let (mut at, mut written) = (0, 0);
        for c in text.chars() {
            written += escaped_len(c);
            if written > OUT_CHUNK {
                break;
            }
            at += c.len_utf8();
        }
        let (piece, rest) = text.split_at(at);
        text = rest;
        Some(piece)
    })
}

/// How many bytes `c` takes inside a JSON string, as serde_json writes it.
fn escaped_len(c: char) -> usize {
    match c {
        '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
        c if (c as u32) < 0x20 => 6,
        c => c.len_utf8(),
    }
}

/// How a routed run ended.
#[derive(Debug, PartialEq, Eq)]
pub enum RouteEnd {
    /// The shim runs the command itself; `settled` when a command it starts
    /// in turn runs here too without asking: builds on hosts are off, or the
    /// host it was meant for did not answer.
    Here { settled: bool },
    /// It ran on the host and came to this exit code.
    Exit(i32),
}

pub fn end_line(end: &RouteEnd) -> String {
    let result = match end {
        RouteEnd::Here { settled } => json!({"run": "here", "settled": settled}),
        RouteEnd::Exit(code) => json!({"exit": code}),
    };
    json!({"jsonrpc": "2.0", "id": 1, "result": result}).to_string()
}

/// Decide where `ask` runs and, for the build host, build it there, handing
/// `emit` what it prints and what onehand says, until it ends or `cancel`
/// is set.
pub fn serve_blocking(ask: &Ask, emit: &mut dyn FnMut(Said), cancel: &AtomicBool) -> RouteEnd {
    if !ask.cwd.is_absolute() {
        return RouteEnd::Here { settled: false };
    }
    let found = project_of_blocking(&crate::config::config_dir(), &ask.cwd);
    let declared = crate::config::AppConfig::load_resolved().0.hosts;
    let hosts_on = route_drive().hosts_on();
    let routed = match decide(found, hosts_on, &Machine::this(&declared), ask) {
        Decision::Here { settled } => return RouteEnd::Here { settled },
        Decision::Host(routed) => routed,
    };
    if !take(&routed.host, route_drive, reachable_blocking, emit) {
        return RouteEnd::Here { settled: true };
    }
    let job = Job {
        top: &routed.top,
        within: &routed.within,
        entry: &routed.entry,
        os: HostOs::Linux,
        command: &routed.command,
        asked_by: AskedBy::Route,
    };
    // The command's lines and onehand's notes both go to the one `emit`.
    let emit = RefCell::new(emit);
    let note = |note: &str| (emit.borrow_mut())(Said::Note(note.to_string()));
    let built = build_cancellable(
        &job,
        &mut |line| (emit.borrow_mut())(Said::Out(line.to_string())),
        &mut |told| match told {
            Told::Note(n) => note(n),
            // Said only now: until the command runs, a run cut off has
            // built nothing, and may fall back.
            Told::Running => (emit.borrow_mut())(Said::Started(routed.host.clone())),
        },
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

/// Whether `host` takes the run, said when it does not: a host that did
/// not answer moments ago is not asked again yet, and one asked
/// is `probe`d before any snapshot. `drive` is held only between asks, never
/// across the probe's wait.
fn take<G: DerefMut<Target = RouteDrive>>(
    host: &HostName,
    drive: impl Fn() -> G,
    probe: impl FnOnce(&HostName) -> bool,
    emit: &mut dyn FnMut(Said),
) -> bool {
    let answered = !drive().recently_down(host, Instant::now()) && {
        let answered = probe(host);
        if drive().reached(host, answered) {
            emit(Said::Reached);
        }
        answered
    };
    if !answered {
        emit(Said::Note(format!("{host} did not answer; building here")));
    }
    answered
}

/// Why a shim's call never reached its end.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Unreached {
    /// Connecting to the socket was refused permission: the agent's
    /// sandbox forbids it.
    Blocked,
    /// No onehand answered, or it went before the command started, and
    /// why: the shim runs it here.
    Gone(String),
    /// The command had started on the host when the run was cut off, in
    /// words: the shim fails, never running it here a second time.
    Cut(String),
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
    let mut reader = std::io::BufReader::new(&stream);
    let mut line = Vec::new();
    // The host the command started on, once it has: past that, what cuts
    // the run off is no fall-back.
    let mut started: Option<String> = None;
    let went = |started: &Option<String>, why: String| match started {
        Some(host) => Unreached::Cut(format!("onehand went away during the build on {host}")),
        None => Unreached::Gone(why),
    };
    loop {
        line.clear();
        let read = (&mut reader)
            .take(LINE_CAP as u64)
            .read_until(b'\n', &mut line)
            .map_err(|e| went(&started, e.to_string()))?;
        if read == 0 {
            break;
        }
        if line.last() != Some(&b'\n') && line.len() >= LINE_CAP {
            return Err(match started {
                Some(_) => Unreached::Cut("a reply line was too long".into()),
                None => Unreached::Gone(format!(
                    "a line from onehand passed {} KiB",
                    LINE_CAP / 1024
                )),
            });
        }
        let msg: Value = match serde_json::from_slice(&line) {
            Ok(msg) => msg,
            Err(_) => continue,
        };
        if let Some(result) = msg.get("result") {
            if let Some(code) = result["exit"].as_i64() {
                return Ok(RouteEnd::Exit(code as i32));
            }
            let settled = result["settled"].as_bool().unwrap_or(false);
            return Ok(RouteEnd::Here { settled });
        }
        // An answer with no result is an error: a onehand that does not
        // know the call, which would otherwise be waited on for ever.
        if msg.get("id").is_some() {
            let why = msg["error"]["message"].as_str().map(str::to_string);
            return Err(went(&started, why.unwrap_or_else(|| msg.to_string())));
        }
        let params = &msg["params"];
        if let Some(host) = params["started"].as_str() {
            started = Some(host.to_string());
        }
        if let Some(text) = params["out"].as_str() {
            // Only a command that started prints; its host is named first.
            started.get_or_insert_with(|| "its host".into());
            let _ = writeln!(out, "{text}");
            let _ = out.flush();
        } else if let Some(note) = params["note"].as_str() {
            let _ = writeln!(err, "onehand: {note}");
            let _ = err.flush();
        }
    }
    Err(went(&started, "onehand went before the run ended".into()))
}

#[cfg(test)]
mod tests;
