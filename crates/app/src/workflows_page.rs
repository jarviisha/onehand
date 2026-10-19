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
mod list;
mod pick;
mod prompt_dialog;
mod step;

pub(crate) use draft::WorkflowDraft;

// The Workflows page's keys: the form open on the page closed, asked first
// when it holds changes, and the list stepped, which opens what it reaches.
gpui::actions!(
    workflows_page,
    [CloseWorkflowForm, WorkflowsUp, WorkflowsDown]
);

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div,
};
use gpui_component::{ActiveTheme, StyledExt};
use gpui_component::{ResizableState, h_resizable, resizable_panel};
use onehand_plugin_host::{
    ListWidths, Measured, back_link, list_detail, measure_width, unmeasured,
};
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
    /// The workflow open beside the list, by key: the list is read again
    /// after every save.
    picked: Option<pick::Pick>,
    /// The window's projects a run may start on, in rail order, and the one
    /// selected in the rail, which *Run…* offers first.
    projects: Vec<(PathBuf, SharedString)>,
    selected: Option<PathBuf>,
    /// A save is being written: *Save* spins and takes no second press, so
    /// one write lands at a time.
    writing: bool,
    /// One focus handle per listed row, by place: each row is a tab stop, so
    /// the keyboard reaches it as the pointer does.
    rows: Vec<FocusHandle>,
    /// The list's width as dragged beside the editor.
    split: Entity<ResizableState>,
    /// The page's width as last laid out, which decides whether the list and
    /// the editor stand side by side.
    width: Measured,
    /// The editor's scroll, and the open step kept in view above the drawer.
    reveal: Reveal,
}

/// Keeps the step open in the drawer in view: the form's scroll, and a mark
/// set when a step opens, which the open step's line answers on the next
/// frame by scrolling the form until the line stands clear of the drawer.
#[derive(Clone)]
pub(super) struct Reveal {
    scroll: gpui::ScrollHandle,
    pending: std::rc::Rc<std::cell::Cell<bool>>,
    page: gpui::WeakEntity<WorkflowsPage>,
}

/// The part of the editor's height the step drawer takes: most of it, the
/// rest left to the form above, enough for the open step's line and its
/// neighbours.
const DRAWER_SHARE: f32 = 0.6;

/// How far the open step's line stands clear of the drawer's edge, or of
/// the form's top, once it has been scrolled into view.
const REVEAL_MARGIN: gpui::Rems = gpui::rems(1.5);

impl Reveal {
    /// Ask for the open step to be brought into view.
    pub(super) fn ask(&self) {
        self.pending.set(true);
    }

    /// What the open step's line holds to say where it landed: laid out over
    /// the line, it scrolls the form once, when asked.
    pub(super) fn watch(&self) -> impl IntoElement {
        let this = self.clone();
        gpui::canvas(
            move |bounds, window, cx| {
                if !this.pending.replace(false) {
                    return;
                }
                let view = this.scroll.bounds();
                let margin = window.rem_size() * REVEAL_MARGIN.0;
                let mut offset = this.scroll.offset();
                if bounds.bottom() + margin > view.bottom() {
                    offset.y -= bounds.bottom() + margin - view.bottom();
                } else if bounds.top() - margin < view.top() {
                    offset.y += view.top() - (bounds.top() - margin);
                }
                offset.y = offset.y.min(gpui::px(0.)).max(-this.scroll.max_offset().y);
                if offset != this.scroll.offset() {
                    this.scroll.set_offset(offset);
                    // Prepaint runs inside the page's own frame: redraw it
                    // after, never from here.
                    let page = this.page.clone();
                    cx.defer(move |cx| {
                        let _ = page.update(cx, |_, cx| cx.notify());
                    });
                }
            },
            |_, _: (), _, _| {},
        )
        .absolute()
        .size_full()
    }
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
            picked: None,
            projects: Vec::new(),
            selected: None,
            writing: false,
            rows: Vec::new(),
            split: cx.new(|_| ResizableState::default()),
            width: unmeasured(),
            reveal: Reveal {
                scroll: gpui::ScrollHandle::new(),
                pending: Default::default(),
                page: cx.entity().downgrade(),
            },
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
    pub(super) fn run_choices(&self) -> Vec<(PathBuf, SharedString)> {
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

/// The list's width before anybody drags it, and the range a drag may take
/// it through: room for a long workflow name on one line beside the head's
/// *New*, never so wide that it crowds the editor.
const LIST: ListWidths = ListWidths::new(20., 16., 30.);

impl Render for WorkflowsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let handle = cx.entity();
        let listed = crate::workflow::templates(cx)
            .len()
            .min(crate::workflow::TEMPLATES_SHOWN);
        while self.rows.len() < listed {
            self.rows.push(cx.focus_handle().tab_stop(true));
        }
        let layout = list_detail(
            self.split.read(cx),
            &LIST,
            self.width.get(),
            window.rem_size(),
        );
        let draft_name = self
            .draft
            .as_ref()
            .map(|draft| draft.name.read(cx).value().to_string());
        let list = div()
            .id("workflows-list")
            .size_full()
            .overflow_y_scroll()
            // Clear of the pane's header, which floats over the page's top.
            .pt(crate::tokens::BAR_H)
            // The rows' fills on this edge and their words a row's inset in,
            // on the line the header's words stand on, as on the Issues page.
            .px_6()
            .pb_6()
            .child(list::list_column(
                &handle,
                self.picked.as_ref(),
                draft_name,
                &self.rows,
                cx,
            ));
        let detail = |under_header: bool| {
            // Only the hint stands in the middle of the region; a form starts
            // at its top, level with the list's head.
            let mut centred = false;
            let body = match &self.draft {
                Some(draft) => form::form(
                    &handle,
                    draft,
                    self.writing,
                    &self.run_choices(),
                    &self.reveal,
                    cx,
                ),
                None => match self.picked_entry(cx) {
                    Some(crate::workflow::Entry {
                        template: Err(why),
                        file: Some(file),
                    }) => form::unreadable(&handle, &why, &file, cx),
                    _ => {
                        centred = true;
                        onehand_plugin_host::hint(
                            "Pick a workflow to see and change it, or make a new one",
                            cx,
                        )
                    }
                },
            };
            let drawer = self
                .draft
                .as_ref()
                .and_then(|draft| step::step_drawer(&handle, draft, draft.place, cx));
            // Centred up to the column a form is read in, its gutters equal,
            // so hiding the list centres it rather than stretching it.
            let form = div()
                .id("workflows-detail")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(&self.reveal.scroll)
                .child(
                    div()
                        .h_flex()
                        .justify_center()
                        .when(!centred, |column| column.items_start())
                        .when(under_header, |column| column.pt(crate::tokens::BAR_H))
                        .when(!under_header, |column| column.pt_6())
                        .px_8()
                        .pb_6()
                        .min_h_full()
                        .child(
                            div()
                                .v_flex()
                                .when(under_header, |column| column.pt_3())
                                .w_full()
                                .max_w(crate::tokens::PAGE_PROSE)
                                .child(body),
                        ),
                );
            // The open step's fields in a drawer docked under the form, which
            // gives up the room it takes: the form above stays in reach, the
            // open step's line kept in view just over the drawer's edge.
            let Some(drawer) = drawer else {
                return form.into_any_element();
            };
            div()
                .size_full()
                .v_flex()
                .child(div().flex_1().min_h_0().child(form))
                .child(
                    div()
                        .flex_none()
                        .h(gpui::relative(DRAWER_SHARE))
                        // A sheet laid over the form's foot: inset from the
                        // region's sides, its top corners rounded, lifted by a
                        // shadow cast upward, on the page's own surface, so it
                        // is the lift and the edges that part it, not a second
                        // fill, its edge a control's.
                        .mx_3()
                        .v_flex()
                        .rounded_t(cx.theme().radius)
                        .border_1()
                        .border_b_0()
                        .border_color(cx.theme().input)
                        .bg(cx.theme().background)
                        .shadow(onehand_plugin_host::tokens::lift_up(cx))
                        .overflow_hidden()
                        .child(
                            div()
                                .id("step-drawer")
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .child(
                                    div().h_flex().justify_center().px_8().py_4().child(
                                        div()
                                            .w_full()
                                            .max_w(crate::tokens::PAGE_PROSE)
                                            .child(drawer),
                                    ),
                                ),
                        ),
                )
                .into_any_element()
        };
        let halves = if layout.side_by_side {
            h_resizable("workflows-split")
                .with_state(&self.split)
                .child(
                    // `flex_none`, or the list grows into the room the editor
                    // was opened to be written in.
                    resizable_panel()
                        .size(layout.start)
                        .size_range(layout.range)
                        .flex_none()
                        .child(
                            div()
                                .size_full()
                                .border_r_1()
                                .border_color(cx.theme().border)
                                .child(list),
                        ),
                )
                .child(resizable_panel().child(detail(true)))
                .into_any_element()
        } else if self.picked.is_some() {
            let back = back_link("workflows-back", "Workflows", cx)
                .on_click(click(&handle, |page, window, cx| {
                    page.close_workflow_form(window, cx)
                }));
            div()
                .size_full()
                .v_flex()
                .child(div().px_8().pt(crate::tokens::BAR_H).child(back))
                .child(div().flex_1().min_h_0().child(detail(false)))
                .into_any_element()
        } else {
            list.into_any_element()
        };
        // A bare page, so it tracks its own focus: a field typed in here
        // must be inside this, or the window's keys land elsewhere.
        div()
            .id("workflows-page")
            .key_context("WorkflowsPage")
            .track_focus(&self.focus_handle)
            .on_action(
                cx.listener(|page: &mut Self, _: &CloseWorkflowForm, window, cx| {
                    page.escape(window, cx);
                }),
            )
            .on_action(cx.listener(|page: &mut Self, _: &WorkflowsUp, window, cx| {
                page.step_pick(-1, window, cx)
            }))
            .on_action(
                cx.listener(|page: &mut Self, _: &WorkflowsDown, window, cx| {
                    page.step_pick(1, window, cx)
                }),
            )
            .relative()
            .size_full()
            .text_size(crate::tokens::CHROME_TEXT)
            .child(measure_width(self.width.clone(), cx.entity().downgrade()))
            .child(halves)
    }
}

/// A click handler that hands `act` the page.
fn click(
    handle: &Entity<WorkflowsPage>,
    act: impl Fn(&mut WorkflowsPage, &mut Window, &mut Context<WorkflowsPage>) + 'static,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let handle = handle.clone();
    move |_, window, cx| handle.update(cx, |page, cx| act(page, window, cx))
}
