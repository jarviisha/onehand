//! The tools' carrier against its contract: a switch's change reaches each
//! connection once and lets go of the ones gone, what it carries runs after
//! the call that asked, an open's deadline comes round and frees its request,
//! and a token let go leaves nothing listening.

use super::*;
use futures::StreamExt as _;
use gpui::TestAppContext;
use onehand_core::control::drive::{Begun, Open};
use std::time::Duration;

fn token_of(token: &Token) -> String {
    token.0.clone()
}

/// A session's switch tells each of its connections once, after the call
/// that switched it; switching to where it already is tells nobody; a token
/// let go is refused and keeps no connection.
#[gpui::test]
fn a_switch_tells_each_connection_once(cx: &mut TestAppContext) {
    let uid = 900_101;
    let token = Token::mint();
    token.claim(uid);
    let name = token_of(&token);
    let (told, mut heard) = futures::channel::mpsc::unbounded();
    let (gone, _) = futures::channel::mpsc::unbounded::<String>();
    (front().joined)(&name, told);
    (front().joined)(&name, gone);
    let early = cx.update(|cx| {
        switch(uid, true, cx);
        heard.try_recv().is_ok()
    });
    assert!(!early, "carried after the call, not inside it");
    cx.run_until_parked();
    let note = futures::executor::block_on(heard.next()).unwrap();
    assert!(note.contains("list_changed"), "{note}");
    assert_eq!(conns()[&name].len(), 1, "the one gone is let go");
    cx.update(|cx| switch(uid, true, cx));
    cx.run_until_parked();
    assert!(heard.try_recv().is_err());
    assert!(matches!(
        standing(&Peer::Session(name.clone())),
        Standing::On
    ));
    cx.update(|cx| forget(uid, cx));
    drop(token);
    assert!(matches!(
        standing(&Peer::Session(name.clone())),
        Standing::Refused(_)
    ));
    assert!(!conns().contains_key(&name));
}

/// An open's deadline, armed when it is asked, comes round on the clock and
/// fails the open, so the same request opens afresh.
#[gpui::test]
fn an_opens_deadline_comes_round_and_frees_its_request(cx: &mut TestAppContext) {
    let lead = 900_102;
    drive().switch(lead, true);
    let open = || Open {
        lead,
        project: "repo".into(),
        agent: "agent".into(),
        prompt: None,
        dirs: Vec::new(),
        request: "r".into(),
        turn: None,
        model: None,
        name: None,
    };
    let projects = vec![("repo".to_string(), PathBuf::from("/repo"))];
    let Ok(Begun::Resolve { fx, .. }) = drive().open_worker(open(), &projects, None) else {
        panic!("a new request");
    };
    let [Effect::Arm { token, .. }] = fx[..] else {
        panic!("the deadline is armed at the call");
    };
    assert!(matches!(
        drive().open_worker(open(), &projects, None),
        Ok(Begun::Said(_))
    ));
    let wait = Duration::from_secs(1);
    cx.update(|cx| carry(vec![Effect::Arm { token, wait }], cx));
    cx.executor().advance_clock(wait);
    cx.run_until_parked();
    assert!(matches!(
        drive().open_worker(open(), &projects, None),
        Ok(Begun::Resolve { .. })
    ));
    let fx = drive().forget(lead);
    assert!(!fx.is_empty());
}
