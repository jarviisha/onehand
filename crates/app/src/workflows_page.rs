//! The Workflows page: every workflow on offer as a card, *Run…* on each,
//! and the editor a workflow is written in, a page of its own.
//!
//! **The draft is the page's**, held on this entity rather than in the
//! render, and the entity is the window's for as long as the window is, so
//! leaving for another page and coming back finds an edit as it was left.
//! Dropping it is always asked first: going back to the cards over unsaved
//! changes, or closing the window on them.
//!
//! **No second start form.** *Run…* asks which project, the one selected in
//! the rail first, and hands the choice to the window, which opens the one
//! launcher the rail and the keymap command open.

mod actions;
mod draft;
mod editor;
mod form;
mod grid;
mod pick;
mod prompt_dialog;
mod step;

pub(crate) use draft::WorkflowDraft;

// The Workflows page's keys: the innermost thing open closed (the step
// open, then the editor, asked first when it holds changes), the open
// step stepped to the step before or after. A card opens on Enter as any
// pressable element does, so no binding takes the key from a button.
gpui::actions!(
    workflows_page,
    [CloseWorkflowForm, WorkflowStepUp, WorkflowStepDown]
);

use gpui::{
    App, AppContext as _, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div,
};
use gpui_component::StyledExt;
use gpui_component::input::{InputEvent, InputState};
use onehand_ui::{Measured, measure_width, unmeasured};
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
    /// The workflow open on the page, by key: the workflows are read again
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
        // A workflow open, or a file of the person's own that cannot be
        // read, takes the page; else the cards.
        let unreadable = match (&self.draft, self.picked_entry(cx)) {
            (
                None,
                Some(crate::workflow::Entry {
                    template: Err(why),
                    file: Some(file),
                }),
            ) => Some(form::unreadable(&handle, &why, &file, cx)),
            _ => None,
        };
        if let Some(draft) = &mut self.draft {
            draft.adopt_offer(window, cx);
        }
        let halves = if let Some(draft) = &self.draft {
            editor::editor(
                &handle,
                draft,
                editor::editor_layout(self.width.get()),
                self.writing,
                &self.run_choices(),
                cx,
            )
        } else if let Some(view) = unreadable {
            editor::unreadable(&handle, view, cx)
        } else {
            // A column page under the floating header: the grid as wide as
            // boxes side by side are, centred, its scroll the page's.
            // The page's width less its `px_8` gutters, up to the column's.
            let grid_width = gpui::rems((self.width.get().0 - 4.).min(crate::tokens::PAGE_WIDE.0));
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
                        .child(div().w_full().max_w(crate::tokens::PAGE_WIDE).pt_3().child(
                            grid::grid_page(
                                &handle,
                                &query,
                                &self.cards,
                                &self.run_choices(),
                                grid_width,
                                cx,
                            ),
                        )),
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
