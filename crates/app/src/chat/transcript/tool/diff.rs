//! A tool's diff: the lines it changed, their path, and the rows they draw as.

use super::*;
use crate::controls::Short as _;

/// A diff's rows as the lines they would be in a file.
pub(super) fn diff_text(t: &ToolItem) -> Vec<String> {
    (0..t.call.content.len())
        .filter_map(|key| t.diff_rows.get(&key))
        .flat_map(|rows| rows_text(rows))
        .collect()
}

/// `rows` as the lines of a unified diff: what Copy and *Open* hand over.
pub(crate) fn rows_text(rows: &[DiffRow]) -> Vec<String> {
    rows.iter()
        .map(|row| match row {
            DiffRow::Context(l) => format!(" {l}"),
            DiffRow::Added(l) => format!("+{l}"),
            DiffRow::Removed(l) => format!("-{l}"),
            DiffRow::Skipped(n) => format!("@@ {n} unchanged lines @@"),
        })
        .collect()
}

/// Every edit the step made, one diff after another, in one box.
pub(super) fn diff_detail(
    session: &Entity<ChatSession>,
    t: &ToolItem,
    target: TranscriptItemId,
    text: Vec<String>,
    copy: Option<String>,
    cx: &App,
) -> gpui::Div {
    let open = session.read(cx).out_is_open(target, 0);
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
        return plain_box(cx)
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
                            .quiet(cx)
                            .short()
                            .label("Load diff")
                            .on_click(move |_, _, cx: &mut App| {
                                session.update(cx, |s, cx| {
                                    s.toggle_out(target, 0);
                                    cx.notify();
                                });
                            }),
                    ),
            );
    }

    let root = session.read(cx).chat().root.clone();
    let mut budget = MAX_DIFF_LINES;
    let mut rows: Vec<gpui::AnyElement> = Vec::new();
    let files = t
        .call
        .content
        .iter()
        .filter(|c| matches!(c, ToolContent::Diff { .. }))
        .count();
    for (key, content) in t.call.content.iter().enumerate() {
        let ToolContent::Diff { path, old, .. } = content else {
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
        rows.extend(diff_rows(hunks, &mut budget, old.is_none(), cx));
    }
    // What the budget left out is said under the box, counted in the diff's
    // own lines, the file headers not among them.
    let drawn = MAX_DIFF_LINES - budget;
    // A diff loaded past the large-diff bound can be put away again.
    let head = (changed > LARGE_DIFF).then(|| {
        super::fold_link(session, target, "Put the large diff away", cx)
            .font_family(cx.theme().font_family.clone())
            .into_any_element()
    });
    detail_box(
        DetailBox {
            key,
            head,
            lines: rows,
            total: text.len(),
            drawn,
            from_end: false,
            whole: (!text.is_empty()).then(|| (text.join("\n").into(), Some("diff".into()))),
            copy,
        },
        cx,
    )
}

/// Which file the rows under it belong to, and the way into it.
pub(super) fn diff_path(
    session: &Entity<ChatSession>,
    root: &Path,
    key: usize,
    path: &str,
    cx: &App,
) -> gpui::AnyElement {
    let shown = onehand_core::chat::activity::display_path(root, path);
    // Resolved here, where the project root is known: the Workbench opens
    // whatever path it is handed, and a relative one there resolves against the
    // process working directory.
    let full = onehand_core::editor::resolve_in_root(root, path);
    let session = session.clone();
    div()
        .h_flex()
        .px_3()
        .child(
            crate::controls::action(("diff-path", key))
                .quiet(cx)
                .short()
                .min_w_0()
                .label(shown)
                .text_color(accent(cx))
                .on_click(move |_, _, cx: &mut App| {
                    session.update(cx, |_, cx| {
                        cx.emit(crate::chat::session::ChatEvent::OpenFile(full.clone()))
                    });
                }),
        )
        .into_any_element()
}

/// One diff, a row per line: a muted line number, the sign, and the text,
/// added and removed lines on their state's ink thinned to a fill.
///
/// The number is the line's in the file as it is now, so a removed line has
/// none and a skipped run moves the count on by what it skipped.
///
/// `fresh` is a file the edit brought into being: its content, numbered, with
/// no fill and no sign, since every line of it is new and marking each one
/// says nothing.
pub(crate) fn diff_rows(
    hunks: &[DiffRow],
    budget: &mut usize,
    fresh: bool,
    cx: &App,
) -> Vec<gpui::AnyElement> {
    let status = crate::theme::status_ink(cx);
    let muted = cx.theme().muted_foreground;
    // A changed line's wash: its hue mixed into the well it lies in, one solid
    // step rather than the hue thinned over whatever is under it.
    let wash = |base| onehand_ui::tint_over(base, cx.theme().muted, cx.theme().foreground).fill;
    let (added, removed) = (wash(cx.theme().green), wash(cx.theme().red));
    let mut out = Vec::new();
    let mut number = 1usize;
    for line in hunks {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let (sign, ink, text) = match line {
            // A skipped run is a line of its own in meta text: nothing opens
            // it, so it says how much is behind it and takes no fill.
            DiffRow::Skipped(count) => {
                out.push(
                    div()
                        .px_3()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("{count} unchanged lines"))
                        .into_any_element(),
                );
                number += count;
                continue;
            }
            DiffRow::Added(l) if fresh => (" ", None, l),
            DiffRow::Added(l) => ("+", Some((status.success, added)), l),
            DiffRow::Removed(l) => ("−", Some((status.danger, removed)), l),
            DiffRow::Context(l) => (" ", None, l),
        };
        let shown = (!matches!(line, DiffRow::Removed(_))).then(|| {
            number += 1;
            (number - 1).to_string()
        });
        out.push(
            div()
                .h_flex()
                .px_3()
                .when_some(ink, |row, (_, fill)| row.bg(fill))
                .child(
                    div()
                        .w_12()
                        .flex_none()
                        .pr_3()
                        .text_right()
                        .text_color(muted)
                        .child(shown.unwrap_or_default()),
                )
                .when(!fresh, |row| {
                    row.child(
                        div()
                            .w_4()
                            .flex_none()
                            .text_color(ink.map_or(muted, |(ink, _)| ink))
                            .child(sign),
                    )
                })
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
