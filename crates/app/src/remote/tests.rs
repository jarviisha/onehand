//! The remote bridge's carrier against its contract: what the drive says goes
//! out once, a session no window holds reaches the drive as gone, a scan's
//! answer lands once off the UI thread, and a channel that ended tells nobody
//! anything more.

use super::*;
use futures::StreamExt as _;
use gpui::TestAppContext;
use onehand_core::chat::Away;
use onehand_core::remote::types::{RemoteChannel, ReqRx};
use std::pin::Pin;
use std::sync::mpsc;
use std::time::Duration;

/// A channel that reports nothing and hands every request to the test.
struct Fake(mpsc::Sender<RemoteRequest>);

impl RemoteChannel for Fake {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn connect(
        self: Box<Self>,
        mut requests: ReqRx,
    ) -> Pin<Box<dyn futures::Stream<Item = RemoteEvent> + Send>> {
        let seen = self.0;
        let read = async move {
            while let Some(request) = requests.recv().await {
                let _ = seen.send(request);
            }
        };
        Box::pin(futures::stream::once(read).filter_map(|()| async { None }))
    }
}

/// The process's state with a bridge for chat `7` on a fake channel, and
/// what the channel is asked.
fn bridge(cx: &mut TestAppContext) -> mpsc::Receiver<RemoteRequest> {
    cx.executor().allow_parking();
    let (tx, rx) = mpsc::channel();
    let (channel, _events) = Channel::start(Box::new(Fake(tx))).unwrap();
    cx.update(|cx| {
        let mut shared = Shared::for_test();
        shared.remote = RemoteBridge {
            drive: RemoteDrive::new(vec!["7".into()]),
            channel: Some(channel),
        };
        cx.set_global(shared);
    });
    rx
}

/// The texts sent within a moment, in order.
fn said(rx: &mpsc::Receiver<RemoteRequest>) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(request) = rx.recv_timeout(Duration::from_millis(200)) {
        if let RemoteRequest::Send(out_) = request {
            out.push(out_.text);
        }
    }
    out
}

fn line(text: &str) -> RemoteEvent {
    RemoteEvent::Message {
        chat: "7".into(),
        from: "7".into(),
        text: text.into(),
    }
}

#[gpui::test]
fn a_line_is_answered_once_and_a_stranger_hears_nothing(cx: &mut TestAppContext) {
    let rx = bridge(cx);
    cx.update(|cx| receive(line("/sessions"), cx));
    cx.update(|cx| {
        receive(
            RemoteEvent::Message {
                chat: "9".into(),
                from: "9".into(),
                text: "/sessions".into(),
            },
            cx,
        )
    });
    assert_eq!(said(&rx), ["No sessions are open."]);
}

/// A prompt for a session no window holds comes back to the drive as gone,
/// and is said once.
#[gpui::test]
fn a_session_no_window_holds_is_said_gone(cx: &mut TestAppContext) {
    let rx = bridge(cx);
    cx.update(|cx| {
        carry(
            vec![Effect::Prompt {
                chat: "7".into(),
                uid: 3,
                text: "go".into(),
            }],
            cx,
        )
    });
    let said = said(&rx);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].starts_with("Session 3 is gone."), "{said:?}");
}

/// `/archive` is scanned off the UI thread and its listing said once.
#[gpui::test]
fn an_archive_scan_lands_once(cx: &mut TestAppContext) {
    let rx = bridge(cx);
    cx.update(|cx| receive(line("/archive"), cx));
    for _ in 0..20 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(said(&rx).len(), 1);
}

/// A channel that ended is let go, away goes with it, and news reaches
/// nobody after.
#[gpui::test]
fn a_channel_that_ended_tells_nobody_more(cx: &mut TestAppContext) {
    let rx = bridge(cx);
    cx.update(|cx| receive(line("/away"), cx));
    assert!(cx.update(|cx| is_away(cx)));
    assert_eq!(said(&rx).len(), 1);
    cx.update(|cx| receive(RemoteEvent::Disconnected("token refused".into()), cx));
    assert!(!cx.update(|cx| is_away(cx)));
    assert!(cx.update(|cx| Shared::global(cx).remote.channel.is_none()));
    let origin = Origin {
        uid: 1,
        agent: "a".into(),
        project: "p".into(),
        conversation: None,
    };
    cx.update(|cx| announce(&origin, Announcement::plain(Away::TurnEnded), cx));
    assert!(said(&rx).is_empty());
}
