//! The form a new issue or an edit is written in, and how it is drawn.

use super::IssuesView;
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, IntoElement, ParentElement,
    SharedString, Styled, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::{Input, InputState, Textarea, TextareaState};
use gpui_component::{ActiveTheme, Sizable as _, StyledExt};
use onehand_core::issues::template::IssueTemplate;
use onehand_core::issues::{self, Draft};
use onehand_plugin_host::action;

/// The form a new issue or an edit is written in.
pub(super) struct Form {
    /// The issue being edited, or `None` for a new one.
    editing: Option<u64>,
    /// How the issue being edited is named on its forge; `None` for a draft
    /// or a new issue.
    editing_reference: Option<String>,
    title: Entity<InputState>,
    labels: Entity<InputState>,
    body: Entity<TextareaState>,
    /// What the three said when the form opened, so a form nobody changed is
    /// dropped without asking.
    opened: (String, String, String),
}

impl Form {
    /// Whether anything was typed since the form opened.
    pub(super) fn changed(&self, cx: &App) -> bool {
        let now = (
            self.title.read(cx).value().to_string(),
            self.labels.read(cx).value().to_string(),
            self.body.read(cx).value().to_string(),
        );
        now != self.opened
    }
}

impl IssuesView {
    /// Open the form, empty for a new issue or holding what issue `editing`
    /// says now, once a draft already open with changes in it has been asked
    /// about.
    pub(super) fn open_form(
        &mut self,
        editing: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.unless_drafting(window, cx, move |view, window, cx| {
            view.open_form_now(editing, window, cx)
        });
    }

    fn open_form_now(&mut self, editing: Option<u64>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let current = editing.and_then(|n| self.issues_of(&root, cx)?.get(n).cloned());
        let (title, labels, body) = match &current {
            Some(issue) => (
                issue.title.clone(),
                issue.labels.join(", "),
                issue.body.clone(),
            ),
            None => Default::default(),
        };
        let opened = (title.clone(), labels.clone(), body.clone());
        let form = Form {
            opened,
            editing,
            editing_reference: current
                .as_ref()
                .and_then(|issue| issue.reference().map(str::to_string)),
            title: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Title")
                    .default_value(title)
            }),
            labels: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Labels, separated by commas")
                    .default_value(labels)
            }),
            body: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder("What is it about? Markdown is fine.")
                    .default_value(body)
            }),
        };
        form.title.update(cx, |input, cx| input.focus(window, cx));
        if let Some(state) = self.state_mut() {
            state.form = Some(form);
        }
        self.status = None;
        cx.notify();
    }

    /// The templates the active project offers: its own, or the shipped
    /// ones until its own are read or when it keeps none.
    fn templates(&self, cx: &App) -> Vec<IssueTemplate> {
        self.root
            .as_ref()
            .and_then(|root| self.roots.get(root))
            .map_or_else(onehand_core::issues::template::shipped, |state| {
                state.file.read(cx).templates.clone()
            })
    }

    /// Fill the new issue's body from the project's template `at`, and add the
    /// labels it carries to those typed. Only a body still empty, or still
    /// another template's untouched, is filled: nothing typed is replaced.
    fn apply_template(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        let templates = self.templates(cx);
        let Some(form) = self.state_mut().and_then(|state| state.form.as_ref()) else {
            return;
        };
        let Some(template) = templates.get(at).cloned() else {
            return;
        };
        let body = form.body.read(cx).value().to_string();
        if !fillable(&body, &templates) {
            return;
        }
        let mut labels = issues::parse_labels(&form.labels.read(cx).value());
        // The template being replaced takes its own labels with it: a body
        // switched from Bug to Feature is no bug.
        if let Some(before) = templates.iter().find(|t| t.body == body) {
            labels
                .retain(|label| !before.labels.contains(label) || template.labels.contains(label));
        }
        for label in template.labels {
            if !labels.contains(&label) {
                labels.push(label);
            }
        }
        form.body
            .update(cx, |body, cx| body.set_value(template.body, window, cx));
        form.labels.update(cx, |input, cx| {
            input.set_value(labels.join(", "), window, cx)
        });
        cx.notify();
    }

    pub(super) fn cancel_form(&mut self, cx: &mut Context<Self>) {
        if let Some(state) = self.state_mut() {
            state.form = None;
            cx.notify();
        }
    }

    pub(super) fn save_form(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.state_mut().and_then(|state| state.form.as_ref()) else {
            return;
        };
        let editing = form.editing;
        let draft = Draft {
            title: form.title.read(cx).value().to_string(),
            body: form.body.read(cx).value().to_string(),
            labels: issues::parse_labels(&form.labels.read(cx).value()),
        };
        let now = issues::now();
        self.change(
            move |kept| {
                match editing {
                    Some(number) => kept.edit(number, draft, now).map(|()| number),
                    None => kept.create(draft, now),
                }
                .map(Some)
            },
            cx,
        );
    }
}

/// Whether a new issue's `body` may be filled from one of `templates`: it is
/// empty, or a template's untouched.
fn fillable(body: &str, templates: &[IssueTemplate]) -> bool {
    body.trim().is_empty() || templates.iter().any(|t| t.body == body)
}

/// The templates a new issue can be started from, while its body is still
/// fillable; a template fills in the body only, and adds no field.
fn templates_row(
    form: &Form,
    templates: &[IssueTemplate],
    cx: &mut Context<IssuesView>,
) -> Option<AnyElement> {
    if form.editing.is_some() || !fillable(&form.body.read(cx).value(), templates) {
        return None;
    }
    let muted = cx.theme().muted_foreground;
    Some(
        div()
            .h_flex()
            .gap_1()
            .items_center()
            .child(div().text_xs().text_color(muted).child("Template"))
            .children(templates.iter().enumerate().map(|(at, t)| {
                action(("issue-form-template", at))
                    .small()
                    .ghost()
                    .label(t.name.clone())
                    .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                        view.apply_template(at, window, cx)
                    }))
            }))
            .into_any_element(),
    )
}

/// The form: title, labels, body, then Save and Cancel under what they act on.
/// `project` names where the issue is kept, on the page, where the list spans
/// projects.
pub(super) fn form_view(
    form: &Form,
    project: Option<SharedString>,
    templates: &[IssueTemplate],
    cx: &mut Context<IssuesView>,
) -> AnyElement {
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
                .child({
                    let said = match form.editing {
                        Some(_) => match &form.editing_reference {
                            Some(reference) => format!("Editing {reference}"),
                            None => "Editing draft".to_string(),
                        },
                        None => "New issue".to_string(),
                    };
                    match project {
                        Some(project) => format!("{said} in {project}"),
                        None => said,
                    }
                }),
        )
        .child(Input::new(&form.title))
        .child(Input::new(&form.labels))
        .children(templates_row(form, templates, cx))
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
