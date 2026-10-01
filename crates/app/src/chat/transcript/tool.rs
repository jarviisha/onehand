use super::fold_key;
use super::metrics::{
    BLOCK_INSET, CODE_LH, CODE_TEXT, COMMAND_OPEN_SHARE, COPY_ICON, COPY_SIZE, DETAIL_INSET,
    DIFF_NUM_PAD, DIFF_NUM_W, DIFF_SIGN_W, DIFF_TEXT_PAD, FOLD_H, FOLD_ROW, FRAME_PAD, LARGE_DIFF,
    MAX_DIFF_LINES, MAX_MONO_LINES, MONO_ADVANCE, OBJECT_TEXT, PILL_PAD_X, PILL_PAD_Y,
    PREVIEW_DIFF, PREVIEW_OUT, ROW_PAD_X, SMOKE_DIFF, SMOKE_OUT, STACK_GAP, TIGHT_GAP,
    radius_block, radius_control,
};
use super::parts::{
    ActivityRow, Object, RowMark, activity_row, copy_button, line_counts, plain_box, row_note,
    scrolled,
};
use super::strip::group_icon;
use crate::chat::session::ChatSession;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Axis, ClickEvent, Entity, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Window, div, relative, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::scroll::{ScrollableMask, Scrollbar, ScrollbarMode};
use gpui_component::{ActiveTheme, Icon, IconName, StyledExt};
use onehand_core::acp::{ToolContent, ToolKind, ToolStatus};
use onehand_core::chat::activity;
use onehand_core::chat::{ToolItem, TranscriptItemId};
use onehand_core::diff::Row as DiffRow;
use std::path::Path;

// ── tool call ───────────────────────────────────────────────────────────────

/// A tool step: one activity row, and what it opens into.
///
/// **One geometry for every kind and every status.** A status is what the disc
/// at the head of the row says and what the column at its end says; it is never
/// what the row *is*. What differs between a read and a command is only the
/// shape of the thing underneath, once somebody has asked for it.
pub(super) fn tool(
    session: &Entity<ChatSession>,
    t: &ToolItem,
    target: TranscriptItemId,
    cx: &App,
) -> impl IntoElement + use<> {
    let root = session.read(cx).chat.root.clone();
    let presented = activity::presentation(t);
    let open = t.is_open();
    // **The live stream, while there is one.** A command that is still running
    // has its output in the session's terminal map rather than in the card --
    // the model folds it in at turn end -- so a detail built from the card's
    // own sections alone draws an empty box under a `cargo build` for as long
    // as the build takes, which is exactly when somebody is looking at it.
    let live: Option<&onehand_core::chat::TermView> =
        t.call.content.iter().find_map(|section| match section {
            ToolContent::Terminal(id) => session.read(cx).chat.terminals.get(id),
            _ => None,
        });
    let detail = tool_detail(t, &presented, &root, live);
    let toggle = {
        let session = session.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            session.update(cx, |s, cx| {
                s.chat.toggle_tool(target);
                cx.notify();
            });
        }
    };

    let (added, removed) = t
        .diff_summary
        .iter()
        .fold((0, 0), |(a, r), (_, plus, minus)| (a + plus, r + minus));
    let deleted = t.call.kind == ToolKind::Delete;
    let object = match t.call.kind {
        // A command is not a path and must not be split at its last slash.
        ToolKind::Execute => Some(Object::plain(activity::first_line_trunc(
            &onehand_core::chat::redact(&presented.subject),
            160,
        ))),
        _ if presented.subject.trim().is_empty() => None,
        _ => Some(Object::path(path_for_display(&root, &presented.subject))),
    };
    let meta = match (t.call.status, deleted) {
        // **The code where there is one, the word where there is not.** Only a
        // command run through the terminal extension reports a status, so a
        // failure arriving as a plain tool call has nothing but the word --
        // and printing `failed` over a step that told us it exited 101 throws
        // away the one thing that says *what* went wrong.
        (ToolStatus::Failed, _) => Some(row_note(
            match t.exit_code {
                Some(code) => format!("exit {code}"),
                None => "failed".to_string(),
            },
            crate::theme::status_ink(cx).danger,
            cx,
        )),
        (ToolStatus::InProgress, _) | (ToolStatus::Pending, _) => None,
        (_, true) => Some(row_note("deleted", cx.theme().muted_foreground, cx)),
        _ => line_counts(added, removed, cx)
            .map(|pair| pair.text_size(OBJECT_TEXT).into_any_element()),
    };

    let mut row = ActivityRow::new(
        ("tool", fold_key(target)).into(),
        RowMark::of(t.call.status),
        group_icon(presented.kind.into()),
        presented.action,
    )
    .object(object)
    .meta(meta)
    .fold(detail.is_some().then_some(open));
    row.struck = deleted;

    div()
        .v_flex()
        .w_full()
        .min_w_0()
        .child(activity_row(row, toggle, cx))
        .children(
            open.then_some(())
                .and(detail)
                .map(|detail| detail_frame(session, t, target, detail, cx)),
        )
}

/// What a step opens into, chosen by what the step is.
enum Detail {
    /// A command and whatever it printed.
    Command { command: String, output: String },
    /// One or more file edits.
    Diffs,
    /// A list of what was looked at, and how many lines of it were dropped to
    /// keep the box bounded.
    Lines {
        lines: Vec<SharedString>,
        hidden: usize,
    },
    /// A picture the step produced.
    Image(std::sync::Arc<Vec<u8>>),
}

impl Detail {
    /// The whole of what the box holds, as text.
    ///
    /// **What Copy hands over, and why there is a Copy at all.** The rows here
    /// are drawn from plain elements, which the renderer does not let a drag
    /// select -- the one selectable text in the transcript is the agent's prose,
    /// and it is selectable because it goes through a markdown renderer that
    /// owns its own selection. A diff cannot: its three columns are layout, and
    /// running them through that renderer to gain a drag would cost the columns.
    /// So the block answers in whole rather than in part, which is also what
    /// somebody pasting a failure into a bug report wants.
    fn text(&self, rows: &[String]) -> Option<String> {
        match self {
            Self::Command { command, output } => Some(match output.is_empty() {
                true => command.clone(),
                false => format!("{command}\n\n{output}"),
            }),
            Self::Lines { lines, .. } => Some(
                lines
                    .iter()
                    .map(|l| l.to_string())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            Self::Diffs => Some(rows.join("\n")),
            // Bytes are not text, and a clipboard image is a different feature.
            Self::Image(_) => None,
        }
    }
}

fn tool_detail(
    t: &ToolItem,
    presented: &activity::Presentation,
    root: &Path,
    live: Option<&onehand_core::chat::TermView>,
) -> Option<Detail> {
    if t.call.kind == ToolKind::Execute {
        let command = onehand_core::chat::redact(t.call.title.trim());
        let mut output = t
            .call
            .content
            .iter()
            .filter_map(|c| match c {
                ToolContent::Text(text) => Some(onehand_core::chat::redact(text)),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        // A running command's output is not in its card yet. Appended rather
        // than substituted, since a step can have said something of its own
        // before the terminal it opened started printing.
        if let Some(view) = live.filter(|view| !view.output.is_empty()) {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(&onehand_core::chat::redact(&view.output));
        }
        return (!command.is_empty()).then_some(Detail::Command { command, output });
    }
    if t.call
        .content
        .iter()
        .any(|c| matches!(c, ToolContent::Diff { .. }))
    {
        return Some(Detail::Diffs);
    }
    if let Some(ToolContent::Image(bytes)) = t
        .call
        .content
        .iter()
        .find(|c| matches!(c, ToolContent::Image(_)))
    {
        return Some(Detail::Image(bytes.clone()));
    }
    // A read or a search says what it looked at, one line each — the paths a
    // merged row stands for, or whatever the tool printed back.
    let all: Vec<SharedString> = t
        .call
        .content
        .iter()
        .filter_map(|c| match c {
            ToolContent::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .flat_map(|text| text.lines())
        .map(|line| SharedString::from(onehand_core::chat::redact(line)))
        .collect();
    // Bounded, and the bound is counted rather than swallowed: a list cut at
    // sixty with nothing said is a list claiming the step looked at sixty
    // things.
    let hidden = all.len().saturating_sub(MAX_MONO_LINES);
    let lines: Vec<SharedString> = all.into_iter().take(MAX_MONO_LINES).collect();
    match lines.is_empty() {
        true => {
            let subject = path_for_display(root, &presented.subject);
            (!subject.trim().is_empty()).then(|| Detail::Lines {
                lines: vec![subject.into()],
                hidden: 0,
            })
        }
        false => Some(Detail::Lines { lines, hidden }),
    }
}

/// The box a row opens into: set in to the row's own words, one step off the
/// frame it sits in, and carrying nothing but the text.
fn detail_frame(
    session: &Entity<ChatSession>,
    t: &ToolItem,
    target: TranscriptItemId,
    detail: Detail,
    cx: &App,
) -> gpui::Div {
    let copy = detail.text(&diff_text(t));
    div()
        .w_full()
        .min_w_0()
        .pl(DETAIL_INSET)
        .pr(ROW_PAD_X)
        .pb(FRAME_PAD)
        .child(
            div()
                .relative()
                .w_full()
                .min_w_0()
                .overflow_hidden()
                .rounded(radius_block(cx))
                .border_1()
                .border_color(cx.theme().border)
                // **A step off the frame and no more.** It has to read as a
                // layer rather than as a card somebody dropped in, and the
                // frame it sits in is already on the reading surface — so the
                // whole of the difference is one step of the ramp.
                .bg(cx.theme().muted.opacity(0.4))
                .font_family(cx.theme().mono_font_family.clone())
                .text_size(OBJECT_TEXT)
                .line_height(relative(CODE_LH))
                .map(|box_| match detail {
                    Detail::Command { command, output } => {
                        command_detail(session, t, target, &command, &output, box_, cx)
                    }
                    Detail::Diffs => diff_detail(session, t, target, box_, cx),
                    // An image has an edge of its own already -- the box it is
                    // in -- so it fills it rather than sitting inside a second
                    // one.
                    Detail::Image(bytes) => box_.map(|box_| match session.read(cx).image(&bytes) {
                        Some(handle) => box_.child(gpui::img(handle).max_w_full()),
                        None => box_.child(
                            div()
                                .px(DIFF_TEXT_PAD)
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("[unrecognized image, {} bytes]", bytes.len())),
                        ),
                    }),
                    Detail::Lines { lines, hidden } => box_
                        .children(lines.into_iter().map(|line| {
                            div()
                                .w_full()
                                .min_w_0()
                                .px(DIFF_TEXT_PAD)
                                .text_color(cx.theme().muted_foreground)
                                .child(line)
                        }))
                        .children((hidden > 0).then(|| {
                            div()
                                .w_full()
                                .min_w_0()
                                .px(DIFF_TEXT_PAD)
                                .text_color(cx.theme().muted_foreground.opacity(0.7))
                                .child(format!("and {hidden} more lines"))
                        })),
                })
                // **Drawn always, never waiting to be hovered.** A control that
                // appears under the pointer is one nobody finds who was not
                // already reaching for it -- and this is the box whose text is
                // most likely to be wanted somewhere else: pasted into a shell,
                // quoted in a bug report, kept as the record of what ran.
                //
                // It covers the tail of the first line, which is the trade the
                // column it would otherwise reserve costs every line below.
                // What a first line carries is its opening, and that is the
                // part it keeps.
                .children(copy.map(|text| {
                    div()
                        .absolute()
                        .top(TIGHT_GAP)
                        .right(TIGHT_GAP)
                        .rounded(radius_control(cx))
                        .bg(cx.theme().muted)
                        .child(
                            copy_button(("detail-copy", fold_key(target)), text)
                                .tooltip("Copy this"),
                        )
                })),
        )
}

/// A diff's rows as the lines they would be in a file.
fn diff_text(t: &ToolItem) -> Vec<String> {
    let mut out = Vec::new();
    for key in 0..t.call.content.len() {
        let Some(rows) = t.diff_rows.get(&key) else {
            continue;
        };
        for row in rows {
            out.push(match row {
                DiffRow::Context(l) => format!(" {l}"),
                DiffRow::Added(l) => format!("+{l}"),
                DiffRow::Removed(l) => format!("-{l}"),
                DiffRow::Skipped(n) => format!("@@ {n} unchanged lines @@"),
            });
        }
    }
    out
}

/// A command, then what it printed.
///
/// **No `IN` and no `OUT`.** The `$` says which is which, and the rule under it
/// says where one ends — two labels in a fixed column were four characters of
/// chrome per section and a column taken off the widest text in the transcript.
fn command_detail(
    session: &Entity<ChatSession>,
    t: &ToolItem,
    target: TranscriptItemId,
    command: &str,
    output: &str,
    box_: gpui::Div,
    cx: &App,
) -> gpui::Div {
    let open = t.out_open.contains(&0);
    let lines: Vec<&str> = output.lines().collect();
    // **Always the tail, opened or closed.** An output says what happened at
    // its end -- the error, the summary line, the prompt coming back -- so the
    // collapsed preview shows the last few. Opened from the *top* instead, as
    // this did, a two-hundred-line build jumped from its last five lines to its
    // first sixty and dropped the rest with nothing saying so: the one part
    // somebody opened the box to read is the part that went away.
    let cap = match open {
        true => MAX_MONO_LINES,
        false => PREVIEW_OUT,
    };
    let hidden = lines.len().saturating_sub(cap);
    let shown: Vec<SharedString> = lines
        .iter()
        .skip(hidden)
        .map(|line| SharedString::from(line.to_string()))
        .collect();
    let danger = crate::theme::status_ink(cx).danger;

    box_.v_flex()
        .child(
            div()
                .w_full()
                .min_w_0()
                .h_flex()
                .items_start()
                .gap(TIGHT_GAP)
                .py(STACK_GAP)
                .px(FRAME_PAD)
                .child(
                    div()
                        .flex_none()
                        .text_color(cx.theme().muted_foreground.opacity(0.7))
                        .child("$"),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(cx.theme().foreground)
                        .child(command.to_string()),
                ),
        )
        .children((!shown.is_empty()).then(|| {
            div()
                .w_full()
                .min_w_0()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(
                    div()
                        .relative()
                        .w_full()
                        .min_w_0()
                        // **Collapsed it does not scroll at all.** A preview
                        // short enough to read whole has nothing to scroll, and
                        // a box that scrolled anyway would be a second scroller
                        // under the reader's finger for no reason.
                        .child(scrolled(
                            open,
                            fold_key(target),
                            div().v_flex().w_full().min_w_0().py(STACK_GAP).children(
                                shown.into_iter().map(|line| {
                                    // A failure names itself in what it printed,
                                    // so the ink follows the words rather than
                                    // the row: an error in the middle of a
                                    // hundred quiet lines is the one somebody
                                    // is looking for.
                                    let bad = is_error_line(&line);
                                    div()
                                        .w_full()
                                        .min_w_0()
                                        .px(FRAME_PAD)
                                        .text_color(match bad {
                                            true => danger,
                                            false => cx.theme().muted_foreground,
                                        })
                                        .child(line)
                                }),
                            ),
                        ))
                        // **The cut fades at the top, because the tail is what
                        // was kept.** A command's failure is the last thing it
                        // printed, so the preview is its end and what is hidden
                        // is above it.
                        .when(hidden > 0, |body| {
                            body.child(div().absolute().top_0().left_0().right_0().h(SMOKE_OUT).bg(
                                gpui::linear_gradient(
                                    180.,
                                    gpui::linear_color_stop(cx.theme().muted, 0.15),
                                    gpui::linear_color_stop(cx.theme().muted.alpha(0.), 1.),
                                ),
                            ))
                        }),
                )
        }))
        .children((hidden > 0 || open).then(|| {
            fold_pill(
                session,
                target,
                // Opened, the box is still bounded -- so the control says
                // what is *still* cut rather than claiming the whole of it is
                // on screen.
                match (open, hidden) {
                    (true, 0) => "Show less".to_string(),
                    (true, n) => format!("Show less · {n} earlier lines not shown"),
                    (false, n) => format!("Show {n} earlier lines"),
                },
                open,
                cx,
            )
        }))
}

/// Whether a line of output is the part somebody went looking for.
pub(super) fn is_error_line(line: &str) -> bool {
    let lower = line.trim_start().to_ascii_lowercase();
    ["error", "failed", "panic", "fatal", "assertion"]
        .iter()
        .any(|mark| lower.starts_with(mark))
}

/// Every edit the step made, one diff after another.
fn diff_detail(
    session: &Entity<ChatSession>,
    t: &ToolItem,
    target: TranscriptItemId,
    box_: gpui::Div,
    cx: &App,
) -> gpui::Div {
    let open = t.out_open.contains(&0);
    let changed: usize = t
        .diff_summary
        .iter()
        .map(|(_, plus, minus)| plus + minus)
        .sum();
    // **Offered rather than drawn.** A diff this size is searched and not read,
    // and every line of it is an element in a list that is already virtualising
    // rows for the same reason.
    if changed > LARGE_DIFF && !open {
        return box_.child(
            div()
                .w_full()
                .min_w_0()
                .py(STACK_GAP)
                .px(FRAME_PAD)
                .text_color(cx.theme().muted_foreground)
                .child(format!("Large diff · {changed} changed lines"))
                .child(fold_pill(session, target, "Load diff", false, cx)),
        );
    }

    let root = session.read(cx).chat.root.clone();
    let mut budget = MAX_DIFF_LINES;
    let mut rows: Vec<gpui::AnyElement> = Vec::new();
    let files = t
        .call
        .content
        .iter()
        .filter(|c| matches!(c, ToolContent::Diff { .. }))
        .count();
    for (key, content) in t.call.content.iter().enumerate() {
        let ToolContent::Diff { path, .. } = content else {
            continue;
        };
        // **A header only where there is more than one file**, and it is the
        // way into the editor: reviewing a diff and wanting to touch it up is
        // one motion, not two. With a single file the row above already names
        // it, and a second copy an inch under the first says nothing twice.
        if files > 1 {
            rows.push(diff_path(session, &root, key, path, cx));
        }
        let hunks = t.diff_rows.get(&key).map(Vec::as_slice).unwrap_or_default();
        rows.extend(diff_rows(hunks, &mut budget, cx));
    }
    let total = rows.len();
    let shown = match open {
        true => total,
        false => total.min(PREVIEW_DIFF),
    };
    let hidden = total - shown;
    rows.truncate(shown);

    box_.child(
        div()
            .relative()
            .w_full()
            .min_w_0()
            .child(scrolled(
                open,
                fold_key(target),
                div().v_flex().w_full().min_w_0().children(rows),
            ))
            // The cut fades at the foot: a diff opens on its change, so what is
            // held back is what comes after.
            .when(hidden > 0, |body| {
                body.child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .h(SMOKE_DIFF)
                        .bg(gpui::linear_gradient(
                            0.,
                            gpui::linear_color_stop(cx.theme().muted, 0.15),
                            gpui::linear_color_stop(cx.theme().muted.alpha(0.), 1.),
                        )),
                )
            }),
    )
    .children((hidden > 0 || open).then(|| {
        fold_pill(
            session,
            target,
            match open {
                true => "Show less".to_string(),
                false => format!("Show {hidden} more lines"),
            },
            open,
            cx,
        )
    }))
}

/// Which file the rows under it belong to, and the way into it.
fn diff_path(
    session: &Entity<ChatSession>,
    root: &Path,
    key: usize,
    path: &str,
    cx: &App,
) -> gpui::AnyElement {
    let shown = path_for_display(root, path);
    // Resolved here, where the project root is known: the Workbench opens
    // whatever path it is handed, and a relative one there resolves against the
    // process working directory.
    let full = onehand_core::editor::resolve_in_root(root, path);
    let session = session.clone();
    crate::controls::action(("diff-path", key))
        .ghost()
        .w_full()
        .min_w_0()
        .h_auto()
        .py(TIGHT_GAP)
        .px(DIFF_TEXT_PAD)
        .rounded_none()
        .label(shown)
        .text_color(crate::theme::hue_ink(cx.theme().blue, cx))
        .on_click(move |_, _, cx: &mut App| {
            session.update(cx, |_, cx| {
                cx.emit(crate::chat::session::ChatEvent::OpenFile(full.clone()))
            });
        })
        .into_any_element()
}

/// One diff, as three columns that hold whatever the text does.
pub(super) fn diff_rows(hunks: &[DiffRow], budget: &mut usize, cx: &App) -> Vec<gpui::AnyElement> {
    let status = crate::theme::status_ink(cx);
    let mut out = Vec::new();
    let mut n = 0usize;
    for line in hunks {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        // **A skipped run is a line of its own, in the ink that means it can be
        // opened.** Blue is the transcript's one word for "there is more behind
        // this", and an elided run is exactly that -- left in the quiet ink it
        // read as a remark about the file rather than as a way into it.
        if let DiffRow::Skipped(count) = line {
            out.push(
                div()
                    .w_full()
                    .min_w_0()
                    .px(DIFF_TEXT_PAD)
                    .bg(crate::theme::hue_ink(cx.theme().blue, cx).alpha(0.06))
                    .text_size(rems(0.71875))
                    .text_color(crate::theme::hue_ink(cx.theme().blue, cx))
                    .child(format!("{count} unchanged lines"))
                    .into_any_element(),
            );
            n += count;
            continue;
        }
        n += 1;
        let (sign, ink, wash) = match line {
            DiffRow::Added(_) => ("+", status.success, Some(status.success.alpha(0.08))),
            DiffRow::Removed(_) => ("−", status.danger, Some(status.danger.alpha(0.08))),
            _ => (" ", cx.theme().muted_foreground, None),
        };
        let text = match line {
            DiffRow::Context(l) | DiffRow::Added(l) | DiffRow::Removed(l) => l.clone(),
            DiffRow::Skipped(_) => unreachable!(),
        };
        out.push(
            div()
                .h_flex()
                // **Top, not centre.** A line that wraps has to keep its number
                // and its sign level with its *first* row, or the column down
                // the side stops being a ruler the moment anything is long.
                .items_start()
                .w_full()
                .min_w_0()
                .when_some(wash, |row, wash| row.bg(wash))
                .child(
                    div()
                        .flex_none()
                        .w(DIFF_NUM_W)
                        .px(DIFF_NUM_PAD)
                        .text_right()
                        .whitespace_nowrap()
                        .text_color(cx.theme().muted_foreground.opacity(0.6))
                        .child(format!("{n}")),
                )
                .child(
                    div()
                        .flex_none()
                        .w(DIFF_SIGN_W)
                        .text_center()
                        .text_color(ink)
                        .child(sign),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .pr(DIFF_TEXT_PAD)
                        .text_color(cx.theme().foreground)
                        .child(text),
                )
                .into_any_element(),
        );
    }
    out
}

/// The control that opens a detail and the one that shuts it, which are one
/// control.
///
/// A pill rather than a bare word: it sits *over* the fade at the cut, so it
/// needs a plate of its own or it is read against the text it is covering.
fn fold_pill(
    session: &Entity<ChatSession>,
    target: TranscriptItemId,
    label: impl Into<SharedString>,
    open: bool,
    cx: &App,
) -> gpui::Div {
    let session = session.clone();
    div()
        .w_full()
        .h_flex()
        .justify_center()
        // Opened, the control is outside the scrolling box and needs the rule
        // that says so; closed, it is floating over the fade and must not draw
        // a second edge across it.
        .when(open, |row| row.border_t_1().border_color(cx.theme().border))
        .py(STACK_GAP)
        .child(
            crate::controls::action(("detail-fold", fold_key(target)))
                .ghost()
                .py(PILL_PAD_Y)
                .px(PILL_PAD_X)
                .h_auto()
                .rounded_full()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().muted)
                .label(label.into())
                .on_click(move |_, _, cx: &mut App| {
                    session.update(cx, |s, cx| {
                        s.chat.toggle_tool_output(target, 0);
                        cx.notify();
                    });
                }),
        )
}

fn path_for_display(root: &Path, value: &str) -> String {
    let path = Path::new(value);
    path.strip_prefix(root)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

/// The command a permission is asking to run.
///
/// **An element of its own because it needs the window.** How far it may open
/// is a share of the viewport, and the keys that answer the card hang off a
/// focus handle the window owns — neither is reachable from a plain builder,
/// and both belong to the one box that holds the command rather than to the
/// pane four levels up.
///
/// What it draws is the agent's text and nothing else: the lines are the
/// newlines the agent wrote, in the order it wrote them, wrapped where the box
/// runs out rather than reflowed. A command edited on its way to the screen is
/// a command approved in one form and run in another.
#[derive(IntoElement)]
pub(super) struct CommandBlock {
    pub(super) session: Entity<ChatSession>,
    pub(super) target: TranscriptItemId,
    /// The whole command, which is what Copy hands back whether or not the
    /// block is folded.
    pub(super) command: SharedString,
    pub(super) lines: Vec<SharedString>,
    /// Real lines behind the fold; zero when the block is whole.
    pub(super) hidden: usize,
    /// How tall the panel this is drawn in was last frame, which is what the
    /// opened block is bounded against. `None` before the list has measured
    /// itself, where the window is the only answer there is.
    pub(super) well: Option<gpui::Pixels>,
    /// Whether the command has more lines than the block draws unopened, which
    /// stays true once it has been opened and `hidden` has gone back to zero.
    /// Asked of the model rather than worked out from `hidden` here: where the
    /// fold falls is a rule about the command, and a second spelling of it at
    /// this call site is a second place for it to move.
    pub(super) long: bool,
    pub(super) total: usize,
    pub(super) expanded: bool,
}

impl RenderOnce for CommandBlock {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let key = fold_key(self.target);
        let folded = self.hidden > 0;
        let long = self.long;
        // **Always a gutter, on every command.** It was drawn only past the
        // second line, on the reasoning that a single line has nothing to be
        // told apart from -- which is true of the numbering and false of
        // everything else the column does. A block with no gutter is a
        // different-looking card, and which card a permission gets was decided
        // by whether the agent happened to put a newline in it: two grants an
        // inch apart in one transcript, drawn as two kinds of thing, neither of
        // them the reader's doing. The width is the digits of the count, so the
        // one-line case costs a single character.
        let gutter = rems(MONO_ADVANCE * CODE_TEXT.0 * self.total.to_string().len() as f32);
        // **A share of the panel this is drawn in, not of the window.** What
        // the bound is for is the card's own heading staying on screen with the
        // command it belongs to, and the card is in the conversation -- so with
        // a dock open, half the window is taller than the whole panel and an
        // opened command pushes *Permission required* off the top, which is the
        // one thing the share was put here to stop. The window is the fallback
        // for the frame before the list has measured itself, where it is the
        // only answer there is.
        let ceiling =
            self.well.unwrap_or_else(|| window.viewport_size().height) * COMMAND_OPEN_SHARE;
        // The command scrolls inside a `gpui::list` row, so it needs a handle
        // of its own and a mask over it: a bubble listener runs too late there,
        // the transcript having already spent the same wheel delta scrolling
        // itself. Without this an opened command is a box the wheel slides the
        // conversation behind.
        let scroll = window
            .use_keyed_state(("perm-command-scroll-state", key), cx, |_, _| {
                ScrollHandle::default()
            })
            .read(cx)
            .clone();
        // **Whether the *box* ran out of height, which is not the question the
        // fold asks.** The fold counts the newlines the agent wrote, and that
        // is right for a fold: measured in drawn rows instead it would close a
        // two-line command on a narrow pane and leave a ten-line one open on a
        // wide one, which is a fold nobody can predict. But a single line three
        // thousand characters long answers *no* to that question and fills the
        // well anyway -- so every affordance hung off the fold went away in the
        // one case where the text runs past the bottom edge with nothing
        // holding it back. One line, one `curl`, no gutter to number, no lines
        // held back to unfold, and the reader with no way to tell that what
        // they can see is not all of it.
        //
        // Last frame's, like everything else measured here. The first frame of
        // a command draws without these and the second has them, which is a
        // frame nobody can see.
        let overflows = scroll.max_offset().y > gpui::px(0.);
        // Text out of sight *below what is drawn*, by either route: lines the
        // fold is holding back, or a box scrolled somewhere above its own end.
        let more_below = folded || scroll.max_offset().y - scroll.offset().y.abs() > gpui::px(1.);

        plain_box(cx)
            .relative()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .py_2p5()
            .pl_3()
            // The copy button's own column, kept clear of the text rather than
            // laid over it: a button that covers the first line of a command
            // covers the part of it somebody is most likely to be reading.
            .pr(COPY_SIZE + BLOCK_INSET + BLOCK_INSET)
            .child(
                div()
                    .id(("perm-command-scroll", key))
                    .v_flex()
                    .w_full()
                    // **Folded, it is a height; opened, it is a share of the
                    // panel.** Both are bounds on drawn rows rather than on the
                    // agent's newlines, which is the only kind of bound that
                    // holds for a command of one very long line -- eight real
                    // lines of a base64 blob is still a screenful of wrapped
                    // rows, and one line of it is too.
                    .max_h(match self.expanded {
                        true => ceiling,
                        false => FOLD_H.to_pixels(window.rem_size()).min(ceiling),
                    })
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .children(self.lines.into_iter().enumerate().map(|(n, line)| {
                        let row = div().h_flex().items_start().gap_3().w_full();
                        row.child(
                            div()
                                .flex_none()
                                .w(gutter)
                                .text_right()
                                // The numbers are a ruler and have to stay one
                                // column: wrapped, they would renumber
                                // themselves down the side of the command.
                                .whitespace_nowrap()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("{}", n + 1)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                // A blank line in a script is a line, and an
                                // empty box is no rows tall — which slides
                                // every number after it up against the wrong
                                // line of the command.
                                .child(match line.is_empty() {
                                    true => SharedString::from(" "),
                                    false => line,
                                }),
                        )
                    })),
            )
            // Over the text and under the copy button, which is added after it:
            // the mask takes the wheel in the capture phase and nothing else,
            // so a press still reaches whatever is drawn on top of it.
            .child(ScrollableMask::new(Axis::Vertical, &scroll).id(("perm-command-mask", key)))
            .child(
                crate::controls::action(("perm-copy", key))
                    .ghost()
                    .absolute()
                    .top(BLOCK_INSET)
                    .right(BLOCK_INSET)
                    .size(COPY_SIZE)
                    .icon(Icon::new(IconName::Copy).size(COPY_ICON))
                    .tooltip("Copy the whole command")
                    // **Drawn on every command, never waiting to be hovered.**
                    // It was hidden until the pointer arrived on anything short,
                    // as one more thing beside a command that fits whole -- but
                    // a control that appears under the pointer is a control
                    // nobody finds who was not already reaching for it, and this
                    // is the card where the text is most likely to be wanted
                    // somewhere else: read elsewhere, pasted into a shell, kept
                    // for the record of what was approved. A command that
                    // scrolls cannot be selected out by dragging either, since
                    // the drag that reaches its end is the drag that moves the
                    // box.
                    .on_click(move |_, _, cx: &mut App| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                            self.command.to_string(),
                        ));
                    }),
            )
            // **What says the command does not end where the box does**, and it
            // answers that question rather than the fold's. Lines held back
            // behind a fold and a box scrolled short of its own end are the
            // same fact to a reader, and the second of them used to draw
            // nothing at all -- the justification being that a scrollbar was
            // already saying it, which was true of the blocking body beside
            // this and never of this. It goes as soon as the end is reached, so
            // it is never a gradient laid over the last line of a command
            // somebody is being asked to approve.
            .when(more_below, |block| {
                block.child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .h(FOLD_ROW + FOLD_ROW)
                        .bg(gpui::linear_gradient(
                            180.,
                            gpui::linear_color_stop(cx.theme().muted.alpha(0.), 0.),
                            gpui::linear_color_stop(cx.theme().muted, 0.75),
                        )),
                )
            })
            // Drawn over the fade rather than under it, and **only once the
            // block has been opened**. Folded, the way to the rest of the
            // command is the control at the corner and the scrollbar would be a
            // second, quieter answer to the same question -- one that moves the
            // text without ever saying how much there is. Opened, it is the only
            // thing that says how far this runs.
            .when(overflows && self.expanded, |block| {
                block.child(Scrollbar::vertical(&scroll).mode(ScrollbarMode::Always))
            })
            // **Offered wherever anything is out of sight, by either route.**
            // Lines the fold is holding back and a box that has run out of
            // height are the same fact to a reader, and gating this on the line
            // count alone left a command of one very long line with no way to
            // open it at all. Where nothing is hidden there is still no control,
            // for the reason there never was: a fold that reveals nothing is a
            // button that has to be pressed to learn it does nothing.
            .when(
                more_below || self.expanded && (long || overflows),
                |block| {
                    block.child(
                        crate::controls::action(("perm-fold", key))
                            .ghost()
                            .absolute()
                            .bottom(BLOCK_INSET)
                            .right(BLOCK_INSET)
                            .h(FOLD_ROW)
                            .px_2()
                            .rounded(cx.theme().radius)
                            // On its own plate, because it sits over the end of
                            // the command: the fade underneath it is the text
                            // it would otherwise be read against.
                            .bg(cx.theme().muted)
                            // The count is what there is more *of*, so it is
                            // said only where lines are what is being held
                            // back. A single line that wraps to a screenful has
                            // no second line to promise, and *Show all · 1
                            // lines* counts the wrong thing and miscounts it.
                            .label(match (self.expanded, self.total > 1) {
                                (true, _) => "Show less".to_string(),
                                (false, true) => format!("Show all · {} lines", self.total),
                                (false, false) => "Show all".to_string(),
                            })
                            .on_click({
                                let (session, target) = (self.session, self.target);
                                move |_, _, cx: &mut App| {
                                    session.update(cx, |s, cx| {
                                        s.chat.toggle_permission(target);
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                },
            )
    }
}

/// The path a permission's command would run in, shortened from the front.
///
/// **From the front, because a path is read from its tail**: the last
/// components are what tell two checkouts of one project apart, and they are
/// exactly what trimming the end throws away. The whole of it is in the
/// tooltip, so the line identifies the directory and the hover confirms it.
pub(super) fn command_cwd(root: &Path) -> Option<(SharedString, SharedString)> {
    let full = root.to_string_lossy().into_owned();
    if full.is_empty() {
        return None;
    }
    // `~` where the home directory is: a line that is mostly somebody's user
    // name is a line spent saying where the machine keeps its accounts.
    let shown = std::env::home_dir()
        .and_then(|home| root.strip_prefix(&home).ok())
        .map(|rest| format!("~/{}", rest.to_string_lossy()))
        .unwrap_or_else(|| full.clone());
    Some((
        crate::rail::ellipsize_front(&shown, MAX_CWD),
        SharedString::from(full),
    ))
}

/// Characters of the working directory the meta line carries.
const MAX_CWD: usize = 48;
