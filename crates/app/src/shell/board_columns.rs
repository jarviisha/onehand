//! A board's columns, as the shell changes them from Settings: the form
//! that adds or changes one, and a column moved or removed. Whether a change
//! goes, and what refuses it, is the board's drive's.

use super::Shell;
use gpui::{AppContext as _, BorrowAppContext as _, Context, Entity, Focusable as _, Window};
use gpui_component::input::InputState;
use onehand_core::board::model::Column;
use onehand_core::board::{MovesOn, Starts, Work};
use onehand_core::project::Toward;
use std::path::PathBuf;

/// The column form's fields, for the project at `root`.
pub(crate) struct ColumnDraft {
    pub(crate) root: PathBuf,
    pub(crate) label: String,
    /// The id of the column being changed; none for one being added.
    pub(crate) editing: Option<String>,
    pub(crate) name: Entity<InputState>,
    /// A person works its issues, rather than a workflow.
    pub(crate) person: bool,
    /// The workflow it runs, by id, once picked.
    pub(crate) workflow: Option<String>,
    pub(crate) starts: Starts,
    pub(crate) moves_on: MovesOn,
    /// Empty for no limit.
    pub(crate) limit: Entity<InputState>,
    /// What the last *Add* or *Save* ran into, until the next one.
    pub(crate) error: Option<String>,
}

impl Shell {
    pub(crate) fn column_draft(&self) -> Option<&ColumnDraft> {
        self.column_draft.as_ref()
    }

    /// Open the form on the board of the project at `root`: on its column
    /// with id `editing`, or empty to add one, a person's that moves its
    /// issues on by itself.
    pub(crate) fn open_column_form(
        &mut self,
        root: PathBuf,
        editing: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.column_draft.is_some() {
            return;
        }
        let Some(id) = self.project_of_row(&root) else {
            return;
        };
        let held = editing.as_ref().and_then(|editing| {
            let boards = &crate::state::Shared::global(cx).boards;
            boards.board(&id)?.column(editing).cloned()
        });
        if editing.is_some() && held.is_none() {
            return;
        }
        let label = self
            .root_index(&root)
            .map(|at| self.window.workspace.roots[at].label.clone())
            .unwrap_or_default();
        let (name, limit) = match &held {
            Some(column) => (
                column.name.clone(),
                column.limit.map(|n| n.to_string()).unwrap_or_default(),
            ),
            None => Default::default(),
        };
        let (person, workflow) = match held.as_ref().map(|c| &c.work) {
            Some(Work::Workflow { id }) => (false, Some(id.clone())),
            Some(Work::Person) | None => (true, None),
        };
        self.remember_focus(window, cx);
        let name = cx.new(|cx| InputState::new(window, cx).default_value(name));
        let limit = cx.new(|cx| InputState::new(window, cx).default_value(limit));
        name.focus_handle(cx).focus(window, cx);
        self.column_draft = Some(ColumnDraft {
            root,
            label,
            editing,
            name,
            person,
            workflow,
            starts: held.as_ref().map_or(Starts::OnEntry, |c| c.starts),
            moves_on: held.as_ref().map_or(MovesOn::ByItself, |c| c.moves_on),
            limit,
            error: None,
        });
        cx.notify();
    }

    /// The form's column works its issues by a person (`person`), or runs a
    /// workflow.
    pub(crate) fn set_column_work(&mut self, person: bool, cx: &mut Context<Self>) {
        self.edit_column_draft(cx, |draft| draft.person = person);
    }

    /// The form's column runs the workflow with id `workflow`.
    pub(crate) fn set_column_workflow(&mut self, workflow: String, cx: &mut Context<Self>) {
        self.edit_column_draft(cx, |draft| {
            draft.person = false;
            draft.workflow = Some(workflow);
        });
    }

    pub(crate) fn set_column_starts(&mut self, starts: Starts, cx: &mut Context<Self>) {
        self.edit_column_draft(cx, |draft| draft.starts = starts);
    }

    pub(crate) fn set_column_moves_on(&mut self, moves_on: MovesOn, cx: &mut Context<Self>) {
        self.edit_column_draft(cx, |draft| draft.moves_on = moves_on);
    }

    pub(crate) fn cancel_column(&mut self, cx: &mut Context<Self>) {
        if self.column_draft.take().is_some() {
            cx.notify();
        }
    }

    /// *Add* or *Save*: read the form as a column and ask the board's drive
    /// to make it one of the board's, last, or in place of the one being
    /// changed. What refuses it is said in the form, which stays.
    pub(crate) fn save_column(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.column_draft.as_ref() else {
            return;
        };
        let Some(id) = self.project_of_row(&draft.root) else {
            return;
        };
        let read = read_column(draft, cx);
        let editing = draft.editing.clone();
        // Read first: the change below borrows the app whole.
        let read = read.and_then(|column| {
            match &column.work {
                Work::Person => {}
                Work::Workflow { id } => {
                    let template = crate::control::template(id, cx)?;
                    onehand_core::board::usable(Some(&template))?;
                }
            }
            Ok(column)
        });
        let busy: std::collections::HashSet<String> = (crate::state::Shared::global(cx).boards.board(&id))
            .into_iter()
            .flat_map(|board| board.columns.iter())
            .flat_map(|column| column.entries.iter())
            .filter_map(|entry| entry.task.clone())
            .filter(|task| crate::task::is_working(task, cx))
            .collect();
        let changed = read.and_then(|column| {
            let working = |task: &str| busy.contains(task);
            let ok = |_: &Work| Ok(());
            cx.update_global::<crate::state::Shared, _>(|shared, _| match &editing {
                Some(editing) => shared.boards.edit_column(&id, editing, column, &working, &ok),
                None => shared.boards.add_column(&id, column, usize::MAX, &ok),
            })
        });
        match changed {
            Ok(fx) => {
                self.column_draft = None;
                crate::board::carry(fx, cx);
            }
            Err(why) => {
                if let Some(draft) = self.column_draft.as_mut() {
                    draft.error = Some(why);
                }
            }
        }
        cx.notify();
    }

    /// Move the column with id `column` of the board of the project at
    /// `root` one place left (up) or right.
    pub(crate) fn move_board_column(
        &mut self,
        root: PathBuf,
        column: String,
        toward: Toward,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.project_of_row(&root) else {
            return;
        };
        let fx = cx.update_global::<crate::state::Shared, _>(|shared, _| {
            shared.boards.move_column(&id, &column, toward)
        });
        crate::board::carry(fx, cx);
        cx.notify();
    }

    /// Take the column with id `column` off the board of the project at
    /// `root`; what refuses it stands on the page.
    pub(crate) fn remove_board_column(
        &mut self,
        root: PathBuf,
        column: String,
        cx: &mut Context<Self>,
    ) {
        self.board_refused = None;
        let Some(id) = self.project_of_row(&root) else {
            return;
        };
        let removed = cx.update_global::<crate::state::Shared, _>(|shared, _| {
            shared.boards.remove_column(&id, &column)
        });
        match removed {
            Ok(fx) => crate::board::carry(fx, cx),
            Err(why) => self.board_refused = Some((root, why)),
        }
        cx.notify();
    }

    fn edit_column_draft(&mut self, cx: &mut Context<Self>, edit: impl FnOnce(&mut ColumnDraft)) {
        if let Some(draft) = self.column_draft.as_mut() {
            draft.error = None;
            edit(draft);
            cx.notify();
        }
    }
}

/// The column `draft` describes, its id kept when it edits one and made new
/// when it adds one, or why it is not one.
fn read_column(draft: &ColumnDraft, cx: &gpui::App) -> Result<Column, String> {
    let name = draft.name.read(cx).value().trim().to_string();
    if name.is_empty() {
        return Err("Give the column a name".to_string());
    }
    let limit = draft.limit.read(cx).value().trim().to_string();
    let limit = match limit.is_empty() {
        true => None,
        false => match limit.parse::<u32>() {
            Ok(n) if n >= 1 => Some(n),
            Ok(_) | Err(_) => return Err("Limit is a whole number of at least 1".to_string()),
        },
    };
    let work = match (draft.person, &draft.workflow) {
        (true, _) => Work::Person,
        (false, Some(id)) => Work::Workflow { id: id.clone() },
        (false, None) => return Err("Pick the workflow it runs".to_string()),
    };
    Ok(Column {
        id: draft.editing.clone().unwrap_or_else(crate::board::column_id),
        name,
        work,
        starts: draft.starts,
        moves_on: draft.moves_on,
        limit,
        entries: Vec::new(),
    })
}

#[cfg(test)]
mod tests;
