//! The ACP client: spawns the adapter subprocess and runs a newline-delimited
//! JSON-RPC loop over its stdio, exposed as a plain
//! `futures` `Stream` of [`AcpEvent`]s via [`connect`]. The stream type is
//! GUI-neutral on purpose: nothing in the type names a UI framework or a
//! runtime, so a front end drives it however it likes -- the GPUI shell polls
//! it from `cx.spawn` on a tokio runtime it owns.
//!
//! Everything runs in **one** async task (no per-frame pump): a `tokio::select!`
//! loop multiplexes (a) the adapter's stdout, (b) UI requests, and (c) the news
//! that the adapter has ended. Because reverse requests (permission, fs) arrive
//! as ordinary stdout lines, they are served *during* a prompt turn without
//! extra plumbing — the turn's `session/prompt` response is just another line.
//! Liveness: the loop races that ending against a handshake deadline, so a
//! dead or stuck adapter surfaces as [`AcpEvent::Disconnected`] instead of
//! hanging.
//!
//! What the loop talks to is a [`Transport`] — two pipes and the news of an
//! ending — rather than a child process. A child is the only thing that
//! satisfies it in production; the split is what lets the loop itself be driven
//! by a test, which is where the handshake deadline, an adapter that goes away,
//! the parked permission round trip and the declined form are checked.
//!
//! Two things it still does not reach. The `biased` ordering — terminal output
//! must never starve the adapter's stdout — would need a race staged on
//! purpose, and a test that stages a race is a test that reports one at random.
//! And the readiness gate cannot be tripped from outside: the request channel is
//! handed to the caller by [`AcpEvent::Connected`], which is sent from the same
//! few statements that set the flag, so reaching the window means winning a race
//! rather than asking for it.

use super::parse;
use super::terminal::{self, TermStream, TerminalRegistry};
use super::types::{AcpEvent, AcpRequest};
use futures::channel::mpsc::Sender as EventTx;
use futures::stream::{self, Stream, StreamExt};
use futures::SinkExt;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio::sync::mpsc;

mod forms;
mod transport;
mod wire;

use forms::*;
use transport::*;
pub use wire::base64_encode;
use wire::*;

/// ACP protocol version this client speaks.
const PROTOCOL_VERSION: i64 = 1;
/// How many events may sit unread before the serve loop back-pressures.
const EVENT_BUFFER: usize = 64;
/// Give the handshake (initialize → session/new) this long before declaring the
/// adapter stuck.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(90);

/// Connect to an agent adapter, with `env` set on it over what it inherits,
/// and stream its events. The returned stream owns
/// the child process; dropping it (subscription removed) kills the adapter
/// (`kill_on_drop`).
pub fn connect(
    command: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    cwd: PathBuf,
    resume: Option<String>,
) -> impl Stream<Item = AcpEvent> {
    // Spawning is deferred into the stream rather than done here, so a command
    // that does not exist surfaces as `Disconnected` on the stream the caller is
    // already reading instead of as a second failure mode at the call site.
    let spawned = Transport::spawn(command, args, env, cwd.clone());
    connect_over_result(spawned, cwd, resume)
}

/// Serve an already-connected transport, as a stream of its events.
///
/// The seam a test drives the loop through. In production a transport is always
/// a child process, so [`connect`] opens one and there is nothing else to hand
/// this.
#[cfg(test)]
fn connect_over(
    transport: Transport,
    cwd: PathBuf,
    resume: Option<String>,
) -> impl Stream<Item = AcpEvent> {
    connect_over_result(Ok(transport), cwd, resume)
}

/// The body of [`connect`], for a transport that may have failed to open.
fn connect_over_result(
    transport: Result<Transport, String>,
    cwd: PathBuf,
    resume: Option<String>,
) -> impl Stream<Item = AcpEvent> {
    let (sender, receiver) = futures::channel::mpsc::channel(EVENT_BUFFER);

    // The serve loop is folded into the stream itself rather than spawned, so
    // the adapter only advances while something is polling us — and dropping
    // the stream drops the child. `runner` never yields an item; it exists to
    // be driven. Combining a driver stream with the event stream this way is
    // what keeps `connect` a plain `Stream` instead of a handle pair the caller
    // has to remember to pump.
    let runner = stream::once(async move {
        let mut output = sender;
        let served = match transport {
            Ok(mut transport) => {
                // Both taken before the transport moves into the loop, so they
                // are still reachable once that loop has given up on it.
                let stderr = transport.stderr.clone();
                let drain = transport.drain.take();
                match run(transport, &cwd, resume, &mut output).await {
                    Ok(()) => Ok(()),
                    Err(why) => {
                        if let Some(drain) = drain {
                            let _ = tokio::time::timeout(STDERR_GRACE, drain).await;
                        }
                        Err(explain(why, &stderr))
                    }
                }
            }
            // A spawn that never started has no stderr to have written to:
            // the error already names the command and what the OS said.
            Err(why) => Err(why),
        };
        if let Err(err) = served {
            let _ = output.send(AcpEvent::Disconnected(err)).await;
        }
    })
    .filter_map(|()| async { None });

    stream::select(receiver, runner)
}

/// Which outgoing request a response id corresponds to.
enum Pending {
    Init,
    NewSession,
    LoadSession,
    PromptTurn,
    /// A mode or a config option asked for: its group and the value.
    Pick {
        group: String,
        value: String,
    },
}

async fn run(
    transport: Transport,
    cwd: &PathBuf,
    resume: Option<String>,
    output: &mut EventTx<AcpEvent>,
) -> Result<(), String> {
    let Transport {
        incoming,
        mut outgoing,
        ended,
        // Both read by the caller, which is where a failure becomes a sentence.
        stderr: _,
        drain: _,
    } = transport;
    let stdin = &mut outgoing;
    let mut reader = incoming.lines();
    tokio::pin!(ended);
    let (req_tx, mut req_rx) = mpsc::unbounded_channel::<AcpRequest>();

    let mut next_id: u64 = 1;
    let mut pending: HashMap<u64, Pending> = HashMap::new();
    let mut ready = false;
    let mut session_id: Option<String> = None;
    // Whether the agent said it reads image blocks; until it says so, it does not.
    let mut images = false;

    // The terminal extension: a registry of live PTYs, a channel its reader
    // threads stream output over, and a channel async waiters use to post their
    // delayed JSON-RPC responses back to this loop (the sole stdin writer).
    // Bounded: PTY readers `blocking_send` into it, so a command flooding
    // output parks the reader thread instead of growing this queue unboundedly
    // (the UI drains at frame rate). 256 × 4 KiB chunks ≈ 1 MiB in flight max.
    let (term_tx, mut term_rx) = mpsc::channel::<TermStream>(256);
    let (resp_tx, mut resp_rx) = mpsc::unbounded_channel::<(Value, Value)>();
    let mut registry = TerminalRegistry::new(term_tx);

    // Kick off the handshake.
    let init_id = alloc(&mut next_id);
    pending.insert(init_id, Pending::Init);
    write_msg(stdin, &request(init_id, "initialize", init_params())).await?;

    let deadline = tokio::time::sleep(HANDSHAKE_TIMEOUT);
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            biased;

            _ = &mut deadline, if !ready => {
                return Err("handshake timed out".into());
            }

            // A parked terminal/wait_for_exit settled → write its response.
            Some((rpc_id, result)) = resp_rx.recv() => {
                write_msg(stdin, &response_ok(rpc_id, result)).await?;
            }

            // Adapter stdout comes BEFORE the terminal stream: this select is
            // `biased`, so a command flooding terminal output must not starve
            // session/updates and permission requests (nor the adapter, which
            // blocks once its stdout pipe fills).
            line = reader.next_line() => {
                let line = match line {
                    Ok(Some(l)) => l,
                    Ok(None) => {
                        // Let it exit on its own before it is dropped. Returning
                        // drops `ended`, and with it the child, which is killed on
                        // drop — and a process that closes stdout first and says
                        // why afterwards is then killed before it has said it.
                        // Bounded, because a child that closed stdout and kept
                        // running would otherwise hold the failure open.
                        let _ = tokio::time::timeout(STDERR_GRACE, ended.as_mut()).await;
                        return Err("adapter closed stdout".into());
                    }
                    Err(e) => return Err(format!("read error: {e}")),
                };
                if line.trim().is_empty() {
                    continue;
                }
                let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                    continue; // ignore non-JSON noise
                };
                handle_message(
                    msg, stdin, &mut pending, &mut next_id, cwd, resume.as_deref(),
                    &mut ready, &mut session_id, &mut images, &req_tx, output, &mut registry,
                    &resp_tx,
                )
                .await?;
            }

            // After stdout, so lines an adapter wrote before it exited are read
            // first: one that answers a turn with an error and then dies has
            // both ready at once, and the answer is the part worth saying.
            why = &mut ended => {
                return Err(why);
            }

            // Live terminal output → forward to the UI.
            Some(ev) = term_rx.recv() => {
                let out = match ev {
                    TermStream::Output { id, data } => AcpEvent::TerminalOutput {
                        terminal_id: id,
                        chunk: String::from_utf8_lossy(&data).into_owned(),
                    },
                    TermStream::Exit { id, code } => AcpEvent::TerminalExit {
                        terminal_id: id,
                        exit_code: code,
                    },
                };
                let _ = output.send(out).await;
            }

            req = req_rx.recv(), if ready => {
                match req {
                    Some(AcpRequest::Prompt { text, attachments }) => {
                        let id = alloc(&mut next_id);
                        pending.insert(id, Pending::PromptTurn);
                        let sid = session_id.clone().unwrap_or_default();
                        let blocks = build_prompt_blocks(&text, &attachments, images).await;
                        let params = json!({ "sessionId": sid, "prompt": blocks });
                        write_msg(stdin, &request(id, "session/prompt", params)).await?;
                    }
                    Some(AcpRequest::SetMode(mode_id)) => {
                        if let Some(sid) = &session_id {
                            let id = alloc(&mut next_id);
                            pending.insert(id, Pending::Pick {
                                group: crate::chat::MODE_SELECTOR.to_string(),
                                value: mode_id.clone(),
                            });
                            let params = json!({ "sessionId": sid, "modeId": mode_id });
                            write_msg(stdin, &request(id, "session/set_mode", params)).await?;
                        }
                    }
                    Some(AcpRequest::SetConfigOption { config_id, value }) => {
                        if let Some(sid) = &session_id {
                            let id = alloc(&mut next_id);
                            pending.insert(id, Pending::Pick {
                                group: config_id.clone(),
                                value: value.clone(),
                            });
                            let params =
                                json!({ "sessionId": sid, "configId": config_id, "value": value });
                            write_msg(stdin, &request(id, "session/set_config_option", params))
                                .await?;
                        }
                    }
                    Some(AcpRequest::Cancel) => {
                        if let Some(sid) = &session_id {
                            let params = json!({ "sessionId": sid });
                            write_msg(stdin, &notification("session/cancel", params)).await?;
                        }
                    }
                    Some(AcpRequest::PermissionResponse { rpc_id, option_id }) => {
                        // Answer the parked permission with the user's choice.
                        let outcome = match option_id {
                            Some(id) => json!({ "outcome": { "outcome": "selected", "optionId": id } }),
                            None => json!({ "outcome": { "outcome": "cancelled" } }),
                        };
                        write_msg(stdin, &response_ok(rpc_id, outcome)).await?;
                    }
                    Some(AcpRequest::ElicitationResponse { rpc_id, outcome }) => {
                        write_msg(stdin, &response_ok(rpc_id, elicit_result(outcome))).await?;
                    }
                    None => {} // all UI senders dropped; keep serving until child exits
                }
            }
        }
    }
}

/// Dispatch one incoming JSON-RPC message: a response to one of our requests, a
/// reverse request from the agent, or a notification.
#[allow(clippy::too_many_arguments)]
async fn handle_message(
    msg: Value,
    stdin: &mut Outgoing,
    pending: &mut HashMap<u64, Pending>,
    next_id: &mut u64,
    cwd: &PathBuf,
    resume: Option<&str>,
    ready: &mut bool,
    session_id: &mut Option<String>,
    images: &mut bool,
    req_tx: &super::types::ReqTx,
    output: &mut EventTx<AcpEvent>,
    registry: &mut TerminalRegistry,
    resp_tx: &mpsc::UnboundedSender<(Value, Value)>,
) -> Result<(), String> {
    // Reverse request (has both method and id) — answer it.
    if let (Some(method), Some(id)) = (msg.get("method").and_then(Value::as_str), msg.get("id")) {
        let params = msg.get("params").cloned().unwrap_or(Value::Null);

        // Permission is *parked*: surface it to the UI and answer later when the
        // user clicks (permission buttons). No response written now.
        if method == "session/request_permission" {
            let _ = output
                .send(AcpEvent::Permission(parse_permission(id.clone(), &params)))
                .await;
            return Ok(());
        }

        // A question (`AskUserQuestion`, an MCP form, the refusal-fallback
        // consent prompt) is parked the same way — the agent's tool call blocks
        // on our answer. A form we can't render (url mode, or a schema with no
        // usable properties) is *declined on the spot*: parking it would hang
        // the turn on a card the user can never fill in.
        if method == "elicitation/create" {
            match parse_elicitation(id.clone(), &params) {
                Some(e) => {
                    let _ = output.send(AcpEvent::Elicitation(e)).await;
                }
                None => {
                    write_msg(
                        stdin,
                        &response_ok(id.clone(), json!({ "action": "decline" })),
                    )
                    .await?
                }
            }
            return Ok(());
        }

        // Terminal extension methods.
        if let Some(rest) = method.strip_prefix("terminal/") {
            return handle_terminal(rest, id, &params, cwd, registry, stdin, resp_tx).await;
        }

        // JSON-RPC's own answer for a method nobody serves, so an agent probing
        // for an optional one reads an absence rather than a fault.
        if !matches!(method, "fs/read_text_file" | "fs/write_text_file") {
            write_msg(stdin, &method_not_found(id.clone(), method)).await?;
            return Ok(());
        }
        let response = match handle_reverse(method, &params, cwd).await {
            Ok(result) => response_ok(id.clone(), result),
            Err(message) => response_err(id.clone(), &message),
        };
        write_msg(stdin, &response).await?;
        return Ok(());
    }

    // Notification (method, no id).
    if let Some(method) = msg.get("method").and_then(Value::as_str) {
        if method == "session/update" {
            if let Some(update) = msg.get("params").and_then(|p| p.get("update")) {
                for u in parse::parse_session_update(update) {
                    let ev = match u {
                        parse::Update::Agent(s) => AcpEvent::AgentChunk(s),
                        parse::Update::Thought(s) => AcpEvent::ThoughtChunk(s),
                        parse::Update::User(s) => AcpEvent::UserChunk(s),
                        parse::Update::ToolCall(tc) => AcpEvent::ToolCall(tc),
                        parse::Update::ToolUpdate(tu) => AcpEvent::ToolUpdate(tu),
                        parse::Update::Plan(p) => AcpEvent::Plan(p),
                        parse::Update::Commands(c) => AcpEvent::AvailableCommands(c),
                        parse::Update::ModeChanged(m) => AcpEvent::ModeChanged(m),
                        parse::Update::ConfigOptions(c) => AcpEvent::ConfigOptions(c),
                    };
                    let _ = output.send(ev).await;
                }
            }
        }
        return Ok(());
    }

    // Response to one of our requests.
    if let Some(id) = msg.get("id").and_then(Value::as_u64) {
        match pending.remove(&id) {
            Some(Pending::Init) => {
                // Surface a failed handshake as what it is — otherwise an
                // error response would read as "no loadSession capability"
                // and the eventual failure would blame `session/new` instead
                // of the real protocol-version/handshake problem.
                if let Some(err) = msg.get("error") {
                    return Err(format!("initialize failed: {err}"));
                }
                let capabilities = msg.get("result").and_then(|r| r.get("agentCapabilities"));
                let load_supported = capabilities
                    .and_then(|c| c.get("loadSession"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                *images = capabilities
                    .and_then(|c| c.get("promptCapabilities"))
                    .and_then(|p| p.get("image"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let id2 = alloc(next_id);
                if let (true, Some(prev)) = (load_supported, resume) {
                    pending.insert(id2, Pending::LoadSession);
                    let params = json!({
                        "sessionId": prev, "cwd": cwd.display().to_string(), "mcpServers": [],
                    });
                    write_msg(stdin, &request(id2, "session/load", params)).await?;
                } else {
                    pending.insert(id2, Pending::NewSession);
                    let params = json!({ "cwd": cwd.display().to_string(), "mcpServers": [] });
                    write_msg(stdin, &request(id2, "session/new", params)).await?;
                }
            }
            Some(Pending::NewSession) => {
                if let Some(err) = msg.get("error") {
                    // The whole object, code and all, for whoever reads the log.
                    eprintln!("[acp] session/new failed: {err}");
                    if login_required(err) {
                        let _ = output.send(AcpEvent::LoginRequired).await;
                    }
                    return Err(format!("Could not start a session: {}", in_words(err).0));
                }
                let result = msg.get("result");
                // Every prompt names the session; one the agent did not name
                // cannot be prompted at all, so it is a start that failed.
                let Some(sid) = result
                    .and_then(|r| r.get("sessionId"))
                    .and_then(Value::as_str)
                    .filter(|sid| !sid.is_empty())
                    .map(String::from)
                else {
                    return Err("Could not start a session: the agent named none".into());
                };
                *session_id = Some(sid.clone());
                let _ = output.send(AcpEvent::SessionId(sid)).await;
                if let Some(modes) = result.and_then(|r| r.get("modes")) {
                    let _ = output.send(parse_modes(modes)).await;
                }
                let cfg = parse::parse_config_options(result.and_then(|r| r.get("configOptions")));
                if !cfg.is_empty() {
                    let _ = output.send(AcpEvent::ConfigOptions(cfg)).await;
                }
                let _ = output
                    .send(AcpEvent::Connected {
                        tx: req_tx.clone(),
                        resumed: false,
                    })
                    .await;
                *ready = true;
            }
            Some(Pending::LoadSession) => {
                if let Some(err) = msg.get("error") {
                    // A stale persisted session id (the adapter's store was
                    // wiped or expired) must not brick the session — failing
                    // here would refreeze the same id on every restart and
                    // refail forever. Note it and fall back to a fresh
                    // session; the loaded history stays as read-only context.
                    eprintln!("[acp] session/load failed: {err}");
                    let (message, detail) = in_words(err);
                    let _ = output
                        .send(AcpEvent::Error {
                            text: format!(
                                "Loading the session failed: {message}; starting a fresh one"
                            ),
                            detail,
                        })
                        .await;
                    let id2 = alloc(next_id);
                    pending.insert(id2, Pending::NewSession);
                    let params = json!({ "cwd": cwd.display().to_string(), "mcpServers": [] });
                    write_msg(stdin, &request(id2, "session/new", params)).await?;
                    return Ok(());
                }
                let result = msg.get("result");
                *session_id = resume.map(String::from);
                if let Some(s) = session_id.clone() {
                    let _ = output.send(AcpEvent::SessionId(s)).await;
                }
                // `session/load` returns the same session info as `session/new`
                // (available modes + config options). Emit them here too, else a
                // *resumed* session shows no mode (permission) / model selectors.
                if let Some(modes) = result.and_then(|r| r.get("modes")) {
                    let _ = output.send(parse_modes(modes)).await;
                }
                let cfg = parse::parse_config_options(result.and_then(|r| r.get("configOptions")));
                if !cfg.is_empty() {
                    let _ = output.send(AcpEvent::ConfigOptions(cfg)).await;
                }
                let _ = output
                    .send(AcpEvent::Connected {
                        tx: req_tx.clone(),
                        resumed: true,
                    })
                    .await;
                *ready = true;
            }
            // The agent's answer to a pick is its confirmation or its
            // refusal: what is shown follows it.
            Some(Pending::Pick { group, value }) => {
                // A model picked may change what else is offered, and an
                // adapter may say so only in this answer. It lands before
                // the confirmation, so whoever plans the next pick on it
                // plans against the offer as it stands now.
                let cfg = parse::parse_config_options(
                    msg.get("result").and_then(|r| r.get("configOptions")),
                );
                if !cfg.is_empty() {
                    let _ = output.send(AcpEvent::ConfigOptions(cfg)).await;
                }
                let event = match msg.get("error") {
                    Some(err) => AcpEvent::PickRefused {
                        group,
                        why: in_words(err).0,
                    },
                    None => AcpEvent::PickConfirmed { group, value },
                };
                let _ = output.send(event).await;
            }
            Some(Pending::PromptTurn) => {
                if let Some(err) = msg.get("error") {
                    // The whole object, code and all, for whoever reads the log.
                    eprintln!("[acp] the turn failed: {err}");
                    if login_required(err) {
                        let _ = output.send(AcpEvent::LoginRequired).await;
                    }
                    let (message, detail) = in_words(err);
                    let _ = output
                        .send(AcpEvent::Error {
                            text: format!("The turn failed: {message}"),
                            detail,
                        })
                        .await;
                }
                let stop = msg
                    .get("result")
                    .and_then(|r| r.get("stopReason"))
                    .and_then(Value::as_str)
                    .unwrap_or("end_turn")
                    .to_string();
                let _ = output.send(AcpEvent::TurnEnded { stop_reason: stop }).await;
            }
            None => {}
        }
    }
    Ok(())
}

/// Answer an agent reverse request (the filesystem methods). Permission is
/// handled separately (parked), so it never reaches here.
async fn handle_reverse(method: &str, params: &Value, _cwd: &PathBuf) -> Result<Value, String> {
    match method {
        "fs/read_text_file" => {
            let path = params
                .get("path")
                .and_then(Value::as_str)
                .ok_or("missing path")?;
            let content = tokio::fs::read_to_string(path)
                .await
                .map_err(|e| e.to_string())?;
            let line = params.get("line").and_then(Value::as_u64);
            let limit = params.get("limit").and_then(Value::as_u64);
            Ok(json!({ "content": lines_of(&content, line, limit) }))
        }
        "fs/write_text_file" => {
            let path = params
                .get("path")
                .and_then(Value::as_str)
                .ok_or("missing path")?;
            let content = params
                .get("content")
                .and_then(Value::as_str)
                .ok_or("missing content")?;
            tokio::fs::write(path, content)
                .await
                .map(|_| Value::Null)
                .map_err(|e| e.to_string())
        }
        other => Err(format!("method not found: {other}")),
    }
}

/// Handle a `terminal/*` reverse request (the ACP terminal extension). All but
/// `wait_for_exit` answer inline; `wait_for_exit` parks an async waiter that
/// posts its response back through `resp_tx` when the process exits.
async fn handle_terminal(
    method: &str,
    id: &Value,
    params: &Value,
    cwd: &Path,
    registry: &mut TerminalRegistry,
    stdin: &mut Outgoing,
    resp_tx: &mpsc::UnboundedSender<(Value, Value)>,
) -> Result<(), String> {
    let term_id = params.get("terminalId").and_then(Value::as_str);

    // Answer-now methods return a Result; `wait_for_exit` is handled specially.
    let inline: Option<Result<Value, String>> = match method {
        "create" => Some(registry.create(params, cwd)),
        "output" => Some(term_id.map_or(Err("missing terminalId".into()), |t| registry.output(t))),
        "kill" => Some(term_id.map_or(Err("missing terminalId".into()), |t| registry.kill(t))),
        "release" => {
            Some(term_id.map_or(Err("missing terminalId".into()), |t| registry.release(t)))
        }
        "wait_for_exit" => None,
        other => {
            let method = format!("terminal/{other}");
            write_msg(stdin, &method_not_found(id.clone(), &method)).await?;
            return Ok(());
        }
    };

    if let Some(result) = inline {
        let response = match result {
            Ok(value) => response_ok(id.clone(), value),
            Err(message) => response_err(id.clone(), &message),
        };
        write_msg(stdin, &response).await?;
        return Ok(());
    }

    // wait_for_exit: respond immediately if already exited, else park a poller.
    let Some(tid) = term_id else {
        write_msg(stdin, &response_err(id.clone(), "missing terminalId")).await?;
        return Ok(());
    };
    if let Some(done) = registry.exit_now(tid) {
        write_msg(stdin, &response_ok(id.clone(), done)).await?;
    } else if let Some(exit) = registry.waiter(tid) {
        let rpc_id = id.clone();
        let resp_tx = resp_tx.clone();
        tokio::spawn(async move {
            // Poll the shared exit state (race-free, no missed notification).
            loop {
                if exit.lock().unwrap().exited {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(40)).await;
            }
            let result = terminal::wait_result(&exit.lock().unwrap());
            let _ = resp_tx.send((rpc_id, result));
        });
    } else {
        write_msg(stdin, &response_err(id.clone(), "unknown terminal")).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
