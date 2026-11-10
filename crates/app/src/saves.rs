//! A change to `onehand.toml` shown at once and written after it: off the UI
//! thread, in the order the changes were made, and put back to its last value
//! known saved if its write fails while no later write carries it, said once.
//!
//! The ordering and the rule are core's ([`onehand_core::saving`]), and for
//! the agents, the appearance and outside control the settings' drive's
//! (`onehand_core::config::drive`); what is here is the spawn and the toast.

use crate::state::Shared;
use gpui::{App, BorrowAppContext as _, Window};
use gpui_component::WindowExt as _;
use onehand_core::config::AppConfig;
use onehand_core::config::drive::{self, Effect, Input, Signal, Value};
use std::rc::Rc;

/// What puts one setting's value back on screen.
pub(crate) type Put = Rc<dyn Fn(&mut Window, &mut App)>;

/// A change a failed write puts back: the value it replaced, the value it
/// shows, and what puts either on screen.
pub(crate) struct Change<V> {
    pub before: V,
    pub after: V,
    pub put: fn(V, &mut Window, &mut App),
}

/// Write `edit` into `onehand.toml` after every write before it. `kind` names
/// the setting it edits, each put back on its own; `doing` names it in the
/// toast. A `change` of `None` is never put back, and its failure is still
/// said once.
pub(crate) fn write_config<V: Clone + 'static>(
    kind: &'static str,
    doing: &'static str,
    edit: impl FnOnce(&mut AppConfig) + Send + 'static,
    change: Option<Change<V>>,
    window: &mut Window,
    cx: &mut App,
) {
    let path = Shared::global(cx).config_path.clone();
    let (before, after): (Put, Put) = match change {
        Some(change) => {
            let (before, after, put) = (change.before, change.after, change.put);
            (
                Rc::new(move |window, cx| put(before.clone(), window, cx)),
                Rc::new(move |window, cx| put(after.clone(), window, cx)),
            )
        }
        None => (Rc::new(|_, _| {}), Rc::new(|_, _| {})),
    };
    let pending = cx.update_global::<Shared, _>(|shared, _| {
        shared.config_saves.change(kind, before, after);
        shared.config_saves.write_of([kind])
    });
    let written = Shared::global(cx)
        .writer
        .write(move || AppConfig::update_in_place(&path, edit));
    window
        .spawn(cx, async move |cx| {
            let result = written.await;
            let _ = cx.update(|window, cx| {
                let said = cx.update_global::<Shared, _>(|shared, _| {
                    shared.config_saves.landed(pending, result)
                });
                let Some((why, back)) = said else {
                    return;
                };
                for put in back {
                    put(window, cx);
                }
                window.push_notification(crate::toast::failed(doing, why), cx);
            });
        })
        .detach();
}

/// Carry out what the settings' drive answers, in order, each once.
///
/// A check's agent is held here until it comes up, fails or is called off:
/// dropping its events is what kills it.
pub(crate) fn carry_settings(fx: Vec<Effect>, window: &mut Window, cx: &mut App) {
    for effect in fx {
        match effect {
            Effect::Show(Value::Appearance(choice)) => {
                crate::shell::apply_appearance(choice, Some(&mut *window), cx)
            }
            Effect::Show(value @ Value::Outside(_)) => {
                onehand_core::control::link::shown(&mut crate::control::drive(), &value);
                cx.refresh_windows();
            }
            Effect::Show(Value::Agents) => {}
            Effect::Write { pending, edit } => {
                let path = Shared::global(cx).config_path.clone();
                let write = edit.clone();
                let written = Shared::global(cx)
                    .writer
                    .write(move || AppConfig::update_in_place(&path, |cfg| write.apply(cfg)));
                window
                    .spawn(cx, async move |cx| {
                        let result = written.await;
                        let _ = cx.update(|window, cx| {
                            let fx = cx.update_global::<Shared, _>(|shared, _| {
                                shared.settings.handle(Input::Written {
                                    pending,
                                    edit,
                                    result,
                                })
                            });
                            carry_settings(fx, window, cx);
                        });
                    })
                    .detach();
            }
            Effect::Probe { spec, cwd, token } => probe(spec, cwd, token, cx),
            Effect::CallOff { token } => {
                cx.default_global::<Checks>().0.remove(&token);
            }
            Effect::Changed => cx.refresh_windows(),
            Effect::Signal(Signal::SaveFailed { doing, why }) => {
                window.push_notification(crate::toast::failed(doing, why), cx)
            }
        }
    }
}

/// The checks under way, by token: dropping one kills its agent.
#[derive(Default)]
struct Checks(std::collections::HashMap<u64, gpui::Task<()>>);

impl gpui::Global for Checks {}

/// Bring `spec` up for a look at what it offers, and report what it found,
/// or that its wait ran out.
fn probe(spec: onehand_core::config::AgentSpec, cwd: std::path::PathBuf, token: u64, cx: &mut App) {
    use futures::StreamExt as _;
    let mut events = Shared::global(cx).acp.probe(&spec, cwd);
    let read = cx.spawn(async move |cx| {
        let mut offer = onehand_core::workflow::Offer::default();
        let mut found = Err(drive::CLOSED_EARLY.to_string());
        while let Some(event) = events.next().await {
            if let Some(ended) = drive::fold_probe(&mut offer, event) {
                found = ended.map(|()| std::mem::take(&mut offer));
                break;
            }
        }
        // The events go with this task, which kills the adapter.
        drop(events);
        cx.update(|cx| land(Input::Probed { token, found }, cx));
    });
    cx.default_global::<Checks>().0.insert(token, read);
    cx.spawn(async move |cx| {
        cx.background_executor().timer(drive::CHECK_WAIT).await;
        cx.update(|cx| land(Input::Elapsed { token }, cx));
    })
    .detach();
}

/// A check's answer, with no window at hand: what it says needs none.
fn land(input: Input, cx: &mut App) {
    let fx = cx.update_global::<Shared, _>(|shared, _| shared.settings.handle(input));
    for effect in fx {
        match effect {
            Effect::CallOff { token } => {
                cx.default_global::<Checks>().0.remove(&token);
            }
            Effect::Changed => cx.refresh_windows(),
            // A check's answer asks for nothing else.
            Effect::Show(_) | Effect::Write { .. } | Effect::Probe { .. } | Effect::Signal(_) => {}
        }
    }
}

#[cfg(test)]
mod tests;
