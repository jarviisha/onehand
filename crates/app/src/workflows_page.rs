//! The Workflows page: every workflow on offer, *Run…* on each, and the
//! editor a workflow is written in, one box per step.
//!
//! **The draft is the page's**, held on this entity rather than in the
//! render, and the entity is the window's for as long as the window is, so
//! leaving for another page and coming back finds an edit as it was left.
//! Dropping it is always asked first: picking another workflow over unsaved
//! changes, or closing the window on them.
//!
//! **No second start form.** *Run…* asks which project, the one selected in
//! the rail first, and hands the choice to the window, which opens the one
//! launcher the rail and the keymap command open.

mod actions;
mod draft;
mod form;
mod step;

pub(crate) use draft::WorkflowDraft;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div, rems,
};
use gpui_component::button::ButtonVariants;
use gpui_component::tag::Tag;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::workflow as core;
use std::path::PathBuf;

/// How wide the page's column runs, in rems: wider than the other pages',
/// since a step's prompt is written here and reads as prose.
const EDITOR_COLUMN: f32 = 48.;

/// What the page asks of its window.
pub(crate) enum WorkflowsPageEvent {
    /// Open the launcher on `root` with workflow `template` picked.
    Run { template: usize, root: PathBuf },
}

pub(crate) struct WorkflowsPage {
    focus_handle: FocusHandle,
    /// The workflow being written, while one is.
    draft: Option<WorkflowDraft>,
    /// The window's projects a run may start on, in rail order, and the one
    /// selected in the rail, which *Run…* offers first.
    projects: Vec<(PathBuf, SharedString)>,
    selected: Option<PathBuf>,
}

impl EventEmitter<WorkflowsPageEvent> for WorkflowsPage {}

impl Focusable for WorkflowsPage {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl WorkflowsPage {
    pub(crate) fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self {
            focus_handle: cx.focus_handle(),
            draft: None,
            projects: Vec::new(),
            selected: None,
        })
    }

    /// Told by the window which projects it has, and which one the rail has
    /// selected.
    pub(crate) fn set_projects(
        &mut self,
        projects: Vec<(PathBuf, SharedString)>,
        selected: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if self.projects != projects || self.selected != selected {
            self.projects = projects;
            self.selected = selected;
            cx.notify();
        }
    }

    /// Whether a workflow is being written with changes not saved.
    pub(crate) fn dirty(&self, cx: &App) -> bool {
        self.draft.as_ref().is_some_and(|d| d.dirty(cx))
    }

    /// Drop the workflow being written, saved or not.
    pub(crate) fn drop_draft(&mut self, cx: &mut Context<Self>) {
        self.draft = None;
        cx.notify();
    }

    /// The projects in the order *Run…* offers them: the one selected in the
    /// rail first, then the rest as the rail has them.
    fn run_choices(&self) -> Vec<(PathBuf, SharedString)> {
        let mut choices = self.projects.clone();
        if let Some(at) = choices
            .iter()
            .position(|(root, _)| Some(root) == self.selected.as_ref())
        {
            let picked = choices.remove(at);
            choices.insert(0, picked);
        }
        choices
    }
}

impl Render for WorkflowsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let handle = cx.entity();
        let list = list(&handle, &self.run_choices(), cx);
        let form = self
            .draft
            .as_ref()
            .map(|draft| form::form(&handle, draft, cx));
        // A bare page, so it tracks its own focus: a field typed in here
        // must be inside this, or the window's keys land elsewhere.
        div()
            .id("workflows-page")
            .track_focus(&self.focus_handle)
            .size_full()
            .overflow_y_scroll()
            .child(
                div().h_flex().justify_center().px_4().py_6().child(
                    div()
                        .v_flex()
                        .gap_6()
                        .w_full()
                        .max_w(rems(EDITOR_COLUMN))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(
                                    "The workflows a run starts from, shared by every \
                                     workspace. The ones onehand ships are read-only: duplicate \
                                     one to change it.",
                                ),
                        )
                        .child(list)
                        .children(form),
                ),
            )
    }
}

/// Every workflow on offer, capped and saying what it left out, each with
/// *Run…* and what may be done to it, then *New workflow* and *Import…*.
fn list(
    handle: &Entity<WorkflowsPage>,
    choices: &[(PathBuf, SharedString)],
    cx: &App,
) -> AnyElement {
    let entries = crate::workflow::templates(cx);
    let muted = cx.theme().muted_foreground;
    let warning = crate::theme::status_ink(cx).warning;
    let left_out = entries
        .len()
        .saturating_sub(crate::workflow::TEMPLATES_SHOWN);
    let rows = entries
        .iter()
        .take(crate::workflow::TEMPLATES_SHOWN)
        .enumerate()
        .map(|(i, entry)| {
            let shipped = entry.file.is_none();
            let about = match &entry.template {
                Ok(template) => div()
                    .text_xs()
                    .child(template.description.clone())
                    .into_any_element(),
                Err(why) => div()
                    .text_xs()
                    .text_color(warning)
                    .child(format!("Cannot be read: {why}"))
                    .into_any_element(),
            };
            let readable = entry.template.is_ok();
            crate::settings::list_row(
                entry.name(),
                Some(about),
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .when(shipped, |row| {
                        row.child(Tag::secondary().small().child("Built in"))
                    })
                    .when(readable, |row| row.child(run_menu(handle, i, choices)))
                    .when(readable, |row| {
                        row.child(row_action(
                            handle,
                            ("duplicate-workflow", i),
                            "Duplicate",
                            move |page, window, cx| page.duplicate_workflow(i, window, cx),
                        ))
                    })
                    .when(readable, |row| {
                        row.child(row_action(
                            handle,
                            ("export-workflow", i),
                            "Export…",
                            move |page, window, cx| page.export_workflow(i, window, cx),
                        ))
                    })
                    .when(readable && !shipped, |row| {
                        row.child(
                            row_icon(("edit-workflow", i), crate::icons::Icon::SquarePen, "Edit")
                                .on_click(click(handle, move |page, window, cx| {
                                    page.edit_workflow(i, window, cx)
                                })),
                        )
                    })
                    .when(!shipped, |row| {
                        row.child(row_delete(
                            handle,
                            ("delete-workflow", i),
                            move |page, window, cx| page.confirm_delete_workflow(i, window, cx),
                            cx,
                        ))
                    }),
                cx,
            )
            .into_any_element()
        })
        .collect::<Vec<_>>();
    div()
        .v_flex()
        .gap_3()
        .w_full()
        .children(rows)
        .when(left_out > 0, |list| {
            list.child(div().text_sm().text_color(muted).child(format!(
                "{left_out} more workflows not shown; remove some from {}",
                core::store::dir().display()
            )))
        })
        .when(entries.is_empty(), |list| {
            list.child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("Reading the workflows…"),
            )
        })
        .child(
            div()
                .h_flex()
                .gap_1()
                .child(
                    crate::controls::action("new-workflow")
                        .ghost()
                        .icon(Icon::new(IconName::Plus))
                        .label("New workflow")
                        .on_click(click(handle, |page, window, cx| {
                            page.new_workflow(window, cx)
                        })),
                )
                .child(
                    crate::controls::action("import-workflow")
                        .ghost()
                        .icon(Icon::new(IconName::FolderOpen))
                        .label("Import…")
                        .on_click(click(handle, |page, window, cx| {
                            page.import_workflow(window, cx)
                        })),
                ),
        )
        .into_any_element()
}

/// *Run…*: which project to start workflow `i` on, the rail's selection
/// first, then the launcher.
fn run_menu(
    handle: &Entity<WorkflowsPage>,
    i: usize,
    choices: &[(PathBuf, SharedString)],
) -> impl IntoElement {
    let (handle, choices) = (handle.clone(), choices.to_vec());
    crate::controls::menu_below(
        ("run-workflow", i),
        crate::controls::action(("run-workflow-trigger", i))
            .ghost()
            .small()
            .label("Run…"),
        move |mut menu, _, _| {
            menu = menu.label("Run on");
            for (root, label) in &choices {
                let (handle, root) = (handle.clone(), root.clone());
                menu = menu.item(crate::controls::menu_item(label.clone()).on_click(
                    move |_, _: &mut Window, cx: &mut App| {
                        let root = root.clone();
                        handle.update(cx, |_, cx| {
                            cx.emit(WorkflowsPageEvent::Run { template: i, root })
                        });
                    },
                ));
            }
            if choices.is_empty() {
                menu = menu.label("Add a project first");
            }
            menu
        },
    )
}

/// A click handler that hands `act` the page.
fn click(
    handle: &Entity<WorkflowsPage>,
    act: impl Fn(&mut WorkflowsPage, &mut Window, &mut Context<WorkflowsPage>) + 'static,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let handle = handle.clone();
    move |_, window, cx| handle.update(cx, |page, cx| act(page, window, cx))
}

fn row_action(
    handle: &Entity<WorkflowsPage>,
    id: (&'static str, usize),
    label: &'static str,
    act: impl Fn(&mut WorkflowsPage, &mut Window, &mut Context<WorkflowsPage>) + 'static,
) -> impl IntoElement {
    crate::controls::action(id)
        .ghost()
        .small()
        .label(label)
        .on_click(click(handle, act))
}

/// A row's *Delete*: a word in the danger tint, like every control that
/// removes something.
fn row_delete(
    handle: &Entity<WorkflowsPage>,
    id: (&'static str, usize),
    act: impl Fn(&mut WorkflowsPage, &mut Window, &mut Context<WorkflowsPage>) + 'static,
    cx: &App,
) -> impl IntoElement {
    crate::controls::action(id)
        .ghost()
        .small()
        .text_color(crate::theme::status_ink(cx).danger)
        .label("Delete")
        .on_click(click(handle, act))
}

fn row_icon(
    id: (&'static str, usize),
    icon: impl Into<Icon>,
    tip: &'static str,
) -> gpui_component::button::Button {
    crate::controls::action(id)
        .ghost()
        .small()
        .icon(Icon::new(icon))
        .tooltip(tip)
}
