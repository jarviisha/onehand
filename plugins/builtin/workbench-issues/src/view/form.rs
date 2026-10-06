//! The form a new issue or an edit is written in, and how it is drawn.

use super::IssuesView;
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, IntoElement, ParentElement,
    Styled, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::{Input, InputState, Textarea, TextareaState};
use gpui_component::{ActiveTheme, Sizable as _, StyledExt};
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
