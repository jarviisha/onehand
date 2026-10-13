//! A tool's diff: the lines it changed, their path, and the rows they draw as.

use super::*;

/// A diff's rows as the lines they would be in a file.
pub(super) fn diff_text(t: &ToolItem) -> Vec<String> {
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

/// How many added and removed lines the drawn rows of `t` hold, walked under
/// the same budget the rows are drawn with.
pub(super) fn changed_drawn(t: &ToolItem) -> usize {
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
pub(super) fn diff_detail(
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
                            .quiet(cx)
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
pub(super) fn diff_path(
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
    div()
        .h_flex()
        .px_3()
        .child(
            crate::controls::action(("diff-path", key))
                .quiet(cx)
                .xsmall()
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
pub(in crate::chat) fn diff_rows(
    hunks: &[DiffRow],
    budget: &mut usize,
    cx: &App,
) -> Vec<gpui::AnyElement> {
    let status = crate::theme::status_ink(cx);
    let muted = cx.theme().muted_foreground;
    // A changed line's wash: its hue mixed into the well it lies in, one solid
    // step rather than the hue thinned over whatever is under it.
    let wash =
        |base| onehand_plugin_host::tint_over(base, cx.theme().muted, cx.theme().foreground).fill;
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
            DiffRow::Added(l) => ("+", Some((status.success, added)), l),
            DiffRow::Removed(l) => ("−", Some((status.danger, removed)), l),
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
                .child(
                    div()
                        .w_4()
                        .flex_none()
                        .text_color(ink.map_or(muted, |(ink, _)| ink))
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
