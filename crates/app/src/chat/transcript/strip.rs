use super::fold_key;
use super::metrics::{FOLD_INDENT, TEXT_SM};
use super::parts::{
    ActivityRow, Object, RowMark, activity_row, chevron, detail_well, fold_line, line_counts,
    plain_box, show_more, sideways,
};
use super::tool::diff_rows;
use crate::chat::session::ChatSession;
use crate::controls::Quiet as _;
use crate::controls::Short as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, ClickEvent, Entity, InteractiveElement, IntoElement, ParentElement, Rems, SharedString,
    StatefulInteractiveElement, Styled, Window, div, rems,
};
use gpui_component::{ActiveTheme, IconName, StyledExt};
use onehand_core::chat::ChatItem;
use onehand_core::chat::activity;
use onehand_core::diff::Row as DiffRow;

// ── activity strip ──────────────────────────────────────────────────────────

/// The kind a stretch of one cluster's members shares, for the rows drawn
/// inside the opened frame.
///
/// The cluster's own line no longer needs this — it names kinds of work in its
/// sentence and carries the *status* in its mark — but the frame under it is
/// still a list grouped by what the work was.
/// The kind a run of adjacent members shares, for the rows the frame lists —
/// and `None` for a step that stands on its own whatever is beside it.
///
/// **Only reads merge.** Three files looked at in a row are one thing the agent
/// did, and `Read 3 files` opening into the three paths is what a reader wants
/// of them. Two edits are not: each carries its own diff, which is the thing
/// somebody opened the block to see, and folding them behind one row puts the
/// diff two clicks away to save a line. The same goes for two commands, whose
/// output is the point of each.
pub fn section_group(item: &ChatItem) -> Option<activity::ActivityGroup> {
    match item {
        ChatItem::Tool(tool)
            if activity::presentation(tool).kind == activity::ActivityKind::Inspect =>
        {
            activity::group(item)
        }
        _ => None,
    }
}

/// A cluster of activity, drawn as one line that opens into a row per step.
///
/// **The line is the whole of what the transcript shows by default** while
/// the work is done: everything the agent did between two of its own
/// paragraphs is one thing it did, and a reader skimming wants one answer
/// about it — what sort of work, and did anything break. While any of it
/// runs the cluster starts open, so the work in flight is in sight.
///
/// Opened, the rows sit on the reading surface inset under the line's words,
/// with no frame around them: the inset already says whose rows they are.
pub fn cluster(
    plan: &crate::chat::viewport::ActivityPlan,
    open: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    id: gpui::ElementId,
    body: Vec<gpui::AnyElement>,
    cx: &App,
) -> gpui::AnyElement {
    div()
        .v_flex()
        .w_full()
        .min_w_0()
        .gap_2()
        .child(cluster_line(plan, open, on_click, id, cx))
        .when(open, |cluster| {
            cluster.child(
                div()
                    .v_flex()
                    .w_full()
                    .min_w_0()
                    .pl(FOLD_INDENT)
                    .gap_2()
                    .children(body),
            )
        })
        .into_any_element()
}

/// The collapsed line, which is also the control that opens the rows: the
/// arrow, a spinner while anything runs, the sentence, what failed in the
/// danger ink, the lines changed and the time taken.
fn cluster_line(
    plan: &crate::chat::viewport::ActivityPlan,
    open: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    id: gpui::ElementId,
    cx: &App,
) -> gpui::AnyElement {
    let summary = &plan.summary;
    let running = summary.running.is_some();
    fold_line(id, open, running, cx)
        .on_click(move |event, window, cx| on_click(event, window, cx))
        .child(div().min_w_0().truncate().child(summary.plain()))
        // In the danger ink while the cluster runs; once it has settled the
        // count is the record, in the line's own ink.
        .when(summary.errors > 0, |line| {
            line.child(div().flex_none().child("·")).child(
                div()
                    .flex_none()
                    .whitespace_nowrap()
                    .when(running, |n| {
                        n.text_color(crate::theme::status_ink(cx).danger)
                    })
                    .child(format!("{} failed", summary.errors)),
            )
        })
        .children(line_counts(summary.added, summary.removed, cx))
        // **Only once the cluster has stopped.** While a step is still going
        // the number is the total of what has already settled, which is not
        // the elapsed time of anything a reader can see.
        .children((!running && summary.seconds > 0).then(|| {
            div()
                .flex_none()
                .whitespace_nowrap()
                .child(onehand_core::duration(summary.seconds))
        }))
        .into_any_element()
}

/// How many file rows a turn's summary lists before the rest fold into one.
///
/// **Eight, ordered by how much of each file the turn touched.** A turn that
/// rewrites a package writes fifty files, and a block listing all of them is
/// the thing it was meant to replace: something to scroll rather than read.
/// The eight that matter are the eight it changed most, and the rest are a
/// count -- which is the honest shape, since somebody asking "what happened to
/// the other forty" wants the list and not the table.
const SUMMARY_ROWS: usize = 8;

/// How many diff lines one opened file row draws before it stops.
const SUMMARY_DIFF: usize = 400;

/// The height of the bar that says how much of a file the turn touched.
const RATIO_H: Rems = rems(0.25);
const RATIO_W: Rems = rems(3.);

/// What a finished turn did to the working tree.
///
/// **A result, not a record.** The clusters above it say what the agent did in
/// the order it did it, which is the question "how did it get here"; this says
/// what is different now, which is the question somebody actually has to act
/// on. A file written three times is three entries up there and one row here,
/// deliberately: the two are not the same list drawn twice.
///
/// How long the turn took is not said here: the footer under its answer
/// already says it.
pub(in crate::chat) fn turn_summary(
    session: &Entity<ChatSession>,
    plan: &crate::chat::viewport::ChangePlan,
    open: bool,
    on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> gpui::AnyElement {
    let changes = &plan.changes;
    let anchor = plan.anchor;
    // **`fold_key`, never the bare index.** History and Live indices overlap,
    // so a resumed conversation can hand two different rows one id -- which is
    // how two elements come to share one piece of retained state.
    let key = fold_key(anchor);

    let head = fold_line(("turn-summary", key), open, false, cx)
        .on_click(move |event, window, cx| on_toggle(event, window, cx))
        .child(
            div()
                .flex_none()
                .whitespace_nowrap()
                .child(match changes.files.len() {
                    1 => "1 file changed".to_string(),
                    n => format!("{n} files changed"),
                }),
        )
        .children(line_counts(changes.added, changes.removed, cx));

    let block = div().v_flex().w_full().min_w_0().gap_2().child(head);
    if !open {
        return block.into_any_element();
    }

    // Most-changed first, and only where there are more than fit: under the
    // cap the order the turn touched them in is the order the reader watched
    // it happen, which is worth more than a ranking.
    //
    // **The rest are behind a control, not cut off.** Whether they are showing
    // is kept in the section fold set keyed by the turn's prompt -- a prompt is
    // a run of its own and never a section's anchor, so that set has room for
    // this the same way the activity set has room for the block itself.
    let mut listed: Vec<&onehand_core::chat::FileChange> = changes.files.iter().collect();
    let over = listed.len().saturating_sub(SUMMARY_ROWS);
    let rest_open = session.read(cx).section_is_open(anchor);
    if over > 0 {
        listed.sort_by_key(|b| std::cmp::Reverse(b.touched()));
        if !rest_open {
            listed.truncate(SUMMARY_ROWS);
        }
    }

    let paths: Vec<String> = changes.files.iter().map(|f| f.path.clone()).collect();
    let all_open = paths
        .iter()
        .all(|path| session.read(cx).file_is_open(anchor, path));

    block
        .child(
            div()
                .v_flex()
                .w_full()
                .min_w_0()
                .pl(FOLD_INDENT)
                .gap_1()
                .text_size(TEXT_SM)
                .children(
                    listed
                        .into_iter()
                        .map(|file| file_row(session, plan, file, cx)),
                )
                // **What was left out says so, says how many, and opens.**
                .children((over > 0).then(|| {
                    div().h_flex().child(
                        show_more(
                            ("turn-rest", key),
                            match rest_open {
                                true => "Show the most changed only".to_string(),
                                false => format!("Show {over} more, least changed"),
                            },
                            cx,
                        )
                        .on_click({
                            let session = session.clone();
                            move |_, _, cx: &mut App| {
                                session.update(cx, |session, cx| {
                                    session.toggle_section(anchor);
                                    cx.notify();
                                });
                            }
                        }),
                    )
                }))
                .child(
                    div().h_flex().child(
                        crate::controls::action(("turn-diff-all", key))
                            .quiet(cx)
                            .short()
                            .label(match all_open {
                                true => "Hide every diff",
                                false => "Show every diff",
                            })
                            .on_click({
                                let session = session.clone();
                                let body = plan.body.clone();
                                move |_, _, cx: &mut App| {
                                    session.update(cx, |session, cx| {
                                        session.toggle_every_file(anchor, &paths, &body);
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
                ),
        )
        .into_any_element()
}

/// One file of a turn's summary, and its diff when it is open.
fn file_row(
    session: &Entity<ChatSession>,
    plan: &crate::chat::viewport::ChangePlan,
    file: &onehand_core::chat::FileChange,
    cx: &App,
) -> gpui::AnyElement {
    use onehand_core::chat::FileVerdict;

    let status = crate::theme::status_ink(cx);
    let anchor = plan.anchor;
    let open = session.read(cx).file_is_open(anchor, &file.path);
    let gone = file.verdict == FileVerdict::Deleted;
    let (mark, ink) = match file.verdict {
        FileVerdict::Added => ("A", status.success),
        FileVerdict::Modified => ("M", status.warning),
        FileVerdict::Deleted => ("D", status.danger),
    };
    // The folder is context for the name, so it is a step quieter than it.
    let (folder, name) = match file.path.rfind('/') {
        Some(at) => file.path.split_at(at + 1),
        None => ("", file.path.as_str()),
    };
    let lit = cx.theme().foreground;

    let row = div()
        .id(gpui::ElementId::NamedInteger(
            SharedString::from(format!("turn-file-{}", file.path)),
            fold_key(anchor) as u64,
        ))
        .h_flex()
        .items_center()
        .gap_2()
        .w_full()
        .min_w_0()
        .cursor_pointer()
        .text_color(cx.theme().muted_foreground)
        // **Ink, not a plate**, as every other row in the transcript answers.
        .hover(move |row| row.text_color(lit))
        .on_click({
            let session = session.clone();
            let path = file.path.clone();
            let body = plan.body.clone();
            move |_, _, cx: &mut App| {
                session.update(cx, |session, cx| {
                    session.toggle_file(anchor, &path, &body);
                    cx.notify();
                });
            }
        })
        .child(chevron(open))
        // **A letter in its own ink, not a coloured dot**: the one every diff
        // tool already uses.
        .child(
            div()
                .flex_none()
                .font_family(cx.theme().mono_font_family.clone())
                .text_color(ink)
                .child(mark),
        )
        .child(
            div()
                .min_w_0()
                .h_flex()
                .overflow_hidden()
                .whitespace_nowrap()
                .font_family(cx.theme().mono_font_family.clone())
                .when(gone, |path| path.line_through())
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .when(!gone, |folder| {
                            folder.text_color(crate::theme::meta_ink(cx))
                        })
                        .child(folder.to_string()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_color(match gone {
                            true => cx.theme().muted_foreground,
                            false => cx.theme().foreground,
                        })
                        .child(name.to_string()),
                ),
        )
        .children(line_counts(file.added, file.removed, cx))
        .child(ratio_bar(file, cx));

    if !open {
        return row.into_any_element();
    }

    // Taken when the row was opened, not now: this runs on every frame the row
    // is on screen, and the diff behind it is an LCS over two whole files.
    let hunks: Vec<DiffRow> = session
        .read(cx)
        .file_diff(anchor, &file.path)
        .unwrap_or_default()
        .to_vec();
    let mut budget = SUMMARY_DIFF;
    div()
        .v_flex()
        .w_full()
        .min_w_0()
        .gap_1()
        .child(row)
        .child(detail_well(plain_box(cx).py_3().child(sideways(
            gpui::ElementId::NamedInteger(
                SharedString::from(format!("turn-diff-{}", file.path)),
                fold_key(anchor) as u64,
            ),
            div().v_flex().children(diff_rows(&hunks, &mut budget, cx)),
        ))))
        .into_any_element()
}

/// How much of a file the turn touched, as a bar.
///
/// **Of the file and not of the turn.** Twenty lines changed is most of a
/// short file and nothing at all in a long one, and the number beside it
/// cannot say which -- so the bar is the only thing here answering "was this
/// rewritten or nudged". The untouched remainder is what gives it that scale,
/// which is why it is drawn rather than left as empty space.
fn ratio_bar(file: &onehand_core::chat::FileChange, cx: &App) -> gpui::Div {
    let status = crate::theme::status_ink(cx);
    // Against the larger of the file and what was done to it: a file emptied
    // by the turn has no lines left to be a proportion of.
    let whole = file.total.max(file.touched()).max(1) as f32;
    let share = |n: usize| gpui::relative(n as f32 / whole);
    div()
        .flex_none()
        .h_flex()
        .items_center()
        .w(RATIO_W)
        .h(RATIO_H)
        .rounded(cx.theme().radius)
        .overflow_hidden()
        .bg(cx.theme().border)
        .children((file.added > 0).then(|| div().h_full().w(share(file.added)).bg(status.success)))
        .children(
            (file.removed > 0).then(|| div().h_full().w(share(file.removed)).bg(status.danger)),
        )
}

/// A stretch of one kind of work inside an opened cluster.
///
/// **A row that stands for a section and a row that is one step are the same
/// row.** The only difference is what each opens into: one unfolds a command
/// and its output, the other unfolds the steps it stands for, inset under it.
pub fn activity_group(
    section: &crate::chat::viewport::Section,
    open: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    id: gpui::ElementId,
    rows: Vec<gpui::AnyElement>,
    cx: &App,
) -> gpui::AnyElement {
    let (name, icon) = activity_identity(section.group);
    // The same rule as the cluster's own line: in the danger ink while the
    // work runs, and once it has settled the count is the record, in the
    // row's own muted words.
    let running = section.outcome.outcome == onehand_core::chat::Outcome::Running;

    div()
        .v_flex()
        .w_full()
        .min_w_0()
        .gap_2()
        .child(activity_row(
            ActivityRow::new(id, RowMark::of_run(section.outcome), icon, name)
                .object(Some(Object::words(section.summary.clone())))
                .meta((section.outcome.errors > 0).then(|| {
                    div()
                        .flex_none()
                        .whitespace_nowrap()
                        .when(running, |n| {
                            n.text_color(crate::theme::status_ink(cx).danger)
                        })
                        .child(match section.outcome.errors {
                            1 => "1 failed".to_string(),
                            n => format!("{n} failed"),
                        })
                        .into_any_element()
                }))
                .fold(Some(open)),
            on_click,
            cx,
        ))
        .when(open, |block| {
            block.child(
                div()
                    .v_flex()
                    .w_full()
                    .min_w_0()
                    .pl(FOLD_INDENT)
                    .gap_2()
                    .children(rows),
            )
        })
        .into_any_element()
}

/// Stable identity for a semantic activity section: its name, and the asset
/// path of its icon.
///
/// The summary changes with every member that joins a run; the name and icon
/// do not. Keeping both on the group lets a reader identify the kind of work
/// before reading its counts.
///
/// A path rather than an icon name because the set these are drawn from is not
/// one enum: the bundled library set has no pencil of any kind, so *Changed*
/// comes from the app's own checked-in assets while its five neighbours do not.
/// The path is what both kinds resolve to anyway.
pub(super) fn activity_identity(group: activity::ActivityGroup) -> (&'static str, SharedString) {
    use gpui_component::IconNamed as _;
    match group {
        activity::ActivityGroup::Explored => ("Explored", IconName::Search.path()),
        // A pencil on a page. The library's nearest shape, `Replace`, is a
        // find-and-replace mark -- one box swapped for another, which is what
        // this group is *not*: it edited files in place.
        activity::ActivityGroup::Changed => ("Changed", crate::icons::Icon::SquarePen.path()),
        activity::ActivityGroup::Ran => ("Ran", IconName::SquareTerminal.path()),
        activity::ActivityGroup::Verified => ("Verified", IconName::CircleCheck.path()),
        activity::ActivityGroup::Reasoned => ("Reasoned", IconName::Info.path()),
        activity::ActivityGroup::Other => ("Other", IconName::Settings2.path()),
    }
}
