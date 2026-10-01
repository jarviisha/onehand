use super::list::{chip, identity};
use super::mentions::FILE_LINK;
use super::{FILES_SHOWN, Form, HISTORY_SHOWN, IssuesView};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, ClipboardItem, Context, Entity, HighlightStyle,
    InteractiveElement as _, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, WeakEntity, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::{Input, Textarea};
use gpui_component::text::{TextView, TextViewState, TextViewStyle};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::issues::{LocalIssue, sync};
use onehand_plugin_host::{action, menu_below, menu_item, status_hue, status_ink};
use std::path::Path;

/// One issue, read: how it is named and its title with what can be done to it,
/// its labels, and its body.
pub(super) fn issue_view(
    root: &Path,
    issue: &LocalIssue,
    body: Option<(Entity<TextViewState>, Vec<String>)>,
    publish_to: Option<&'static str>,
    working: Option<String>,
    window: &mut Window,
    cx: &mut Context<IssuesView>,
) -> AnyElement {
    let number = issue.number;
    let open = issue.open;
    let muted = cx.theme().muted_foreground;
    // The title wraps rather than cutting: it is the one place the whole of
    // it is read, and what is done to the issue stays to its right.
    let header =
        div()
            .h_flex()
            .items_start()
            .gap_2()
            .w_full()
            .flex_none()
            .px_3()
            .pt_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .font_semibold()
                    .child(issue.title.clone()),
            )
            .child(
                div()
                    .h_flex()
                    .flex_none()
                    .items_center()
                    .gap_1()
                    .children(publish_to.map(|forge| {
                        action("issue-publish")
                            .xsmall()
                            .ghost()
                            .label(format!("Publish to {forge}"))
                            .tooltip("Open it there too, and keep the two in step")
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                view.publish(number, cx)
                            }))
                    }))
                    // A session still going on it is where the work is: a
                    // second one started here would be two agents editing
                    // one checkout from two conversations.
                    .map(|actions| match working {
                        Some(session) => actions.child(
                            action("issue-open-working")
                                .xsmall()
                                .ghost()
                                .icon(Icon::new(IconName::Bot))
                                .label("Open session")
                                .tooltip("A session is working this issue; show it")
                                .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                                    view.open_session(session.clone(), window, cx)
                                })),
                        ),
                        None if open => actions.child(
                            action("issue-work-here")
                                .xsmall()
                                .ghost()
                                .icon(Icon::new(IconName::Bot))
                                .label("Work here")
                                .tooltip(
                                    "Start a session on this checkout with the issue as its \
                                     prompt. No branch or worktree; changes are left uncommitted",
                                )
                                .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                                    view.work_here(number, window, cx)
                                })),
                        ),
                        None => actions,
                    })
                    .child(
                        action("issue-edit")
                            .xsmall()
                            .ghost()
                            .label("Edit")
                            .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                                view.open_form(Some(number), window, cx)
                            })),
                    )
                    .child(more_menu(root, issue, cx)),
            );

    // What it is at a glance: open or closed, where it lives, its labels, and
    // how urgent the body says it is.
    let state_ink = if open { status_ink(cx).success } else { muted };
    let facts = div()
        .h_flex()
        .flex_wrap()
        .items_center()
        .gap_1()
        .flex_none()
        .px_3()
        .pt_1()
        .pb_2()
        .border_b_1()
        .border_color(cx.theme().border)
        .text_xs()
        .child(
            div()
                .flex_none()
                .px_1p5()
                .rounded(cx.theme().radius)
                .bg(state_ink.opacity(0.15))
                .text_color(state_ink)
                .child(if open { "Open" } else { "Closed" }),
        )
        .child(match (issue.reference(), &issue.link) {
            (Some(reference), Some(link)) => div()
                .id("issue-reference")
                .flex_none()
                .text_color(muted)
                .cursor_pointer()
                .hover(|reference| reference.underline())
                .tooltip({
                    let tip = format!("Open on {}", link.connector);
                    move |window, cx| Tooltip::new(tip.clone()).build(window, cx)
                })
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.with_url(number, |url, cx| cx.open_url(&url), cx)
                }))
                .child(reference.to_string())
                .into_any_element(),
            _ => identity(issue, cx),
        })
        .children(issue.labels.iter().map(|label| chip(label.clone(), cx)))
        .children(priority(&issue.body).map(|priority| {
            div()
                .flex_none()
                .text_color(muted)
                .child(format!("Priority: {priority}"))
        }));
    let conflict = conflict_view(issue, cx);

    let history = history(issue, cx);

    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .v_flex()
        .child(header)
        .child(facts)
        .children(conflict)
        .child(match &body {
            Some((parsed, _)) => div()
                .flex_1()
                .min_h_0()
                .p_3()
                .child(
                    TextView::new(parsed)
                        .selectable(true)
                        .scrollable(true)
                        .style(body_style(window.rem_size(), cx))
                        .on_link_click(on_link(cx.entity().downgrade())),
                )
                .into_any_element(),
            None => div()
                .px_2()
                .py_1()
                .text_sm()
                .text_color(muted)
                .child("No description")
                .into_any_element(),
        })
        .children(body.and_then(|(_, files)| referenced(files, cx)))
        .child(history)
        .into_any_element()
}

/// Everything that happened to the issue, oldest first under its arrival:
/// each with when, in local time and how long ago, and for one a session took
/// up, a way to open that session. The latest ones only, with the cut said.
fn history(issue: &LocalIssue, cx: &mut Context<IssuesView>) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let now = onehand_core::issues::now();
    let arrived = (issue.created > 0).then(|| (issue.created, issue.arrival(), None));
    let entries: Vec<(u64, String, Option<String>)> = arrived
        .into_iter()
        .chain(
            issue
                .notes
                .iter()
                .map(|note| (note.at, note.text.clone(), note.session.clone())),
        )
        .collect();
    let left_out = entries.len().saturating_sub(HISTORY_SHOWN);
    div()
        .id("issue-history")
        .flex_none()
        .max_h(gpui::rems(14.))
        .overflow_y_scroll()
        .v_flex()
        .gap_1()
        .px_3()
        .py_2()
        .border_t_1()
        .border_color(cx.theme().border)
        .child(div().text_xs().text_color(muted).child("History"))
        .when(left_out > 0, |list| {
            list.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("… {left_out} earlier not shown")),
            )
        })
        .children(
            entries
                .into_iter()
                .skip(left_out)
                .enumerate()
                .map(|(i, (at, text, session))| {
                    div()
                        .h_flex()
                        .items_start()
                        .gap_2()
                        .text_xs()
                        .child(div().flex_none().text_color(muted).child(format!(
                            "{} · {}",
                            moment(at),
                            onehand_core::rel_time(now, at)
                        )))
                        .child(div().flex_1().min_w_0().child(text))
                        .children(session.map(|session| {
                            action(("issue-history-session", i))
                                .xsmall()
                                .ghost()
                                .label("Open session")
                                .tooltip("Show the conversation that took this issue up")
                                .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                                    view.open_session(session.clone(), window, cx)
                                }))
                        }))
                }),
        )
        .into_any_element()
}

/// `secs` since the epoch as a date and minute in the machine's own time zone,
/// which is the one the person reading it lives in.
fn moment(secs: u64) -> String {
    i64::try_from(secs)
        .ok()
        .and_then(|secs| chrono::DateTime::from_timestamp(secs, 0))
        .map(|at| {
            at.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// The first line under a *Priority* heading in `body`, list marker and bold
/// taken off, or `None` where the body has no such section or it is empty.
fn priority(body: &str) -> Option<String> {
    let heading = |line: &str| line.trim_start().starts_with('#');
    let mut lines = body.lines();
    lines.find(|line| {
        heading(line)
            && line
                .trim()
                .trim_start_matches('#')
                .trim()
                .trim_end_matches(':')
                .eq_ignore_ascii_case("priority")
    })?;
    let first = lines
        .take_while(|line| !heading(line))
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let first = first
        .strip_prefix("- ")
        .or_else(|| first.strip_prefix("* "))
        .or_else(|| first.strip_prefix("+ "))
        .unwrap_or(first)
        .replace("**", "")
        .replace("__", "");
    let first = first.trim();
    (!first.is_empty()).then(|| first.to_string())
}

/// The ⋯ menu beside *Edit*: where the issue lives on its forge, and closing
/// or reopening it. Closing is here rather than a button of its own because a
/// bare *Close* beside an issue reads as closing the view.
fn more_menu(root: &Path, issue: &LocalIssue, cx: &mut Context<IssuesView>) -> AnyElement {
    let number = issue.number;
    let open = issue.open;
    let forge = issue.link.as_ref().map(|link| link.connector.clone());
    let view = cx.entity();
    let trigger = action("issue-more")
        .xsmall()
        .ghost()
        .icon(Icon::new(IconName::Ellipsis))
        .tooltip("More actions");
    // Named by the project and the issue, so a menu held open across a switch
    // is one for this issue and never for whichever took its place.
    let id = SharedString::from(format!("issue-more-menu-{}-{number}", root.display()));
    menu_below(id, trigger, move |menu, _, _| {
        let mut menu = menu;
        if let Some(forge) = &forge {
            let (open_view, copy_view) = (view.clone(), view.clone());
            menu = menu
                .item(
                    menu_item(format!("Open on {forge}"))
                        .icon(Icon::new(IconName::ExternalLink))
                        .on_click(move |_, _, cx: &mut App| {
                            open_view.update(cx, |view, cx| {
                                view.with_url(number, |url, cx| cx.open_url(&url), cx)
                            })
                        }),
                )
                .item(
                    menu_item("Copy link")
                        .icon(Icon::new(IconName::Copy))
                        .on_click(move |_, _, cx: &mut App| {
                            copy_view.update(cx, |view, cx| {
                                view.with_url(
                                    number,
                                    |url, cx| cx.write_to_clipboard(ClipboardItem::new_string(url)),
                                    cx,
                                )
                            })
                        }),
                )
                .separator();
        }
        let view = view.clone();
        menu.item(if open {
            menu_item("Close issue")
                .icon(Icon::new(IconName::CircleX))
                .on_click(move |_, window, cx: &mut App| {
                    view.update(cx, |view, cx| view.confirm_close(number, window, cx))
                })
        } else {
            menu_item("Reopen issue")
                .icon(Icon::new(IconName::Redo))
                .on_click(move |_, _, cx: &mut App| {
                    view.update(cx, |view, cx| view.set_open(number, true, cx))
                })
        })
    })
    .into_any_element()
}

/// What a person has to decide about an issue both sides changed: each field in
/// conflict, as it reads here and as it reads on the forge, and the two ways
/// out. `None` for an issue with nothing to decide.
fn conflict_view(issue: &LocalIssue, cx: &mut Context<IssuesView>) -> Option<AnyElement> {
    let link = issue.link.as_ref()?;
    let theirs = link.conflict.as_ref()?;
    let ours = issue.snapshot();
    let number = issue.number;
    let forge = link.connector.clone();
    let open = |open: bool| if open { "open" } else { "closed" }.to_string();
    let lines: Vec<String> = sync::merge(&link.base, &ours, theirs)
        .conflicts
        .into_iter()
        .map(|field| match field {
            sync::Field::Title => {
                format!("Title — here: {}; on {forge}: {}", ours.title, theirs.title)
            }
            sync::Field::State => format!(
                "State — here: {}; on {forge}: {}",
                open(ours.open),
                open(theirs.open)
            ),
            sync::Field::Description => {
                let (here, there) = (excerpt(&ours.body), excerpt(&theirs.body));
                if here == there {
                    // Both begin alike and part further on, past what the line
                    // shows: said, or the two would read as one.
                    format!("Description — the two differ further on; here it begins: {here}")
                } else {
                    format!("Description — here: {here}\non {forge}: {there}")
                }
            }
        })
        .collect();
    let ink = status_ink(cx).warning;
    Some(
        div()
            .flex_none()
            .v_flex()
            .gap_1()
            .mx_2()
            .my_1()
            .p_2()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(ink)
            .child(
                div()
                    .text_xs()
                    .text_color(ink)
                    .child(format!("Changed here and on {forge} since the last sync:")),
            )
            .children(lines.into_iter().map(|line| div().text_xs().child(line)))
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        action("issue-keep-mine")
                            .xsmall()
                            .ghost()
                            .label("Keep mine")
                            .tooltip(format!("Send what it says here to {forge}"))
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                view.resolve(number, true, cx)
                            })),
                    )
                    .child(
                        action("issue-take-theirs")
                            .xsmall()
                            .ghost()
                            .label(format!("Take {forge}'s"))
                            .tooltip("Replace what it says here with the other side")
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                view.resolve(number, false, cx)
                            })),
                    ),
            )
            .into_any_element(),
    )
}

/// The form: title, labels, body, then Save and Cancel under what they act on.
pub(super) fn form_view(form: &Form, cx: &mut Context<IssuesView>) -> AnyElement {
    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .v_flex()
        .gap_2()
        .p_2()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(match form.editing {
                    Some(_) => match &form.editing_reference {
                        Some(reference) => format!("Editing {reference}"),
                        None => "Editing draft".to_string(),
                    },
                    None => "New issue".to_string(),
                }),
        )
        .child(Input::new(&form.title))
        .child(Input::new(&form.labels))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .child(Textarea::new(&form.body).h_full()),
        )
        .child(
            div()
                .h_flex()
                .gap_2()
                .justify_end()
                .child(
                    action("issue-form-cancel")
                        .small()
                        .ghost()
                        .label("Cancel")
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.cancel_form(cx))),
                )
                .child(
                    action("issue-form-save")
                        .small()
                        .primary()
                        .label(match form.editing {
                            Some(_) => "Save",
                            None => "Create issue",
                        })
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.save_form(cx))),
                ),
        )
        .into_any_element()
}

/// What pressing a link in the body does: a file it names opens in the editor,
/// and anything else goes to the system, as it would without this hook.
fn on_link(
    view: WeakEntity<IssuesView>,
) -> impl Fn(&SharedString, &ClickEvent, &mut Window, &mut App) + Send + Sync + 'static {
    move |url, _, window, cx| match url.strip_prefix(FILE_LINK) {
        Some(path) => {
            let _ = view.update(cx, |view, cx| view.open_path(path, window, cx));
        }
        None => cx.open_url(url),
    }
}

/// The files the body names, under it: each once, in the order it names them,
/// pressed to open in the editor. `None` for a body that names none.
fn referenced(files: Vec<String>, cx: &mut Context<IssuesView>) -> Option<AnyElement> {
    if files.is_empty() {
        return None;
    }
    let muted = cx.theme().muted_foreground;
    let left_out = files.len().saturating_sub(FILES_SHOWN);
    Some(
        div()
            .flex_none()
            .v_flex()
            .gap_0p5()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(div().text_xs().text_color(muted).child("Referenced files"))
            .children(
                files
                    .into_iter()
                    .take(FILES_SHOWN)
                    .enumerate()
                    .map(|(i, path)| {
                        let open = path.clone();
                        div()
                            .id(("issue-file", i))
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .min_w_0()
                            .px_1()
                            .rounded(cx.theme().radius)
                            .cursor_pointer()
                            .hover(|row| row.bg(cx.theme().list_hover))
                            .text_xs()
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(Icon::new(IconName::File).xsmall().text_color(muted))
                            .child(div().min_w_0().truncate().child(path))
                            .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                                view.open_path(&open, window, cx)
                            }))
                    }),
            )
            .when(left_out > 0, |list| {
                list.child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("… {left_out} more not shown")),
                )
            })
            .into_any_element(),
    )
}

/// Body styling. The renderer sizes its headings from an absolute pixel base,
/// which the panel's rem-base zoom cannot reach by itself, so the base in force
/// is written in by hand.
fn body_style(rem: gpui::Pixels, cx: &App) -> TextViewStyle {
    let theme = cx.theme();
    let mut style = TextViewStyle::default()
        .code_block(
            gpui::StyleRefinement::default()
                .p(gpui::rems(0.75))
                .text_size(gpui::rems(0.8125))
                .bg(theme.muted),
        )
        // Inline code on the well rather than the renderer's own accent, which
        // in the dark palette is a slab louder than the prose around it, and
        // in the blue the transcript gives code. The renderer styles inline
        // code through a highlight, which carries colour and background but
        // no font family and no padding, so those two are out of reach here.
        .inline_code(HighlightStyle {
            color: Some(status_hue(theme.blue, theme.foreground)),
            background_color: Some(theme.muted),
            ..HighlightStyle::default()
        });
    style.heading_base_font_size = rem;
    style
}

/// The start of a description, for the line that sets two of them side by
/// side: enough to tell them apart, bounded so a long one does not push the
/// choice off the panel.
fn excerpt(text: &str) -> String {
    const SHOWN: usize = 160;
    match text.char_indices().nth(SHOWN) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None if text.is_empty() => "(empty)".to_string(),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests;
