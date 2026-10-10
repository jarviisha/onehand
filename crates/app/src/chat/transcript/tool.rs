use super::fold_key;
use super::metrics::{
    BLOCK_INSET, COMMAND_FADE, COMMAND_H, COMMAND_SHARE, COPY_ICON, COPY_SIZE, LARGE_DIFF,
    MONO_ADVANCE, PREVIEW_DIFF, PREVIEW_OUT, STATE_TINT, TEXT_SM, TIGHT_GAP, radius_control,
};
use super::parts::{
    ActivityRow, Object, RowMark, accent, activity_row, copy_button, detail_well, line_counts,
    plain_box, row_note, scrolled, sideways,
};
use crate::chat::session::ChatSession;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Axis, ClickEvent, Entity, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Window, div, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::scroll::{ScrollableMask, Scrollbar, ScrollbarMode};
use gpui_component::{ActiveTheme, Icon, IconName, IconNamed as _, Sizable as _, StyledExt};
use onehand_core::acp::{ToolContent, ToolKind, ToolStatus};
use onehand_core::chat::activity;
use onehand_core::chat::{ToolItem, TranscriptItemId};
use onehand_core::diff::Row as DiffRow;
use std::path::Path;

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
    plain_box(cx)
        .relative()
        .map(|well| match detail {
            Detail::Command { command, output } => {
                command_detail(session, t, target, &command, &output, well, cx)
            }
            Detail::Diffs => diff_detail(session, t, target, well, cx),
            Detail::Image(bytes) => well.p_3().map(|well| match session.read(cx).image(&bytes) {
                Some(handle) => well.child(gpui::img(handle).max_w_full()),
                None => well
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("[unrecognized image, {} bytes]", bytes.len())),
            }),
            Detail::Lines { lines, hidden } => well
                .p_3()
                .text_color(cx.theme().muted_foreground)
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
        })
        // **Drawn always, never waiting to be hovered.** A control that appears
        // under the pointer is one nobody finds who was not already reaching
        // for it -- and this is the well whose text is most likely to be
        // wanted somewhere else: pasted into a shell, quoted in a bug report.
        .children(copy.map(|text| {
            div()
                .absolute()
                .top(TIGHT_GAP)
                .right(TIGHT_GAP)
                .rounded(radius_control(cx))
                .bg(cx.theme().muted)
                .child(copy_button(("detail-copy", key), text).tooltip("Copy this"))
        }))
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
                                false => cx.theme().muted_foreground,
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

/// How many added and removed lines the drawn rows of `t` hold, walked under
/// the same budget the rows are drawn with.
fn changed_drawn(t: &ToolItem) -> usize {
    let mut budget = MAX_DIFF_LINES;
    let mut changed = 0;
    for key in 0..t.call.content.len() {
        for row in t.diff_rows.get(&key).map(Vec::as_slice).unwrap_or_default() {
            if budget == 0 {
                return changed;
            }
            budget -= 1;
            if matches!(row, DiffRow::Added(_) | DiffRow::Removed(_)) {
                changed += 1;
            }
        }
    }
    changed
}

/// Every edit the step made, one diff after another.
fn diff_detail(
    session: &Entity<ChatSession>,
    t: &ToolItem,
    target: TranscriptItemId,
    well: gpui::Div,
    cx: &App,
) -> gpui::Div {
    let open = t.out_open.contains(&0);
    let key = fold_key(target);
    let changed: usize = t
        .diff_summary
        .iter()
        .map(|(_, plus, minus)| plus + minus)
        .sum();
    // **Offered rather than drawn.** A diff this size is searched and not read,
    // and every line of it is an element in a list that is already virtualising
    // rows for the same reason.
    if changed > LARGE_DIFF && !open {
        let session = session.clone();
        return well
            .p_3()
            .font_family(cx.theme().font_family.clone())
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("Large diff · {changed} changed lines, not drawn"))
                    .child(div().flex_1())
                    .child(
                        crate::controls::action(("load-diff", key))
                            .ghost()
                            .xsmall()
                            .label("Load diff")
                            .on_click(move |_, _, cx: &mut App| {
                                session.update(cx, |s, cx| {
                                    s.chat.toggle_tool_output(target, 0);
                                    cx.notify();
                                });
                            }),
                    ),
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
    // What the budget left out, said after the rows rather than cut silently.
    let left = changed.saturating_sub(changed_drawn(t));

    well.py_3()
        .v_flex()
        .gap_1()
        .child(scrolled(
            open,
            key,
            div().child(sideways(("diff-rows", key), div().v_flex().children(rows))),
        ))
        .children((hidden > 0 || open).then(|| {
            div().px_3().child(fold_link(
                session,
                target,
                match open {
                    true => "Show less".to_string(),
                    false => format!("Show {hidden} more lines"),
                },
                cx,
            ))
        }))
        .children((open && left > 0).then(|| {
            div()
                .px_3()
                .text_xs()
                .font_family(cx.theme().font_family.clone())
                .text_color(cx.theme().muted_foreground)
                .child(format!("{left} more changed lines not shown"))
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
        .px_3()
        .rounded_none()
        .label(shown)
        .text_color(accent(cx))
        .on_click(move |_, _, cx: &mut App| {
            session.update(cx, |_, cx| {
                cx.emit(crate::chat::session::ChatEvent::OpenFile(full.clone()))
            });
        })
        .into_any_element()
}

/// One diff, a row per line: a muted line number, the sign, and the text,
/// added and removed lines on their state's ink thinned to a fill.
///
/// The number is the line's in the file as it is now, so a removed line has
/// none and a skipped run moves the count on by what it skipped.
pub(in crate::chat) fn diff_rows(
    hunks: &[DiffRow],
    budget: &mut usize,
    cx: &App,
) -> Vec<gpui::AnyElement> {
    let status = crate::theme::status_ink(cx);
    let muted = cx.theme().muted_foreground;
    let mut out = Vec::new();
    let mut number = 1usize;
    for line in hunks {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let (sign, ink, text) = match line {
            // **A skipped run is a line of its own, in the ink that means it
            // can be opened**, the transcript's word for "there is more
            // behind this".
            DiffRow::Skipped(count) => {
                out.push(
                    div()
                        .px_3()
                        .bg(accent(cx).opacity(STATE_TINT))
                        .text_xs()
                        .text_color(accent(cx))
                        .child(format!("{count} unchanged lines"))
                        .into_any_element(),
                );
                number += count;
                continue;
            }
            DiffRow::Added(l) => ("+", Some(status.success), l),
            DiffRow::Removed(l) => ("−", Some(status.danger), l),
            DiffRow::Context(l) => (" ", None, l),
        };
        let shown = (sign != "−").then(|| {
            number += 1;
            (number - 1).to_string()
        });
        out.push(
            div()
                .h_flex()
                .px_3()
                .when_some(ink, |row, ink| row.bg(ink.opacity(STATE_TINT)))
                .child(
                    div()
                        .w_12()
                        .flex_none()
                        .pr_3()
                        .text_right()
                        .text_color(muted)
                        .child(shown.unwrap_or_default()),
                )
                .child(
                    div()
                        .w_4()
                        .flex_none()
                        .text_color(ink.unwrap_or(muted))
                        .child(sign),
                )
                .child(
                    div()
                        .whitespace_nowrap()
                        .text_color(cx.theme().foreground)
                        .child(text.clone()),
                )
                .into_any_element(),
        );
    }
    out
}

/// The link that shows what a well holds back and takes it away again: plain
/// words in the quiet ink, lit under the pointer.
fn fold_link(
    session: &Entity<ChatSession>,
    target: TranscriptItemId,
    label: impl Into<SharedString>,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let session = session.clone();
    let lit = cx.theme().foreground;
    div()
        .id(("detail-fold", fold_key(target)))
        .cursor_pointer()
        .font_family(cx.theme().font_family.clone())
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .hover(move |link| link.text_color(lit))
        .on_click(move |_, _, cx: &mut App| {
            session.update(cx, |s, cx| {
                s.chat.toggle_tool_output(target, 0);
                cx.notify();
            });
        })
        .child(label.into())
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
    pub(super) target: TranscriptItemId,
    /// The whole command, which is what Copy hands back.
    pub(super) command: SharedString,
    /// Every line of it: the box scrolls rather than holding lines back.
    pub(super) lines: Vec<SharedString>,
    /// How tall the panel this is drawn in was last frame, which is what the
    /// box is bounded against. `None` before the list has measured itself,
    /// where the window is the only answer there is.
    pub(super) well: Option<gpui::Pixels>,
}

impl RenderOnce for CommandBlock {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let key = fold_key(self.target);
        // **Always a gutter, on every command.** It was drawn only past the
        // second line, on the reasoning that a single line has nothing to be
        // told apart from -- which is true of the numbering and false of
        // everything else the column does. A block with no gutter is a
        // different-looking card, and which card a permission gets was decided
        // by whether the agent happened to put a newline in it: two grants an
        // inch apart in one transcript, drawn as two kinds of thing, neither of
        // them the reader's doing. The width is the digits of the count, so the
        // one-line case costs a single character.
        let total = self.lines.len().max(1);
        let gutter = rems(MONO_ADVANCE * TEXT_SM.0 * total.to_string().len() as f32);
        // **A share of the panel this is drawn in, not of the window.** What
        // the bound is for is the card's own heading staying on screen with the
        // command it belongs to, and the card is in the conversation -- so with
        // a dock open, half the window is taller than the whole panel and an
        // opened command pushes *Permission required* off the top, which is the
        // one thing the share was put here to stop. The window is the fallback
        // for the frame before the list has measured itself, where it is the
        // only answer there is.
        let ceiling = self.well.unwrap_or_else(|| window.viewport_size().height) * COMMAND_SHARE;
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
        // Last frame's, like everything else measured here. The first frame of
        // a command draws without these and the second has them, which is a
        // frame nobody can see.
        let overflows = scroll.max_offset().y > gpui::px(0.);
        let more_below = scroll.max_offset().y - scroll.offset().y.abs() > gpui::px(1.);

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
                    // **A bound on drawn rows rather than on the agent's
                    // newlines**, which is the only kind that holds for a
                    // command of one very long line: one line of a base64 blob
                    // is a screenful of wrapped rows. Past it the box scrolls.
                    .max_h(COMMAND_H.to_pixels(window.rem_size()).min(ceiling))
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
            // **What says the command does not end where the box does.** It goes
            // as soon as the end is reached, so
            // it is never a gradient laid over the last line of a command
            // somebody is being asked to approve.
            .when(more_below, |block| {
                block.child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .h(COMMAND_FADE)
                        .bg(gpui::linear_gradient(
                            180.,
                            gpui::linear_color_stop(cx.theme().muted.alpha(0.), 0.),
                            gpui::linear_color_stop(cx.theme().muted, 0.75),
                        )),
                )
            })
            // Over the fade: the one thing that says how far the command runs.
            .when(overflows, |block| {
                block.child(Scrollbar::vertical(&scroll).mode(ScrollbarMode::Always))
            })
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
