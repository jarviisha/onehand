use super::fold_key;
use super::metrics::{
    CHEVRON_MARK, CHEVRON_SLOT, CLUSTER_TEXT, DETAIL_INSET, FENCE_TEXT, FRAME_PAD, KIND_ICON,
    OBJECT_TEXT, PART_GAP, ROW_PAD_X, ROW_PAD_Y, STACK_GAP, TEXT, TIGHT_GAP, radius_block,
    radius_tag,
};
use super::parts::{ActivityRow, Object, RowMark, activity_row, elapsed, line_counts, mark_slot};
use super::tool::diff_rows;
use crate::chat::session::ChatSession;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, ClickEvent, Entity, InteractiveElement, IntoElement, ParentElement, Rems, SharedString,
    StatefulInteractiveElement, Styled, Window, div, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::chat::activity;
use onehand_core::chat::{ChatItem, TranscriptItemId};
use onehand_core::diff::Row as DiffRow;

// ── activity strip ──────────────────────────────────────────────────────────

/// Split the transcript into runs: either a single item, or a stretch of
/// adjacent completed steps that folds into one line.
///
/// Grouping is core's call (`onehand_core::chat::activity`), so the two front
/// ends cannot disagree about semantic sections or summaries. This view adds
/// one presentation rule: attention states never disappear into a strip.
pub fn runs(items: &[(TranscriptItemId, &ChatItem)]) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for &(target, item) in items {
        match (is_activity(item), out.last_mut()) {
            // Still the same stretch of work: extend it.
            (true, Some(Run::Activity { members })) => members.push(target),
            (true, _) => out.push(Run::Activity {
                members: vec![target],
            }),
            (false, _) => out.push(Run::Single(target)),
        }
    }
    out
}

/// Whether an item is part of what the agent *did* rather than what it said.
///
/// **Every status, and no kind boundary.** What a cluster is bounded by is the
/// agent's own words: everything between two paragraphs is one thing it did
/// between saying two things, however many kinds of work that took and whatever
/// state each is in. Splitting on the kind instead gave three reads and a
/// command two headers with nothing between them, which is two claims about one
/// stretch of work; splitting on the status put a running step outside the
/// cluster it belonged to and left it there once it finished.
///
/// A settled question and a settled grant are in it too. While either is
/// waiting it is pinned above the composer and not in the transcript at all;
/// what is left afterwards is a record of one exchange, which is one line of
/// the same list a step is a line of.
fn is_activity(item: &ChatItem) -> bool {
    match item {
        ChatItem::Tool(_) | ChatItem::Thought(_) => true,
        ChatItem::Permission(p) => p.resolved.is_some(),
        ChatItem::Ask(a) => a.resolved.is_some(),
        _ => false,
    }
}

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

pub enum Run {
    Single(TranscriptItemId),
    /// Every adjacent activity between two of the agent's paragraphs, which is
    /// one cluster and draws one muted line.
    Activity {
        members: Vec<TranscriptItemId>,
    },
}

/// A cluster of activity, drawn as one muted line that opens into a frame.
///
/// **The line is the whole of what the transcript shows by default.** Everything
/// the agent did between two of its own paragraphs is one thing it did, and a
/// reader skimming wants one answer about it — what sort of work, and did
/// anything break. That fits in a sentence, and a sentence needs no border, no
/// fill and no rule: it sits on the reading surface at the same left edge as
/// the prose either side of it, in the ink a marginal note is set in.
///
/// **The frame only exists once it is asked for.** Drawn always, it was a box
/// the height of a paragraph standing between two paragraphs, for steps nobody
/// had asked to see — the detail claiming the space of the answer. Opened, it
/// is the investigation, and then it can have every edge it needs.
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
        // **The line shrinks to its sentence, and this is what lets it.** A
        // column flex stretches its children across by default, and the line is
        // a library `Button` -- which centres its own content and offers no way
        // out of it. Stretched, the sentence came out down the middle of the
        // column with the prose above and below it starting at the left edge,
        // which reads as a caption for the paragraph rather than as a note
        // beside it. The frame below is unaffected: it asks for the full width
        // itself.
        .items_start()
        .w_full()
        .min_w_0()
        .gap(STACK_GAP)
        .child(cluster_line(plan, open, on_click, id, cx))
        .when(open, |cluster| {
            cluster.child(
                div()
                    .v_flex()
                    .w_full()
                    .min_w_0()
                    .rounded(radius_block(cx))
                    .border_1()
                    .border_color(cx.theme().border)
                    // **The edge and nothing else.** A fill would make this a
                    // second surface inside the reading one, which is a slab of
                    // another colour standing between two paragraphs for as
                    // long as it is open -- and it is not needed: the border
                    // already says where the detail begins and ends, and what
                    // separates the rows inside it is their own hairlines. It
                    // also keeps the hover fill on those rows legible, which
                    // against a surface already a step off the reading one had
                    // half the contrast to work with.
                    //
                    // The rules between its rows run the full width, so without
                    // this each ends in a square nib a pixel outside the
                    // rounded edge above it.
                    .overflow_hidden()
                    .children(body),
            )
        })
        .into_any_element()
}

/// The collapsed line, which is also the control that opens the frame.
fn cluster_line(
    plan: &crate::chat::viewport::ActivityPlan,
    open: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    id: gpui::ElementId,
    cx: &App,
) -> gpui::AnyElement {
    let summary = &plan.summary;
    // **A run of text, not a row of columns.** The sentence is read as a
    // sentence, so the verbs sit inside it rather than in a column of their
    // own — and the whole thing shrinks to what it says instead of ruling a
    // line across the column.
    let mut sentence = div()
        .h_flex()
        .items_center()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .children(summary.running.as_ref().map(|part| {
            div()
                .flex_none()
                .whitespace_nowrap()
                .child(format!("{}{}", part.verb, part.rest))
        }));
    for (n, part) in summary.done.iter().enumerate() {
        let lead = match (n, summary.running.is_some()) {
            (0, false) => "",
            (0, true) => " · ",
            _ => ", ",
        };
        sentence = sentence
            .child(div().flex_none().child(lead))
            .child(
                div()
                    .flex_none()
                    .whitespace_nowrap()
                    .child(part.verb.clone()),
            )
            .child(div().min_w_0().truncate().child(part.rest.clone()));
    }

    // **A stateful `div`, not the app's button wrapper.** The wrapper is a
    // library `Button`, and reaching its hover state from the call site meant
    // going through three layers -- the button's own refinement, the
    // `Stateful<Div>` underneath it, and the group-hitbox registry a
    // `group_hover` resolves against. Two attempts at that changed nothing on
    // screen. `hover` on a stateful div is the primitive all three are built
    // out of: it styles the element whose own hitbox the pointer is over, with
    // nothing in between to go wrong.
    //
    // What it costs is the keyboard, which a `Button` would have carried. The
    // rail's rows made the same trade for the same kind of reason.
    div()
        .id(id)
        .h_flex()
        .items_center()
        .gap(STACK_GAP)
        .h(rems(1.75))
        // **Shrink to the sentence.** A control the width of the column is a
        // bar, and a bar is a thing in the transcript rather than a note in the
        // margin of one. Past the column it truncates instead.
        .w_auto()
        .max_w_full()
        .min_w_0()
        .cursor_pointer()
        .text_size(CLUSTER_TEXT)
        // **A weight is a request, like a family.** It lands only where the
        // resolved face carries that cut; where it does not, the platform hands
        // back the nearest it has. Nothing here depends on it: the ink carries
        // the line on its own, and this is the second channel, not the first.
        .font_weight(gpui::FontWeight::EXTRA_LIGHT)
        .text_color(cx.theme().muted_foreground)
        // **Hover is the ink and the weight, and no fill.** Both are the line's
        // own two channels turned up rather than a plate put behind it -- which
        // is what a note in the margin has to do, since a rectangle appearing
        // between two paragraphs is the chrome answering instead of the thing
        // hovered. The meaning colours on the counts are set per child and are
        // left alone.
        .hover(|line| {
            line.font_weight(gpui::FontWeight::NORMAL)
                .text_color(crate::theme::meta_ink(cx))
        })
        .on_click(move |event, window, cx| on_click(event, window, cx))
        .child(sentence)
        // **What went wrong is not counted here.** The line carries what the
        // work *was*; how it came out is the business of the rows inside it,
        // each of which names its own failure and its own exit code. A tally
        // on the outside is a number nobody can act on without opening the
        // block anyway, and it was the loudest thing on a line whose whole job
        // is to stay behind the answer above it.
        // The total, after the sentence and before the counts: it is about the
        // *work* rather than about the files. Only where something reported
        // one, or a cluster whose steps never said would claim to have taken no
        // time at all.
        //
        // **And only once the cluster has stopped.** While a step is still
        // going the number is the total of what has already settled, which is
        // not the elapsed time of anything a reader can see: it sits next to a
        // line saying work is in flight and reads as that work's duration,
        // frozen. The line already says it is running; how long it took is an
        // answer, and an answer belongs after the fact.
        .children((summary.running.is_none() && summary.seconds > 0).then(|| {
            div()
                .flex_none()
                .whitespace_nowrap()
                .font_family(cx.theme().mono_font_family.clone())
                .child(elapsed(summary.seconds))
        }))
        .children(line_counts(summary.added, summary.removed, cx))
        // **Last, as it is on every row inside the frame.** The arrow means the
        // same thing in both places, and a control that moves ends of the line
        // depending on which kind of row it is on is one the eye has to find
        // twice.
        .child(
            mark_slot(CHEVRON_SLOT).child(
                Icon::new(match open {
                    true => IconName::ChevronDown,
                    false => IconName::ChevronRight,
                })
                .size(CHEVRON_MARK),
            ),
        )
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

    let head = div()
        .id(("turn-summary", key))
        .h_flex()
        .items_center()
        .gap(PART_GAP)
        .w_full()
        .min_w_0()
        .px(ROW_PAD_X)
        .py(ROW_PAD_Y)
        .cursor_pointer()
        // **One step above the reading size.** This is where a turn ends, and
        // the thing a reader scrolling past a long answer is looking for. Every
        // other line in the block is at or below the transcript's own size, so
        // the step is what makes the block have a top rather than a first row.
        .text_size(CLUSTER_TEXT)
        .text_color(cx.theme().foreground)
        .hover(|row| row.text_color(crate::theme::meta_ink(cx)))
        .on_click(move |event, window, cx| on_toggle(event, window, cx))
        .child(
            mark_slot(CHEVRON_SLOT).child(
                Icon::new(match open {
                    true => IconName::ChevronDown,
                    false => IconName::ChevronRight,
                })
                .size(CHEVRON_MARK),
            ),
        )
        .child(
            div()
                .flex_none()
                .whitespace_nowrap()
                .child(match changes.files.len() {
                    1 => "1 file changed".to_string(),
                    n => format!("{n} files changed"),
                }),
        )
        .children(line_counts(changes.added, changes.removed, cx))
        .child(div().flex_1().min_w_0())
        // Right-aligned, because it is the one number here that is about the
        // turn rather than about the tree.
        .children(changes.seconds.map(|secs| {
            div()
                .flex_none()
                .whitespace_nowrap()
                .text_size(TEXT)
                .text_color(cx.theme().muted_foreground)
                .child(elapsed(secs))
        }));

    let card = div()
        .v_flex()
        .w_full()
        .min_w_0()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().border)
        .child(head);

    if !open {
        return card.into_any_element();
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

    card.child(
        div()
            .v_flex()
            .w_full()
            .min_w_0()
            .border_t_1()
            .border_color(cx.theme().border)
            .children(
                listed
                    .into_iter()
                    .map(|file| file_row(session, plan, file, cx)),
            )
            // **What was left out says so, says how many, and opens.** A list
            // silently cut at eight is a list claiming the turn touched eight
            // files; one that says how many were dropped and cannot show them
            // is a question with no answer in the room.
            .children((over > 0).then(|| {
                div()
                    .id(("turn-rest", key))
                    .w_full()
                    .px(ROW_PAD_X)
                    .py(ROW_PAD_Y)
                    .cursor_pointer()
                    .text_size(OBJECT_TEXT)
                    .text_color(cx.theme().muted_foreground)
                    .hover(|row| row.text_color(crate::theme::meta_ink(cx)))
                    .on_click({
                        let session = session.clone();
                        move |_, _, cx: &mut App| {
                            session.update(cx, |session, cx| {
                                session.toggle_section(anchor);
                                cx.notify();
                            });
                        }
                    })
                    .child(match rest_open {
                        true => "Show the most changed only".to_string(),
                        false => format!("and {over} more, least changed"),
                    })
            })),
    )
    .child(
        div()
            .h_flex()
            .items_center()
            .gap(PART_GAP)
            .w_full()
            .min_w_0()
            .px(ROW_PAD_X)
            .py(ROW_PAD_Y)
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                crate::controls::action(("turn-diff-all", key))
                    .ghost()
                    .xsmall()
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
    // The folder is context for the name, so it is a step quieter than it --
    // the same two strengths a completion row puts a name and its folder at.
    let (folder, name) = match file.path.rfind('/') {
        Some(at) => file.path.split_at(at + 1),
        None => ("", file.path.as_str()),
    };

    let row = div()
        .id(gpui::ElementId::NamedInteger(
            SharedString::from(format!("turn-file-{}", file.path)),
            fold_key(anchor) as u64,
        ))
        .h_flex()
        .items_center()
        .gap(PART_GAP)
        .w_full()
        .min_w_0()
        .px(ROW_PAD_X)
        .py(TIGHT_GAP)
        .cursor_pointer()
        .text_size(OBJECT_TEXT)
        .text_color(cx.theme().muted_foreground)
        // **Ink, not a plate**, which is what every other row inside a frame
        // answers a hover with -- a fill here would make one list in the
        // transcript behave unlike the list an inch above it.
        .hover(|row| row.text_color(crate::theme::meta_ink(cx)))
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
        // **A letter in its own ink, not a coloured dot.** Four states that a
        // reader has to tell apart on a dense row is more than colour alone
        // carries, and the letter is the one every diff tool already uses.
        .child(
            div()
                .flex_none()
                .w(KIND_ICON)
                .font_family(cx.theme().mono_font_family.clone())
                .text_color(ink)
                .child(mark),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .when(gone, |path| path.line_through())
                .child(folder.to_string()),
        )
        .child(
            div()
                .flex_none()
                .min_w_0()
                .truncate()
                .text_color(cx.theme().foreground)
                .when(gone, |name| name.line_through())
                .child(name.to_string()),
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
        .child(row)
        .child(
            div()
                .v_flex()
                .w_full()
                .min_w_0()
                .border_t_1()
                .border_color(cx.theme().border)
                .font_family(cx.theme().mono_font_family.clone())
                .text_size(FENCE_TEXT)
                .children(diff_rows(&hunks, &mut budget, cx)),
        )
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
        .rounded(radius_tag(cx))
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
/// row.** Collapsed they are indistinguishable, and the only difference is what
/// each opens into: one unfolds a command and its output, the other unfolds the
/// steps it stands for — children at a shorter height, set in to where the
/// parent's verb starts, carrying no frame and separated only by hairlines.
pub fn activity_group(
    section: &crate::chat::viewport::Section,
    open: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    id: gpui::ElementId,
    rows: Vec<gpui::AnyElement>,
    cx: &App,
) -> gpui::AnyElement {
    let (name, icon) = activity_identity(section.group);

    div()
        .v_flex()
        .w_full()
        .min_w_0()
        .child(activity_row(
            ActivityRow::new(id, RowMark::of_run(section.outcome), icon, name)
                .object(Some(Object::plain(section.summary.clone())))
                .meta((section.outcome.errors > 0).then(|| {
                    div()
                        .flex_none()
                        .whitespace_nowrap()
                        .text_xs()
                        .text_color(crate::theme::status_ink(cx).danger)
                        .child(match section.outcome.errors {
                            1 => "1 error".to_string(),
                            n => format!("{n} errors"),
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
                    .pl(DETAIL_INSET)
                    .pr(ROW_PAD_X)
                    .pb(FRAME_PAD)
                    .children(rows),
            )
        })
        .into_any_element()
}

/// The hairline that separates one row of a block from the next.
pub fn rule(cx: &App) -> gpui::Div {
    div().w_full().flex_none().h_px().bg(cx.theme().border)
}

/// The drawing that stands for a kind of work, in the column every row keeps
/// for one.
pub(super) fn group_icon(group: activity::ActivityGroup) -> SharedString {
    activity_identity(group).1
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

/// A semantic, counted summary of the work in one activity run.
///
/// Per-target phrases make the folded form almost as expensive to scan as its
/// expanded rows (`Inspected a · Inspected b · Inspected c`). Counts preserve
/// what happened while letting one glance answer how much happened.
pub fn activity_summary(members: &[&ChatItem]) -> String {
    let mut counts = [0usize; 10];
    let mut reason_secs = 0u64;
    for item in members {
        let kind = match item {
            ChatItem::Thought(th) => {
                counts[8] += 1;
                if let Some(secs) = th.elapsed_secs {
                    reason_secs += secs;
                }
                continue;
            }
            ChatItem::Tool(tool) => {
                let p = activity::presentation(tool);
                if p.kind == activity::ActivityKind::Change {
                    // One MultiEdit is one tool step but can touch many files;
                    // the summary names files, so count its distinct paths.
                    counts[3] += tool.diff_summary.len().max(1);
                    continue;
                }
                p.kind
            }
            _ => continue,
        };
        let index = match kind {
            activity::ActivityKind::Inspect => 0,
            activity::ActivityKind::Search => 1,
            activity::ActivityKind::Fetch => 2,
            activity::ActivityKind::Change => 3,
            activity::ActivityKind::Test => 4,
            activity::ActivityKind::Check => 5,
            activity::ActivityKind::Build => 6,
            activity::ActivityKind::Run => 7,
            activity::ActivityKind::Reason => 8,
            activity::ActivityKind::Other => 9,
        };
        counts[index] += 1;
    }

    // **Nouns, with no verb on any of them.** The row this lands in already
    // carries one, in full ink and a weight up, two columns to the left -- so
    // a phrase that opened with its own gave `Ran · Ran 7 commands`, which is
    // the same word twice within an inch and the count pushed a column right
    // for it.
    let mut phrases = Vec::new();
    let counted = [
        (0, "file", "files"),
        (1, "search", "searches"),
        (2, "resource", "resources"),
        (3, "file", "files"),
        (4, "test", "tests"),
        (5, "check", "checks"),
        (6, "target", "targets"),
        (7, "command", "commands"),
        (9, "tool", "tools"),
    ];
    for (index, singular, plural) in counted {
        let count = counts[index];
        if count > 0 {
            phrases.push(format!(
                "{count} {}",
                if count == 1 { singular } else { plural }
            ));
        }
    }
    if counts[8] > 0 {
        phrases.push(match reason_secs > 0 {
            true => format!("{reason_secs}s reasoning"),
            false => "reasoning".to_string(),
        });
    }

    let mut label = phrases.join(" · ");
    if label.is_empty() {
        label.push_str("activity");
    }
    label
}
