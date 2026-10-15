//! The left half of the Issues mode: the search, the filters, a row per issue
//! and the sync footer under them.

use super::{IssuesView, LIST_CAP};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, InteractiveElement as _,
    IntoElement, ParentElement, SharedString, StatefulInteractiveElement as _, Styled, Window, div,
};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::tag::Tag;
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::issues::{self, Issues, LocalIssue};
use onehand_plugin_host::Quiet as _;
use onehand_plugin_host::search_input;
use onehand_plugin_host::{
    ControlSize, Tint, action, menu_below, menu_item, middle_ink, picker, row as row_surface,
    status_ink, switch,
};
use std::collections::BTreeSet;

/// How many labels the label filter offers, in name order. A project with more
/// is said to have them under the last one.
const LABELS_SHOWN: usize = 100;

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
        issues::matches_query(issue, self.query)
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
    pub(super) fn query(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(query) = &self.query {
            return query.clone();
        }
        // The page narrows to a reference; only the tab jumps to it.
        let hint = match self.page {
            Some(_) => "Search issues or #number",
            None => "Search titles, or #6 to jump",
        };
        let query = cx.new(|cx| InputState::new(window, cx).placeholder(hint));
        cx.subscribe(&query, |view, query, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let text = query.read(cx).value().to_string();
                match view.page.as_mut() {
                    // Typing is looking through the list, so a narrow tab
                    // shows it: a filter applied behind the issue changes
                    // nothing on screen.
                    None => {
                        view.detail = false;
                        view.jump(&text, cx);
                    }
                    // A search is a filter: the page's list is drawn afresh.
                    Some(page) => page.refilter(),
                }
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
    fn jump(&mut self, query: &str, cx: &App) {
        let Some(root) = self.root.clone() else {
            return;
        };
        if self.state_mut().is_none_or(|state| state.form.is_some()) {
            return;
        }
        let Some((number, open, labels)) = self
            .issues_of(&root, cx)
            .and_then(|kept| named(kept, query))
            .map(|issue| (issue.number, issue.open, issue.labels.clone()))
        else {
            return;
        };
        // Selected, not opened: the query may still be on its way to
        // another number (`#1` on the way to `#12`), so a narrow tab keeps
        // the list showing with the row marked, and a click opens it.
        if let Some(state) = self.state_mut() {
            state.selected = Some(number);
        }
        self.showing = if open { Showing::Open } else { Showing::Closed };
        if self
            .label
            .as_ref()
            .is_some_and(|label| !labels.contains(label))
        {
            self.label = None;
        }
    }

    /// The search and the way to start a new issue, over the list and the
    /// issue both, so they stay in reach while the two show one at a time.
    pub(super) fn search_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let query = self.query(window, cx);
        // A filter bar, as a page's is, with no line under it: the space and
        // the change from controls to rows part the two.
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .w_full()
            .flex_none()
            .px_3()
            .py_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(search_input(&query, cx).xsmall()),
            )
            .child(
                action("issues-new")
                    .xsmall()
                    .quiet_icon(false, cx)
                    .icon(Icon::new(IconName::Plus))
                    .tooltip("New issue")
                    .on_click(cx.listener(|view, _: &ClickEvent, window, cx| {
                        view.open_form(None, window, cx)
                    })),
            )
            .into_any_element()
    }

    /// The list: the filters, then a row per issue, and the sync footer.
    pub(super) fn list(
        &mut self,
        issues: &Issues,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // A label no issue carries any more is dropped: its menu is gone with
        // it, and a filter nothing on screen can clear hides every issue.
        if let Some(label) = &self.label
            && !issues
                .listed()
                .iter()
                .any(|issue| issue.labels.contains(label))
        {
            self.label = None;
        }
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

        let view = cx.entity();
        let filters = div()
            .h_flex()
            .items_center()
            .gap_2()
            .w_full()
            .flex_none()
            .px_3()
            .pb_2()
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
                ControlSize::XSmall,
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
            .child(filters)
            .child(
                div()
                    .id("issues-list")
                    .flex_1()
                    .min_h_0()
                    .v_flex()
                    .gap_1()
                    .p_2()
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
        let left_out = labels.len().saturating_sub(LABELS_SHOWN);
        let picked = self.label.clone();
        let trigger = picker(
            "issues-label-filter",
            picked.clone().unwrap_or_else(|| "Label".to_string()),
            picked.is_some(),
            ControlSize::XSmall,
            cx,
        )
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
                    for label in labels.iter().take(LABELS_SHOWN) {
                        menu = menu.item(
                            menu_item(label.clone())
                                .checked(picked.as_ref() == Some(label))
                                .on_click(pick(Some(label.clone()))),
                        );
                    }
                    if left_out > 0 {
                        menu = menu.label(format!("… {left_out} more labels not shown"));
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
        let file = self.roots.get(self.root.as_ref()?)?.file.read(cx);
        let (forge, syncing, synced) = (file.forge?, file.syncing, file.synced.clone());
        let name = forge.name();
        let on = issues.in_step_with(name);
        let muted = cx.theme().muted_foreground;
        let warning = status_ink(cx).warning;
        // A project that was kept in step once still holds its links, which is
        // what tells a pause from never having started.
        let paused = issues.listed().iter().any(|issue| issue.link.is_some());
        let (ink, line, detail) = match (on, syncing, &synced) {
            (true, true, _) => (
                muted,
                format!("Syncing with {name}…"),
                format!("Bringing in what changed on {name} and sending what changed here"),
            ),
            (true, false, Some((at, Ok(said)))) => (
                muted,
                format!(
                    "Synced with {name} · {}",
                    onehand_core::rel_time(issues::now(), *at)
                ),
                said.clone(),
            ),
            (true, false, Some((_, Err(why)))) => (
                warning,
                why.lines().next().unwrap_or_default().to_string(),
                why.clone(),
            ),
            (true, false, None) => (
                muted,
                format!("Kept in step with {name}"),
                format!("No sync with {name} has finished since onehand started"),
            ),
            (false, _, _) if paused => (
                muted,
                format!("Sync with {name} paused"),
                format!(
                    "Nothing is sent to or brought in from {name}; links to its issues are kept"
                ),
            ),
            (false, _, _) => (
                muted,
                format!("Not synced with {name}"),
                format!("These issues are kept here only; resume to keep them in step with {name}"),
            ),
        };
        let failed = on && !syncing && matches!(synced, Some((_, Err(_))));
        let status = div()
            .id("issues-sync-status")
            .flex_1()
            .min_w_0()
            .h_flex()
            .items_center()
            .gap_1()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(ink)
                    .child(line),
            )
            .tooltip(move |window, cx| Tooltip::new(detail.clone()).build(window, cx));
        let footer = div()
            .h_flex()
            .items_center()
            .gap_1()
            .w_full()
            .flex_none()
            .px_3()
            .py_1()
            .child(status)
            // *Sync now* stays where it is while a sync runs, spinning: a
            // control that leaves as it is pressed moves the one beside it
            // under the pointer.
            .when(on, |footer| {
                footer.child(
                    if failed {
                        action("issues-sync-retry")
                            .xsmall()
                            .quiet(cx)
                            .label("Retry")
                            .tooltip(format!("Sync with {name} again"))
                    } else {
                        action("issues-sync-now")
                            .xsmall()
                            .quiet_icon(false, cx)
                            .icon(Icon::new(IconName::Redo))
                            .loading(syncing)
                            .tooltip("Sync now")
                    }
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.sync(true, cx))),
                )
            })
            .child(if on {
                action("issues-sync-pause")
                    .xsmall()
                    .quiet_icon(false, cx)
                    .icon(Icon::new(IconName::Pause))
                    .tooltip("Pause auto-sync; links to issues are kept")
                    .on_click(
                        cx.listener(|view, _: &ClickEvent, _, cx| view.set_syncing(false, cx)),
                    )
            } else {
                action("issues-sync-resume")
                    .xsmall()
                    .quiet_icon(false, cx)
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
        // On the selected fill the quieter ink is taken under that fill's
        // own, so it stays a step quieter there rather than sinking into it.
        let muted = match selected {
            true => middle_ink(theme.accent_foreground, theme.muted_foreground),
            false => theme.muted_foreground,
        };
        let conflicted = issue.link.as_ref().is_some_and(|l| l.conflict.is_some());
        let working = super::working_in(issue, &self.live).is_some();
        let meta = div()
            .h_flex()
            .items_center()
            .gap_1()
            .min_w_0()
            .text_xs()
            .text_color(muted)
            .child(identity(issue, muted, cx))
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
            .when(working, |meta| meta.child("·").child("working"));
        // A selection is a fill and only a fill, the selected step the ramp
        // holds clear of the hover step: no bar or ring beside it.
        row_surface(selected, cx)
            .id(("issue-row", number))
            .v_flex()
            .gap_0p5()
            .w_full()
            .py_1p5()
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
            .on_click(
                cx.listener(move |view, _: &ClickEvent, window, cx| {
                    view.select(number, window, cx)
                }),
            )
            .into_any_element()
    }
}

/// How an issue is named on screen: its forge's reference in `ink`, or a
/// *Draft* tag for one that has not left onehand. Its number here is a key and
/// is never shown.
fn identity(issue: &LocalIssue, ink: gpui::Hsla, cx: &App) -> AnyElement {
    match issue.reference() {
        Some(reference) => div()
            .flex_none()
            .text_xs()
            .text_color(ink)
            .child(reference.to_string())
            .into_any_element(),
        None => plain_tag("Draft".to_string(), cx).into_any_element(),
    }
}

/// A label as a small tag, on a tint of its kind's hue.
pub(super) fn chip(label: String, cx: &App) -> Tag {
    onehand_plugin_host::label_badge(label, cx)
}

/// A tag that is not a label (the *Draft* mark, a project's name), always
/// neutral, so a name that happens to read like a label is never drawn as
/// one.
pub(super) fn plain_tag(text: String, cx: &App) -> Tag {
    tag(text, neutral(cx))
}

/// The tint a tag takes when it says nothing about state.
fn neutral(cx: &App) -> Tint {
    let theme = cx.theme();
    Tint {
        fill: theme.secondary,
        ink: theme.secondary_foreground,
    }
}

/// A badge that keeps its width in a row.
fn tag(text: String, tint: Tint) -> Tag {
    onehand_plugin_host::badge(text, tint).flex_none()
}

#[cfg(test)]
mod tests;
