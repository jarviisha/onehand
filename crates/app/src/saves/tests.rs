//! The settings' carrier against its contract: a write goes once through the
//! one writer, a failed one reaches the drive and puts the setting back, the
//! agent list is kept, and an agent check's answer, or its wait running out,
//! lands once.

use super::*;
use gpui::{AppContext as _, TestAppContext, VisualTestContext};
use gpui_component::Root;
use onehand_core::config::{AgentEdit, AgentSpec, Appearance};
use std::path::PathBuf;
use std::time::Duration;

struct Blank;

impl gpui::Render for Blank {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        gpui::div()
    }
}

/// The process's state writing its config to `path`, and a window to say
/// things in.
fn app(cx: &mut TestAppContext, path: PathBuf) -> &mut VisualTestContext {
    cx.executor().allow_parking();
    cx.update(gpui_component::init);
    cx.update(|cx| {
        let mut shared = Shared::for_test();
        shared.config_path = path;
        cx.set_global(shared);
    });
    let window = cx.add_window(|window, cx| Root::new(cx.new(|_| Blank), window, cx));
    VisualTestContext::from_window(window.into(), cx).into_mut()
}

/// Let the writer's thread land what it was given, and its answer come back.
fn settle(cx: &mut VisualTestContext) {
    let writer = cx.update(|_, cx| Shared::global(cx).writer.clone());
    for _ in 0..10 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(writer.flush(Duration::from_secs(5)));
    cx.run_until_parked();
}

/// A path no write can land at: under a file.
fn unwritable(name: &str) -> PathBuf {
    let file = std::env::temp_dir().join(format!("onehand-saves-{name}-{}", std::process::id()));
    std::fs::write(&file, b"").unwrap();
    file.join("onehand.toml")
}

#[gpui::test]
fn a_written_appearance_lands_in_the_file(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("onehand-saves-ok-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("onehand.toml");
    let _ = std::fs::remove_file(&path);
    let cx = app(cx, path.clone());
    cx.update(|window, cx| {
        let fx = cx.update_global::<Shared, _>(|s, _| s.settings.set_appearance(Appearance::Dark));
        carry_settings(fx, window, cx);
    });
    settle(cx);
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("dark"), "{written}");
    assert_eq!(
        cx.update(|_, cx| Shared::global(cx).settings.appearance),
        Appearance::Dark
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui::test]
fn a_failed_appearance_write_is_put_back(cx: &mut TestAppContext) {
    let cx = app(cx, unwritable("appearance"));
    cx.update(|window, cx| {
        let fx = cx.update_global::<Shared, _>(|s, _| s.settings.set_appearance(Appearance::Dark));
        carry_settings(fx, window, cx);
    });
    assert_eq!(
        cx.update(|_, cx| Shared::global(cx).settings.appearance),
        Appearance::Dark,
        "shown at once"
    );
    settle(cx);
    assert_eq!(
        cx.update(|_, cx| Shared::global(cx).settings.appearance),
        Appearance::System,
        "put back"
    );
}

#[gpui::test]
fn a_failed_agents_write_keeps_the_list(cx: &mut TestAppContext) {
    let cx = app(cx, unwritable("agents"));
    let new = AgentSpec {
        name: "extra".into(),
        ..AgentSpec::default()
    };
    let before = cx.update(|_, cx| Shared::global(cx).settings.agents.len());
    cx.update(|window, cx| {
        let edit = AgentEdit::Put { old: None, new };
        let fx = cx.update_global::<Shared, _>(|s, _| s.settings.edit_agents(edit));
        carry_settings(fx, window, cx);
    });
    settle(cx);
    assert_eq!(
        cx.update(|_, cx| Shared::global(cx).settings.agents.len()),
        before + 1
    );
}

/// Start a check of `spec` through the drive, its events read off the
/// returned channel rather than an agent's.
fn probing(
    spec: &AgentSpec,
    cx: &mut VisualTestContext,
) -> futures::channel::mpsc::Sender<onehand_core::acp::AcpEvent> {
    let (tx, rx) = futures::channel::mpsc::channel(8);
    cx.update(|_, cx| {
        let fx = cx.update_global::<Shared, _>(|shared, _| {
            shared.settings.check(spec.clone(), PathBuf::from("/tmp"))
        });
        let token = fx
            .iter()
            .find_map(|effect| match effect {
                Effect::Probe { token, .. } => Some(*token),
                Effect::Show(_)
                | Effect::Write { .. }
                | Effect::Look { .. }
                | Effect::CallOff { .. }
                | Effect::Changed
                | Effect::Signal(_) => None,
            })
            .expect("a probe");
        read_probe(onehand_io::acp::Events::from_channel(rx), token, cx);
    });
    tx
}

fn checking(spec: &AgentSpec, cx: &mut VisualTestContext) -> Option<drive::Checking> {
    cx.update(|_, cx| Shared::global(cx).settings.checking(spec).cloned())
}

/// The agent's answer lands once: what it offers is seen, the check is over
/// and its events let go; the wait running out after it changes nothing.
#[gpui::test]
fn a_probes_answer_lands_once(cx: &mut TestAppContext) {
    use onehand_core::acp::{AcpEvent, Mode};
    let cx = app(cx, std::env::temp_dir().join("onehand-saves-probe.toml"));
    let spec = AgentSpec {
        name: "probed".into(),
        ..Default::default()
    };
    let mut tx = probing(&spec, cx);
    assert_eq!(checking(&spec, cx), Some(drive::Checking::Running));
    let mode = Mode {
        id: "code".into(),
        name: "Code".into(),
    };
    tx.try_send(AcpEvent::Modes {
        current: None,
        available: vec![mode],
    })
    .unwrap();
    let (req, _requests) = tokio::sync::mpsc::unbounded_channel();
    tx.try_send(AcpEvent::Connected {
        tx: req,
        resumed: false,
        forkable: false,
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(checking(&spec, cx), None);
    let seen = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            Shared::global(cx)
                .settings
                .offers_seen
                .iter()
                .filter(|(seen, _)| *seen == spec)
                .map(|(_, offer)| offer.modes.clone())
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(seen(cx), vec![vec!["code".to_string()]]);
    assert!(tx.is_closed(), "its events let go");
    cx.executor().advance_clock(drive::CHECK_WAIT);
    cx.run_until_parked();
    assert_eq!(checking(&spec, cx), None, "the wait lands on nothing");
    assert_eq!(seen(cx).len(), 1);
}

/// An agent that never comes up fails the check once its wait runs out,
/// and its read is dropped.
#[gpui::test]
fn a_probe_that_never_answers_fails_after_its_wait(cx: &mut TestAppContext) {
    let cx = app(
        cx,
        std::env::temp_dir().join("onehand-saves-probe-wait.toml"),
    );
    let spec = AgentSpec {
        name: "silent".into(),
        ..Default::default()
    };
    let tx = probing(&spec, cx);
    cx.run_until_parked();
    assert_eq!(checking(&spec, cx), Some(drive::Checking::Running));
    cx.executor().advance_clock(drive::CHECK_WAIT);
    cx.run_until_parked();
    match checking(&spec, cx) {
        Some(drive::Checking::Failed(why)) => assert!(why.contains("did not come up"), "{why}"),
        still @ (Some(drive::Checking::Running) | None) => panic!("{still:?}"),
    }
    assert!(cx.update(|_, cx| cx.default_global::<Checks>().0.is_empty()));
    assert!(tx.is_closed(), "its read dropped");
}
