//! The Schedules page: every schedule of the window's projects, when each
//! runs and runs next, what its last look said, and its switch.
//!
//! A column page, as Tasks is, filtered by project the way Tasks is; a
//! project's page leads here filtered to it. What it lists is what the clock
//! last read (`crate::schedules::listed`), so a schedule edited in the file
//! shows within a minute. Making and editing one is a form of its own.

use crate::chat::pane::page_parts::{page_card, page_head, ruled};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement as _, Styled,
    Window, div,
};
use gpui_component::switch::Switch;
use gpui_component::{ActiveTheme as _, Sizable as _, StyledExt as _};
use onehand_core::schedule::{self, Schedule};
use std::path::PathBuf;

/// How many schedules the page lists before saying how many more there are.
const SCHEDULES_SHOWN: usize = 50;

pub(crate) struct SchedulesPage {
    focus_handle: FocusHandle,
    /// The window's projects, by root and name.
    projects: Vec<(PathBuf, SharedString)>,
    /// The project the list is narrowed to; none, every one.
    filter: Option<PathBuf>,
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

impl Render for SchedulesPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        let rows: Vec<_> = mine
            .iter()
            .take(SCHEDULES_SHOWN)
            .map(|s| schedule_row(s, &self.label_of(&s.project), now, cx))
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
                        .child(page_head(
                            "Schedules",
                            facts.map(IntoElement::into_any_element),
                            Vec::new(),
                            cx,
                        ))
                        .child(div().h_flex().child(self.filter_picker(cx)))
                        .child(list),
                ),
            )
    }
}

impl SchedulesPage {
    /// The project filter, as the Tasks page's.
    fn filter_picker(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let name = self.filter.as_deref().map_or_else(
            || crate::chat::pane::ALL_PROJECTS.into(),
            |root| self.label_of(root),
        );
        let choices: Vec<(SharedString, Option<PathBuf>)> =
            std::iter::once((crate::chat::pane::ALL_PROJECTS.into(), None))
                .chain(
                    self.projects
                        .iter()
                        .map(|(root, label)| (label.clone(), Some(root.clone()))),
                )
                .collect();
        let current = self.filter.clone();
        let this = cx.entity();
        crate::controls::menu_below(
            "schedules-filter",
            onehand_ui::picker(
                "schedules-filter-trigger",
                name,
                self.filter.is_some(),
                onehand_ui::ControlSize::Short,
                cx,
            ),
            move |mut menu, _, _| {
                for (label, only) in &choices {
                    let (only, this) = (only.clone(), this.clone());
                    menu = menu.item(
                        crate::controls::menu_item(label.clone())
                            .checked(only == current)
                            .on_click(move |_, _, cx: &mut App| {
                                this.update(cx, |page: &mut Self, cx| {
                                    page.filter = only.clone();
                                    cx.notify();
                                });
                            }),
                    );
                }
                menu
            },
        )
    }
}

/// One schedule, as a task's row is drawn: its name, then when it runs, its
/// project and when it runs next, then what its last look said; its switch
/// at the end.
fn schedule_row(s: &Schedule, project: &SharedString, now: u64, cx: &App) -> gpui::AnyElement {
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
            div()
                .v_flex()
                .gap_0p5()
                .flex_1()
                .min_w_0()
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
                Switch::new(SharedString::from(format!("schedule-on-{}", s.id)))
                    .checked(s.on)
                    .small()
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
