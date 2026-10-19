use super::fold_key;
use super::metrics::{
    COMMAND_FADE, COMMAND_H, COMMAND_SHARE, LARGE_DIFF, MONO_ADVANCE, PREVIEW_DIFF, PREVIEW_OUT,
    TIGHT_GAP,
};
use super::parts::{
    ActivityRow, Object, RowMark, accent, activity_row, copy_button, detail_well, line_counts,
    plain_box, row_note, scrolled, show_more, sideways,
};
use crate::chat::session::ChatSession;
use crate::controls::Quiet as _;
use crate::tokens::SCROLLBAR_W;
use gpui::StyledImage as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Axis, ClickEvent, Entity, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Window, div, rems,
};
use gpui_component::scroll::{ScrollableMask, Scrollbar, ScrollbarMode};
use gpui_component::{ActiveTheme, IconName, IconNamed as _, StyledExt};
use onehand_core::acp::{ToolContent, ToolKind, ToolStatus};
use onehand_core::chat::activity;
use onehand_core::chat::{ToolItem, TranscriptItemId};
use onehand_core::diff::Row as DiffRow;
use std::path::Path;

mod command;
mod diff;
pub(super) use command::{CommandBlock, command_cwd};
pub(in crate::chat) use diff::diff_rows;
use diff::{diff_detail, diff_text};

/// Less than a pixel left to scroll is the scroller's rounding, not more text
/// under the fade.
const SCROLL_SLACK: gpui::Pixels = gpui::px(1.);

/// Diff lines drawn per tool card, **shared across all its hunks** — a
/// MultiEdit touching twenty files must not cost twenty times the budget.
const MAX_DIFF_LINES: usize = 200;

/// Lines of a mono output well before the tail is dropped.
const MAX_MONO_LINES: usize = 60;

// ── tool call ───────────────────────────────────────────────────────────────

/// The drawing for a tool's kind, at the head of its row.
fn kind_icon(kind: ToolKind) -> SharedString {
    match kind {
        ToolKind::Read => IconName::File.path(),
        ToolKind::Search => IconName::Search.path(),
        // A pencil on a page: the bundled set has no pencil of any kind.
        ToolKind::Edit => crate::icons::Icon::SquarePen.path(),
        ToolKind::Delete => IconName::Delete.path(),
        ToolKind::Move => IconName::ArrowRight.path(),
        ToolKind::Execute => IconName::SquareTerminal.path(),
        ToolKind::Fetch => IconName::Globe.path(),
        ToolKind::Think => IconName::Info.path(),
        ToolKind::Other => IconName::Settings2.path(),
    }
}

/// A tool step: one activity row, and what it opens into.
///
/// **One geometry for every kind and every status.** What differs between a
/// read and a command is only the shape of the thing underneath, once somebody
/// has asked for it.
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
    let object = match t.call.kind {
        // A command is not a path and must not be split at its last slash.
        ToolKind::Execute => Some(Object::plain(activity::first_line_trunc(
            &onehand_core::chat::redact(&presented.subject),
            160,
        ))),
        _ if presented.subject.trim().is_empty() => None,
        _ => Some(Object::path(path_for_display(&root, &presented.subject))),
    };

    // **The code where there is one, the word where there is not.** Only a
    // command run through the terminal extension reports a status, so a
    // failure arriving as a plain tool call has nothing but the word -- and
    // printing `failed` over a step that told us it exited 101 throws away the
    // one thing that says *what* went wrong.
    let failure = (t.call.status == ToolStatus::Failed).then(|| {
        row_note(
            match t.exit_code {
                Some(code) => format!("exit {code}"),
                None => "failed".to_string(),
            },
            crate::theme::status_ink(cx).danger,
        )
    });
    let meta = div()
        .flex_none()
        .h_flex()
        .items_center()
        .gap_2()
        .children(line_counts(added, removed, cx))
        .children(failure)
        .into_any_element();

    let mut row = ActivityRow::new(
        ("tool", fold_key(target)).into(),
        RowMark::of(t.call.status),
        kind_icon(t.call.kind),
        presented.action,
    )
    .object(object)
    .meta(Some(meta))
    .fold(detail.is_some().then_some(open));
    // A file that is gone strikes its own name out.
    row.struck = t.call.kind == ToolKind::Delete;

    div()
        .v_flex()
        .w_full()
        .min_w_0()
        .child(activity_row(row, toggle, cx))
        .children(
            open.then_some(())
                .and(detail)
                .map(|detail| detail_well(detail_frame(session, t, target, detail, cx))),
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

/// The well a row opens into, carrying nothing but the text and a way to copy
/// it.
fn detail_frame(
    session: &Entity<ChatSession>,
    t: &ToolItem,
    target: TranscriptItemId,
    detail: Detail,
    cx: &App,
) -> gpui::Div {
    let copy = detail.text(&diff_text(t));
    let key = fold_key(target);
    let body = div().flex_1().min_w_0().map(|well| match detail {
        Detail::Command { command, output } => {
            command_detail(session, t, target, &command, &output, well, cx)
        }
        Detail::Diffs => diff_detail(session, t, target, well, cx),
        Detail::Image(bytes) => well.p_3().map(|well| {
            // A recognised signature is no promise the bytes decode, and gpui
            // draws nothing for an image that fails to; both say so the same way.
            let ink = cx.theme().muted_foreground;
            let unrecognized = format!("[unrecognized image, {} bytes]", bytes.len());
            match session.read(cx).image(&bytes) {
                Some(handle) => {
                    well.child(gpui::img(handle).max_w_full().with_fallback(move || {
                        div()
                            .text_color(ink)
                            .child(unrecognized.clone())
                            .into_any_element()
                    }))
                }
                None => well.text_color(ink).child(unrecognized),
            }
        }),
        Detail::Lines { lines, hidden } => well
            .p_3()
            .text_color(crate::theme::meta_ink(cx))
            .child(sideways(
                ("detail-lines", key),
                div().v_flex().children(
                    lines
                        .into_iter()
                        .map(|line| div().whitespace_nowrap().child(line)),
                ),
            ))
            // Bounded, and the bound is counted rather than swallowed.
            .children((hidden > 0).then(|| {
                div()
                    .text_xs()
                    .font_family(cx.theme().font_family.clone())
                    .child(format!("and {hidden} more lines"))
            })),
    });
    // **Drawn always, never waiting to be hovered**, and in a column of its
    // own rather than over the text: a control that appears under the pointer
    // is one nobody finds who was not already reaching for it, and one laid
    // over the first line covers the part most likely to be read.
    plain_box(cx)
        .h_flex()
        .items_start()
        .child(body)
        .children(copy.map(|text| {
            div().flex_none().p(TIGHT_GAP).child(copy_button(
                ("detail-copy", key),
                text,
                "Copy this",
                cx,
            ))
        }))
}

/// A command, then the tail of what it printed.
///
/// **Always the tail, opened or closed.** An output says what happened at its
/// end -- the error, the summary line, the prompt coming back -- so what is
/// shown is the last lines, and what is held back is above them, which is
/// where the way to them is offered.
fn command_detail(
    session: &Entity<ChatSession>,
    t: &ToolItem,
    target: TranscriptItemId,
    command: &str,
    output: &str,
    well: gpui::Div,
    cx: &App,
) -> gpui::Div {
    let open = t.out_open.contains(&0);
    let lines: Vec<&str> = output.lines().collect();
    let cap = match open {
        true => MAX_MONO_LINES,
        false => PREVIEW_OUT,
    };
    let hidden = lines.len().saturating_sub(cap);
    let danger = crate::theme::status_ink(cx).danger;
    let key = fold_key(target);

    well.p_3()
        .v_flex()
        .gap_1()
        .child(sideways(
            ("command-head", key),
            div()
                .h_flex()
                .gap_2()
                .whitespace_nowrap()
                .child(div().text_color(cx.theme().muted_foreground).child("$"))
                .child(
                    div()
                        .text_color(cx.theme().foreground)
                        .child(command.to_string()),
                ),
        ))
        // Opened, the well is still bounded, so the link says what is
        // *still* cut rather than claiming the whole of it is on screen.
        .children((hidden > 0 || open).then(|| {
            fold_link(
                session,
                target,
                match (open, hidden) {
                    (true, 0) => "Show less".to_string(),
                    (true, n) => format!("Show less · {n} earlier lines not shown"),
                    (false, n) => format!("Show {n} earlier lines"),
                },
                cx,
            )
        }))
        .children((lines.len() > hidden).then(|| {
            scrolled(
                open,
                key,
                div().child(sideways(
                    ("command-out", key),
                    div().v_flex().children(lines[hidden..].iter().map(|line| {
                        // A failure names itself in what it printed, so the
                        // ink follows the words rather than the row.
                        div()
                            .whitespace_nowrap()
                            .text_color(match is_error_line(line) {
                                true => danger,
                                false => crate::theme::meta_ink(cx),
                            })
                            .child(line.to_string())
                    })),
                )),
            )
        }))
}

/// Whether a line of output reports a failure: one that starts with what a
/// failure starts with, or a test marked `FAILED`. A count such as `0 failed`
/// in a passing summary is not one.
pub(super) fn is_error_line(line: &str) -> bool {
    let lower = line.trim_start().to_lowercase();
    ["error", "failed", "fatal", "panic", "assertion", "thread '"]
        .iter()
        .any(|mark| lower.starts_with(mark))
        || line.ends_with(" FAILED")
        || line.contains("result: FAILED")
}

/// The [`show_more`] under a well, opening and closing what it holds back.
fn fold_link(
    session: &Entity<ChatSession>,
    target: TranscriptItemId,
    label: impl Into<SharedString>,
    cx: &App,
) -> gpui::Div {
    let session = session.clone();
    div().h_flex().child(
        show_more(("detail-fold", fold_key(target)), label, cx).on_click(
            move |_, _, cx: &mut App| {
                session.update(cx, |s, cx| {
                    s.chat.toggle_tool_output(target, 0);
                    cx.notify();
                });
            },
        ),
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
