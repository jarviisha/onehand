//! The pane's half of a session's carrier against its contract: the code is
//! sent once for a prompt that waits on it, the prompt goes again from the
//! composer once the code is up and only then, a send that fails reaches the
//! session's drive, and an answer that comes after a newer send was asked
//! for sends nothing. The send itself is stood in for: a real one attaches
//! to a host.

use super::ChatPane;
use crate::chat::composer::Composer;
use crate::chat::session::ChatSession;
use crate::chat::session::tests::carrier::{self, Seen};
use futures::channel::{mpsc, oneshot};
use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use gpui_component::Root;
use onehand_core::acp::{AcpEvent, AcpRequest};
use onehand_core::chat::drive::{Code, Intake, Origin, Signal};
use onehand_core::chat::{Chat, QueuedPrompt};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::Path;

type Sent = Result<(), String>;

thread_local! {
    /// The sends asked for and not yet answered, oldest first.
    static ASKED: RefCell<VecDeque<oneshot::Sender<Sent>>> = RefCell::default();
    /// Whether the code is no longer this machine's to send.
    static GONE: Cell<bool> = const { Cell::new(false) };
}

/// Stands in for the offloaded projects' send: asked, and answered when the
/// test says how it went, or refused as a send of code no longer here is.
pub(super) fn send_up(_: &mut gpui::App, _: &Path) -> Option<impl Future<Output = Sent> + use<>> {
    if GONE.get() {
        return None;
    }
    let (tx, rx) = oneshot::channel();
    ASKED.with_borrow_mut(|asked| asked.push_back(tx));
    Some(async move { rx.await.unwrap_or_else(|_| Err("dropped".into())) })
}

fn asked() -> usize {
    ASKED.with_borrow(VecDeque::len)
}

/// The oldest send asked for comes to `sent`.
fn answer(sent: Sent, cx: &mut VisualTestContext) {
    let tx = ASKED.with_borrow_mut(VecDeque::pop_front).expect("a send");
    tx.send(sent).unwrap();
    cx.run_until_parked();
}

struct Rig {
    session: Entity<ChatSession>,
    composer: Entity<Composer>,
    seen: Seen,
    requests: tokio::sync::mpsc::UnboundedReceiver<AcpRequest>,
    _tx: mpsc::Sender<AcpEvent>,
}

/// A session whose agent is up, and a composer in a window of its own.
fn rig(cx: &mut TestAppContext) -> (Rig, &mut VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(gpui_component::init);
    ASKED.with_borrow_mut(VecDeque::clear);
    GONE.set(false);
    let (session, mut tx, seen) = carrier::driven(cx, Chat::default());
    let requests = carrier::up(&mut tx);
    let mut made = None;
    let window = cx.add_window(|window, cx| {
        let composer = cx.new(|cx| Composer::new(window, cx));
        made = Some(composer.clone());
        Root::new(composer, window, cx)
    });
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.run_until_parked();
    let rig = Rig {
        session,
        composer: made.expect("the composer"),
        seen,
        requests,
        _tx: tx,
    };
    (rig, cx)
}

impl Rig {
    /// Put `text` in the composer.
    fn write(&self, text: &str, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            self.composer.update(cx, |composer, cx| {
                composer.clear(window, cx);
                let queued = QueuedPrompt {
                    text: text.into(),
                    attachments: Vec::new(),
                };
                composer.restore_queued(queued, window, cx);
            })
        });
    }

    /// A person's prompt of what the composer holds, for a project whose
    /// code has to go up first, carried as the pane carries it.
    fn prompt_code_first(&self, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let text = self.composer.read(cx).text(cx);
            let code = Code::MustGo {
                line: "Sending the code".into(),
            };
            let (intake, left) = self.session.update(cx, |session, cx| {
                session.prompt(&text, &[], Origin::Person, code, cx)
            });
            assert_eq!(intake, Intake::CodeFirst);
            ChatPane::carry(&self.session, &self.composer, text, left, window, cx);
        });
        cx.run_until_parked();
    }

    /// How many prompts reached the agent since last asked.
    fn prompts(&mut self) -> usize {
        std::iter::from_fn(|| self.requests.try_recv().ok())
            .filter(|request| matches!(request, AcpRequest::Prompt { .. }))
            .count()
    }

    fn written(&self, cx: &mut VisualTestContext) -> String {
        cx.update(|_, cx| self.composer.read(cx).text(cx))
    }

    /// How many times the session said a send of its code failed.
    fn failures(&self) -> usize {
        let seen = self.seen.borrow();
        seen.iter()
            .filter(|signal| matches!(signal, Signal::CodeFailed(_)))
            .count()
    }
}

/// The code is asked for once, and the prompt goes once it is up: once, from
/// the composer, which is cleared as for any prompt sent.
#[gpui::test]
fn the_prompt_goes_once_after_its_code(cx: &mut TestAppContext) {
    let (mut rig, cx) = rig(cx);
    rig.write("go", cx);
    rig.prompt_code_first(cx);
    assert_eq!(asked(), 1);
    assert_eq!(rig.prompts(), 0, "nothing goes ahead of the code");
    assert_eq!(
        rig.written(cx),
        "go",
        "the prompt waits where it was written"
    );
    answer(Ok(()), cx);
    assert_eq!(rig.prompts(), 1);
    assert_eq!(rig.written(cx), "");
    assert_eq!((asked(), rig.failures()), (0, 0));
}

/// A send that fails reaches the session's drive, which says so once; the
/// prompt is not sent and stays in the composer.
#[gpui::test]
fn a_failed_send_is_said_and_keeps_the_prompt(cx: &mut TestAppContext) {
    let (mut rig, cx) = rig(cx);
    rig.write("go", cx);
    rig.prompt_code_first(cx);
    answer(Err("the host is out of reach".into()), cx);
    assert_eq!(rig.failures(), 1);
    assert_eq!(rig.prompts(), 0);
    assert_eq!(rig.written(cx), "go");
}

/// A prompt rewritten while its code went up is not sent in the words it no
/// longer has: the code is up, and what is written now waits to be sent.
#[gpui::test]
fn a_prompt_rewritten_meanwhile_is_not_sent(cx: &mut TestAppContext) {
    let (mut rig, cx) = rig(cx);
    rig.write("go", cx);
    rig.prompt_code_first(cx);
    rig.write("go on", cx);
    answer(Ok(()), cx);
    assert_eq!(rig.prompts(), 0);
    assert_eq!(rig.written(cx), "go on");
    assert_eq!(rig.failures(), 0);
}

/// The answer to a send asked for before a newer one is late: it sends no
/// prompt and says no failure, whichever way it went. The newer send's
/// answer is the one the prompt goes on.
#[gpui::test]
fn a_late_answer_sends_nothing(cx: &mut TestAppContext) {
    let (mut rig, cx) = rig(cx);
    rig.write("go", cx);
    rig.prompt_code_first(cx);
    rig.prompt_code_first(cx);
    assert_eq!(asked(), 2);
    answer(Err("the first send was cut".into()), cx);
    assert_eq!((rig.prompts(), rig.failures()), (0, 0));
    assert_eq!(rig.written(cx), "go");
    answer(Ok(()), cx);
    assert_eq!((rig.prompts(), rig.failures()), (1, 0));
    assert_eq!(rig.written(cx), "");
}

/// Code that is no longer this machine's to send is a send that failed: said
/// once, and the prompt stays unsent in the composer.
#[gpui::test]
fn code_no_longer_here_to_send_is_a_failed_send(cx: &mut TestAppContext) {
    let (mut rig, cx) = rig(cx);
    GONE.set(true);
    rig.write("go", cx);
    rig.prompt_code_first(cx);
    assert_eq!((asked(), rig.failures()), (0, 1));
    assert_eq!(rig.prompts(), 0);
    assert_eq!(rig.written(cx), "go");
}
