//! The board page's columns: the Backlog, the board's own, each issue a row
//! with its line, its one waiting act and its moves, and Done.

use super::column::{ColumnKey, ColumnParts, column, column_head};
use super::rows::Button;
use super::{BoardPage, BoardPageEvent};
use crate::state::Shared;
use gpui::{Context, IntoElement, Stateful};
use gpui_component::ActiveTheme as _;
use onehand_core::board::line::{EntryAct, Line};
use onehand_core::board::offers::{Offer, offers};
use onehand_core::board::{BOARD_SHOWN, Board, Entry, Work};
use onehand_core::project::ProjectId;
use onehand_ui::RowAct;
use std::rc::Rc;

impl BoardPage {
    /// Every column, left to right: the Backlog, the board's own, Done.
    pub(super) fn columns(
        &mut self,
        id: &ProjectId,
        board: &Board,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let view = crate::task::board_view(id, cx);
        // What the rows need of the issues, taken out first so the rows can
        // be drawn while the page is borrowed to make their focus handles.
        let kept = self.issues.as_ref().map(|(_, held)| held.kept(cx));
        let (backlog_rows, backlog_note, titles) = match kept {
            Some(onehand_core::issues::drive::Kept {
                issues: Some(issues),
                forge,
                ..
            }) => {
                let listed: Vec<(u64, String)> =
                    onehand_core::board::backlog(issues, *forge, board, &view.taken)
                        .into_iter()
                        .map(|issue| (issue.number(), issue.title().to_string()))
                        .collect();
                let note = listed.is_empty().then(|| {
                    "No open issues; talk one through with an agent, which can write it here \
                     with create_issue"
                        .to_string()
                });
                let titles: std::collections::HashMap<u64, String> = (board.columns.iter())
                    .flat_map(|c| c.entries.iter())
                    .chain(board.done())
                    .filter_map(|e| Some((e.issue, issues.get(e.issue)?.title().to_string())))
                    .collect();
                (listed, note, titles)
            }
            unread => (
                Vec::new(),
                Some(match unread.and_then(|kept| kept.failed.as_ref()) {
                    Some(why) => format!("The issues could not be read: {why}"),
                    None => "Reading the issues…".to_string(),
                }),
                Default::default(),
            ),
        };
        let title_of = |n: u64| titles.get(&n).cloned().unwrap_or_else(|| format!("#{n}"));
        let first = board
            .columns
            .first()
            .map(|c| (c.id.clone(), c.name.clone()));
        let count = backlog_rows.len();
        let drawn = backlog_rows
            .into_iter()
            .take(BOARD_SHOWN)
            .map(|(n, title)| {
                let acts: Rc<[RowAct]> = first
                    .iter()
                    .map(|(column, name)| {
                        RowAct::new(
                            format!("Move to {name}"),
                            place(id, n, Some(column.clone()), None),
                        )
                    })
                    .collect();
                let line = (format!("#{n}"), crate::theme::meta_ink(cx));
                self.issue_row(n, title, line, acts, None, |row| row, cx)
            })
            .collect();
        let mut columns = vec![
            column(
                ColumnParts {
                    title: "Backlog".into(),
                    count,
                    rows: drawn,
                    note: backlog_note,
                    control: None,
                    scroll: self.column_scroll(&ColumnKey::Backlog),
                    key: ColumnKey::Backlog,
                },
                cx,
            )
            .into_any_element(),
        ];
        let now = onehand_core::chat::now_secs();
        let templates = crate::workflow::templates(cx);
        for (at, col) in board.columns.iter().enumerate() {
            let gone = match &col.work {
                Work::Workflow { id } => crate::workflow::find(&templates, id).is_none(),
                Work::Person => false,
            };
            let drawn = (col.entries.iter())
                .take(BOARD_SHOWN)
                .map(|entry| {
                    self.entry_row(id, board, entry, &view, title_of(entry.issue), now, cx)
                })
                .collect();
            let key = ColumnKey::Board(col.id.clone());
            let more = self.column_more(at, col, cx);
            columns.push(
                column(
                    ColumnParts {
                        title: column_head(col, gone).into(),
                        count: col.entries.len(),
                        rows: drawn,
                        note: col.entries.is_empty().then(|| "Nothing here".to_string()),
                        control: Some(more),
                        scroll: self.column_scroll(&key),
                        key,
                    },
                    cx,
                )
                .into_any_element(),
            );
        }
        let done: Vec<gpui::AnyElement> = (board.done().iter())
            .take(BOARD_SHOWN)
            .map(|entry| self.done_row(entry, &view, title_of(entry.issue), cx))
            .collect();
        columns.push(
            column(
                ColumnParts {
                    title: "Done".into(),
                    count: board.done().len(),
                    note: done.is_empty().then(|| "Nothing done yet".to_string()),
                    rows: done,
                    control: None,
                    scroll: self.column_scroll(&ColumnKey::Done),
                    key: ColumnKey::Done,
                },
                cx,
            )
            .into_any_element(),
        );
        columns
    }

    /// The row of `entry` of `board`: a task's when a kept task works it, an
    /// issue's else, its line from where it stands, its one waiting act drawn
    /// and its moves in its menu.
    #[allow(clippy::too_many_arguments)]
    fn entry_row(
        &mut self,
        id: &ProjectId,
        board: &Board,
        entry: &Entry,
        view: &crate::task::BoardView,
        title: String,
        now: u64,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let issue = entry.issue;
        let task = entry.task.as_deref();
        let row = task.and_then(|t| view.rows.get(t)).cloned();
        let working = task.is_some_and(|t| crate::task::is_working(t, cx));
        let run_here = task
            .and_then(|t| crate::task::task(t, cx))
            .is_some_and(|t| t.runs().iter().any(|r| Some(&r.id) == entry.run.as_ref()));
        let starting = Shared::global(cx).boards.starting(id).any(|n| n == issue);
        let (line, act) =
            onehand_core::board::line::line(board, issue, run_here, working, starting);
        let offered = offers(board, issue, act.as_ref(), working);
        let has_task = row.is_some();
        let button = act.as_ref().and_then(|act| {
            let (offer, refused) = offered.iter().find(|(o, _)| is_act(o, act))?.clone();
            Some(Button {
                label: offer.words(has_task),
                press: press(id, board, issue, &offer),
                refused,
            })
        });
        let moves: Vec<RowAct> = (offered.iter())
            .map(|(offer, refused)| {
                let act = RowAct::new(offer.words(has_task), press(id, board, issue, offer))
                    .refuses(refused.is_some());
                match offer {
                    Offer::Back => act.danger(),
                    Offer::Start | Offer::MoveTo { .. } | Offer::Up | Offer::Down => act,
                }
            })
            .collect();
        let words = match &line {
            Line::Task => None,
            Line::Since(since) => Some(format!("#{issue} · {}", onehand_core::age(now, *since))),
            Line::WaitingForStart
            | Line::WaitingForTeam
            | Line::Starting
            | Line::DoneHere
            | Line::WaitingForRoom(_)
            | Line::DidNotStart => Some(line.words()),
        };
        let drags = |row: Stateful<gpui::Div>| row;
        match row {
            Some(row) => self.task_row(row, words, moves, button, drags, cx),
            None => {
                let ink = match line {
                    Line::DidNotStart => crate::theme::status_ink(cx).warning,
                    Line::Since(_) => crate::theme::meta_ink(cx),
                    Line::Task
                    | Line::WaitingForStart
                    | Line::WaitingForTeam
                    | Line::Starting
                    | Line::DoneHere
                    | Line::WaitingForRoom(_) => cx.theme().muted_foreground,
                };
                let line = (words.unwrap_or_else(|| format!("#{issue}")), ink);
                self.issue_row(issue, title, line, moves.into(), button, drags, cx)
            }
        }
    }

    /// A row of Done: its task's, which it opens, or its issue's.
    fn done_row(
        &mut self,
        entry: &Entry,
        view: &crate::task::BoardView,
        title: String,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let row = entry
            .task
            .as_deref()
            .and_then(|t| view.rows.get(t))
            .cloned();
        match row {
            Some(row) => self.task_row(row, None, Vec::new(), None, |row| row, cx),
            None => {
                let line = (format!("#{}", entry.issue), crate::theme::meta_ink(cx));
                self.issue_row(entry.issue, title, line, Rc::new([]), None, |row| row, cx)
            }
        }
    }

    /// A column's `⋯`: *Edit column…*, and *Edit workflow* on one that runs
    /// a workflow.
    fn column_more(
        &mut self,
        at: usize,
        col: &onehand_core::board::Column,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let page = cx.entity().downgrade();
        let column = col.id.clone();
        let edit: onehand_ui::Press = {
            let page = page.clone();
            Rc::new(move |_, _, cx| {
                let column = column.clone();
                let _ = page.update(cx, |_, cx| cx.emit(BoardPageEvent::EditColumn { column }));
            })
        };
        let mut acts = vec![RowAct::new("Edit column…", edit)];
        if let Work::Workflow { id } = &col.work {
            let workflow = id.clone();
            let open: onehand_ui::Press = Rc::new(move |_, _, cx| {
                let workflow = workflow.clone();
                let _ = page.update(cx, |_, cx| {
                    cx.emit(BoardPageEvent::EditWorkflow { workflow })
                });
            });
            acts.push(RowAct::new("Edit workflow", open));
        }
        onehand_ui::row_more(
            format!("board-column-{at}-more"),
            "Column actions",
            acts.into(),
            cx,
        )
        .into_any_element()
    }
}

/// Whether `offer` is what an entry's waiting `act` offers.
fn is_act(offer: &Offer, act: &EntryAct) -> bool {
    match (offer, act) {
        (Offer::Start, EntryAct::Start) => true,
        (Offer::MoveTo { column, .. }, EntryAct::MoveTo(to)) => column == to,
        (Offer::Start | Offer::MoveTo { .. } | Offer::Up | Offer::Down | Offer::Back, _) => false,
    }
}

/// The press of `offer` on `issue` of project `id`'s `board`.
fn press(id: &ProjectId, board: &Board, issue: u64, offer: &Offer) -> onehand_ui::Press {
    let id = id.clone();
    match offer {
        Offer::Start => {
            Rc::new(move |_, window, cx| crate::board::start_on_press(&id, issue, window, cx))
        }
        Offer::MoveTo { column, .. } => place(&id, issue, column.clone(), None),
        Offer::Up | Offer::Down => {
            let Some((at, _)) = board.at(issue) else {
                return Rc::new(|_, _, _| {});
            };
            let column = &board.columns[at];
            let place_at = column
                .entries
                .iter()
                .position(|e| e.issue == issue)
                .unwrap_or(0);
            // Just before the one above, or before the one two below: the
            // one below then stands above it.
            let before = match offer {
                Offer::Up => place_at.checked_sub(1).map(|i| column.entries[i].issue),
                Offer::Start | Offer::MoveTo { .. } | Offer::Back | Offer::Down => {
                    column.entries.get(place_at + 2).map(|e| e.issue)
                }
            };
            place(&id, issue, Some(column.id.clone()), before)
        }
        Offer::Back => {
            let task = (board.at(issue)).and_then(|(_, e)| e.task.clone());
            Rc::new(move |_, window, cx| {
                crate::board::remove(&id, issue, task.as_deref(), window, cx)
            })
        }
    }
}

/// A press putting `issue` of project `id`'s board in column `to`, or Done.
fn place(id: &ProjectId, issue: u64, to: Option<String>, before: Option<u64>) -> onehand_ui::Press {
    let id = id.clone();
    Rc::new(move |_, window, cx| crate::board::place(&id, issue, to.clone(), before, window, cx))
}
