//! The Plugins mode's own state — which project, what `claude` last said about
//! it, the change in flight — and how it is drawn.

use crate::cli::{self, Available, Catalog, Change, Installed, Scope};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, InteractiveElement as _,
    IntoElement, ParentElement, Render, StatefulInteractiveElement as _, Styled, Task, Window, div,
};
use gpui_component::button::{ButtonGroup, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::switch::Switch;
use gpui_component::{ActiveTheme, Disableable as _, Selectable as _, Sizable as _, StyledExt};
use onehand_plugin_host::{action, hint, status_line};
use std::path::{Path, PathBuf};

/// How many of the marketplaces' plugins are drawn at once. The official
/// marketplace alone offers a few hundred, and a list nobody scrolls to the end
/// of is not worth building to the end — the search is how the rest are
/// reached, and the list says how many it left out.
const MARKET_CAP: usize = 60;

pub(crate) struct PluginsView {
    root: Option<PathBuf>,
    /// What `claude` last said about the active root, or why it could not.
    /// Cleared on a project switch, since another project's installs are not
    /// this one's.
    catalog: Option<Result<Catalog, String>>,
    /// Whether the listing needs reading again before it is next drawn.
    stale: bool,
    /// The listing in flight, held only so that starting another drops it.
    _list: Option<Task<()>>,
    /// The one change in flight.
    ///
    /// **One at a time**, because every change is a rewrite of a settings file
    /// or of Claude Code's install record, and two landing together can each
    /// write a copy missing the other's.
    busy: Option<Change>,
    _change: Option<Task<()>>,
    /// Why the last change did not happen, kept until the next one does.
    ///
    /// A standing line rather than a notification: an install that was refused
    /// — most often for a command it wants run, which is left to a person on
    /// purpose — says what to do next, and a toast that fades takes that with it.
    status: Option<String>,
    /// Where an install from the marketplace list lands.
    install_scope: Scope,
    /// The marketplace search. Made on the first draw, since an input needs a
    /// window to be made in.
    query: Option<Entity<InputState>>,
}

impl PluginsView {
    pub(crate) fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|_| Self {
            root: None,
            catalog: None,
            stale: false,
            _list: None,
            busy: None,
            _change: None,
            status: None,
            install_scope: Scope::User,
            query: None,
        })
    }

    pub(crate) fn set_root(&mut self, root: &Path, cx: &mut Context<Self>) {
        if self.root.as_deref() == Some(root) {
            return;
        }
        self.root = Some(root.to_path_buf());
        self.catalog = None;
        self.stale = true;
        cx.notify();
    }

    pub(crate) fn forget_root(&mut self, root: &Path, cx: &mut Context<Self>) {
        if self.root.as_deref() == Some(root) {
            self.root = None;
            self.catalog = None;
            self._list = None;
            cx.notify();
        }
    }

    pub(crate) fn mark_stale(&mut self, cx: &mut Context<Self>) {
        self.stale = true;
        cx.notify();
    }

    /// Ask `claude` what is installed for the active root.
    ///
    /// The answer is dropped if the root has moved on while it was out, since
    /// it names another project's project and local installs.
    fn list(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        self._list = Some(cx.spawn(async move |view, cx| {
            let listed = cx
                .background_executor()
                .spawn({
                    let root = root.clone();
                    async move { cli::list_blocking(&root) }
                })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                if view.root.as_deref() == Some(root.as_path()) {
                    view.catalog = Some(listed);
                    cx.notify();
                }
            });
        }));
    }

    /// Make one change, then read the listing again — the command line's
    /// answer to a change is a sentence, and what it did is only certain from
    /// what it lists afterwards.
    fn change(&mut self, change: Change, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        let Some(root) = self.root.clone() else {
            return;
        };
        self.busy = Some(change.clone());
        self.status = None;
        cx.notify();
        self._change = Some(cx.spawn(async move |view, cx| {
            let done = cx
                .background_executor()
                .spawn({
                    let root = root.clone();
                    let change = change.clone();
                    async move { cli::change_blocking(&root, &change) }
                })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                view.busy = None;
                if let Err(why) = done {
                    view.status = Some(why);
                }
                view.stale = true;
                cx.notify();
            });
        }));
    }

    fn query(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(query) = &self.query {
            return query.clone();
        }
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search the marketplaces"));
        cx.subscribe(&query, |_, _, _: &InputEvent, cx| cx.notify())
            .detach();
        self.query = Some(query.clone());
        query
    }
}

impl Render for PluginsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Being drawn is what says the listing is worth reading, as it is for
        // the Markdown mode's walk.
        if self.stale && self.busy.is_none() {
            self.stale = false;
            self.list(cx);
        }
        let body = self.body(window, cx);
        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .child(body)
            .children(self.status.clone().map(|status| status_line(status, cx)))
    }
}

impl PluginsView {
    fn body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.root.is_none() {
            return hint("No project root", cx);
        }
        let catalog = match &self.catalog {
            None => return hint("Reading Claude Code's plugins…", cx),
            Some(Err(why)) => return status_line(why.clone(), cx),
            Some(Ok(catalog)) => catalog.clone(),
        };
        let query = self.query(window, cx);
        let needle = query.read(cx).value().trim().to_lowercase();

        let muted = cx.theme().muted_foreground;
        let installed: Vec<AnyElement> = catalog
            .installed
            .iter()
            .enumerate()
            .map(|(i, plugin)| self.installed_row(i, plugin, cx))
            .collect();
        let matching: Vec<&Available> = catalog
            .available
            .iter()
            .filter(|plugin| {
                needle.is_empty()
                    || plugin.name.to_lowercase().contains(&needle)
                    || plugin.description.to_lowercase().contains(&needle)
                    || plugin.marketplace.to_lowercase().contains(&needle)
            })
            .collect();
        let cut = matching.len().saturating_sub(MARKET_CAP);
        let offered: Vec<AnyElement> = matching
            .iter()
            .take(MARKET_CAP)
            .enumerate()
            .map(|(i, plugin)| self.available_row(i, plugin, &catalog.installed, cx))
            .collect();

        div()
            .id("plugins-body")
            .flex_1()
            .min_h_0()
            .v_flex()
            .overflow_y_scroll()
            .child(self.top_bar(cx))
            .child(heading("Installed", cx))
            .when(installed.is_empty(), |list| {
                list.child(note("Nothing installed that reaches this project", cx))
            })
            .children(installed)
            .child(heading("Marketplace", cx))
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .px_2()
                    .pb_1()
                    .child(div().flex_1().min_w_0().child(Input::new(&query).xsmall()))
                    .child(self.scope_picker(cx)),
            )
            .when(offered.is_empty(), |list| {
                list.child(note("No plugin matches the search", cx))
            })
            .children(offered)
            .when(cut > 0, |list| {
                list.child(
                    div()
                        .px_2()
                        .py_1()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("… {cut} more not shown — narrow the search")),
                )
            })
            .into_any_element()
    }

    /// What a change here reaches, and the busy line or the catalog refresh.
    fn top_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let line = match &self.busy {
            Some(change) => change.doing(),
            // A session reads its plugins when its agent starts, so a change
            // here is not seen by one already running — said, because a switch
            // that visibly did nothing reads as broken.
            None => "Changes reach a session when its agent next starts".to_string(),
        };
        div()
            .h_flex()
            .items_center()
            .gap_1()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(line),
            )
            .when(self.busy.is_none(), |bar| {
                bar.child(
                    action("plugins-refresh")
                        .xsmall()
                        .ghost()
                        .label("Update marketplaces")
                        .tooltip("Fetch every marketplace's catalog again")
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                            view.change(Change::Refresh, cx)
                        })),
                )
            })
            .into_any_element()
    }

    /// Where an install lands, as one segmented control above the list rather
    /// than a menu on every row: the choice is usually made once for a run of
    /// installs, and three hundred menus are three hundred places to make it.
    fn scope_picker(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let current = self.install_scope;
        ButtonGroup::new("plugins-scope")
            .outline()
            .xsmall()
            .children(Scope::ALL.into_iter().enumerate().map(|(i, scope)| {
                action(("plugins-scope", i))
                    .label(scope.label())
                    .tooltip(scope.meaning())
                    .selected(scope == current)
            }))
            .on_click(cx.listener(|view, clicked: &Vec<usize>, _, cx| {
                if let Some(scope) = clicked.first().and_then(|i| Scope::ALL.get(*i)) {
                    view.install_scope = *scope;
                    cx.notify();
                }
            }))
    }

    fn installed_row(&self, i: usize, plugin: &Installed, cx: &mut Context<Self>) -> AnyElement {
        let busy = self.busy.as_ref().and_then(Change::plugin) == Some(plugin.id.as_str());
        let idle = self.busy.is_none();
        let (id, scope, enabled) = (plugin.id.clone(), plugin.scope, plugin.enabled);
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .w_full()
            .px_2()
            .py_1()
            .text_sm()
            .child(
                // The switch sets no cursor of its own, and an arrow over a
                // control that acts reads as one that does not.
                div().flex_none().when(idle, |d| d.cursor_pointer()).child(
                    Switch::new(("plugin-enabled", i))
                        .checked(enabled)
                        .disabled(!idle)
                        .on_click(cx.listener(move |view, on: &bool, _, cx| {
                            let change = if *on {
                                Change::Enable(id.clone(), scope)
                            } else {
                                Change::Disable(id.clone(), scope)
                            };
                            view.change(change, cx)
                        })),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .when(!enabled, |name| {
                        name.text_color(cx.theme().muted_foreground)
                    })
                    .child(plugin.id.clone()),
            )
            .children(plugin.version.clone().map(|version| {
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(version)
            }))
            .child(scope_tag(plugin.scope, cx))
            .child(if busy {
                note("…", cx)
            } else {
                let id = plugin.id.clone();
                action(("plugin-remove", i))
                    .xsmall()
                    .ghost()
                    .label("Remove")
                    .disabled(!idle)
                    .tooltip("Uninstall at this scope; the plugin's saved data is kept")
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.change(Change::Uninstall(id.clone(), scope), cx)
                    }))
                    .into_any_element()
            })
            .into_any_element()
    }

    fn available_row(
        &self,
        i: usize,
        plugin: &Available,
        installed: &[Installed],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let scope = self.install_scope;
        let muted = cx.theme().muted_foreground;
        let here = installed
            .iter()
            .any(|p| p.id == plugin.id && p.scope == scope);
        let busy = self.busy.as_ref().and_then(Change::plugin) == Some(plugin.id.as_str());
        let meta = match plugin.install_count {
            Some(count) => format!("{} · {count} installs", plugin.marketplace),
            None => plugin.marketplace.clone(),
        };
        div()
            .v_flex()
            .w_full()
            .px_2()
            .py_1()
            .gap_0p5()
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .child(plugin.name.clone()),
                    )
                    .child(div().flex_none().text_xs().text_color(muted).child(meta))
                    .child(if here {
                        note("Installed", cx)
                    } else if busy {
                        note("…", cx)
                    } else {
                        let id = plugin.id.clone();
                        action(("plugin-install", i))
                            .xsmall()
                            .outline()
                            .label("Install")
                            .disabled(self.busy.is_some())
                            .tooltip(format!("Install for: {}", scope.meaning()))
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                view.change(Change::Install(id.clone(), scope), cx)
                            }))
                            .into_any_element()
                    }),
            )
            .when(!plugin.description.is_empty(), |row| {
                row.child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .line_clamp(2)
                        .child(plugin.description.clone()),
                )
            })
            .into_any_element()
    }
}

fn heading(text: &'static str, cx: &App) -> AnyElement {
    div()
        .px_2()
        .pt_3()
        .pb_1()
        .text_xs()
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .child(text)
        .into_any_element()
}

fn note(text: &'static str, cx: &App) -> AnyElement {
    div()
        .flex_none()
        .px_2()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text)
        .into_any_element()
}

/// Which scope an install is at, as a small bordered word: the one fact that
/// tells apart two rows of the same plugin.
fn scope_tag(scope: Scope, cx: &App) -> AnyElement {
    div()
        .flex_none()
        .px_1()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(scope.label())
        .into_any_element()
}
