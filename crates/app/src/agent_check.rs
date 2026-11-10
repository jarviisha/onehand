//! *Check the agent*: start an agent with no session, read what it offers,
//! and close it, so a start whose settings are not known yet can be judged
//! before it is made.
//!
//! **Never run unasked.** It is the one check that costs an agent start, so
//! nothing but a press of its button starts one; opening a form never does.
//! Nothing is prompted: the adapter is brought up as far as `session/new`,
//! whose answer carries the modes and config options, and dropped at once,
//! which kills it. What it learns goes where any sighting of the agent's
//! offer goes (`SettingsDrive::saw_offer`), so every later start in this process
//! reads it.

use crate::controls::Short as _;
use crate::state::Shared;
use gpui::BorrowAppContext as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{AnyElement, App, IntoElement, ParentElement, Styled, Window, div};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme as _, StyledExt as _};
use onehand_core::config::{self, AgentSpec};
use std::path::{Path, PathBuf};

pub(crate) use onehand_core::config::drive::Checking;

/// What a *Check the agent* button checks: the agent a start would run, in
/// the project it would start in, for the mode it would start in.
#[derive(Clone, Debug)]
pub(crate) struct Ask {
    spec: AgentSpec,
    cwd: PathBuf,
    mode: Option<String>,
}

impl Ask {
    /// The configured agent named `agent` (the first configured for none),
    /// started in `cwd`, judged for `mode`; `None` when no agent is
    /// configured at all.
    pub(crate) fn new(
        agent: Option<&str>,
        cwd: &Path,
        mode: Option<&str>,
        cx: &App,
    ) -> Option<Self> {
        let spec = config::agent_or_first(&Shared::global(cx).settings.agents, agent)?;
        Some(Self {
            spec: spec.clone(),
            cwd: cwd.to_path_buf(),
            mode: mode.and_then(config::mode_set),
        })
    }

    /// The agent and mode the preflight judged in `facts`, started in `cwd`.
    pub(crate) fn of(facts: &onehand_core::preflight::Facts, cwd: &Path, cx: &App) -> Option<Self> {
        Self::new(facts.agent.as_deref(), cwd, facts.mode.as_deref(), cx)
    }
}

/// Start `ask`'s agent, read what it offers, and close it.
fn check(ask: &Ask, window: &mut Window, cx: &mut App) {
    let (spec, cwd) = (ask.spec.clone(), ask.cwd.clone());
    let fx = cx.update_global::<Shared, _>(|shared, _| shared.settings.check(spec, cwd));
    crate::saves::carry_settings(fx, window, cx);
}

/// The check of `spec` under way, or its last failure.
fn checking(spec: &AgentSpec, cx: &App) -> Option<Checking> {
    Shared::global(cx).settings.checking(spec).cloned()
}

/// The button, drawn beside a mode not known yet: *Check the agent*, then
/// *Checking…*, then what the check found where the finding beside it is not
/// judged again, or why it failed.
pub(crate) fn button(id: usize, ask: &Ask, cx: &App) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let state = checking(&ask.spec, cx);
    let learned = crate::unattended::offered(&ask.spec, cx);
    let said = match &state {
        Some(Checking::Failed(why)) => {
            Some(format!("{} could not be checked: {why}", ask.spec.name))
        }
        Some(Checking::Running) | None => learned.zip(ask.mode.as_ref()).map(|(offered, mode)| {
            onehand_core::preflight::mode_refused(mode, &offered.modes)
                .unwrap_or_else(|| format!("{} offers `{mode}`.", ask.spec.name))
        }),
    };
    let running = state == Some(Checking::Running);
    let ask = ask.clone();
    div()
        .h_flex()
        .gap_2()
        .items_center()
        .when_some(said, |row, said| {
            row.child(div().text_xs().text_color(muted).child(said))
        })
        .child(
            // Filled: beside a sentence, a ghost reads as more of the text.
            crate::controls::action(("check-agent-modes", id))
                .secondary()
                .short()
                .loading(running)
                .label("Check the agent")
                .on_click(move |_, window: &mut Window, cx: &mut App| check(&ask, window, cx)),
        )
        .into_any_element()
}
