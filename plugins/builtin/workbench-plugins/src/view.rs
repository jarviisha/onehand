//! The Plugins mode's own state — which project, what `claude` last said about
//! it, the change in flight — and how it is drawn.

use crate::cli::{self, Available, Catalog, Change, Plugin, Scope, Verb};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, InteractiveElement as _,
    IntoElement, ParentElement, Render, StatefulInteractiveElement as _, Styled, Task, Window, div,
};
use gpui_component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme, Disableable as _, Selectable as _, Sizable as _, StyledExt};
use onehand_plugin_host::{action, hint, status_line};
use std::path::{Path, PathBuf};

/// How many installed plugins are drawn. Far past what anybody installs, so it
/// bounds the work of building rows rather than editing the list; the one time
/// it bites, it says so.
const INSTALLED_CAP: usize = 200;

/// How many of the marketplaces' plugins are drawn at once. The official
/// marketplace alone offers a few hundred, and a list nobody scrolls to the end
/// of is not worth building to the end — the search is how the rest are
/// reached, and the list says how many it left out.
const MARKET_CAP: usize = 60;

pub(crate) struct PluginsView {
    root: Option<PathBuf>,
    /// What `claude` last said about the active root, or why it could not.
    /// Cleared on a project switch, since another project's installs and
    /// settings are not this one's.
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
    /// Why the last change did not happen, kept until the next one does or the
    /// project changes.
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
        // A refusal is about the project it happened in.
        self.status = None;
        self.stale = true;
        cx.notify();
    }

    pub(crate) fn forget_root(&mut self, root: &Path, cx: &mut Context<Self>) {
        if self.root.as_deref() == Some(root) {
            self.root = None;
            self.catalog = None;
            self.status = None;
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
    /// it names another project's installs and settings.
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
    ///
    /// The change was made in the project it was pressed in, so a refusal is
    /// said only while that project is still the one on screen.
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
                if view.root.as_deref() == Some(root.as_path())
                    && let Err(why) = done
                {
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

    /// Whether `change` is the one in flight, so the control that asked for
    /// it — and only that one — shows it working.
    fn running(&self, change: &Change) -> bool {
        self.busy.as_ref() == Some(change)
    }
}

impl Render for PluginsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Being drawn is what says the listing is worth reading, as it is for
        // the Markdown mode's walk. Not while a change is out: its own landing
        // asks for one, and a listing read halfway through it is already stale.
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
        let query = self.query(window, cx);
        let catalog = match &self.catalog {
            None => return hint("Reading Claude Code's plugins…", cx),
            Some(Err(why)) => return self.failed(why.clone(), cx),
            Some(Ok(catalog)) => catalog,
        };
        let needle = query.read(cx).value().trim().to_lowercase();
        let muted = cx.theme().muted_foreground;

        let installed_cut = catalog.installed.len().saturating_sub(INSTALLED_CAP);
        let installed: Vec<AnyElement> = catalog
            .installed
            .iter()
            .take(INSTALLED_CAP)
            .enumerate()
            .map(|(i, plugin)| self.installed_row(i, plugin, cx))
            .collect();
        let matching: Vec<&Available> = catalog
            .available
            .iter()
            .filter(|plugin| plugin.matches(&needle))
            .collect();
        let market_cut = matching.len().saturating_sub(MARKET_CAP);
        let offered: Vec<AnyElement> = matching
            .iter()
            .take(MARKET_CAP)
            .enumerate()
            .map(|(i, plugin)| self.available_row(i, plugin, catalog, cx))
            .collect();

        let top = match &self.busy {
            Some(change) => change.doing(),
            // A session reads its plugins when its agent starts, so a change
            // here is not seen by one already running — said, because a switch
            // that visibly did nothing reads as broken.
            None => "Changes reach a session when its agent next starts".to_string(),
        };

        div()
            .id("plugins-body")
            .flex_1()
            .min_h_0()
            .v_flex()
            .overflow_y_scroll()
            .child(
                div()
                    .px_2()
                    .py_1()
                    .truncate()
                    .text_xs()
                    .text_color(muted)
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(top),
            )
            .child(heading("Installed", cx))
            .when(installed.is_empty(), |list| {
                list.child(note("Nothing installed reaches this project", cx))
            })
            .children(installed)
            .when(installed_cut > 0, |list| {
                list.child(note(format!("… {installed_cut} more not shown"), cx))
            })
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
            .when(market_cut > 0, |list| {
                list.child(note(
                    format!("… {market_cut} more not shown — narrow the search"),
                    cx,
                ))
            })
            .into_any_element()
    }

    /// A listing that failed: why, and the way to ask again — otherwise the
    /// only retry is leaving the mode and coming back, which nothing says.
    fn failed(&self, why: String, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex_1()
            .v_flex()
            .gap_2()
            .p_2()
            .child(status_line(why, cx))
            .child(
                div().px_2().child(
                    action("plugins-retry")
                        .xsmall()
                        .outline()
                        .label("Try again")
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                            view.catalog = None;
                            view.mark_stale(cx)
                        })),
                ),
            )
            .into_any_element()
    }

    /// Where an install lands, as one segmented control above the list rather
    /// than a menu on every row: the choice is usually made once for a run of
    /// installs, and sixty menus are sixty places to make it.
    fn scope_picker(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let current = self.install_scope;
        ButtonGroup::new("plugins-scope")
            .outline()
            .xsmall()
            .children(Scope::ALL.into_iter().enumerate().map(|(i, scope)| {
                action(("plugins-scope", i))
                    .label(scope.label())
                    .tooltip(format!("Install for: {}", scope.meaning()))
                    .selected(scope == current)
            }))
            .on_click(cx.listener(|view, clicked: &Vec<usize>, _, cx| {
                if let Some(scope) = clicked.first().and_then(|i| Scope::ALL.get(*i)) {
                    view.install_scope = *scope;
                    cx.notify();
                }
            }))
    }

    /// One installed plugin: its name, and a switch per scope.
    ///
    /// **Each scope's switch shows what is in force at that scope** — what it
    /// sets itself, else what it takes from a wider one — and pressing it
    /// writes the other answer *at that scope*. So turning a global plugin off
    /// for one project is the Project switch, and leaves every other project
    /// alone. A switch whose scope sets the answer itself is outlined; one
    /// taking it from a wider scope is not, which is the difference between
    /// "this project turns it off" and "it is off everywhere".
    fn installed_row(&self, i: usize, plugin: &Plugin, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let on_here = plugin.in_force(Scope::Local);
        let switches = Scope::ALL.into_iter().map(|scope| {
            let flip = plugin.flip(scope);
            let on = plugin.in_force(scope);
            let own = plugin.set_at(scope);
            // Only a wider scope can be where an answer came from; with none
            // setting it, the plugin is off because nothing turned it on.
            let inherited = Scope::ALL
                .iter()
                .any(|wider| *wider < scope && plugin.set_at(*wider));
            let said = match (on, own, inherited) {
                (true, true, _) => "on, set here",
                (false, true, _) => "off, set here",
                (true, false, _) => "on, from a wider scope",
                (false, false, true) => "off, from a wider scope",
                (false, false, false) => "off, set nowhere",
            };
            let press = if on { "off" } else { "on" };
            let button = action(("plugin-scope", i * Scope::ALL.len() + scope as usize))
                .xsmall()
                .label(scope.label())
                .selected(on)
                .loading(self.running(&flip))
                .disabled(self.busy.is_some())
                .tooltip(format!(
                    "{}: {said} — press to turn it {press}",
                    scope.meaning()
                ));
            let button: Button = if own {
                button.outline()
            } else {
                button.ghost()
            };
            button.on_click(
                cx.listener(move |view, _: &ClickEvent, _, cx| view.change(flip.clone(), cx)),
            )
        });
        let removes = plugin.installed.iter().map(|scope| {
            let remove = Change {
                id: plugin.id.clone(),
                scope: *scope,
                verb: Verb::Uninstall,
            };
            // Named by scope only where there is more than one to choose from.
            let label = if plugin.installed.len() > 1 {
                format!("Remove · {}", scope.label())
            } else {
                "Remove".to_string()
            };
            action(("plugin-remove", i * Scope::ALL.len() + *scope as usize))
                .xsmall()
                .ghost()
                .label(label)
                .loading(self.running(&remove))
                .disabled(self.busy.is_some())
                .tooltip(format!(
                    "Uninstall from {}; the plugin's saved data is kept",
                    scope.label()
                ))
                .on_click(
                    cx.listener(move |view, _: &ClickEvent, _, cx| view.change(remove.clone(), cx)),
                )
        });
        div()
            .v_flex()
            .w_full()
            .px_2()
            .py_1()
            .gap_1()
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            // Drawn quieter while a session started here would
                            // not get it, whichever scope decided that.
                            .when(!on_here, |name| name.text_color(muted))
                            .child(plugin.id.clone()),
                    )
                    .children(plugin.version.clone().map(|version| {
                        div().flex_none().text_xs().text_color(muted).child(version)
                    })),
            )
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .children(switches)
                    .child(div().flex_1())
                    .children(removes),
            )
            .into_any_element()
    }

    fn available_row(
        &self,
        i: usize,
        plugin: &Available,
        catalog: &Catalog,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let scope = self.install_scope;
        let muted = cx.theme().muted_foreground;
        let meta = match plugin.install_count {
            Some(count) => format!("{} · {count} installs", plugin.marketplace),
            None => plugin.marketplace.clone(),
        };
        let install = Change {
            id: plugin.id.clone(),
            scope,
            verb: Verb::Install,
        };
        let control =
            if catalog.has(&plugin.id, scope) {
                note("Installed", cx)
            } else {
                action(("plugin-install", i))
                    .xsmall()
                    .outline()
                    .label("Install")
                    .loading(self.running(&install))
                    .disabled(self.busy.is_some())
                    .tooltip(format!("Install for: {}", scope.meaning()))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.change(install.clone(), cx)
                    }))
                    .into_any_element()
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
                    .child(control),
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

/// One muted line inside the list — an empty section, a cut, a plugin that is
/// already installed where an install would land.
fn note(text: impl Into<gpui::SharedString>, cx: &App) -> AnyElement {
    div()
        .flex_none()
        .px_2()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
        .into_any_element()
}
