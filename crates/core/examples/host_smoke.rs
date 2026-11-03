//! Smoke test of a host session surviving a detach.
//! Run: `host_smoke <ssh-dest|--local> <cwd> [go|watch <session>]`. `go` installs
//! `target/debug/onehand-host`, opens a session on `$ACP_CMD` (space-split, as
//! `acp_smoke` reads it), sends one prompt, prints events for 3 s and exits
//! mid-turn with the session id. `watch <session>` attaches again and prints
//! every event until `TurnEnded`. `--local` runs the relay directly, not over ssh.

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::StreamExt;
use onehand_core::acp::{AcpEvent, AcpRequest, Opening};
use onehand_core::config::AgentSpec;
use onehand_core::host::client::{connect, install, HostAddr, HostConn};
use onehand_core::host::wire::build_id;

const BINARY: &str = "target/debug/onehand-host";

fn show(event: &AcpEvent) {
    let text = format!("{event:?}");
    let cut: String = text.chars().take(200).collect();
    println!("{cut}");
}

async fn attach(addr: &HostAddr) -> HostConn {
    let binary = Path::new(BINARY);
    let remote = install(addr, binary).await.expect("install");
    let build = build_id(&std::fs::read(binary).expect("read binary"));
    connect(addr, &remote, &build).await.expect("connect")
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let dest = args
        .next()
        .expect("usage: host_smoke <ssh-dest|--local> <cwd> [go|watch <session>]");
    let cwd = PathBuf::from(args.next().expect("a cwd"));
    let mode = args.next().unwrap_or_else(|| "go".into());
    let addr = HostAddr {
        local: dest == "--local",
        ssh: dest,
    };
    let conn = attach(&addr).await;

    if mode == "watch" {
        let session = args.next().expect("a session id");
        let mut stream = std::pin::pin!(conn.watch(&session));
        while let Some(event) = stream.next().await {
            show(&event);
            if matches!(
                event,
                AcpEvent::TurnEnded { .. } | AcpEvent::Disconnected(_)
            ) {
                break;
            }
        }
        return;
    }

    let cmd = std::env::var("ACP_CMD").expect("set ACP_CMD to the agent command");
    let mut parts = cmd.split_whitespace().map(String::from);
    let agent = AgentSpec {
        name: "smoke".into(),
        command: parts.next().expect("an ACP_CMD"),
        args: parts.collect(),
        ..Default::default()
    };
    let session = conn.open(cwd, agent, Opening::Fresh).await.expect("open");
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
}
