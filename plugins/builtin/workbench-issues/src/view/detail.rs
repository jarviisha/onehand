use super::{Form, IssuesView, NOTES_SHOWN};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, IntoElement, ParentElement, Styled, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::{Input, Textarea};
use gpui_component::text::{TextView, TextViewState, TextViewStyle};
use gpui_component::{ActiveTheme, Sizable as _, StyledExt};
use onehand_core::issues::{LocalIssue, sync};
use onehand_plugin_host::{action, status_ink};

/// One issue, read: its number and title with what can be done to it, its
/// labels, and its body.
pub(super) fn issue_view(
    issue: &LocalIssue,
    body: Option<Entity<TextViewState>>,
    publish_to: Option<&'static str>,
    window: &mut Window,
    cx: &mut Context<IssuesView>,
) -> AnyElement {
    let number = issue.number;
    let open = issue.open;
    let muted = cx.theme().muted_foreground;
    let header = div()
        .h_flex()
        .items_center()
        .gap_2()
        .w_full()
        .flex_none()
        .px_2()
        .py_1()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .flex_none()
                .text_color(muted)
                .child(format!("#{number}")),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_semibold()
                .child(issue.title.clone()),
        )
        .children(publish_to.map(|forge| {
            action("issue-publish")
                .xsmall()
                .ghost()
                .label(format!("Publish to {forge}"))
                .tooltip("Open it there too, and keep the two in step")
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.publish(number, cx)))
        }))
        .child(
            action("issue-edit")
                .xsmall()
                .ghost()
                .label("Edit")
                .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                    view.open_form(Some(number), window, cx)
                })),
        )
        .child(
            action("issue-close")
                .xsmall()
                .ghost()
                .label(if open { "Close" } else { "Reopen" })
                .on_click(
                    cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.set_open(number, !open, cx)
                    }),
                ),
        );

    let mut facts = vec![(if open { "Open" } else { "Closed" }).to_string()];
    if let Some(link) = &issue.link {
        facts.push(format!("{} {}", link.connector, link.reference));
    }
    if !issue.labels.is_empty() {
        facts.push(issue.labels.join(", "));
    }
    let facts = div()
        .px_2()
        .py_1()
        .text_xs()
        .text_color(muted)
        .child(facts.join(" · "));
    let conflict = conflict_view(issue, cx);

    // What runs have said about it, newest last, under the body — the record of
    // what was tried, which the body itself never changes to say.
    let notes = (!issue.notes.is_empty()).then(|| {
        div()
            .flex_none()
            .max_h(gpui::rems(12.))
            .v_flex()
            .gap_1()
            .px_2()
            .py_1()
            .border_t_1()
            .border_color(cx.theme().border)
            .when(issue.notes.len() > NOTES_SHOWN, |notes| {
                notes.child(div().text_xs().text_color(muted).child(format!(
                    "… {} earlier notes not shown",
                    issue.notes.len() - NOTES_SHOWN
                )))
            })
            .children(
                issue
                    .notes
                    .iter()
                    .rev()
                    .take(NOTES_SHOWN)
                    .rev()
                    .map(|note| div().text_xs().text_color(muted).child(note.text.clone())),
            )
    });

    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .v_flex()
        .child(header)
        .child(facts)
        .children(conflict)
        .child(match body {
            Some(body) => div()
                .flex_1()
                .min_h_0()
                .p_3()
                .child(
                    TextView::new(&body)
                        .selectable(true)
                        .scrollable(true)
                        .style(body_style(window.rem_size(), cx)),
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
        .children(notes)
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
                    Some(number) => format!("Editing #{number}"),
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

/// Body styling. The renderer sizes its headings from an absolute pixel base,
/// which the panel's rem-base zoom cannot reach by itself, so the base in force
/// is written in by hand.
fn body_style(rem: gpui::Pixels, cx: &App) -> TextViewStyle {
    let mut style = TextViewStyle::default().code_block(
        gpui::StyleRefinement::default()
            .p(gpui::rems(0.75))
            .text_size(gpui::rems(0.8125))
            .bg(cx.theme().muted),
    );
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
