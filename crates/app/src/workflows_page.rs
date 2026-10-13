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

// The Workflows page's key: the form open on the page closed, asked first
// when it holds changes.
gpui::actions!(workflows_page, [CloseWorkflowForm]);

use crate::controls::Quiet as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, Icon, Sizable as _, StyledExt};
use onehand_core::workflow as core;
use std::path::PathBuf;

/// What the page asks of its window.
pub(crate) enum WorkflowsPageEvent {
    /// Open the launcher on `root` with the workflow of id `workflow`
    /// picked: by id, since the list may be read again before the press.
    Run { workflow: String, root: PathBuf },
}

pub(crate) struct WorkflowsPage {
    focus_handle: FocusHandle,
    /// The workflow being written, while one is.
    draft: Option<WorkflowDraft>,
    /// The window's projects a run may start on, in rail order, and the one
    /// selected in the rail, which *Run…* offers first.
    projects: Vec<(PathBuf, SharedString)>,
    selected: Option<PathBuf>,
    /// A save is being written: *Save* spins and takes no second press, so
    /// one write lands at a time.
    writing: bool,
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
            writing: false,
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
            .map(|draft| form::form(&handle, draft, self.writing, cx));
        // A bare page, so it tracks its own focus: a field typed in here
        // must be inside this, or the window's keys land elsewhere.
        div()
            .id("workflows-page")
            .key_context("WorkflowsPage")
            .track_focus(&self.focus_handle)
            .on_action(
                cx.listener(|page: &mut Self, _: &CloseWorkflowForm, window, cx| {
                    page.close_workflow_form(window, cx);
                }),
            )
            .size_full()
            .overflow_y_scroll()
            .child(
                div().h_flex().justify_center().px_8().py_6().child(
                    div()
                        .v_flex()
                        .gap_6()
                        .w_full()
                        .max_w(crate::tokens::PAGE_PROSE)
                        .child(head(&handle, cx))
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

/// The page's head: the actions that make a workflow at the start, *New
/// workflow* on the secondary fill, and how many there are at the end. No
/// primary, so the form, when it is open, never has one to outweigh.
fn head(handle: &Entity<WorkflowsPage>, cx: &App) -> AnyElement {
    let count = crate::workflow::templates(cx).len();
    div()
        .h_flex()
        .items_center()
        .gap_2()
        .child(
            crate::controls::action("new-workflow")
                .small()
                .secondary()
                .label("New workflow")
                .on_click(click(handle, |page, window, cx| {
                    page.new_workflow(window, cx)
                })),
        )
        .child(
            crate::controls::action("import-workflow")
                .small()
                .quiet(cx)
                .label("Import…")
                .on_click(click(handle, |page, window, cx| {
                    page.import_workflow(window, cx)
                })),
        )
        .child(div().flex_1())
        .children((count > 0).then(|| {
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(match count {
                    1 => "1 workflow".to_string(),
                    n => format!("{n} workflows"),
                })
        }))
        .into_any_element()
}

/// Every workflow on offer, capped and saying what it left out: a row each,
/// its name over what it does, *Built in* in a slot of its own, and *Run…*
/// and what may be done to it while it is pointed at.
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
                    .text_color(muted)
                    .child(template.description.clone())
                    .into_any_element(),
                Err(why) => div()
                    .text_xs()
                    .text_color(warning)
                    .child(format!("Cannot be read: {why}"))
                    .into_any_element(),
            };
            let readable = entry.template.is_ok();
            let group = SharedString::from(format!("workflow-row-{i}"));
            let actions = div()
                .h_flex()
                .items_center()
                .gap_1()
                .when_some(entry.template.as_ref().ok(), |row, template| {
                    row.child(run_menu(handle, i, &template.id, choices, cx))
                })
                .when(readable, |row| {
                    row.child(row_action(
                        handle,
                        ("duplicate-workflow", i),
                        "Duplicate",
                        move |page, window, cx| page.duplicate_workflow(i, window, cx),
                        cx,
                    ))
                })
                .when(readable, |row| {
                    row.child(row_action(
                        handle,
                        ("export-workflow", i),
                        "Export…",
                        move |page, window, cx| page.export_workflow(i, window, cx),
                        cx,
                    ))
                })
                .when(readable && !shipped, |row| {
                    row.child(
                        row_icon(
                            ("edit-workflow", i),
                            crate::icons::Icon::SquarePen,
                            "Edit",
                            cx,
                        )
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
                });
            onehand_plugin_host::row(false, cx)
                .cursor_default()
                .group(group.clone())
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
                        .child(div().truncate().child(entry.name()))
                        .child(about),
                )
                // A content slot, always drawn and as wide whether or not it
                // holds the badge, so the actions after it never move.
                .child(div().flex_none().w(BUILT_IN_W).h_flex().justify_end().when(
                    shipped,
                    |slot| {
                        slot.child(
                            onehand_plugin_host::badge(
                                "Built in",
                                onehand_plugin_host::Tint {
                                    fill: cx.theme().secondary,
                                    ink: cx.theme().secondary_foreground,
                                },
                            )
                            .flex_none(),
                        )
                    },
                ))
                .child(onehand_plugin_host::hover_slot(group, false, actions))
                .into_any_element()
        })
        .collect::<Vec<_>>();
    let meta = |text: String| div().text_xs().text_color(muted).child(text);
    div()
        .v_flex()
        .w_full()
        .children(rows)
        .when(left_out > 0, |list| {
            list.child(
                div()
                    .h_flex()
                    .flex_wrap()
                    .gap_1()
                    .text_xs()
                    .text_color(muted)
                    .child(format!(
                        "{left_out} more workflows not shown; remove some from"
                    ))
                    .child(
                        div()
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(core::store::dir().display().to_string()),
                    ),
            )
        })
        .when(entries.is_empty(), |list| {
            list.child(meta("Reading the workflows…".to_string()))
        })
        .into_any_element()
}

/// The slot *Built in* stands in on every row, held whether or not a row
/// carries it.
const BUILT_IN_W: gpui::Rems = gpui::rems(4.5);

/// *Run…* on row `i`: which project to start workflow `workflow` on, the
/// rail's selection first, then the launcher.
fn run_menu(
    handle: &Entity<WorkflowsPage>,
    i: usize,
    workflow: &str,
    choices: &[(PathBuf, SharedString)],
    cx: &App,
) -> impl IntoElement {
    let (handle, choices, workflow) = (handle.clone(), choices.to_vec(), workflow.to_string());
    crate::controls::menu_below(
        ("run-workflow", i),
        crate::controls::action(("run-workflow-trigger", i))
            .quiet(cx)
            .small()
            .label("Run…"),
        move |mut menu, _, _| {
            menu = menu.label("Run on");
            for (root, label) in &choices {
                let (handle, root, workflow) = (handle.clone(), root.clone(), workflow.clone());
                menu = menu.item(crate::controls::menu_item(label.clone()).on_click(
                    move |_, _: &mut Window, cx: &mut App| {
                        let (root, workflow) = (root.clone(), workflow.clone());
                        handle.update(cx, |_, cx| {
                            cx.emit(WorkflowsPageEvent::Run { workflow, root })
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
    cx: &App,
) -> impl IntoElement {
    crate::controls::action(id)
        .quiet(cx)
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
        .quiet(cx)
        .small()
        .text_color(crate::theme::status_ink(cx).danger)
        .label("Delete…")
        .on_click(click(handle, act))
}

fn row_icon(
    id: (&'static str, usize),
    icon: impl Into<Icon>,
    tip: &'static str,
    cx: &App,
) -> gpui_component::button::Button {
    crate::controls::action(id)
        .quiet_icon(false, cx)
        .small()
        .icon(Icon::new(icon))
        .tooltip(tip)
}
