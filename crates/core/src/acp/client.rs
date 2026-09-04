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
//! satisfies it in production; the split is what lets the loop itself be
//! driven by a test, which is where the handshake deadline, the readiness gate
//! and the parked permission round trip are checked.

use super::parse;
use super::terminal::{self, TermStream, TerminalRegistry};
use super::types::{
    AcpEvent, AcpRequest, Attachment, ElicitChoice, ElicitField, ElicitKind, ElicitOutcome,
    ElicitValue, Elicitation, Mode, PermissionOption, PermissionRequest,
};
use crate::attachment::{inline_image_mime, MAX_INLINE_IMAGE_BYTES};
use futures::channel::mpsc::Sender as EventTx;
use futures::stream::{self, Stream, StreamExt};
use futures::SinkExt;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

/// ACP protocol version this client speaks.
const PROTOCOL_VERSION: i64 = 1;
/// How many events may sit unread before the serve loop back-pressures.
const EVENT_BUFFER: usize = 64;
/// Give the handshake (initialize → session/new) this long before declaring the
/// adapter stuck.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(90);

/// Connect to an agent adapter and stream its events. The returned stream owns
/// the child process; dropping it (subscription removed) kills the adapter
/// (`kill_on_drop`).
pub fn connect(
    command: String,
    args: Vec<String>,
    cwd: PathBuf,
    resume: Option<String>,
) -> impl Stream<Item = AcpEvent> {
    // Spawning is deferred into the stream rather than done here, so a command
    // that does not exist surfaces as `Disconnected` on the stream the caller is
    // already reading instead of as a second failure mode at the call site.
    let spawned = Transport::spawn(command, args, cwd.clone());
    connect_over_result(spawned, cwd, resume)
}

/// Anything the serve loop needs from the far end: the two pipes, and the news
/// that it will not be answering again.
///
/// **The seam is the byte stream, not the process.** A child is one thing that
/// satisfies it and the only one in production; what the split buys is that the
/// loop above it — the handshake deadline, the readiness gate, the parked
/// permission round trip — can be driven by a test at all. Boxed rather than
/// generic so the four functions that write into it keep their signatures free
/// of a type parameter that says nothing about what they do.
struct Transport {
    /// Lines the far end sends.
    incoming: Pin<Box<dyn tokio::io::AsyncBufRead + Send>>,
    /// Where messages to the far end go.
    outgoing: Outgoing,
    /// Resolves, with a sentence, once the far end can no longer answer.
    ///
    /// **Owns whatever it is waiting on**, which for a child process is the
    /// child itself — so dropping this future is what kills the adapter, and the
    /// stream owning it is what ties the agent's life to the subscription.
    ended: Pin<Box<dyn std::future::Future<Output = String> + Send>>,
}

/// The write half of a [`Transport`], as every message-writing function takes
/// it.
type Outgoing = Pin<Box<dyn tokio::io::AsyncWrite + Send>>;

impl Transport {
    /// Run `command` as a child process and talk to it over its stdio.
    fn spawn(command: String, args: Vec<String>, cwd: PathBuf) -> Result<Self, String> {
        let mut child = Command::new(&command)
            .args(&args)
            .current_dir(&cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("spawn {command}: {e}"))?;

        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let stderr = child.stderr.take().ok_or("no stderr")?;

        // Drain stderr so the adapter never blocks on a full pipe; echo for debug.
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                eprintln!("[acp:stderr] {line}");
            }
        });

        Ok(Self {
            incoming: Box::pin(BufReader::new(stdout)),
            outgoing: Box::pin(stdin),
            // The child moves in here, which is what keeps `kill_on_drop`
            // meaning what it says: this future is dropped with the transport,
            // the transport with the stream, and the stream with the caller's
            // interest in the agent.
            ended: Box::pin(async move {
                match child.wait().await {
                    Ok(status) => format!("adapter exited ({status})"),
                    Err(e) => format!("adapter wait failed: {e}"),
                }
            }),
        })
    }

    /// A transport with a test on the other end of it.
    ///
    /// Bidirectional like a socket pair: what the client writes is what the
    /// returned half reads, and the other way round. Nothing ends it, so a test
    /// that wants a dead adapter closes its half.
    #[cfg(test)]
    fn scripted() -> (Self, tokio::io::DuplexStream) {
        let (ours, theirs) = tokio::io::duplex(64 * 1024);
        let (read, write) = tokio::io::split(ours);
        (
            Self {
                incoming: Box::pin(BufReader::new(read)),
                outgoing: Box::pin(write),
                ended: Box::pin(std::future::pending()),
            },
            theirs,
        )
    }
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
            Ok(transport) => run(transport, &cwd, resume, &mut output).await,
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
    } = transport;
    let stdin = &mut outgoing;
    let mut reader = incoming.lines();
    tokio::pin!(ended);
    let (req_tx, mut req_rx) = mpsc::unbounded_channel::<AcpRequest>();

    let mut next_id: u64 = 1;
    let mut pending: HashMap<u64, Pending> = HashMap::new();
    let mut ready = false;
    let mut session_id: Option<String> = None;

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

            why = &mut ended => {
                return Err(why);
            }

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
                    Ok(None) => return Err("adapter closed stdout".into()),
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
                    &mut ready, &mut session_id, &req_tx, output, &mut registry, &resp_tx,
                )
                .await?;
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
                        let blocks = build_prompt_blocks(&text, &attachments).await;
                        let params = json!({ "sessionId": sid, "prompt": blocks });
                        write_msg(stdin, &request(id, "session/prompt", params)).await?;
                    }
                    Some(AcpRequest::SetMode(mode_id)) => {
                        if let Some(sid) = &session_id {
                            let id = alloc(&mut next_id);
                            let params = json!({ "sessionId": sid, "modeId": mode_id });
                            write_msg(stdin, &request(id, "session/set_mode", params)).await?;
                        }
                    }
                    Some(AcpRequest::SetConfigOption { config_id, value }) => {
                        if let Some(sid) = &session_id {
                            let id = alloc(&mut next_id);
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
                let load_supported = msg
                    .get("result")
                    .and_then(|r| r.get("agentCapabilities"))
                    .and_then(|c| c.get("loadSession"))
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
                    return Err(format!("session/new failed: {err}"));
                }
                let result = msg.get("result");
                let sid = result
                    .and_then(|r| r.get("sessionId"))
                    .and_then(Value::as_str)
                    .map(String::from);
                *session_id = sid.clone();
                if let Some(s) = sid {
                    let _ = output.send(AcpEvent::SessionId(s)).await;
                }
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
                    let _ = output
                        .send(AcpEvent::Error(format!(
                            "session/load failed ({err}) — starting a fresh session"
                        )))
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
            Some(Pending::PromptTurn) => {
                if let Some(err) = msg.get("error") {
                    let _ = output.send(AcpEvent::Error(err.to_string())).await;
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
            tokio::fs::read_to_string(path)
                .await
                .map(|content| json!({ "content": content }))
                .map_err(|e| e.to_string())
        }
        "fs/write_text_file" => {
            let path = params
                .get("path")
                .and_then(Value::as_str)
                .ok_or("missing path")?;
            let content = params
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default();
            tokio::fs::write(path, content)
                .await
                .map(|_| Value::Null)
                .map_err(|e| e.to_string())
        }
        other => Err(format!("unsupported method {other}")),
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
        other => Some(Err(format!("unsupported terminal/{other}"))),
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

/// Parse a `session/request_permission` into a [`PermissionRequest`], carrying
/// the raw JSON-RPC `id` so the eventual response echoes it.
fn parse_permission(rpc_id: Value, params: &Value) -> PermissionRequest {
    let tool_call = params.get("toolCall");
    let tool_call_id = tool_call
        .and_then(|t| t.get("toolCallId"))
        .and_then(Value::as_str)
        .map(String::from);
    let title = tool_call
        .and_then(|t| t.get("title"))
        .and_then(Value::as_str)
        .unwrap_or("Permission requested")
        .to_string();
    let options = params
        .get("options")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|o| PermissionOption {
                    id: o
                        .get("optionId")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    name: o
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("Option")
                        .to_string(),
                    kind: o
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    PermissionRequest {
        rpc_id,
        tool_call_id,
        title,
        options,
    }
}

// ── Elicitation (`elicitation/create`) ───────────────────────────────────────

/// Suffix marking a select field's paired free-text "Other" property. Claude's
/// adapter emits one per question (`question_0` + `question_0_custom`).
const CUSTOM_SUFFIX: &str = "_custom";

/// Parse an `elicitation/create` into an [`Elicitation`]. `None` for anything we
/// can't render as a form — url mode, a missing/empty `requestedSchema` — so the
/// caller declines instead of parking it.
fn parse_elicitation(rpc_id: Value, params: &Value) -> Option<Elicitation> {
    if params.get("mode").and_then(Value::as_str) == Some("url") {
        return None;
    }
    let props = params
        .get("requestedSchema")
        .and_then(|s| s.get("properties"))
        .and_then(Value::as_object)?;

    // `serde_json` maps are sorted, not insertion-ordered, so `question_10`
    // would sort before `question_2` and a question's `_custom` box between
    // them. Order by the numeric suffix where the adapter's convention holds,
    // and leave any other schema (an MCP form) in plain key order after it.
    let mut keys: Vec<&String> = props.keys().collect();
    keys.sort_by_key(|k| field_order(k));

    // A `<key>_custom` property belongs to `<key>` as its "Other" box rather
    // than standing on its own.
    let fields = keys
        .iter()
        .filter(|k| {
            k.strip_suffix(CUSTOM_SUFFIX)
                .is_none_or(|stem| !props.contains_key(stem))
        })
        .map(|k| {
            let schema = &props[*k];
            let custom = format!("{k}{CUSTOM_SUFFIX}");
            ElicitField {
                key: (*k).clone(),
                title: str_field(schema, "title"),
                description: str_field(schema, "description"),
                kind: parse_elicit_kind(schema),
                custom_key: props.contains_key(&custom).then_some(custom),
            }
        })
        .collect::<Vec<_>>();
    if fields.is_empty() {
        return None;
    }

    Some(Elicitation {
        rpc_id,
        tool_call_id: str_field(params, "toolCallId"),
        message: str_field(params, "message").unwrap_or_else(|| "The agent is asking".into()),
        fields,
    })
}

/// Sort key placing `question_<n>` (and its `_custom` box) in the adapter's own
/// order; everything else sorts after, by key.
fn field_order(key: &str) -> (u8, u64, u8, String) {
    let (stem, custom) = match key.strip_suffix(CUSTOM_SUFFIX) {
        Some(stem) => (stem, 1),
        None => (key, 0),
    };
    match stem
        .strip_prefix("question_")
        .and_then(|n| n.parse::<u64>().ok())
    {
        Some(n) => (0, n, custom, String::new()),
        None => (1, 0, custom, key.to_string()),
    }
}

/// A string property of a JSON object, if present and non-empty.
fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// Classify one schema property: an array of choices → multi-select, a scalar
/// with choices → single select, anything else → free text.
fn parse_elicit_kind(schema: &Value) -> ElicitKind {
    if schema.get("type").and_then(Value::as_str) == Some("array") {
        return match schema.get("items").map(parse_choices) {
            Some(c) if !c.is_empty() => ElicitKind::MultiSelect(c),
            _ => ElicitKind::Text,
        };
    }
    match parse_choices(schema) {
        c if c.is_empty() => ElicitKind::Text,
        c => ElicitKind::Select(c),
    }
}

/// The choices of an enum-typed schema: ACP's titled `oneOf`/`anyOf` option
/// objects, or a plain JSON-Schema `enum` array (optionally labelled by the
/// MCP-style `enumNames`).
fn parse_choices(schema: &Value) -> Vec<ElicitChoice> {
    if let Some(arr) = schema
        .get("oneOf")
        .or_else(|| schema.get("anyOf"))
        .and_then(Value::as_array)
    {
        return arr
            .iter()
            .filter_map(|o| {
                let value = o.get("const").and_then(Value::as_str)?.to_string();
                let label = str_field(o, "title").unwrap_or_else(|| value.clone());
                Some(ElicitChoice {
                    value,
                    label,
                    description: str_field(o, "description"),
                })
            })
            .collect();
    }
    let names = schema.get("enumNames").and_then(Value::as_array);
    schema
        .get("enum")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .enumerate()
                .filter_map(|(i, v)| {
                    let value = v.as_str()?.to_string();
                    let label = names
                        .and_then(|n| n.get(i))
                        .and_then(Value::as_str)
                        .map(String::from)
                        .unwrap_or_else(|| value.clone());
                    Some(ElicitChoice {
                        value,
                        label,
                        description: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Build the `elicitation/create` result from the user's outcome.
fn elicit_result(outcome: ElicitOutcome) -> Value {
    match outcome {
        ElicitOutcome::Accept(answers) => {
            let content: serde_json::Map<String, Value> = answers
                .into_iter()
                .map(|(k, v)| {
                    let v = match v {
                        ElicitValue::Text(s) => Value::String(s),
                        ElicitValue::List(l) => {
                            Value::Array(l.into_iter().map(Value::String).collect())
                        }
                    };
                    (k, v)
                })
                .collect();
            json!({ "action": "accept", "content": content })
        }
        ElicitOutcome::Decline => json!({ "action": "decline" }),
        ElicitOutcome::Cancel => json!({ "action": "cancel" }),
    }
}

// ── JSON-RPC helpers ─────────────────────────────────────────────────────────

fn alloc(next: &mut u64) -> u64 {
    let id = *next;
    *next += 1;
    id
}

fn init_params() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "clientCapabilities": {
            "fs": { "readTextFile": true, "writeTextFile": true },
            "terminal": true,
            // Form elicitation (`elicitation/create`) is what unlocks the agent's
            // *question* prompts: Claude's adapter puts `AskUserQuestion` in
            // `disallowedTools` unless this is advertised, so without it the model
            // can never offer the user a choice — it just guesses and proceeds.
            // `{}` is the whole capability ("we can render a form"); we answer the
            // request in `handle_message` and, for a form we can't render, decline
            // immediately rather than park it forever.
            "elicitation": { "form": {} }
        }
    })
}

fn request(id: u64, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

fn notification(method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "method": method, "params": params })
}

fn response_ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn response_err(id: Value, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32603, "message": message } })
}

async fn write_msg(stdin: &mut Outgoing, msg: &Value) -> Result<(), String> {
    let mut line = serde_json::to_string(msg).map_err(|e| e.to_string())?;
    line.push('\n');
    stdin
        .write_all(line.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    stdin.flush().await.map_err(|e| e.to_string())?;
    Ok(())
}

/// Parse the `modes` object from a `session/new` result into a `Modes` event.
fn parse_modes(modes: &Value) -> AcpEvent {
    let current = modes
        .get("currentModeId")
        .and_then(Value::as_str)
        .map(String::from);
    let available = modes
        .get("availableModes")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let id = m
                        .get("id")
                        .or_else(|| m.get("modeId"))
                        .and_then(Value::as_str)?
                        .to_string();
                    let name = m
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or(&id)
                        .to_string();
                    Some(Mode { id, name })
                })
                .collect()
        })
        .unwrap_or_default();
    AcpEvent::Modes { current, available }
}

/// Build the ACP `prompt` content-block array: a text block plus one block per
/// attachment — inline `image` (base64) for images, else a `resource_link`.
/// Max image size sent inline (base64 in the prompt JSON). Reading is
/// whole-file-into-memory and base64 inflates it ~4/3× (plus a serialize copy),
/// so an uncapped read of a huge file would transiently allocate several times
/// its size. Anything larger degrades to a `resource_link`.
async fn build_prompt_blocks(text: &str, attachments: &[Attachment]) -> Value {
    let mut blocks = vec![json!({ "type": "text", "text": text })];
    for att in attachments {
        let name = att
            .path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Some(mime) = inline_image_mime(&att.path) {
            let size_ok = tokio::fs::metadata(&att.path)
                .await
                .map(|m| m.len() <= MAX_INLINE_IMAGE_BYTES)
                .unwrap_or(false);
            if size_ok {
                if let Ok(bytes) = tokio::fs::read(&att.path).await {
                    blocks.push(json!({
                        "type": "image",
                        "mimeType": mime,
                        "data": base64_encode(&bytes),
                    }));
                    continue;
                }
            }
        }
        blocks.push(json!({
            "type": "resource_link",
            "uri": file_uri(&att.path),
            "name": name,
        }));
    }
    Value::Array(blocks)
}

/// A `file://` URI with the path percent-encoded (RFC 3986): unreserved chars
/// and `/` pass through, everything else — spaces, `#`, `?`, non-ASCII — is
/// escaped byte-wise. A raw `path.display()` URI truncates at the first `#`
/// on the adapter side.
fn file_uri(path: &std::path::Path) -> String {
    let mut out = String::from("file://");
    for &b in path.display().to_string().as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Dependency-free standard base64 encoder.
pub fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | (b[2] as u32);
        out.push(ALPHABET[(n >> 18 & 63) as usize] as char);
        out.push(ALPHABET[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{
        base64_encode, connect_over, elicit_result, file_uri, parse_elicitation, Transport,
    };
    use crate::acp::{AcpEvent, ElicitKind, ElicitOutcome, ElicitValue};
    use futures::StreamExt as _;
    use serde_json::{json, Value};
    use std::path::PathBuf;
    use tokio::io::{AsyncBufReadExt as _, BufReader};

    /// The agent's side of a scripted transport: read what the client wrote,
    /// write back what an adapter would have said.
    struct Agent {
        lines: tokio::io::Lines<BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>,
        /// Held rather than read: dropping this half closes the pipe the client
        /// is reading, which ends its loop with "adapter closed stdout" before a
        /// test has said anything.
        #[allow(dead_code)]
        out: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    }

    impl Agent {
        /// The next JSON-RPC message the client sent.
        async fn heard(&mut self) -> Value {
            let line = self
                .lines
                .next_line()
                .await
                .expect("read the client's pipe")
                .expect("the client wrote a line");
            serde_json::from_str(&line).expect("the client writes JSON")
        }
    }

    /// Run the client over a scripted transport, with its events collected on a
    /// task of their own.
    ///
    /// The serve loop is folded into the stream, so something has to poll it for
    /// the client to make any progress at all — driving it here is what lets the
    /// test body read and write the agent's side as a conversation.
    fn client() -> (Agent, tokio::task::JoinHandle<Vec<AcpEvent>>) {
        let (transport, agent) = Transport::scripted();
        let (read, out) = tokio::io::split(agent);
        let events = tokio::spawn(async move {
            let stream = connect_over(transport, PathBuf::from("/tmp"), None);
            futures::pin_mut!(stream);
            let mut seen = Vec::new();
            while let Some(event) = stream.next().await {
                seen.push(event);
            }
            seen
        });
        (
            Agent {
                lines: BufReader::new(read).lines(),
                out,
            },
            events,
        )
    }

    /// **The capability that makes multiple-choice prompts appear at all.** The
    /// adapter puts `AskUserQuestion` in `disallowedTools` unless the client
    /// advertises it, so dropping it does not break anything visibly — the model
    /// simply stops asking and starts guessing, which is a wrong answer wearing
    /// the shape of a right one.
    #[tokio::test]
    async fn the_handshake_advertises_the_form_capability_questions_need() {
        let (mut agent, _events) = client();

        let hello = agent.heard().await;

        assert_eq!(hello["method"], "initialize");
        // Present, whatever it holds. The capability *is* the key: the value is
        // an empty object today, and an adapter reads this the same way — the
        // question it asks is whether the client claimed it can render a form.
        assert!(
            !hello["params"]["clientCapabilities"]["elicitation"]["form"].is_null(),
            "{hello:#}"
        );
    }

    /// The shape Claude's adapter sends for a two-question `AskUserQuestion`:
    /// indexed fields, each with its own `_custom` "Other" box.
    fn ask_params() -> serde_json::Value {
        json!({
            "mode": "form",
            "sessionId": "s1",
            "toolCallId": "tool-7",
            "message": "Please answer the following questions.",
            "requestedSchema": {
                "type": "object",
                "properties": {
                    "question_0": {
                        "type": "string",
                        "title": "Storage",
                        "description": "Where should it live?",
                        "oneOf": [
                            { "const": "sqlite", "title": "SQLite", "description": "One file" },
                            { "const": "postgres", "title": "Postgres" },
                        ],
                    },
                    "question_0_custom": { "type": "string", "title": "Other" },
                    "question_1": {
                        "type": "array",
                        "title": "Extras",
                        "items": { "anyOf": [{ "const": "tests" }, { "const": "docs" }] },
                    },
                    "question_1_custom": { "type": "string", "title": "Other" },
                },
            },
        })
    }

    #[test]
    fn parses_ask_user_question_form() {
        let e = parse_elicitation(json!(3), &ask_params()).unwrap();
        assert_eq!(e.tool_call_id.as_deref(), Some("tool-7"));
        // The `_custom` boxes attach to their question instead of standing as
        // fields of their own.
        assert_eq!(e.fields.len(), 2);

        let q0 = &e.fields[0];
        assert_eq!(q0.key, "question_0");
        assert_eq!(q0.custom_key.as_deref(), Some("question_0_custom"));
        assert_eq!(q0.description.as_deref(), Some("Where should it live?"));
        let ElicitKind::Select(choices) = &q0.kind else {
            panic!("expected a select")
        };
        assert_eq!(choices[0].value, "sqlite");
        assert_eq!(choices[0].label, "SQLite");
        assert_eq!(choices[0].description.as_deref(), Some("One file"));
        // No `title` → the label falls back to the wire value.
        assert_eq!(choices[1].label, "Postgres");

        let ElicitKind::MultiSelect(extras) = &e.fields[1].kind else {
            panic!("expected multi")
        };
        assert_eq!(extras.len(), 2);
    }

    #[test]
    fn orders_questions_numerically_not_lexically() {
        // `serde_json` maps are sorted, so "question_10" sorts before
        // "question_2" — the numeric suffix has to drive the order.
        let mut props = serde_json::Map::new();
        for n in [0u32, 2, 10] {
            props.insert(format!("question_{n}"), json!({ "type": "string" }));
        }
        let params = json!({
            "message": "m",
            "requestedSchema": { "type": "object", "properties": props },
        });
        let e = parse_elicitation(json!(1), &params).unwrap();
        let keys: Vec<&str> = e.fields.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, ["question_0", "question_2", "question_10"]);
    }

    #[test]
    fn parses_plain_enum_and_free_text_fields() {
        // A generic form (an MCP server's, or the refusal-fallback prompt):
        // arbitrary keys, MCP-style `enum` + `enumNames`, and a bare string.
        let params = json!({
            "message": "Retry?",
            "requestedSchema": { "type": "object", "properties": {
                "choice": { "type": "string", "enum": ["retry", "keep"],
                            "enumNames": ["Retry", "Keep the refusal"] },
                "note": { "type": "string", "title": "Note" },
            }},
        });
        let e = parse_elicitation(json!(1), &params).unwrap();
        let ElicitKind::Select(c) = &e.fields[0].kind else {
            panic!("expected a select")
        };
        assert_eq!(c[0].label, "Retry");
        assert_eq!(c[1].value, "keep");
        assert!(e.fields[0].custom_key.is_none()); // no `choice_custom` property
        assert_eq!(e.fields[1].kind, ElicitKind::Text);
    }

    #[test]
    fn unrenderable_forms_are_rejected_so_the_caller_declines() {
        let url = json!({ "mode": "url", "url": "https://example.com" });
        assert!(parse_elicitation(json!(1), &url).is_none());
        // No schema at all, and a schema with no properties.
        assert!(parse_elicitation(json!(1), &json!({ "message": "hi" })).is_none());
        let empty = json!({ "requestedSchema": { "type": "object", "properties": {} } });
        assert!(parse_elicitation(json!(1), &empty).is_none());
    }

    #[test]
    fn accept_result_carries_answers_by_key() {
        let out = elicit_result(ElicitOutcome::Accept(vec![
            ("question_0".into(), ElicitValue::Text("sqlite".into())),
            (
                "question_1".into(),
                ElicitValue::List(vec!["tests".into(), "docs".into()]),
            ),
        ]));
        assert_eq!(
            out,
            json!({ "action": "accept",
                    "content": { "question_0": "sqlite", "question_1": ["tests", "docs"] } })
        );
        assert_eq!(
            elicit_result(ElicitOutcome::Decline),
            json!({ "action": "decline" })
        );
        assert_eq!(
            elicit_result(ElicitOutcome::Cancel),
            json!({ "action": "cancel" })
        );
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn file_uri_percent_encodes_reserved_and_non_ascii() {
        use std::path::Path;
        assert_eq!(
            file_uri(Path::new("/home/u/proj/src/lib.rs")),
            "file:///home/u/proj/src/lib.rs"
        );
        assert_eq!(
            file_uri(Path::new("/home/u/My Docs/notes#1.txt")),
            "file:///home/u/My%20Docs/notes%231.txt"
        );
        // Multibyte (Vietnamese) escapes per UTF-8 byte.
        assert_eq!(
            file_uri(Path::new("/a/tệp.txt")),
            "file:///a/t%E1%BB%87p.txt"
        );
    }
}
