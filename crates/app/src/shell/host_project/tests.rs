//! A window's waits on its hosts let go of on a real shell: what waits on a
//! host goes once nothing here waits on it any more, so the host's wait is
//! no longer listened for from this window.

use crate::shell::rig::{folder, shell};
use gpui::{Entity, TestAppContext, VisualTestContext};
use onehand_core::config::AgentSpec;
use onehand_core::host::waits::Want;
use onehand_core::workspace::HostName;

/// An agent that cannot start: its session opens and its stream ends.
fn nowhere() -> AgentSpec {
    AgentSpec {
        name: "nowhere".into(),
        command: "/nonexistent/onehand-test-agent".into(),
        ..Default::default()
    }
}

fn host() -> HostName {
    HostName("box".into())
}

/// A shell with one session shown, its agent gone, and that session told it
/// waits on [`host`]'s busy sessions, as the window is.
fn waiting<'a>(
    name: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<crate::shell::Shell>, u64, &'a mut VisualTestContext) {
    let dir = folder(name);
    let (_, shell, cx) = shell(cx, dir.join("tasks"), vec![nowhere()]);
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.new_session(window, cx)));
    cx.run_until_parked();
    let uid = cx.update(|_, cx| shell.read(cx).active_session_uid().expect("a session"));
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let session = shell.chat.read(cx).session_entity(uid).expect("live");
            session.update(cx, |session, _| {
                let _ = session.chat.wait_on_host(host(), 1, 0);
            });
            shell.host_busy(Some(uid), host(), window, cx);
            assert!(!shell.host_waits.is_empty(), "the session waits");
        })
    });
    assert!(
        !waits_nothing(&shell, cx),
        "still waiting, and listened for"
    );
    (shell, uid, cx)
}

fn waits_nothing(shell: &Entity<crate::shell::Shell>, cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        shell.host_waits.is_empty() && shell.host_listening.is_empty()
    })
}

/// A session closed waits on its host no longer.
#[gpui::test]
fn a_session_closed_waits_no_longer(cx: &mut TestAppContext) {
    let (shell, _, cx) = waiting("waits-closed", cx);
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.close_session(0, 0, window, cx)));
    assert!(waits_nothing(&shell, cx));
}

/// A session restarted is a new attach: what it waited on is over.
#[gpui::test]
fn a_session_restarted_waits_no_longer(cx: &mut TestAppContext) {
    let (shell, uid, cx) = waiting("waits-restarted", cx);
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.restart_now(uid, window, cx)));
    assert!(waits_nothing(&shell, cx));
}

/// The form put away takes its wait with it.
#[gpui::test]
fn the_form_put_away_waits_no_longer(cx: &mut TestAppContext) {
    let (shell, _, cx) = waiting("waits-form", cx);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.close_session(0, 0, window, cx);
            shell.begin_host_project(window, cx);
            let attempt = shell.host_waits.attempt();
            if let Some(draft) = shell.host_project.as_mut() {
                draft.attempt = Some(attempt);
                draft.waiting = Some(host());
            }
            shell.wait_on_host(host(), Want::Add(attempt), window, cx);
            assert!(!shell.host_waits.is_empty(), "the form waits");
        })
    });
    cx.update(|_, cx| shell.update(cx, |shell, cx| shell.cancel_host_project(cx)));
    assert!(waits_nothing(&shell, cx));
}

/// The project page left takes what waited on it with it.
#[gpui::test]
fn a_page_left_waits_no_longer(cx: &mut TestAppContext) {
    let dir = folder("waits-page");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), vec![nowhere()]);
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.select_root(0, window, cx)));
    cx.run_until_parked();
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.host_busy(None, host(), window, cx)));
    assert!(
        !waits_nothing(&shell, cx),
        "the page waits, and is listened for"
    );
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.show_tasks(None, window, cx)));
    assert!(waits_nothing(&shell, cx));
}
