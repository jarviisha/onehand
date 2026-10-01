//! The left half of the Issues mode: the search, the filters, a row per issue
//! and the sync footer under them.

use super::{IssuesView, LIST_CAP};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, InteractiveElement as _,
    IntoElement, ParentElement, SharedString, StatefulInteractiveElement as _, Styled, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, Size, StyledExt};
use onehand_core::issues::{self, Issues, LocalIssue};
use onehand_plugin_host::{action, menu_below, menu_item, status_ink, switch};
use std::collections::BTreeSet;

/// Which half of the issues the list shows. Open by default: an open issue is
/// work, a closed one a record.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Showing {
    #[default]
    Open,
    Closed,
}

/// What the list is narrowed to: the search, and a label if one is picked.
/// Whether an issue is open is decided apart, since the counts on the
/// open/closed switch are counts of what this lets through.
struct Narrowing<'a> {
    query: &'a str,
    label: Option<&'a str>,
}

impl Narrowing<'_> {
    /// Whether `issue` gets through. A query starting with `#` is a forge
    /// reference and matches by its start, so `#1` finds `#12` while typing;
    /// anything else is looked for in the title, case aside.
    fn lets_through(&self, issue: &LocalIssue) -> bool {
        let query = self.query.trim();
        let found = if query.is_empty() {
            true
        } else if query.starts_with('#') {
            issue
                .reference()
                .is_some_and(|reference| reference.starts_with(query))
        } else {
            issue.title.to_lowercase().contains(&query.to_lowercase())
        };
        found
            && self
                .label
                .is_none_or(|label| issue.labels.iter().any(|l| l == label))
    }
}

/// The issue a query names outright — `#6` when the forge's `#6` is kept here —
/// which the search jumps to rather than only narrowing the list.
pub(super) fn named<'a>(issues: &'a Issues, query: &str) -> Option<&'a LocalIssue> {
    let query = query.trim();
    query.starts_with('#').then_some(())?;
    issues
        .listed()
        .into_iter()
        .find(|issue| issue.reference() == Some(query))
}

/// The rows the list draws and the counts its switch carries: the issues the
/// narrowing lets through on the side `showing` picks, in the core's order, and
/// how many it lets through open and closed.
fn listed<'a>(
    issues: &'a Issues,
    narrowing: &Narrowing<'_>,
    showing: Showing,
) -> (Vec<&'a LocalIssue>, usize, usize) {
    let through: Vec<&LocalIssue> = issues
        .listed()
        .into_iter()
        .filter(|issue| narrowing.lets_through(issue))
        .collect();
    let open = through.iter().filter(|issue| issue.open).count();
    let closed = through.len() - open;
    let rows = through
        .into_iter()
        .filter(|issue| issue.open == (showing == Showing::Open))
        .collect();
    (rows, open, closed)
}

impl IssuesView {
    /// The search box, made on first draw since an input needs a window.
    fn query(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(query) = &self.query {
            return query.clone();
        }
        let query =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search titles, or #6 to jump"));
        cx.subscribe(&query, |view, query, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let text = query.read(cx).value().to_string();
                view.jump(&text);
            }
            cx.notify();
        })
        .detach();
        self.query = Some(query.clone());
        query
    }

    /// Select the issue `query` names outright, and turn the filters so its
    /// row is in the list. Not while a form is open: a search is not a reason
    /// to throw away what is being written.
    fn jump(&mut self, query: &str) {
        let Some(state) = self.state_mut() else {
            return;
        };
        if state.form.is_some() {
            return;
        }
        let Some((number, open, labels)) = state
            .issues
            .as_ref()
            .and_then(|kept| named(kept, query))
            .map(|issue| (issue.number, issue.open, issue.labels.clone()))
        else {
            return;
        };
        state.show(number);
        self.showing = if open { Showing::Open } else { Showing::Closed };
        if self
            .label
            .as_ref()
            .is_some_and(|label| !labels.contains(label))
        {
            self.label = None;
        }
    }

    /// The list: the search and the way to start a new issue, the filters,
    /// then a row per issue, and the sync footer.
    pub(super) fn list(
        &mut self,
        issues: &Issues,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let query = self.query(window, cx);
        let text = query.read(cx).value().to_string();
        let narrowing = Narrowing {
            query: &text,
            label: self.label.as_deref(),
        };
        let (rows, open, closed) = listed(issues, &narrowing, self.showing);
        let selected = self
            .root
            .as_ref()
            .and_then(|root| self.roots.get(root))
            .and_then(|state| state.selected);
        let muted = cx.theme().muted_foreground;

        let search = div()
            .h_flex()
            .items_center()
            .gap_1()
            .w_full()
            .flex_none()
            .px_2()
            .pt_2()
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&query)
                        .small()
                        .prefix(Icon::new(IconName::Search).xsmall().text_color(muted))
                        .cleanable(true),
                ),
            )
            .child(
                action("issues-new")
                    .small()
                    .ghost()
                    .icon(Icon::new(IconName::Plus))
                    .tooltip("New issue")
                    .on_click(cx.listener(|view, _: &ClickEvent, window, cx| {
                        view.open_form(None, window, cx)
                    })),
            );

        let view = cx.entity();
        let filters = div()
            .h_flex()
            .items_center()
            .gap_1()
            .w_full()
            .flex_none()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(div().flex_1().min_w_0().child(switch(
                "issues-showing",
                &[
                    SharedString::from(format!("Open {open}")),
                    SharedString::from(format!("Closed {closed}")),
                ],
                match self.showing {
                    Showing::Open => 0,
                    Showing::Closed => 1,
                },
                Size::XSmall,
                move |picked, _, cx| {
                    view.update(cx, |view, cx| {
                        view.showing = if *picked == 0 {
                            Showing::Open
                        } else {
                            Showing::Closed
                        };
                        cx.notify();
                    })
                },
                cx,
            )))
            .children(self.label_menu(issues, cx));

        let cut = rows.len().saturating_sub(LIST_CAP);
        let empty = if issues.listed().is_empty() {
            Some("No issues yet")
        } else if rows.is_empty() {
            Some(match self.showing {
                Showing::Open => "No open issues match",
                Showing::Closed => "No closed issues match",
            })
        } else {
            None
        };
        let now = issues::now();
        let drawn: Vec<AnyElement> = rows
            .iter()
            .take(LIST_CAP)
            .map(|issue| self.row(issue, selected == Some(issue.number), now, cx))
            .collect();
        let footer = self.sync_footer(issues, cx);

        div()
            .size_full()
            .v_flex()
            .border_r_1()
            .border_color(cx.theme().border)
            .child(search)
            .child(filters)
            .child(
                div()
                    .id("issues-list")
                    .flex_1()
                    .min_h_0()
                    .v_flex()
                    .gap_0p5()
                    .p_1()
                    .overflow_y_scroll()
                    .children(
                        empty.map(|empty| {
                            div().px_2().py_1().text_xs().text_color(muted).child(empty)
                        }),
                    )
                    .children(drawn)
                    .when(cut > 0, |list| {
                        list.child(
                            div()
                                .px_2()
                                .py_1()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("… {cut} more not shown")),
                        )
                    }),
            )
            .children(footer)
            .into_any_element()
    }

    /// The label filter: a menu of every label the project's issues carry, the
    /// one picked named on its trigger. Not drawn while no issue has a label.
    fn label_menu(&self, issues: &Issues, cx: &mut Context<Self>) -> Option<AnyElement> {
        let labels: Vec<String> = issues
            .listed()
            .iter()
            .flat_map(|issue| issue.labels.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if labels.is_empty() {
            return None;
        }
        let picked = self.label.clone();
        let trigger = action("issues-label-filter")
            .xsmall()
            .ghost()
            .label(picked.clone().unwrap_or_else(|| "Label".to_string()))
            .icon(Icon::new(IconName::ChevronDown))
            .tooltip("Show only issues with a label");
        let root = self.root.as_ref()?.display().to_string();
        let view = cx.entity();
        let pick = move |label: Option<String>| {
            let view = view.clone();
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                let label = label.clone();
                view.update(cx, |view, cx| {
                    view.label = label;
                    cx.notify();
                })
            }
        };
        Some(
            menu_below(
                SharedString::from(format!("issues-label-menu-{root}")),
                trigger,
                move |menu, window, _| {
                    // In rems' worth of pixels, so a zoomed panel's menu keeps
                    // its proportion to the rows inside it.
                    let tall = window.rem_size() * 20.;
                    let mut menu = menu.scrollable(true).max_h(tall).item(
                        menu_item("All labels")
                            .checked(picked.is_none())
                            .on_click(pick(None)),
                    );
                    for label in &labels {
                        menu = menu.item(
                            menu_item(label.clone())
                                .checked(picked.as_ref() == Some(label))
                                .on_click(pick(Some(label.clone()))),
                        );
                    }
                    menu
                },
            )
            .into_any_element(),
        )
    }

    /// The foot of the list: whether this project is kept in step with its
    /// forge, when it last was, and the controls for it — a sync now, and a
    /// pause. Drawn only where a forge serves the project; elsewhere there is
    /// nothing to be in step with.
    fn sync_footer(&self, issues: &Issues, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.roots.get(self.root.as_ref()?)?;
        let forge = state.forge?;
        let name = forge.name();
        let on = issues.in_step_with(name);
        let muted = cx.theme().muted_foreground;
        let warning = status_ink(cx).warning;
        // A project that was kept in step once still holds its links, which is
        // what tells a pause from never having started.
        let paused = issues.listed().iter().any(|issue| issue.link.is_some());
        let (icon, ink, line, detail) = match (on, state.syncing, &state.synced) {
            (true, true, _) => (
                IconName::LoaderCircle,
                muted,
                format!("Syncing with {name}…"),
                None,
            ),
            (true, false, Some((at, Ok(said)))) => (
                IconName::CircleCheck,
                muted,
                format!(
                    "Synced with {name} · {}",
                    onehand_core::rel_time(issues::now(), *at)
                ),
                Some(said.clone()),
            ),
            (true, false, Some((_, Err(why)))) => (
                IconName::TriangleAlert,
                warning,
                why.lines().next().unwrap_or_default().to_string(),
                Some(why.clone()),
            ),
            (true, false, None) => (
                IconName::CircleCheck,
                muted,
                format!("Kept in step with {name}"),
                None,
            ),
            (false, _, _) if paused => (
                IconName::Pause,
                muted,
                format!("Sync with {name} paused"),
                None,
            ),
            (false, _, _) => (
                IconName::Info,
                muted,
                format!("Not synced with {name}"),
                None,
            ),
        };
        let failed = on && !state.syncing && matches!(state.synced, Some((_, Err(_))));
        let status = div()
            .id("issues-sync-status")
            .flex_1()
            .min_w_0()
            .h_flex()
            .items_center()
            .gap_1()
            .child(Icon::new(icon).xsmall().text_color(ink))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(ink)
                    .child(line),
            )
            .when_some(detail, |status, detail| {
                status.tooltip(move |window, cx| Tooltip::new(detail.clone()).build(window, cx))
            });
        let footer = div()
            .h_flex()
            .items_center()
            .gap_1()
            .w_full()
            .flex_none()
            .px_2()
            .py_1()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(status)
            // A sync is not offered while one runs: pressed then it would do
            // nothing, and a control that answers with nothing reads as broken.
            .when(on && !state.syncing, |footer| {
                footer.child(
                    if failed {
                        action("issues-sync-retry")
                            .xsmall()
                            .ghost()
                            .label("Retry")
                            .tooltip(format!("Sync with {name} again"))
                    } else {
                        action("issues-sync-now")
                            .xsmall()
                            .ghost()
                            .icon(Icon::new(IconName::Redo))
                            .tooltip("Sync now")
                    }
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.sync(true, cx))),
                )
            })
            .child(if on {
                action("issues-sync-pause")
                    .xsmall()
                    .ghost()
                    .icon(Icon::new(IconName::Pause))
                    .tooltip("Pause auto-sync; links to issues are kept")
                    .on_click(
                        cx.listener(|view, _: &ClickEvent, _, cx| view.set_syncing(false, cx)),
                    )
            } else {
                action("issues-sync-resume")
                    .xsmall()
                    .ghost()
                    .icon(Icon::new(IconName::Play))
                    .tooltip(if paused {
                        format!("Resume auto-sync with {name}")
                    } else {
                        format!(
                            "Sync with {name}: bring in its open issues and keep both sides in step"
                        )
                    })
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.set_syncing(true, cx)))
            });
        Some(footer.into_any_element())
    }

    /// One issue: its title, up to two lines of it, over a muted line saying
    /// how it is named, its first label and how many more, and when it last
    /// changed.
    fn row(
        &self,
        issue: &LocalIssue,
        selected: bool,
        now: u64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let number = issue.number;
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let conflicted = issue.link.as_ref().is_some_and(|l| l.conflict.is_some());
        let working = self.working(issue).is_some();
        let meta = div()
            .h_flex()
            .items_center()
            .gap_1()
            .min_w_0()
            .text_xs()
            .text_color(muted)
            .child(identity(issue, cx))
            .when_some(issue.labels.first(), |meta, label| {
                meta.child("·")
                    .child(chip(label.clone(), cx))
                    .when(issue.labels.len() > 1, |meta| {
                        meta.child(format!("+{}", issue.labels.len() - 1))
                    })
            })
            .child("·")
            .child(
                div()
                    .flex_none()
                    .child(onehand_core::rel_time(now, issue.updated)),
            )
            // An issue waiting on a person is marked in words, in the warning
            // ink: it is the one row nothing will move until somebody opens it.
            .when(conflicted, |meta| {
                meta.child("·")
                    .child(div().text_color(status_ink(cx).warning).child("decide"))
            })
            // One a live session is on, so it is not started a second time.
            .when(working, |meta| {
                meta.child("·").child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_0p5()
                        .child(Icon::new(IconName::Bot).xsmall())
                        .child("working"),
                )
            });
        div()
            .id(("issue-row", number))
            .v_flex()
            .gap_0p5()
            .w_full()
            .px_2()
            .py_1()
            .rounded(theme.radius)
            // The bar is always there and only coloured when selected, so a
            // row does not shift sideways as the selection moves onto it.
            .border_l_2()
            .border_color(if selected {
                theme.list_active_border
            } else {
                gpui::transparent_black()
            })
            .cursor_pointer()
            .map(|row| {
                if selected {
                    row.bg(theme.list_active)
                } else {
                    row.hover(|row| row.bg(theme.list_hover))
                }
            })
            .child(
                div()
                    .text_sm()
                    .line_clamp(2)
                    // A closed issue stays in the list as a record, drawn
                    // quieter than the work that is still open.
                    .when(!issue.open, |title| title.text_color(muted).line_through())
                    .child(issue.title.clone()),
            )
            .child(meta)
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.select(number, cx)))
            .into_any_element()
    }
}

/// How an issue is named on screen: its forge's reference, muted, or a *Draft*
/// tag for one that has not left onehand. Its number here is a key and is never
/// shown.
pub(super) fn identity(issue: &LocalIssue, cx: &App) -> AnyElement {
    match issue.reference() {
        Some(reference) => div()
            .flex_none()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(reference.to_string())
            .into_any_element(),
        None => chip("Draft".to_string(), cx),
    }
}

/// A small tag: a label, or the *Draft* mark.
pub(super) fn chip(text: String, cx: &App) -> AnyElement {
    div()
        .flex_none()
        .px_1()
        .rounded(cx.theme().radius)
        .bg(cx.theme().secondary)
        .text_xs()
        .text_color(cx.theme().secondary_foreground)
        .child(text)
        .into_any_element()
}

#[cfg(test)]
mod tests;
