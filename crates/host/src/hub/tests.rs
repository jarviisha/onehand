use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::channel::mpsc as fmpsc;
use onehand_core::acp::types::ReqRx;
use onehand_core::acp::{AcpEvent, AcpRequest, Attachment, Opening, PermissionRequest};
use onehand_core::config::{AgentSpec, PresetId};
use onehand_core::host::journal;
use onehand_core::host::wire::{Entry, FromHost, HostSession, Readiness};
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
    session: HostSession,
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

fn watch(hub: &Arc<Mutex<Hub>>, session: &HostSession) -> mpsc::UnboundedReceiver<FromHost> {
    watch_after(hub, session, None)
}

fn watch_after(
    hub: &Arc<Mutex<Hub>>,
    session: &HostSession,
    after: Option<u64>,
) -> mpsc::UnboundedReceiver<FromHost> {
    let (sink, lines) = mpsc::unbounded_channel();
    Hub::watch(hub, session, after, sink).unwrap();
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
            // Each watch's replay starts with one.
            FromHost::Replay { .. } => {}
            FromHost::Answer { .. }
            | FromHost::Kicked
            | FromHost::TermOutput { .. }
            | FromHost::TermExit { .. } => panic!("a sink only carries entries"),
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

/// What a journal read holds, without the times.
fn untimed(read: Vec<(u64, u64, Entry)>) -> Vec<(u64, Entry)> {
    read.into_iter()
        .map(|(seq, _, entry)| (seq, entry))
        .collect()
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
    let dir = hub
        .lock()
        .unwrap()
        .dir
        .join("sessions")
        .join(&fake.session.0);
    let journal = journal::read(&dir).unwrap();
    // In the order the laptop saw them live: its prompt, then the notice.
    assert_eq!(
        kinds(&untimed(journal.clone())),
        ["ready", "request:Prompt", "event:Notice"]
    );
    let Entry::Event(AcpEvent::Notice(text)) = &journal[2].2 else {
        panic!("the third entry is not the notice");
    };
    assert_eq!(
        text,
        "Attachments are not sent to a host yet; the prompt went without them"
    );
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
    let sessions = dir.join("sessions").join(&fake.session.0);
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
    j.append(
        0,
        &Entry::Ready(Readiness {
            resumed: false,
            forkable: false,
        }),
    )
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

#[tokio::test]
async fn a_watch_marks_its_replay_even_when_the_journal_is_empty() {
    let (hub, opened) = hub("marked");
    let fake = open(&hub, &opened, false);
    let mut lines = watch(&hub, &fake.session);
    assert!(matches!(
        lines.try_recv(),
        Ok(FromHost::Replay { session, .. }) if session == fake.session
    ));
    assert!(lines.try_recv().is_err());
}

#[tokio::test]
async fn what_lands_while_a_replay_is_read_is_sent_once() {
    let (hub, opened) = hub("relock");
    let fake = open(&hub, &opened, true);
    fake.push(chunk("a"));
    settle().await;
    // The steps of a watch, with an entry landing between the read, which
    // runs with the hub unlocked, and the replay.
    let dir = hub.lock().unwrap().unwatch(&fake.session).unwrap();
    let (read, mark) = journal::read_from(&dir, journal::Mark::default()).unwrap();
    fake.push(chunk("b"));
    settle().await;
    let (sink, mut lines) = mpsc::unbounded_channel();
    hub.lock()
        .unwrap()
        .replay(&fake.session, None, sink, read, mark)
        .unwrap();
    fake.push(chunk("c"));
    let got = entries(&mut lines, 4).await;
    assert_eq!(
        got.iter().map(|(s, _)| *s).collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    settle().await;
    assert!(lines.try_recv().is_err(), "an entry was sent twice");
}

#[tokio::test]
async fn a_request_the_agent_cannot_take_is_not_journaled() {
    let (hub, opened) = hub("unsent");
    let Fake {
        session,
        events,
        requests,
    } = open(&hub, &opened, true);
    settle().await;
    // The agent's side went away, though its stream has not ended yet.
    drop(requests);
    let refused = hub.lock().unwrap().request(&session, prompt("go"));
    assert_eq!(refused, Err("this session has ended".to_string()));
    let dir = hub.lock().unwrap().dir.join("sessions").join(&session.0);
    assert_eq!(kinds(&untimed(journal::read(&dir).unwrap())), ["ready"]);
    assert!(!hub.lock().unwrap().list()[0].busy);
    drop(events);
}

#[tokio::test]
async fn recorded_entries_carry_their_time() {
    let (hub, opened) = hub("time");
    let before = unix_secs();
    let fake = open(&hub, &opened, true);
    settle().await;
    let mut lines = watch(&hub, &fake.session);
    fake.push(chunk("hi"));
    settle().await;
    let after = unix_secs();
    let mut times = Vec::new();
    while let Ok(line) = lines.try_recv() {
        match line {
            FromHost::Entry { at, .. } => times.push(at),
            FromHost::Replay { .. } => {}
            FromHost::Answer { .. }
            | FromHost::Kicked
            | FromHost::TermOutput { .. }
            | FromHost::TermExit { .. } => panic!("a sink only carries entries"),
        }
    }
    // The replayed Ready, then the live chunk.
    assert_eq!(times.len(), 2);
    assert!(
        times.iter().all(|at| (before..=after).contains(at)),
        "{times:?}"
    );
    let dir = hub
        .lock()
        .unwrap()
        .dir
        .join("sessions")
        .join(&fake.session.0);
    let journaled: Vec<u64> = journal::read(&dir)
        .unwrap()
        .iter()
        .map(|(_, at, _)| *at)
        .collect();
    assert_eq!(journaled, times);
}

/// The `after` a watch's `Replay` says, then the seqs of every entry sent.
fn replayed(lines: &mut mpsc::UnboundedReceiver<FromHost>) -> (Option<u64>, Vec<u64>) {
    let Ok(FromHost::Replay { after, .. }) = lines.try_recv() else {
        panic!("no Replay first");
    };
    let mut seqs = Vec::new();
    while let Ok(FromHost::Entry { seq, .. }) = lines.try_recv() {
        seqs.push(seq);
    }
    (after, seqs)
}

#[tokio::test]
async fn watch_after_replays_only_later_seqs() {
    let (hub, opened) = hub("after");
    let fake = open(&hub, &opened, true);
    fake.push(chunk("a"));
    fake.push(chunk("b"));
    settle().await;
    let mut lines = watch_after(&hub, &fake.session, Some(1));
    assert_eq!(replayed(&mut lines), (Some(1), vec![2]));
}

#[tokio::test]
async fn watch_after_past_the_end_replays_all_and_says_none() {
    let (hub, opened) = hub("past");
    let fake = open(&hub, &opened, true);
    fake.push(chunk("a"));
    settle().await;
    // Seq 2 is not in a journal of two lines.
    let mut lines = watch_after(&hub, &fake.session, Some(2));
    assert_eq!(replayed(&mut lines), (None, vec![0, 1]));
}

fn asking(rpc: u64) -> AcpEvent {
    let option = |id: &str, kind: &str| onehand_core::acp::PermissionOption {
        id: id.into(),
        name: id.into(),
        kind: kind.into(),
    };
    AcpEvent::Permission(PermissionRequest {
        rpc_id: json!(rpc),
        tool_call_id: None,
        kind: onehand_core::acp::ToolKind::Other,
        title: "run it".into(),
        options: vec![option("always", "allow_always"), option("once", "allow_once")],
        child: None,
    })
}

fn allowed(request: AcpRequest, rpc: u64) {
    let AcpRequest::PermissionResponse { rpc_id, option_id } = request else {
        panic!("the agent got something other than an answer");
    };
    assert_eq!(rpc_id, json!(rpc));
    assert_eq!(option_id.as_deref(), Some("once"));
}

fn last_line(hub: &Arc<Mutex<Hub>>, session: &HostSession) -> Entry {
    let dir = lock(hub).sessions[session].dir.clone();
    let (read, _) = journal::read_from(&dir, journal::Mark::default()).unwrap();
    read.into_iter().last().unwrap().2
}

#[tokio::test]
async fn a_card_is_answered_while_the_switch_is_on() {
    let (hub, opened) = hub("bypass-on");
    let mut fake = open(&hub, &opened, true);
    let mut lines = watch(&hub, &fake.session);
    fake.push(asking(7));
    allowed(fake.request().await, 7);
    let live = entries(&mut lines, 2).await;
    assert!(matches!(
        &live[1].1,
        Entry::Answered {
            card: Some(AcpEvent::Permission(_)),
            ..
        }
    ));
    assert!(matches!(
        last_line(&hub, &fake.session),
        Entry::Answered { card: Some(_), .. }
    ));
}

#[tokio::test]
async fn a_card_waits_while_the_switch_is_off() {
    let (hub, opened) = hub("bypass-off");
    lock(&hub).set_bypass("/tmp".into(), false).unwrap();
    let mut fake = open(&hub, &opened, true);
    fake.push(asking(7));
    settle().await;
    assert!(fake.requests.try_recv().is_err());
    assert!(matches!(
        last_line(&hub, &fake.session),
        Entry::Event(AcpEvent::Permission(_))
    ));
}

#[tokio::test]
async fn switching_on_answers_cards_already_waiting() {
    let (hub, opened) = hub("bypass-later");
    lock(&hub).set_bypass("/tmp".into(), false).unwrap();
    let mut fake = open(&hub, &opened, true);
    fake.push(asking(7));
    settle().await;
    assert!(lock(&hub).set_bypass("/tmp".into(), true).unwrap());
    allowed(fake.request().await, 7);
    assert!(matches!(
        last_line(&hub, &fake.session),
        Entry::Answered { card: None, .. }
    ));
}

#[tokio::test]
async fn a_card_answered_by_the_person_is_not_answered_again() {
    let (hub, opened) = hub("bypass-twice");
    lock(&hub).set_bypass("/tmp".into(), false).unwrap();
    let mut fake = open(&hub, &opened, true);
    fake.push(asking(7));
    settle().await;
    let answer = AcpRequest::PermissionResponse {
        rpc_id: json!(7),
        option_id: Some("once".into()),
    };
    lock(&hub).request(&fake.session, answer.clone()).unwrap();
    lock(&hub).set_bypass("/tmp".into(), true).unwrap();
    // A second answer from the person, after the card has one, goes nowhere.
    lock(&hub).request(&fake.session, answer).unwrap();
    allowed(fake.request().await, 7);
    settle().await;
    assert!(fake.requests.try_recv().is_err(), "one answer only");
}

#[tokio::test]
async fn the_switch_survives_a_restart() {
    let dir = scratch("bypass-restart");
    {
        let (hub, _) = hub_on(dir.clone());
        lock(&hub).set_bypass("/srv/app".into(), false).unwrap();
    }
    let (hub, _) = hub_on(dir);
    assert!(!lock(&hub).bypass(std::path::Path::new("/srv/app")));
    assert!(lock(&hub).bypass(std::path::Path::new("/srv/other")));
}
