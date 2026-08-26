//! Headless smoke test of the ACP client against the real adapter.
//! Run: `cargo run -p onehand-core --example acp_smoke`. Connects, sends one
//! prompt, prints the streamed reply, and exits on TurnEnded/Disconnected.
//!
//! Lives in core because that is where the client lives: it drives ACP with no
//! window and no front end, which is the whole point of it.

use futures::StreamExt;
use onehand_core::acp::{connect, AcpEvent, AcpRequest, ElicitKind, ElicitOutcome, ElicitValue};
use std::io::Write;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    // A prompt that forces a tool call (+ likely a permission request).
    let prompt = std::env::args().nth(1).unwrap_or_else(|| {
        "Run the shell command `echo hello-from-onehand` and report its output.".into()
    });

    // $ACP_CMD overrides the adapter command (space-split) — e.g. point it at a
    // mock agent. Defaults to the real Claude ACP adapter.
    let (command, args) = match std::env::var("ACP_CMD") {
        Ok(c) => {
            let parts: Vec<String> = c.split_whitespace().map(String::from).collect();
            (parts[0].clone(), parts[1..].to_vec())
        }
        // The same pin the default agent uses, so the smoke test exercises the
        // adapter the app actually ships with.
        Err(_) => ("npx".into(), onehand_core::config::default_adapter_args()),
    };

    let cwd = std::env::current_dir().unwrap();
    let stream = connect(command, args, cwd, None);
    let mut stream = std::pin::pin!(stream);
    let mut tx_keep = None;

    while let Some(event) = stream.next().await {
        match event {
            AcpEvent::Connected { tx, resumed } => {
                eprintln!("== CONNECTED (resumed={resumed}) ==");
                let _ = tx.send(AcpRequest::Prompt {
                    text: prompt.clone(),
                    attachments: Vec::new(),
                });
                tx_keep = Some(tx);
            }
            AcpEvent::SessionId(id) => eprintln!("== SESSION {id} =="),
            AcpEvent::AgentChunk(s) => {
                print!("{s}");
                std::io::stdout().flush().ok();
            }
            AcpEvent::ThoughtChunk(s) => eprintln!("[think] {s}"),
            AcpEvent::UserChunk(_) => {}
            AcpEvent::ToolCall(tc) => {
                eprintln!("== TOOL {:?} [{:?}] {} ==", tc.kind, tc.status, tc.title);
            }
            AcpEvent::Plan(entries) => {
                eprintln!("[plan] {} entries", entries.len());
            }
            AcpEvent::ToolUpdate(tu) => {
                eprintln!("== TOOL UPDATE {} -> {:?} ==", tu.id, tu.status);
            }
            AcpEvent::Permission(req) => {
                // Auto-allow the first allow* option to let the turn proceed.
                let pick = req
                    .options
                    .iter()
                    .find(|o| o.kind.starts_with("allow"))
                    .or_else(|| req.options.first());
                eprintln!(
                    "== PERMISSION: {} -> picking {:?} ==",
                    req.title,
                    pick.map(|o| &o.name)
                );
                if let Some(tx) = &tx_keep {
                    let _ = tx.send(AcpRequest::PermissionResponse {
                        rpc_id: req.rpc_id.clone(),
                        option_id: pick.map(|o| o.id.clone()),
                    });
                }
            }
            AcpEvent::Elicitation(e) => {
                // Auto-answer with each field's first choice so a question can't
                // stall the headless run (the GUI parks it for the user instead).
                eprintln!(
                    "== QUESTION: {} ({} field(s)) ==",
                    e.message,
                    e.fields.len()
                );
                let answers = e
                    .fields
                    .iter()
                    .filter_map(|f| {
                        let first = f.kind.choices().first()?;
                        eprintln!("   {} -> {}", f.key, first.label);
                        Some(match f.kind {
                            ElicitKind::MultiSelect(_) => {
                                (f.key.clone(), ElicitValue::List(vec![first.value.clone()]))
                            }
                            _ => (f.key.clone(), ElicitValue::Text(first.value.clone())),
                        })
                    })
                    .collect();
                if let Some(tx) = &tx_keep {
                    let _ = tx.send(AcpRequest::ElicitationResponse {
                        rpc_id: e.rpc_id.clone(),
                        outcome: ElicitOutcome::Accept(answers),
                    });
                }
            }
            AcpEvent::AvailableCommands(cmds) => {
                eprintln!("== COMMANDS: {} ==", cmds.len());
            }
            AcpEvent::Modes { current, available } => {
                eprintln!("== MODES: current={current:?} of {} ==", available.len());
            }
            AcpEvent::ModeChanged(id) => eprintln!("== MODE -> {id} =="),
            AcpEvent::ConfigOptions(opts) => {
                for o in &opts {
                    eprintln!(
                        "== CONFIG {}: current={:?} of {} ==",
                        o.id,
                        o.current,
                        o.choices.len()
                    );
                }
            }
            AcpEvent::TerminalOutput { terminal_id, chunk } => {
                eprint!("[term {terminal_id}] {chunk}");
            }
            AcpEvent::TerminalExit {
                terminal_id,
                exit_code,
            } => {
                eprintln!("== TERM {terminal_id} EXIT {exit_code:?} ==");
            }
            AcpEvent::TurnEnded { stop_reason } => {
                eprintln!("\n== TURN ENDED: {stop_reason} ==");
                break;
            }
            AcpEvent::Error(e) => eprintln!("== ERROR: {e} =="),
            AcpEvent::Disconnected(e) => {
                eprintln!("== DISCONNECTED: {e} ==");
                break;
            }
        }
    }
}
