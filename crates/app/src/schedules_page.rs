//! The Schedules page: every schedule of the window's projects, when each
//! runs and runs next, what its last look said, and its switch.
//!
//! A column page, as Tasks is, filtered by project the way Tasks is; a
//! project's page leads here filtered to it. What it lists is what the clock
//! last read (`crate::schedules::listed`), so a schedule edited in the file
//! shows within a minute. *New schedule*, or a row, opens the form that makes
//! or edits one (`form`, `when`), in place of the list.

mod draft;
mod form;
mod when;

gpui::actions!(schedules_page, [CloseScheduleForm]);

use crate::controls::Refuses as _;
use crate::page_parts::{page_card, page_head, page_top, ruled};
use crate::state::Shared;
use draft::ScheduleDraft;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement as _, Styled,
    Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme as _, StyledExt as _};
use onehand_core::schedule::{self, Schedule};
use onehand_ui::Tall as _;
use std::path::PathBuf;

/// How many schedules the page lists before saying how many more there are.
const SCHEDULES_SHOWN: usize = 50;

pub(crate) struct SchedulesPage {
    focus_handle: FocusHandle,
    /// The window's projects, by root and name.
    projects: Vec<(PathBuf, SharedString)>,
    /// The project the list is narrowed to; none, every one.
    filter: Option<PathBuf>,
    /// The schedule being made or edited, shown in place of the list.
    draft: Option<ScheduleDraft>,
    /// A save or a delete is being written.
    writing: bool,
}

impl Focusable for SchedulesPage {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl SchedulesPage {
    pub(crate) fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self {
            focus_handle: cx.focus_handle(),
            projects: Vec::new(),
            filter: None,
            draft: None,
            writing: false,
        })
    }

    /// The window's projects, and the project to narrow to when one is asked
    /// for: else the one left is kept, while it is still a project here.
    pub(crate) fn set_projects(
        &mut self,
        projects: Vec<(PathBuf, SharedString)>,
        filter: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let filter = filter
            .or_else(|| self.filter.take())
            .filter(|only| projects.iter().any(|(root, _)| root == only));
        self.projects = projects;
        self.filter = filter;
        cx.notify();
    }

    fn label_of(&self, root: &std::path::Path) -> SharedString {
        self.projects.iter().find(|(r, _)| r == root).map_or_else(
            || root.display().to_string().into(),
            |(_, name)| name.clone(),
        )
    }
}

impl SchedulesPage {
    /// Open the form on a new schedule, on the project the list is narrowed
    /// to, else the first.
    fn new_schedule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let project = self
            .filter
            .clone()
            .or_else(|| self.projects.first().map(|(root, _)| root.clone()));
        self.open_form(&draft::blank(project), true, window, cx);
    }

    /// Open the form on schedule `id`.
    fn edit_schedule(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = crate::schedules::listed(cx)
            .iter()
            .find(|s| s.id == id)
            .cloned()
        else {
            return;
        };
        self.open_form(&s, false, window, cx);
    }

    fn open_form(&mut self, s: &Schedule, new: bool, window: &mut Window, cx: &mut Context<Self>) {
        let mut draft = ScheduleDraft::open(s, new, window, cx);
        draft.opened = Some(draft.schedule(cx));
        self.draft = Some(draft);
        // The page holds the keys while the form is up, so Esc reaches it.
        self.focus_handle.clone().focus(window, cx);
        cx.notify();
    }

    /// Close the form, asking first while it holds changes.
    fn close_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The keys come back to the page as the form's fields go.
        if !self.draft.as_ref().is_some_and(|d| d.dirty(cx)) {
            self.draft = None;
            self.focus_handle.clone().focus(window, cx);
            cx.notify();
            return;
        }
        crate::shell::confirm_on(
            cx.entity(),
            crate::shell::Confirm {
                id: "discard-schedule-edit",
                title: "Discard the schedule being edited?".into(),
                description: "Its changes are not saved. Closing the form throws them away.".into(),
                act: "Discard",
                ..Default::default()
            },
            window,
            cx,
            |page, window, cx| {
                page.draft = None;
                page.focus_handle.clone().focus(window, cx);
                cx.notify();
            },
        );
    }

    /// Save the form: the schedule replaces its own in the file, or joins it,
    /// through the one writer. Switched on by this save, nothing due before
    /// now is caught up.
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.draft.as_ref().filter(|_| !self.writing) else {
            return;
        };
        if !draft.problems(cx).is_empty() {
            return;
        }
        let mut s = draft.schedule(cx);
        let original = draft.original.clone();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        // Switched on by this save, or told a new time while on: it counts
        // from now, so no time before the save is caught up.
        let restart = s.on && original.as_ref().is_none_or(|o| !o.on || o.when != s.when);
        if restart {
            s.on_since = now;
        }
        let (path, id) = (schedule::path(), s.id.clone());
        let write = Shared::global(cx).writer.write(move || {
            let mut all = schedule::load_blocking(&path)?;
            match (all.iter_mut().find(|kept| kept.id == s.id), &original) {
                // What the clock moved while the form was open stands: a
                // time it fired for is not fired again.
                (Some(kept), _) => {
                    s.last_fired = kept.last_fired;
                    s.said = kept.said.clone();
                    if !restart {
                        s.on_since = kept.on_since;
                    }
                    *kept = s;
                }
                (None, None) => all.push(s),
                (None, Some(_)) => {
                    return Err("it was deleted from the file meanwhile".to_string());
                }
            }
            schedule::save_blocking(&path, &all).map(|()| all)
        });
        self.finish(
            write,
            &id,
            "Saving the schedule",
            "Schedule saved",
            window,
            cx,
        );
    }

    /// Ask, then delete the schedule open in the form. Its tasks stay.
    fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(original) = self.draft.as_ref().and_then(|d| d.original.clone()) else {
            return;
        };
        crate::shell::confirm_on(
            cx.entity(),
            crate::shell::Confirm {
                id: "delete-schedule",
                title: format!("Delete {}?", original.name).into(),
                description: "It runs no more. The tasks it started stay on the Tasks page.".into(),
                act: "Delete",
                ..Default::default()
            },
            window,
            cx,
            move |page, window, cx| {
                let (id, path) = (original.id.clone(), schedule::path());
                let gone = id.clone();
                let write = Shared::global(cx).writer.write(move || {
                    let mut all = schedule::load_blocking(&path)?;
                    all.retain(|kept| kept.id != gone);
                    schedule::save_blocking(&path, &all).map(|()| all)
                });
                page.finish(
                    write,
                    &id,
                    "Deleting the schedule",
                    "Schedule deleted",
                    window,
                    cx,
                );
            },
        );
    }

    /// Wait for `write`, then show what the file holds and close the form,
    /// or say why not and keep it.
    fn finish(
        &mut self,
        write: impl std::future::Future<Output = Result<Vec<Schedule>, String>> + 'static,
        id: &str,
        doing: &'static str,
        done: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.writing = true;
        cx.notify();
        let id = id.to_string();
        cx.spawn_in(window, async move |page, cx| {
            let written = write.await;
            let _ = page.update_in(cx, |page, window, cx| {
                page.writing = false;
                let landed = written.map(|all| {
                    crate::schedules::put(all, cx);
                    // Only the form it was written from: one opened since
                    // stays.
                    if page.draft.as_ref().is_some_and(|d| d.id == id) {
                        page.draft = None;
                        page.focus_handle.clone().focus(window, cx);
                    }
                });
                crate::toast::report(window, cx, doing, Some(done), landed);
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for SchedulesPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(draft) = &self.draft {
            let handle = cx.entity();
            let head = form::head(&handle, draft, &self.projects, self.writing, cx);
            let here = SharedString::from(form::title(draft, cx));
            let body = form::body(&handle, draft, &self.projects, cx);
            // A new form nobody has changed says nothing yet: *Save* refuses
            // and says why on hover.
            let problems = draft.problems(cx);
            let say = !problems.is_empty() && (draft.original.is_some() || draft.dirty(cx));
            let bar = say.then(|| {
                crate::page_parts::foot_bar(
                    gpui_component::IconName::TriangleAlert,
                    crate::theme::status_ink(cx).danger,
                    problems,
                    cx,
                )
            });
            return div()
                .key_context("SchedulesPage")
                .track_focus(&self.focus_handle)
                .on_action(
                    cx.listener(|page: &mut Self, _: &CloseScheduleForm, window, cx| {
                        page.close_form(window, cx)
                    }),
                )
                .size_full()
                .v_flex()
                .child(
                    div()
                        .id("schedule-form")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .px_8()
                        .py_6()
                        .child(
                            div().h_flex().w_full().justify_center().child(
                                div()
                                    .v_flex()
                                    .gap_6()
                                    .w_full()
                                    .max_w(crate::tokens::PAGE_PROSE)
                                    .text_size(crate::tokens::CHROME_TEXT)
                                    .child(onehand_ui::crumbs(
                                        onehand_ui::crumb_link("schedules-back", "Schedules", cx)
                                            .on_click(cx.listener(
                                                |page: &mut Self, _, window, cx| {
                                                    page.close_form(window, cx)
                                                },
                                            )),
                                        [here],
                                        cx,
                                    ))
                                    .child(head)
                                    .children(body),
                            ),
                        ),
                )
                .children(bar)
                .into_any_element();
        }
        let mine: Vec<Schedule> = crate::schedules::listed(cx)
            .iter()
            .filter(|s| self.projects.iter().any(|(root, _)| *root == s.project))
            .filter(|s| self.filter.as_ref().is_none_or(|only| *only == s.project))
            .cloned()
            .collect();
        let on = mine.iter().filter(|s| s.on).count();
        let facts = (!mine.is_empty()).then(|| format!("{on} on · {} off", mine.len() - on));
        let left_out = mine.len().saturating_sub(SCHEDULES_SHOWN);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let handle = cx.entity();
        let rows: Vec<_> = mine
            .iter()
            .take(SCHEDULES_SHOWN)
            .enumerate()
            .map(|(i, s)| schedule_row(&handle, i, s, &self.label_of(&s.project), now, cx))
            .collect();
        let empty = rows.is_empty();
        let meta = crate::theme::meta_ink(cx);
        // Before the first read, waiting; a read that failed, said here.
        let read = crate::schedules::read(cx);
        let narrowed = self.filter.is_some();

        let list = page_card("Schedules", Some(mine.len()), None, cx)
            .when(read.is_none(), |card| {
                card.child(
                    div()
                        .text_xs()
                        .text_color(meta)
                        .child("Reading the schedules…"),
                )
            })
            .when_some(read.and_then(Result::err), |card, why| {
                card.child(
                    div()
                        .text_xs()
                        .text_color(crate::theme::status_ink(cx).warning)
                        .child(format!("The schedules could not be read: {why}")),
                )
            })
            .when(empty && !narrowed, |card| card.child(nothing_yet(cx)))
            .when(empty && narrowed, |card| {
                card.child(div().text_xs().text_color(meta).child("No schedules match"))
            })
            .when(!empty, |card| card.child(ruled(rows, cx)))
            .when(left_out > 0, |card| {
                card.child(
                    div()
                        .text_xs()
                        .text_color(meta)
                        .child(format!("{left_out} more not shown")),
                )
            });

        div()
            .id("schedules-page")
            .track_focus(&self.focus_handle)
            .size_full()
            .v_flex()
            .text_size(crate::tokens::CHROME_TEXT)
            // The head and the filter at the row's end stay put; the box
            // scrolls under them.
            .child(page_top(
                Some(crate::tokens::PAGE_WIDE),
                [
                    page_head(
                        "Schedules",
                        facts.map(IntoElement::into_any_element),
                        vec![
                            crate::controls::action("new-schedule")
                                .tall()
                                .primary()
                                .label("New schedule")
                                .refuses(self.projects.is_empty().then_some("Add a project first"))
                                .on_click(cx.listener(|page: &mut Self, _, window, cx| {
                                    page.new_schedule(window, cx)
                                }))
                                .into_any_element(),
                        ],
                        cx,
                    )
                    .into_any_element(),
                    div()
                        .h_flex()
                        .justify_end()
                        .child(self.filter_picker(cx))
                        .into_any_element(),
                ],
            ))
            .child(
                div()
                    .id("schedules-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_8()
                    .py_6()
                    .child(
                        div()
                            .h_flex()
                            .w_full()
                            .justify_center()
                            .child(div().w_full().max_w(crate::tokens::PAGE_WIDE).child(list)),
                    ),
            )
            .into_any_element()
    }
}

impl SchedulesPage {
    /// The project filter, as the Tasks page's.
    fn filter_picker(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        crate::page_parts::project_filter(
            "schedules-filter",
            self.filter.as_deref().map(|root| self.label_of(root)),
            self.projects
                .iter()
                .map(|(root, label)| (label.clone(), root.clone())),
            self.filter.clone(),
            onehand_ui::ControlSize::Tall,
            move |only, cx| {
                this.update(cx, |page: &mut Self, cx| {
                    page.filter = only;
                    cx.notify();
                });
            },
            cx,
        )
    }
}

/// One schedule, as a task's row is drawn: its name, then when it runs, its
/// project and when it runs next, then what its last look said; its switch
/// at the end.
fn schedule_row(
    handle: &Entity<SchedulesPage>,
    i: usize,
    s: &Schedule,
    project: &SharedString,
    now: u64,
    cx: &App,
) -> gpui::AnyElement {
    let muted = cx.theme().muted_foreground;
    let ink = crate::theme::status_ink(cx);
    let parses = schedule::parse(&s.when).is_ok();
    let next = match (s.on, parses) {
        (false, _) => "off".to_string(),
        (true, false) => String::new(),
        (true, true) => schedule::next_times(&s.when, now, 1, &chrono::Local)
            .first()
            .map_or_else(
                || "never".to_string(),
                |t| format!("next {}", schedule::next_said(*t, now, &chrono::Local)),
            ),
    };
    let line: Vec<String> = [schedule::when_said(&s.when), project.to_string(), next]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect();
    let id = s.id.clone();
    let tip = if s.on {
        "Switch it off"
    } else {
        "Switch it on"
    };
    onehand_ui::row(false, cx)
        .h_flex()
        .items_center()
        .gap_2()
        .py_1p5()
        .child(
            // Pressed, it opens the schedule in the form, as a task's row
            // opens its task.
            div()
                .id(("schedule-open", i))
                .v_flex()
                .gap_0p5()
                .flex_1()
                .min_w_0()
                .cursor_pointer()
                .on_click({
                    let (handle, id) = (handle.clone(), s.id.clone());
                    move |_, window: &mut Window, cx: &mut App| {
                        handle.update(cx, |page, cx| page.edit_schedule(&id, window, cx))
                    }
                })
                .child(div().truncate().child(s.name.clone()))
                .child(
                    div()
                        .text_xs()
                        .truncate()
                        .text_color(if parses { muted } else { ink.warning })
                        .child(line.join(" · ")),
                )
                .children(s.said.clone().map(|said| {
                    div()
                        .text_xs()
                        .truncate()
                        .text_color(ink.warning)
                        .child(said)
                })),
        )
        // Held to its own width, as the other switches are, and given the
        // pointer the switch does not set itself. Its name is the row's.
        .child(
            div().flex_none().cursor_pointer().child(
                onehand_ui::toggle(SharedString::from(format!("schedule-on-{}", s.id)), s.on)
                    .tooltip(tip)
                    .on_click(move |on: &bool, window: &mut Window, cx: &mut App| {
                        crate::schedules::set_on(&id, *on, window, cx);
                    }),
            ),
        )
        .into_any_element()
}

/// What the list says with nothing in it: where schedules are kept.
fn nothing_yet(cx: &App) -> impl IntoElement + use<> {
    div()
        .h_flex()
        .flex_wrap()
        .gap_1()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child("No schedules yet, kept in")
        .child(
            div()
                .font_family(cx.theme().mono_font_family.clone())
                .child(schedule::path().display().to_string()),
        )
}
