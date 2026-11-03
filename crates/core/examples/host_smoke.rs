//! Smoke test of a host session surviving a detach.
//! Run: `host_smoke <ssh-dest|--local> <cwd> [go|watch <session>]`. `go` installs
//! `target/debug/onehand-host`, opens a session on `$ACP_CMD` (space-split, as
//! `acp_smoke` reads it), sends one prompt, prints events for 3 s and exits
//! mid-turn with the session id. `watch <session>` attaches again and prints
//! every event until `TurnEnded` and for 1 s after, exiting 1 on `Disconnected`. `--local` runs
//! the relay directly, not over ssh.
//!
//! The agent runs in the session's cwd, so the paths in `ACP_CMD` must be
//! absolute. `mock_terminal_agent.js` ends its turn; `mock_ui_agent.js` parks
//! on its cards and never sends `TurnEnded`, so a `watch` of it never ends.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use futures::StreamExt;
use onehand_core::acp::{AcpEvent, AcpRequest, Opening};
use onehand_core::config::AgentSpec;
use onehand_core::host::client::{connect, install, HostAddr, HostConn};
use onehand_core::host::wire::build_id;

const BINARY: &str = "target/debug/onehand-host";
const USAGE: &str = "usage: host_smoke <ssh-dest|--local> <cwd> [go|watch <session>]";

fn show(event: &AcpEvent) {
    let text = format!("{event:?}");
    let cut: String = text.chars().take(200).collect();
    println!("{cut}");
}

async fn attach(addr: &HostAddr) -> Result<HostConn, String> {
    let binary = Path::new(BINARY);
    let remote = install(addr, binary)
        .await
        .map_err(|e| format!("install: {e:?}"))?;
    let bytes = std::fs::read(binary).map_err(|e| format!("{BINARY}: {e}"))?;
    connect(addr, &remote, &build_id(&bytes))
        .await
        .map_err(|e| format!("connect: {e:?}"))
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    match smoke().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("host_smoke: {why}");
            ExitCode::FAILURE
        }
    }
}

async fn smoke() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let (Some(dest), Some(cwd)) = (args.next(), args.next()) else {
        return Err(USAGE.into());
    };
    let mode = args.next().unwrap_or_else(|| "go".into());
    let addr = HostAddr {
        local: dest == "--local",
        ssh: dest,
    };
    match mode.as_str() {
        "watch" => {
            let session = args.next().ok_or(USAGE)?;
            watch(&attach(&addr).await?, &session).await
        }
        "go" => go(&attach(&addr).await?, PathBuf::from(cwd)).await,
        _ => Err(USAGE.into()),
    }
}

async fn watch(conn: &HostConn, session: &str) -> Result<(), String> {
    let mut stream = std::pin::pin!(conn.watch(session));
    while let Some(event) = stream.next().await {
        show(&event);
        if matches!(event, AcpEvent::TurnEnded { .. }) {
            // A replayed turn end comes ahead of the watch's one `Connected`.
            let grace = tokio::time::sleep(Duration::from_secs(1));
            tokio::pin!(grace);
            loop {
                tokio::select! {
                    () = &mut grace => return Ok(()),
                    event = stream.next() => match event {
                        Some(event) => show(&event),
                        None => return Ok(()),
                    },
                }
            }
        }
        if matches!(event, AcpEvent::Disconnected(_)) {
            return Err("the session disconnected before its turn ended".into());
        }
    }
    Err("the watch ended before the turn did".into())
}

async fn go(conn: &HostConn, cwd: PathBuf) -> Result<(), String> {
    let cmd = std::env::var("ACP_CMD").map_err(|_| "set ACP_CMD to the agent command")?;
    let mut parts = cmd.split_whitespace().map(String::from);
    let agent = AgentSpec {
        name: "smoke".into(),
        command: parts.next().ok_or("ACP_CMD is empty")?,
        args: parts.collect(),
        ..Default::default()
    };
    let session = conn
        .open(cwd, agent, Opening::Fresh)
        .await
        .map_err(|e| format!("open: {e:?}"))?;
    let mut stream = std::pin::pin!(conn.watch(&session));
    let deadline = tokio::time::sleep(Duration::from_secs(3));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            () = &mut deadline => break,
            event = stream.next() => {
                let Some(event) = event else { break };
                show(&event);
                if let AcpEvent::Connected { tx, .. } = &event {
                    let _ = tx.send(AcpRequest::Prompt {
                        text: "hello from host_smoke".into(),
                        attachments: Vec::new(),
                    });
                }
            }
        }
    }
    println!("session {session}");
    Ok(())
}
