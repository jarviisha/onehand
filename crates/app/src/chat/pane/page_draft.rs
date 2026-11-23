//! What is being written on a project's page, set aside per project as a
//! session's draft is: the words, the agent picked, where the session is to
//! run, and the branch a new worktree is to be cut on. Drawn here too: the
//! heading, the card and the strip under it.

use super::{ChatPane, ChatPaneEvent, StartIn};
use crate::chat::composer::Draft;
use crate::controls::text_input;
use gpui::Focusable as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Context, IntoElement, ParentElement, SharedString, Styled, Window, div,
};
use gpui_component::{ActiveTheme, StyledExt};
use std::path::{Path, PathBuf};

/// A project page's draft, kept while the person is elsewhere.
#[derive(Default)]
pub(super) struct PageDraft {
    text: Draft,
    agent: Option<SharedString>,
    /// What is picked of the agent's offer, by group (`mode` for the mode).
    picks: Vec<(String, String)>,
    start: StartIn,
    branch: String,
    /// What is under way for it: a worktree being made.
    busy: Option<String>,
    /// Why the last send did not start a session.
    failure: Option<String>,
}

impl ChatPane {
    /// The project whose page is on screen, by its folder.
    pub(crate) fn page_root(&self) -> Option<PathBuf> {
        (self.active.is_none() && self.page.is_none())
            .then(|| self.empty.as_ref().map(|project| project.path.clone()))
            .flatten()
    }

    /// Set aside what the composer holds, `draft`, for the page on screen.
    pub(super) fn stash_page_draft(&mut self, draft: Draft, cx: &App) {
        let Some(root) = self.page_root() else {
            return;
        };
        let branch = self.page_branch.read(cx).value().to_string();
        let kept = self.page_drafts.entry(root).or_default();
        kept.text = draft;
        kept.branch = branch;
    }

    /// Give the page now on screen what it had written.
    pub(super) fn restore_page_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.page_root() else {
            return;
        };
        let (text, branch) = match self.page_drafts.get_mut(&root) {
            Some(kept) => (std::mem::take(&mut kept.text), kept.branch.clone()),
            None => (Draft::default(), String::new()),
        };
        self.composer
            .update(cx, |composer, cx| composer.restore_draft(text, window, cx));
        self.page_branch
            .update(cx, |input, cx| input.set_value(branch, window, cx));
    }

    /// Pick where the page on screen starts its session, and which agent
    /// runs it when one is named, then put the caret in its field.
    pub(crate) fn set_page_start(
        &mut self,
        start: StartIn,
        agent: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.page_root() else {
            return;
        };
        let kept = self.page_drafts.entry(root).or_default();
        let cut = start == StartIn::NewWorktree;
        kept.start = start;
        if agent.is_some() {
            kept.agent = agent;
        }
        // The branch is what a new worktree waits on; else the words.
        match cut {
            true => self.page_branch.focus_handle(cx).focus(window, cx),
            false => self
                .composer
                .read(cx)
                .state
                .focus_handle(cx)
                .focus(window, cx),
        }
        cx.notify();
    }

    /// Say what the send from `root`'s page is doing, or that it is done.
    pub(crate) fn page_busy(&mut self, root: &Path, busy: Option<String>, cx: &mut Context<Self>) {
        let kept = self.page_drafts.entry(root.to_path_buf()).or_default();
        kept.busy = busy;
        cx.notify();
    }

    /// Say why the send from `root`'s page started nothing; its words stay.
    pub(crate) fn page_failed(&mut self, root: &Path, why: String, cx: &mut Context<Self>) {
        let kept = self.page_drafts.entry(root.to_path_buf()).or_default();
        kept.busy = None;
        kept.failure = Some(why);
        cx.notify();
    }

    /// The send from `root`'s page started its session: what was written
    /// there is gone with it.
    pub(crate) fn page_sent(&mut self, root: &Path, window: &mut Window, cx: &mut Context<Self>) {
        self.page_drafts.remove(root);
        if self.page_root().as_deref() == Some(root) {
            self.composer
                .update(cx, |composer, cx| composer.clear(window, cx));
            self.page_branch
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        cx.notify();
    }

    /// Send what the page holds: ask the shell for its session.
    pub(super) fn submit_page(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.page_root() else {
            return;
        };
        if self.page_refuses(cx).is_some() {
            return;
        }
        let text = self.composer.read(cx).text(cx);
        let branch = self.page_branch.read(cx).value().trim().to_string();
        let agent = self.page_agent(&root, cx);
        let kept = self.page_drafts.entry(root.clone()).or_default();
        kept.failure = None;
        let start = kept.start.clone();
        let picks = kept.picks.clone();
        let branch = (start == StartIn::NewWorktree).then_some(branch);
        cx.emit(ChatPaneEvent::StartFromPage(super::PageSend {
            root,
            text,
            agent,
            start,
            branch,
            picks,
        }));
    }

    /// Whether what `root`'s page holds is locked: a worktree is being cut
    /// for it, and the words sent are the words the session gets.
    pub(crate) fn page_locked(&self, root: &Path) -> bool {
        self.page_drafts
            .get(root)
            .is_some_and(|kept| kept.busy.is_some())
    }

    /// What is picked on `root`'s page of its agent's offer.
    pub(super) fn page_picks(&self, root: &Path) -> Vec<(String, String)> {
        self.page_drafts
            .get(root)
            .map(|kept| kept.picks.clone())
            .unwrap_or_default()
    }

    /// Pick `value` of group `group` on `root`'s page.
    pub(crate) fn pick_on_page(
        &mut self,
        root: &Path,
        group: &str,
        value: String,
        cx: &mut Context<Self>,
    ) {
        let kept = self.page_drafts.entry(root.to_path_buf()).or_default();
        kept.picks.retain(|(g, _)| g != group);
        kept.picks.push((group.to_string(), value));
        cx.notify();
    }

    /// Why Send does nothing yet on the page on screen, if it does not.
    fn page_refuses(&self, cx: &App) -> Option<SharedString> {
        let root = self.page_root()?;
        let kept = self.page_drafts.get(&root);
        if let Some(busy) = kept.and_then(|kept| kept.busy.clone()) {
            return Some(busy.into());
        }
        if self.composer.read(cx).text(cx).trim().is_empty() {
            return Some("Write a prompt first".into());
        }
        if kept.is_some_and(|kept| kept.start == StartIn::NewWorktree) {
            let branch = self.page_branch.read(cx).value();
            if let Err(why) = onehand_core::worktree::validate_branch(branch.trim()) {
                return Some(why.into());
            }
        }
        None
    }

    /// The agent the page on screen starts its session with: the one picked
    /// while it is configured, else the first.
    fn page_agent(&self, root: &Path, cx: &App) -> Option<SharedString> {
        let agents = &crate::state::Shared::global(cx).settings.agents;
        let picked = self
            .page_drafts
            .get(root)
            .and_then(|kept| kept.agent.clone())
            .filter(|name| agents.iter().any(|spec| spec.name == name.as_ref()));
        picked.or_else(|| agents.first().map(|spec| spec.name.clone().into()))
    }

    /// The heading, the card and the strip under it, for the page of the
    /// project at `root`. `worktrees` are its folders on disk by branch;
    /// `splits` whether a new worktree can be cut from it.
    pub(super) fn page_composer(
        &self,
        root: &Path,
        worktrees: &[(PathBuf, String)],
        splits: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let refuses = self.page_refuses(cx);
        let locked = self.page_locked(root);
        let name = self.page_agent(root, cx).unwrap_or_default();
        let agent = div()
            .h_flex()
            .items_center()
            .gap_2()
            .min_w_0()
            .child(self.agent_chip(root, cx))
            .children(self.page_options(root, &name, cx))
            .into_any_element();
        let card = self.composer.update(cx, |composer, cx| {
            composer.page_card(agent, refuses, locked, cx)
        });
        let kept = self.page_drafts.get(root);
        let start = kept.map(|kept| kept.start.clone()).unwrap_or_default();
        let busy = kept.and_then(|kept| kept.busy.clone());
        let failure = kept.and_then(|kept| kept.failure.clone());
        let ink = crate::theme::status_ink(cx);
        let muted = cx.theme().muted_foreground;
        let branch = self.page_branch.read(cx).value().trim().to_string();
        let invalid = (start == StartIn::NewWorktree && !branch.is_empty())
            .then(|| onehand_core::worktree::validate_branch(&branch).err())
            .flatten();
        let strip = div()
            .h_flex()
            .items_center()
            .gap_2()
            .w_full()
            .child(self.start_chip(&start, worktrees, splits, cx))
            .when(start == StartIn::NewWorktree, |strip| {
                strip.child(
                    div()
                        .w(gpui::rems(16.))
                        .child(text_input(&self.page_branch, cx).disabled(locked)),
                )
            });
        div()
            .v_flex()
            .gap_2()
            .w_full()
            .child(
                div()
                    .w_full()
                    .text_center()
                    .text_xl()
                    .font_semibold()
                    .child(super::greeting::GREETING),
            )
            .child(card)
            .child(strip)
            .children(invalid.map(|why| div().text_xs().text_color(ink.danger).child(why)))
            .children(busy.map(|busy| div().text_xs().text_color(muted).child(busy)))
            .children(failure.map(|why| div().text_xs().text_color(ink.danger).child(why)))
            .into_any_element()
    }

    /// The chip picking the agent the session runs.
    fn agent_chip(&self, root: &Path, cx: &mut Context<Self>) -> AnyElement {
        let names: Vec<SharedString> = crate::state::Shared::global(cx)
            .settings
            .agents
            .iter()
            .map(|spec| spec.name.clone().into())
            .collect();
        let picked = self.page_agent(root, cx).unwrap_or_default();
        let pane = cx.entity().downgrade();
        let at = root.to_path_buf();
        onehand_ui::menu_below(
            "page-agent",
            onehand_ui::picker(
                "page-agent-trigger",
                picked.clone(),
                false,
                onehand_ui::ControlSize::Short,
                cx,
            ),
            move |mut menu, _, _| {
                menu = menu.item(crate::controls::menu_label("Agent"));
                for name in &names {
                    let (pane, at, name) = (pane.clone(), at.clone(), name.clone());
                    menu = menu.item(
                        crate::controls::menu_item(name.clone())
                            .checked(name == picked)
                            .on_click(move |_, _, cx: &mut App| {
                                let _ = pane.update(cx, |pane: &mut Self, cx| {
                                    let kept = pane.page_drafts.entry(at.clone()).or_default();
                                    // Another agent offers its own choices.
                                    if kept.agent.as_ref() != Some(&name) {
                                        kept.picks.clear();
                                    }
                                    kept.agent = Some(name.clone());
                                    cx.notify();
                                });
                            }),
                    );
                }
                menu
            },
        )
        .into_any_element()
    }

    /// The chip picking where the session runs: the project's folder, one of
    /// its worktrees, or a new one.
    fn start_chip(
        &self,
        start: &StartIn,
        worktrees: &[(PathBuf, String)],
        splits: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = match start {
            StartIn::Project => "Project folder".to_string(),
            StartIn::Folder(folder) => worktrees.iter().find(|(at, _)| at == folder).map_or_else(
                || folder.display().to_string(),
                |(_, branch)| branch.clone(),
            ),
            StartIn::NewWorktree => "New worktree".to_string(),
        };
        let pane = cx.entity().downgrade();
        let picks: Vec<(StartIn, String)> =
            std::iter::once((StartIn::Project, "Project folder".into()))
                .chain(
                    worktrees
                        .iter()
                        .take(crate::projects::WORKTREES_SHOWN)
                        .map(|(folder, branch)| (StartIn::Folder(folder.clone()), branch.clone())),
                )
                .collect();
        let more = crate::projects::more_not_shown(worktrees.len());
        let current = start.clone();
        onehand_ui::menu_below(
            "page-start-in",
            onehand_ui::picker(
                "page-start-in-trigger",
                label,
                false,
                onehand_ui::ControlSize::Short,
                cx,
            ),
            move |mut menu, _, _| {
                menu = menu.item(crate::controls::menu_label("Start in"));
                let pick = |start: StartIn| {
                    let pane = pane.clone();
                    move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                        let _ = pane.update(cx, |pane: &mut Self, cx| {
                            pane.set_page_start(start.clone(), None, window, cx);
                        });
                    }
                };
                for (start, label) in &picks {
                    menu = menu.item(
                        crate::controls::menu_item(label.clone())
                            .checked(*start == current)
                            .on_click(pick(start.clone())),
                    );
                }
                if let Some(more) = more.clone() {
                    menu = menu.item(crate::controls::menu_label(more));
                }
                if splits {
                    menu = menu.separator().item(
                        crate::controls::menu_item("New worktree…")
                            .checked(current == StartIn::NewWorktree)
                            .on_click(pick(StartIn::NewWorktree)),
                    );
                }
                menu
            },
        )
        .into_any_element()
    }
}

#[cfg(test)]
impl ChatPane {
    /// Write `text` into the composer, as typing would.
    pub(crate) fn type_in_composer(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = self.composer.read(cx).state.clone();
        state.update(cx, |state, cx| {
            state.set_value(text.to_string(), window, cx)
        });
    }

    /// What the composer holds.
    pub(crate) fn composer_text(&self, cx: &App) -> String {
        self.composer.read(cx).text(cx)
    }

    /// Press Send.
    pub(crate) fn press_send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.submit(window, cx);
    }
}

#[cfg(test)]
impl ChatPane {
    /// Write `branch` into the page's branch field.
    pub(crate) fn type_branch(
        &mut self,
        branch: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.page_branch.update(cx, |input, cx| {
            input.set_value(branch.to_string(), window, cx)
        });
    }

    /// Why the last send from `root`'s page started nothing.
    pub(crate) fn page_failure(&self, root: &Path) -> Option<String> {
        self.page_drafts.get(root)?.failure.clone()
    }
}

#[cfg(test)]
impl ChatPane {
    /// Where `root`'s page starts its session.
    pub(crate) fn page_start_of(&self, root: &Path) -> StartIn {
        self.page_drafts
            .get(root)
            .map(|kept| kept.start.clone())
            .unwrap_or_default()
    }
}
