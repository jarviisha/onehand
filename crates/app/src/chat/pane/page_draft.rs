//! What is being written on a project's page, set aside per project as a
//! session's draft is: the words, the agent picked, where the session is to
//! run, and the branch a new worktree is to be cut on. Drawn here too: the
//! heading, the card and the strip under it.

use super::{ChatPane, ChatPaneEvent, StartIn};
use crate::chat::composer::Draft;
use crate::controls::text_input;
use gpui::Focusable as _;
use gpui::InteractiveElement as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Context, IntoElement, ParentElement, SharedString, Styled, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, StyledExt};
use onehand_ui::ShortIcon as _;
use std::path::{Path, PathBuf};

/// A page's project files and folders for `@`, scanned off the UI loop.
#[derive(Clone, Default)]
pub(super) struct PageFiles {
    files: std::rc::Rc<Vec<String>>,
    folders: std::rc::Rc<Vec<(String, usize)>>,
    scanned: bool,
    /// The latest scan asked for: an older one landing after it is dropped.
    asked: u64,
}

/// How many files a page offers `@`, as a session does.
const PAGE_MENTION_FILES: usize = crate::chat::session::MAX_MENTION_FILES;

/// How wide the new worktree's branch field stands beside its start chip.
const BRANCH_FIELD_W: gpui::Rems = gpui::rems(16.);

/// A picker of the page's composer opening its menu below: its words cut,
/// rather than run past the card or the pane, when its row runs short, as it
/// does in a narrow pane at a large zoom.
pub(super) fn page_menu(
    id: impl Into<gpui::ElementId>,
    trigger: gpui_component::button::Button,
    build: impl Fn(
        gpui_component::menu::PopupMenu,
        &mut Window,
        &mut Context<gpui_component::menu::PopupMenu>,
    ) -> gpui_component::menu::PopupMenu
    + 'static,
) -> gpui_component::popover::Popover {
    let id = id.into();
    let marked = id.to_string();
    let trigger = trigger
        .debug_selector(move || marked)
        .flex_shrink_1()
        .min_w_0();
    onehand_ui::menu_below(id, trigger, build)
        .trigger_style(gpui::StyleRefinement::default().min_w_0())
}

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
    /// The past conversation the session is to continue.
    continuing: Option<super::Continuing>,
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

    /// Scan `root`'s files for the page's `@` each time its page shows, off
    /// the UI loop, what was read before offered meanwhile; a folder on a
    /// host offers none, as a host session does not.
    fn scan_page_files(&mut self, root: &Path, cx: &mut Context<Self>) {
        let here = self
            .empty
            .as_ref()
            .is_some_and(|project| project.host.is_none());
        if !here {
            let none = PageFiles {
                scanned: true,
                ..PageFiles::default()
            };
            self.page_files.insert(root.to_path_buf(), none);
            return;
        }
        let kept = self.page_files.entry(root.to_path_buf()).or_default();
        kept.asked += 1;
        let asked = kept.asked;
        let at = root.to_path_buf();
        cx.spawn(async move |pane, cx| {
            let scan_root = at.clone();
            let files = cx
                .background_executor()
                .spawn(async move {
                    onehand_core::completion::scan_files(&scan_root, PAGE_MENTION_FILES)
                })
                .await;
            let _ = pane.update(cx, |pane: &mut Self, cx| {
                let Some(kept) = pane
                    .page_files
                    .get_mut(&at)
                    .filter(|kept| kept.asked == asked)
                else {
                    return;
                };
                kept.folders = std::rc::Rc::new(onehand_core::completion::folders(&files));
                kept.files = std::rc::Rc::new(files);
                kept.scanned = true;
                cx.notify();
            });
        })
        .detach();
    }

    /// What `root`'s page completes against for `agent`.
    fn page_lists(&self, root: &Path, agent: &str, cx: &App) -> crate::chat::composer::Lists {
        let files = self.page_files.get(root).cloned().unwrap_or_default();
        let commands = crate::state::Shared::global(cx)
            .settings
            .facts_of(agent)
            .map(|facts| facts.commands.clone())
            .unwrap_or_default();
        crate::chat::composer::Lists::Page(std::rc::Rc::new(crate::chat::composer::PageLists {
            files: files.files,
            folders: files.folders,
            scanned: files.scanned,
            commands,
        }))
    }

    /// What the page on screen completes against, if a page is on screen.
    pub(super) fn page_lists_on_screen(&self, cx: &App) -> Option<crate::chat::composer::Lists> {
        let root = self.page_root()?;
        let agent = self.page_agent(&root, cx).unwrap_or_default();
        Some(self.page_lists(&root, &agent, cx))
    }

    /// Give the page now on screen what it had written.
    pub(super) fn restore_page_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.page_root() else {
            return;
        };
        self.scan_page_files(&root, cx);
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
        let staged = self.composer.read(cx).attachments.clone();
        let cards = self.page_cards(&root, cx);
        let continuing = self
            .page_drafts
            .get(&root)
            .and_then(|kept| kept.continuing.clone());
        let branch = self.page_branch.read(cx).value().trim().to_string();
        let agent = self.page_agent(&root, cx);
        let kept = self.page_drafts.entry(root.clone()).or_default();
        kept.failure = None;
        let start = kept.start.clone();
        // What the chips show is what the session gets.
        let picks = self.page_shown_picks(&root, agent.as_deref().unwrap_or_default(), cx);
        let branch = (start == StartIn::NewWorktree).then_some(branch);
        cx.emit(ChatPaneEvent::StartFromPage(super::PageSend {
            root,
            text,
            agent,
            start,
            branch,
            picks,
            staged,
            cards,
            continuing,
        }));
    }

    /// Whether what `root`'s page holds is locked: a worktree is being cut
    /// for it, and the words sent are the words the session gets.
    pub(crate) fn page_locked(&self, root: &Path) -> bool {
        self.page_drafts
            .get(root)
            .is_some_and(|kept| kept.busy.is_some())
    }

    /// Carry the past conversation `meta` into the session the page on screen
    /// sends: on its composer, its agent picked, the caret in its field.
    pub(super) fn continue_on_page(
        &mut self,
        meta: &onehand_core::chat::ConvMeta,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.page_root() else {
            return;
        };
        let kept = self.page_drafts.entry(root).or_default();
        if kept.agent.as_deref() != Some(meta.agent.as_str()) {
            kept.picks.clear();
        }
        kept.agent = Some(meta.agent.clone().into());
        kept.continuing = Some(super::Continuing {
            dir: meta.dir.clone(),
            title: meta.title.clone(),
            summarise: false,
        });
        self.composer
            .read(cx)
            .state
            .focus_handle(cx)
            .focus(window, cx);
        cx.notify();
    }

    /// The chip over the page's card for the conversation it continues:
    /// its title, *Summarise first* and the way to drop it.
    fn continuing_chip(&self, root: &Path, cx: &mut Context<Self>) -> Option<AnyElement> {
        let carried = self.page_drafts.get(root)?.continuing.clone()?;
        let (toggle_at, drop_at) = (root.to_path_buf(), root.to_path_buf());
        Some(
            div()
                .h_flex()
                .items_center()
                .gap_3()
                .w_full()
                .text_xs()
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("Continues “{}”", carried.title)),
                )
                .child(
                    onehand_ui::toggle("page-summarise", carried.summarise)
                        .label("Summarise first")
                        .on_click(cx.listener(move |pane: &mut Self, on: &bool, _, cx| {
                            if let Some(carried) = pane
                                .page_drafts
                                .get_mut(&toggle_at)
                                .and_then(|kept| kept.continuing.as_mut())
                            {
                                carried.summarise = *on;
                            }
                            cx.notify();
                        })),
                )
                .child(
                    crate::controls::action("page-continuing-drop")
                        .short_icon()
                        .ghost()
                        .icon(gpui_component::Icon::new(gpui_component::IconName::Close))
                        .tooltip("Start fresh instead")
                        .on_click(cx.listener(move |pane: &mut Self, _, _, cx| {
                            if let Some(kept) = pane.page_drafts.get_mut(&drop_at) {
                                kept.continuing = None;
                            }
                            cx.notify();
                        })),
                )
                .into_any_element(),
        )
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
        let composer = self.composer.read(cx);
        if let Some(block) =
            onehand_core::chat::draft_blocker(&composer.text(cx), &composer.attachments)
        {
            return Some(block.hint().into());
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
        window: &Window,
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
        let lists = self.page_lists(root, &name, cx);
        // The `@` and `/` list floats over the card, out of the page's scroll,
        // with the room above the card to grow into.
        let rem = window.rem_size() * self.zoom.factor();
        let above = crate::tokens::CONTROL_H.to_pixels(rem) * 2.;
        let popup = self.composer.update(cx, |composer, cx| {
            composer.page_popup(&lists, above, rem, cx)
        });
        let card = self.composer.update(cx, |composer, cx| {
            composer.page_card(agent, refuses, locked, lists, cx)
        });
        let card = div()
            .w_full()
            // A press anywhere but the card or its list closes the list, as
            // on a session's.
            .on_mouse_down_out(cx.listener(
                |pane: &mut Self, event: &gpui::MouseDownEvent, _, cx| {
                    let composer = pane.composer.read(cx);
                    let on_list = composer
                        .menu_bounds
                        .get()
                        .is_some_and(|bounds| bounds.contains(&event.position));
                    if composer.overlay_open() && !on_list {
                        pane.composer
                            .update(cx, |composer, cx| composer.close_overlay(cx));
                    }
                },
            ))
            .child(card)
            .children(popup);
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
                        .w(BRANCH_FIELD_W)
                        .min_w_0()
                        .child(text_input(&self.page_branch, cx).disabled(locked)),
                )
            });
        let (cards, cost) = self.page_cards_chip(root, cx).unzip();
        let strip = strip.child(div().flex_1()).children(cards);
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
            .children(self.continuing_chip(root, cx))
            // What is staged rests over the card, as on a session's.
            .children(
                self.composer
                    .update(cx, |composer, cx| composer.page_tray(locked, cx)),
            )
            .child(card)
            .child(strip)
            .children(
                cost.flatten()
                    .map(|cost| div().text_xs().text_color(ink.warning).child(cost)),
            )
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
        page_menu(
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
        page_menu(
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
        self.composer
            .update(cx, |composer, cx| composer.typed(text, window, cx));
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

#[cfg(test)]
impl ChatPane {
    /// Stage `path` on what the composer holds.
    pub(crate) fn stage_on_page(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.composer
            .update(cx, |composer, cx| composer.stage_picked(path, cx));
    }
}

#[cfg(test)]
impl ChatPane {
    /// The labels the page's open list offers.
    pub(crate) fn page_matches(&self, cx: &App) -> Vec<String> {
        let Some(lists) = self.page_lists_on_screen(cx) else {
            return Vec::new();
        };
        self.composer.read(cx).match_labels(&lists, cx)
    }
}

#[cfg(test)]
impl ChatPane {
    /// Press Enter in the composer.
    pub(crate) fn press_enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.enter(window, cx);
    }
}

#[cfg(test)]
impl ChatPane {
    /// Carry `meta` onto the page, as its row's *Continue* does.
    pub(crate) fn press_continue(
        &mut self,
        meta: &onehand_core::chat::ConvMeta,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.continue_on_page(meta, window, cx);
    }
}
