//! *Check the agent*: start an agent with no session, read what it offers,
//! and close it, so a start whose settings are not known yet can be judged
//! before it is made.
//!
//! **Never run unasked.** It is the one check that costs an agent start, so
//! nothing but a press of its button starts one; opening a form never does.
//! Nothing is prompted: the adapter is brought up as far as `session/new`,
//! whose answer carries the modes and config options, and dropped at once,
//! which kills it. What it learns goes where any sighting of the agent's
//! offer goes (`Shared::saw_offer`), so every later start in this process
//! reads it.

use crate::controls::Short as _;
use crate::state::Shared;
use futures::StreamExt as _;
use gpui::BorrowAppContext as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{AnyElement, App, IntoElement, ParentElement, Styled, Window, div};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme as _, StyledExt as _};
use onehand_core::acp::AcpEvent;
use onehand_core::config::{self, AgentSpec};
use onehand_core::workflow::Offer;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long an agent is given to come up before the check gives up on it.
const CHECK_WAIT: Duration = Duration::from_secs(90);

/// A check under way, or the last one that failed, per spec.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Checking {
    Running,
    Failed(String),
}

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
        let spec = config::agent_or_first(&Shared::global(cx).agents, agent)?;
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
fn check(ask: &Ask, cx: &mut App) {
    if checking(&ask.spec, cx) == Some(Checking::Running) {
        return;
    }
    set(&ask.spec, Some(Checking::Running), cx);
    let mut events = Shared::global(cx).acp.probe(&ask.spec, ask.cwd.clone());
    let spec = ask.spec.clone();
    let timer = cx.background_executor().timer(CHECK_WAIT);
    cx.spawn(async move |cx| {
        let read = async move {
            let mut offer = Offer::default();
            while let Some(event) = events.next().await {
                match event {
                    AcpEvent::Modes { available, .. } => {
                        offer.modes = available.into_iter().map(|m| m.id).collect();
                    }
                    AcpEvent::ConfigOptions(opts) => {
                        offer.options = opts
                            .into_iter()
                            .map(|opt| (opt.id, opt.choices.into_iter().map(|c| c.value).collect()))
                            .collect();
                    }
                    AcpEvent::Connected { .. } => return Ok(offer),
                    AcpEvent::Error { text: why, .. } | AcpEvent::Disconnected(why) => {
                        return Err(why);
                    }
                    // Nothing else is asked of an agent that is never
                    // prompted; whatever it says before it comes up is let go.
                    AcpEvent::SessionId(_)
                    | AcpEvent::AgentChunk(_)
                    | AcpEvent::ThoughtChunk(_)
                    | AcpEvent::UserChunk(_)
                    | AcpEvent::Interrupted
                    | AcpEvent::ToolCall(_)
                    | AcpEvent::ToolUpdate(_)
                    | AcpEvent::Plan(_)
                    | AcpEvent::Usage(_)
                    | AcpEvent::Background(_)
                    | AcpEvent::Permission(_)
                    | AcpEvent::Elicitation(_)
                    | AcpEvent::AvailableCommands(_)
                    | AcpEvent::ModeChanged(_)
                    | AcpEvent::PickConfirmed { .. }
                    | AcpEvent::PickRefused { .. }
                    | AcpEvent::TerminalOutput { .. }
                    | AcpEvent::TerminalExit { .. }
                    | AcpEvent::TurnEnded { .. }
                    // The refusal it comes ahead of ends the check, in words.
                    | AcpEvent::LoginRequired { .. } => {}
                }
            }
            Err("the agent closed before it came up".to_string())
        };
        // Whichever way it ends, the stream is dropped with the read, which
        // kills the adapter.
        let found = match futures::future::select(Box::pin(read), timer).await {
            futures::future::Either::Left((found, _)) => found,
            futures::future::Either::Right(_) => Err(format!(
                "the agent did not come up within {}s",
                CHECK_WAIT.as_secs()
            )),
        };
        cx.update(|cx| {
            match found {
                Ok(offer) => {
                    set(&spec, None, cx);
                    cx.update_global::<Shared, _>(|s, _| s.saw_offer(spec, offer));
                }
                Err(why) => set(&spec, Some(Checking::Failed(why)), cx),
            }
            cx.refresh_windows();
        });
    })
    .detach();
}

/// The check of `spec` under way, or its last failure.
fn checking(spec: &AgentSpec, cx: &App) -> Option<Checking> {
    Shared::global(cx)
        .agent_checks
        .iter()
        .find(|(seen, _)| seen == spec)
        .map(|(_, checking)| checking.clone())
}

fn set(spec: &AgentSpec, to: Option<Checking>, cx: &mut App) {
    cx.update_global::<Shared, _>(|s, _| {
        s.agent_checks.retain(|(seen, _)| seen != spec);
        if let Some(to) = to {
            s.agent_checks.push((spec.clone(), to));
        }
    });
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
                .on_click(move |_, _: &mut Window, cx: &mut App| check(&ask, cx)),
        )
        .into_any_element()
}
