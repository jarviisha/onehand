//! The Plugins mode's own state — which project, what `claude` last said about
//! it, the change in flight — and how it is drawn.

use crate::cli::{self, Available, Catalog, Change, Plugin, Scope, Verb};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, ClipboardItem, Context, Entity, FocusHandle,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, ParentElement, Render,
    SharedString, StatefulInteractiveElement as _, Styled, Task, Window, div, px,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::dialog::{DialogClose, DialogFooter};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{PopupMenu, PopupMenuItem};
use gpui_component::switch::Switch;
use gpui_component::tooltip::Tooltip;
use gpui_component::{
    ActiveTheme, Disableable as _, Icon, IconName, Selectable as _, Sizable as _, StyledExt,
    WindowExt as _,
};
use onehand_plugin_host::{
    Ask, Request, action, hint, menu_below, menu_item, menu_row, status_ink, status_line, switch,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// How many installed plugins are drawn. Far past what anybody installs, so it
/// bounds the work of building rows rather than editing the list; the one time
/// it bites, it says so.
const INSTALLED_CAP: usize = 200;

/// How many of the marketplaces' plugins are drawn at once. The official
/// marketplace alone offers a few hundred, and a list nobody scrolls to the end
/// of is not worth building to the end — the search is how the rest are
/// reached, and the list says how many it left out.
const MARKET_CAP: usize = 60;

/// Which of the mode's two lists is showing.
///
/// Two lists and not one page, because they answer different questions — what
/// this project has, and what could be added — and stacked, the second pushed
/// the first's last rows out of reach beneath a few hundred it had nothing to
/// do with, and a search at the top of that half scrolled away with it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tab {
    Installed,
    Marketplace,
}

impl Tab {
    const ALL: [Tab; 2] = [Tab::Installed, Tab::Marketplace];

    fn label(self) -> &'static str {
        match self {
            Tab::Installed => "Installed",
            Tab::Marketplace => "Marketplace",
        }
    }
}

/// Which installed plugins are listed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Filter {
    All,
    Global,
    Project,
    Local,
    HasUpdate,
}

impl Filter {
    const ALL: [Filter; 5] = [
        Filter::All,
        Filter::Global,
        Filter::Project,
        Filter::Local,
        Filter::HasUpdate,
    ];

    fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Global => "Global",
            Filter::Project => "Project",
            Filter::Local => "Local",
            Filter::HasUpdate => "Has update",
        }
    }

    /// A scope filter keeps a plugin installed at that scope; it is where a
    /// plugin can be removed from, which is what somebody filtering by scope
    /// is usually about to do.
    fn keeps(self, plugin: &Plugin) -> bool {
        match self {
            Filter::All => true,
            Filter::Global => plugin.installed.contains(&Scope::User),
            Filter::Project => plugin.installed.contains(&Scope::Project),
            Filter::Local => plugin.installed.contains(&Scope::Local),
            Filter::HasUpdate => plugin.update.is_some(),
        }
    }
}

pub(crate) struct PluginsView {
    root: Option<PathBuf>,
    /// Which list is showing. Per window rather than per root, and not
    /// persisted: which list somebody was reading is not a fact about the
    /// project, and a mode that came back on the catalog would hide what is
    /// installed behind a click.
    tab: Tab,
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
    /// Where *Install* lands: the scope last picked from its caret, so the
    /// choice made once carries on to the next install.
    install_scope: Scope,
    /// The marketplace the catalog is narrowed to, or all of them.
    market: Option<String>,
    /// The marketplace search. Made on the first draw, since an input needs a
    /// window to be made in.
    query: Option<Entity<InputState>>,
    /// The installed list's search, made the same way.
    installed_query: Option<Entity<InputState>>,
    filter: Filter,
    /// Every change that went through, with the project it was made in and
    /// when. What the agent on screen has not seen is the ones after it
    /// started — which is what the pending banner counts.
    made: Vec<(PathBuf, Instant)>,
    /// When the agent on screen started, as the shell last said; `None` while
    /// no session shows, when there is nothing running to be out of date.
    since: Option<Instant>,
    /// One focus handle per installed row, by plugin, so the rows are tab
    /// stops and a row keeps its focus across a re-list.
    rows: HashMap<String, FocusHandle>,
    /// How the banner's *Restart agent* reaches the shell.
    ask: Ask,
}

impl PluginsView {
    pub(crate) fn new(ask: Ask, cx: &mut App) -> Entity<Self> {
        cx.new(|_| Self {
            root: None,
            tab: Tab::Installed,
            catalog: None,
            stale: false,
            _list: None,
            busy: None,
            _change: None,
            status: None,
            install_scope: Scope::User,
            market: None,
            query: None,
            installed_query: None,
            filter: Filter::All,
            made: Vec::new(),
            since: None,
            rows: HashMap::new(),
            ask,
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

    pub(crate) fn agent_started(&mut self, since: Option<Instant>, cx: &mut Context<Self>) {
        self.since = since;
        cx.notify();
    }

    /// How many changes the agent on screen has not loaded: those made in
    /// this project since it started. None while no agent is running, since a
    /// session started next will load all of them.
    ///
    /// Counts what went through this mode alone — a plugin installed from a
    /// terminal is not seen here, and is not claimed to be.
    fn pending(&self) -> usize {
        let (Some(root), Some(since)) = (&self.root, self.since) else {
            return 0;
        };
        self.made
            .iter()
            .filter(|(at_root, at)| at_root == root && *at > since)
            .count()
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
                match done {
                    Ok(()) => view.made.push((root.clone(), Instant::now())),
                    Err(why) if view.root.as_deref() == Some(root.as_path()) => {
                        view.status = Some(why)
                    }
                    Err(_) => {}
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

    fn installed_query(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(query) = &self.installed_query {
            return query.clone();
        }
        let query =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search installed plugins"));
        cx.subscribe(&query, |_, _, _: &InputEvent, cx| cx.notify())
            .detach();
        self.installed_query = Some(query.clone());
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
        let installed_query = self.installed_query(window, cx);
        // Focus handles first, while nothing else here is borrowed: a row is
        // a tab stop, and its handle has to outlive the frame it is drawn in.
        if let Some(Ok(catalog)) = &self.catalog {
            let ids: Vec<String> = catalog.installed.iter().map(|p| p.id.clone()).collect();
            self.rows.retain(|id, _| ids.contains(id));
            for id in ids {
                self.rows
                    .entry(id)
                    .or_insert_with(|| cx.focus_handle().tab_stop(true));
            }
        }
        let catalog = match &self.catalog {
            None => return hint("Reading Claude Code's plugins…", cx),
            Some(Err(why)) => return self.failed(why.clone(), cx),
            Some(Ok(catalog)) => catalog,
        };
        let labels: [SharedString; 2] = [
            format!("Installed {}", catalog.installed.len()).into(),
            format!("Marketplace {}", catalog.available.len()).into(),
        ];
        let tabs = switch(
            "plugins-tab",
            &labels,
            Tab::ALL
                .iter()
                .position(|tab| *tab == self.tab)
                .unwrap_or(0),
            gpui_component::Size::Small,
            cx.listener(|view: &mut Self, i: &usize, _, cx| {
                view.tab = Tab::ALL[*i];
                cx.notify();
            }),
            cx,
        );

        // Only the list on screen is built: the other is a click away and
        // costs nothing until it is shown.
        let (controls, rows): (AnyElement, Vec<AnyElement>) =
            match self.tab {
                Tab::Installed => {
                    let needle = installed_query.read(cx).value().trim().to_lowercase();
                    let shown: Vec<&Plugin> = catalog
                        .installed
                        .iter()
                        .filter(|plugin| self.filter.keeps(plugin))
                        .filter(|plugin| {
                            needle.is_empty()
                                || plugin.id.to_lowercase().contains(&needle)
                                || plugin
                                    .inventory
                                    .description
                                    .as_deref()
                                    .is_some_and(|d| d.to_lowercase().contains(&needle))
                        })
                        .take(INSTALLED_CAP)
                        .collect();
                    // On first, off after: what a session here gets is what is read
                    // first, and a plugin turned off is a record of a choice, kept
                    // below the ones in use rather than mixed in among them.
                    let (on, off): (Vec<&Plugin>, Vec<&Plugin>) = shown
                        .into_iter()
                        .partition(|plugin| plugin.in_force(Scope::Local));
                    let mut rows: Vec<AnyElement> = on
                        .iter()
                        .enumerate()
                        .map(|(i, plugin)| self.installed_row(i, plugin, window, cx))
                        .collect();
                    if !off.is_empty() {
                        rows.push(
                            div()
                                .px_2()
                                .pt_3()
                                .pb_1()
                                .text_xs()
                                .font_medium()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("Disabled · {}", off.len()))
                                .into_any_element(),
                        );
                        rows.extend(off.iter().enumerate().map(|(i, plugin)| {
                            self.installed_row(on.len() + i, plugin, window, cx)
                        }));
                    }
                    if rows.is_empty() {
                        rows.push(note(
                            if catalog.installed.is_empty() {
                                "Nothing installed reaches this project"
                            } else {
                                "No plugin matches"
                            },
                            cx,
                        ));
                    }
                    let cut = catalog.installed.len().saturating_sub(INSTALLED_CAP);
                    if cut > 0 {
                        rows.push(note(format!("… {cut} more not shown"), cx));
                    }
                    (self.installed_controls(&installed_query, catalog, cx), rows)
                }
                Tab::Marketplace => {
                    let needle = query.read(cx).value().trim().to_lowercase();
                    let matching: Vec<&Available> = catalog
                        .available
                        .iter()
                        .filter(|plugin| {
                            self.market
                                .as_ref()
                                .is_none_or(|market| &plugin.marketplace == market)
                        })
                        .filter(|plugin| plugin.matches(&needle))
                        .collect();
                    let cut = matching.len().saturating_sub(MARKET_CAP);
                    let mut rows: Vec<AnyElement> = matching
                        .iter()
                        .take(MARKET_CAP)
                        .enumerate()
                        .map(|(i, plugin)| self.available_row(i, plugin, catalog, cx))
                        .collect();
                    if rows.is_empty() {
                        rows.push(note("No plugin matches the search", cx));
                    }
                    if cut > 0 {
                        rows.push(note(
                            format!("… {cut} more not shown — narrow the search"),
                            cx,
                        ));
                    }
                    (self.market_controls(&query, catalog, cx), rows)
                }
            };

        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .gap_3()
            .p_3()
            .child(tabs)
            // What is happening, while it happens — only then, so an idle
            // panel carries no line above its list.
            .children(self.busy.as_ref().map(|change| note(change.doing(), cx)))
            .children(self.pending_banner(cx))
            .child(controls)
            .child(
                div()
                    // Keyed by the tab, so each list keeps a scroll of its own
                    // instead of the one showing inheriting the other's.
                    .id(self.tab.label())
                    .flex_1()
                    .min_h_0()
                    .v_flex()
                    .gap_1()
                    .overflow_y_scroll()
                    .children(rows),
            )
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

    /// The catalog's search, and a chip per marketplace to narrow it by —
    /// said once above the list rather than on every row under it. Offered
    /// only where there is more than one marketplace to choose between.
    fn market_controls(
        &self,
        query: &Entity<InputState>,
        catalog: &Catalog,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut markets: Vec<String> = catalog
            .available
            .iter()
            .map(|plugin| plugin.marketplace.clone())
            .collect();
        markets.sort();
        markets.dedup();
        let current = self.market.clone();
        let chips = (markets.len() > 1).then(|| {
            let all = action("plugins-market-all")
                .xsmall()
                .ghost()
                .label("All")
                .selected(current.is_none())
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                    view.market = None;
                    cx.notify();
                }));
            let each = markets.into_iter().enumerate().map(|(i, market)| {
                let on = current.as_deref() == Some(market.as_str());
                action(("plugins-market", i))
                    .xsmall()
                    .ghost()
                    .label(market.clone())
                    .selected(on)
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.market = Some(market.clone());
                        cx.notify();
                    }))
            });
            div().h_flex().flex_wrap().gap_1().child(all).children(each)
        });
        div()
            .v_flex()
            .gap_2()
            .child(Input::new(query).small())
            .children(chips)
            .into_any_element()
    }

    /// What the agent on screen has not loaded, and the way to load it —
    /// only while there is something, so an idle panel carries no line about
    /// restarting.
    fn pending_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pending = self.pending();
        if pending == 0 {
            return None;
        }
        let ask = self.ask.clone();
        let changes = if pending == 1 {
            "change applies"
        } else {
            "changes apply"
        };
        Some(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
                .text_sm()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(format!("{pending} {changes} when the agent restarts")),
                )
                .child(
                    action("plugins-restart")
                        .small()
                        .outline()
                        .label("Restart agent")
                        .disabled(self.busy.is_some())
                        .on_click(move |_, window, cx| ask(&Request::RestartAgent, window, cx)),
                )
                .into_any_element(),
        )
    }

    /// The installed list's search and its filter chips. *Has update* is
    /// offered only while a plugin has one: a filter that can only ever come
    /// back empty is a control that says nothing.
    fn installed_controls(
        &self,
        query: &Entity<InputState>,
        catalog: &Catalog,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let any_update = catalog.installed.iter().any(|p| p.update.is_some());
        let current = self.filter;
        let chips = Filter::ALL
            .into_iter()
            .enumerate()
            .filter(|(_, filter)| *filter != Filter::HasUpdate || any_update)
            .map(|(i, filter)| {
                action(("plugins-filter", i))
                    .xsmall()
                    .ghost()
                    .label(filter.label())
                    .selected(filter == current)
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.filter = filter;
                        cx.notify();
                    }))
            });
        div()
            .v_flex()
            .gap_2()
            .child(Input::new(query).small())
            .child(div().h_flex().flex_wrap().gap_1().children(chips))
            .into_any_element()
    }

    /// One installed plugin.
    ///
    /// Its name, then a quieter line saying where it came from and where it
    /// is installed, then what it carries — one chip per kind, *hooks* in the
    /// warning ink since a hook is code that runs on its own. The version is
    /// in the monospace face, a commit cut to the seven characters a person
    /// reads with the whole of it on hover.
    ///
    /// **The switch is "on in this project", and it writes to Local** — this
    /// project on this machine, the one file that reaches nobody else. The
    /// other two scopes each reach somebody else, so they are in the ••• menu,
    /// chosen on purpose rather than hit on the way past.
    ///
    /// The whole row opens the plugin's details and is a tab stop: Enter
    /// opens, Space flips the switch. The switch, the update chip and the menu
    /// keep their presses to themselves, so reaching for one never opens the
    /// details as well.
    fn installed_row(
        &self,
        i: usize,
        plugin: &Plugin,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let (muted, radius, ring, hover) = (
            theme.muted_foreground,
            theme.radius,
            theme.ring,
            theme.accent.opacity(0.5),
        );
        let on_here = plugin.in_force(Scope::Local);
        let busy = self.busy.is_some();
        let here = plugin.flip(Scope::Local);

        let switch = Switch::new(("plugin-on", i))
            .checked(on_here)
            .disabled(busy)
            .tooltip(if on_here {
                "On in this project — turn it off on this machine only"
            } else {
                "Off in this project — turn it on on this machine only"
            })
            .on_click({
                let here = here.clone();
                cx.listener(move |view, _: &bool, _, cx| view.change(here.clone(), cx))
            });

        let trigger = action(SharedString::from(format!(
            "plugin-menu-button-{}",
            plugin.id
        )))
        .ghost()
        .xsmall()
        .icon(Icon::new(IconName::Ellipsis))
        .disabled(busy)
        .tooltip("Where it is on, and removing it");
        let menu = self.row_menu(plugin, trigger, cx);

        // The version: monospace, a commit cut to seven characters with what
        // is known of it on hover.
        let version = plugin.version.clone().map(|version| {
            let shown = if cli::is_hash(&version) {
                version[..7].to_string()
            } else {
                version.clone()
            };
            let cut = shown != version;
            div()
                .id(("plugin-version", i))
                .flex_none()
                .text_xs()
                .font_family(cx.theme().mono_font_family.clone())
                .text_color(muted)
                .child(shown)
                .when(cut, |v| {
                    v.tooltip(move |window, cx| Tooltip::new(version.clone()).build(window, cx))
                })
        });
        let update = plugin.update.clone().map(|to| {
            let change = Change {
                id: plugin.id.clone(),
                scope: plugin.installed[0],
                verb: Verb::Update,
            };
            action(("plugin-update", i))
                .xsmall()
                .outline()
                .label(format!("Update {to}"))
                .loading(self.running(&change))
                .disabled(busy)
                .on_click(
                    cx.listener(move |view, _: &ClickEvent, _, cx| view.change(change.clone(), cx)),
                )
        });

        let scopes = plugin
            .installed
            .iter()
            .map(|scope| scope.label())
            .collect::<Vec<_>>()
            .join(", ");
        let origin = div()
            .h_flex()
            .items_center()
            .gap_1p5()
            .text_xs()
            .text_color(muted)
            .child(format!("{} · {scopes}", plugin.marketplace()))
            .when(plugin.official(), |line| {
                line.child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_0p5()
                        .child(Icon::new(IconName::CircleCheck).xsmall())
                        .child("Official"),
                )
            })
            .when(plugin.decided_elsewhere(), |line| {
                line.child(
                    div()
                        .text_color(status_ink(cx).warning)
                        .child("· set elsewhere"),
                )
            });

        let chips = component_chips(plugin, cx);
        let handle = self.rows.get(&plugin.id).cloned();
        let open = plugin.clone();
        let key_plugin = plugin.clone();
        let key_flip = here;

        div()
            .id(SharedString::from(format!("plugin-row-{}", plugin.id)))
            .when_some(handle, |row, handle| row.track_focus(&handle))
            .v_flex()
            .gap_1()
            .w_full()
            .px_2()
            .py_2()
            .rounded(radius)
            .border_1()
            .border_color(gpui::transparent_black())
            .cursor_pointer()
            .hover(|row| row.bg(hover))
            .focus(|row| row.border_color(ring))
            .on_click(move |_, window, cx| open_details(&open, window, cx))
            .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "enter" => open_details(&key_plugin, window, cx),
                    "space" => view.change(key_flip.clone(), cx),
                    _ => return,
                }
                cx.stop_propagation();
            }))
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
                            .when(!on_here, |name| name.text_color(muted))
                            .child(plugin.name().to_string()),
                    )
                    .children(version)
                    .child(
                        // The controls keep their presses: without this, the
                        // press that flips the switch also opens the details.
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .children(update)
                            .child(
                                div()
                                    .flex_none()
                                    .when(!busy, |d| d.cursor_pointer())
                                    .child(switch),
                            )
                            .child(menu),
                    ),
            )
            .child(origin)
            .children(chips)
            .into_any_element()
    }

    /// The ••• menu on an installed row, in five groups — where it lives,
    /// its version, where it applies, its files, and removing it — each
    /// separated from the next, with anything that does not apply to this
    /// plugin left out rather than drawn refusing.
    fn row_menu(
        &self,
        plugin: &Plugin,
        trigger: gpui_component::button::Button,
        cx: &mut Context<Self>,
    ) -> gpui_component::popover::Popover {
        // Named by what it acts on, so a menu held open across a re-list is
        // one for this plugin and never for whichever row took its place.
        let menu_id = SharedString::from(format!("plugin-menu-{}", plugin.id));
        let plugin = plugin.clone();
        let view = cx.entity();
        let ask = self.ask.clone();
        menu_below(menu_id, trigger, move |menu, window, cx| {
            let danger = status_ink(cx).danger;
            let mut groups: Vec<Vec<PopupMenuItem>> = Vec::new();

            // Where it lives.
            let mut about = Vec::new();
            if let Some(url) = plugin.inventory.repository.clone() {
                about.push(
                    menu_item("Open repository")
                        .on_click(move |_, _, cx: &mut App| cx.open_url(&url)),
                );
            }
            if let Some(path) = plugin.inventory.changelog.clone() {
                let ask = ask.clone();
                about.push(menu_item("View changelog").on_click(
                    move |_, window: &mut Window, cx: &mut App| {
                        ask(&Request::OpenFile(&path), window, cx)
                    },
                ));
            }
            groups.push(about);

            // Its version. *Update to* only where a newer one is known;
            // *Check for updates* always, since fetching the catalog is how
            // one becomes known.
            let scope = plugin.installed[0];
            let mut version = Vec::new();
            if let Some(to) = &plugin.update {
                let to = if cli::is_hash(to) || to.starts_with('v') {
                    to.clone()
                } else {
                    format!("v{to}")
                };
                let update = Change {
                    id: plugin.id.clone(),
                    scope,
                    verb: Verb::Update,
                };
                version.push(menu_item(format!("Update to {to}")).on_click(act(&view, update)));
            }
            let check = Change {
                id: plugin.id.clone(),
                scope,
                verb: Verb::CheckUpdates,
            };
            version.push(menu_item("Check for updates").on_click(act(&view, check)));
            groups.push(version);

            // Its files.
            let mut files = Vec::new();
            if let Some(path) = plugin.install_path.clone() {
                files.push(
                    menu_item("Open install folder")
                        .on_click(move |_, _, cx: &mut App| cx.open_with_system(&path)),
                );
            }
            let id = plugin.id.clone();
            files.push(
                menu_item("Copy plugin ID").on_click(move |_, _, cx: &mut App| {
                    cx.write_to_clipboard(ClipboardItem::new_string(id.clone()));
                }),
            );

            // Removing it: last, in the danger ink, and asked about first.
            let removes: Vec<PopupMenuItem> = plugin
                .installed
                .iter()
                .map(|scope| {
                    let label = if plugin.installed.len() > 1 {
                        format!("Uninstall from {}…", scope.label())
                    } else {
                        "Uninstall…".to_string()
                    };
                    let (view, plugin, scope) = (view.clone(), plugin.clone(), *scope);
                    menu_row(move |_, _| div().text_color(danger).child(label.clone())).on_click(
                        move |_, window: &mut Window, cx: &mut App| {
                            confirm_uninstall(&view, &plugin, scope, window, cx)
                        },
                    )
                })
                .collect();

            let mut menu = menu;
            let mut first = true;
            let mut divide = |menu: PopupMenu| {
                let menu = if first { menu } else { menu.separator() };
                first = false;
                menu
            };
            for group in groups.into_iter().filter(|group| !group.is_empty()) {
                menu = group.into_iter().fold(divide(menu), PopupMenu::item);
            }

            // Where it applies, as two submenus. *Change scope* only for a
            // plugin installed at one scope: installed at two, which of them
            // moves is a question the menu cannot ask.
            menu = divide(menu);
            if plugin.installed.len() == 1 {
                let (plugin, view, from) = (plugin.clone(), view.clone(), plugin.installed[0]);
                menu = menu.submenu("Change scope", window, cx, move |menu, _, _| {
                    Scope::ALL.into_iter().fold(menu, |menu, to| {
                        let item = menu_item(to.reach()).checked(to == from);
                        menu.item(if to == from {
                            item
                        } else {
                            item.on_click(act(
                                &view,
                                Change {
                                    id: plugin.id.clone(),
                                    scope: to,
                                    verb: Verb::Move(from),
                                },
                            ))
                        })
                    })
                });
            }
            let (on_for, view_on) = (plugin.clone(), view.clone());
            menu = menu.submenu("Turn on for", window, cx, move |menu, _, _| {
                Scope::ALL
                    .into_iter()
                    .filter(|scope| on_for.reaches(*scope))
                    .fold(menu, |menu, scope| {
                        let from = match on_for.source(scope) {
                            Some(source) if source != scope => {
                                format!(" · from {}", source.label())
                            }
                            _ => String::new(),
                        };
                        menu.item(
                            menu_item(format!("{}{from}", scope.reach()))
                                .checked(on_for.in_force(scope))
                                .on_click(act(&view_on, on_for.flip(scope))),
                        )
                    })
            });

            menu = files.into_iter().fold(divide(menu), PopupMenu::item);
            removes.into_iter().fold(divide(menu), PopupMenu::item)
        })
    }

    /// One plugin the marketplaces offer.
    ///
    /// **Its control follows what is already true of it here**: *Installed*
    /// where it is installed and on, *Enable* where it is installed and off —
    /// which turns it on on this machine, as the installed row's switch does —
    /// and *Install ▾* otherwise. The last is split: the press installs at the
    /// scope used last, the caret picks another and installs there, so the
    /// choice is made at the moment of installing rather than by a control
    /// above the list that has to be remembered.
    fn available_row(
        &self,
        i: usize,
        plugin: &Available,
        catalog: &Catalog,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let busy = self.busy.is_some();
        let installed = catalog.installed.iter().find(|p| p.id == plugin.id);
        let control = match installed {
            Some(have) if have.in_force(Scope::Local) => note("Installed", cx),
            Some(have) => {
                let enable = have.flip(Scope::Local);
                action(("plugin-enable", i))
                    .xsmall()
                    .outline()
                    .label("Enable")
                    .tooltip("Installed but off here — turn it on on this machine only")
                    .loading(self.running(&enable))
                    .disabled(busy)
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.change(enable.clone(), cx)
                    }))
                    .into_any_element()
            }
            None => self.install_split(i, plugin, cx),
        };

        // Official and the count, and nothing else: which marketplace it is
        // from is the chip above the list.
        let meta: Vec<AnyElement> = [
            plugin.official().then(|| {
                div()
                    .h_flex()
                    .items_center()
                    .gap_0p5()
                    .child(Icon::new(IconName::CircleCheck).xsmall())
                    .child("Official")
                    .into_any_element()
            }),
            plugin.install_count.map(|count| {
                div()
                    .child(format!("{} installs", compact(count)))
                    .into_any_element()
            }),
        ]
        .into_iter()
        .flatten()
        .collect();
        let meta = (!meta.is_empty()).then(|| {
            let mut line = div()
                .h_flex()
                .items_center()
                .gap_1p5()
                .text_xs()
                .text_color(muted);
            for (n, part) in meta.into_iter().enumerate() {
                if n > 0 {
                    line = line.child("·");
                }
                line = line.child(part);
            }
            line
        });

        div()
            .v_flex()
            .w_full()
            .min_w_0()
            .px_2()
            .py_2()
            .gap_1()
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
                    .child(control),
            )
            // Two lines, then an ellipsis. Clipped as well as clamped: a
            // description can hold a word with nowhere to break — a URL — and
            // unclipped that ran past the panel's edge.
            .when(!plugin.description.is_empty(), |row| {
                row.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .overflow_hidden()
                        .text_xs()
                        .text_color(muted)
                        .line_clamp(2)
                        .text_ellipsis()
                        .child(plugin.description.clone()),
                )
            })
            .children(meta)
            .into_any_element()
    }

    /// *Install ▾*: the press installs at the scope used last, the caret
    /// installs at another and makes it the one used next.
    fn install_split(&self, i: usize, plugin: &Available, cx: &mut Context<Self>) -> AnyElement {
        let scope = self.install_scope;
        let busy = self.busy.is_some();
        let install = Change {
            id: plugin.id.clone(),
            scope,
            verb: Verb::Install,
        };
        let running = Scope::ALL.into_iter().any(|at| {
            self.running(&Change {
                id: plugin.id.clone(),
                scope: at,
                verb: Verb::Install,
            })
        });
        let main = action(("plugin-install", i))
            .xsmall()
            .outline()
            .label("Install")
            .tooltip(format!("Install for: {}", scope.meaning()))
            .loading(running)
            .disabled(busy)
            .on_click(
                cx.listener(move |view, _: &ClickEvent, _, cx| view.change(install.clone(), cx)),
            );
        let caret = action(SharedString::from(format!(
            "plugin-install-scope-{}",
            plugin.id
        )))
        .xsmall()
        .outline()
        .icon(Icon::new(IconName::ChevronDown))
        .disabled(busy)
        .tooltip("Install for…");
        let (id, view) = (plugin.id.clone(), cx.entity());
        let menu = menu_below(
            SharedString::from(format!("plugin-install-menu-{}", plugin.id)),
            caret,
            move |menu, _, _| {
                Scope::ALL
                    .into_iter()
                    .fold(menu.label("Install for"), |menu, at| {
                        let (id, view) = (id.clone(), view.clone());
                        menu.item(menu_item(at.reach()).checked(at == scope).on_click(
                            move |_, _, cx: &mut App| {
                                view.update(cx, |view, cx| {
                                    view.install_scope = at;
                                    view.change(
                                        Change {
                                            id: id.clone(),
                                            scope: at,
                                            verb: Verb::Install,
                                        },
                                        cx,
                                    )
                                })
                            },
                        ))
                    })
            },
        );
        div()
            .h_flex()
            .flex_none()
            .gap_px()
            .child(main)
            .child(menu)
            .into_any_element()
    }
}

/// A menu row's press that makes `change`.
fn act(
    view: &Entity<PluginsView>,
    change: Change,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let view = view.clone();
    move |_, _, cx| view.update(cx, |view, cx| view.change(change.clone(), cx))
}

/// Ask before uninstalling, naming who loses it and what goes with it.
///
/// A dialog rather than an armed second press, for the reason the app's other
/// removal asks this way: an armed control looks like one that did nothing.
/// The plugin's saved data is kept (`--keep-data`), and it says so.
fn confirm_uninstall(
    view: &Entity<PluginsView>,
    plugin: &Plugin,
    scope: Scope,
    window: &mut Window,
    cx: &mut App,
) {
    let inventory = &plugin.inventory;
    let parts: Vec<String> = [
        (inventory.skills.len(), "skill", "skills"),
        (inventory.commands.len(), "command", "commands"),
        (inventory.agents.len(), "agent", "agents"),
        (inventory.mcp.len(), "MCP server", "MCP servers"),
        (inventory.hooks.len(), "hook", "hooks"),
    ]
    .into_iter()
    .filter(|(n, _, _)| *n > 0)
    .map(|(n, one, many)| format!("{n} {}", if n == 1 { one } else { many }))
    .collect();
    let losing = if parts.is_empty() {
        String::new()
    } else {
        format!(" {} will no longer be available.", parts.join(", "))
    };
    let description = format!(
        "{} is removed from {} ({}).{losing} Its saved data is kept.",
        plugin.name(),
        scope.label(),
        scope.reach().to_lowercase(),
    );
    let title = format!("Uninstall {}?", plugin.name());
    let change = Change {
        id: plugin.id.clone(),
        scope,
        verb: Verb::Uninstall,
    };
    let view = view.clone();
    window.open_alert_dialog(cx, move |alert, _, _| {
        // Cloned per build: a dialog's builder runs again on every frame it is
        // on screen, so nothing captured here can be consumed by one.
        let (view, change) = (view.clone(), change.clone());
        alert
            .title(title.clone())
            .description(description.clone())
            .footer(
                DialogFooter::new()
                    .child(DialogClose::new().child(action("plugin-keep").ghost().label("Cancel")))
                    .child(
                        action("plugin-confirm-uninstall")
                            .danger()
                            .label("Uninstall")
                            .on_click(move |_, window: &mut Window, cx: &mut App| {
                                window.close_dialog(cx);
                                view.update(cx, |view, cx| view.change(change.clone(), cx));
                            }),
                    ),
            )
    });
}

/// What a plugin carries, one chip per kind it has any of — none for a kind
/// it has none of, and no row at all for a plugin whose folder said nothing.
/// *hooks* is in the warning ink and carries no count: a hook is code that
/// runs on its own, and that there is any is the fact worth reading.
fn component_chips(plugin: &Plugin, cx: &App) -> Option<AnyElement> {
    let inventory = &plugin.inventory;
    let counted = [
        (inventory.skills.len(), "skill", "skills"),
        (inventory.commands.len(), "command", "commands"),
        (inventory.agents.len(), "agent", "agents"),
        (inventory.mcp.len(), "MCP", "MCP"),
    ];
    let mut chips: Vec<AnyElement> = counted
        .into_iter()
        .filter(|(n, _, _)| *n > 0)
        .map(|(n, one, many)| {
            chip(
                format!("{n} {}", if n == 1 { one } else { many }),
                false,
                cx,
            )
        })
        .collect();
    if !inventory.hooks.is_empty() {
        chips.push(chip("hooks".to_string(), true, cx));
    }
    (!chips.is_empty()).then(|| {
        div()
            .h_flex()
            .flex_wrap()
            .gap_1()
            .pt_0p5()
            .children(chips)
            .into_any_element()
    })
}

fn chip(text: String, warn: bool, cx: &App) -> AnyElement {
    let ink = if warn {
        status_ink(cx).warning
    } else {
        cx.theme().muted_foreground
    };
    div()
        .px_1p5()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(if warn { ink } else { cx.theme().border })
        .text_xs()
        .text_color(ink)
        .child(text)
        .into_any_element()
}

/// A plugin's details, in a drawer over the window: what it says it is, and
/// every skill, command, agent, MCP server and hook it carries — a hook with
/// the command it runs, which is what somebody opens this to check.
fn open_details(plugin: &Plugin, window: &mut Window, cx: &mut App) {
    let plugin = plugin.clone();
    window.open_sheet(cx, move |sheet, _, cx| {
        sheet
            .title(plugin.name().to_string())
            .size(px(420.))
            .child(details(&plugin, cx))
    });
}

fn details(plugin: &Plugin, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let mono = theme.mono_font_family.clone();
    let inventory = &plugin.inventory;
    let section = |title: &str, names: &[String]| -> Option<AnyElement> {
        (!names.is_empty()).then(|| {
            div()
                .v_flex()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_medium()
                        .text_color(muted)
                        .child(format!("{title} · {}", names.len())),
                )
                .children(names.iter().map(|name| div().text_sm().child(name.clone())))
                .into_any_element()
        })
    };
    let hooks = (!inventory.hooks.is_empty()).then(|| {
        div()
            .v_flex()
            .gap_1p5()
            .child(
                div()
                    .text_xs()
                    .font_medium()
                    .text_color(status_ink(cx).warning)
                    .child(format!(
                        "Hooks · {} — run on their own",
                        inventory.hooks.len()
                    )),
            )
            .children(inventory.hooks.iter().map(|hook| {
                let when = match &hook.matcher {
                    Some(matcher) => format!("{} · {matcher}", hook.event),
                    None => hook.event.clone(),
                };
                div()
                    .v_flex()
                    .gap_0p5()
                    .child(div().text_xs().text_color(muted).child(when))
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded(theme.radius)
                            .bg(theme.secondary)
                            .text_xs()
                            .font_family(mono.clone())
                            .child(hook.command.clone()),
                    )
            }))
            .into_any_element()
    });
    let origin = format!(
        "{} · {}",
        plugin.marketplace(),
        plugin
            .installed
            .iter()
            .map(|scope| scope.label())
            .collect::<Vec<_>>()
            .join(", ")
    );
    div()
        .id("plugin-details")
        .size_full()
        .v_flex()
        .gap_4()
        .overflow_y_scroll()
        .children(
            inventory
                .description
                .clone()
                .map(|text| div().text_sm().child(text)),
        )
        .child(
            div()
                .text_xs()
                .text_color(muted)
                .child(match &plugin.version {
                    Some(version) => format!("{origin} · {version}"),
                    None => origin,
                }),
        )
        .children(section("Skills", &inventory.skills))
        .children(section("Commands", &inventory.commands))
        .children(section("Agents", &inventory.agents))
        .children(section("MCP servers", &inventory.mcp))
        .children(hooks)
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

/// A count as a person reads one at a glance: `3327` as `3.3k`. The exact
/// number says nothing more about which plugin to pick, and four digits on
/// every row are four digits of noise.
fn compact(count: u64) -> String {
    match count {
        0..1_000 => count.to_string(),
        1_000..1_000_000 => format!("{:.1}k", count as f64 / 1_000.0),
        _ => format!("{:.1}M", count as f64 / 1_000_000.0),
    }
    .replace(".0", "")
}

#[cfg(test)]
mod tests {
    use super::compact;

    #[test]
    fn a_count_reads_at_a_glance() {
        assert_eq!(compact(7), "7");
        assert_eq!(compact(3327), "3.3k");
        assert_eq!(compact(12_000), "12k");
        assert_eq!(compact(2_450_000), "2.5M");
    }
}
