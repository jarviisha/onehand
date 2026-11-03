use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::channel::mpsc as fmpsc;
use onehand_core::acp::types::ReqRx;
use onehand_core::acp::{AcpEvent, AcpRequest, Attachment, Opening, PermissionRequest};
use onehand_core::config::{AgentSpec, PresetId};
use onehand_core::host::journal;
use onehand_core::host::wire::{Entry, FromHost, Readiness};
use serde_json::json;
use tokio::sync::mpsc;

use super::*;

/// The agents the fake connector opened, in order: the sender each one's
/// event stream is fed from.
type Opened = Arc<Mutex<Vec<fmpsc::UnboundedSender<AcpEvent>>>>;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-host-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn hub(name: &str) -> (Arc<Mutex<Hub>>, Opened) {
    let opened: Opened = Arc::default();
    let seen = opened.clone();
    let connector: Connector = Arc::new(move |_launch, _opts| {
        let (tx, rx) = fmpsc::unbounded();
        seen.lock().unwrap().push(tx);
        rx.boxed()
    });
    let hub = Hub::new(scratch(name), None, connector);
    (Arc::new(Mutex::new(hub)), opened)
}

fn agent() -> AgentSpec {
    AgentSpec {
        name: "Fake".into(),
        command: "fake-agent".into(),
        ..Default::default()
    }
}

/// A fake agent on a session that is open: push its events, read its requests.
struct Fake {
    session: String,
    events: fmpsc::UnboundedSender<AcpEvent>,
    requests: ReqRx,
}

impl Fake {
    fn push(&self, event: AcpEvent) {
        self.events.unbounded_send(event).unwrap();
    }

    async fn request(&mut self) -> AcpRequest {
        tokio::time::timeout(Duration::from_secs(5), self.requests.recv())
            .await
            .expect("the agent got a request in time")
            .expect("the request channel is open")
    }
}

/// Open a session and, unless `ready` is false, bring its agent up.
fn open(hub: &Arc<Mutex<Hub>>, opened: &Opened, ready: bool) -> Fake {
    let session = Hub::open(hub, "/tmp".into(), agent(), Opening::Fresh).unwrap();
    let events = opened.lock().unwrap().last().unwrap().clone();
    let (tx, requests) = mpsc::unbounded_channel();
    let fake = Fake {
        session,
        events,
        requests,
    };
    if ready {
        fake.push(AcpEvent::Connected {
            tx,
            resumed: false,
            forkable: true,
        });
    }
    fake
}

fn watch(hub: &Arc<Mutex<Hub>>, session: &str) -> mpsc::UnboundedReceiver<FromHost> {
    let (sink, lines) = mpsc::unbounded_channel();
    hub.lock().unwrap().watch(session, sink).unwrap();
    lines
}

/// The next `n` entries a sink gets, as (seq, entry).
async fn entries(lines: &mut mpsc::UnboundedReceiver<FromHost>, n: usize) -> Vec<(u64, Entry)> {
    let mut out = Vec::new();
    while out.len() < n {
        let line = tokio::time::timeout(Duration::from_secs(5), lines.recv())
            .await
            .expect("an entry arrived in time")
            .expect("the sink is open");
        match line {
            FromHost::Entry { seq, entry, .. } => out.push((seq, entry)),
            FromHost::Answer { .. } | FromHost::Kicked => panic!("a sink only carries entries"),
        }
    }
    out
}

/// Let the sessions' tasks run what is queued for them.
async fn settle() {
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
}

fn chunk(text: &str) -> AcpEvent {
    AcpEvent::AgentChunk {
        text: text.into(),
        message: None,
    }
}

fn prompt(text: &str) -> AcpRequest {
    AcpRequest::Prompt {
        text: text.into(),
        attachments: vec![],
    }
}

/// A short name for an entry, so a test can say which ones it expects.
fn kind(entry: &Entry) -> String {
    let json = serde_json::to_value(entry).unwrap();
    match json {
        serde_json::Value::Object(map) => {
            let (outer, inner) = map.into_iter().next().unwrap();
            match inner {
                serde_json::Value::Object(inner) if outer != "ready" => {
                    format!("{outer}:{}", inner.keys().next().unwrap())
                }
                serde_json::Value::String(inner) => format!("{outer}:{inner}"),
                _ => outer,
            }
        }
        serde_json::Value::String(s) => s,
        other => other.to_string(),
    }
}

fn kinds(entries: &[(u64, Entry)]) -> Vec<String> {
    entries.iter().map(|(_, e)| kind(e)).collect()
}

#[tokio::test]
async fn opened_session_streams_ready_then_events() {
    let (hub, opened) = hub("streams");
    let fake = open(&hub, &opened, true);
    fake.push(AcpEvent::SessionId("s".into()));
    fake.push(chunk("hi"));
    let mut lines = watch(&hub, &fake.session);
    let got = entries(&mut lines, 3).await;
    assert_eq!(got.iter().map(|(s, _)| *s).collect::<Vec<_>>(), [0, 1, 2]);
    assert_eq!(
        kinds(&got),
        ["ready", "event:SessionId", "event:AgentChunk"]
    );
}

#[tokio::test]
async fn detach_mid_turn_loses_nothing() {
    let (hub, opened) = hub("detach");
    let fake = open(&hub, &opened, true);
    let mut lines = watch(&hub, &fake.session);
    entries(&mut lines, 1).await;
    hub.lock()
        .unwrap()
        .request(&fake.session, prompt("go"))
        .unwrap();
    drop(lines);
    for text in ["a", "b", "c"] {
        fake.push(chunk(text));
    }
    fake.push(AcpEvent::TurnEnded {
        stop_reason: "end_turn".into(),
    });
    let mut lines = watch(&hub, &fake.session);
    let got = entries(&mut lines, 6).await;
    assert_eq!(
        kinds(&got),
        [
            "ready",
            "request:Prompt",
            "event:AgentChunk",
            "event:AgentChunk",
            "event:AgentChunk",
            "event:TurnEnded"
        ]
    );
    let list = hub.lock().unwrap().list();
    assert_eq!(list.len(), 1);
    assert!(!list[0].busy);
}

#[tokio::test]
async fn live_requests_are_not_echoed() {
    let (hub, opened) = hub("echo");
    let mut fake = open(&hub, &opened, true);
    let mut lines = watch(&hub, &fake.session);
    entries(&mut lines, 1).await;
    hub.lock()
        .unwrap()
        .request(&fake.session, prompt("go"))
        .unwrap();
    let AcpRequest::Prompt { text, .. } = fake.request().await else {
        panic!("the agent got something other than the prompt");
    };
    assert_eq!(text, "go");
    settle().await;
    assert!(
        lines.try_recv().is_err(),
        "the live sink got the request back"
    );
    let mut fresh = watch(&hub, &fake.session);
    assert_eq!(
        kinds(&entries(&mut fresh, 2).await),
        ["ready", "request:Prompt"]
    );
    assert!(hub.lock().unwrap().list()[0].busy);
}

#[tokio::test]
async fn card_parked_while_detached_is_answerable_after() {
    let (hub, opened) = hub("card");
    let mut fake = open(&hub, &opened, true);
    fake.push(AcpEvent::Permission(PermissionRequest {
        rpc_id: json!(7),
        tool_call_id: None,
        kind: onehand_core::acp::ToolKind::Other,
        title: "run it".into(),
        options: vec![],
        child: None,
    }));
    let mut lines = watch(&hub, &fake.session);
    assert_eq!(
        kinds(&entries(&mut lines, 2).await),
        ["ready", "event:Permission"]
    );
    hub.lock()
        .unwrap()
        .request(
            &fake.session,
            AcpRequest::PermissionResponse {
                rpc_id: json!(7),
                option_id: Some("allow".into()),
            },
        )
        .unwrap();
    let AcpRequest::PermissionResponse { rpc_id, option_id } = fake.request().await else {
        panic!("the agent got something other than the answer");
    };
    assert_eq!(rpc_id, json!(7));
    assert_eq!(option_id.as_deref(), Some("allow"));
}

#[tokio::test]
async fn attachments_are_dropped_and_said() {
    let (hub, opened) = hub("attach");
    let mut fake = open(&hub, &opened, true);
    settle().await;
    hub.lock()
        .unwrap()
        .request(
            &fake.session,
            AcpRequest::Prompt {
                text: "look".into(),
                attachments: vec![Attachment {
                    path: "/laptop/file.png".into(),
                }],
            },
        )
        .unwrap();
    let AcpRequest::Prompt { text, attachments } = fake.request().await else {
        panic!("the agent got something other than the prompt");
    };
    assert_eq!(text, "look");
    assert!(attachments.is_empty());
    let dir = hub.lock().unwrap().dir.join("sessions").join(&fake.session);
    let journal = journal::read(&dir).unwrap();
    // In the order the laptop saw them live: its prompt, then the error.
    assert_eq!(kinds(&journal), ["ready", "request:Prompt", "event:Error"]);
    let Entry::Event(AcpEvent::Error { text, detail }) = &journal[2].1 else {
        panic!("the third entry is not the error");
    };
    assert_eq!(
        text,
        "Attachments are not sent to a host yet; the prompt went without them"
    );
    assert!(detail.is_none());
}

#[tokio::test]
async fn request_before_ready_is_refused() {
    let (hub, opened) = hub("early");
    let fake = open(&hub, &opened, false);
    settle().await;
    let refused = hub.lock().unwrap().request(&fake.session, prompt("go"));
    assert_eq!(refused, Err("the agent is still starting".to_string()));
}

#[tokio::test]
async fn ended_session_is_refused_and_listed_ended() {
    let (hub, opened) = hub("ended");
    let fake = open(&hub, &opened, true);
    let mut lines = watch(&hub, &fake.session);
    let Fake {
        session, events, ..
    } = fake;
    drop(events);
    opened.lock().unwrap().clear();
    let got = entries(&mut lines, 2).await;
    assert_eq!(kinds(&got), ["ready", "event:Disconnected"]);
    let list = hub.lock().unwrap().list();
    assert!(list[0].ended);
    assert!(!list[0].busy);
    let refused = hub.lock().unwrap().request(&session, prompt("go"));
    assert_eq!(refused, Err("this session has ended".to_string()));
}

#[tokio::test]
async fn unknown_preset_fails_open() {
    let (hub, _opened) = hub("preset");
    let spec = AgentSpec {
        name: "Gone".into(),
        preset: Some(PresetId::Unknown("gone-agent".into())),
        ..Default::default()
    };
    let want = spec.launch(None).unwrap_err();
    let got = Hub::open(&hub, "/tmp".into(), spec, Opening::Fresh);
    assert_eq!(got, Err(want));
    assert!(hub.lock().unwrap().list().is_empty());
}

/// A hub on `dir` as it is, the way a service started again finds it.
fn hub_on(dir: PathBuf) -> (Arc<Mutex<Hub>>, Opened) {
    let opened: Opened = Arc::default();
    let seen = opened.clone();
    let connector: Connector = Arc::new(move |_launch, _opts| {
        let (tx, rx) = fmpsc::unbounded();
        seen.lock().unwrap().push(tx);
        rx.boxed()
    });
    let mut hub = Hub::new(dir, None, connector);
    hub.load();
    (Arc::new(Mutex::new(hub)), opened)
}

#[tokio::test]
async fn a_restarted_hub_lists_and_replays_earlier_sessions() {
    let (hub, opened) = hub("restart");
    let fake = open(&hub, &opened, true);
    fake.push(AcpEvent::SessionId("s".into()));
    fake.push(chunk("hi"));
    settle().await;
    let dir = hub.lock().unwrap().dir.clone();
    drop(hub);

    let (hub, _opened) = hub_on(dir);
    let list = hub.lock().unwrap().list();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].session, fake.session);
    assert_eq!(list[0].cwd, PathBuf::from("/tmp"));
    assert_eq!(list[0].agent, "Fake");
    assert!(list[0].ended);
    assert!(!list[0].busy);

    let mut lines = watch(&hub, &fake.session);
    let got = entries(&mut lines, 4).await;
    assert_eq!(
        kinds(&got),
        [
            "ready",
            "event:SessionId",
            "event:AgentChunk",
            "event:Disconnected"
        ]
    );
    let Entry::Event(AcpEvent::Disconnected(why)) = &got[3].1 else {
        panic!("the last entry is not the end");
    };
    assert_eq!(
        why,
        "The host service stopped while this session was running"
    );
    settle().await;
    assert!(
        lines.try_recv().is_err(),
        "the session ended more than once"
    );

    let refused = hub.lock().unwrap().request(&fake.session, prompt("go"));
    assert_eq!(refused, Err("this session has ended".to_string()));
}

#[tokio::test]
async fn a_session_that_had_ended_is_not_ended_twice() {
    let (hub, opened) = hub("ended-twice");
    let fake = open(&hub, &opened, true);
    fake.push(AcpEvent::Disconnected("the agent died".into()));
    settle().await;
    let dir = hub.lock().unwrap().dir.clone();
    drop(hub);
    let sessions = dir.join("sessions").join(&fake.session);
    let before = journal::read(&sessions).unwrap().len();

    let (hub, _opened) = hub_on(dir);
    assert!(hub.lock().unwrap().list()[0].ended);
    assert_eq!(journal::read(&sessions).unwrap().len(), before);
}

#[tokio::test]
async fn a_directory_without_its_session_file_is_skipped() {
    let dir = scratch("no-session-file");
    let stray = dir.join("sessions").join("1-1");
    let mut j = Journal::open(&stray).unwrap();
    j.append(&Entry::Ready(Readiness {
        resumed: false,
        forkable: false,
    }))
    .unwrap();
    drop(j);
    let (hub, _opened) = hub_on(dir);
    assert!(hub.lock().unwrap().list().is_empty());
}

#[tokio::test]
async fn an_ended_session_holds_no_open_journal() {
    let (hub, opened) = hub("no-handle");
    let fake = open(&hub, &opened, true);
    let left = open(&hub, &opened, true);
    settle().await;
    let Fake { events, .. } = fake;
    drop(events);
    opened.lock().unwrap().remove(0);
    settle().await;
    {
        let hub = hub.lock().unwrap();
        assert!(hub.sessions[&left.session].journal.is_some());
        let ended = hub.sessions.values().find(|s| s.ended).unwrap();
        assert!(
            ended.journal.is_none(),
            "an ended session keeps its journal open"
        );
    }
    let dir = hub.lock().unwrap().dir.clone();
    drop(hub);
    let (hub, _opened) = hub_on(dir);
    let hub = hub.lock().unwrap();
    assert_eq!(hub.sessions.len(), 2);
    assert!(hub.sessions.values().all(|s| s.journal.is_none()));
}
