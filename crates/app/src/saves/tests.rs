//! The settings' carrier against its contract: a write goes once through the
//! one writer, a failed one reaches the drive and puts the setting back, and
//! the agent list is kept.

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
