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
mod grid;
mod pick;
mod prompt_dialog;
mod step;

pub(crate) use draft::WorkflowDraft;

// The Workflows page's keys: the innermost thing open closed (the step
// drawer, then the form, asked first when it holds changes), the step
// drawer stepped to the step before or after, and the card the keyboard is
// on opened.
gpui::actions!(
    workflows_page,
    [
        CloseWorkflowForm,
        WorkflowStepUp,
        WorkflowStepDown,
        OpenWorkflowCard
    ]
);

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div,
};
use gpui_component::{ActiveTheme, StyledExt};
use gpui_component::input::{InputEvent, InputState};
use onehand_plugin_host::{Measured, back_link, measure_width, unmeasured};
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
    /// One focus handle per card shown, by place: each card is a tab stop,
    /// so the keyboard reaches it as the pointer does.
    cards: Vec<FocusHandle>,
    /// The grid's search, made with the first frame that has a window, and
    /// kept for as long as the page is: leaving and coming back finds it as
    /// it was left.
    query: Option<Entity<InputState>>,
    /// The page's width as last laid out, which decides how many cards a
    /// row holds.
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

/// The most of the editor's height the step drawer takes: the rest is left
/// to the form above, enough for the open step's line and its neighbours.
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
            cards: Vec::new(),
            query: None,
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

impl WorkflowsPage {
    /// The grid's search, made on first use: an input needs a window.
    fn query(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(query) = &self.query {
            return query.clone();
        }
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search workflows"));
        cx.subscribe(&query, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        self.query = Some(query.clone());
        query
    }

    /// Enter: open the card the keyboard is on, if it is on one.
    fn open_focused_card(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picked.is_some() {
            return;
        }
        let Some(place) = self.cards.iter().position(|card| card.is_focused(window)) else {
            return;
        };
        let text = self
            .query
            .as_ref()
            .map(|query| query.read(cx).value().to_string())
            .unwrap_or_default();
        let entries = crate::workflow::templates(cx);
        let listed = pick::listed(&entries, &text);
        let key = listed
            .shown
            .get(place)
            .and_then(|&at| pick::pick_of(&entries[at]));
        if let Some(key) = key {
            self.pick(key, window, cx);
        }
    }
}

impl Render for WorkflowsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let handle = cx.entity();
        let query = self.query(window, cx);
        let listed = crate::workflow::templates(cx)
            .len()
            .min(crate::workflow::TEMPLATES_SHOWN);
        while self.cards.len() < listed {
            self.cards.push(cx.focus_handle().tab_stop(true));
        }
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
            // Beside the list the region starts under the floating header,
            // as the list does, so the form never scrolls behind its controls.
            let region = div()
                .size_full()
                .v_flex()
                .when(under_header, |region| region.pt(crate::tokens::BAR_H));
            let Some(drawer) = drawer else {
                return region
                    .child(div().flex_1().min_h_0().child(form))
                    .into_any_element();
            };
            region
                .child(div().flex_1().min_h_0().child(form))
                .child(
                    div()
                        .flex_none()
                        // As tall as the open step's fields, up to its share:
                        // a step with few fields leaves the form the room.
                        .max_h(gpui::relative(DRAWER_SHARE))
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
                            div().id("step-drawer").min_h_0().overflow_y_scroll().child(
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
        let halves = if self.picked.is_some() {
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
            // A column page under the floating header: the grid as wide as
            // boxes side by side are, centred, its scroll the page's.
            // The page's width less its `px_8` gutters, up to the column's.
            let grid_width =
                gpui::rems((self.width.get().0 - 4.).min(crate::tokens::PAGE_WIDE.0));
            div()
                .id("workflows-grid")
                .size_full()
                .overflow_y_scroll()
                .child(
                    div()
                        .h_flex()
                        .justify_center()
                        .px_8()
                        .pt(crate::tokens::BAR_H)
                        .pb_6()
                        .child(
                            div()
                                .w_full()
                                .max_w(crate::tokens::PAGE_WIDE)
                                .pt_3()
                                .child(grid::grid_page(
                                    &handle,
                                    &query,
                                    &self.cards,
                                    &self.run_choices(),
                                    grid_width,
                                    cx,
                                )),
                        ),
                )
                .into_any_element()
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
            .on_action(
                cx.listener(|page: &mut Self, _: &WorkflowStepUp, window, cx| {
                    page.step_open(-1, window, cx)
                }),
            )
            .on_action(
                cx.listener(|page: &mut Self, _: &WorkflowStepDown, window, cx| {
                    page.step_open(1, window, cx)
                }),
            )
            .on_action(
                cx.listener(|page: &mut Self, _: &OpenWorkflowCard, window, cx| {
                    page.open_focused_card(window, cx)
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
