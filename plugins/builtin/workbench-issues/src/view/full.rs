//! What the Issues page draws of an issue that the tab does not: the steps
//! still to come, and what the work left as read off git — whether its check
//! still vouches for the work, the files this run and the branch changed,
//! each opening its diff in place, and the commits past where it started.
//!
//! Read on the background executor, never in a render, keyed to the task and
//! run it was read for, so an answer about another never shows.

use super::IssuesView;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement,
    StatefulInteractiveElement as _, Styled, div,
};
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::diff::Row as DiffRow;
use onehand_core::issues;
use onehand_core::task::marks::{self, Change};
use onehand_core::task::work::left::{Left, left_blocking};
use onehand_core::task::work::{IssueWork, Reading, Work};
use onehand_plugin_host::status_ink;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// How many changed files a list draws.
const FILES_SHOWN: usize = 100;

/// How many lines of one file's diff are drawn.
const DIFF_LINES: usize = 400;

/// The work a read is about: a retry that starts a new run makes the old
/// run's read stale.
type About = (String, Option<String>);

/// One file's diff: the folder it is read in, the two marks, the path.
type DiffKey = (PathBuf, String, String, String);

/// What the page holds of the issue on screen's full form.
#[derive(Default)]
pub(super) struct FullState {
    left: Reading<About, Left>,
    /// Where the work stood when it was last read, so a step moving reads it
    /// again.
    seen: Option<String>,
    /// Which file lists are open: this run's, the branch's.
    open: HashSet<Side>,
    /// Diffs opened, and what each read came to; `None` while it is read.
    diffs: HashMap<DiffKey, Option<Result<Vec<DiffRow>, String>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Side {
    Run,
    Branch,
}

/// What the full form draws, borrowed from the page.
pub(super) struct Full<'a> {
    state: &'a FullState,
}

impl FullState {
    pub(super) fn view(&self) -> Full<'_> {
        Full { state: self }
    }
}

/// Where `work` stands, in one string a move changes.
fn standing(work: &Work) -> String {
    format!(
        "{}|{:?}|{:?}|{}|{:?}",
        work.task,
        work.run,
        work.step.as_ref().map(|step| step.at),
        work.said,
        work.span
    )
}

impl IssuesView {
    /// Read what `work` left when it is due: never read for it, or it moved.
    pub(super) fn read_left_if_due(&mut self, work: Option<&IssueWork>, cx: &mut Context<Self>) {
        let Some(page) = self.page.as_mut() else {
            return;
        };
        let Some(work) = work.map(|work| &work.work) else {
            return;
        };
        let now = standing(work);
        if page.full.seen.as_ref() == Some(&now) {
            return;
        }
        page.full.seen = Some(now);
        self.read_left(work.clone(), cx);
    }

    /// Read what `work` left now.
    pub(super) fn read_left(&mut self, work: Work, cx: &mut Context<Self>) {
        let Some(page) = self.page.as_mut() else {
            return;
        };
        let generation = page.full.left.ask((work.task.clone(), work.run.clone()));
        cx.spawn(async move |view, cx| {
            let left = cx
                .background_executor()
                .spawn(async move { left_blocking(&work) })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                if let Some(page) = view.page.as_mut()
                    && page.full.left.land(generation, Ok(left), issues::now())
                {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Read again what the issue on screen left, asked by a person.
    pub(super) fn refresh_left(&mut self, work: Option<&IssueWork>, cx: &mut Context<Self>) {
        if let Some(work) = work {
            self.read_left(work.work.clone(), cx);
        }
    }

    fn toggle_side(&mut self, side: Side, cx: &mut Context<Self>) {
        if let Some(page) = self.page.as_mut() {
            if !page.full.open.remove(&side) {
                page.full.open.insert(side);
            }
            cx.notify();
        }
    }

    /// Open or close one file's diff, reading it the first time.
    fn toggle_diff(&mut self, key: DiffKey, cx: &mut Context<Self>) {
        let Some(page) = self.page.as_mut() else {
            return;
        };
        if page.full.diffs.remove(&key).is_some() {
            cx.notify();
            return;
        }
        page.full.diffs.insert(key.clone(), None);
        cx.notify();
        cx.spawn(async move |view, cx| {
            let read = cx
                .background_executor()
                .spawn({
                    let (dir, from, to, path) = key.clone();
                    async move { marks::file_diff_blocking(&dir, &from, &to, &path) }
                })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                if let Some(slot) = view
                    .page
                    .as_mut()
                    .and_then(|page| page.full.diffs.get_mut(&key))
                {
                    *slot = Some(read);
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

/// The steps after the one the work is at, for the line under its progress.
/// A space when there are none, so the line keeps its height.
pub(super) fn rest_line(work: Option<&Work>, cx: &Context<IssuesView>) -> AnyElement {
    let rest = work.map(|work| work.rest.join(" · ")).unwrap_or_default();
    div()
        .min_w_0()
        .truncate()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(if rest.is_empty() {
            "\u{a0}".to_string()
        } else {
            format!("Then: {rest}")
        })
        .into_any_element()
}

/// The lines the full form adds under *What the work left*: the check, the
/// files of this run and of the branch, and the commits past the base.
pub(super) fn left_lines(
    full: &Full<'_>,
    work: &Work,
    line: &dyn Fn(&'static str, AnyElement) -> AnyElement,
    cx: &mut Context<IssuesView>,
) -> Vec<AnyElement> {
    let muted = cx.theme().muted_foreground;
    let warning = status_ink(cx).warning;
    let reading = &full.state.left;
    let ours = reading.about() == Some(&(work.task.clone(), work.run.clone()));
    let Some((left, at)) = reading.value.as_ref().filter(|_| ours) else {
        let said = match reading.failed.as_deref().filter(|_| ours) {
            Some(why) => div()
                .text_color(warning)
                .child(format!("could not be read: {why}")),
            None => div().text_color(muted).child("reading…"),
        };
        return vec![line("Check", said.into_any_element())];
    };
    let now = issues::now();
    let mut lines = vec![line(
        "Check",
        match &left.check {
            Ok(check) => div().truncate().child(format!(
                "{} · read {}",
                check.said(),
                onehand_core::rel_time(now, *at)
            )),
            Err(why) => div()
                .truncate()
                .text_color(warning)
                .child(format!("could not be read: {why}")),
        }
        .into_any_element(),
    )];
    let marks_of = |side: Side| match side {
        Side::Run => work.span.clone(),
        Side::Branch => work.branch_span(),
    };
    for (name, side, files) in [
        ("This run", Side::Run, &left.run_files),
        ("Whole branch", Side::Branch, &left.branch_files),
    ] {
        let value = match files {
            None => div()
                .text_color(muted)
                .child("not recorded for this run")
                .into_any_element(),
            Some(Err(why)) => div()
                .truncate()
                .text_color(warning)
                .child(format!("could not be read: {why}"))
                .into_any_element(),
            Some(Ok(files)) => files_view(full, side, files, &work.dir, marks_of(side), cx),
        };
        lines.push(line(name, value));
    }
    lines.push(line(
        "Commits",
        match &left.commits {
            None => div().text_color(muted).child("not recorded for this run"),
            Some(Ok(n)) => div().child(match n {
                1 => "1 past where the task started".to_string(),
                n => format!("{n} past where the task started"),
            }),
            Some(Err(why)) => div()
                .truncate()
                .text_color(warning)
                .child(format!("could not be read: {why}")),
        }
        .into_any_element(),
    ));
    lines
}

/// A count of changed files that opens into the list, each file opening its
/// diff in place.
fn files_view(
    full: &Full<'_>,
    side: Side,
    files: &[Change],
    dir: &std::path::Path,
    marks: Option<(String, String)>,
    cx: &mut Context<IssuesView>,
) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let open = full.state.open.contains(&side);
    let count = match files.len() {
        0 => "no files changed".to_string(),
        1 => "1 file changed".to_string(),
        n => format!("{n} files changed"),
    };
    let id = match side {
        Side::Run => "issue-full-run-files",
        Side::Branch => "issue-full-branch-files",
    };
    let left_out = files.len().saturating_sub(FILES_SHOWN);
    div()
        .v_flex()
        .gap_0p5()
        .min_w_0()
        .child(
            div()
                .id(id)
                .h_flex()
                .gap_1()
                .items_center()
                .cursor_pointer()
                .when(!files.is_empty(), |row| row.hover(|row| row.underline()))
                .child(count)
                .when(!files.is_empty(), |row| {
                    row.child(
                        Icon::new(if open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .xsmall()
                        .text_color(muted),
                    )
                })
                .on_click(
                    cx.listener(move |view, _: &ClickEvent, _, cx| view.toggle_side(side, cx)),
                ),
        )
        .when(open, |column| {
            column
                .children(
                    files
                        .iter()
                        .take(FILES_SHOWN)
                        .enumerate()
                        .map(|(i, change)| {
                            let key = marks.clone().map(|(from, to)| {
                                (dir.to_path_buf(), from, to, change.path.clone())
                            });
                            file_row(full, side, i, change, key, cx)
                        }),
                )
                .when(left_out > 0, |column| {
                    column.child(
                        div()
                            .text_color(muted)
                            .child(format!("… {left_out} more not shown")),
                    )
                })
        })
        .into_any_element()
}

/// One changed file, and its diff under it once opened.
fn file_row(
    full: &Full<'_>,
    side: Side,
    i: usize,
    change: &Change,
    key: Option<DiffKey>,
    cx: &mut Context<IssuesView>,
) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let ink = status_ink(cx);
    let counts = change.lines.map_or_else(
        || "binary".to_string(),
        |(added, removed)| format!("+{added} −{removed}"),
    );
    let diff = key.as_ref().and_then(|key| full.state.diffs.get(key));
    let row =
        div()
            .id((
                match side {
                    Side::Run => "issue-full-run-file",
                    Side::Branch => "issue-full-branch-file",
                },
                i,
            ))
            .h_flex()
            .gap_2()
            .min_w_0()
            .px_1()
            .rounded(cx.theme().radius)
            .font_family(cx.theme().mono_font_family.clone())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(change.path.clone()),
            )
            .child(div().flex_none().text_color(muted).child(counts))
            .when_some(key, |row, key| {
                row.cursor_pointer()
                    .hover(|row| row.bg(cx.theme().list_hover))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.toggle_diff(key.clone(), cx)
                    }))
            });
    div()
        .v_flex()
        .min_w_0()
        .child(row)
        .children(diff.map(|read| {
            match read {
                None => div()
                    .px_2()
                    .text_color(muted)
                    .child("reading…")
                    .into_any_element(),
                Some(Err(why)) => div()
                    .px_2()
                    .text_color(ink.warning)
                    .child(format!("could not be read: {why}"))
                    .into_any_element(),
                Some(Ok(rows)) => diff_view(rows, cx),
            }
        }))
        .into_any_element()
}

/// One file's diff, a line per row, the first [`DIFF_LINES`] of them.
fn diff_view(rows: &[DiffRow], cx: &Context<IssuesView>) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let ink = status_ink(cx);
    let left_out = rows.len().saturating_sub(DIFF_LINES);
    div()
        .v_flex()
        .min_w_0()
        .my_1()
        .rounded(cx.theme().radius)
        .bg(cx.theme().muted)
        .font_family(cx.theme().mono_font_family.clone())
        .children(rows.iter().take(DIFF_LINES).map(|row| {
            let (sign, text, color) = match row {
                DiffRow::Added(text) => ("+", text.clone(), ink.success),
                DiffRow::Removed(text) => ("−", text.clone(), ink.danger),
                DiffRow::Context(text) => (" ", text.clone(), cx.theme().foreground),
                DiffRow::Skipped(n) => (" ", format!("{n} unchanged lines"), muted),
            };
            div()
                .h_flex()
                .items_start()
                .min_w_0()
                .px_1()
                .text_color(color)
                .child(div().flex_none().w(gpui::rems(1.)).child(sign))
                .child(div().flex_1().min_w_0().child(text))
        }))
        .when(left_out > 0, |column| {
            column.child(
                div()
                    .px_1()
                    .text_color(muted)
                    .child(format!("… {left_out} more lines not shown")),
            )
        })
        .into_any_element()
}
